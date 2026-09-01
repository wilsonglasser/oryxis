pub mod backend;
pub mod highlight_rules;
pub mod host_clipboard;
pub mod input_tracker;
pub mod osc;
pub mod prompt_detect;
pub mod screen_title;
pub mod trigger;
pub mod widget;
pub mod pty;
pub mod colors;
pub mod mouse;

pub use backend::{set_clipboard_access, set_default_scrollback, TerminalBackend, DEFAULT_WORD_DELIMITERS};
pub use host_clipboard::{
    has_primary_selection, take_clipboard_requests, ClipboardRequest, ClipboardSink,
};
pub use input_tracker::{InputTracker, SubmittedLine};
pub use highlight_rules::{parse_hex_color, CompiledRule, CompiledRules};
pub use osc::{PositionedShellMark, Progress, ShellMark};
pub use prompt_detect::PasswordPrompt;
pub use trigger::TriggerHit;
pub use colors::{TerminalPalette, TerminalTheme};
pub use widget::{
    ime_caret_rect, ipv4_is_private_or_loopback, ipv6_is_local, looks_like_ipv6, open_url,
    take_privacy_mask_drawn, wrap_paste, Backdrop, BackgroundImage, BgFit, HoveredLink,
    NetHud, PrivacyClasses, RegionText, RightClickAction, TerminalState, TerminalView,
};
pub use pty::PtyHandle;

/// DECRST/DECSET sequence the app feeds a pane when its session ends
/// and again when a fresh one attaches (`dispatch_ssh::session`).
///
/// A mode belongs to the program that armed it, and a program killed
/// by a dropped connection never gets to disarm its own: mouse
/// tracking (1000/1002/1003 and its 1005/1006 encodings), focus
/// reporting (1004), bracketed paste (2004), application cursor keys
/// (1), autowrap (7), cursor visibility (25) and the scrolling region
/// all outlive it. Each one either changes the bytes the WIDGET
/// synthesizes or takes something away from the user, so leaving them
/// armed made the widget keep reporting pointer moves into a shell
/// that never asked for them (the shell's echo of those reports is
/// the post-reconnect garbage), left local selection and scrollback
/// disabled on a pane whose session was already dead, and kept a
/// cursor a dead app hid hidden.
///
/// The reset is spelled out mode by mode because the emulator
/// implements neither RIS nor DECSTR, and the scrolling region is
/// reset between DECSC / DECRC since DECSTBM homes the cursor as a
/// side effect. What the previous session PRINTED is deliberately
/// untouched: this restores the terminal's input contract, not a
/// blank pane. See [`LEAVE_ALT_SCREEN`] for the half only an attach
/// sends.
pub const SESSION_MODE_RESET: &[u8] =
    b"\x1b[?1;1000;1002;1003;1004;1005;1006;2004l\x1b[?7;25h\x1b7\x1b[r\x1b8";

/// DECRST 1049, sent right before [`SESSION_MODE_RESET`] when a fresh
/// session attaches: a full-screen app the connection killed leaves the
/// pane on the alternate screen, where the new shell would paint over
/// the dead app's frame and the real buffer plus its scrollback would
/// stay unreachable. Leaving it is exactly what the app itself would
/// have done on a clean exit, and it is a no-op on a pane that never
/// entered. A disconnect deliberately does NOT send it: the frozen
/// frame is what the user is still reading.
pub const LEAVE_ALT_SCREEN: &[u8] = b"\x1b[?1049l";

// The backend exposes `Term` and grid types in its public surface
// (`TerminalBackend::term`), so consumers that inspect the grid (the
// app's session player tests, the harness) need the crate's types
// without pinning their own copy of the dependency.
pub use alacritty_terminal;
