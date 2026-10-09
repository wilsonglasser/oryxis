//! The factory bindings, and the per-action list that holds a user's
//! overrides.
//!
//! Data plus its container: what ships bound out of the box, and how a
//! row is parsed from and serialized back to the settings table.

use std::collections::HashMap;

use iced::keyboard::{self, Key, Modifiers};

use super::{FamilyMatch, HotkeyAction, HotkeyBinding, MouseButton, PrimaryKey, WheelDirection};

/// Settings-table token for "the user deliberately unbound this
/// action", as opposed to `""`, which means "no override, use the
/// factory chords". Not a parseable chord (`HotkeyBinding::parse`
/// rejects it: no such primary), so it can never collide with a real
/// binding.
const UNBOUND: &str = "none";

/// Which chord of an action's list an edit is aimed at. The Shortcuts
/// editor renders one chip per chord plus a trailing add button, so a
/// capture has to carry the target alongside the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySlot {
    /// Overwrite the chord at this index.
    Replace(usize),
    /// Append a chord.
    Add,
}

/// The ordered chords bound to one action.
///
/// `primary()` (index 0) is the canonical chord: it is what the
/// command palette, the tab strip and every tooltip show. The rest are
/// equal-footing alternates that fire the same action. Order is
/// display order, and `push` appends, so the factory chord stays the
/// visible one unless the user removes it.
///
/// Several actions need more than one chord out of the box, which is
/// why this is a list rather than a single binding: `Ctrl+Shift+V` and
/// `Shift+Insert` are both standard paste chords, and dropping either
/// would break muscle memory that every other terminal honours.
///
/// An empty list means "deliberately unbound", which is distinct from
/// the action being absent from the map (no stored row, so the factory
/// default applies).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotkeyBindings(Vec<HotkeyBinding>);

impl HotkeyBindings {
    /// One chord. The common case for actions with no alternate.
    pub fn single(binding: HotkeyBinding) -> Self {
        Self(vec![binding])
    }

    /// Several chords, in display order. Duplicates are dropped so the
    /// caller doesn't have to care.
    pub fn many(bindings: impl IntoIterator<Item = HotkeyBinding>) -> Self {
        let mut out = Self::default();
        for b in bindings {
            out.push(b);
        }
        out
    }

    /// The chord shown in the UI, or `None` when unbound.
    pub fn primary(&self) -> Option<HotkeyBinding> {
        self.0.first().copied()
    }

    /// Badges for the chord shown in the UI. `None` when unbound, so
    /// callers rendering a hint drop the row rather than printing an
    /// empty chord.
    pub fn badges(&self) -> Option<Vec<String>> {
        self.primary().map(|b| b.badges())
    }

