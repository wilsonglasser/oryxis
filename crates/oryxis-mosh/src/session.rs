//! A mosh session, shaped like every other transport a pane can hold.
//!
//! The pane takes bytes. That is not a simplification: `Backend::process`
//! filters `screen`'s window titles out of the stream, scans it for the
//! highlight rules that carry an action, and sniffs OSC 7 / 133 / 9 for
//! the working directory, the prompt marks and desktop notifications.
//! A transport that handed over a grid instead would silently lose all
//! of it. So this drives `mosh_rs::MoshSession` and publishes what it
//! says the terminal is missing, which is bytes, on the same channel
//! shape Telnet and Serial use.
//!
//! The screen those bytes are computed against is alacritty, the SAME
//! emulator the pane draws with, so there is one implementation and one
//! opinion about the screen rather than two. See [`crate::screen`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use mosh_rs::{Base64Key, LinkHealth, MoshSession as Protocol};
use tokio::sync::mpsc;

/// The screen the protocol keeps its states on. See the module note.
type Screen = crate::screen::AlacrittyScreen;

/// How long the driving task sleeps when the session has nothing due.
///
/// Not a poll interval for INPUT: what the user types arrives on a
/// channel that wakes the task at once. This only bounds how long an
/// idle session goes between looking at its own clock, which is what
/// keeps the link-health figures moving on a link that has gone quiet.
const IDLE_TICK: Duration = Duration::from_millis(100);

/// Nothing heard from the server for this long and the link is worth
/// mentioning.
///
/// mosh's own client thresholds (`terminaloverlay.cc`), which is the
/// point: a user who knows mosh should find Oryxis saying the same thing
/// at the same moment, and the numbers are tuned against a protocol that
/// heartbeats several times a second.
const SERVER_LATE_MS: u64 = 6_500;

/// Nothing WE sent acknowledged for this long, same.
///
/// Longer than [`SERVER_LATE_MS`] because an acknowledgement only comes
/// back when there was something to acknowledge; a link nobody is typing
/// on takes longer to prove itself broken in this direction.
const REPLY_LATE_MS: u64 = 10_000;

/// What a mosh link's silence means, once it has gone on long enough to
/// be worth saying.
///
/// Two clocks, because the link fails in two different ways and naming
/// the wrong one sends someone debugging in the wrong direction: nothing
/// arriving at all is [`LinkState::NoContact`], while things arriving
/// with nothing we send acknowledged is [`LinkState::NoReply`], which is
/// what a one-way path looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// In touch. mosh says nothing at all here, and neither do we: the
    /// reading is an answer to a question the user is already asking,
    /// not a status display.
    Healthy,
    /// Nothing has arrived at all.
    NoContact {
        /// How long the server has been silent, in milliseconds.
        ms: u64,
    },
    /// Things arrive, but nothing we sent is being acknowledged: the
    /// path back is the broken one.
    NoReply {
        /// How long our own state has gone unacknowledged, in
        /// milliseconds.
        ms: u64,
    },
}

/// Read a link's health as one of the three things worth reporting.
///
/// `NoContact` wins a tie because it is the broader failure: a link
/// nothing arrives on is not acknowledging us either, and reporting the
/// narrower "no reply" there would describe a one-way path that is
/// actually a dead one.
#[must_use]
pub fn classify(health: LinkHealth) -> LinkState {
    let no_contact = health.since_heard_ms > SERVER_LATE_MS;
    let no_reply = health.since_ack_ms > REPLY_LATE_MS;
    if no_contact {
        LinkState::NoContact { ms: health.since_heard_ms }
    } else if no_reply {
        LinkState::NoReply { ms: health.since_ack_ms }
    } else {
        LinkState::Healthy
    }
}

/// Why a session could not be started or could not go on.
#[derive(Debug, thiserror::Error)]
pub enum MoshError {
    /// The key the host announced is not a key.
    #[error("the session key from the host is malformed")]
    BadKey,
    /// The UDP socket could not be opened or the address is unusable.
    #[error("could not open a session to {0}: {1}")]
    Unreachable(String, String),
}

