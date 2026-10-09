//! The action catalog: everything the user can bind a key or a mouse
//! button to, and the rules each action carries (which view it applies
//! in, whether its primary is editable, who dispatches its mouse form).
//!
//! Split out of the old single-file `hotkeys.rs`: this half answers
//! "what can be bound and under which rules", `binding.rs` answers
//! "what did the user press", `defaults.rs` "what ships bound".

use super::MouseButton;

/// Stable identifier for every editable action. Persisted to the
/// settings table as `hotkey_<snake_case_name>` so renames are
/// breaking changes; treat the variant order as append-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyAction {
    // Navigation / global pickers
    ShowNewTabPicker,
    ShowTabJump,
    /// Command palette (C4): fuzzy search over every action. Global,
    /// so no `terminal_only` / `vault_only` gate.
    ShowCommandPalette,
    OpenLocalShell,
    NewWindow,
    CloseActiveTab,
    OpenPortForwards,
    OpenSettings,
    FocusViewSearch,
    /// Open a new SFTP browser tab.
    OpenSftp,
    /// Open an interactive SFTP console (issue #188): a terminal tab
    /// running an `sftp(1)`-style prompt instead of a shell.
    ///
    /// Global rather than `terminal_only`. With a live SSH tab in front
    /// it opens a console on that host; anywhere else it opens the host
    /// picker, which is what `OpenSftp` already does. Gating it on the
    /// terminal view would leave its palette row visible and inert
    /// exactly where the host cards are.
    OpenSftpConsole,
    // Tab strip
    SwitchToTabSlot,   // family: Ctrl + digit 1..9
    CycleTabs,         // family: Alt + ArrowLeft/Right
    // Window
    ToggleFullscreen,
    // Font zoom (the three discrete keys; wheel zoom isn't editable)
    FontZoomIn,
    FontZoomOut,
    FontZoomReset,
    // Terminal split panes. These only fire while the terminal view is
    // focused (`terminal_only`); elsewhere the key is left free.
    SplitPaneVertical,
    SplitPaneHorizontal,
    FocusPaneLeft,
    FocusPaneRight,
    FocusPaneUp,
    FocusPaneDown,
    /// Expand the focused pane to the whole tab, and back. The layout is
    /// untouched while zoomed, so restoring puts every pane back exactly
    /// where it was.
    ToggleMaximizePane,
    /// Break the focused pane out into a tab of its own. Reachable
    /// without a mouse, and without the Menu right-click scheme that
    /// the pane's own context menu needs.
    MovePaneToNewTab,
    /// Ring the terminal-sidebar list rows (Snippets / History):
    /// opens the sidebar when closed, cycles the two list tabs on
    /// repeat. Terminal-only, like the split-pane family.
    FocusSidebarList,
    /// Open/close the terminal sidebar for the focused tab. With tabs
    /// docked to BOTH sides it drives the open region (else the right
    /// one); `ToggleSidebarOther` reaches the counterpart.
    ToggleSidebar,
    /// Hybrid tab (issue #61): flip the focused SSH tab between its
    /// terminal and its host's files (full SFTP surface).
    ToggleTabFiles,
    /// Broadcast input (C2): arm / disarm fan-out of keystrokes to every
    /// pane of the focused tab. Terminal-scoped; Ctrl+Shift+U by default.
    ToggleBroadcastInput,
    /// Jump to a vault section by position. Family: Ctrl+Shift +
    /// digit 1..8 (Hosts, Keychain, Snippets, Port Forwarding,
    /// Logs, Cloud Accounts, Proxies, Known Hosts); 9 is spare.
    VaultSectionSlot,
    // Vault-area section cycling (Hosts -> Keychain -> ... in sub-nav
    // order). Only fire in the vault area (`vault_only`); inside a
    // terminal tab the key is left free for TUI apps.
    VaultSectionPrev,
    VaultSectionNext,
    // Vault entity creation. Each opens its editor panel, navigating
    // to the owning vault section first (the panels only render
    // there). Appended per the order contract above.
    /// Open the new-host editor (Hosts section).
    NewHost,
    /// Open the key import panel (Keychain).
    NewKey,
    /// Open the new-identity panel (Keychain).
    NewIdentity,
    // Terminal clipboard + scrollback (#75). These were hard-coded
    // chords until they moved into this table, which is why they sit
    // at the end of the append-only order despite being among the
    // oldest behaviours in the app.
    /// Copy the terminal selection. Handled inside the terminal widget
    /// (it owns the selection), so the resolved chords are pushed down
    /// to it rather than dispatched here.
    TerminalCopy,
    /// Paste the clipboard into the focused pane. Handled in the
    /// dispatcher, which is the only layer that can reach an SSH
    /// session.
    TerminalPaste,
    /// Paste the X11 PRIMARY selection (the text of the last completed
    /// selection, remembered independently of the highlight) into the
    /// focused pane: the keyboard twin of middle-click. Widget-side like
    /// `TerminalCopy`, because PRIMARY lives in the canvas state; the
    /// widget hands the text over and the dispatcher pastes it.
    /// Factory inputs: Shift+Insert (the xterm / kitty / Alacritty
    /// convention) and the middle mouse button.
    TerminalPasteSelection,
    /// Select the whole terminal buffer. Widget-side, like `TerminalCopy`.
    TerminalSelectAll,
    /// Page the scrollback up. Widget-side (it owns `scroll_offset`).
    ScrollbackPageUp,
    /// Page the scrollback down. Widget-side, like `ScrollbackPageUp`.
    ScrollbackPageDown,
    /// Privacy Mode session override (issue #78): flip a volatile
    /// forced-on/off state that sits above the global setting AND the
    /// per-host overrides, for "I'm about to share my screen" moments.
    /// Never persisted. Global (not `terminal_only`): the vault
    /// surfaces mask too.
    TogglePrivacyMode,
    /// Ad-hoc quick connect (issue #99): open the "+ Host" editor
    /// empty, where "Connect without saving" runs a session that is
    /// never persisted. Global: unlike `NewHost`'s bare Ctrl+N this
    /// ships with a Shift chord, so it also fires inside a terminal.
    ShowQuickConnect,
    /// Reconnect the focused tab's session: the tab context menu's
    /// "Reconnect" entry on a chord. Works on live tabs too (a
    /// "restart this host"), same handler either way. Terminal-only:
    /// there is no focused tab to reconnect anywhere else.
    ReconnectTab,
    /// Open a second session like the focused tab's: the tab context
    /// menu's "Duplicate Tab" entry on a chord, same handler (so the
    /// copy lands where `duplicate_tab_position` says). Terminal-only,
    /// like `ReconnectTab`: there is no focused tab to copy elsewhere.
    DuplicateTab,
    /// Open/close the OTHER sidebar region: the counterpart of
    /// whatever `ToggleSidebar` targets right now. Only meaningful
    /// with tabs docked to both sides (issue #102); no-op otherwise,
    /// since the primary key already reaches a lone region.
    ToggleSidebarOther,
    /// Bring back the last closed tab (issue #186), terminal or SFTP.
    /// Global rather than `terminal_only`: the moment it is wanted most
    /// is right after closing the last tab, which lands on Home.
    ReopenClosedTab,
    // Menu verbs that ship UNBOUND: no factory chord, absent from
    // `default_bindings()` (absence IS the representation, never an
    // empty list there). The Shortcuts editor offers each as a row
    // whose only chip reads "(unbound)", so the chord is the user's
    // choice, and the command palette lists them like any other action.
    // Nothing ships bound because no terminal or browser agrees on a
    // chord for any of these (Chrome, Firefox, VS Code and kitty leave
    // them to the menu), and a two-key chord on the Ctrl+Shift row fires
    // by accident next to N / W (Konsole dropped its Ctrl+Shift+L
    // detach-tab chord for exactly that in 2020). `ships_unbound` names
    // the set; a test pins it against the factory table.
    /// Move the focused tab into a window of its own: the tab menu's
    /// "Move to New Window". Global, not `terminal_only`: an SFTP tab
    /// moves too, and the terminal gate would never fire there.
    MoveTabToNewWindow,
    /// Open a second session like the focused tab's, in a new window.
    DuplicateInNewWindow,
    /// Rename the focused tab (terminal or SFTP) inline on its chip.
    RenameTab,
    /// Pin / unpin the focused tab (terminal or SFTP).
    ToggleTabPin,
    /// Copy the focused pane's host address to the clipboard.
    CopyTabAddress,
    /// Close every other tab of this window, through the same confirm
    /// the tab menu's entry raises.
    CloseOtherTabs,
    /// Close every tab of this window, through its confirm.
    CloseAllTabs,
    /// Copy the focused pane's whole buffer (scrollback + screen).
    /// App-dispatched, unlike `TerminalCopy`: the handler reads the
    /// pane's buffer under its lock, so the palette can run it.
    TerminalCopyAll,
    /// Copy the focused pane's displayed screen only.
    TerminalCopyScreen,
    /// Drop the focused pane's scrollback.
    TerminalClearScrollback,
    /// Lock the vault, through its confirm (the burger menu's row). The
    /// palette used to carry this as a hand-written extra; it is an
    /// ordinary action now so a chord can reach it.
    LockVault,
}

