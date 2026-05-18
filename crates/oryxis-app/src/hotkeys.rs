//! Editable keyboard binding model.
//!
//! Each `HotkeyAction` is something the user can trigger from the
//! keyboard (open settings, switch tab, close active tab, ...). A
//! `HotkeyBinding` pairs a modifier set with a `PrimaryKey`; the
//! `match_event` helper turns an incoming iced KeyPressed into an
//! optional `FamilyMatch` which the dispatcher inspects to build the
//! final `Message`.
//!
//! Families (`Digit1to9`, `ArrowLeftRight`) are bindings where the
//! suffix isn't editable, mirroring Termius's "Ctrl + [1...9]" row.
//! Only their modifier set can change.

use std::collections::HashMap;
use std::fmt::Write;

use iced::keyboard::{self, key::Named, Key, Modifiers};

/// Stable identifier for every editable action. Persisted to the
/// settings table as `hotkey_<snake_case_name>` so renames are
/// breaking changes; treat the variant order as append-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyAction {
    // Navigation / global pickers
    ShowNewTabPicker,
    ShowTabJump,
    OpenLocalShell,
    NewWindow,
    CloseActiveTab,
    OpenPortForwards,
    OpenSettings,
    FocusViewSearch,
    // Tab strip
    SwitchToTabSlot,   // family: Ctrl + digit 1..9
    CycleTabs,         // family: Alt + ArrowLeft/Right
    // Window
    ToggleFullscreen,
    // Font zoom (the three discrete keys; wheel zoom isn't editable)
    FontZoomIn,
    FontZoomOut,
    FontZoomReset,
}

impl HotkeyAction {
    /// All actions in display order. Used by the Settings panel to
    /// iterate without forgetting one.
    pub fn all() -> &'static [HotkeyAction] {
        use HotkeyAction::*;
        &[
            ShowNewTabPicker,
            ShowTabJump,
            OpenLocalShell,
            NewWindow,
            CloseActiveTab,
            OpenPortForwards,
            OpenSettings,
            FocusViewSearch,
            SwitchToTabSlot,
            CycleTabs,
            ToggleFullscreen,
            FontZoomIn,
            FontZoomOut,
            FontZoomReset,
        ]
    }

    /// Stable snake_case id used in the settings key
    /// (`hotkey_show_new_tab_picker`, ...). Must not change after a
    /// release ships.
    pub fn id(self) -> &'static str {
        use HotkeyAction::*;
        match self {
            ShowNewTabPicker => "show_new_tab_picker",
            ShowTabJump => "show_tab_jump",
            OpenLocalShell => "open_local_shell",
            NewWindow => "new_window",
            CloseActiveTab => "close_active_tab",
            OpenPortForwards => "open_port_forwards",
            OpenSettings => "open_settings",
            FocusViewSearch => "focus_view_search",
            SwitchToTabSlot => "switch_to_tab_slot",
            CycleTabs => "cycle_tabs",
            ToggleFullscreen => "toggle_fullscreen",
            FontZoomIn => "font_zoom_in",
            FontZoomOut => "font_zoom_out",
            FontZoomReset => "font_zoom_reset",
        }
    }

    /// i18n key for the action's display label.
    pub fn label_key(self) -> &'static str {
        use HotkeyAction::*;
        match self {
            ShowNewTabPicker => "hotkey_show_new_tab_picker",
            ShowTabJump => "hotkey_show_tab_jump",
            OpenLocalShell => "hotkey_open_local_shell",
            NewWindow => "hotkey_new_window",
            CloseActiveTab => "hotkey_close_active_tab",
            OpenPortForwards => "hotkey_open_port_forwards",
            OpenSettings => "hotkey_open_settings",
            FocusViewSearch => "hotkey_focus_view_search",
            SwitchToTabSlot => "hotkey_switch_to_tab_slot",
            CycleTabs => "hotkey_cycle_tabs",
            ToggleFullscreen => "hotkey_toggle_fullscreen",
            FontZoomIn => "hotkey_font_zoom_in",
            FontZoomOut => "hotkey_font_zoom_out",
            FontZoomReset => "hotkey_font_zoom_reset",
        }
    }

    /// Whether the primary key (suffix) is editable. Family actions
    /// are modifier-only; everything else accepts any single primary.
    pub fn primary_editable(self) -> bool {
        !matches!(self, HotkeyAction::SwitchToTabSlot | HotkeyAction::CycleTabs)
    }

    /// Skip this action while the active view is the Terminal so the
    /// shell still receives the keystroke (Ctrl+L = clear, Ctrl+T =
    /// transpose, Ctrl+P = previous history, Ctrl+F = forward char).
    /// Pre-v0.7 marker that was checked per-action to decide whether
    /// the app should claim a key while the terminal view was active.
    /// Kept around for any caller that still wants a hint, but the
    /// dispatcher now uses [`HotkeyBinding::is_terminal_control_sequence`]
    /// against the CURRENT binding so rebinding `CloseActiveTab` away
    /// from a shell sequence frees that key for the PTY, and rebinding
    /// a non-gated action TO a shell sequence correctly gates it.
    #[deprecated(note = "use HotkeyBinding::is_terminal_control_sequence on the active binding")]
    #[allow(dead_code)]
    pub fn gate_in_terminal(self) -> bool {
        matches!(
            self,
            HotkeyAction::ShowNewTabPicker
                | HotkeyAction::OpenLocalShell
                | HotkeyAction::OpenPortForwards
                | HotkeyAction::FocusViewSearch
        )
    }
}

