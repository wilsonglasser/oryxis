use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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
}

impl TerminalBackend {
    pub fn new(cols: u16, rows: u16) -> Self {
        let size = TermSize { cols, rows };
        let config = TermConfig {
            scrolling_history: 10_000,
            ..Default::default()
        };
        let event_proxy = EventProxy::new();
        let term = Term::new(config, &size, event_proxy.clone());
        let processor = ansi::Processor::new();

        Self {
            term,
            processor,
            event_proxy,
            cols,
            rows,
        }
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
