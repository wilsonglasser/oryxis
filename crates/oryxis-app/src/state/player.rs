//! Session-recording player (issue #71): replays a recorded session's
//! chunks through the same alacritty backend the live terminal uses,
//! read-only by construction (no PTY, no input wiring). The state is a
//! playback clock over a preprocessed event timeline; the view renders
//! the backend with the regular terminal widget pinned to the
//! recording's geometry.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use oryxis_terminal::widget::TerminalState;
use oryxis_terminal::TerminalPalette;
use uuid::Uuid;

/// Replay step for rows recorded before the timing migration
/// (`offset_ms = NULL`). Mirrors the `.cast` export's fallback in
/// `dispatch_history.rs` so both replays pace legacy logs identically.
const LEGACY_DELTA_MS: i64 = 50;

/// Geometry for recordings that carry no resize row (legacy logs).
/// Same fallback the `.cast` export header uses.
const FALLBACK_GEOMETRY: (u16, u16) = (80, 24);

/// Playback speed steps the speed button cycles through.
pub(crate) const PLAYER_SPEEDS: [f32; 5] = [0.5, 1.0, 1.5, 2.0, 4.0];

/// One playable event on an absolute, non-decreasing timeline.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlayerEvent {
    /// Milliseconds since the start of the recording, clamped
    /// non-decreasing (same interleaving guard as the `.cast` export).
    pub at_ms: i64,
    pub kind: PlayerEventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlayerEventKind {
    /// Raw output bytes for the emulator.
    Output(Vec<u8>),
    /// Terminal resize to (cols, rows).
    Resize(u16, u16),
}

/// Convert the vault's timed rows into the player timeline: absolute
/// non-decreasing times, typed-command rows dropped (replay is
/// output-only, like the `.cast` export), malformed resize rows
/// skipped, legacy untimed rows paced with a fixed delta. Returns the
/// events plus the recording's duration and initial geometry.
pub(crate) fn preprocess_events(
    rows: &[oryxis_vault::SessionLogEvent],
) -> (Vec<PlayerEvent>, i64, (u16, u16)) {
    let mut events: Vec<PlayerEvent> = Vec::with_capacity(rows.len());
    let mut last_ms: i64 = 0;
    for row in rows {
        if row.kind == 'c' {
            continue;
        }
        let kind = if row.kind == 'r' {
            let s = String::from_utf8_lossy(&row.data);
            let parsed = s.split_once('x').and_then(|(c, r)| {
                Some((c.parse::<u16>().ok()?, r.parse::<u16>().ok()?))
            });
            match parsed {
                // The emulator rejects grids under 2x2; dropping the
                // row keeps the timeline usable instead of wedging the
                // canvas at a degenerate size.
                Some((c, r)) if c >= 2 && r >= 2 => PlayerEventKind::Resize(c, r),
                _ => continue,
            }
        } else {
            if row.data.is_empty() {
                continue;
            }
            PlayerEventKind::Output(row.data.clone())
        };
        let at_ms = match row.offset_ms {
            // Clamp against interleavings: a resize stamped at flush
            // time can sit a hair before chunk rows written in the
            // same batch (same rule as the `.cast` export).
            Some(ms) => ms.max(last_ms),
            None => last_ms + LEGACY_DELTA_MS,
        };
        last_ms = at_ms;
        events.push(PlayerEvent { at_ms, kind });
    }
    let duration_ms = events.last().map(|e| e.at_ms).unwrap_or(0);
    let geometry = events
        .iter()
        .find_map(|e| match e.kind {
            PlayerEventKind::Resize(c, r) => Some((c, r)),
            _ => None,
        })
        .unwrap_or(FALLBACK_GEOMETRY);
    (events, duration_ms, geometry)
}

/// Scrollback budget for the static transcript viewer. The whole
/// recording is fed at once (no clock), so the history must hold every
/// line the session scrolled past, not just the user's live
/// `scrollback_rows`. alacritty grows the history lazily, so this
/// ceiling only costs what the recording actually fills; a session that
/// scrolled past it loses its oldest lines (a documented edge, far above
/// any readable transcript length).
const TRANSCRIPT_SCROLLBACK: usize = 100_000;

/// How a recording is fed into the transcript viewer's emulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TranscriptMode {
    /// Replay the stream faithfully: the emulator ends up in whatever
    /// state the session left it, which is what a transcript should be
    /// for an ordinary shell session.
    #[default]
    Rendered,
    /// Feed the stream through the linear renderer first
    /// (`ansi_render`), which has no viewport: absolute cursor
    /// addressing degrades to appended lines. A full-screen app
    /// (tmux, vim, htop) repaints one screen forever, so the faithful
    /// replay of a session that lived inside one has nothing to scroll
    /// through; the linear dump has every repaint, in order.
    Linear,
}

