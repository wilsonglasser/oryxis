use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Process-wide scrollback (lines of history) applied to every terminal
/// created afterwards. The app sets this from the user's `scrollback_rows`
/// setting at boot and whenever it changes; terminals already open keep
/// their current buffer. Defaults to 10,000 to match the historical
/// hard-coded value, so behavior is unchanged until the app overrides it.
static DEFAULT_SCROLLBACK: AtomicUsize = AtomicUsize::new(10_000);

/// Set the scrollback used by terminals created after this call.
pub fn set_default_scrollback(lines: usize) {
    DEFAULT_SCROLLBACK.store(lines, Ordering::Relaxed);
}

fn default_scrollback() -> usize {
    DEFAULT_SCROLLBACK.load(Ordering::Relaxed)
}

/// OSC 52 clipboard access gates, process-wide so the per-terminal
/// `EventProxy` can read them without threading the setting through every
/// constructor (mirrors `DEFAULT_SCROLLBACK`). Write defaults on (the common,
/// low-risk direction, tmux/vim yank-to-clipboard); read defaults off (a
/// remote app reading the local clipboard is a privacy risk).
static OSC52_WRITE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static OSC52_READ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set the OSC 52 clipboard access policy (write = apps may set the system
/// clipboard; read = apps may query it). Called by the app from its setting.
pub fn set_clipboard_access(write: bool, read: bool) {
    OSC52_WRITE.store(write, Ordering::Relaxed);
    OSC52_READ.store(read, Ordering::Relaxed);
}

/// Default set of characters that terminate a word for double-click
/// selection (the "word delimiters" / semantic-escape set). Matches
/// alacritty's own default minus the literal tab: terminal cells never
/// hold a raw `\t` (the emulator expands tabs into cursor moves and
/// spaces), so the tab delimiter is behaviorally inert and only made
/// the Settings text field awkward to edit. Space is kept since it is
/// the most common word boundary.
pub const DEFAULT_WORD_DELIMITERS: &str = ",│`|:\"' ()[]{}<>";

/// Event proxy that collects terminal events.
#[derive(Clone)]
pub struct EventProxy {
    /// Pending title from the shell.
    pub title: Arc<Mutex<Option<String>>>,
    /// Set when the shell rings the bell (BEL / `\a`). The app drains it each
    /// output batch and turns it into the user's chosen bell action
    /// (audible beep / visual flash / nothing).
    pub bell: Arc<std::sync::atomic::AtomicBool>,
    /// Sender wired to the PTY writer thread. The terminal emulator
    /// uses this to write replies back into the PTY for queries that
    /// the host (e.g. ConPTY's `\x1b[6n` cursor-position request)
    /// blocks on. Without it cmd.exe / wsl.exe stall after a few
    /// startup bytes and never paint a banner.
    pty_write_tx: Arc<Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>>,
    /// Per-instance OSC 52 clipboard override (C5 per-host quirk):
    /// `-1` = inherit the global policy, `0` = force off, `1` = force on.
    /// Checked before the global statics. Read is only ever forced OFF
    /// per-host (a host can tighten read, never grant it).
    osc52_write: Arc<std::sync::atomic::AtomicI8>,
    osc52_read: Arc<std::sync::atomic::AtomicI8>,
}

impl Default for EventProxy {
    fn default() -> Self {
        Self::new()
    }
}

impl EventProxy {
    pub fn new() -> Self {
        Self {
            title: Arc::new(Mutex::new(None)),
            bell: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pty_write_tx: Arc::new(Mutex::new(None)),
            osc52_write: Arc::new(std::sync::atomic::AtomicI8::new(-1)),
            osc52_read: Arc::new(std::sync::atomic::AtomicI8::new(-1)),
        }
    }

    /// Wires the back-channel from the terminal emulator to the PTY
    /// writer. Called by `PtyHandle::spawn_command` once the writer
    /// thread is running.
    pub fn set_pty_write_tx(&self, tx: mpsc::UnboundedSender<Vec<u8>>) {
        if let Ok(mut slot) = self.pty_write_tx.lock() {
            *slot = Some(tx);
        }
    }

    /// Set the per-instance OSC 52 clipboard overrides (C5). `None`
    /// inherits the global policy for that direction; `Some(bool)` forces
    /// it. Read is only ever forced OFF per-host (a host can tighten read,
    /// never grant it).
    pub fn set_osc52_override(&self, write: Option<bool>, read: Option<bool>) {
        let enc = |o: Option<bool>| match o {
            None => -1,
            Some(false) => 0,
            Some(true) => 1,
        };
        self.osc52_write.store(enc(write), Ordering::Relaxed);
        self.osc52_read.store(enc(read), Ordering::Relaxed);
    }

    /// Effective OSC 52 write policy: the per-instance override when set,
    /// else the global `OSC52_WRITE`.
    fn osc52_write_allowed(&self) -> bool {
        match self.osc52_write.load(Ordering::Relaxed) {
            0 => false,
            1 => true,
            _ => OSC52_WRITE.load(Ordering::Relaxed),
        }
    }

    /// Effective OSC 52 read policy: the per-instance override (only ever
    /// force-off) when set, else the global `OSC52_READ`.
    fn osc52_read_allowed(&self) -> bool {
        match self.osc52_read.load(Ordering::Relaxed) {
            0 => false,
            1 => true,
            _ => OSC52_READ.load(Ordering::Relaxed),
        }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => {
                if let Ok(mut t) = self.title.lock() {
                    *t = Some(title);
                }
            }
            // OSC ResetTitle: surface as an empty string so the app drops the
            // custom title and falls back to its connection label.
            Event::ResetTitle => {
                if let Ok(mut t) = self.title.lock() {
                    *t = Some(String::new());
                }
            }
            Event::PtyWrite(s) => {
                if let Ok(slot) = self.pty_write_tx.lock()
                    && let Some(tx) = slot.as_ref()
                {
                    let _ = tx.send(s.into_bytes());
                }
            }
            Event::Wakeup => {}
            Event::Bell => {
                self.bell.store(true, Ordering::Relaxed);
            }
            // OSC 52: an app sets the system clipboard. Gated, so a remote
            // session can't silently overwrite the clipboard when disabled.
            Event::ClipboardStore(_ty, text) if self.osc52_write_allowed() => {
                crate::widget::set_clipboard_text(&text);
            }
            // OSC 52: an app reads the system clipboard. Off by default (a
            // remote reading your clipboard is a privacy risk). When enabled,
            // the read is queued for the host (never performed here: see
            // `host_clipboard`) and the formatter builds the reply once the
            // text arrives, sent back through the PTY back-channel (the same
            // one cursor-position replies use).
            Event::ClipboardLoad(_ty, formatter) if self.osc52_read_allowed() => {
                let reply_to = Arc::clone(&self.pty_write_tx);
                crate::host_clipboard::read_text(move |text| {
                    let reply = formatter(text);
                    if let Ok(slot) = reply_to.lock()
                        && let Some(tx) = slot.as_ref()
                    {
                        let _ = tx.send(reply.into_bytes());
                    }
                });
            }
            _ => {}
        }
    }
}