    pub fn iter(&self) -> impl Iterator<Item = &HotkeyBinding> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, binding: &HotkeyBinding) -> bool {
        self.0.contains(binding)
    }

    /// Append a chord, ignoring an exact duplicate.
    pub fn push(&mut self, binding: HotkeyBinding) {
        if !self.contains(&binding) {
            self.0.push(binding);
        }
    }

    /// Write a chord into `slot`, appending when the slot is `Add` or
    /// points past the end (a stale index from a row that shrank under
    /// the editor). Any other copy of the chord in this list is dropped
    /// first, so a list can never hold the same chord twice.
    pub fn set(&mut self, slot: HotkeySlot, binding: HotkeyBinding) {
        match slot {
            HotkeySlot::Replace(i) if i < self.0.len() => {
                self.0[i] = binding;
                let mut seen = false;
                self.0.retain(|b| {
                    if *b == binding {
                        let first = !seen;
                        seen = true;
                        return first;
                    }
                    true
                });
            }
            _ => self.push(binding),
        }
    }

    /// Drop one chord. Returns whether it was there.
    pub fn remove(&mut self, binding: &HotkeyBinding) -> bool {
        let before = self.0.len();
        self.0.retain(|b| b != binding);
        self.0.len() != before
    }

    /// First chord that fires for this event, if any. Chords are
    /// modifier-exact (see `HotkeyBinding::match_event`), so at most
    /// one can match and the order only decides which is checked first.
    pub fn match_event(&self, key: &Key, modifiers: &Modifiers) -> Option<FamilyMatch> {
        self.match_event_where(key, modifiers, |_| true)
    }

    /// Whether any chord in this list is the given mouse button with
    /// exactly these modifiers.
    pub fn match_mouse(&self, button: MouseButton, modifiers: &Modifiers) -> bool {
        self.0.iter().any(|b| b.match_mouse(button, modifiers))
    }

    /// The mouse chords in this list, in display order.
    pub fn mouse_chords(&self) -> impl Iterator<Item = &HotkeyBinding> {
        self.0.iter().filter(|b| b.is_mouse())
    }

    /// Whether any chord in this list is the given wheel direction with
    /// exactly these modifiers.
    pub fn match_wheel(&self, direction: WheelDirection, modifiers: &Modifiers) -> bool {
        self.0.iter().any(|b| b.match_wheel(direction, modifiers))
    }

    /// The wheel chords in this list, in display order.
    pub fn wheel_chords(&self) -> impl Iterator<Item = &HotkeyBinding> {
        self.0.iter().filter(|b| b.is_wheel())
    }

    /// `match_event`, restricted to the chords `accept` keeps.
    ///
    /// The PTY gate is per-chord, never per-action: an action bound to
    /// both `Ctrl+Shift+V` and `Ctrl+V` must keep the first inside a
    /// terminal while yielding the second to the shell. Deciding for
    /// the whole action would either steal `^V` (if any chord being
    /// safe rescued the rest) or kill `Ctrl+Shift+V` too (if any chord
    /// being a control sequence condemned the rest).
    pub fn match_event_where(
        &self,
        key: &Key,
        modifiers: &Modifiers,
        mut accept: impl FnMut(&HotkeyBinding) -> bool,
    ) -> Option<FamilyMatch> {
        self.0
            .iter()
            .filter(|b| accept(b))
            .find_map(|b| b.match_event(key, modifiers))
    }

    /// Serialize for the settings table: chords space-separated, each
    /// in `HotkeyBinding::serialize` form (`"ctrl+shift+v shift+ins"`).
    ///
    /// The separator is a space and must stay one: every other
    /// candidate is a bindable primary. `,` is `OpenSettings`'s factory
    /// chord and `;` `.` `=` `-` `+` `/` `\` `[` `]` are all in the
    /// `Punct` set, and all of them serialize literally. A space can
    /// never appear inside a chord, because `Named::Space` serializes
    /// to the word `"space"`.
    pub fn serialize(&self) -> String {
        if self.0.is_empty() {
            return UNBOUND.to_string();
        }
        self.0
            .iter()
            .map(|b| b.serialize())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Reverse of `serialize`. `None` means "no usable override", and
    /// the caller keeps the factory chords.
    ///
    /// Three cases the settings table can hold:
    ///
    /// * `""` is *no override* (what `ResetHotkey` writes to drop a
    ///   row back to factory), so it returns `None`.
    /// * `UNBOUND` is a *deliberate* unbind and returns an empty list.
    ///   It needs its own token precisely because `""` was already
    ///   taken: the single-binding model wrote `""` for both and could
    ///   not tell them apart, so an unbound action silently regained
    ///   its factory chord on the next boot.
    /// * anything else is a chord list.
    ///
    /// Unparseable chords are skipped rather than failing the whole
    /// row, so a value written by a newer build (one that knows a key
    /// name this build doesn't) degrades to the chords this build
    /// understands instead of resetting the action. A row where
    /// nothing parses at all is malformed, not an unbind.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if s == UNBOUND {
            return Some(Self::default());
        }
        let parsed = Self::many(s.split_whitespace().filter_map(HotkeyBinding::parse));
        if parsed.is_empty() {
            return None;
        }
        Some(parsed)
    }
}