impl TranscriptMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Rendered => Self::Linear,
            Self::Linear => Self::Rendered,
        }
    }

    /// i18n key describing THIS mode (the button offers the other one).
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Rendered => "transcript_mode_rendered",
            Self::Linear => "transcript_mode_linear",
        }
    }
}

/// Share of the recording's wall time spent on the alternate screen,
/// found by scanning the recorded bytes rather than by emulating them.
///
/// A session run entirely inside tmux is ~1.0 and is the case the
/// faithful replay cannot show, so the viewer opens such a recording in
/// [`TranscriptMode::Linear`]. Chunk boundaries can split an escape
/// sequence, so the scan carries the tail of each chunk into the next.
pub(crate) fn alt_screen_share(rows: &[oryxis_vault::SessionLogEvent]) -> f32 {
    // Every private mode that swaps in an alternate screen. Scanned as
    // BYTES, not text: a recording carries whatever the host printed, so
    // slicing a lossy `String` by byte offsets would panic the moment a
    // tmux status bar drew a box-drawing glyph.
    const ENTER: [&[u8]; 3] = [b"[?1049h", b"[?1047h", b"[?47h"];
    const LEAVE: [&[u8]; 3] = [b"[?1049l", b"[?1047l", b"[?47l"];
    // Longest sequence above minus one: what a chunk split could hide.
    const CARRY: usize = 6;

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    let mut carry: Vec<u8> = Vec::new();
    let mut in_alt = false;
    let mut alt_ms: i64 = 0;
    let mut last_ms: i64 = 0;
    let mut total_ms: i64 = 0;
    for row in rows.iter().filter(|r| r.kind == 'o') {
        let at_ms = row.offset_ms.unwrap_or(last_ms);
        if in_alt {
            alt_ms += (at_ms - last_ms).max(0);
        }
        last_ms = at_ms;
        total_ms = total_ms.max(at_ms);
        let mut buf = std::mem::take(&mut carry);
        buf.extend_from_slice(&row.data);
        // Walk the chunk in order so the LAST switch in it wins.
        let mut idx = 0;
        while idx < buf.len() {
            let rest = &buf[idx..];
            let hit = ENTER
                .iter()
                .map(|p| (*p, true))
                .chain(LEAVE.iter().map(|p| (*p, false)))
                .filter_map(|(p, enter)| find(rest, p).map(|at| (at, p.len(), enter)))
                .min_by_key(|(at, _, _)| *at);
            match hit {
                Some((at, len, enter)) => {
                    in_alt = enter;
                    idx += at + len;
                }
                None => break,
            }
        }
        // Keep only the tail, so a sequence split across the boundary is
        // whole on the next pass. Re-seeing the last switch is harmless:
        // it is the most recent one, so replaying it keeps the order.
        let tail = buf.len().saturating_sub(CARRY);
        carry = buf.split_off(tail);
    }
    if total_ms <= 0 {
        // No timing (legacy rows): fall back to "did it end in alt".
        return if in_alt { 1.0 } else { 0.0 };
    }
    (alt_ms as f32 / total_ms as f32).clamp(0.0, 1.0)
}

/// Above this share of the recording spent on the alternate screen, the
/// viewer opens in [`TranscriptMode::Linear`]: the faithful replay would
/// show the login banner and one final screen, and the reporter of #92
/// read that as "the session was not recorded at all".
pub(crate) const LINEAR_ALT_SHARE: f32 = 0.5;

/// The static transcript viewer (issues #90/#91). The whole recording is
/// rendered into a read-only terminal backend, so selection,
/// copy-on-select and the right-click schemes behave exactly like the
/// live terminal, with cell-exact highlight hit-testing. The old
/// `text::Rich` path had none of that: it only copied via Ctrl+Shift+C,
/// ignored the right-click schemes, and its selection highlight drifted
/// vertically from the text in a long session (a fork-side layout quirk
/// the terminal widget's own metrics sidestep). Fed once with no clock;
/// the widget's own scrollback scrolls it. Mutually exclusive with the
/// [`SessionPlayer`] surface (opening either closes the other).
pub(crate) struct SessionLogViewer {
    /// The recording being shown (index resolution for the header
    /// actions, and to drop the viewer when its log is deleted).
    pub log_id: Uuid,
    /// The PTY-less emulator holding the whole recording, shared with the
    /// terminal widget the same way live panes are.
    pub terminal: Arc<Mutex<TerminalState>>,
    /// How this viewer was fed, so the header can offer the other mode
    /// and the rebuild knows what it is switching from.
    pub mode: TranscriptMode,
}