/// Wraps alacritty_terminal's Term + ansi Processor.
pub struct TerminalBackend {
    pub term: Term<EventProxy>,
    processor: ansi::Processor,
    pub event_proxy: EventProxy,
    cols: u16,
    rows: u16,
    /// Kept so `set_word_delimiters` can hand a full `Config` back to
    /// `Term::set_options` (alacritty has no narrower setter exposed).
    config: TermConfig,
    /// Sniffs OSC 7/133/9 out of the byte stream (alacritty doesn't surface
    /// those as events).
    pub osc: crate::osc::OscSniffer,
    /// Strips screen's `ESC k … ST` window-title sequences before the
    /// emulator can print them as text (issue #88). Runs first, so the
    /// OSC sniffer's byte offsets refer to the filtered stream.
    screen_title: crate::screen_title::ScreenTitleFilter,
    /// OSC 133 shell-integration marks captured by `process`, each stamped
    /// with the cursor position at the moment the emulator reached the mark.
    /// Drained by `take_marks`; bounded so an undrained pane can't grow it.
    /// A deque so evicting the oldest mark at the cap is O(1): `process`
    /// runs on the UI thread, and a mark flood paying a 4096-element
    /// shift per mark is exactly the silent per-batch cost class #104
    /// hunts.
    marks: std::collections::VecDeque<crate::osc::PositionedShellMark>,
    /// Watches the output for the highlight rules that carry an action.
    /// Inert (an early return, no accumulation) while no such rule
    /// exists, which is the normal case and the whole state of a replay
    /// surface like the session player.
    trigger: crate::trigger::TriggerScanner,
    /// The rules the scanner runs. Shared with the widget, which paints
    /// the same set, so a rule can never colour one thing and fire on
    /// another.
    rules: std::sync::Arc<crate::highlight_rules::CompiledRules>,
    /// Trigger hits captured by `process`, drained by `take_trigger_hits`.
    /// Bounded for the same reason `marks` is: an undrained pane must not
    /// grow without limit.
    hits: std::collections::VecDeque<crate::trigger::TriggerHit>,
}

/// How many undrained trigger hits a backend holds. The app drains after
/// every output batch, so reaching this means nothing is listening.
const MAX_PENDING_HITS: usize = 256;