impl HotkeyAction {
    /// All actions in display order. Used by the Settings panel to
    /// iterate without forgetting one.
    pub fn all() -> &'static [HotkeyAction] {
        use HotkeyAction::*;
        &[
            ShowNewTabPicker,
            ShowTabJump,
            ShowCommandPalette,
            OpenLocalShell,
            NewWindow,
            NewHost,
            ShowQuickConnect,
            NewKey,
            NewIdentity,
            ReconnectTab,
            DuplicateTab,
            DuplicateInNewWindow,
            MoveTabToNewWindow,
            RenameTab,
            ToggleTabPin,
            CopyTabAddress,
            CloseActiveTab,
            CloseOtherTabs,
            CloseAllTabs,
            OpenPortForwards,
            OpenSettings,
            FocusViewSearch,
            OpenSftp,
            OpenSftpConsole,
            SwitchToTabSlot,
            CycleTabs,
            ToggleFullscreen,
            FontZoomIn,
            FontZoomOut,
            FontZoomReset,
            SplitPaneVertical,
            SplitPaneHorizontal,
            FocusPaneLeft,
            FocusPaneRight,
            FocusPaneUp,
            FocusPaneDown,
            ToggleMaximizePane,
            MovePaneToNewTab,
            FocusSidebarList,
            ToggleSidebar,
            ToggleSidebarOther,
            ToggleTabFiles,
            ToggleBroadcastInput,
            TogglePrivacyMode,
            TerminalCopy,
            TerminalPaste,
            TerminalPasteSelection,
            TerminalSelectAll,
            ScrollbackPageUp,
            ScrollbackPageDown,
            TerminalCopyAll,
            TerminalCopyScreen,
            TerminalClearScrollback,
            VaultSectionSlot,
            VaultSectionPrev,
            VaultSectionNext,
            ReopenClosedTab,
            LockVault,
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
            ShowCommandPalette => "show_command_palette",
            OpenLocalShell => "open_local_shell",
            NewWindow => "new_window",
            CloseActiveTab => "close_active_tab",
            OpenPortForwards => "open_port_forwards",
            OpenSettings => "open_settings",
            FocusViewSearch => "focus_view_search",
            OpenSftp => "open_sftp",
            OpenSftpConsole => "open_sftp_console",
            SwitchToTabSlot => "switch_to_tab_slot",
            CycleTabs => "cycle_tabs",
            ToggleFullscreen => "toggle_fullscreen",
            FontZoomIn => "font_zoom_in",
            FontZoomOut => "font_zoom_out",
            FontZoomReset => "font_zoom_reset",
            SplitPaneVertical => "split_pane_vertical",
            SplitPaneHorizontal => "split_pane_horizontal",
            FocusPaneLeft => "focus_pane_left",
            FocusPaneRight => "focus_pane_right",
            FocusPaneUp => "focus_pane_up",
            FocusPaneDown => "focus_pane_down",
            ToggleMaximizePane => "toggle_maximize_pane",
            MovePaneToNewTab => "move_pane_to_new_tab",
            FocusSidebarList => "focus_sidebar_list",
            ToggleSidebar => "toggle_sidebar",
            ToggleSidebarOther => "toggle_sidebar_other",
            ToggleTabFiles => "toggle_tab_files",
            ToggleBroadcastInput => "toggle_broadcast_input",
            TogglePrivacyMode => "toggle_privacy_mode",
            VaultSectionSlot => "vault_section_slot",
            VaultSectionPrev => "vault_section_prev",
            VaultSectionNext => "vault_section_next",
            NewHost => "new_host",
            ShowQuickConnect => "show_quick_connect",
            ReconnectTab => "reconnect_tab",
            DuplicateTab => "duplicate_tab",
            NewKey => "new_key",
            NewIdentity => "new_identity",
            TerminalCopy => "terminal_copy",
            TerminalPaste => "terminal_paste",
            TerminalPasteSelection => "terminal_paste_selection",
            TerminalSelectAll => "terminal_select_all",
            ScrollbackPageUp => "scrollback_page_up",
            ScrollbackPageDown => "scrollback_page_down",
            ReopenClosedTab => "reopen_closed_tab",
            MoveTabToNewWindow => "move_tab_to_new_window",
            DuplicateInNewWindow => "duplicate_in_new_window",
            RenameTab => "rename_tab",
            ToggleTabPin => "toggle_tab_pin",
            CopyTabAddress => "copy_tab_address",
            CloseOtherTabs => "close_other_tabs",
            CloseAllTabs => "close_all_tabs",
            TerminalCopyAll => "terminal_copy_all",
            TerminalCopyScreen => "terminal_copy_screen",
            TerminalClearScrollback => "terminal_clear_scrollback",
            LockVault => "lock_vault",
        }
    }

    /// i18n key for the action's display label.
    pub fn label_key(self) -> &'static str {
        use HotkeyAction::*;
        match self {
            ShowNewTabPicker => "hotkey_show_new_tab_picker",
            ShowTabJump => "hotkey_show_tab_jump",
            ShowCommandPalette => "hotkey_show_command_palette",
            OpenLocalShell => "hotkey_open_local_shell",
            NewWindow => "hotkey_new_window",
            CloseActiveTab => "hotkey_close_active_tab",
            OpenPortForwards => "hotkey_open_port_forwards",
            OpenSettings => "hotkey_open_settings",
            FocusViewSearch => "hotkey_focus_view_search",
            OpenSftp => "hotkey_open_sftp",
            OpenSftpConsole => "hotkey_open_sftp_console",
            SwitchToTabSlot => "hotkey_switch_to_tab_slot",
            CycleTabs => "hotkey_cycle_tabs",
            ToggleFullscreen => "hotkey_toggle_fullscreen",
            FontZoomIn => "hotkey_font_zoom_in",
            FontZoomOut => "hotkey_font_zoom_out",
            FontZoomReset => "hotkey_font_zoom_reset",
            // Reuse the context-menu split labels (already translated in
            // all 17 languages) rather than minting parallel keys.
            SplitPaneVertical => "split_side_by_side",
            SplitPaneHorizontal => "split_stacked",
            FocusPaneLeft => "hotkey_focus_pane_left",
            FocusPaneRight => "hotkey_focus_pane_right",
            FocusPaneUp => "hotkey_focus_pane_up",
            FocusPaneDown => "hotkey_focus_pane_down",
            ToggleMaximizePane => "hotkey_toggle_maximize_pane",
            // Reuses the menu row's own sentence rather than minting a
            // parallel key, the way the split actions reuse theirs.
            MovePaneToNewTab => "pane_to_new_tab",
            FocusSidebarList => "hotkey_focus_sidebar_list",
            ToggleSidebar => "hotkey_toggle_sidebar",
            ToggleSidebarOther => "hotkey_toggle_sidebar_other",
            ToggleTabFiles => "hotkey_toggle_tab_files",
            ToggleBroadcastInput => "hotkey_toggle_broadcast_input",
            TogglePrivacyMode => "hotkey_toggle_privacy_mode",
            VaultSectionSlot => "hotkey_vault_section_slot",
            VaultSectionPrev => "hotkey_vault_section_prev",
            VaultSectionNext => "hotkey_vault_section_next",
            // Reuse the vault-area button labels (already translated
            // in all 17 languages), same as the split-pane pair.
            NewHost => "new_host",
            ShowQuickConnect => "quick_connect",
            // Reuses the tab context menu's entry label, same pattern.
            ReconnectTab => "reconnect",
            DuplicateTab => "duplicate_tab",
            NewKey => "import_key",
            NewIdentity => "new_identity",
            // Reuse the terminal context-menu labels (already
            // translated in all 23 languages) rather than minting
            // parallel keys, same as the split-pane pair above.
            TerminalCopy => "terminal_copy",
            TerminalPaste => "terminal_paste",
            TerminalPasteSelection => "hotkey_terminal_paste_selection",
            TerminalSelectAll => "select_all",
            ScrollbackPageUp => "hotkey_scrollback_page_up",
            ScrollbackPageDown => "hotkey_scrollback_page_down",
            // Reuses the tab context menu's entry label, same pattern as
            // `ReconnectTab` above.
            ReopenClosedTab => "reopen_closed_tab",
            // Own keys: the menu rows say "Move to New Window" and
            // "Pin tab" / "Unpin tab", which read wrong in a flat list
            // of actions (move WHAT; a toggle has one row).
            MoveTabToNewWindow => "hotkey_move_tab_to_new_window",
            ToggleTabPin => "hotkey_toggle_tab_pin",
            // The rest reuse their menu row's label, same pattern as
            // `ReconnectTab`.
            DuplicateInNewWindow => "duplicate_new_window",
            RenameTab => "rename_tab",
            CopyTabAddress => "copy_host_address",
            CloseOtherTabs => "close_other_tabs",
            CloseAllTabs => "close_all_tabs",
            TerminalCopyAll => "terminal_copy_all",
            TerminalCopyScreen => "terminal_copy_screen",
            TerminalClearScrollback => "terminal_clear_scrollback",
            LockVault => "lock_vault",
        }
    }

    /// Whether the action ships with NO factory chord: offered in the
    /// Shortcuts editor and the palette, bound only by the user. The
    /// one list the factory table is checked against
    /// (`defaults::tests::every_action_ships_bound_or_declares_unbound`),
    /// so an action cannot be left out of `default_bindings()` by
    /// accident, and one declared here cannot quietly grow a chord.
    pub fn ships_unbound(self) -> bool {
        use HotkeyAction::*;
        matches!(
            self,
            MoveTabToNewWindow
                | DuplicateInNewWindow
                | RenameTab
                | ToggleTabPin
                | CopyTabAddress
                | CloseOtherTabs
                | CloseAllTabs
                | TerminalCopyAll
                | TerminalCopyScreen
                | TerminalClearScrollback
                | LockVault
        )
    }

    /// Whether the action only applies while the terminal view is
    /// focused. The dispatch loop skips these elsewhere so the key
    /// stays free in other views (and doesn't swallow the event).
    pub fn terminal_only(self) -> bool {
        use HotkeyAction::*;
        matches!(
            self,
            SplitPaneVertical
                | SplitPaneHorizontal
                | FocusPaneLeft
                | FocusPaneRight
                | FocusPaneUp
                | FocusPaneDown
                | ToggleMaximizePane
                | MovePaneToNewTab
                | FocusSidebarList
                | ToggleSidebar
                | ToggleSidebarOther
                | ToggleTabFiles
                | ToggleBroadcastInput
                | ReconnectTab
                | DuplicateTab
                | TerminalCopy
                | TerminalPaste
                | TerminalPasteSelection
                | TerminalSelectAll
                | ScrollbackPageUp
                | ScrollbackPageDown
                | DuplicateInNewWindow
                | CopyTabAddress
                | TerminalCopyAll
                | TerminalCopyScreen
                | TerminalClearScrollback
        )
    }

    /// Actions the terminal WIDGET performs from its own canvas state
    /// (selection, scroll offset), which `dispatch_hotkey_action` only
    /// swallows. They fire from a real keystroke reaching the widget,
    /// never from a `RunHotkeyAction` message, so the command palette
    /// (which dispatches the message) must not list them: a click would
    /// silently do nothing.
    pub fn widget_dispatched(self) -> bool {
        use HotkeyAction::*;
        matches!(
            self,
            TerminalCopy
                | TerminalPasteSelection
                | TerminalSelectAll
                | ScrollbackPageUp
                | ScrollbackPageDown
        )
    }

    /// Whether the action only applies in the vault area (Home and
    /// its sub-sections). The dispatch loop skips these elsewhere,
    /// leaving the key free: Ctrl+PageUp/Down inside a terminal tab
    /// belongs to the TUI running there, not to Oryxis.
    pub fn vault_only(self) -> bool {
        matches!(
            self,
            HotkeyAction::VaultSectionPrev | HotkeyAction::VaultSectionNext
        )
    }

    /// Whether the primary key (suffix) is editable. Family actions
    /// are modifier-only; everything else accepts any single primary.
    pub fn primary_editable(self) -> bool {
        !matches!(
            self,
            HotkeyAction::SwitchToTabSlot
                | HotkeyAction::CycleTabs
                | HotkeyAction::VaultSectionSlot
        )
    }

    /// Whether ANY mouse button may be bound to this action, which is
    /// what the Shortcuts chip's placeholder announces.
    ///
    /// A family action edits its modifiers only, so it has no primary
    /// slot for a button to occupy; everything else takes at least the
    /// side buttons.
    pub fn accepts_mouse(self) -> bool {
        self.primary_editable()
    }

    /// Whether THIS button may be bound to this action.
    ///
    /// Side buttons are free window-wide (see
    /// [`MouseButton::is_side_button`]), so they carry any action. The
    /// wheel click is only ever read inside the terminal canvas, so it
    /// stays on the actions that belong there (`canvas_scoped`).
    pub fn accepts_mouse_button(self, button: MouseButton) -> bool {
        self.accepts_mouse() && (button.is_side_button() || self.canvas_scoped())
    }

    /// Whether a wheel direction may be bound to this action.
    ///
    /// The wheel is only ever read over the terminal canvas (the app has
    /// no window-wide wheel path, and must not grow one: a Ctrl+wheel
    /// over a list is that list's), so it takes the same set as the
    /// wheel click.
    pub fn accepts_wheel(self) -> bool {
        self.accepts_mouse() && self.canvas_scoped()
    }

    /// The actions a gesture that exists ONLY over the terminal canvas
    /// (the wheel click, the wheel) may carry: the terminal-only set,
    /// plus the font zoom trio, which fires anywhere by key (the
    /// Settings preview follows it) but is what the canvas's own
    /// gestures are for. An action outside this set bound to such a
    /// gesture would read as a dead binding on every other screen.
    pub fn canvas_scoped(self) -> bool {
        self.terminal_only()
            || matches!(self, Self::FontZoomIn | Self::FontZoomOut | Self::FontZoomReset)
    }

    /// Which layer runs a mouse binding on this action.
    ///
    /// The single authority for the split, called by BOTH sides
    /// (`views::terminal::terminal_mouse_resolver` and
    /// `shortcuts::dispatch_mouse_binding`) precisely so they cannot
    /// drift: the two see the same press, so a pair claimed twice fires
    /// twice and a pair claimed by neither is a dead button.
    pub fn mouse_binding_owner(self, button: MouseButton) -> MouseBindingOwner {
        if self.widget_dispatched() || !button.is_side_button() {
            // The five canvas-state gestures can only run in the widget,
            // whatever the button; and the wheel click is only readable
            // over the canvas in the first place.
            MouseBindingOwner::Widget
        } else {
            // A side button is free window-wide, so the app runs it and
            // the gesture works outside a terminal too.
            MouseBindingOwner::App
        }
    }
}