impl SessionLogViewer {
    /// Build a viewer over a recording's rows: feed every output and
    /// resize event (typed-command rows dropped, malformed resizes
    /// skipped) into a PTY-less backend at the recorded geometry, in
    /// capture order and with no timing. Fails only if the emulator
    /// can't be constructed.
    pub fn build(
        log_id: Uuid,
        rows: &[oryxis_vault::SessionLogEvent],
        palette: TerminalPalette,
        mode: TranscriptMode,
    ) -> oryxis_terminal::widget::TerminalResult<Self> {
        let (events, _duration, geometry) = preprocess_events(rows);
        let mut state = TerminalState::new_no_pty_with_scrollback(
            geometry.0,
            geometry.1,
            TRANSCRIPT_SCROLLBACK,
        )?;
        state.palette = palette;
        if mode == TranscriptMode::Linear {
            // No viewport, so no alternate screen and no repaint: the
            // linear renderer turns absolute cursor addressing into
            // appended lines, which is the only way a session that spent
            // its life inside tmux has anything to scroll through. Resize
            // rows are deliberately not applied (a mid-recording resize
            // would reflow a dump that has no live edge to reflow toward);
            // the recorded geometry sets the width once.
            let raw: Vec<u8> = events
                .iter()
                .filter_map(|ev| match &ev.kind {
                    PlayerEventKind::Output(bytes) => Some(bytes.as_slice()),
                    PlayerEventKind::Resize(..) => None,
                })
                .collect::<Vec<_>>()
                .concat();
            let spans = crate::ansi_render::render(&raw, &state.palette);
            state.process(&crate::ansi_render::to_ansi_bytes(&spans));
            state.pending_scroll.set(Some(i32::MAX));
            return Ok(Self {
                log_id,
                terminal: Arc::new(Mutex::new(state)),
                mode,
            });
        }
        for ev in &events {
            match &ev.kind {
                PlayerEventKind::Output(bytes) => state.process(bytes),
                PlayerEventKind::Resize(c, r) => {
                    state.resize(*c, *r);
                }
            }
        }
        // A recording that ends inside an alternate-screen app (top /
        // less / vim / htop / tmux still open at disconnect) leaves the
        // emulator on the alt screen, which carries no scrollback and
        // pins the view to the live edge. The transcript would then open
        // at the last frame with no scrollbar and dead PageUp/wheel, the
        // whole session hidden in the primary buffer's scrollback behind
        // it (#91, Mazwak's report). A static log is read top to bottom,
        // so capture the app's final frame, leave the alt screen, and
        // append the frame to the primary buffer under a dim separator:
        // nothing is lost and the whole session scrolls. The `.cast`
        // player stays the faithful timed replay of the app itself.
        if state.is_alt_screen() {
            let frame = state.screen_as_ansi();
            // Exit every alt-screen variant the recording might have
            // entered (47 / 1047 / 1049); alacritty no-ops the exits for
            // the ones that were not active. The cursor lands where the
            // app was launched, so the frame reads in session order.
            state.process(b"\x1b[?1049l\x1b[?1047l\x1b[?47l");
            let label = crate::i18n::t("session_final_screen");
            state.process(format!("\r\n\x1b[0;2m── {label} ──\x1b[0m\r\n").as_bytes());
            state.process(&frame);
            state.process(b"\x1b[0m\r\n");
        }
        // A recording is read like a log, top to bottom, so open at the
        // very start of the session rather than the terminal-native live
        // edge. `i32::MAX` is clamped to the real top by the draw pass, so
        // it lands exactly there even after the widget reflows the buffer
        // to the panel width on first layout.
        state.pending_scroll.set(Some(i32::MAX));
        Ok(Self {
            log_id,
            mode,
            terminal: Arc::new(Mutex::new(state)),
        })
    }
}

/// GIF export machinery for session recordings (issue #71). A sibling
/// of [`SessionPlayer`] rather than a field on it: an export is
/// triggered from the History list without the player open, and it
/// must survive the player closing while the `gif` plugin renders, so
/// it cannot live inside `Oryxis.session_player` (an `Option` that
/// may be `None` the whole time). One field on `Oryxis`
/// (`self.gif_export`).
#[derive(Default)]
pub(crate) struct GifExportState {
    /// One GIF render at a time: re-entry shows the "rendering" toast
    /// instead of racing two renders over the save dialog.
    pub running: bool,
}

