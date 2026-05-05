//! `Oryxis::handle_tabs` — match arms for the tab strip + tab modals
//! (new-tab picker, tab-jump, icon picker), card hover/menu, folder
//! actions, window chrome (drag/resize/min/max/close).

#![allow(clippy::result_large_err)]

use iced::{Point, Task};

use crate::app::{Message, Oryxis};
use crate::state::{OverlayContent, OverlayState, View};

/// Smallest gap between two `WindowDrag` / `WindowResizeDrag`
/// presses we'll accept. iced's `MouseArea` re-fires `on_press` on
/// the second click of a double-click before `on_double_click` lands;
/// honouring that second drag races our `toggle_maximize` /
/// `WindowExpand*` follow-up. `300ms` is wider than any realistic
/// double-click and short enough that a deliberate two-quick-clicks-
/// to-drag still feels responsive.
const WINDOW_PRESS_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

impl Oryxis {
    /// Returns `true` when this press should be forwarded to the OS.
    /// Returns `false` when the previous press was within
    /// [`WINDOW_PRESS_DEBOUNCE`], swallowing the spurious second
    /// `on_press` that a double-click emits.
    pub(crate) fn consume_window_press(&mut self) -> bool {
        let now = std::time::Instant::now();
        let allow = self
            .last_window_press_at
            .is_none_or(|prev| now.duration_since(prev) >= WINDOW_PRESS_DEBOUNCE);
        if allow {
            self.last_window_press_at = Some(now);
        }
        allow
    }