impl TerminalBackend {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self::new_with_scrollback(cols, rows, default_scrollback())
    }

    /// Like [`new`](Self::new) but with an explicit scrollback line
    /// budget instead of the process-wide default. The session-log
    /// viewer uses this to hold a whole recording (which can exceed the
    /// user's live `scrollback_rows`) without truncating the oldest
    /// lines. alacritty grows the history lazily, so a high budget costs
    /// only what the content actually fills.
    pub fn new_with_scrollback(cols: u16, rows: u16, scrollback: usize) -> Self {
        let size = TermSize { cols, rows };
        let config = TermConfig {
            scrolling_history: scrollback,
            semantic_escape_chars: DEFAULT_WORD_DELIMITERS.to_string(),
            // Let the emulator forward BOTH OSC 52 directions; the policy is
            // ours (`osc52_write_allowed` / `osc52_read_allowed`, driven by the
            // Clipboard access setting plus the per-host override). alacritty's
            // own default is `OnlyCopy`, which silently dropped every query
            // before our gate ever saw it, so "Clipboard access: read/write"
            // could never actually answer a paste request.
            osc52: alacritty_terminal::term::Osc52::CopyPaste,
            ..Default::default()
        };
        let event_proxy = EventProxy::new();
        let term = Term::new(config.clone(), &size, event_proxy.clone());
        let processor = ansi::Processor::new();

        Self {
            term,
            processor,
            event_proxy,
            cols,
            rows,
            config,
            osc: crate::osc::OscSniffer::default(),
            screen_title: crate::screen_title::ScreenTitleFilter::default(),
            marks: std::collections::VecDeque::new(),
            trigger: crate::trigger::TriggerScanner::default(),
            rules: std::sync::Arc::default(),
            hits: std::collections::VecDeque::new(),
        }
    }

    /// Install the compiled highlight rules. The same set the widget
    /// paints with, so the scanner and the colours can never disagree.
    ///
    /// Cheap to call on every output batch (a pointer comparison), which
    /// is how the app installs them: panes are created down half a dozen
    /// paths (ssh, telnet, serial, local, session groups) and a rule set
    /// that had to be pushed at creation would eventually miss one.
    pub fn set_highlight_rules(
        &mut self,
        rules: std::sync::Arc<crate::highlight_rules::CompiledRules>,
    ) {
        if std::sync::Arc::ptr_eq(&self.rules, &rules) {
            return;
        }
        self.rules = rules;
    }

    /// Drain the trigger hits captured since the last call.
    pub fn take_trigger_hits(&mut self) -> Vec<crate::trigger::TriggerHit> {
        std::mem::take(&mut self.hits).into()
    }

    /// Update the word-delimiter set used by double-click semantic
    /// selection. No-op when unchanged so the per-click sync stays
    /// cheap (`set_options` marks the grid fully damaged, so we must
    /// not call it on every mouse event).
    pub fn set_word_delimiters(&mut self, delimiters: &str) {
        if self.config.semantic_escape_chars == delimiters {
            return;
        }
        self.config.semantic_escape_chars = delimiters.to_string();
        self.term.set_options(self.config.clone());
    }

    /// Set whether Unicode "Ambiguous" width characters occupy two cells.
    ///
    /// No-op when unchanged, for the same reason
    /// `set_highlight_rules` is: the app installs this on every output
    /// batch, which is the one place every pane passes through no matter
    /// which creation path made it.
    ///
    /// A flip mid-session only governs what is written AFTER it. Cells
    /// already on the grid keep the width they were written with, and
    /// nothing rewrites them: a re-measured backscroll would move text the
    /// user already read.
    pub fn set_ambiguous_width_wide(&mut self, wide: bool) {
        if self.config.ambiguous_width_wide == wide {
            return;
        }
        self.config.ambiguous_width_wide = wide;
        self.term.set_options(self.config.clone());
    }

    /// Feed raw bytes from PTY into the terminal emulator.
    pub fn process(&mut self, bytes: &[u8]) {
        // Strip screen's `ESC k … ST` window titles first (issue #88): the
        // emulator would print their payload as text, and everything below
        // (OSC offsets, mark positions) must see the same stream it does.
        let (filtered, screen_titles) = self.screen_title.filter(bytes);
        for title in screen_titles {
            if let Ok(mut slot) = self.event_proxy.title.lock() {
                *slot = Some(title);
            }
        }
        let bytes = filtered.as_ref();
        // Watch for the rules that carry an action, on the same filtered
        // stream the emulator sees. Outside the `catch_unwind` below
        // because it is ours: a panic here is a bug to surface, not
        // third-party parser state to recover from. Suppression is read
        // BEFORE the batch: a chunk that enters the alternate screen
        // still ends with whatever the shell printed on its way in.
        if self.rules.any_triggers() {
            self.trigger.set_suppressed(
                self.term
                    .mode()
                    .contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
            );
            let rules = self.rules.clone();
            for hit in self.trigger.feed(bytes, &rules) {
                if self.hits.len() >= MAX_PENDING_HITS {
                    // Said out loud rather than dropped quietly: the app
                    // drains after every batch, so a full queue means
                    // nothing is listening, and the symptom (actions
                    // that stop firing) gives no other clue.
                    if self.hits.len() == MAX_PENDING_HITS {
                        tracing::warn!(
                            "trigger hits are not being drained; dropping the oldest"
                        );
                    }
                    self.hits.pop_front();
                }
                self.hits.push_back(hit);
            }
        }
        // Sniff OSC 7/133/9 before handing the bytes to the emulator (which
        // ignores those OSC numbers); a no-op for the common no-OSC chunk.
        let events = self.osc.feed(bytes);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if events.is_empty() {
                self.processor.advance(&mut self.term, bytes);
                return;
            }
            // OSC 133 marks in this batch: advance in mark-aligned segments
            // so each mark's cursor snapshot is taken exactly where the shell
            // emitted it. Advancing the whole batch first would sample the
            // end-of-batch cursor, which lies whenever the batch carries more
            // output after the mark (right-side prompts, command echo, ...).
            let mut start = 0;
            for ev in &events {
                self.processor.advance(&mut self.term, &bytes[start..ev.offset]);
                start = ev.offset;
                let point = self.term.grid().cursor.point;
                let abs_line =
                    self.term.grid().history_size() as i64 + i64::from(point.line.0);
                if self.marks.len() >= 4096 {
                    self.marks.pop_front();
                }
                self.marks.push_back(crate::osc::PositionedShellMark {
                    mark: ev.mark,
                    abs_line,
                    col: point.column.0 as u16,
                });
            }
            self.processor.advance(&mut self.term, &bytes[start..]);
        }));
        if result.is_err() {
            tracing::error!("Terminal processor panic on {} bytes (ignored)", bytes.len());
        }
    }

    /// Drain the OSC 133 marks captured since the last call.
    pub fn take_marks(&mut self) -> Vec<crate::osc::PositionedShellMark> {
        std::mem::take(&mut self.marks).into()
    }

    /// The password prompt printed in front of the cursor, if that is
    /// what the cursor is sitting behind (issue #117).
    ///
    /// Reads the grid rather than the byte stream: a program that
    /// blocks on a password has left its prompt on screen with the
    /// escapes already applied, so the text here is exactly what the
    /// user sees. Soft wraps are joined; the read stops AT the cursor,
    /// because "what is printed before the cursor" is the definition of
    /// a prompt and anything past it belongs to a previous frame.
    ///
    /// `None` on the alternate screen (a full-screen app draws its own
    /// prompts and owns its keys) and whenever the line does not match
    /// [`crate::prompt_detect::looks_like_password_prompt`].
    pub fn password_prompt_at_cursor(&self) -> Option<crate::prompt_detect::PasswordPrompt> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
        use alacritty_terminal::term::TermMode;

        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        let grid = self.term.grid();
        let cols = grid.columns();
        if cols == 0 {
            return None;
        }
        let point = grid.cursor.point;
        let cursor_line = point.line.0;
        let cursor_col = point.column.0.min(cols);
        let topmost = grid.topmost_line().0;

        // Walk up the soft-wrap chain to the row the logical line
        // starts on. Bounded like `read_logical_line`: a prompt longer
        // than four screen widths is not a prompt.
        let mut first = cursor_line;
        let mut walked = 0;
        while first > topmost && walked < 4 {
            let prev = &grid[Line(first - 1)];
            if !prev[Column(cols - 1)].flags.contains(CellFlags::WRAPLINE) {
                break;
            }
            first -= 1;
            walked += 1;
        }

        let mut text = String::new();
        for line in first..=cursor_line {
            let row = &grid[Line(line)];
            let end = if line == cursor_line { cursor_col } else { cols };
            for c in 0..end {
                let cell = &row[Column(c)];
                if cell.c != '\0' && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    text.push(cell.c);
                }
            }
        }
        if !crate::prompt_detect::looks_like_password_prompt(&text) {
            return None;
        }
        Some(crate::prompt_detect::PasswordPrompt {
            text: text.trim_end().to_string(),
            abs_line: grid.history_size() as i64 + i64::from(first),
        })
    }

    /// Deadline at which an open synchronized update (DEC `?2026`) must be
    /// force-flushed, or `None` when nothing is buffering. vte buffers every
    /// byte after a BSU (`ESC[?2026h`) and only applies it on the matching
    /// ESU (`ESC[?2026l`), a 2 MiB overflow, or an explicit `stop_sync`, it
    /// never expires the 150 ms timeout from inside `advance`. Driving that
    /// timeout is the host's job: without it an app that opens a sync update
    /// and then blocks on input (docker compose's `(y/N)` prompt) leaves the
    /// screen frozen on the frame before the update began. The caller
    /// schedules a wake-up at this instant and calls `flush_sync`.
    pub fn sync_timeout(&self) -> Option<std::time::Instant> {
        self.processor.sync_timeout().sync_timeout()
    }

    /// Force-end a buffered synchronized update, applying the buffered bytes
    /// to the grid. No-op when none is pending. Mirrors the 150 ms abort
    /// alacritty's own event loop performs so a never-closed update can't
    /// freeze the terminal indefinitely.
    pub fn flush_sync(&mut self) {
        if self.processor.sync_timeout().sync_timeout().is_some() {
            self.processor.stop_sync(&mut self.term);
        }
    }

    /// Resize the terminal grid.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        let size = TermSize { cols, rows };
        self.term.resize(size);
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }
}