/// The non-modifier half of a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryKey {
    /// A printable character, ASCII case-insensitive (`'k'` matches
    /// both `"k"` and `"K"`).
    Char(char),
    /// A named key (F11, Escape, ',', '=', ...). Stored as
    /// `iced::keyboard::key::Named` plus an optional character
    /// fallback for punctuation.
    Named(Named),
    /// Single-char punctuation that iced reports as `Character` not
    /// `Named` (`,`, `=`, `-`, `+`). Kept as a distinct variant from
    /// `Char` because the editor needs to know it's punctuation when
    /// rendering the badge.
    Punct(&'static str),
    /// Family: any digit 1..9. Suffix isn't editable.
    Digit1to9,
    /// Family: ArrowLeft or ArrowRight. Suffix isn't editable.
    ArrowLeftRight,
}

/// What `HotkeyBinding::match_event` returns: `None` if the event
/// didn't match this binding; `Some(FamilyMatch)` if it did, carrying
/// any extracted payload from the family variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyMatch {
    /// Plain match, no payload.
    Plain,
    /// Digit family matched digit `n` (1..=9).
    Digit(u8),
    /// Arrow family matched left arrow.
    ArrowLeft,
    /// Arrow family matched right arrow.
    ArrowRight,
}

/// A modifier set + primary key. `Modifiers` from iced isn't stored
/// directly so we can `PartialEq` and serialize it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
    pub primary: PrimaryKey,
}

impl HotkeyBinding {
    /// Returns `Some(FamilyMatch)` when this binding fires for the
    /// given event, otherwise `None`. Modifier match is exact (a
    /// binding with no Shift won't fire when Shift is held), this
    /// avoids the `Ctrl+1` / `Ctrl+!` confusion on US layouts.
    pub fn match_event(&self, key: &Key, modifiers: &Modifiers) -> Option<FamilyMatch> {
        if modifiers.control() != self.ctrl
            || modifiers.shift() != self.shift
            || modifiers.alt() != self.alt
            || modifiers.logo() != self.logo
        {
            return None;
        }
        match self.primary {
            PrimaryKey::Char(c) => match key {
                Key::Character(s) => {
                    let s = s.as_str();
                    if s.len() == 1 && s.eq_ignore_ascii_case(&c.to_string()) {
                        Some(FamilyMatch::Plain)
                    } else {
                        None
                    }
                }
                _ => None,
            },
            PrimaryKey::Named(n) => match key {
                Key::Named(actual) if *actual == n => Some(FamilyMatch::Plain),
                _ => None,
            },
            PrimaryKey::Punct(p) => match key {
                Key::Character(s) if s.as_str() == p => Some(FamilyMatch::Plain),
                _ => None,
            },
            PrimaryKey::Digit1to9 => match key {
                Key::Character(s) => s
                    .as_str()
                    .chars()
                    .next()
                    .and_then(|ch| ch.to_digit(10))
                    .filter(|d| (1..=9).contains(d))
                    .map(|d| FamilyMatch::Digit(d as u8)),
                _ => None,
            },
            PrimaryKey::ArrowLeftRight => match key {
                Key::Named(Named::ArrowLeft) => Some(FamilyMatch::ArrowLeft),
                Key::Named(Named::ArrowRight) => Some(FamilyMatch::ArrowRight),
                _ => None,
            },
        }
    }

