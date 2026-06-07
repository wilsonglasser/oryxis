pub mod backend;
pub mod widget;
pub mod pty;
pub mod colors;
pub mod mouse;

pub use backend::{set_default_scrollback, TerminalBackend};
pub use colors::{TerminalPalette, TerminalTheme};
pub use widget::{wrap_paste, TerminalView};
pub use pty::PtyHandle;
