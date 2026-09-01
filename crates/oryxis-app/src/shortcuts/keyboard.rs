//! The keyboard router: what a keypress means, and what it fires.
//!
//! Three stages, in this order: capture mode (the Settings > Shortcuts
//! editor is recording a chord), the binding table, and the action
//! dispatch. The view gates live with the dispatch rather than the
//! table, so an action that does not apply on the current screen leaves
//! its key free instead of consuming it.

use iced::keyboard::{key::Named, Key, Modifiers};
use iced::Task;

use crate::app::{SftpMessage, SettingsMessage, TabsMessage, EditorMessage, KeysMessage, TerminalMessage, NavigationMessage, SnippetMessage, AiMessage, Message, Oryxis};
use crate::hotkeys::{FamilyMatch, HotkeyAction};
use crate::state::View;

impl Oryxis {
    /// Main entry point for `dispatch_terminal::|v| Message::Terminal(TerminalMessage::KeyboardEvent(v))`.
    /// Returns `Some(task)` when the event was consumed (by capture
    /// mode, a binding match, or the Esc-closes-modal fallback), or
    /// `None` to let the caller fall through to PTY routing.
    pub(crate) fn handle_hotkey_keypress(
        &mut self,
        key: &Key,
        modifiers: &Modifiers,
    ) -> Option<Task<Message>> {
        // 1. Capture mode for the Settings → Shortcuts editor wins
        //    over everything: Esc cancels, anything else (modulo
        //    pure-modifier presses) becomes the new binding. Belt
        //    and suspenders: capture only fires when the user is
        //    still on the Shortcuts editor, navigating away cancels
        //    the pending capture so the next keystroke doesn't
        //    silently rebind something on another screen.
        if self.editing_hotkey.is_some() {
            let on_shortcuts_editor = self.active_view == View::Settings
                && self.settings_section == crate::state::SettingsSection::Shortcuts;
            if !on_shortcuts_editor {
                self.editing_hotkey = None;
            } else if let Some(task) = self.handle_hotkey_capture(key, modifiers) {
                return Some(task);
            }
        }

        // 1.5. Snippet-shortcut recorder (armed from either snippet
        //      editor). The next chord becomes the snippet's custom
        //      run hotkey; Esc cancels. Guarded on the editor being
        //      open so a stale flag can't eat keys elsewhere.
        if self.snippet_form.hotkey_capturing {
            if !self.panels.snippet_panel {
                self.snippet_form.hotkey_capturing = false;
            } else {
                if matches!(key, Key::Named(Named::Escape)) {
                    self.snippet_form.hotkey_capturing = false;
                    return Some(Task::none());
                }
                if matches!(
                    key,
                    Key::Named(
                        Named::Control | Named::Shift | Named::Alt | Named::Super | Named::Meta
                    )
                ) {
                    // Mid-chord modifier press; keep waiting.
                    return Some(Task::none());
                }
                let Some(binding) = crate::hotkeys::binding_from_event(key, modifiers, true)
                else {
                    self.set_toast(crate::i18n::t("hotkey_must_have_modifier").to_string());
                    return Some(super::toast_clear_after_secs(2));
                };
                // Plain Ctrl+letter belongs to the shell; a snippet
                // hotkey only ever fires inside a terminal, so binding
                // one would shadow readline/SIGINT keys.
                if binding.is_terminal_control_sequence() {
                    self.set_toast(crate::i18n::t("snippet_hotkey_reserved").to_string());
                    return Some(super::toast_clear_after_secs(2));
                }
                // Conflicts: the static table and other snippets.
                let in_table = self.hotkey_bindings.values().any(|b| b.contains(&binding));
                let in_snippets = self.snippets.iter().any(|sn| {
                    self.snippet_form.editing_id != Some(sn.id)
                        && sn.hotkey.as_deref()
                            == Some(binding.serialize()).as_deref()
                });
                if in_table || in_snippets {
                    self.set_toast(crate::i18n::t("snippet_hotkey_in_use").to_string());
                    return Some(super::toast_clear_after_secs(2));
                }
                self.snippet_form.hotkey = Some(binding);
                self.snippet_form.hotkey_capturing = false;
                return Some(Task::none());
            }
        }

        // 2. Binding-table dispatch. First match wins. When the
        //    terminal view is focused, any binding shaped like a
        //    shell control sequence (Ctrl+letter with no other
        //    modifier) is skipped so Ctrl+L/Ctrl+P/Ctrl+K/etc. reach
        //    the PTY. The gate is computed from the CURRENT binding,
        //    so a user who rebinds CloseActiveTab onto a shell key
        //    loses the rebound action in the terminal (but it still
        //    fires elsewhere), and rebinding an old gated action OFF
        //    a shell key restores it everywhere. Iterates over the
        //    'static slice directly; HotkeyBinding is Copy, so we
        //    materialise it before calling dispatch_hotkey_action
        //    (which takes &mut self) and avoid the per-press
        //    allocation that the prior `.to_vec()` paid.
        //
        //    "In a terminal" means A TERMINAL TAB IS FOCUSED, not
        //    `active_view == Terminal`: in workspace mode a focused
        //    terminal runs under the Dashboard view (the PTY key
        //    routing in dispatch_terminal.rs already goes by
        //    `active_tab` for the same reason). Field bug 2026-07-03:
        //    every terminal_only hotkey (FocusSidebarList, splits,
        //    pane focus) was dead on tabs opened under the workspace,
        //    while the same chord worked on a View::Terminal tab of
        //    the same build. `active_tab` is cleared on every
        //    navigation into the vault / settings / SFTP surfaces, so
        //    it is exactly the "keys route to a PTY" signal.
        let in_terminal = self.active_view == View::Terminal || self.active_tab.is_some();
        // Whether the PTY actually owns plain control sequences right
        // now: a hybrid tab in Files mode hides the terminal and gates
        // its byte routing off, so Ctrl+letter bindings (Ctrl+F search)
        // may fire there, exactly like on the standalone SFTP view. The
        // `terminal_only` skip keeps using `in_terminal` so the toggle
        // hotkey itself still works from Files mode.
        let pty_owns_keys = in_terminal
            && !self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .is_some_and(|t| t.files_mode);
        // The scrollback find-bar (Ctrl+F) may only steal the key on the
        // NORMAL screen. On the alternate screen (vim / less / htop / tmux)
        // Ctrl+F is the app's own page-forward, and there is no scrollback
        // to search anyway (the widget pins scroll_offset=0 there), so let
        // it fall through to the PTY like every other shell control key.
        let alt_screen = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.active())
            .and_then(|p| p.terminal.lock().ok().map(|s| s.is_alt_screen()))
            .unwrap_or(false);
        // A blocking modal owns the keyboard: only Esc may pass (step 3
        // below closes the modal). Skip binding-table and snippet dispatch
        // so chords like ClosePane / SplitPane / the host-editor hotkey
        // cannot fire on the surface hidden behind the modal. The modal's
        // own keyboard navigation runs earlier in the router
        // (`handle_modal_nav_key`), so movement / activation keys are
        // unaffected.
        let modal_owns_keys = self.any_modal_blocks_input();
        // The new-tab picker is the one blocking modal that PRINTS
        // chords on its own rows (Local Shell / SFTP carry the same hint
        // the burger menu shows). A hint for a key the surface then
        // swallows is worse than no hint, so those two fire while it is
        // open. Scoped to exactly what the picker offers: it hides the
        // SFTP row while a split pane is pending (SFTP is a tab, never a
        // pane), so the chord stays blocked there too.
        let picker_exempt = |a: HotkeyAction| {
            self.panels.new_tab_picker
                && match a {
                    HotkeyAction::OpenLocalShell => true,
                    HotkeyAction::OpenSftp => self.pending_pane_split.is_none(),
                    _ => false,
                }
        };
        // Resolve the hit under an immutable borrow of the binding
        // table, then dispatch once the borrow is gone:
        // `dispatch_hotkey_action` takes `&mut self`. The old code
        // copied the binding out per action to the same end, which
        // stopped being free when a binding became a list; matching
        // in place keeps the loop allocation-free per keypress.
        let mut hit: Option<(HotkeyAction, FamilyMatch)> = None;
        for &action in HotkeyAction::all() {
            // `continue`, not `break`: an exempt action can sit anywhere
            // in the table, so the scan has to reach it.
            if modal_owns_keys && !picker_exempt(action) {
                continue;
            }
            // Split-pane actions only apply inside the terminal view.
            // Skipping (not consuming) elsewhere leaves their key free
            // in other views and avoids a confusing no-op.
            if action.terminal_only() && !in_terminal {
                continue;
            }
            // Vault section cycling only applies in the vault area.
            // Skipping (not consuming) leaves Ctrl+PageUp/Down to the
            // PTY inside a terminal tab, where TUIs use it.
            if action.vault_only() && !self.in_vault_area() {
                continue;
            }
            let Some(binds) = self.hotkey_bindings.get(&action) else {
                continue;
            };
            // Plain Ctrl+letter bindings normally yield to the PTY (shell
            // control sequences: Ctrl+L clear, Ctrl+R history, ...). The
            // scrollback find-bar (Ctrl+F) is the deliberate exception on
            // the NORMAL screen: like every GUI terminal, Ctrl+F opens Find
            // over the buffer instead of reaching readline's forward-char
            // (arrow keys cover that). On the alternate screen it yields to
            // the PTY so vim / less / htop keep their own Ctrl+F.
            let find_bar_exempt =
                action == HotkeyAction::FocusViewSearch && !alt_screen;
            // Scrollback paging yields the WHOLE action on the alternate
            // screen: there is no scrollback there (the widget pins
            // scroll_offset to 0), so vim / less / htop own PageUp and
            // page themselves. It needs this explicit gate because the
            // control-sequence one below can never catch it: Shift+PageUp
            // carries no Ctrl, so `is_terminal_control_sequence` is
            // always false for it. The widget self-gates on the same
            // signal; the two must agree or the key is either eaten twice
            // or not at all.
            if alt_screen
                && matches!(
                    action,
                    HotkeyAction::ScrollbackPageUp | HotkeyAction::ScrollbackPageDown
                )
            {
                continue;
            }
            // Per CHORD, not per action: an action carrying both a
            // Ctrl+Shift chord and a bare Ctrl+letter one keeps the
            // former here and yields only the latter to the shell.
            let found = binds.match_event_where(key, modifiers, |b| {
                !(pty_owns_keys && !find_bar_exempt && b.is_terminal_control_sequence())
            });
            if let Some(family) = found {
                hit = Some((action, family));
                break;
            }
        }
        if let Some((action, family)) = hit {
            tracing::debug!(action = action.id(), "hotkey matched");
            return Some(self.dispatch_hotkey_action(action, family));
        }

