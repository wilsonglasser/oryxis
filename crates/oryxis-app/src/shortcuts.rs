//! Helpers for the global keyboard shortcuts wired in
//! `dispatch_terminal.rs`. Kept in its own module so the dispatcher
//! files stay focused on message routing.

use iced::keyboard::{key::Named, Key, Modifiers};
use iced::widget;
use iced::Task;

use crate::app::{Message, Oryxis};
use crate::hotkeys::{FamilyMatch, HotkeyAction};
use crate::state::View;

impl Oryxis {
    /// Resolves slot N (0-indexed) of the visual tab strip to the
    /// `Message` that activates that slot, mirroring the order
    /// `views/tab_bar.rs` renders. Returns `None` when the slot is
    /// out of range so Ctrl+5 on a window with two tabs is a no-op
    /// instead of bouncing focus around.
    pub(crate) fn strip_slot_target(&self, slot: usize) -> Option<Message> {
        // Hosts (and SFTP when enabled) sit in the strip ahead of the
        // terminal tabs, so Ctrl+1 / Ctrl+2 land on them.
        let mut slots: Vec<Message> = Vec::new();
        slots.push(Message::ChangeView(View::Dashboard));
        if self.sftp_enabled {
            slots.push(Message::ChangeView(View::Sftp));
        }
        for idx in 0..self.tabs.len() {
            slots.push(Message::SelectTab(idx));
        }
        slots.into_iter().nth(slot)
    }

    /// Resolves the active tab to its position in `self.connections`,
    /// or `None` when no tab is active, the tab is a local shell, or
    /// the saved host has since been deleted. Used by Ctrl+P to open
    /// the host editor for the current connection.
    pub(crate) fn active_tab_connection_idx(&self) -> Option<usize> {
        let tab_idx = self.active_tab?;
        let tab = self.tabs.get(tab_idx)?;
        let base_label = tab.label.trim_end_matches(" (disconnected)");
        self.connections.iter().position(|c| c.label == base_label)
    }

    /// Returns the `widget::Id` of the search/filter input for the
    /// current view, or `None` when the view has no search field.
    /// Consumed by `Message::FocusViewSearch` (Ctrl+F).
    pub(crate) fn active_view_search_id(&self) -> Option<widget::Id> {
        match self.active_view {
            View::Dashboard => Some(widget::Id::new("search-dashboard")),
            View::Keys => Some(widget::Id::new("search-keys")),
            // Snippets and History only expose their search field on
            // the Workspace-mode sub-nav. In Classic mode there's no
            // search input to focus, so Ctrl+F harmlessly tries to
            // focus an Id that doesn't exist (iced no-ops on a miss).
            View::Snippets => Some(widget::Id::new("search-snippets")),
            View::PortForwarding => Some(widget::Id::new("search-port-forwards")),
            View::History => Some(widget::Id::new("search-history")),
            View::Sftp => {
                // Two filter inputs (local + remote panes); focus
                // the remote one since that's the side that costs an
                // SSH round-trip and is where a typed filter matters
                // most.
                Some(widget::Id::new("search-sftp-remote"))
            }
            View::Settings
            | View::Terminal
            | View::Cloud
            | View::Proxies
            | View::KnownHosts => None,
        }
    }