impl<'a> IntoIterator for &'a HotkeyBindings {
    type Item = &'a HotkeyBinding;
    type IntoIter = std::slice::Iter<'a, HotkeyBinding>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Map from action to its current chords (default or user override).
pub type HotkeyMap = HashMap<HotkeyAction, HotkeyBindings>;

/// Hardcoded factory defaults. Settings overrides land on top of
/// this map in `boot.rs::load_data_from_vault`.
///
/// macOS swaps Ctrl for Logo (Cmd) on the primary actions to match
/// the platform convention (Termius, VSCode, Safari all use Cmd
/// for new-tab / close-tab / find / etc on macOS). Modifier-only
/// fields are still settable in the editor so a user who wants
/// Ctrl-everywhere on macOS can rebind.
pub fn default_bindings() -> HotkeyMap {
    use HotkeyAction::*;
    use PrimaryKey::*;
    let mut m = HotkeyMap::new();
    let put = |m: &mut HotkeyMap, a, ctrl, shift, alt, logo, p| {
        m.insert(
            a,
            HotkeyBindings::single(HotkeyBinding {
                ctrl,
                shift,
                alt,
                logo,
                primary: p,
            }),
        );
    };
    // Same as `put`, for the actions that ship with alternates.
    let put_many = |m: &mut HotkeyMap, a, chords: &[(bool, bool, bool, bool, PrimaryKey)]| {
        m.insert(
            a,
            HotkeyBindings::many(chords.iter().map(|&(ctrl, shift, alt, logo, primary)| {
                HotkeyBinding {
                    ctrl,
                    shift,
                    alt,
                    logo,
                    primary,
                }
            })),
        );
    };
    // Platform-primary modifier: Cmd (logo) on macOS, Ctrl elsewhere.
    let mac = cfg!(target_os = "macos");
    let primary_ctrl = !mac;
    let primary_logo = mac;
    // Ctrl+Shift+T (Cmd+Shift+T on macOS): the terminal-world "new tab"
    // chord (GNOME Terminal / Windows Terminal), which is exactly what
    // the saved-host picker opens. NOT plain Ctrl+K as before: a bare
    // Ctrl+letter is a terminal control sequence (Ctrl+K = readline
    // kill-line) that is_terminal_control_sequence() leaves with the
    // PTY, so the picker was unreachable from inside a live terminal.
    // The Shift lifts it out of that gate.
    put(&mut m, ShowNewTabPicker, primary_ctrl, true, false, primary_logo, Char('t'));
    // Ctrl+Shift+J (Cmd+Shift+J on macOS), not plain Ctrl+J: a bare
    // Ctrl+letter is a terminal control sequence (Ctrl+J IS line feed,
    // emacs/readline accept-line) and is_terminal_control_sequence()
    // suppresses it inside the terminal view, which is exactly where
    // jumping between tabs is most useful (issue #100). The Shift lifts
    // it out of that gate, same rationale as OpenLocalShell below.
    put(&mut m, ShowTabJump, primary_ctrl, true, false, primary_logo, Char('j'));
    // Ctrl+Shift+P (Cmd+Shift+P on macOS), the VS Code convention. Plain
    // Ctrl+P is OpenPortForwards and a bare Ctrl+letter is a PTY control
    // sequence anyway; the Shift both frees it from that gate and clears
    // the OpenPortForwards binding (modifier match is exact).
    put(&mut m, ShowCommandPalette, primary_ctrl, true, false, primary_logo, Char('p'));
    // Ctrl+Shift+L (Cmd+Shift+L on macOS), not plain Ctrl+L: a bare
    // Ctrl+letter is a terminal control sequence (Ctrl+L = clear) and
    // is_terminal_control_sequence() suppresses it inside the terminal
    // view, which is exactly where opening a local shell is useful. The
    // Shift modifier lifts it out of that gate so the action fires
    // everywhere while plain Ctrl+L still reaches the PTY to clear.
    put(&mut m, OpenLocalShell, primary_ctrl, true, false, primary_logo, Char('l'));
    put(&mut m, NewWindow, primary_ctrl, true, false, primary_logo, Char('n'));
    // Ctrl+N (Cmd+N on macOS): the Termius new-host convention. A bare
    // Ctrl+letter, so inside a terminal it stays with the PTY (readline
    // next-history), like Ctrl+K / Ctrl+P above.
    put(&mut m, NewHost, primary_ctrl, false, false, primary_logo, Char('n'));
    // Ctrl+Shift+G (Cmd+Shift+G on macOS), issue #99. Moved off
    // Ctrl+Shift+T: that is the terminal-world "new tab" chord, which
    // the saved-host picker (the primary new-tab action) now owns; an
    // ad-hoc quick connect is the secondary path, so "G" for "go to a
    // host". NOT Q despite the "Quick" mnemonic: Cmd+Shift+Q is the
    // macOS system Log Out chord, and every other Oryxis default
    // teaches Cmd+Shift+<letter> muscle memory on the Mac, so binding
    // Quick there would invite the exact wrong-modifier slip into
    // logout. The Shift lifts it out of the terminal control-sequence
    // gate so it fires from inside a live terminal too.
    put(&mut m, ShowQuickConnect, primary_ctrl, true, false, primary_logo, Char('g'));
    // Ctrl+Shift+R (Cmd+Shift+R on macOS): reconnect the focused tab,
    // the browser-reload mnemonic. NOT plain Ctrl+R: that is readline
    // reverse-history-search, which the control-sequence gate rightly
    // leaves with the PTY; the Shift lifts the chord out of that gate
    // so it fires from inside a live terminal, where a dropped session
    // is actually noticed.
    put(&mut m, ReconnectTab, primary_ctrl, true, false, primary_logo, Char('r'));
    // Ctrl+Shift+D (Cmd+Shift+D on macOS): Windows Terminal's
    // duplicate-tab chord, the only terminal that ships one. It used to
    // be the side-by-side split; that moved to Windows Terminal's own
    // split chords below so the two keep the convention together.
    put(&mut m, DuplicateTab, primary_ctrl, true, false, primary_logo, Char('d'));
    // Keychain pair on Ctrl+Shift (Cmd+Shift on macOS): Shift lifts
    // both out of the terminal control-sequence gate; K mirrors the
    // key mnemonic (exact-modifier matching keeps it clear of the
    // plain Ctrl+K picker), I the identity one.
    put(&mut m, NewKey, primary_ctrl, true, false, primary_logo, Char('k'));
    put(&mut m, NewIdentity, primary_ctrl, true, false, primary_logo, Char('i'));
    put(&mut m, CloseActiveTab, primary_ctrl, true, false, primary_logo, Char('w'));
    // Ctrl+Shift+Y (Cmd+Shift+Y on macOS), NOT the browsers' Ctrl+Shift+T:
    // that chord is the terminal world's "new tab" and `ShowNewTabPicker`
    // owns it here, so the browser mnemonic is already spoken for. Y is
    // the redo letter on Windows, and undoing a close is what this is.
    // Shift for the usual reason: it lifts the chord out of the terminal
    // control-sequence gate, so it fires from inside a live session.
    put(&mut m, ReopenClosedTab, primary_ctrl, true, false, primary_logo, Char('y'));
    put(&mut m, OpenPortForwards, primary_ctrl, false, false, primary_logo, Char('p'));
    put(&mut m, OpenSettings, primary_ctrl, false, false, primary_logo, Punct(","));
    put(&mut m, FocusViewSearch, primary_ctrl, false, false, primary_logo, Char('f'));
    // Ctrl+Shift+E (Cmd+Shift+E on macOS): Shift lifts it out of the terminal
    // control-sequence gate, same rationale as OpenLocalShell. Configurable.
    put(&mut m, OpenSftp, primary_ctrl, true, false, primary_logo, Char('e'));
    // Ctrl+Shift+S (Cmd+Shift+S on macOS): S for SFTP, one key away from
    // the browser tab's E, and Shift lifts it out of the terminal's
    // control-sequence gate the same way. Ctrl+S alone would be XOFF to
    // the far side; with Shift it never reaches it.
    put(&mut m, OpenSftpConsole, primary_ctrl, true, false, primary_logo, Char('s'));
    put(&mut m, SwitchToTabSlot, primary_ctrl, false, false, primary_logo, Digit1to9);
    put(&mut m, CycleTabs, false, false, true, false, ArrowLeftRight);
    // F11 everywhere, plus Ctrl+Cmd+F on macOS, the platform's own
    // fullscreen chord (issue #238): on a Mac keyboard F11 is Show
    // Desktop unless Fn is held, so F11 alone left the Mac with no key.
    if mac {
        put_many(
            &mut m,
            ToggleFullscreen,
            &[
                (false, false, false, false, Named(keyboard::key::Named::F11)),
                (true, false, false, true, Char('f')),
            ],
        );
    } else {
        put(&mut m, ToggleFullscreen, false, false, false, false, Named(keyboard::key::Named::F11));
    }
    // Font zoom: the key chord plus Ctrl+wheel in the matching
    // direction, the gnome-terminal / Konsole / Windows Terminal
    // gesture, and the one a Windows precision touchpad turns a pinch
    // into. The wheel half is an ordinary chord in this list, so
    // Settings > Terminal's "zoom with Ctrl + wheel" toggle is a
    // shortcut for adding / removing exactly these two, the way the
    // middle-click toggle is for paste-selection. Ctrl on macOS too:
    // see `wheel_zoom_chord`.
    put_many(
        &mut m,
        FontZoomIn,
        &[
            (primary_ctrl, false, false, primary_logo, Punct("=")),
            (true, false, false, false, Wheel(WheelDirection::Up)),
        ],
    );
    put_many(
        &mut m,
        FontZoomOut,
        &[
            (primary_ctrl, false, false, primary_logo, Punct("-")),
            (true, false, false, false, Wheel(WheelDirection::Down)),
        ],
    );
    put(&mut m, FontZoomReset, primary_ctrl, false, false, primary_logo, Char('0'));
    // Terminal split panes. Off macOS, Windows Terminal's chords:
    // Alt+Shift+Plus side by side, Alt+Shift+Minus stacked. Matching
    // reads the key WITHOUT modifiers, so "Plus" on a US / ABNT row is
    // Shift+`=` and is bound as `=`; the bare `+` chord covers layouts
    // with a `+` key of its own (German, Nordic) and the numpad. Plain
    // Ctrl+D could never be the split: it is EOF, and the terminal
    // control-sequence gate rightly leaves it with the PTY. Ctrl+Shift+O
    // (the GNOME Terminal / Terminator stacked split, this binding's
    // factory chord before the move) stays as the stacked alternate.
    //
    // macOS takes iTerm2's Cmd+D: Cmd sends no control byte, so the
    // bare chord is free there. iTerm2's stacked Cmd+Shift+D is
    // `DuplicateTab` above, so the stacked split keeps Cmd+Shift+O.
    if mac {
        put(&mut m, SplitPaneVertical, false, false, false, true, Char('d'));
        put(&mut m, SplitPaneHorizontal, false, true, false, true, Char('o'));
    } else {
        put_many(
            &mut m,
            SplitPaneVertical,
            &[
                (false, true, true, false, Punct("=")),
                (false, true, true, false, Punct("+")),
            ],
        );
        put_many(
            &mut m,
            SplitPaneHorizontal,
            &[
                (false, true, true, false, Punct("-")),
                (true, true, false, false, Char('o')),
            ],
        );
    }
    put(&mut m, FocusPaneLeft, primary_ctrl, true, false, primary_logo, Named(keyboard::key::Named::ArrowLeft));
    put(&mut m, FocusPaneRight, primary_ctrl, true, false, primary_logo, Named(keyboard::key::Named::ArrowRight));
    put(&mut m, FocusPaneUp, primary_ctrl, true, false, primary_logo, Named(keyboard::key::Named::ArrowUp));
    put(&mut m, FocusPaneDown, primary_ctrl, true, false, primary_logo, Named(keyboard::key::Named::ArrowDown));
    // Ctrl+Shift+Z (Cmd+Shift+Z on macOS): Z for zoom, the tmux
    // `prefix z` convention for exactly this toggle. Shift lifts it out
    // of the terminal control-sequence gate.
    put(&mut m, ToggleMaximizePane, primary_ctrl, true, false, primary_logo, Char('z'));
    // Ctrl+Alt+P (Cmd+Alt+P on macOS): P for the pane leaving. Ctrl+Alt
    // because the Ctrl+Shift family is where the split and zoom chords
    // live, and the natural echo of Ctrl+Shift+D, Ctrl+Alt+D, belongs to
    // the desktop on two platforms (Cmd+Alt+D shows and hides the Dock
    // on macOS, Ctrl+Alt+D is Show Desktop on KDE), so the app would
    // never receive it there.
    //
    // Note for international layouts: AltGr IS Ctrl+Alt on Windows, so
    // this can collide with an accented character there. Same trade
    // `ToggleSidebarOther` already makes on Ctrl+Alt+B, and every
    // binding is rebindable.
    put(&mut m, MovePaneToNewTab, primary_ctrl, false, true, primary_logo, Char('p'));
    // Ctrl+Shift+H (Cmd+Shift+H on macOS): Shift lifts it out of the
    // terminal control-sequence gate (plain Ctrl+H is backspace on the
    // PTY), H for the History/lists sidebar. Rebindable like the rest.
    put(&mut m, FocusSidebarList, primary_ctrl, true, false, primary_logo, Char('h'));
    // Ctrl+Shift+B (Cmd+Shift+B on macOS): the VS Code toggle-sidebar
    // convention; Shift lifts it out of the control-sequence gate.
    put(&mut m, ToggleSidebar, primary_ctrl, true, false, primary_logo, Char('b'));
    // Ctrl+Alt+B (Cmd+Alt+B on macOS): the VS Code SECONDARY-side-bar
    // convention, driving the counterpart region when tabs are docked
    // to both sides (issue #102). Alt instead of Shift keeps the pair
    // one modifier apart, mirroring how VS Code relates the two.
    put(&mut m, ToggleSidebarOther, primary_ctrl, false, true, primary_logo, Char('b'));
    // Ctrl+Shift+F (Cmd+Shift+F on macOS): flip the focused SSH tab
    // between Terminal and Files. Shift lifts it out of the terminal
    // control-sequence gate (plain Ctrl+F is readline forward-char /
    // the app's FocusViewSearch elsewhere).
    put(&mut m, ToggleTabFiles, primary_ctrl, true, false, primary_logo, Char('f'));
    // Ctrl+Shift+U (Cmd+Shift+U on macOS): arm / disarm broadcast input
    // across the tab's panes. Shift lifts it out of the terminal
    // control-sequence gate (plain Ctrl+U is readline kill-line).
    put(&mut m, ToggleBroadcastInput, primary_ctrl, true, false, primary_logo, Char('u'));
    // Ctrl+Shift+M (Cmd+Shift+M on macOS): Privacy Mode session
    // override (issue #78). Shift lifts it out of the terminal
    // control-sequence gate (plain Ctrl+M IS carriage return). Users
    // who want an F-key can rebind (F11=fullscreen is the precedent);
    // F1-F10 stay with TUI apps by default.
    put(&mut m, TogglePrivacyMode, primary_ctrl, true, false, primary_logo, Char('m'));
    // Terminal clipboard (#75). Deliberately NOT built from the
    // primary_ctrl / primary_logo pair: the platforms disagree on more
    // than which modifier to use. Elsewhere the convention is
    // Ctrl+Shift+X, because plain Ctrl+X is a control sequence the
    // shell wants (is_terminal_control_sequence). macOS has no such
    // idiom and uses bare Cmd.
    //
    // Paste also ships Shift+Insert, and only paste. That chord has real
    // pedigree (X11 tradition; PuTTY documents it as an alternative to
    // its right-click paste) and it is modifier-neutral, so it rides
    // along on macOS rather than being dropped. There is deliberately NO
    // Ctrl+Insert copy to match it: the symmetry is tempting and wrong.
    // PuTTY's own docs say copy is mouse-only by default, and GNOME
    // Terminal documents only Ctrl+Shift+C. Anyone who wants it can bind
    // it, which is the whole point of this table.
    //
    // Plain Ctrl+V is NOT a paste chord. It is the literal-next byte
    // (vim visual block, readline quoted-insert), and binding it here
    // would take it away from the PTY with no way back.
    if mac {
        put_many(&mut m, TerminalCopy, &[(false, false, false, true, Char('c'))]);
        put_many(&mut m, TerminalPaste, &[(false, false, false, true, Char('v'))]);
        put_many(&mut m, TerminalSelectAll, &[(false, false, false, true, Char('a'))]);
    } else {
        put_many(&mut m, TerminalCopy, &[(true, true, false, false, Char('c'))]);
        put_many(&mut m, TerminalPaste, &[(true, true, false, false, Char('v'))]);
        put_many(&mut m, TerminalSelectAll, &[(true, true, false, false, Char('a'))]);
    }
    // Shift+Insert pastes the PRIMARY selection, not the clipboard: the
    // xterm default, kitty's `paste_from_selection` and Alacritty's
    // `PasteSelection` all bind exactly this chord to exactly this
    // buffer, so this IS the convention, on every platform (our PRIMARY
    // is app-internal, so it exists off X11 too). The widget falls back
    // to a clipboard paste when the pane never had a selection or under
    // copy_on_select (the PuTTY single-buffer model), so the chord
    // still always pastes something, matching what it did while it was
    // a plain paste chord.
    //
    // The middle mouse button is the SECOND factory input, and it is an
    // ordinary chord in this list rather than a separate setting: the
    // binding table is the single authority for the gesture, so
    // rebinding it (or moving it onto a thumb button) is the same edit
    // as any other. Settings > Terminal's "middle-click paste" toggle is
    // a shortcut for adding / removing exactly this chord.
    put_many(
        &mut m,
        TerminalPasteSelection,
        &[
            (false, true, false, false, Named(keyboard::key::Named::Insert)),
            (false, false, false, false, Mouse(MouseButton::Middle)),
        ],
    );
    // Scrollback paging. Shift+PageUp/PageDown on every platform: it is
    // universal across terminals and macOS has no competing idiom. The
    // widget yields these to the PTY on the alternate screen, where
    // there is no scrollback and the running app owns the key.
    put(&mut m, ScrollbackPageUp, false, true, false, false, Named(keyboard::key::Named::PageUp));
    put(&mut m, ScrollbackPageDown, false, true, false, false, Named(keyboard::key::Named::PageDown));
    // Ctrl+Shift+digit (Cmd+Shift on macOS): the vault-section
    // jump family, one digit per burger-menu VAULT entry. Shift
    // keeps it clear of the Ctrl+digit tab slots.
    put(&mut m, VaultSectionSlot, primary_ctrl, true, false, primary_logo, Digit1to9);
    // Vault section cycling. Plain Ctrl on every platform (the
    // browser/IDE tab-strip convention; Cmd+PageUp/Down has no macOS
    // precedent), rebindable like everything else.
    put(&mut m, VaultSectionPrev, true, false, false, false, Named(keyboard::key::Named::PageUp));
    put(&mut m, VaultSectionNext, true, false, false, false, Named(keyboard::key::Named::PageDown));
    // Not every action is in this table on purpose: the ones
    // `HotkeyAction::ships_unbound` names (the tab menu's window verbs,
    // rename / pin / copy address, close others / all, the pane's copy
    // all / copy screen / clear scrollback, lock vault) ship with NO
    // chord. Absence from the map IS "unbound out of the box"; an empty
    // list here would read as a deliberate user unbind in the editor
    // (its Reset compares against this map). The test
    // `every_action_ships_bound_or_declares_unbound` keeps the two
    // lists in step; the debug build checks it on every boot too.
    debug_assert!(
        HotkeyAction::all()
            .iter()
            .all(|a| m.contains_key(a) != a.ships_unbound()),
        "an action is neither in the factory table nor declared unbound (or both)"
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkeys::middle_click_chord;
    use iced::keyboard::key::Named;

    #[test]
    fn round_trip_serialize_parse() {
        let defaults = default_bindings();
        for binds in defaults.values() {
            // Every chord on its own.
            for binding in binds.iter() {
                let s = binding.serialize();
                let parsed = HotkeyBinding::parse(&s)
                    .unwrap_or_else(|| panic!("parse failed for {s}"));
                assert_eq!(
                    *binding, parsed,
                    "round-trip mismatch for {s}: {binding:?} != {parsed:?}"
                );
            }
            // And the whole list, which is what the settings row holds.
            let s = binds.serialize();
            let parsed = HotkeyBindings::parse(&s)
                .unwrap_or_else(|| panic!("list parse failed for {s:?}"));
            assert_eq!(*binds, parsed, "list round-trip mismatch for {s:?}");
        }
    }

    /// A chord must never serialize to something that the list
    /// separator (a space) would split. If this ever fails, a new
    /// `PrimaryKey` is serializing to a multi-word token and every
    /// stored multi-chord row silently loses chords.
    #[test]
    fn no_default_chord_serializes_with_a_space() {
        for binds in default_bindings().values() {
            for b in binds.iter() {
                let s = b.serialize();
                assert!(
                    !s.contains(char::is_whitespace),
                    "chord {b:?} serializes to {s:?}, which the list separator would split"
                );
            }
        }
    }

    #[test]
    fn vault_section_defaults_parse_from_settings_strings() {
        // The serialized form users end up with in the settings table
        // must parse back to the exact default bindings.
        let defaults = default_bindings();
        for (action, expected) in [
            (HotkeyAction::VaultSectionPrev, "ctrl+pgup"),
            (HotkeyAction::VaultSectionNext, "ctrl+pgdn"),
        ] {
            let b = defaults.get(&action).expect("default missing");
            assert_eq!(b.serialize(), expected);
            assert_eq!(HotkeyBindings::parse(expected).as_ref(), Some(b));
        }
    }

    /// No two factory actions ship the same chord.
    ///
    /// The map is a table of hand-written `put` lines, so a new action
    /// is added by picking a chord that "looks free" out of forty-odd
    /// rows. A pair claimed twice fires both actions on one press, and
    /// nothing else in the build says so: the settings editor only
    /// resolves conflicts for chords the USER types.
    #[test]
    fn no_two_factory_actions_share_a_chord() {
        let defaults = default_bindings();
        let mut claimed: Vec<(HotkeyBinding, HotkeyAction)> = Vec::new();
        for (&action, bindings) in &defaults {
            for &b in bindings.iter() {
                if let Some((_, other)) = claimed.iter().find(|(c, _)| *c == b) {
                    panic!("{action:?} and {other:?} both ship {b:?}");
                }
                claimed.push((b, action));
            }
        }
    }

    /// Every action either ships a factory chord or is declared unbound
    /// by `ships_unbound`, never neither and never both. Both lists are
    /// hand-written: an action left out of the table by mistake would
    /// silently ship unbound, and one declared unbound that later grew a
    /// `put` line would ship a chord its own doc denies.
    #[test]
    fn every_action_ships_bound_or_declares_unbound() {
        let defaults = default_bindings();
        for &action in HotkeyAction::all() {
            let bound = defaults.get(&action).is_some_and(|b| !b.is_empty());
            assert!(
                !defaults.get(&action).is_some_and(|b| b.is_empty()),
                "{action:?} is in the factory table with an empty list; leave it out instead"
            );
            assert_ne!(
                bound,
                action.ships_unbound(),
                "{action:?}: factory chord {bound}, ships_unbound {}",
                action.ships_unbound()
            );
        }
    }

    /// The three settings-row states are distinct. `""` (no override)
    /// must not read as an unbind, or `ResetHotkey` would unbind instead
    /// of restoring the factory chord; `UNBOUND` must not read as "no
    /// override", or an unbind would silently come back on next boot,
    /// which is exactly what the single-binding model did.
    #[test]
    fn empty_row_is_not_an_unbind() {
        assert_eq!(HotkeyBindings::parse(""), None);
        assert_eq!(HotkeyBindings::parse("   "), None);
        assert_eq!(HotkeyBindings::parse(UNBOUND), Some(HotkeyBindings::default()));
        assert_eq!(HotkeyBindings::default().serialize(), UNBOUND);
        // Round-trips as a real state, not by accident.
        assert_eq!(
            HotkeyBindings::parse(&HotkeyBindings::default().serialize()),
            Some(HotkeyBindings::default())
        );
        // A row that parses to nothing usable is malformed, not an
        // unbind: the caller keeps the factory chords.
        assert_eq!(HotkeyBindings::parse("wat nonsense"), None);
    }

    /// A row written by a newer build (one that knows a key name this
    /// build doesn't) degrades to the chords this build understands
    /// rather than resetting the action to factory.
    #[test]
    fn unknown_chord_in_a_row_is_skipped_not_fatal() {
        let parsed = HotkeyBindings::parse("ctrl+shift+v futurekey shift+ins")
            .expect("should keep the chords it understands");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.serialize(), "ctrl+shift+v shift+ins");
    }

    /// Factory paste must carry BOTH conventional chords, and must not
    /// carry plain Ctrl+V: that is the shell's literal-next byte (vim
    /// visual block, readline quoted-insert), and binding it would take
    /// it from the PTY with no way back.
    #[test]
    fn paste_defaults_follow_the_terminal_convention() {
        let defaults = default_bindings();
        let paste = defaults.get(&HotkeyAction::TerminalPaste).expect("paste bound");
        let ctrl_v = HotkeyBinding {
            ctrl: true,
            shift: false,
            alt: false,
            logo: false,
            primary: PrimaryKey::Char('v'),
        };
        assert!(!paste.contains(&ctrl_v), "Ctrl+V must stay with the PTY");
        // Shift+Insert is NOT a clipboard-paste chord: xterm binds it to
        // the PRIMARY selection, kitty ships it as `paste_from_selection`
        // and Alacritty as `PasteSelection`, so it belongs to the
        // separate `TerminalPasteSelection` action. Leaving it on plain
        // paste as well would double-fire the chord (the widget resolver
        // and the app router both matching it).
        let shift_ins = HotkeyBinding {
            ctrl: false,
            shift: true,
            alt: false,
            logo: false,
            primary: PrimaryKey::Named(Named::Insert),
        };
        assert!(!paste.contains(&shift_ins), "Shift+Insert belongs to PasteSelection");
        assert_eq!(paste.len(), 1);
        let paste_sel = defaults
            .get(&HotkeyAction::TerminalPasteSelection)
            .expect("paste-selection bound");
        assert!(paste_sel.contains(&shift_ins), "the xterm/kitty/Alacritty chord");
        // Plus the X11 middle click, the gesture's other half. It is a
        // chord in this list rather than a setting of its own, which is
        // what makes it rebindable in Settings > Shortcuts.
        assert!(
            paste_sel.contains(&middle_click_chord()),
            "middle click is a factory input for PRIMARY paste"
        );
        assert_eq!(paste_sel.len(), 2);

        // Shift+Insert paste has real pedigree (X11; PuTTY documents it).
        // A matching Ctrl+Insert copy does NOT: PuTTY's docs say copy is
        // mouse-only by default and GNOME Terminal documents only
        // Ctrl+Shift+C. Shipping it "for symmetry" would be inventing a
        // convention. It stays bindable, just not factory.
        let copy = defaults.get(&HotkeyAction::TerminalCopy).expect("copy bound");
        let ctrl_ins = HotkeyBinding {
            ctrl: true,
            shift: false,
            alt: false,
            logo: false,
            primary: PrimaryKey::Named(Named::Insert),
        };
        assert!(!copy.contains(&ctrl_ins), "Ctrl+Insert copy is not a real convention");
        assert_eq!(copy.len(), 1);

        // No factory chord anywhere is a bare Ctrl+letter that the shell
        // would want, which is what keeps the whole clipboard set clear
        // of the is_terminal_control_sequence gate.
        for action in [
            HotkeyAction::TerminalCopy,
            HotkeyAction::TerminalPaste,
            HotkeyAction::TerminalSelectAll,
            HotkeyAction::ScrollbackPageUp,
            HotkeyAction::ScrollbackPageDown,
        ] {
            for b in defaults.get(&action).expect("bound").iter() {
                assert!(
                    !b.is_terminal_control_sequence(),
                    "{action:?} chord {b:?} would be swallowed by the PTY gate"
                );
                assert!(b.is_safe(), "{action:?} chord {b:?} is not recordable");
            }
        }
    }

    /// Side buttons are free window-wide, so they bind to anything with
    /// an editable primary. The WHEEL CLICK is only ever read inside the
    /// canvas, so it stays on terminal actions: offering it elsewhere
    /// would record a binding that could never fire.
    #[test]
    fn side_buttons_bind_anywhere_the_wheel_click_does_not() {
        assert!(MouseButton::Back.is_side_button());
        assert!(MouseButton::Forward.is_side_button());
        assert!(MouseButton::Other(8).is_side_button());
        assert!(!MouseButton::Middle.is_side_button());

        for action in HotkeyAction::all() {
            assert_eq!(action.accepts_mouse(), action.primary_editable(), "{}", action.id());
            for side in [MouseButton::Back, MouseButton::Forward, MouseButton::Other(9)] {
                assert_eq!(
                    action.accepts_mouse_button(side),
                    action.primary_editable(),
                    "{} / {side:?}",
                    action.id()
                );
            }
            assert_eq!(
                action.accepts_mouse_button(MouseButton::Middle),
                action.primary_editable() && action.canvas_scoped(),
                "{}",
                action.id()
            );
            // The wheel follows the wheel click: both are only ever read
            // over the canvas.
            assert_eq!(
                action.accepts_wheel(),
                action.accepts_mouse_button(MouseButton::Middle),
                "{}",
                action.id()
            );
        }
        // Font zoom is the one non-terminal-only trio the canvas gestures
        // may carry: Ctrl+wheel zoom is what they exist for.
        assert!(HotkeyAction::FontZoomIn.accepts_wheel());
        assert!(HotkeyAction::FontZoomOut.accepts_wheel());
        assert!(HotkeyAction::FontZoomReset.accepts_wheel());
        assert!(!HotkeyAction::FontZoomIn.terminal_only());
        // A family action has no primary slot for a button at all.
        assert!(!HotkeyAction::SwitchToTabSlot.accepts_mouse());
        assert!(!HotkeyAction::SwitchToTabSlot.accepts_mouse_button(MouseButton::Back));
        // The two concrete cases the design turns on.
        assert!(HotkeyAction::TerminalPasteSelection.accepts_mouse_button(MouseButton::Middle));
        assert!(HotkeyAction::CloseActiveTab.accepts_mouse_button(MouseButton::Back));
        assert!(!HotkeyAction::CloseActiveTab.accepts_mouse_button(MouseButton::Middle));
    }

    /// Every factory mouse chord sits on an action that accepts one,
    /// and `mouse_chords` sees exactly those.
    #[test]
    fn factory_mouse_chords_are_on_mouse_capable_actions() {
        let defaults = default_bindings();
        let mut found = 0;
        for (action, binds) in defaults.iter() {
            for chord in binds.mouse_chords() {
                found += 1;
                let PrimaryKey::Mouse(button) = chord.primary else {
                    unreachable!("mouse_chords yielded a keyboard chord")
                };
                assert!(
                    action.accepts_mouse_button(button),
                    "{} ships a mouse chord it can never fire",
                    action.id()
                );
            }
        }
        assert_eq!(found, 1, "middle-click paste is the only factory gesture");
    }

    /// Every factory wheel chord sits on an action that accepts one,
    /// `wheel_chords` sees exactly those, and `mouse_chords` sees none
    /// of them (a wheel primary is not a button, and the resolver scans
    /// the two lists separately).
    #[test]
    fn factory_wheel_chords_are_ctrl_wheel_zoom() {
        let defaults = default_bindings();
        let mut found = Vec::new();
        for (action, binds) in defaults.iter() {
            for chord in binds.wheel_chords() {
                assert!(!chord.is_mouse());
                assert!(chord.is_safe(), "{} ships an unrecordable wheel chord", action.id());
                assert!(
                    action.accepts_wheel(),
                    "{} ships a wheel chord it can never fire",
                    action.id()
                );
                found.push((*action, *chord));
            }
            assert!(binds.mouse_chords().all(|b| !b.is_wheel()));
        }
        found.sort_by_key(|(a, _)| a.id());
        assert_eq!(
            found,
            vec![
                (HotkeyAction::FontZoomIn, super::super::wheel_zoom_chord(WheelDirection::Up)),
                (HotkeyAction::FontZoomOut, super::super::wheel_zoom_chord(WheelDirection::Down)),
            ]
        );
        // The key chords are still there beside the wheel: the toggle
        // removes the wheel half only.
        let zoom_in = defaults.get(&HotkeyAction::FontZoomIn).expect("bound");
        assert_eq!(zoom_in.len(), 2);
        assert!(zoom_in.iter().any(|b| b.primary == PrimaryKey::Punct("=")));
    }
}