/// A live mosh session.
///
/// The same surface a `TelnetSession` offers, so the pane that holds it
/// does not have to know which one it has.
#[derive(Debug)]
pub struct MoshSession {
    writer_tx: mpsc::UnboundedSender<Vec<u8>>,
    resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    alive: Arc<AtomicBool>,
    /// Set when the user or the app wants the session to end, so the
    /// driving task says goodbye to the server rather than vanishing
    /// and leaving it to time out with a shell still open.
    closing: Arc<AtomicBool>,
    /// The link's two clocks, republished by the driving task every
    /// cycle.
    ///
    /// Copies rather than a reference to the protocol, because the
    /// protocol is OWNED by that task and the interface has to read this
    /// from `view()`. Atomics rather than a lock for the same reason: a
    /// render pass that could block behind the network is a render pass
    /// that stutters when the network is exactly what has gone wrong.
    /// The task loops at least every [`IDLE_TICK`], so these are never
    /// more than that stale.
    since_heard_ms: Arc<AtomicU64>,
    since_ack_ms: Arc<AtomicU64>,
}

impl MoshSession {
    /// Open a session against a server that has already announced
    /// itself, and start driving it.
    ///
    /// `host` is where the SSH connection went, not what the server
    /// reported: the server binds the address the SSH session arrived
    /// on (`mosh-server -s`), which is the address already known to be
    /// reachable from here.
    ///
    /// That pairing holds for a direct dial and NOT through a jump
    /// chain, where SSH arrives from the last hop and the caller dials
    /// from somewhere else. It is not repairable at this layer, or any
    /// other: mosh is UDP, and UDP does not travel down an SSH tunnel.
    ///
    /// `ambiguous_width_wide` is the host's own setting, and it has to be
    /// the same one the pane was given: the screen here decides where the
    /// server's output lands, the pane draws the diff taken from it.
    pub fn connect(
        host: &str,
        port: u16,
        key: &str,
        cols: u16,
        rows: u16,
        ambiguous_width_wide: bool,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Vec<u8>>), MoshError> {
        let key = Base64Key::from_printable(key).map_err(|_| MoshError::BadKey)?;
        // The screen is supplied rather than asked for: `connect_with_size`
        // only exists for the built-in one, which is not in the build.
        let protocol = Protocol::connect_with_screen(
            host,
            port,
            &key,
            Screen::new(rows, cols, ambiguous_width_wide),
        )
        .map_err(|e| MoshError::Unreachable(format!("{host}:{port}"), e.to_string()))?;

        let (output_tx, output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (writer_tx, writer_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();
        let alive = Arc::new(AtomicBool::new(true));
        let closing = Arc::new(AtomicBool::new(false));
        let since_heard_ms = Arc::new(AtomicU64::new(0));
        let since_ack_ms = Arc::new(AtomicU64::new(0));

        tokio::spawn(drive(
            protocol,
            output_tx,
            writer_rx,
            resize_rx,
            Arc::clone(&alive),
            Arc::clone(&closing),
            Health {
                since_heard_ms: Arc::clone(&since_heard_ms),
                since_ack_ms: Arc::clone(&since_ack_ms),
            },
        ));

        Ok((
            Self { writer_tx, resize_tx, alive, closing, since_heard_ms, since_ack_ms },
            output_rx,
        ))
    }

    /// How long the link has been quiet, in each direction.
    ///
    /// Cheap enough to call from a render pass, which is where it is
    /// called from.
    #[must_use]
    pub fn link_health(&self) -> LinkHealth {
        LinkHealth {
            since_heard_ms: self.since_heard_ms.load(Ordering::SeqCst),
            since_ack_ms: self.since_ack_ms.load(Ordering::SeqCst),
        }
    }

    /// [`classify`] of this session's current health.
    #[must_use]
    pub fn link_state(&self) -> LinkState {
        classify(self.link_health())
    }

    /// Send what the user typed.
    pub fn write(&self, data: &[u8]) -> Result<(), MoshError> {
        let _ = self.writer_tx.send(data.to_vec());
        Ok(())
    }

    /// Tell the far end the window changed shape.
    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.resize_tx.send((cols, rows));
    }

    /// The channel a caller can hand to something that produces input
    /// of its own, the way the other transports expose theirs.
    pub fn write_sender(&self) -> mpsc::UnboundedSender<Vec<u8>> {
        self.writer_tx.clone()
    }

    /// The resize channel, same reason.
    pub fn resize_sender(&self) -> mpsc::UnboundedSender<(u16, u16)> {
        self.resize_tx.clone()
    }

    /// Whether the session is still running.
    ///
    /// A mosh session is alive across a network that is not: losing the
    /// path does NOT end it, which is the whole point of the protocol,
    /// so this stays true through a change of address and only goes
    /// false once the far end has actually gone.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// End the session, letting the server hear about it.
    ///
    /// Asks rather than aborts: a mosh server whose client vanishes
    /// holds the shell open until it times out, and a user who closed a
    /// tab does not expect to find it still there.
    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
    }
}