    /// `true` when a global picker / modal overlay is open and should
    /// swallow keyboard input instead of letting it fall through to the
    /// PTY underneath. Mirrors the set checked by `close_topmost_modal`
    /// (minus the burger menu, which carries no text field). Used by the
    /// keyboard router in `dispatch_terminal.rs` so typing in a picker's
    /// search field doesn't also leak into the terminal behind it.
    /// True when a blocking modal owns the keyboard, so the global key
    /// subscription must NOT route the press to the active PTY.
    ///
    /// INVARIANT: every modal that contains a text field MUST appear here.
    /// The terminal input arrives via a global subscription
    /// (`subscription.rs`) that bypasses the widget tree, so a modal's own
    /// focused `text_input` does not stop the same press from also reaching
    /// the PTY, only this predicate does. Every modal here MUST also be a
    /// full-window overlay (so a set flag always means a visible, input-
    /// owning modal) and SHOULD appear in `close_topmost_modal` so Esc
    /// dismisses it. The SFTP modals now layer at the app root via
    /// `layer_sftp_modals`, so they satisfy that invariant too.
    pub(crate) fn any_modal_blocks_input(&self) -> bool {
        self.show_new_tab_picker
            || self.show_tab_jump
            || self.show_icon_picker
            || self.show_theme_picker
            || self.show_chain_editor
            || self.show_session_group_panel
            || self.folder_rename.is_some()
            || self.folder_delete.is_some()
            // Keyboard-interactive (2FA / OTP) prompt: its text fields own
            // the keyboard. Without this, a split-pane connect (where the
            // terminal stays live behind the app-level modal) would echo
            // the OTP into the PTY as well. The inline connect-progress
            // path is already covered by the `connecting.is_none()` gate.
            || self.pending_kbi_prompt.is_some()
            // Theme + share + cloud-import modals (all carry text inputs).
            || self.theme_editor.is_some()
            || self.show_theme_import
            || self.ui_theme_editor.is_some()
            || self.show_share_dialog
            || self.cloud_import_confirm_visible
            // SFTP modals (full-window overlays via `layer_sftp_modals`).
            || self.sftp.rename.is_some()
            || self.sftp.new_entry.is_some()
            || self.sftp.properties.is_some()
            || self.sftp.overwrite_prompt.is_some()
            || self.sftp.picker_open
    }

    /// Closes the topmost open modal / overlay if any, and returns
    /// `true` when something was closed. Lets the Esc handler in
    /// `dispatch_terminal.rs` decide whether to also forward the
    /// byte to the active PTY (it doesn't, when this returns true).
    /// Priority follows visual stacking order: pickers on top of
    /// the chrome are checked before background panels.
    pub(crate) fn close_topmost_modal(&mut self) -> bool {
        // Global pickers (rendered over the whole window).
        if self.show_new_tab_picker {
            self.show_new_tab_picker = false;
            return true;
        }
        if self.show_tab_jump {
            self.show_tab_jump = false;
            return true;
        }
        if self.show_icon_picker {
            self.show_icon_picker = false;
            self.icon_picker_for = None;
            return true;
        }
        if self.show_theme_picker {
            self.show_theme_picker = false;
            return true;
        }
        if self.show_chain_editor {
            // Esc in "add a hop" mode pops back to the chain list;
            // only a second Esc closes the whole editor.
            if self.chain_editor_adding {
                self.chain_editor_adding = false;
                self.chain_editor_search.clear();
            } else {
                self.show_chain_editor = false;
            }
            return true;
        }
        if self.folder_rename.is_some() {
            self.folder_rename = None;
            return true;
        }
        if self.folder_delete.is_some() {
            self.folder_delete = None;
            return true;
        }
        if self.show_session_group_panel {
            self.show_session_group_panel = false;
            self.session_group_panel_error = None;
            return true;
        }
        // Settings theme + share + cloud-import modals. Cleanup mirrors each
        // modal's own Cancel handler so Esc leaves no stale companion state.
        if self.theme_editor.is_some() {
            self.theme_editor = None;
            self.theme_color_popover = None;
            return true;
        }
        if self.ui_theme_editor.is_some() {
            self.ui_theme_editor = None;
            self.ui_color_popover = None;
            return true;
        }
        if self.show_theme_import {
            self.show_theme_import = false;
            return true;
        }
        if self.show_share_dialog {
            self.show_share_dialog = false;
            self.share_filter = None;
            self.share_status = None;
            self.share_suggested_name = None;
            return true;
        }
        if self.cloud_import_confirm_visible {
            self.cloud_import_confirm_visible = false;
            self.cloud_discover_default_group_picker_open = false;
            return true;
        }
        // SFTP host picker: no inline Cancel button (it dismisses on a
        // backdrop click), so Esc is its keyboard equivalent. Mirrors the
        // `SftpClosePicker` handler, which only flips the flag.
        if self.sftp.picker_open {
            self.sftp.picker_open = false;
            return true;
        }
        // Burger menu last; it's a dropdown rather than a modal but
        // Esc still feels right for it.
        if self.show_burger_menu {
            self.show_burger_menu = false;
            return true;
        }
        false
    }