/// The open player: one recording, one read-only terminal backend, a
/// scaled playback clock. Lives in `Oryxis.session_player` while the
/// player surface is up on the History screen.
pub(crate) struct SessionPlayer {
    /// The recording being played (used to close the player when its
    /// log is deleted underneath it).
    pub log_id: Uuid,
    /// Connection label of the recording, for the header.
    pub label: String,
    /// Preprocessed timeline (see [`preprocess_events`]).
    pub events: Vec<PlayerEvent>,
    /// Index of the first event not yet fed to the backend.
    pub next_event: usize,
    /// Playback position in milliseconds. `f64` so sub-tick speed
    /// scaling accumulates without drift.
    pub clock_ms: f64,
    /// Timeline length in milliseconds (last event's time).
    pub duration_ms: i64,
    pub playing: bool,
    /// While the user drags the scrubber, the pending target in
    /// milliseconds. The knob and time label follow it live (O(1)), but
    /// the emulator is only rebuilt/replayed once, on release
    /// ([`commit_scrub`]). Without this a backward drag rebuilt and
    /// replayed the whole timeline on every per-millisecond slider
    /// event, freezing the UI on a long recording.
    pub scrub: Option<f64>,
    /// Clock multiplier, one of [`PLAYER_SPEEDS`].
    pub speed: f32,
    /// Wall-clock instant of the previous tick while playing; `None`
    /// while paused so resuming can't count the paused gap.
    pub last_tick: Option<Instant>,
    /// The replay emulator, PTY-less and never wired for input.
    /// `Arc<Mutex<..>>` because the terminal widget shares state with
    /// the app the same way the live panes do.
    pub terminal: Arc<Mutex<TerminalState>>,
    /// Current grid geometry (tracks fed resize events), used by the
    /// view to size the fixed-grid canvas.
    pub cols: u16,
    pub rows: u16,
    /// The largest grid the recording ever reached (max cols, max rows
    /// across every resize event). The replay font is fitted against
    /// THIS, once, and held constant for the whole playback: a session
    /// that was resized mid-recording would otherwise refit per frame
    /// and the text would visibly jump size at each resize (and land at
    /// whatever the final geometry dictated). Fitting the biggest frame
    /// guarantees none overflow, and smaller frames just center with
    /// margins at the same, stable font.
    pub fit_cols: u16,
    pub fit_rows: u16,
    /// Geometry to rebuild with on a backward seek / restart.
    initial_geometry: (u16, u16),
    /// Palette applied to (re)built backends, resolved once at open
    /// like the live pane (per-host override, then global).
    palette: TerminalPalette,
}

impl SessionPlayer {
    /// Build a player over a preprocessed timeline. Fails only if the
    /// emulator can't be constructed.
    pub fn new(
        log_id: Uuid,
        label: String,
        events: Vec<PlayerEvent>,
        duration_ms: i64,
        geometry: (u16, u16),
        palette: TerminalPalette,
    ) -> oryxis_terminal::widget::TerminalResult<Self> {
        let terminal = Self::build_terminal(geometry, &palette)?;
        // Fit geometry: the largest grid any frame reaches, so the
        // replay font is chosen once for the biggest frame and never
        // rescales mid-playback. Seeded with the initial geometry so a
        // recording with no resize row still has a valid fit target.
        let (fit_cols, fit_rows) = events.iter().fold(geometry, |(mc, mr), e| {
            match e.kind {
                PlayerEventKind::Resize(c, r) => (mc.max(c), mr.max(r)),
                PlayerEventKind::Output(_) => (mc, mr),
            }
        });
        Ok(Self {
            log_id,
            label,
            events,
            next_event: 0,
            clock_ms: 0.0,
            duration_ms,
            playing: true,
            scrub: None,
            speed: 1.0,
            last_tick: None,
            terminal,
            cols: geometry.0,
            rows: geometry.1,
            fit_cols,
            fit_rows,
            initial_geometry: geometry,
            palette,
        })
    }

    fn build_terminal(
        geometry: (u16, u16),
        palette: &TerminalPalette,
    ) -> oryxis_terminal::widget::TerminalResult<Arc<Mutex<TerminalState>>> {
        let mut state = TerminalState::new_no_pty(geometry.0, geometry.1)?;
        state.palette = palette.clone();
        Ok(Arc::new(Mutex::new(state)))
    }

    /// Advance the clock by `dt_ms` of wall time (pre-clamped by the
    /// tick handler) scaled by the current speed, feed the events that
    /// became due, and pause at the end of the timeline.
    pub fn advance(&mut self, dt_ms: f64) {
        self.clock_ms =
            (self.clock_ms + dt_ms * f64::from(self.speed)).min(self.duration_ms as f64);
        self.feed_due();
        if self.finished() {
            self.playing = false;
            self.last_tick = None;
        }
    }

    /// Whether the whole timeline has been fed.
    pub fn finished(&self) -> bool {
        self.next_event >= self.events.len()
    }

    /// Feed every event at or before the current clock into the
    /// backend, in order.
    pub fn feed_due(&mut self) {
        let due = self.clock_ms.floor() as i64;
        let mut state = self
            .terminal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while let Some(ev) = self.events.get(self.next_event) {
            if ev.at_ms > due {
                break;
            }
            match &ev.kind {
                PlayerEventKind::Output(bytes) => state.process(bytes),
                PlayerEventKind::Resize(c, r) => {
                    state.resize(*c, *r);
                    self.cols = *c;
                    self.rows = *r;
                }
            }
            self.next_event += 1;
        }
    }