/// Which layer performs a mouse binding. See
/// [`HotkeyAction::mouse_binding_owner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseBindingOwner {
    /// The terminal widget, over its own canvas.
    Widget,
    /// The app's global press handler, anywhere in the window.
    App,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every action must be reachable from the editor, and every stored
    /// row keyed by a stable id. A new action that forgets `all()` is
    /// invisible in Settings and the palette; one that forgets `id()`
    /// panics. Cheap guard, since both are hand-maintained tables.
    #[test]
    fn every_action_is_listed_and_has_a_unique_id() {
        let all = HotkeyAction::all();
        let mut ids: Vec<&str> = all.iter().map(|a| a.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate hotkey id");
        for a in [
            HotkeyAction::TerminalCopy,
            HotkeyAction::TerminalPaste,
            HotkeyAction::TerminalPasteSelection,
            HotkeyAction::TerminalSelectAll,
            HotkeyAction::ScrollbackPageUp,
            HotkeyAction::ScrollbackPageDown,
        ] {
            assert!(all.contains(&a), "{a:?} missing from all()");
            assert!(a.terminal_only(), "{a:?} must not fire outside a terminal");
        }
    }

    #[test]
    fn paste_selection_is_widget_dispatched_and_listed() {
        assert!(HotkeyAction::all().contains(&HotkeyAction::TerminalPasteSelection));
        // Performed by the widget (PRIMARY lives in the canvas state), so
        // the command palette must not list it: a palette row dispatches
        // `RunHotkeyAction`, which never reaches the canvas, and the row
        // would silently do nothing.
        assert!(HotkeyAction::TerminalPasteSelection.widget_dispatched());
        // Paste, by contrast, IS dispatched app-side (it has to reach an
        // SSH session), so it stays clickable in the palette. Guards the
        // pair against being lumped together by a later edit.
        assert!(!HotkeyAction::TerminalPaste.widget_dispatched());
    }

    /// Exactly one layer claims each (action, button) pair. Both the
    /// widget resolver and the global press handler gate on this, so a
    /// pair claimed twice would fire twice and a pair claimed by neither
    /// would be a dead button.
    #[test]
    fn every_mouse_binding_has_exactly_one_owner() {
        use MouseBindingOwner::*;
        for action in HotkeyAction::all() {
            for button in [
                MouseButton::Middle,
                MouseButton::Back,
                MouseButton::Forward,
                MouseButton::Other(8),
            ] {
                let owner = action.mouse_binding_owner(button);
                // The canvas-state gestures are never the app's, at any
                // button: `RunHotkeyAction` only swallows them.
                if action.widget_dispatched() {
                    assert_eq!(owner, Widget, "{} / {button:?}", action.id());
                }
                // The wheel click is only ever readable over the canvas.
                if !button.is_side_button() {
                    assert_eq!(owner, Widget, "{} / {button:?}", action.id());
                }
                // A side button on a non-canvas action must reach the
                // app, or binding one outside the terminal is a no-op.
                if button.is_side_button() && !action.widget_dispatched() {
                    assert_eq!(owner, App, "{} / {button:?}", action.id());
                }
            }
        }
        // The case the whole split exists for: Back closes a tab from
        // anywhere, and it is the app that runs it.
        assert_eq!(
            HotkeyAction::CloseActiveTab.mouse_binding_owner(MouseButton::Back),
            App
        );
        assert_eq!(
            HotkeyAction::TerminalCopy.mouse_binding_owner(MouseButton::Back),
            Widget
        );
    }
}
