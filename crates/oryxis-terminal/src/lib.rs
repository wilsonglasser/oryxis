pub mod backend;
pub mod widget;
pub mod pty;
pub mod colors;
pub mod mouse;

pub use backend::{set_clipboard_access, set_default_scrollback, TerminalBackend, DEFAULT_WORD_DELIMITERS};
pub use colors::{TerminalPalette, TerminalTheme};
pub use widget::{ime_caret_rect, wrap_paste, TerminalState, TerminalView};
pub use pty::PtyHandle;