    /// Jump to `target_ms`. Forward seeks feed incrementally; backward
    /// seeks rebuild the emulator and replay from zero up to the
    /// target (`process()` is fast enough that keyframes aren't
    /// needed; see the issue #71 spec). A failed rebuild leaves the
    /// current frame in place rather than blanking the player.
    pub fn seek(&mut self, target_ms: f64) {
        // A committed seek supersedes any in-flight scrub preview.
        self.scrub = None;
        let target = target_ms.clamp(0.0, self.duration_ms as f64);
        if target < self.clock_ms {
            let Ok(fresh) = Self::build_terminal(self.initial_geometry, &self.palette) else {
                return;
            };
            self.terminal = fresh;
            self.next_event = 0;
            self.cols = self.initial_geometry.0;
            self.rows = self.initial_geometry.1;
        }
        self.clock_ms = target;
        self.feed_due();
        // Seeking away from the end revives the play button's meaning;
        // seeking onto the end pauses like natural completion.
        if self.finished() {
            self.playing = false;
            self.last_tick = None;
        }
    }

    /// The position the transport should display: the live scrub target
    /// while dragging, otherwise the playback clock.
    pub fn display_ms(&self) -> f64 {
        self.scrub.unwrap_or(self.clock_ms)
    }

    /// Record a scrubber drag without touching the emulator (cheap): the
    /// knob and label follow, the frame catches up on release.
    pub fn scrub_to(&mut self, target_ms: f64) {
        self.scrub = Some(target_ms.clamp(0.0, self.duration_ms as f64));
    }

    /// Apply the pending scrub target once, on release (a single
    /// rebuild/replay instead of one per drag event).
    pub fn commit_scrub(&mut self) {
        if let Some(target) = self.scrub.take() {
            self.seek(target);
        }
    }

    /// Restart from zero, playing.
    pub fn restart(&mut self) {
        self.scrub = None;
        self.seek(0.0);
        self.playing = true;
        self.last_tick = Some(Instant::now());
    }

    /// Toggle play/pause. Playing again after the timeline ended
    /// restarts from zero (the expected media-player affordance).
    pub fn toggle_play(&mut self) {
        if self.playing {
            self.playing = false;
            self.last_tick = None;
        } else if self.finished() {
            self.restart();
        } else {
            self.playing = true;
            self.last_tick = Some(Instant::now());
        }
    }

    /// Step to the next speed in [`PLAYER_SPEEDS`], wrapping.
    pub fn cycle_speed(&mut self) {
        let idx = PLAYER_SPEEDS
            .iter()
            .position(|s| (*s - self.speed).abs() < f32::EPSILON)
            .unwrap_or(1);
        self.speed = PLAYER_SPEEDS[(idx + 1) % PLAYER_SPEEDS.len()];
    }
}

/// A saved AI conversation open for reading.
///
/// Deliberately read-only, and deliberately NOT resumable: the terminal
/// the conversation was held against is gone, its captured context is
/// stale, and the commands it ran no longer describe the host. Re-reading
/// is the useful half ("how did I fix that?"), the same way a recording is
/// re-watched rather than re-entered.
pub(crate) struct ChatViewer {
    /// The conversation being shown, so a delete can close the reader.
    pub conversation_id: Uuid,
    /// Row label at save time.
    pub label: String,
    /// Turns, already decrypted, in the order they happened.
    pub messages: Vec<oryxis_vault::ChatMessageEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use oryxis_vault::SessionLogEvent;

    fn ev(offset_ms: Option<i64>, kind: char, data: &[u8]) -> SessionLogEvent {
        SessionLogEvent { offset_ms, kind, data: data.to_vec() }
    }

    #[test]
    fn preprocess_builds_a_non_decreasing_timeline() {
        let (events, duration, _) = preprocess_events(&[
            ev(Some(0), 'r', b"120x30"),
            ev(Some(100), 'o', b"hi"),
            // Stamped earlier than the previous event: clamps forward.
            ev(Some(40), 'o', b"there"),
        ]);
        let times: Vec<i64> = events.iter().map(|e| e.at_ms).collect();
        assert_eq!(times, vec![0, 100, 100]);
        assert_eq!(duration, 100);
    }

    #[test]
    fn preprocess_paces_untimed_rows_with_the_legacy_delta() {
        let (events, duration, geometry) = preprocess_events(&[
            ev(None, 'o', b"one"),
            ev(None, 'o', b"two"),
        ]);
        let times: Vec<i64> = events.iter().map(|e| e.at_ms).collect();
        assert_eq!(times, vec![LEGACY_DELTA_MS, LEGACY_DELTA_MS * 2]);
        assert_eq!(duration, LEGACY_DELTA_MS * 2);
        // No resize row anywhere: same 80x24 fallback as the export.
        assert_eq!(geometry, FALLBACK_GEOMETRY);
    }