struct TermSize {
    cols: u16,
    rows: u16,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::{Column, Line, Point};

    /// OSC 52 store: the backend must NOT touch the system clipboard itself,
    /// it queues the write for the host (see `host_clipboard`).
    #[test]
    fn osc52_store_queues_a_host_write() {
        let _serial = crate::host_clipboard::test_exclusive();
        set_clipboard_access(true, false);
        let mut backend = TerminalBackend::new(40, 5);
        // ESC ] 52 ; c ; base64("QUEUED") BEL
        backend.process(b"\x1b]52;c;UVVFVUVE\x07");
        let reqs = crate::host_clipboard::take_clipboard_requests();
        assert_eq!(reqs.len(), 1, "one queued request: {reqs:?}");
        match &reqs[0] {
            crate::host_clipboard::ClipboardRequest::Write(text) => {
                assert_eq!(text, "QUEUED");
            }
            other => panic!("expected a queued write, got {other:?}"),
        }
        set_clipboard_access(true, false);
    }

    /// OSC 52 query: the read is queued for the host, and delivering the text
    /// formats the reply and writes it back through the PTY back-channel.
    #[test]
    fn osc52_query_queues_a_host_read_and_replies_through_the_pty() {
        let _serial = crate::host_clipboard::test_exclusive();
        set_clipboard_access(true, true);
        let mut backend = TerminalBackend::new(40, 5);
        let (tx, mut rx) = mpsc::unbounded_channel();
        backend.event_proxy.set_pty_write_tx(tx);
        backend.process(b"\x1b]52;c;?\x07");
        let reqs = crate::host_clipboard::take_clipboard_requests();
        assert_eq!(reqs.len(), 1, "one queued request: {reqs:?}");
        match &reqs[0] {
            crate::host_clipboard::ClipboardRequest::Read(sink) => sink.deliver("HELLO"),
            other => panic!("expected a queued read, got {other:?}"),
        }
        let bytes = rx.try_recv().expect("reply written to the PTY back-channel");
        let reply = String::from_utf8_lossy(&bytes);
        // base64("HELLO") == "SEVMTE8="
        assert!(reply.contains("SEVMTE8"), "reply carries the clipboard: {reply:?}");
        set_clipboard_access(true, false);
    }

