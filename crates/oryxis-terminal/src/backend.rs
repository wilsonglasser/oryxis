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
            Event::PtyWrite(s) => {
                if let Ok(slot) = self.pty_write_tx.lock()
                    && let Some(tx) = slot.as_ref()
                {
                    let _ = tx.send(s.into_bytes());
                }
            }
            Event::Wakeup => {}
            Event::Bell => {}
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
}