    #[test]
    fn preprocess_drops_command_rows_and_malformed_resizes() {
        let (events, _, geometry) = preprocess_events(&[
            ev(Some(0), 'c', b"ls -la"),
            ev(Some(10), 'r', b"garbage"),
            ev(Some(20), 'r', b"1x1"),
            ev(Some(30), 'r', b"100x40"),
            ev(Some(40), 'o', b"total 0"),
        ]);
        assert_eq!(events.len(), 2, "only the valid resize and the output stay");
        assert_eq!(geometry, (100, 40));
        assert!(events.iter().all(|e| match &e.kind {
            PlayerEventKind::Output(d) => d == b"total 0",
            PlayerEventKind::Resize(c, r) => (*c, *r) == (100, 40),
        }));
    }

    fn player_over(rows: &[SessionLogEvent]) -> SessionPlayer {
        let (events, duration, geometry) = preprocess_events(rows);
        SessionPlayer::new(
            Uuid::nil(),
            "test".into(),
            events,
            duration,
            geometry,
            TerminalPalette::default(),
        )
        .expect("headless player")
    }

    fn cell(p: &SessionPlayer, row: i32, col: usize) -> char {
        use oryxis_terminal::alacritty_terminal::index::{Column, Line};
        let state = p.terminal.lock().unwrap();
        state.backend.term.grid()[Line(row)][Column(col)].c
    }

    #[test]
    fn advance_feeds_due_events_and_pauses_at_the_end() {
        let mut p = player_over(&[
            ev(Some(0), 'o', b"A"),
            ev(Some(1_000), 'o', b"B"),
        ]);
        p.feed_due();
        assert_eq!(cell(&p, 0, 0), 'A');
        assert_eq!(cell(&p, 0, 1), ' ', "future event must not be fed yet");

        // 300 ms of wall time at 2x = 600 ms of playback: still short.
        p.speed = 2.0;
        p.advance(300.0);
        assert_eq!(cell(&p, 0, 1), ' ');
        assert!(p.playing);

        // Another 300 ms lands on 1200 ms: the second event plays and
        // the clock clamps to the duration, pausing playback.
        p.advance(300.0);
        assert_eq!(cell(&p, 0, 1), 'B');
        assert!(!p.playing, "reaching the end pauses");
        assert_eq!(p.clock_ms, 1_000.0, "clock clamps to the duration");
    }

    #[test]
    fn fit_geometry_is_the_per_axis_max_across_resizes() {
        // A recording that grows then shrinks: the fit target is the
        // largest cols AND the largest rows seen, taken independently, so
        // the replay font is chosen for the biggest frame and never
        // rescales mid-playback. Here no single frame is 120x50, but the
        // fit must still be 120x50 so neither the widest nor the tallest
        // frame overflows.
        let p = player_over(&[
            ev(Some(0), 'r', b"120x30"),
            ev(Some(10), 'o', b"a"),
            ev(Some(20), 'r', b"80x50"),
            ev(Some(30), 'o', b"b"),
            ev(Some(40), 'r', b"100x40"),
        ]);
        assert_eq!((p.fit_cols, p.fit_rows), (120, 50));
    }

    #[test]
    fn fit_geometry_falls_back_to_the_initial_when_unresized() {
        // No resize row: the fit target is the initial geometry (the
        // legacy 80x24 fallback), never zero.
        let p = player_over(&[ev(None, 'o', b"hi")]);
        assert_eq!((p.fit_cols, p.fit_rows), FALLBACK_GEOMETRY);
    }

    #[test]
    fn backward_seek_rebuilds_and_replays_from_zero() {
        // Real recordings stamp their initial size at t=0 (first
        // flush); that first resize is the header geometry the player
        // (re)builds with, mirroring the `.cast` export.
        let mut p = player_over(&[
            ev(Some(0), 'r', b"120x30"),
            ev(Some(100), 'o', b"A"),
            ev(Some(500), 'r', b"100x40"),
            ev(Some(1_000), 'o', b"B"),
        ]);
        p.seek(1_000.0);
        assert_eq!((p.cols, p.rows), (100, 40));
        assert_eq!(cell(&p, 0, 1), 'B');

        // Back to 200 ms: fresh emulator, replayed through the first
        // two events only, geometry back to the recording's initial.
        p.seek(200.0);
        assert_eq!(cell(&p, 0, 0), 'A');
        assert_eq!(cell(&p, 0, 1), ' ');
        assert_eq!((p.cols, p.rows), (120, 30));
        assert_eq!(p.next_event, 2);
    }

    #[test]
    fn scrub_defers_the_rebuild_to_commit() {
        let mut p = player_over(&[
            ev(Some(0), 'o', b"A"),
            ev(Some(1_000), 'o', b"B"),
        ]);
        p.seek(1_000.0);
        assert_eq!(cell(&p, 0, 1), 'B');
        let events_at_end = p.next_event;

        // Dragging backward only moves the knob/label; the emulator is
        // untouched (no replay), so the frame still shows the end.
        p.scrub_to(100.0);
        assert_eq!(p.display_ms(), 100.0, "knob follows the scrub");
        assert_eq!(p.clock_ms, 1_000.0, "clock not moved yet");
        assert_eq!(p.next_event, events_at_end, "no rebuild during the drag");
        assert_eq!(cell(&p, 0, 1), 'B', "frame unchanged until release");

        // Release applies it once: clock jumps back and the frame
        // rebuilds to the earlier position.
        p.commit_scrub();
        assert_eq!(p.scrub, None);
        assert_eq!(p.clock_ms, 100.0);
        assert_eq!(p.display_ms(), 100.0);
        assert_eq!(cell(&p, 0, 1), ' ', "past the future event again");
    }