    /// Whether the binding is valid for the editor: it must carry at
    /// least one of Ctrl / Alt / Logo (Shift alone doesn't count,
    /// `Shift+letter` is just uppercase typing) OR target a function
    /// key, otherwise the binding would silently intercept the
    /// user's typing.
    pub fn is_safe(&self) -> bool {
        if self.ctrl || self.alt || self.logo {
            return true;
        }
        // Shift + function key is fine; Shift alone on a letter is
        // just uppercase typing and would steal text input.
        if self.is_function_key_primary() {
            return true;
        }
        false
    }

    /// `true` when this binding looks like a sequence the terminal
    /// shell normally consumes itself: Ctrl + printable character with
    /// no other modifier. Examples: Ctrl+L = clear, Ctrl+P = history
    /// prev, Ctrl+K = readline kill, Ctrl+[ = Escape byte. Ctrl+Shift+X
    /// is NOT included because shells don't interpret it as a control
    /// byte. Used by the dispatcher to suppress app-level handling
    /// when the terminal view is focused.
    pub fn is_terminal_control_sequence(&self) -> bool {
        if !self.ctrl || self.alt || self.logo || self.shift {
            return false;
        }
        match self.primary {
            PrimaryKey::Char(c) => c.is_ascii_alphanumeric(),
            PrimaryKey::Punct(_) => true,
            _ => false,
        }
    }

    /// `true` when the primary is F1..F12. Extracted as a helper so
    /// both `is_safe` and the family-capture guard read the same
    /// definition.
    fn is_function_key_primary(&self) -> bool {
        matches!(
            self.primary,
            PrimaryKey::Named(
                Named::F1
                    | Named::F2
                    | Named::F3
                    | Named::F4
                    | Named::F5
                    | Named::F6
                    | Named::F7
                    | Named::F8
                    | Named::F9
                    | Named::F10
                    | Named::F11
                    | Named::F12
            )
        )
    }

