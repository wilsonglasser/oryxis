//! The four overlays that pick something: new tab, tab jump, the command
//! palette, and the host icon picker.
//!
//! All modal, all with their own search, and all closing on Esc through
//! the shared modal layer. Grouped because they behave alike, not
//! because they show alike.

use super::*;

impl Oryxis {
    pub(super) fn handle_tabs_pickers(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            TabsMessage::ShowNewTabPicker => {
                // Opening the picker from the `+` button always targets a new
                // tab, never a split (only SplitPane sets that).
                self.overlay = None; // dismiss the `+` hover popover if open
                self.pending_pane_split = None;
                self.panels.new_tab_picker = true;
                self.new_tab_picker_search.clear();
                self.new_tab_picker_group = None;
                // Land focus on the search so the picker is
                // type-to-filter from the first keystroke.
                return crate::widgets::focus_input(iced::widget::Id::new(
                    crate::state::NEW_TAB_PICKER_SEARCH_ID,
                ));
            }
            TabsMessage::HideNewTabPicker => {
                self.panels.new_tab_picker = false;
                self.pending_pane_split = None;
                self.new_tab_picker_group = None;
            }
            TabsMessage::NewTabPickerOpenGroup(gid) => {
                // Drill into the group; the search box now filters this
                // group's members instead of the top-level list, so clear
                // the leftover top-level needle.
                self.new_tab_picker_group = Some(gid);
                self.new_tab_picker_search.clear();
            }
            TabsMessage::NewTabPickerBack => {
                self.new_tab_picker_group = None;
                self.new_tab_picker_search.clear();
            }
            TabsMessage::NewTabPickerSearchChanged(v) => {
                self.new_tab_picker_search = v;
            }
            TabsMessage::NewTabPickerSubmit => {
                // Enter in the picker. Owned by the search input's
                // on_submit (the modal key router declines Enter here
                // so the two paths can never double-fire). Priority:
                // the explicit keyboard selection, then the ad-hoc
                // quick-connect target, then the top row of the
                // filtered list.
                if let Some((surface, _)) = self.modal_nav_surface()
                    && let Some(idx) = self.modal_nav_effective(surface)
                {
                    let action = self.keynav.modal.items.borrow().get(idx).cloned();
                    if let Some(msg) = action.and_then(|a| a.activate) {
                        return self.update(msg);
                    }
                }
                if let Some(conn) = self.quick_connect_target(&self.new_tab_picker_search)
                {
                    return self.update(Message::Ssh(SshMessage::QuickConnect(Box::new(
                        crate::state::QuickConnectEntry::bare(conn),
                    ))));
                }
                let top = self.keynav.modal.items.borrow().first().cloned();
                if let Some(msg) = top.and_then(|a| a.activate) {
                    return self.update(msg);
                }
            }
            TabsMessage::PickLocalShell => {
                self.panels.new_tab_picker = false;
                // Both destinations (a pending split pane and a new tab)
                // take the same route: the local-shell decision applies the
                // user's curated list / "always open X" default and raises
                // the shell picker when there is a real choice to make. The
                // split target stays pending across that picker and is
                // consumed by `open_local_shell_resolved` once a shell is
                // actually chosen. Splitting used to jump straight to the
                // OS default shell instead (issue #108).
                return self.update(Message::Settings(SettingsMessage::OpenLocalShell));
            }
            TabsMessage::ShowTabJump => {
                self.panels.tab_jump = true;
                self.tab_jump_search.clear();
                // Land focus on the search so the modal is
                // type-to-filter from the first keystroke, matching the
                // new-tab picker and the command palette. The modal's
                // Up/Down/Enter navigation arrives via the global key
                // subscription, so the focused input never blocks it.
                return crate::widgets::focus_input(iced::widget::Id::new(
                    crate::state::TAB_JUMP_SEARCH_ID,
                ));
            }
            TabsMessage::HideTabJump => {
                self.panels.tab_jump = false;
            }
            TabsMessage::TabJumpSearchChanged(v) => {
                self.tab_jump_search = v;
            }
            TabsMessage::TabJumpSelect(inner) => {
                self.panels.tab_jump = false;
                return Task::done(*inner);
            }
            TabsMessage::ShowCommandPalette => {
                // The palette assumes an unlocked vault (its actions do).
                // The hotkey path already gates on this; guard here too so
                // no other producer can open it over the lock screen.
                if self.vault_ui.state != crate::state::VaultState::Unlocked {
                    return Task::none();
                }
                self.palette.open = true;
                self.palette.query.clear();
                // Focus the query input so the user types immediately.
                return crate::widgets::focus_input(
                    iced::widget::Id::new(crate::palette::PALETTE_INPUT_ID),
                );
            }
            TabsMessage::HideCommandPalette => {
                self.palette.open = false;
                self.palette.query.clear();
            }
            TabsMessage::PaletteQueryChanged(v) => {
                self.palette.query = v;
            }
            TabsMessage::PaletteActivate(inner) => {
                // Two-step dispatch like TabJumpSelect: close first, then
                // fire the row's real message (it may open another modal).
                self.palette.open = false;
                self.palette.query.clear();
                return Task::done(*inner);
            }
            TabsMessage::ShowIconPicker(conn_id) => {
                // Pre-fill the picker with the icon the user is
                // currently seeing on the host card: custom override
                // first, then auto-detected OS, then the generic
                // "server" fallback as last resort. Using just
                // `custom_icon || "server"` here was buggy: hosts
                // whose icon comes from `detected_os` (Ubuntu, etc.)
                // showed "server" highlighted in the picker, so a
                // user clicking Save (even just to change the color)
                // accidentally overrode the auto-detected icon with
                // the generic stack glyph.
                if let Some(conn) = self.connections.iter().find(|c| c.id == conn_id) {
                    self.icon_picker.icon = conn
                        .custom_icon
                        .clone()
                        .or_else(|| conn.detected_os.clone())
                        .or_else(|| Some("server".to_string()));
                    self.icon_picker.color = conn.custom_color.clone();
                    self.icon_picker.hex_input = conn.custom_color.clone().unwrap_or_default();
                }
                self.icon_picker.icon_search.clear();
                self.icon_color_popover = None;
                self.icon_picker.for_id = Some(conn_id);
                self.icon_picker.for_local_terminal = false;
                self.panels.icon_picker = true;
            }
            TabsMessage::HideIconPicker => {
                self.panels.icon_picker = false;
                self.icon_picker.for_id = None;
                self.icon_picker.for_session_group = false;
                self.icon_picker.for_group_edit = false;
                self.icon_picker.for_local_terminal = false;
                self.icon_picker.icon_search.clear();
                self.icon_color_popover = None;
            }
            TabsMessage::IconPickerSelectIcon(name) => {
                self.icon_picker.icon = Some(name);
            }
            TabsMessage::IconPickerIconSearchChanged(q) => {
                self.icon_picker.icon_search = q;
            }
            TabsMessage::IconPickerOpenColorPopover => {
                self.icon_color_popover = Some(self.mouse_position);
            }
            TabsMessage::IconPickerCloseColorPopover => {
                self.icon_color_popover = None;
            }
            TabsMessage::IconPickerSelectColor(hex) => {
                self.icon_picker.hex_input = hex.clone();
                self.icon_picker.color = Some(hex);
            }
            TabsMessage::IconPickerHexInputChanged(v) => {
                self.icon_picker.hex_input = v.clone();
                // Validate + commit only on well-formed #RRGGBB.
                let trimmed = v.trim().trim_start_matches('#');
                if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    self.icon_picker.color = Some(format!("#{}", trimmed.to_uppercase()));
                }
            }
            TabsMessage::IconPickerSave => return self.handle_icon_picker_save(),
            TabsMessage::IconPickerResetAuto => return self.handle_icon_picker_reset_auto(),
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
