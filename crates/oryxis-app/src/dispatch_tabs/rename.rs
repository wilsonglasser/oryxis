//! Renaming a tab or a folder, inline.
//!
//! Same three-step shape for both (start, type, confirm or cancel), with
//! the confirm doing the work: a tab rename becomes a custom name that
//! outranks every automatic label source, and a folder rename has to
//! keep its children pointing at it.

use super::*;

impl Oryxis {
    pub(super) fn handle_tabs_rename(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            TabsMessage::StartRenameTab(idx) => {
                self.overlay = None;
                if let Some(tab) = self.tabs.get(idx) {
                    // Prefill with what the strip currently shows (custom
                    // name, group name or OSC title), minus the state
                    // suffix, so "rename" starts from the visible truth.
                    let auto = self.tab_auto_title(tab);
                    let current = tab
                        .display_label(auto)
                        .trim_end_matches(" (disconnected)")
                        .to_string();
                    self.tab_rename =
                        Some((crate::state::TabRef::Terminal(tab._id), current));
                    // Drop the keyboard straight into the input, mirroring
                    // the SFTP inline rename.
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        crate::views::layout::TAB_RENAME_INPUT_ID,
                    ));
                }
            }
            TabsMessage::StartRenameSftpTab(idx) => {
                self.overlay = None;
                if let Some(tab) = self.sftp_tabs.get(idx) {
                    let current = tab.display_label().to_string();
                    self.tab_rename = Some((crate::state::TabRef::Sftp(tab.id), current));
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        crate::views::layout::TAB_RENAME_INPUT_ID,
                    ));
                }
            }
            TabsMessage::TabRenameInput(val) => {
                if let Some((_, ref mut buf)) = self.tab_rename {
                    *buf = val;
                }
            }
            TabsMessage::ConfirmTabRename => {
                if let Some((tab_ref, name)) = self.tab_rename.take() {
                    let trimmed = name.trim();
                    // Empty clears the custom name: the automatic label
                    // (host / group / OSC title) takes over again.
                    let new_name =
                        (!trimmed.is_empty()).then(|| trimmed.to_string());
                    match tab_ref {
                        crate::state::TabRef::Terminal(id) => {
                            if let Some(tab) =
                                self.tabs.iter_mut().find(|t| t._id == id)
                            {
                                tab.custom_name = new_name;
                            }
                        }
                        crate::state::TabRef::Sftp(id) => {
                            if let Some(tab) =
                                self.sftp_tabs.iter_mut().find(|t| t.id == id)
                            {
                                tab.custom_name = new_name;
                            }
                        }
                        // Not renameable: it has no per-tab identity to
                        // name, and the rename entry is never offered for
                        // it. Reachable only if some future surface starts
                        // a rename on it, which should do nothing.
                        crate::state::TabRef::Panel(_) => {}
                    }
                }
            }
            TabsMessage::CancelTabRename => {
                self.tab_rename = None;
            }
            TabsMessage::ShowFolderActions(gid) => {
                // Anchor the menu to the cursor, matches the host-card
                // "..." pattern. The global MouseMoved subscription keeps
                // `mouse_position` fresh.
                let anchor = self.keynav_take_menu_anchor();
                self.overlay = Some(OverlayState {
                    content: OverlayContent::FolderActions(gid),
                    x: anchor.0,
                    y: anchor.1,
                });
            }
            TabsMessage::StartRenameFolder(gid) => {
                self.overlay = None;
                let current = self
                    .groups
                    .iter()
                    .find(|g| g.id == gid)
                    .map(|g| g.label.clone())
                    .unwrap_or_default();
                self.folder_rename = Some((gid, current));
            }
            TabsMessage::FolderRenameInput(val) => {
                if let Some((_, ref mut buf)) = self.folder_rename {
                    *buf = val;
                }
            }
            TabsMessage::ConfirmRenameFolder => {
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
            TabsMessage::CancelFolderModal => {
                self.folder_rename = None;
                self.close_modal(crate::state::Modal::FolderDelete);
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
