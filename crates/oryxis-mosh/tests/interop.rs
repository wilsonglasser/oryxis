//! The crate against a real `mosh-server`.
//!
//! Ignored by default because it needs one running. The bootstrap half
//! is pure and tested without a server; this is the half that can only
//! be answered by the thing it interoperates with.
//!
//! ```text
//! mosh-server new -i 127.0.0.1 -p 60123 -- /bin/bash
//! MOSH_RS_TEST_PORT=60123 MOSH_RS_TEST_KEY=<the 22-char key it printed> \
//!     cargo test -p oryxis-mosh --test interop -- --ignored --nocapture
//! ```
//!
//! One server per test: `mosh-server` serves a single session, so a
//! second test against the same one fails.

use std::time::{Duration, Instant};

use oryxis_mosh::{AlacrittyScreen, MoshSession};

fn endpoint() -> (u16, String) {
    let port = std::env::var("MOSH_RS_TEST_PORT").expect("MOSH_RS_TEST_PORT");
    let key = std::env::var("MOSH_RS_TEST_KEY").expect("MOSH_RS_TEST_KEY");
    (port.parse().expect("a port"), key)
}

/// Feed what the session sends into a terminal, and wait for `want` to
/// be ON THE SCREEN.
///
/// Not a search of the byte stream, which is the trap this protocol
/// sets: the client sends only what CHANGED, so a word whose spaces
/// were already spaces arrives as two runs with a cursor move between
/// them and no amount of grepping finds it. What is being asserted is
/// what a terminal would be SHOWING, so a terminal is what looks.
async fn until(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    screen: &mut AlacrittyScreen,
    want: &str,
    limit: Duration,
) -> bool {
    use mosh_rs::Screen as _;
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(frame)) => screen.feed(&frame),
            Ok(None) | Err(_) => return false,
        }
        if screen.text().contains(want) {
            return true;
        }
    }
    false
}

/// A shell, a command, and its output back: the whole point.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a live mosh-server; see the module docs"]
async fn a_shell_answers_through_the_session() {
    let (port, key) = endpoint();
    let (session, mut rx) =
        MoshSession::connect("127.0.0.1", port, &key, 80, 24, false).expect("open the session");

    let mut seen = AlacrittyScreen::new(24, 80, false);
    assert!(
        until(&mut rx, &mut seen, "$", Duration::from_secs(10)).await,
        "no prompt arrived: {:?}",
        { use mosh_rs::Screen as _; seen.text() }
    );

    session.write(b"echo ORYXIS-MOSH-OK\r").expect("send");
    assert!(
        until(&mut rx, &mut seen, "ORYXIS-MOSH-OK", Duration::from_secs(10)).await,
        "the command never came back: {:?}",
        { use mosh_rs::Screen as _; seen.text() }
    );
    assert!(session.is_alive());
}

/// What the pane is handed has to be ESCAPES, not a grid, because
/// everything downstream of it reads the byte stream: the highlight
/// rule triggers, the OSC 7 working directory, the prompt marks.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a live mosh-server; see the module docs"]
async fn what_arrives_is_a_byte_stream_a_terminal_can_eat() {
    use mosh_rs::Screen as _;
    let (port, key) = endpoint();
    let (_session, mut rx) =
        MoshSession::connect("127.0.0.1", port, &key, 80, 24, false).expect("open the session");

    // Here the RAW bytes are the claim, so here they are kept.
    let mut raw = Vec::new();
    let mut screen = AlacrittyScreen::new(24, 80, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !screen.text().contains('$') {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(frame)) => {
                raw.extend_from_slice(&frame);
                screen.feed(&frame);
            }
            Ok(None) | Err(_) => break,
        }
    }
    assert!(screen.text().contains('$'), "no prompt arrived");
    assert!(
        raw.contains(&0x1b),
        "a frame with no escape in it is not a terminal stream"
    );
}

/// A resize has to reach the shell, or the server paints for a window
/// that is not there and every cursor position lands wrong.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a live mosh-server; see the module docs"]
async fn a_resize_reaches_the_shell() {
    let (port, key) = endpoint();
    let (session, mut rx) =
        MoshSession::connect("127.0.0.1", port, &key, 80, 24, false).expect("open the session");

    let mut seen = AlacrittyScreen::new(24, 80, false);
    assert!(until(&mut rx, &mut seen, "$", Duration::from_secs(10)).await, "no prompt");

    session.resize(100, 30);
    session.write(b"stty size\r").expect("send");
    assert!(
        until(&mut rx, &mut seen, "30 100", Duration::from_secs(10)).await,
        "the shell never saw 30x100: {:?}",
        { use mosh_rs::Screen as _; seen.text() }
    );
}

/// Closing says goodbye rather than vanishing. A server whose client
/// disappears holds the shell open until it times out, and a user who
/// closed a tab does not expect to find it still running.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a live mosh-server; see the module docs"]
async fn closing_ends_the_session_rather_than_abandoning_it() {
    let (port, key) = endpoint();
    let (session, mut rx) =
        MoshSession::connect("127.0.0.1", port, &key, 80, 24, false).expect("open the session");

    let mut seen = AlacrittyScreen::new(24, 80, false);
    assert!(until(&mut rx, &mut seen, "$", Duration::from_secs(10)).await, "no prompt");

    session.close();
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.is_alive() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!session.is_alive(), "the session never finished shutting down");
}