    /// Spawns a fresh top-level Oryxis process. When `source_tab`
    /// names a tab bound to a saved connection, passes
    /// `--connect <uuid>` so the new window auto-opens it. When the
    /// caller already has a master password unlocked, also passes
    /// `--inherit-vault` and pipes the password through stdin so the
    /// secret never appears in argv (which `ps aux` would expose).
    pub(crate) fn spawn_oryxis_child(&self, source_tab: Option<usize>) {
        // Map the tab back to a saved connection so the child opens the
        // same host. SSM-into-EC2 tabs carry a title prefix; strip it so
        // the lookup matches (the child re-routes to SSM via cloud_ref).
        // ECS Exec / kubectl tabs are ephemeral dynamic-group resources
        // with no saved connection, so they resolve to None and the child
        // opens a plain window (a fresh process can't carry an in-memory
        // relaunch message across the boundary).
        let connect_uuid = source_tab.and_then(|idx| {
            self.tabs.get(idx).and_then(|tab| {
                let base_label = tab
                    .label
                    .trim_end_matches(" (disconnected)")
                    .trim_start_matches(crate::app::SSM_TAB_PREFIX)
                    .to_string();
                self.connections
                    .iter()
                    .find(|c| c.label == base_label)
                    .map(|c| c.id)
            })
        });
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("current_exe unavailable: {}", e);
                return;
            }
        };
        let mut cmd = std::process::Command::new(exe);
        if let Some(uuid) = connect_uuid {
            cmd.arg("--connect").arg(uuid.to_string());
        }
        let inherit = self.master_password.is_some();
        if inherit {
            cmd.arg("--inherit-vault");
            cmd.stdin(std::process::Stdio::piped());
        }
        match cmd.spawn() {
            Ok(mut child) => {
                if inherit
                    && let Some(mut stdin) = child.stdin.take()
                    && let Some(pw) = self.master_password.as_ref()
                {
                    use std::io::Write as _;
                    let _ = writeln!(stdin, "{}", pw);
                    // Closing the pipe signals EOF to the child.
                    drop(stdin);
                }
            }
            Err(e) => tracing::error!("Failed to spawn new window: {}", e),
        }
    }

    /// Pretty-printed binding for the given action (`"Ctrl + K"`),
    /// or `None` when the action has no binding (conflict-unbound).
    /// Used by the burger menu / context menus to surface the
    /// current shortcut next to its action.
    pub(crate) fn hotkey_label_for_action(
        &self,
        action: HotkeyAction,
    ) -> Option<String> {
        let binding = self.hotkey_bindings.get(&action)?;
        Some(binding.badges().join(" + "))
    }

    /// Pretty-printed binding for the Nth strip slot (0-indexed),
    /// e.g. `"Ctrl + 1"` for slot 0 when `SwitchToTabSlot` is
    /// bound to Ctrl + digit. Returns `None` when the family is
    /// unbound. Used by the burger menu to show the per-area
    /// shortcut next to Hosts / SFTP.
    pub(crate) fn hotkey_label_for_strip_slot(
        &self,
        slot: usize,
    ) -> Option<String> {
        let binding = self.hotkey_bindings.get(&HotkeyAction::SwitchToTabSlot)?;
        let mut parts = binding.badges();
        // Drop the family suffix ("1...9") and append the concrete
        // slot digit so the hint reads like a real chord.
        parts.pop();
        parts.push((slot + 1).to_string());
        Some(parts.join(" + "))
    }

    /// Main entry point for `dispatch_terminal::Message::KeyboardEvent`.
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

        // 1b. Split-pane shortcuts (terminal view only). Fixed bindings,
        //     mirroring GNOME Terminal, not (yet) in the rebind editor:
        //     Ctrl+Shift+E / O split the focused pane side-by-side /
        //     stacked, Ctrl+Shift+W closes it, and Ctrl+Shift+arrows move
        //     focus between panes.
        if self.active_view == View::Terminal && modifiers.control() && modifiers.shift() {
            use iced::widget::pane_grid::{Axis, Direction};
            if let Key::Character(c) = key {
                match c.as_str() {
                    "e" | "E" => return Some(self.update(Message::SplitPane(Axis::Vertical))),
                    "o" | "O" => return Some(self.update(Message::SplitPane(Axis::Horizontal))),
                    "w" | "W" => return Some(self.update(Message::ClosePane)),
                    _ => {}
                }
            }
            if let Key::Named(named) = key {
                let dir = match named {
                    Named::ArrowLeft => Some(Direction::Left),
                    Named::ArrowRight => Some(Direction::Right),
                    Named::ArrowUp => Some(Direction::Up),
                    Named::ArrowDown => Some(Direction::Down),
                    _ => None,
                };
                if let Some(d) = dir {
                    return Some(self.update(Message::FocusPaneDir(d)));
                }
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
        let in_terminal = self.active_view == View::Terminal;
        for &action in HotkeyAction::all() {
            let bind_copy = self.hotkey_bindings.get(&action).copied();
            if in_terminal
                && bind_copy.is_some_and(|b| b.is_terminal_control_sequence())
            {
                continue;
            }
            let Some(b) = bind_copy else { continue };
            if let Some(family) = b.match_event(key, modifiers) {
                return Some(self.dispatch_hotkey_action(action, family));
            }
        }

        // 3. Esc closes the topmost open modal as a fallback. Only
        //    fires when nothing else above claimed it, so terminal
        //    apps that rely on raw Esc (vim, less) keep getting the
        //    byte when no modal is open.
        if matches!(key, Key::Named(Named::Escape)) && self.close_topmost_modal() {
            return Some(Task::none());
        }

        None
    }

    /// Capture-mode branch of `handle_hotkey_keypress`. Esc cancels;
    /// pure-modifier presses are ignored (they fire `KeyPressed` too);
    /// anything else becomes the new binding (validated by
    /// `binding_from_event::is_safe`). Conflicts with another action
    /// unbind the loser and surface a toast naming it.
    fn handle_hotkey_capture(
        &mut self,
        key: &Key,
        modifiers: &Modifiers,
    ) -> Option<Task<Message>> {
        let action = self.editing_hotkey?;
        // Esc cancels without saving.
        if matches!(key, Key::Named(Named::Escape)) {
            self.editing_hotkey = None;
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
            self.toast = Some(crate::i18n::t("hotkey_must_have_modifier").to_string());
            return Some(toast_clear_after_secs(2));
        };
        // For family actions we only edit modifiers; preserve the
        // existing primary so the suffix glyph (1...9 / arrows) stays.
        if !primary_editable
            && let Some(existing) = self.hotkey_bindings.get(&action)
        {
            new_binding.primary = existing.primary;
        }

        // Conflict resolution: if another action already owns this
        // exact binding, unbind that action and surface a toast that
        // names *the action* (not the key combo) so the family case
        // reads "Switch to specific Tab is now unbound" instead of
        // "Ctrl+1 is now unbound".
        let conflict: Option<HotkeyAction> = self
            .hotkey_bindings
            .iter()
            .find(|(a, b)| **a != action && **b == new_binding)
            .map(|(a, _)| *a);
        let conflict_toast: Option<Task<Message>> = conflict.map(|other| {
            // Auto-rebind the conflicting action to its factory default
            // when that default doesn't itself collide with the new
            // binding (or with any other live binding). Beats leaving
            // the user with an orphaned action they have to discover
            // and re-set themselves. Falls back to unbinding when the
            // default would be a fresh conflict.
            let defaults = crate::hotkeys::default_bindings();
            let default_for_other = defaults.get(&other).copied();
            let default_safe = default_for_other.is_some_and(|d| {
                d != new_binding
                    && !self.hotkey_bindings.iter().any(|(a, b)| {
                        *a != action && *a != other && *b == d
                    })
            });
            if let Some(d) = default_for_other.filter(|_| default_safe) {
                self.hotkey_bindings.insert(other, d);
                self.persist_setting(
                    &format!("hotkey_{}", other.id()),
                    &d.serialize(),
                );
                self.toast = Some(
                    crate::i18n::t("hotkey_conflict_rebound_default")
                        .replace("{action}", crate::i18n::t(other.label_key())),
                );
            } else {
                self.hotkey_bindings.remove(&other);
                self.persist_setting(&format!("hotkey_{}", other.id()), "");
                self.toast = Some(
                    crate::i18n::t("hotkey_conflict_unbound")
                        .replace("{action}", crate::i18n::t(other.label_key())),
                );
            }
            toast_clear_after_secs(3)
        });

        self.hotkey_bindings.insert(action, new_binding);
        self.persist_setting(
            &format!("hotkey_{}", action.id()),
            &new_binding.serialize(),
        );
        self.editing_hotkey = None;

        Some(conflict_toast.unwrap_or_else(Task::none))
    }

    /// Translates a matched `(HotkeyAction, FamilyMatch)` into the
    /// concrete `Task<Message>` to dispatch. Returns `Task::none()`
    /// for matched-but-no-op cases (Ctrl+Shift+W with no active tab,
    /// Ctrl+P with no saved-host tab, Alt+arrow with no tabs open).
    /// The action is still considered consumed, so the key doesn't
    /// leak into PTY routing.
    fn dispatch_hotkey_action(
        &mut self,
        action: HotkeyAction,
        family: FamilyMatch,
    ) -> Task<Message> {
        use HotkeyAction::*;
        match action {
            ShowNewTabPicker => {
                self.show_new_tab_picker = true;
                self.new_tab_picker_search.clear();
                Task::none()
            }
            ShowTabJump => {
                self.show_tab_jump = true;
                self.tab_jump_search.clear();
                Task::none()
            }
            OpenLocalShell => Task::done(Message::OpenLocalShell),
            NewWindow => Task::done(Message::SpawnNewWindow),
            CloseActiveTab => {
                if let Some(idx) = self.active_tab {
                    Task::done(Message::CloseTab(idx))
                } else {
                    Task::none()
                }
            }
            OpenPortForwards => {
                if let Some(idx) = self.active_tab_connection_idx() {
                    Task::done(Message::EditConnection(idx))
                } else {
                    Task::none()
                }
            }
            OpenSettings => Task::done(Message::ChangeView(View::Settings)),
            FocusViewSearch => Task::done(Message::FocusViewSearch),
            SwitchToTabSlot => match family {
                FamilyMatch::Digit(d) => {
                    Task::done(Message::ActivateStripSlot(d as usize - 1))
                }
                _ => Task::none(),
            },
            CycleTabs => {
                if self.tabs.is_empty() {
                    return Task::none();
                }
                let n = self.tabs.len();
                let current = self.active_tab.unwrap_or(0);
                let next = match family {
                    FamilyMatch::ArrowRight => (current + 1) % n,
                    FamilyMatch::ArrowLeft => (current + n - 1) % n,
                    _ => return Task::none(),
                };
                Task::done(Message::SelectTab(next))
            }
            ToggleFullscreen => Task::done(Message::WindowFullscreenToggle),
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
        }
    }
}

/// Helper used by the capture branch: dispatch a `Message::ToastClear`
/// after `secs` seconds. Same shape as the existing `CopyToClipboard`
/// toast flow.
fn toast_clear_after_secs(secs: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        },
        |_| Message::ToastClear,
    )
}