    /// Serialize for the settings table: `"ctrl+shift+n"` /
    /// `"alt+arrows"` / `"f11"`. Lowercase, plus-separated, modifiers
    /// in canonical order so a round-trip never reformats.
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("ctrl+");
        }
        if self.shift {
            out.push_str("shift+");
        }
        if self.alt {
            out.push_str("alt+");
        }
        if self.logo {
            out.push_str("logo+");
        }
        match self.primary {
            PrimaryKey::Char(c) => {
                let _ = write!(out, "{}", c.to_ascii_lowercase());
            }
            PrimaryKey::Named(n) => out.push_str(named_to_str(n)),
            PrimaryKey::Punct(p) => out.push_str(p),
            PrimaryKey::Digit1to9 => out.push_str("digit"),
            PrimaryKey::ArrowLeftRight => out.push_str("arrows"),
        }
        out
    }

    /// Reverse of `serialize`. Returns `None` for malformed input or
    /// unknown tokens (the caller falls back to the default binding).
    pub fn parse(s: &str) -> Option<Self> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut logo = false;
        let parts: Vec<&str> = s.split('+').collect();
        let (mods, primary_str) = parts.split_at(parts.len().saturating_sub(1));
        let primary_str = primary_str.first()?;
        for m in mods {
            match *m {
                "ctrl" => ctrl = true,
                "shift" => shift = true,
                "alt" => alt = true,
                "logo" => logo = true,
                _ => return None,
            }
        }
        let primary = match *primary_str {
            "digit" => PrimaryKey::Digit1to9,
            "arrows" => PrimaryKey::ArrowLeftRight,
            "," | "." | ";" | "=" | "-" | "+" | "/" | "\\" | "[" | "]" => {
                // Static slice lookup keeps the &'static str alive.
                match *primary_str {
                    "," => PrimaryKey::Punct(","),
                    "." => PrimaryKey::Punct("."),
                    ";" => PrimaryKey::Punct(";"),
                    "=" => PrimaryKey::Punct("="),
                    "-" => PrimaryKey::Punct("-"),
                    "+" => PrimaryKey::Punct("+"),
                    "/" => PrimaryKey::Punct("/"),
                    "\\" => PrimaryKey::Punct("\\"),
                    "[" => PrimaryKey::Punct("["),
                    "]" => PrimaryKey::Punct("]"),
                    _ => unreachable!(),
                }
            }
            other => {
                if let Some(named) = str_to_named(other) {
                    PrimaryKey::Named(named)
                } else if other.len() == 1
                    && other
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
                {
                    // Digit chars (0..=9) round-trip as Char; the
                    // `digit` family token is reserved for the 1..9
                    // suffix variant of `SwitchToTabSlot`.
                    PrimaryKey::Char(other.chars().next().unwrap().to_ascii_lowercase())
                } else {
                    return None;
                }
            }
        };
        Some(HotkeyBinding {
            ctrl,
            shift,
            alt,
            logo,
            primary,
        })
    }

    /// Returns the user-facing badges for the binding (e.g.
    /// `["Ctrl", "Shift", "N"]`). Family suffixes render as their
    /// fixed glyph token (`"1...9"`, `"←/→"`).
    pub fn badges(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.ctrl {
            out.push("Ctrl".into());
        }
        if self.shift {
            out.push("Shift".into());
        }
        if self.alt {
            out.push("Alt".into());
        }
        if self.logo {
            // Render as Win on Windows / Linux, ⌘ on macOS. iced
            // doesn't expose the host OS at this layer so we pick
            // the cross-platform "Super" token.
            out.push("Super".into());
        }
        let primary = match self.primary {
            PrimaryKey::Char(c) => c.to_ascii_uppercase().to_string(),
            PrimaryKey::Named(n) => named_to_str(n).to_uppercase(),
            PrimaryKey::Punct(p) => p.to_string(),
            PrimaryKey::Digit1to9 => "1...9".into(),
            PrimaryKey::ArrowLeftRight => "←/→".into(),
        };
        out.push(primary);
        out
    }
}

fn named_to_str(n: Named) -> &'static str {
    match n {
        Named::Escape => "esc",
        Named::Enter => "enter",
        Named::Tab => "tab",
        Named::Backspace => "backspace",
        Named::Delete => "del",
        Named::Insert => "ins",
        Named::Home => "home",
        Named::End => "end",
        Named::PageUp => "pgup",
        Named::PageDown => "pgdn",
        Named::ArrowUp => "up",
        Named::ArrowDown => "down",
        Named::ArrowLeft => "left",
        Named::ArrowRight => "right",
        Named::Space => "space",
        Named::F1 => "f1",
        Named::F2 => "f2",
        Named::F3 => "f3",
        Named::F4 => "f4",
        Named::F5 => "f5",
        Named::F6 => "f6",
        Named::F7 => "f7",
        Named::F8 => "f8",
        Named::F9 => "f9",
        Named::F10 => "f10",
        Named::F11 => "f11",
        Named::F12 => "f12",
        _ => "?",
    }
}

