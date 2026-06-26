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
}

impl TerminalBackend {
    pub fn new(cols: u16, rows: u16) -> Self {
        let size = TermSize { cols, rows };
        let config = TermConfig {
            scrolling_history: default_scrollback(),
            semantic_escape_chars: DEFAULT_WORD_DELIMITERS.to_string(),
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
        }
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

    /// Feed raw bytes from PTY into the terminal emulator.
    pub fn process(&mut self, bytes: &[u8]) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.processor.advance(&mut self.term, bytes);
        }));
        if result.is_err() {
            tracing::error!("Terminal processor panic on {} bytes (ignored)", bytes.len());
        }
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
}