    /// Read denied (the default): a query queues nothing at all, so a remote
    /// can't even learn that a clipboard exists.
    #[test]
    fn osc52_query_is_ignored_when_read_is_off() {
        let _serial = crate::host_clipboard::test_exclusive();
        set_clipboard_access(true, false);
        let mut backend = TerminalBackend::new(40, 5);
        backend.process(b"\x1b]52;c;?\x07");
        assert!(
            crate::host_clipboard::take_clipboard_requests().is_empty(),
            "a denied query must queue nothing"
        );
    }

    #[test]
    fn osc52_per_instance_override_beats_global_both_ways() {
        let _serial = crate::host_clipboard::test_exclusive();
        let proxy = EventProxy::new();
        // Inherit (default): the effective policy tracks the global.
        set_clipboard_access(true, false);
        assert!(proxy.osc52_write_allowed(), "inherit follows global-on");
        set_clipboard_access(false, false);
        assert!(!proxy.osc52_write_allowed(), "inherit follows global-off");
        // Write: force-on beats global-off; force-off beats global-on.
        proxy.set_osc52_override(Some(true), None);
        assert!(proxy.osc52_write_allowed(), "force-on beats global-off");
        set_clipboard_access(true, false);
        proxy.set_osc52_override(Some(false), None);
        assert!(!proxy.osc52_write_allowed(), "force-off beats global-on");
        // Read is only ever force-off: with global read ON, an "Off" host
        // (read forced off) still blocks read, while an inherit host reads.
        set_clipboard_access(true, true);
        proxy.set_osc52_override(Some(false), Some(false));
        assert!(!proxy.osc52_read_allowed(), "force-off read beats global-on");
        proxy.set_osc52_override(Some(true), None);
        assert!(proxy.osc52_read_allowed(), "inherit read follows global-on");
        // Back to inherit tracks the global again.
        proxy.set_osc52_override(None, None);
        assert!(proxy.osc52_write_allowed());
        // Restore the process default (write on, read off) for other tests.
        set_clipboard_access(true, false);
    }

