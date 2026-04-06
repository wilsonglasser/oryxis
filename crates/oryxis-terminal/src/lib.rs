pub mod backend;
pub mod widget;
pub mod pty;
pub mod colors;

pub use backend::TerminalBackend;
pub use colors::{TerminalPalette, TerminalTheme};
pub use widget::TerminalView;
pub use pty::PtyHandle;