    pub(crate) fn handle_tabs(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Card interactions --
            Message::CardHovered(idx) => {
                self.hovered_card = Some(idx);
            }
            Message::CardUnhovered => {
                self.hovered_card = None;
            }
            Message::FolderCardHovered(gid) => {
                self.hovered_folder_card = Some(gid);
            }
            Message::FolderCardUnhovered => {
                self.hovered_folder_card = None;
            }
            Message::KeyCardHovered(idx) => {
                self.hovered_key_card = Some(idx);
            }
            Message::KeyCardUnhovered => {
                self.hovered_key_card = None;
            }
            Message::IdentityCardHovered(idx) => {
                self.hovered_identity_card = Some(idx);
            }
            Message::IdentityCardUnhovered => {
                self.hovered_identity_card = None;
            }
            Message::MouseMoved(pos) => {
                self.mouse_position = pos;
                // While the chat-sidebar resize handle is held down, the
                // sidebar width tracks the cursor — dragging left grows
                // the panel, dragging right shrinks it. Clamp to a sane
                // band so the user can't accidentally make it unusable.
                if let Some((start_x, start_width)) = self.chat_sidebar_drag {
                    let new_width = (start_width - (pos.x - start_x)).clamp(260.0, 700.0);
                    self.chat_sidebar_width = new_width;
                }
                // Promote a pending press to an active drag once the
                // cursor moves past the threshold. Below the threshold
                // we leave it pending so the click handler still fires
                // for plain clicks (jitter < 5px).
                if let Some(drag) = self.sftp.drag.as_mut()
                    && !drag.active
                {
                    let dx = pos.x - drag.press_pos.x;
                    let dy = pos.y - drag.press_pos.y;
                    if (dx * dx + dy * dy).sqrt() > 5.0 {
                        drag.active = true;
                    }
                }
            }
            Message::WindowResized(size) => {
                self.window_size = size;
            }
            Message::WindowDrag => {
                if !self.consume_window_press() {
                    return Ok(Task::none());
                }
                return Ok(iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::drag(id),
                    None => Task::none(),
                }));
            }
            Message::WindowResizeDrag(direction) => {
                // Ignore resize requests while maximized — the window has no
                // borders to grab and the OS will reject/misbehave on WinIt.
                if self.window_maximized {
                    return Ok(Task::none());
                }
                if !self.consume_window_press() {
                    return Ok(Task::none());
                }
                return Ok(iced::window::latest().then(move |id_opt| match id_opt {
                    Some(id) => iced::window::drag_resize(id, direction),
                    None => Task::none(),
                }));
            }
            Message::WindowExpandVertical => {
                if self.window_maximized {
                    return Ok(Task::none());
                }
                let current_width = self.window_size.width;
                return Ok(iced::window::latest().then(move |id_opt| {
                    let Some(id) = id_opt else { return Task::none(); };
                    iced::window::position(id).then(move |pos_opt| {
                        let Some(pos) = pos_opt else { return Task::none(); };
                        iced::window::monitor_size(id).then(move |size_opt| {
                            let Some(size) = size_opt else { return Task::none(); };
                            iced::window::monitor_position(id).then(move |origin_opt| {
                                // Default to (0, 0) when the platform
                                // can't report the monitor origin so we
                                // at least fall back to the primary —
                                // same as the old behaviour.
                                let origin = origin_opt.unwrap_or(Point::ORIGIN);
                                Task::batch([
                                    iced::window::move_to(
                                        id,
                                        Point::new(pos.x, origin.y),
                                    ),
                                    iced::window::resize(
                                        id,
                                        iced::Size::new(current_width, size.height),
                                    ),
                                ])
                            })
                        })
                    })
                }));
            }
            Message::WindowMinimize => {
                return Ok(iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::minimize(id, true),
                    None => Task::none(),
                }));
            }
            Message::WindowMaximizeToggle => {
                self.window_maximized = !self.window_maximized;
                return Ok(iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::toggle_maximize(id),
                    None => Task::none(),
                }));
            }
            Message::WindowClose => {
                return Ok(iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::close(id),
                    None => Task::none(),
                }));
            }
            Message::HideOverlayMenu => {
                self.overlay = None;
                self.card_context_menu = None;
                self.key_context_menu = None;
                self.identity_context_menu = None;
                self.show_keychain_add_menu = false;
            }
            Message::ShowCardMenu(idx) => {
                if self.card_context_menu == Some(idx) {
                    self.card_context_menu = None;
                    self.overlay = None;
                } else {
                    self.card_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::HostActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::HideCardMenu => {
                self.card_context_menu = None;
                self.overlay = None;
            }

            // -- Tabs --
            Message::SelectTab(idx) => {
                if idx < self.tabs.len() {
                    self.active_tab = Some(idx);
                    self.active_view = View::Terminal;
                    return Ok(self.tab_scroll_to_active());
                }
            }
            Message::TabHovered(idx) => {
                self.hovered_tab = Some(idx);
            }
            Message::TabUnhovered => {
                self.hovered_tab = None;
            }
            Message::ShowNewTabPicker => {
                self.show_new_tab_picker = true;
                self.new_tab_picker_search.clear();
            }
            Message::HideNewTabPicker => {
                self.show_new_tab_picker = false;
            }
            Message::ShowTabJump => {
                self.show_tab_jump = true;
                self.tab_jump_search.clear();
            }
            Message::HideTabJump => {
                self.show_tab_jump = false;
            }
            Message::TabJumpSearchChanged(v) => {
                self.tab_jump_search = v;
            }
            Message::TabBarWheel(dy) => {
                // Vertical wheel over the tab bar scrolls horizontally —
                // iced's horizontal-only scrollable ignores y deltas, so
                // we translate them via scroll_by here. Sign flip so
                // wheel-down brings later tabs into view (matches the
                // direction Chrome/VS Code use).
                return Ok(iced::widget::operation::scroll_by(
                    iced::widget::Id::new("tab-scroll"),
                    iced::widget::scrollable::AbsoluteOffset { x: -dy, y: 0.0 },
                ));
            }
            Message::TabJumpSelect(inner) => {
                self.show_tab_jump = false;
                return Ok(Task::done(*inner));
            }
            Message::NoOp => {}
            Message::NewTabPickerSearchChanged(v) => {
                self.new_tab_picker_search = v;
            }
            Message::ShowIconPicker(conn_id) => {
                // Pre-fill the picker with whatever the connection currently
                // has (custom > detected). The user either confirms, edits,
                // or clicks "Reset to auto" to drop the override entirely.
                if let Some(conn) = self.connections.iter().find(|c| c.id == conn_id) {
                    self.icon_picker_icon = conn
                        .custom_icon
                        .clone()
                        .or_else(|| Some("server".to_string()));
                    self.icon_picker_color = conn.custom_color.clone();
                    self.icon_picker_hex_input = conn.custom_color.clone().unwrap_or_default();
                }
                self.icon_picker_for = Some(conn_id);
                self.show_icon_picker = true;
            }
            Message::HideIconPicker => {
                self.show_icon_picker = false;
                self.icon_picker_for = None;
            }
            Message::IconPickerSelectIcon(name) => {
                self.icon_picker_icon = Some(name);
            }
            Message::IconPickerSelectColor(hex) => {
                self.icon_picker_hex_input = hex.clone();
                self.icon_picker_color = Some(hex);
            }
            Message::IconPickerHexInputChanged(v) => {
                self.icon_picker_hex_input = v.clone();
                // Validate + commit only on well-formed #RRGGBB.
                let trimmed = v.trim().trim_start_matches('#');
                if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    self.icon_picker_color = Some(format!("#{}", trimmed.to_uppercase()));
                }
            }
            Message::IconPickerSave => {
                if let Some(conn_id) = self.icon_picker_for {
                    let icon = self.icon_picker_icon.clone();
                    let color = self.icon_picker_color.clone();
                    if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                        conn.custom_icon = icon.clone();
                        conn.custom_color = color.clone();
                        // Full save so the row persists (and other fields
                        // aren't accidentally overwritten).
                        if let Some(vault) = &self.vault {
                            let _ = vault.save_connection(conn, None);
                        }
                    }
                }
                self.show_icon_picker = false;
                self.icon_picker_for = None;
            }
            Message::IconPickerResetAuto => {
                // Clears the override, letting the OS-detection result (if any)
                // drive the icon again. Does not trigger re-detection — that
                // happens on the next connect if the OS is still unknown.
                if let Some(conn_id) = self.icon_picker_for
                    && let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                    conn.custom_icon = None;
                    conn.custom_color = None;
                    if let Some(vault) = &self.vault {
                        let _ = vault.save_connection(conn, None);
                    }
                }
                self.show_icon_picker = false;
                self.icon_picker_for = None;
            }
            Message::CloseTab(idx) => {
                // Also dismiss any open context menu so the menu doesn't linger
                // after the user clicks Close from it.
                self.overlay = None;
                if idx < self.tabs.len() {
                    self.tabs.remove(idx);
                    if self.tabs.is_empty() {
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        self.active_tab = Some(idx.min(self.tabs.len() - 1));
                    }
                }
            }
            Message::ShowTabMenu(idx) => {
                self.overlay = Some(OverlayState {
                    content: OverlayContent::TabActions(idx),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::ReconnectTab(idx) => {
                self.overlay = None;
                // Find the connection matching this tab's label; close the tab and
                // dispatch ConnectSsh for that connection index. Dead tabs (no matching
                // connection) are just closed.
                if let Some(tab) = self.tabs.get(idx) {
                    let base_label = tab.label.trim_end_matches(" (disconnected)").to_string();
                    let conn_idx = self.connections.iter().position(|c| c.label == base_label);
                    self.tabs.remove(idx);
                    if self.tabs.is_empty() {
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        self.active_tab = Some(idx.min(self.tabs.len() - 1));
                    }
                    if let Some(ci) = conn_idx {
                        return Ok(Task::done(Message::ConnectSsh(ci)));
                    }
                }
            }
            Message::DuplicateTab(idx) => {
                self.overlay = None;
                // Local shell tabs aren't backed by a saved connection; for
                // those we just open a fresh shell tab. SSH tabs find their
                // connection by label and dispatch `ConnectSsh` so the user
                // gets a second live session into the same box.
                if let Some(tab) = self.tabs.get(idx) {
                    let is_local_shell = tab.active().ssh_session.is_none()
                        && tab.label == "Local Shell";
                    if is_local_shell {
                        return Ok(Task::done(Message::OpenLocalShell));
                    }
                    let base_label = tab.label.trim_end_matches(" (disconnected)").to_string();
                    if let Some(ci) = self.connections.iter().position(|c| c.label == base_label) {
                        return Ok(Task::done(Message::ConnectSsh(ci)));
                    }
                }
            }
            Message::DuplicateInNewWindow(idx) => {
                self.overlay = None;
                // Spawn a fresh Oryxis process. When the source tab is
                // bound to a saved connection we pass `--connect <uuid>`
                // so the new window auto-opens it. When the user has a
                // master password we also pass `--inherit-vault` and pipe
                // the password through stdin — keeps the secret out of
                // command-line arguments (which `ps aux` would expose).
                let connect_uuid = self.tabs.get(idx).and_then(|tab| {
                    let base_label = tab.label.trim_end_matches(" (disconnected)").to_string();
                    self.connections
                        .iter()
                        .find(|c| c.label == base_label)
                        .map(|c| c.id)
                });
                let exe = match std::env::current_exe() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("current_exe unavailable: {}", e);
                        return Ok(Task::none());
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
            Message::ShowFolderActions(gid) => {
                // Anchor the menu to the cursor — matches the host-card
                // "..." pattern. The global MouseMoved subscription keeps
                // `mouse_position` fresh.
                self.overlay = Some(OverlayState {
                    content: OverlayContent::FolderActions(gid),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::StartRenameFolder(gid) => {
                self.overlay = None;
                let current = self
                    .groups
                    .iter()
                    .find(|g| g.id == gid)
                    .map(|g| g.label.clone())
                    .unwrap_or_default();
                self.folder_rename = Some((gid, current));
            }
            Message::FolderRenameInput(val) => {
                if let Some((_, ref mut buf)) = self.folder_rename {
                    *buf = val;
                }
            }
            Message::ConfirmRenameFolder => {
                if let Some((gid, new_label)) = self.folder_rename.take() {
                    let trimmed = new_label.trim();
                    if !trimmed.is_empty()
                        && let Some(group) = self.groups.iter_mut().find(|g| g.id == gid)
                    {
                        group.label = trimmed.to_string();
                        group.updated_at = chrono::Utc::now();
                        if let Some(vault) = &self.vault {
                            let _ = vault.save_group(group);
                        }
                    }
                }
            }
            Message::CancelFolderModal => {
                self.folder_rename = None;
                self.folder_delete = None;
            }
            Message::StartDeleteFolder(gid) => {
                self.overlay = None;
                self.folder_delete = Some(gid);
            }
            Message::DeleteFolderKeepHosts => {
                if let Some(gid) = self.folder_delete.take() {
                    // Move every host inside the folder to the root.
                    for conn in self.connections.iter_mut() {
                        if conn.group_id == Some(gid) {
                            conn.group_id = None;
                            conn.updated_at = chrono::Utc::now();
                            if let Some(vault) = &self.vault {
                                let _ = vault.save_connection(conn, None);
                            }
                        }
                    }
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_group(&gid);
                    }
                    self.groups.retain(|g| g.id != gid);
                    if self.active_group == Some(gid) {
                        self.active_group = None;
                    }
                }
            }
            Message::DeleteFolderWithHosts => {
                if let Some(gid) = self.folder_delete.take() {
                    // Drop every host inside the folder, then the folder.
                    let to_drop: Vec<_> = self
                        .connections
                        .iter()
                        .filter(|c| c.group_id == Some(gid))
                        .map(|c| c.id)
                        .collect();
                    if let Some(vault) = &self.vault {
                        for cid in &to_drop {
                            let _ = vault.delete_connection(cid);
                        }
                        let _ = vault.delete_group(&gid);
                    }
                    self.connections.retain(|c| !to_drop.contains(&c.id));
                    self.groups.retain(|g| g.id != gid);
                    if self.active_group == Some(gid) {
                        self.active_group = None;
                    }
                }
            }
            Message::CloseOtherTabs(idx) => {
                self.overlay = None;
                if idx < self.tabs.len() {
                    let keep = self.tabs.remove(idx);
                    self.tabs.clear();
                    self.tabs.push(keep);
                    self.active_tab = Some(0);
                }
            }
            Message::CloseAllTabs => {
                self.overlay = None;
                self.tabs.clear();
                self.active_tab = None;
                self.active_view = View::Dashboard;
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
