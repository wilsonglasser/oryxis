//! XTerm mouse-reporting encoders. When the remote application turns on
//! mouse tracking (DECSET 1000 / 1002 / 1003, optionally with the SGR
//! 1006 extension) the emulator has to translate local pointer events
//! into the escape sequences the app reads on its stdin. tmux
//! `set -g mouse on`, vim `set mouse=a`, htop, less and friends all
//! rely on this; without it those apps never see the mouse and the
//! feature looks broken.
//!
//! Two wire formats:
//!   - **SGR (1006)** — `ESC [ < Cb ; Cx ; Cy M` for press / motion,
//!     `... m` for release. The modern default; no coordinate ceiling.
//!   - **Legacy X10** — `ESC [ M Cb Cx Cy`, each value a single byte
//!     offset by 32. Can't disambiguate the released button (reports
//!     code 3) and breaks past column / row 223. Used only when the
//!     app didn't request SGR.

use alacritty_terminal::term::TermMode;

/// Logical button for a report. Wheel is modelled as a button because
/// that's exactly how the protocol encodes it (codes 64 / 65). `None`
/// is the "no button" sentinel used for any-motion tracking (1003)
/// while nothing is pressed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    None,
}

impl MouseButton {
    /// Base button code before the motion / modifier bits are folded in.
    fn code(self) -> u8 {
        match self {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::WheelUp => 64,
            MouseButton::WheelDown => 65,
            // "No button" shares the release code; only meaningful with
            // the motion bit set (any-motion tracking).
            MouseButton::None => 3,
        }
    }
}

/// The three event shapes the encoder handles.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    Press,
    Release,
    /// Pointer moved (drag while a button is held, or bare motion under
    /// any-motion tracking).
    Motion,
}

/// Keyboard modifier state folded into the report's button byte.
#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Encode one mouse event, or `None` when the current terminal mode says
/// it shouldn't be reported (e.g. motion while only click tracking is on,
/// or a legacy coordinate past the single-byte ceiling).
///
/// `col` / `row` are zero-based cell coordinates; the wire protocol is
/// one-based, so we add 1 here.
pub fn encode(
    mode: TermMode,
    kind: MouseEventKind,
    button: MouseButton,
    col: u16,
    row: u16,
    mods: Mods,
) -> Option<Vec<u8>> {
    // Motion is only reported when the app asked for drag tracking
    // (1002) or any-motion tracking (1003).
    if kind == MouseEventKind::Motion
        && !mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
    {
        return None;
    }

    let mut cb = button.code();
    if kind == MouseEventKind::Motion {
        cb += 32;
    }
    if mods.shift {
        cb += 4;
    }
    if mods.alt {
        cb += 8;
    }
    if mods.ctrl {
        cb += 16;
    }

    // One-based coordinates.
    let cx = col.saturating_add(1);
    let cy = row.saturating_add(1);

    if mode.contains(TermMode::SGR_MOUSE) {
        // ESC [ < Cb ; Cx ; Cy  (M = press / motion, m = release)
        let final_byte = if kind == MouseEventKind::Release { b'm' } else { b'M' };
        let mut out = format!("\x1b[<{cb};{cx};{cy}").into_bytes();
        out.push(final_byte);
        Some(out)
    } else {
        // Legacy X10. Release can't carry which button went up, so the
        // low two bits become 3 while the motion / modifier bits stay.
        let cb_legacy = if kind == MouseEventKind::Release {
            (cb & 0xFC) | 0x03
        } else {
            cb
        };
        // Each field is a single byte offset by 32; anything past 255
        // (column / row > 223) can't be expressed, so drop the report
        // rather than emit garbage. Apps relying on big grids request
        // SGR anyway.
        let bx = cx.checked_add(32).filter(|v| *v <= 255)? as u8;
        let by = cy.checked_add(32).filter(|v| *v <= 255)? as u8;
        let cb_byte = cb_legacy.checked_add(32)?;
        Some(vec![0x1b, b'[', b'M', cb_byte, bx, by])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sgr() -> TermMode {
        TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE
    }

    fn s(bytes: Option<Vec<u8>>) -> String {
        String::from_utf8(bytes.unwrap()).unwrap()
    }

    #[test]
    fn sgr_left_press_release_is_one_based() {
        // Cell (0,0) -> column 1, row 1; 'M' press, 'm' release.
        let press = encode(sgr(), MouseEventKind::Press, MouseButton::Left, 0, 0, Mods::default());
        assert_eq!(s(press), "\x1b[<0;1;1M");
        let release =
            encode(sgr(), MouseEventKind::Release, MouseButton::Left, 0, 0, Mods::default());
        assert_eq!(s(release), "\x1b[<0;1;1m");
    }

    #[test]
    fn sgr_right_button_and_modifiers() {
        // Right button = 2, +ctrl 16 -> 18, at cell (9,4) -> col 10 row 5.
        let mods = Mods { shift: false, alt: false, ctrl: true };
        let press = encode(sgr(), MouseEventKind::Press, MouseButton::Right, 9, 4, mods);
        assert_eq!(s(press), "\x1b[<18;10;5M");
    }

    #[test]
    fn sgr_wheel_uses_64_65() {
        let up = encode(sgr(), MouseEventKind::Press, MouseButton::WheelUp, 0, 0, Mods::default());
        assert_eq!(s(up), "\x1b[<64;1;1M");
        let down =
            encode(sgr(), MouseEventKind::Press, MouseButton::WheelDown, 0, 0, Mods::default());
        assert_eq!(s(down), "\x1b[<65;1;1M");
    }

    #[test]
    fn motion_gated_on_drag_or_motion_modes() {
        // Click-only tracking: motion suppressed.
        let none = encode(sgr(), MouseEventKind::Motion, MouseButton::Left, 0, 0, Mods::default());
        assert!(none.is_none());
        // Drag tracking on: motion reported with the +32 motion bit (left = 0 + 32).
        let drag_mode = sgr() | TermMode::MOUSE_DRAG;
        let moved = encode(drag_mode, MouseEventKind::Motion, MouseButton::Left, 0, 0, Mods::default());
        assert_eq!(s(moved), "\x1b[<32;1;1M");
    }

    #[test]
    fn legacy_release_loses_button_and_offsets_by_32() {
        // No SGR bit: X10 form. Press left at cell (0,0): Cb=32, Cx=33, Cy=33.
        let click = TermMode::MOUSE_REPORT_CLICK;
        let press = encode(click, MouseEventKind::Press, MouseButton::Left, 0, 0, Mods::default());
        assert_eq!(press.unwrap(), vec![0x1b, b'[', b'M', 32, 33, 33]);
        // Release reports button code 3 regardless of which went up: 3+32 = 35.
        let release = encode(click, MouseEventKind::Release, MouseButton::Left, 0, 0, Mods::default());
        assert_eq!(release.unwrap(), vec![0x1b, b'[', b'M', 35, 33, 33]);
    }

    #[test]
    fn legacy_drops_coords_past_single_byte_ceiling() {
        // Column 224 (-> 225 one-based -> +32 = 257 > 255) can't be encoded.
        let click = TermMode::MOUSE_REPORT_CLICK;
        let over = encode(click, MouseEventKind::Press, MouseButton::Left, 224, 0, Mods::default());
        assert!(over.is_none());
    }
}