        // 2.5. Per-snippet custom hotkeys, derived LIVE from the vault
        //      list (no side registry: deleting a snippet deletes its
        //      shortcut by construction). Terminal-focused only, since
        //      the action types into the focused session; a hybrid tab
        //      in Files mode gates PTY writes off, so firing here would
        //      just dead-end (worst case through the vars modal).
        if !modal_owns_keys && pty_owns_keys && !self.panels.snippet_panel {
            let hit = self.snippets.iter().position(|sn| {
                sn.hotkey
                    .as_deref()
                    .and_then(crate::hotkeys::HotkeyBinding::parse)
                    .is_some_and(|b| b.match_event(key, modifiers).is_some())
            });
            if let Some(idx) = hit {
                return Some(self.update(Message::Snippet(SnippetMessage::RunSnippet(idx))));
            }
        }

        // 3. Esc closes the topmost open modal as a fallback. Only
        //    fires when nothing else above claimed it, so terminal
        //    apps that rely on raw Esc (vim, less) keep getting the
        //    byte when no modal is open. The close itself decides the
        //    follow-up task (an answer-bearing modal routes its safe
        //    default through its real handler).
        if matches!(key, Key::Named(Named::Escape))
            && let Some(task) = self.close_topmost_modal()
        {
            return Some(task);
        }