    /// `set_word_delimiters` must actually drive alacritty's native
    /// semantic search: with the default set, `foo-bar` is one word
    /// (no `-` delimiter), but after adding `-` it splits at the dash.
    /// This is the behavior the double-click word selection rides on.
    #[test]
    fn word_delimiters_drive_semantic_search() {
        let mut backend = TerminalBackend::new(40, 5);
        backend.process(b"foo-bar baz");
        let origin = Point::new(Line(0), Column(0));

        // Default set has no `-`: the word spans the whole `foo-bar`.
        let right_default = backend.term.semantic_search_right(origin).column.0;
        assert_eq!(right_default, 6, "default should treat foo-bar as one word");

        // Adding `-` as a delimiter stops the word at `foo`.
        backend.set_word_delimiters("-");
        let right_dash = backend.term.semantic_search_right(origin).column.0;
        assert_eq!(right_dash, 2, "`-` delimiter should split foo|bar");
    }

    fn cell0(backend: &TerminalBackend) -> char {
        backend.term.grid()[Line(0)][Column(0)].c
    }

    fn is_wide(backend: &TerminalBackend, column: usize) -> bool {
        backend.term.grid()[Line(0)][Column(column)]
            .flags
            .contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR)
    }

    /// `│` U+2502 is East Asian "Ambiguous": one cell for us by default,
    /// two once the host says its remote measures it that way. Everything
    /// downstream (selection, cursor math, the draw pass) reads the flag,
    /// so this one bit is the whole feature.
    #[test]
    fn ambiguous_width_is_narrow_until_asked() {
        let mut backend = TerminalBackend::new(40, 5);
        backend.process("│x".as_bytes());
        assert!(!is_wide(&backend, 0), "ambiguous defaults to one cell");
        assert_eq!(backend.term.grid()[Line(0)][Column(1)].c, 'x');

        let mut wide = TerminalBackend::new(40, 5);
        wide.set_ambiguous_width_wide(true);
        wide.process("│x".as_bytes());
        assert!(is_wide(&wide, 0), "the option must reach the emulator");
        assert_eq!(wide.term.grid()[Line(0)][Column(2)].c, 'x', "the glyph took two cells");
    }

    /// Hiragana is wide in both tables: the option must not be doing
    /// something as blunt as "everything non-ASCII is two cells".
    #[test]
    fn unambiguous_widths_are_untouched() {
        for wide in [false, true] {
            let mut backend = TerminalBackend::new(40, 5);
            backend.set_ambiguous_width_wide(wide);
            backend.process("aあ".as_bytes());
            assert!(!is_wide(&backend, 0));
            assert!(is_wide(&backend, 1));
        }
    }

    /// The app calls this on every output batch, so an unchanged value
    /// must cost nothing, and a changed one must not re-measure the text
    /// already on screen (`set_options` damages the grid, it does not
    /// rewrite it).
    #[test]
    fn flipping_ambiguous_width_leaves_written_cells_alone() {
        let mut backend = TerminalBackend::new(40, 5);
        backend.process("│".as_bytes());
        assert!(!is_wide(&backend, 0));

        backend.set_ambiguous_width_wide(true);
        assert!(!is_wide(&backend, 0), "the old cell keeps the width it was written with");

        backend.process("│".as_bytes());
        assert!(is_wide(&backend, 1), "new output follows the new setting");
    }

    /// An open DEC `?2026` synchronized update buffers output in vte: the
    /// glyph must not reach the grid, and a flush deadline must be armed.
    /// `flush_sync` (the host-driven 150 ms abort) then applies it. This is
    /// the freeze the host MUST break, vte never expires the timeout itself.
    #[test]
    fn synchronized_update_buffers_until_flush() {
        let mut backend = TerminalBackend::new(40, 5);
        backend.process(b"\x1b[?2026hX");
        assert_eq!(cell0(&backend), ' ', "buffered glyph must not reach the grid");
        assert!(backend.sync_timeout().is_some(), "an open update arms a deadline");

        backend.flush_sync();
        assert_eq!(cell0(&backend), 'X', "flush_sync must apply the buffered glyph");
        assert!(backend.sync_timeout().is_none(), "deadline clears after flush");
    }

    /// A complete BSU...ESU pair in one feed applies immediately and leaves
    /// no pending deadline, so the host arms no needless timer.
    #[test]
    fn closed_synchronized_update_needs_no_flush() {
        let mut backend = TerminalBackend::new(40, 5);
        backend.process(b"\x1b[?2026hY\x1b[?2026l");
        assert_eq!(cell0(&backend), 'Y', "closed update applies on its own");
        assert!(backend.sync_timeout().is_none(), "closed update leaves no deadline");
    }

    /// In-band terminal queries must be answered through the PtyWrite
    /// back-channel once a sender is wired (issue #48: docker compose's
    /// raw-mode prompt blocks forever on an unanswered query, freezing
    /// the session for the user). DSR `\x1b[6n` asks for the cursor
    /// position; the reply is `\x1b[{row};{col}R`, 1-based.
    #[test]
    fn dsr_query_reply_reaches_back_channel() {
        let mut backend = TerminalBackend::new(40, 5);
        let (tx, mut rx) = mpsc::unbounded_channel();
        backend.event_proxy.set_pty_write_tx(tx);
        backend.process(b"ab\x1b[6n");
        let reply = rx.try_recv().expect("DSR query must produce a reply");
        assert_eq!(reply, b"\x1b[1;3R", "cursor sits on row 1, column 3 after `ab`");
    }

    /// DECRQM private-mode queries get a report too; buildkit / docker
    /// compose probe `?2026` (synchronized output) this way before its
    /// prompt. `\x1b[?2026;2$y` = mode recognized, currently reset.
    #[test]
    fn decrqm_query_reply_reaches_back_channel() {
        let mut backend = TerminalBackend::new(40, 5);
        let (tx, mut rx) = mpsc::unbounded_channel();
        backend.event_proxy.set_pty_write_tx(tx);
        backend.process(b"\x1b[?2026$p");
        let reply = rx.try_recv().expect("DECRQM query must produce a reply");
        assert_eq!(reply, b"\x1b[?2026;2$y");
    }

    /// `flush_sync` with no update pending is a no-op (must not corrupt the
    /// grid or panic), since the timer can fire after a normal close.
    #[test]
    fn flush_sync_without_pending_update_is_noop() {
        let mut backend = TerminalBackend::new(40, 5);
        backend.process(b"Z");
        backend.flush_sync();
        assert_eq!(cell0(&backend), 'Z');
        assert!(backend.sync_timeout().is_none());
    }

    /// Read a rendered row back as text, trailing blanks trimmed. Asserting
    /// on the grid (not on the filter's output) is the point: it is the only
    /// way to prove the payload never became visible cells.
    fn line(backend: &TerminalBackend, row: usize) -> String {
        let grid = backend.term.grid();
        let cols = grid.columns();
        let mut s = String::with_capacity(cols);
        for col in 0..cols {
            s.push(grid[Line(row as i32)][Column(col)].c);
        }
        s.trim_end().to_string()
    }

    /// Issue #88 follow-up (Mazwak, CentOS 7). On a `screen*` TERM the stock
    /// `/etc/bashrc` sets
    /// `PROMPT_COMMAND='printf "\033k%s@%s:%s\033\\" ...'`, so every prompt is
    /// preceded by screen's window-title sequence. vte dispatches `ESC k` as an
    /// unhandled escape and PRINTS the payload, which is what rendered the
    /// prompt twice: `root@oldserver:~[root@oldserver ~]#`. The grid must carry
    /// the shell's prompt alone, and the title must arrive as a title.
    #[test]
    fn centos_screen_prompt_command_does_not_paint_a_second_prompt() {
        let mut backend = TerminalBackend::new(40, 5);
        // Byte for byte what bash emits on that host.
        backend.process(b"\x1bkroot@oldserver:~\x1b\\[root@oldserver ~]# ");
        assert_eq!(
            line(&backend, 0),
            "[root@oldserver ~]#",
            "the window title must not reach the grid as text"
        );
        let title = backend.event_proxy.title.lock().unwrap().clone();
        assert_eq!(title.as_deref(), Some("root@oldserver:~"), "title is surfaced");
    }

    /// The second half of the same report: with the payload occupying real
    /// columns, readline's Ctrl+R redraw (which returns to column 0 and
    /// overwrites) could not cover the stale prompt, leaving its tail visible
    /// (`(reverse-i-search)`':ot@oldserver ~]#`). With the sequence stripped
    /// the redraw covers the whole prompt, exactly as it does on xterm-256color.
    #[test]
    fn reverse_search_redraw_covers_the_whole_prompt() {
        let mut backend = TerminalBackend::new(60, 5);
        backend.process(b"\x1bkroot@oldserver:~\x1b\\[root@oldserver ~]# ");
        // Ctrl+R: bash returns to column 0 and paints the search prompt over
        // whatever was there.
        backend.process(b"\r(reverse-i-search)`': ");
        assert_eq!(
            line(&backend, 0),
            "(reverse-i-search)`':",
            "no tail of the old prompt may survive the redraw"
        );
    }

    // ── Password-prompt detection (issue #117) ───────────────────────
    //
    // These read the GRID, which is the whole point of doing it this
    // way: color, chunk splits and carriage-return redraws all resolve
    // before the matcher ever runs.

    #[test]
    fn a_colored_sudo_prompt_is_detected() {
        let mut backend = TerminalBackend::new(80, 5);
        // sudo under a theme that bolds the prompt.
        backend.process(b"\x1b[1m[sudo] password for wilson:\x1b[0m ");
        let hit = backend.password_prompt_at_cursor().expect("prompt detected");
        assert_eq!(hit.text, "[sudo] password for wilson:");
        assert_eq!(hit.abs_line, 0);
    }

    #[test]
    fn a_prompt_split_across_two_batches_is_detected() {
        // The PTY cuts wherever it likes; the grid does not care.
        let mut backend = TerminalBackend::new(80, 5);
        backend.process(b"[sudo] passw");
        assert!(backend.password_prompt_at_cursor().is_none(), "half a prompt is not a prompt");
        backend.process(b"ord for wilson: ");
        assert!(backend.password_prompt_at_cursor().is_some());
    }

    #[test]
    fn a_carriage_return_redraw_reads_the_line_that_is_actually_on_screen() {
        let mut backend = TerminalBackend::new(80, 5);
        backend.process(b"Enter password: ");
        assert!(backend.password_prompt_at_cursor().is_some());
        // The program clears the line and asks for something else.
        backend.process(b"\r\x1b[KVerification code: ");
        assert!(
            backend.password_prompt_at_cursor().is_none(),
            "the redrawn line is the one that counts"
        );
    }

    #[test]
    fn a_wrapped_prompt_is_joined_across_rows() {
        // 40 columns forces ssh's passphrase prompt onto two rows.
        let mut backend = TerminalBackend::new(40, 5);
        backend.process(b"Enter passphrase for key '/home/wilson/.ssh/id_ed25519': ");
        let hit = backend.password_prompt_at_cursor().expect("prompt detected");
        assert_eq!(hit.text, "Enter passphrase for key '/home/wilson/.ssh/id_ed25519':");
        assert_eq!(hit.abs_line, 0, "the logical line starts on its first physical row");
    }

    #[test]
    fn only_what_is_before_the_cursor_counts() {
        let mut backend = TerminalBackend::new(80, 5);
        backend.process(b"Password: ");
        assert!(backend.password_prompt_at_cursor().is_some());
        // Cursor home: the prompt is still painted, but nothing is
        // printed BEFORE the cursor any more, so nothing is waiting on
        // an answer there.
        backend.process(b"\x1b[1;1H");
        assert!(backend.password_prompt_at_cursor().is_none());
    }

    #[test]
    fn the_alternate_screen_is_never_offered_a_password() {
        let mut backend = TerminalBackend::new(80, 5);
        backend.process(b"\x1b[?1049h");
        backend.process(b"Password: ");
        assert!(
            backend.password_prompt_at_cursor().is_none(),
            "a full-screen app owns its own prompts"
        );
    }

    #[test]
    fn abs_line_follows_the_scrollback() {
        // Two prompts in a row must not share a signature, or the
        // second one (the retry after a wrong password) is swallowed as
        // a duplicate by the host's edge detection.
        let mut backend = TerminalBackend::new(80, 3);
        backend.process(b"[sudo] password for wilson: ");
        let first = backend.password_prompt_at_cursor().expect("first prompt");
        backend.process(b"\r\nSorry, try again.\r\n[sudo] password for wilson: ");
        let second = backend.password_prompt_at_cursor().expect("second prompt");
        assert_eq!(first.text, second.text, "same text, by construction");
        assert_ne!(first.abs_line, second.abs_line, "different rows: two prompts");
    }

    #[test]
    fn a_rule_with_an_action_fires_on_the_output_stream() {
        // The whole path in one place: install the compiled rules, feed
        // bytes the way the app does, drain what fired.
        let mut b = TerminalBackend::new(80, 24);
        let rule = crate::highlight_rules::CompiledRule::new(
            "r1",
            "Disk",
            "No space left",
            false,
            false,
            iced::Color::WHITE,
            true,
        )
        .unwrap();
        b.set_highlight_rules(std::sync::Arc::new(
            crate::highlight_rules::CompiledRules::new(vec![rule]),
        ));
        b.process(b"writing: No space left on device\r\n");
        let hits = b.take_trigger_hits();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rule_id, "r1");
        // Drained, so the next batch starts clean.
        assert!(b.take_trigger_hits().is_empty());
    }

    #[test]
    fn a_backend_with_no_rules_never_collects_hits() {
        let mut b = TerminalBackend::new(80, 24);
        b.process(b"No space left on device\r\n");
        assert!(b.take_trigger_hits().is_empty());
    }
}