/// The two clocks the driving task republishes for readers outside it.
struct Health {
    since_heard_ms: Arc<AtomicU64>,
    since_ack_ms: Arc<AtomicU64>,
}

/// The task that owns the protocol session.
///
/// Everything the session needs to be told arrives on a channel, and
/// everything it produces goes out on one, so the only thing that ever
/// touches `Protocol` is this task. That is what lets the session be
/// held behind an `Arc` by a pane without a lock.
async fn drive(
    mut protocol: Protocol<Screen>,
    output_tx: mpsc::UnboundedSender<Vec<u8>>,
    mut writer_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut resize_rx: mpsc::UnboundedReceiver<(u16, u16)>,
    alive: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    health: Health,
) {
    let mut said_goodbye = false;
    loop {
        if closing.load(Ordering::SeqCst) && !said_goodbye {
            protocol.shutdown();
            said_goodbye = true;
        }

        // How long there is before the protocol needs to send. Waiting
        // exactly that long is what keeps a keystroke from sitting out
        // a fixed poll interval, which measured as the difference
        // between this being as fast as the C++ client and being seven
        // times slower.
        let wait = Duration::from_millis(protocol.wait_time_ms()).min(IDLE_TICK);

        tokio::select! {
            // Biased so input is taken before the timer: a keystroke
            // that arrived while the timer was expiring goes out in
            // THIS cycle rather than the next one.
            biased;
            Some(bytes) = writer_rx.recv() => protocol.send_input(&bytes),
            Some((cols, rows)) = resize_rx.recv() => {
                protocol.send_resize(i32::from(cols), i32::from(rows));
            }
            () = tokio::time::sleep(wait) => {}
        }

        if let Err(error) = protocol.pump_ready() {
            tracing::warn!(%error, "mosh session ended");
            break;
        }

        // Published every cycle rather than on change, because the
        // interesting value is the one that grows while NOTHING happens:
        // a link that has gone quiet produces no event to hang an update
        // on, and that silence is precisely what is worth reporting.
        let reading = protocol.link_health();
        health.since_heard_ms.store(reading.since_heard_ms, Ordering::SeqCst);
        health.since_ack_ms.store(reading.since_ack_ms, Ordering::SeqCst);

        // What the terminal is missing, and nothing more. Empty on most
        // passes, which is what makes a retransmitted frame free.
        let frame = protocol.render();
        if !frame.is_empty() && output_tx.send(frame).is_err() {
            // Nobody is drawing this any more.
            break;
        }

        if protocol.finished() {
            break;
        }
    }
    // Dead BEFORE silent, and in that order on purpose. The app takes
    // the end of this stream as the disconnect notice and asks
    // `is_alive()` before acting on it, discarding a notice whose pane
    // still holds a live transport as one from a session it already
    // replaced. So a session that really ended must never still answer
    // "alive" here. Same task, no await between, which is what makes it
    // an ordering rather than a race; the SSH, Telnet and Serial readers
    // uphold the same contract through their own `reader_done` flag.
    // The drop is explicit so the pair reads as one statement rather
    // than as a store followed by an accident of scope.
    alive.store(false, Ordering::SeqCst);
    drop(output_tx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_that_is_not_a_key_is_refused_before_a_socket_is_opened() {
        let opened = MoshSession::connect("127.0.0.1", 1, "not a key", 80, 24, false);
        assert!(matches!(opened, Err(MoshError::BadKey)));
    }

    fn health(heard: u64, ack: u64) -> LinkHealth {
        LinkHealth { since_heard_ms: heard, since_ack_ms: ack }
    }

    #[test]
    fn a_link_in_touch_reports_nothing() {
        assert_eq!(classify(health(0, 0)), LinkState::Healthy);
        assert_eq!(classify(health(6_500, 10_000)), LinkState::Healthy);
    }

    /// The boundaries belong to the healthy side, so a link sitting
    /// exactly on one never reads worse than it is.
    #[test]
    fn each_clock_crosses_at_its_own_threshold() {
        assert_eq!(classify(health(6_501, 0)), LinkState::NoContact { ms: 6_501 });
        assert_eq!(classify(health(0, 10_001)), LinkState::NoReply { ms: 10_001 });
    }

    /// The whole reason there are two clocks: a link nothing arrives on
    /// is not acknowledging us either, so reporting "no reply" there
    /// would describe a one-way path when the path is dead in both
    /// directions, and send someone debugging the wrong half.
    #[test]
    fn silence_in_both_directions_reads_as_no_contact() {
        assert_eq!(
            classify(health(30_000, 30_000)),
            LinkState::NoContact { ms: 30_000 }
        );
    }

    /// Things arriving while nothing we sent is acknowledged IS the
    /// narrow case, and it has to survive the branch above.
    #[test]
    fn a_one_way_path_reads_as_no_reply() {
        assert_eq!(classify(health(200, 15_000)), LinkState::NoReply { ms: 15_000 });
    }

    #[tokio::test]
    async fn a_session_with_nowhere_to_go_still_opens_and_stays_alive() {
        // Pointed at a port nothing answers on. mosh is a protocol for
        // links that are not working yet, so opening has to succeed and
        // the session has to stay up: a client that gave up here would
        // give up on a laptop that had not joined the wifi.
        let (session, _rx) = MoshSession::connect(
            "127.0.0.1",
            1,
            "AAAAAAAAAAAAAAAAAAAAAA",
            80,
            24,
            false,
        )
        .expect("a socket is all it takes to start");
        assert!(session.is_alive());
        session.write(b"x").expect("input is queued, not delivered");
        session.resize(100, 30);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(session.is_alive(), "silence is not the end of a mosh session");
    }

    /// The clock has to advance with no server, no packets and nobody
    /// typing, because that is the only condition under which anyone
    /// looks at it. A reading that only moved when something arrived
    /// would read healthy forever on a link that had stopped.
    #[tokio::test]
    async fn the_quiet_clock_runs_with_nothing_arriving() {
        let (session, _rx) =
            MoshSession::connect("127.0.0.1", 1, "AAAAAAAAAAAAAAAAAAAAAA", 80, 24, false)
                .expect("a socket is all it takes to start");
        // Two idle cycles' worth, so the reading is one the driving task
        // published rather than the zero it was born with.
        tokio::time::sleep(IDLE_TICK * 3).await;
        let first = session.link_health().since_heard_ms;
        assert!(first > 0, "the clock never started: {first} ms");
        tokio::time::sleep(IDLE_TICK * 3).await;
        assert!(
            session.link_health().since_heard_ms > first,
            "the clock stopped at {first} ms",
        );
    }
}