        None
    }

    /// Capture-mode branch of `handle_hotkey_keypress`. Esc cancels;
    /// Delete / Backspace drop the chord being edited; pure-modifier
    /// presses are ignored (they fire `KeyPressed` too); anything else
    /// becomes the new chord (validated by
    /// `binding_from_event::is_safe`). A conflict with another action
    /// takes the chord from the loser and surfaces a toast naming it.
    fn handle_hotkey_capture(
        &mut self,
        key: &Key,
        modifiers: &Modifiers,
    ) -> Option<Task<Message>> {
        let (action, slot) = self.editing_hotkey?;
        // Esc cancels without saving.
        if matches!(key, Key::Named(Named::Escape)) {
            self.editing_hotkey = None;
            return Some(Task::none());
        }
        // BARE Delete / Backspace remove the chord under edit. Neither
        // is bindable on its own (`is_safe` only clears a modifier-free
        // primary for function keys), so neither can be the chord the
        // user meant to record, which is what leaves them free to mean
        // "remove" here. Shift is excluded along with the rest:
        // `Shift+Delete` IS bindable (shift + non-text primary), so
        // swallowing it here would make it unrecordable.
        if matches!(key, Key::Named(Named::Delete | Named::Backspace))
            && !modifiers.control()
            && !modifiers.shift()
            && !modifiers.alt()
            && !modifiers.logo()
        {
            self.editing_hotkey = None;
            let crate::hotkeys::HotkeySlot::Replace(i) = slot else {
                // Nothing recorded yet in an Add slot: the remove is
                // just a cancel.
                return Some(Task::none());
            };
            let mut binds = self.hotkey_bindings.get(&action).cloned().unwrap_or_default();
            let Some(chord) = binds.iter().nth(i).copied() else {
                return Some(Task::none());
            };
            binds.remove(&chord);
            self.persist_setting(&format!("hotkey_{}", action.id()), &binds.serialize());
            if binds.is_empty() {
                self.hotkey_bindings.remove(&action);
            } else {
                self.hotkey_bindings.insert(action, binds);
            }
            return Some(Task::none());
        }
        // Pure-modifier KeyPressed (Ctrl alone, Shift alone, ...)
        // shouldn't terminate the capture: the user is mid-way to
        // pressing the full chord.
        if matches!(
            key,
            Key::Named(
                Named::Control
                    | Named::Shift
                    | Named::Alt
                    | Named::Super
                    | Named::Meta
            )
        ) {
            return Some(Task::none());
        }

        let primary_editable = action.primary_editable();
        let captured = crate::hotkeys::binding_from_event(key, modifiers, primary_editable);
        let Some(mut new_binding) = captured else {
            // Plain letter without modifier → reject with toast,
            // leave editing_hotkey set so the user can try again.
            self.set_toast(crate::i18n::t("hotkey_must_have_modifier").to_string());
            return Some(super::toast_clear_after_secs(2));
        };
        // For family actions we only edit modifiers; preserve the
        // existing primary so the suffix glyph (1...9 / arrows) stays.
        if !primary_editable
            && let Some(existing) = self
                .hotkey_bindings
                .get(&action)
                .and_then(|b| b.primary())
        {
            new_binding.primary = existing.primary;
        }

        Some(self.commit_captured_binding(action, slot, new_binding))
    }

    /// Translates a matched `(HotkeyAction, FamilyMatch)` into the
    /// concrete `Task<Message>` to dispatch. Returns `Task::none()`
    /// for matched-but-no-op cases (Ctrl+Shift+W with no active tab,
    /// Ctrl+P with no saved-host tab, Alt+arrow with no tabs open).
    /// The action is still considered consumed, so the key doesn't
    /// leak into PTY routing.
    pub(crate) fn dispatch_hotkey_action(
        &mut self,
        action: HotkeyAction,
        family: FamilyMatch,
    ) -> Task<Message> {
        use HotkeyAction::*;
        match action {
            // Route through the message so the new-tab intent is reset the
            // same way the `+` button does: the picker hotkey always opens
            // a fresh new-tab picker, never inherits a `pending_pane_split` left
            // armed by an earlier split-picker that was dismissed with Esc
            // (which would otherwise fill the old tab's split instead of
            // opening a new tab).
            ShowNewTabPicker => Task::done(Message::Tabs(TabsMessage::ShowNewTabPicker)),
            // Route through the message so the hotkey and the tab bar's
            // `⋯` button share one open path: search cleared and the
            // input focused for immediate type-to-filter.
            ShowTabJump => Task::done(Message::Tabs(TabsMessage::ShowTabJump)),
            // The palette assumes an unlocked vault (its actions do): if
            // the vault isn't unlocked, decline to open. Otherwise route
            // through the message so the query is reset + input focused.
            ShowCommandPalette => {
                if self.vault_ui.state == crate::state::VaultState::Unlocked {
                    Task::done(Message::Tabs(TabsMessage::ShowCommandPalette))
                } else {
                    Task::none()
                }
            }
            // While the new-tab picker is open the chord must do exactly
            // what clicking its Local Shell row does, which is fill a
            // pending split pane when the picker was opened from a split
            // (and dismiss the picker either way). The global action
            // opens a new tab and would quietly drop the split, so the
            // printed hint would lie about its own row.
            OpenLocalShell if self.panels.new_tab_picker => {
                Task::done(Message::Tabs(TabsMessage::PickLocalShell))
            }
            OpenLocalShell => Task::done(Message::Settings(SettingsMessage::OpenLocalShell)),
            NewWindow => Task::done(Message::Tabs(TabsMessage::SpawnNewWindow)),
            // Entity creation: the editor panels only render in their
            // owning vault section, so land there first (ShowKeyPanel
            // already navigates itself).
            NewHost => {
                self.active_view = View::Dashboard;
                self.active_tab = None;
                self.update(Message::Editor(EditorMessage::ShowNewConnection))
            }
            // Same "+ Host" editor as NewHost, opened empty: its
            // "Connect without saving" button is the ad-hoc session
            // path (issue #99). A distinct action (rather than an
            // alternate NewHost chord) so it reads as "Quick connect"
            // in the shortcut list and the palette, and rebinds
            // independently.
            ShowQuickConnect => {
                self.active_view = View::Dashboard;
                self.active_tab = None;
                self.update(Message::Editor(EditorMessage::ShowNewConnection))
            }
            NewKey => self.update(Message::Keys(KeysMessage::ShowKeyPanel)),
            NewIdentity => {
                self.active_view = View::Keys;
                self.active_tab = None;
                self.update(Message::Keys(KeysMessage::ShowIdentityPanel))
            }
            CloseActiveTab => {
                // With a terminal tab focused (View::Terminal or the
                // workspace) this closes the focused split pane;
                // ClosePane already falls back to closing the whole
                // tab when it's the last pane. Elsewhere there are no
                // panes, so close the active tab directly.
                if self.active_view == View::Terminal || self.active_tab.is_some() {
                    Task::done(Message::Terminal(TerminalMessage::ClosePane(None)))
                } else if let Some(idx) = self.active_tab {
                    Task::done(Message::Tabs(TabsMessage::CloseTab(idx)))
                } else {
                    Task::none()
                }
            }
            // Routed through the message so the chord, the tab context
            // menu and the command palette share one reopen path.
            ReopenClosedTab => Task::done(Message::Tabs(TabsMessage::ReopenClosedTab)),
            OpenPortForwards => {
                if let Some(id) = self
                    .active_tab_connection_idx()
                    .and_then(|idx| self.connections.get(idx))
                    .map(|c| c.id)
                {
                    Task::done(Message::Editor(EditorMessage::EditConnection(id)))
                } else if let Some(qid) = self.active_tab.and_then(|i| {
                    self.tabs.get(i).and_then(|t| match &t.active().origin {
                        crate::state::PaneOrigin::QuickHost(qid) => Some(*qid),
                        _ => None,
                    })
                }) {
                    // Ad-hoc tab: "edit host" becomes the save-to-vault
                    // prefill (there is no saved row to edit in place).
                    Task::done(Message::Editor(EditorMessage::SaveQuickHost(qid)))
                } else {
                    Task::none()
                }
            }
            OpenSettings => Task::done(Message::Navigation(NavigationMessage::ChangeView(View::Settings))),
            FocusViewSearch => Task::done(Message::Tabs(TabsMessage::FocusViewSearch)),
            OpenSftp => {
                if self.sftp_enabled {
                    Task::done(Message::Sftp(SftpMessage::NewSftpTab))
                } else {
                    Task::none()
                }
            }
            // With a live SSH tab in front, the console opens on THAT
            // host, at the directory its shell had reached: pressing
            // this while looking at a session means "this one". With
            // anything else in front there is no such host, so it falls
            // back to the picker the browser tab uses.
            OpenSftpConsole => {
                if !self.sftp_enabled {
                    return Task::none();
                }
                // On a tab that already HAS a console this is the switch,
                // not a second open: away from the console it reveals it,
                // on it it goes back to the shell. That makes one chord
                // the whole round trip, which is what a split needs and
                // what a zoomed console needs even more (there the other
                // pane is not on screen to be clicked).
                if let Some(idx) = self.active_tab
                    && self.tabs.get(idx).is_some_and(|t| t.console_pane().is_some())
                {
                    let target = match self.tab_surface(idx) {
                        crate::state::TabSurface::Console => crate::state::TabSurface::Terminal,
                        _ => crate::state::TabSurface::Console,
                    };
                    return self.show_tab_surface(idx, target);
                }
                match self
                    .active_tab
                    .and_then(|idx| self.tab_console_target(idx))
                {
                    Some((conn, dir)) => match self.active_tab {
                        Some(idx) => self.open_sftp_console_in_tab(idx, conn, dir),
                        None => self.open_sftp_console(conn, dir),
                    },
                    // No session in front to open a console ON, so the
                    // answer is "pick a host": the dashboard is where
                    // the cards live, and every card's menu offers the
                    // console. Doing nothing here would leave a palette
                    // row that is silently inert, which reads as broken
                    // the first time it is tried.
                    None => Task::done(Message::Navigation(NavigationMessage::ChangeView(
                        View::Dashboard,
                    ))),
                }
            }
            SwitchToTabSlot => match family {
                FamilyMatch::Digit(d) => {
                    Task::done(Message::Tabs(TabsMessage::ActivateStripSlot(d as usize - 1)))
                }
                _ => Task::none(),
            },
            // Ctrl+Shift+digit: jump straight to a vault section, in
            // the burger menu's VAULT order. Works from anywhere
            // (ChangeView handles leaving a terminal tab); digit 9 is
            // spare, and the Logs slot respects its visibility gate
            // like the menu entry does.
            VaultSectionSlot => match family {
                FamilyMatch::Digit(d) => {
                    let view = match d {
                        1 => Some(View::Dashboard),
                        2 => Some(View::Keys),
                        3 => Some(View::Snippets),
                        4 => Some(View::PortForwarding),
                        5 => self.logs_surface_visible().then_some(View::History),
                        6 => Some(View::Cloud),
                        7 => Some(View::Proxies),
                        8 => Some(View::KnownHosts),
                        // Monitoring pill (issue #95): same visibility
                        // gate as its pill and burger entry.
                        9 => self.prefs.host_monitoring.then_some(View::Monitoring),
                        _ => None,
                    };
                    match view {
                        Some(v) => Task::done(Message::Navigation(NavigationMessage::ChangeView(v))),
                        None => Task::none(),
                    }
                }
                _ => Task::none(),
            },
            CycleTabs => {
                // Walk the unified visual strip (terminal + SFTP, pinned-first)
                // so Alt+arrows step through every chip the user sees, in the
                // order they see it, instead of a raw `self.tabs` index that
                // skipped SFTP tabs and ignored pinning.
                let refs: Vec<crate::state::TabRef> = self
                    .ordered_tab_refs()
                    .into_iter()
                    .filter(|r| self.tab_ref_select_msg(r).is_some())
                    .collect();
                let n = refs.len();
                if n == 0 {
                    return Task::none();
                }
                let cur_pos = self
                    .active_tab_ref()
                    .and_then(|cr| refs.iter().position(|r| *r == cr))
                    .unwrap_or(0);
                let next_pos = match family {
                    FamilyMatch::ArrowRight => (cur_pos + 1) % n,
                    FamilyMatch::ArrowLeft => (cur_pos + n - 1) % n,
                    _ => return Task::none(),
                };
                match self.tab_ref_select_msg(&refs[next_pos]) {
                    Some(msg) => Task::done(msg),
                    None => Task::none(),
                }
            }
            ToggleFullscreen => Task::done(Message::Tabs(TabsMessage::WindowFullscreenToggle)),
            FontZoomIn => {
                self.terminal_font_size = (self.terminal_font_size + 1.0).min(24.0);
                self.persist_setting(
                    "terminal_font_size",
                    &format!("{}", self.terminal_font_size),
                );
                Task::none()
            }
            FontZoomOut => {
                self.terminal_font_size = (self.terminal_font_size - 1.0).max(10.0);
                self.persist_setting(
                    "terminal_font_size",
                    &format!("{}", self.terminal_font_size),
                );
                Task::none()
            }
            FontZoomReset => {
                self.terminal_font_size = 14.0;
                self.persist_setting("terminal_font_size", "14");
                Task::none()
            }
            // Terminal split panes. The loop only reaches these in the
            // terminal view (terminal_only gate), so no view check here.
            SplitPaneVertical => {
                Task::done(Message::Terminal(TerminalMessage::SplitPane(iced::widget::pane_grid::Axis::Vertical)))
            }
            SplitPaneHorizontal => {
                Task::done(Message::Terminal(TerminalMessage::SplitPane(iced::widget::pane_grid::Axis::Horizontal)))
            }
            FocusPaneLeft => {
                Task::done(Message::Terminal(TerminalMessage::FocusPaneDir(iced::widget::pane_grid::Direction::Left)))
            }
            FocusPaneRight => {
                Task::done(Message::Terminal(TerminalMessage::FocusPaneDir(iced::widget::pane_grid::Direction::Right)))
            }
            FocusPaneUp => {
                Task::done(Message::Terminal(TerminalMessage::FocusPaneDir(iced::widget::pane_grid::Direction::Up)))
            }
            FocusPaneDown => {
                Task::done(Message::Terminal(TerminalMessage::FocusPaneDir(iced::widget::pane_grid::Direction::Down)))
            }
            // Ring the sidebar lists (Snippets / History); repeat
            // presses cycle the two tabs. Terminal-only like the
            // split-pane family above.
            FocusSidebarList => self.focus_sidebar_list(),
            // Open/close the focused tab's sidebar (owner ask: a
            // keyboard path to close it; the handler already drops
            // the ring + dropdown gate on close). The target region
            // is resolved by `sidebar_toggle_target`: the lone
            // populated region, else the OPEN one, else the
            // historical right bias, so an open left region is
            // always closeable from the keyboard (issue #102).
            ToggleSidebar => match self.sidebar_toggle_target() {
                Some(side) => Task::done(Message::Ai(AiMessage::ToggleSidebarRegion(side))),
                None => Task::none(),
            },
            // The counterpart region, for setups with tabs docked to
            // both sides; no-op otherwise (the primary key already
            // reaches a lone region).
            ToggleSidebarOther => match self.sidebar_toggle_other_target() {
                Some(side) => Task::done(Message::Ai(AiMessage::ToggleSidebarRegion(side))),
                None => Task::none(),
            },
            // Hybrid tab: Terminal <-> Files for the focused tab (the
            // handler no-ops for tabs without a live SSH session).
            ToggleTabFiles => match self.active_tab {
                Some(idx) => Task::done(Message::Tabs(TabsMessage::ToggleTabFilesMode(idx))),
                None => Task::none(),
            },
            // Zoom the focused pane to the whole tab, and back. The
            // handler no-ops on a single-pane tab (already full size).
            ToggleMaximizePane => {
                Task::done(Message::Terminal(TerminalMessage::ToggleMaximizePane(None)))
            }
            // Broadcast input: arm / disarm fan-out across the focused
            // tab's panes.
            ToggleBroadcastInput => match self.active_tab {
                Some(idx) => Task::done(Message::Terminal(TerminalMessage::ToggleTabBroadcast(idx))),
                None => Task::none(),
            },
            // Reconnect the focused tab: same handler as the tab context
            // menu's entry, so live tabs restart and dead ones rebuild.
            ReconnectTab => match self.active_tab {
                Some(idx) => Task::done(Message::Tabs(TabsMessage::ReconnectTab(idx))),
                None => Task::none(),
            },
            // Privacy Mode session override (issue #78): volatile
            // forced-on/off above the global setting and the per-host
            // overrides; global, works from any surface.
            TogglePrivacyMode => Task::done(Message::Settings(SettingsMessage::TogglePrivacySessionOverride)),
            // Paste is the one clipboard action the dispatcher performs
            // itself: the widget can only write to a local PTY, so a
            // widget-side paste would silently do nothing over SSH.
            TerminalPaste => self.paste_clipboard_into_active(),
            // Copy / select-all / scrollback paging are performed by the
            // terminal WIDGET, which owns the selection and the scroll
            // offset (both live in its canvas state, out of reach from
            // here). These arms exist purely to SWALLOW the key: the
            // widget and this router are independent paths (see the note
            // in dispatch_terminal.rs), so without a match here the key
            // would also fall through to the PTY writer and echo a byte
            // on top of the widget's action.
            //
            // Copy / select-all get away without an alt-screen gate
            // because their chords are PTY-inert. Scrollback does not:
            // Shift+PageUp really does encode to ESC[5~, which is why it
            // is gated in the router loop above rather than here.
            TerminalCopy | TerminalPasteSelection | TerminalSelectAll | ScrollbackPageUp
            | ScrollbackPageDown => {
                Task::none()
            }
            // Vault section cycling: neighbor of the active view in the
            // sub-nav pill order, wrapping. The loop only reaches these
            // in the vault area (vault_only gate above).
            VaultSectionPrev | VaultSectionNext => {
                let sections: Vec<View> =
                    self.subnav_pill_defs().iter().map(|(_, v)| *v).collect();
                let forward = matches!(action, VaultSectionNext);
                let Some(next) = crate::keynav::movement::linear_move(
                    &sections,
                    Some(self.active_view),
                    forward,
                ) else {
                    return Task::none();
                };
                // Keep an active SubNav pill highlight through the
                // switch so arrows / Enter keep working from it.
                if matches!(
                    self.keynav.focus,
                    Some((crate::keynav::FocusZone::SubNav, _))
                ) {
                    self.keynav.focus = Some((
                        crate::keynav::FocusZone::SubNav,
                        crate::keynav::NavItem::SubNav(next),
                    ));
                    self.keynav.keep_focus_through_change_view = true;
                }
                Task::done(Message::Navigation(NavigationMessage::ChangeView(next)))
            }
        }
    }
}