    #[test]
    fn toggle_play_after_the_end_restarts() {
        let mut p = player_over(&[ev(Some(0), 'o', b"A"), ev(Some(100), 'o', b"B")]);
        p.seek(100.0);
        assert!(p.finished());
        assert!(!p.playing);
        p.toggle_play();
        assert!(p.playing, "play at the end restarts");
        assert_eq!(p.clock_ms, 0.0);
        assert_eq!(p.next_event, 1, "the t=0 event is re-fed on restart");
    }

    fn viewer_char_at(v: &SessionLogViewer, row: i32, col: usize) -> char {
        use oryxis_terminal::alacritty_terminal::index::{Column, Line};
        let state = v.terminal.lock().unwrap();
        state.backend.term.grid()[Line(row)][Column(col)].c
    }

    #[test]
    fn session_log_viewer_renders_the_recording_and_drops_command_rows() {
        let viewer = SessionLogViewer::build(
            Uuid::nil(),
            &[
                ev(Some(0), 'r', b"80x24"),
                ev(Some(10), 'o', b"hello"),
                // A typed-command row must never paint into the grid.
                ev(Some(20), 'c', b"secret cmd"),
            ],
            TerminalPalette::default(),
            TranscriptMode::Rendered,
        )
        .expect("headless viewer");
        let row: String = (0..5).map(|c| viewer_char_at(&viewer, 0, c)).collect();
        assert_eq!(row, "hello");
    }