fn str_to_named(s: &str) -> Option<Named> {
    Some(match s {
        "esc" => Named::Escape,
        "enter" => Named::Enter,
        "tab" => Named::Tab,
        "backspace" => Named::Backspace,
        "del" => Named::Delete,
        "ins" => Named::Insert,
        "home" => Named::Home,
        "end" => Named::End,
        "pgup" => Named::PageUp,
        "pgdn" => Named::PageDown,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "space" => Named::Space,
        "f1" => Named::F1,
        "f2" => Named::F2,
        "f3" => Named::F3,
        "f4" => Named::F4,
        "f5" => Named::F5,
        "f6" => Named::F6,
        "f7" => Named::F7,
        "f8" => Named::F8,
        "f9" => Named::F9,
        "f10" => Named::F10,
        "f11" => Named::F11,
        "f12" => Named::F12,
        _ => return None,
    })
}

/// Builds a `HotkeyBinding` from a captured iced KeyPressed event,
/// or `None` if the event can't be turned into a safe binding (no
/// modifier and not a function key). Used by capture mode in the
/// Settings → Shortcuts editor.
pub fn binding_from_event(
    key: &Key,
    modifiers: &Modifiers,
    primary_editable: bool,
) -> Option<HotkeyBinding> {
    // For family bindings (modifier-only edit) we ignore the primary
    // and just take the modifier set; the caller substitutes the
    // existing primary back in. The editor passes `primary_editable
    // = false` for those.
    let primary_opt: Option<PrimaryKey> = if primary_editable {
        match key {
            Key::Character(s) => {
                let txt = s.as_str();
                if txt.len() == 1 {
                    let ch = txt.chars().next().unwrap();
                    if ch.is_ascii_alphanumeric() {
                        Some(PrimaryKey::Char(ch.to_ascii_lowercase()))
                    } else if matches!(
                        ch,
                        ',' | '.' | ';' | '=' | '-' | '+' | '/' | '\\' | '[' | ']'
                    ) {
                        let p: &'static str = match ch {
                            ',' => ",",
                            '.' => ".",
                            ';' => ";",
                            '=' => "=",
                            '-' => "-",
                            '+' => "+",
                            '/' => "/",
                            '\\' => "\\",
                            '[' => "[",
                            ']' => "]",
                            _ => unreachable!(),
                        };
                        Some(PrimaryKey::Punct(p))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Key::Named(n) => Some(PrimaryKey::Named(*n)),
            _ => None,
        }
    } else {
        None
    };

    let binding = HotkeyBinding {
        ctrl: modifiers.control(),
        shift: modifiers.shift(),
        alt: modifiers.alt(),
        logo: modifiers.logo(),
        primary: primary_opt.unwrap_or(PrimaryKey::Char('?')),
    };
    if primary_editable {
        if !binding.is_safe() {
            return None;
        }
    } else {
        // Family captures keep the existing primary (a digit, an
        // arrow, etc.) and only swap the modifiers. The user must
        // still pick at least one of Ctrl / Alt / Logo, otherwise
        // any bare digit press would hijack tab switching.
        if !binding.ctrl && !binding.alt && !binding.logo {
            return None;
        }
    }
    Some(binding)
}

/// Map from action to its current binding (default or user override).
pub type HotkeyMap = HashMap<HotkeyAction, HotkeyBinding>;

/// Hardcoded factory defaults. Settings overrides land on top of
/// this map in `boot.rs::load_data_from_vault`.
pub fn default_bindings() -> HotkeyMap {
    use HotkeyAction::*;
    use PrimaryKey::*;
    let mut m = HotkeyMap::new();
    let put = |m: &mut HotkeyMap, a, ctrl, shift, alt, logo, p| {
        m.insert(
            a,
            HotkeyBinding {
                ctrl,
                shift,
                alt,
                logo,
                primary: p,
            },
        );
    };
    put(&mut m, ShowNewTabPicker, true, false, false, false, Char('k'));
    put(&mut m, ShowTabJump, true, false, false, false, Char('j'));
    put(&mut m, OpenLocalShell, true, false, false, false, Char('l'));
    put(&mut m, NewWindow, true, true, false, false, Char('n'));
    put(&mut m, CloseActiveTab, true, true, false, false, Char('w'));
    put(&mut m, OpenPortForwards, true, false, false, false, Char('p'));
    put(&mut m, OpenSettings, true, false, false, false, Punct(","));
    put(&mut m, FocusViewSearch, true, false, false, false, Char('f'));
    put(&mut m, SwitchToTabSlot, true, false, false, false, Digit1to9);
    put(&mut m, CycleTabs, false, false, true, false, ArrowLeftRight);
    put(&mut m, ToggleFullscreen, false, false, false, false, Named(keyboard::key::Named::F11));
    put(&mut m, FontZoomIn, true, false, false, false, Punct("="));
    put(&mut m, FontZoomOut, true, false, false, false, Punct("-"));
    put(&mut m, FontZoomReset, true, false, false, false, Char('0'));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_serialize_parse() {
        let defaults = default_bindings();
        for binding in defaults.values() {
            let s = binding.serialize();
            let parsed = HotkeyBinding::parse(&s)
                .unwrap_or_else(|| panic!("parse failed for {s}"));
            assert_eq!(
                *binding, parsed,
                "round-trip mismatch for {s}: {binding:?} != {parsed:?}"
            );
        }
    }

    #[test]
    fn family_match_extracts_digit() {
        let b = HotkeyBinding {
            ctrl: true,
            shift: false,
            alt: false,
            logo: false,
            primary: PrimaryKey::Digit1to9,
        };
        let mods = Modifiers::CTRL;
        let key = Key::Character("3".into());
        assert_eq!(b.match_event(&key, &mods), Some(FamilyMatch::Digit(3)));
        let bad = Key::Character("0".into());
        assert_eq!(b.match_event(&bad, &mods), None);
    }

    #[test]
    fn family_match_extracts_arrow() {
        let b = HotkeyBinding {
            ctrl: false,
            shift: false,
            alt: true,
            logo: false,
            primary: PrimaryKey::ArrowLeftRight,
        };
        let mods = Modifiers::ALT;
        assert_eq!(
            b.match_event(&Key::Named(Named::ArrowRight), &mods),
            Some(FamilyMatch::ArrowRight),
        );
        assert_eq!(
            b.match_event(&Key::Named(Named::ArrowLeft), &mods),
            Some(FamilyMatch::ArrowLeft),
        );
        assert_eq!(b.match_event(&Key::Named(Named::ArrowUp), &mods), None);
    }

    #[test]
    fn shift_diff_blocks_match() {
        // Ctrl+K binding should NOT fire on Ctrl+Shift+K, the editor
        // exact-matches modifiers so the two combos can be bound to
        // different actions.
        let b = HotkeyBinding {
            ctrl: true,
            shift: false,
            alt: false,
            logo: false,
            primary: PrimaryKey::Char('k'),
        };
        assert_eq!(
            b.match_event(&Key::Character("k".into()), &(Modifiers::CTRL | Modifiers::SHIFT)),
            None
        );
        assert_eq!(
            b.match_event(&Key::Character("k".into()), &Modifiers::CTRL),
            Some(FamilyMatch::Plain)
        );
    }

    #[test]
    fn safe_requires_modifier_or_function_key() {
        let unsafe_binding = HotkeyBinding {
            ctrl: false,
            shift: false,
            alt: false,
            logo: false,
            primary: PrimaryKey::Char('a'),
        };
        assert!(!unsafe_binding.is_safe());

        let f_key = HotkeyBinding {
            ctrl: false,
            shift: false,
            alt: false,
            logo: false,
            primary: PrimaryKey::Named(Named::F11),
        };
        assert!(f_key.is_safe());

        let ctrl_a = HotkeyBinding {
            ctrl: true,
            shift: false,
            alt: false,
            logo: false,
            primary: PrimaryKey::Char('a'),
        };
        assert!(ctrl_a.is_safe());
    }
}