    #[test]
    fn session_log_viewer_keeps_the_whole_scrollback() {
        // 500 lines fed into a 24-row screen: the rest must survive in
        // scrollback, not get truncated to the visible grid.
        let mut data = Vec::new();
        for i in 0..500 {
            data.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let viewer = SessionLogViewer::build(
            Uuid::nil(),
            &[ev(Some(0), 'r', b"80x24"), ev(Some(10), 'o', &data)],
            TerminalPalette::default(),
            TranscriptMode::Rendered,
        )
        .expect("headless viewer");
        use oryxis_terminal::alacritty_terminal::grid::Dimensions;
        let history = viewer
            .terminal
            .lock()
            .unwrap()
            .backend
            .term
            .grid()
            .history_size();
        assert!(
            history >= 470,
            "scrollback dropped lines: history_size = {history}"
        );
    }

    /// A recording that ends inside an alternate-screen app (top / less /
    /// vim / htop / tmux left active at disconnect) must still be a
    /// scrollable log. The alt screen carries no history and pins the
    /// view to the live edge, so without leaving it the transcript opens
    /// at the end with no scrollbar and PageUp/wheel dead (Mazwak's
    /// report on #91): the primary scrollback with the whole session is
    /// there but hidden behind the alt buffer. `build` must return the
    /// primary screen so those lines are reachable, and the app's final
    /// frame must survive as reinjected content instead of vanishing
    /// with the alt buffer.
    #[test]
    fn session_log_viewer_ending_in_alt_screen_stays_scrollable() {
        use oryxis_terminal::alacritty_terminal::grid::Dimensions;
        let mut data = Vec::new();
        for i in 0..500 {
            data.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        // …then the session entered `top` (alt screen) and was still
        // there at disconnect: enter alt, paint a frame, never exit.
        data.extend_from_slice(b"\x1b[?1049h");
        data.extend_from_slice(b"\x1b[1;31mtop - live frame\x1b[0m");
        let viewer = SessionLogViewer::build(
            Uuid::nil(),
            &[ev(Some(0), 'r', b"80x24"), ev(Some(10), 'o', &data)],
            TerminalPalette::default(),
            TranscriptMode::Rendered,
        )
        .expect("headless viewer");
        let state = viewer.terminal.lock().unwrap();
        assert!(
            !state.is_alt_screen(),
            "viewer must leave the alternate screen so the log scrolls"
        );
        let history = state.backend.term.grid().history_size();
        assert!(
            history >= 470,
            "primary scrollback must be reachable: history_size = {history}"
        );
        // The final frame is appended after the log, in session order.
        let text = state.all_text();
        let log_tail = text.rfind("line 499").expect("log tail present");
        let frame = text
            .rfind("top - live frame")
            .expect("final alt frame reinjected into the transcript");
        assert!(frame > log_tail, "frame lands after the log tail");
    }

    /// The #92 shape: a session that ran ENTIRELY inside tmux. It enters
    /// the alternate screen right after login and never leaves, so the
    /// faithful replay has one repainted frame and no scrollback, and the
    /// reporter read that as "the session was not recorded". The linear
    /// mode has to carry every repaint, in order.
    #[test]
    fn linear_mode_recovers_a_session_spent_inside_tmux() {
        let mut data = b"login banner\r\n\x1b[?1049h".to_vec();
        // tmux repainting its pane: absolute addressing, no newlines,
        // which is exactly what a viewport-less dump has to linearize.
        for i in 0..40 {
            data.extend_from_slice(format!("\x1b[1;1H\x1b[2Jcommand-{i} output").as_bytes());
        }
        let rows = [ev(Some(0), 'r', b"80x24"), ev(Some(10), 'o', &data)];

        let rendered = SessionLogViewer::build(
            Uuid::nil(),
            &rows,
            TerminalPalette::default(),
            TranscriptMode::Rendered,
        )
        .expect("headless viewer");
        let faithful = rendered.terminal.lock().unwrap().all_text();
        assert!(
            !faithful.contains("command-0 output"),
            "the faithful replay only has the last frame, which is the bug"
        );

        let linear = SessionLogViewer::build(
            Uuid::nil(),
            &rows,
            TerminalPalette::default(),
            TranscriptMode::Linear,
        )
        .expect("headless viewer");
        let state = linear.terminal.lock().unwrap();
        assert!(!state.is_alt_screen(), "the linear dump has no alt screen");
        let text = state.all_text();
        for i in [0, 17, 39] {
            assert!(
                text.contains(&format!("command-{i} output")),
                "repaint {i} missing from the linear transcript"
            );
        }
        let first = text.find("command-0 output").unwrap();
        let last = text.find("command-39 output").unwrap();
        assert!(first < last, "repaints must keep capture order");
        assert!(text.contains("login banner"), "pre-tmux output survives");
    }

    #[test]
    fn alt_screen_share_measures_time_not_bytes() {
        // Enters tmux at 100ms of a 1000ms recording and never leaves:
        // 90% of the session is unreadable in the faithful replay.
        let rows = [
            ev(Some(0), 'o', b"banner"),
            ev(Some(100), 'o', b"\x1b[?1049h"),
            ev(Some(1000), 'o', b"frame"),
        ];
        let share = alt_screen_share(&rows);
        assert!((share - 0.9).abs() < 0.01, "share = {share}");
        assert!(share >= LINEAR_ALT_SHARE);

        // A pager opened and closed inside a long session stays Rendered.
        let rows = [
            ev(Some(0), 'o', b"work"),
            ev(Some(100), 'o', b"\x1b[?1049h"),
            ev(Some(200), 'o', b"\x1b[?1049l"),
            ev(Some(1000), 'o', b"more work"),
        ];
        let share = alt_screen_share(&rows);
        assert!((share - 0.1).abs() < 0.01, "share = {share}");
        assert!(share < LINEAR_ALT_SHARE);
    }

    /// Chunk boundaries fall wherever the flush landed, so the scan must
    /// carry a split sequence across rows or a tmux session reads as 0%.
    #[test]
    fn alt_screen_share_survives_a_split_escape_sequence() {
        let rows = [
            ev(Some(0), 'o', b"x\x1b[?10"),
            ev(Some(10), 'o', b"49h"),
            ev(Some(1000), 'o', b"frame"),
        ];
        let share = alt_screen_share(&rows);
        assert!(share > 0.98, "split sequence missed: share = {share}");
    }

    /// The scan reads recorded BYTES, and a tmux status bar is full of
    /// box-drawing glyphs: a text-slicing scan panics on the first
    /// multi-byte boundary it lands on.
    #[test]
    fn alt_screen_share_scans_non_ascii_output() {
        let rows = [
            ev(Some(0), 'o', "┌── tmux ──┐\x1b[?1049h│ pane │".as_bytes()),
            ev(Some(1000), 'o', "│ 状態 │ статус │".as_bytes()),
        ];
        assert!(alt_screen_share(&rows) > 0.98);
    }

    /// Legacy rows carry no timing, so there is no wall clock to divide
    /// by; the fallback is "did the recording end on the alt screen".
    #[test]
    fn alt_screen_share_falls_back_for_untimed_rows() {
        assert_eq!(alt_screen_share(&[ev(None, 'o', b"\x1b[?1049hframe")]), 1.0);
        assert_eq!(alt_screen_share(&[ev(None, 'o', b"plain output")]), 0.0);
    }

    #[test]
    fn cycle_speed_walks_the_steps_and_wraps() {
        let mut p = player_over(&[ev(Some(0), 'o', b"A")]);
        assert_eq!(p.speed, 1.0);
        p.cycle_speed();
        assert_eq!(p.speed, 1.5);
        p.speed = 4.0;
        p.cycle_speed();
        assert_eq!(p.speed, 0.5);
    }
}
