//! Host groups: create, nest, edit, and delete.
//!
//! Deleting is the largest thing here for a reason: a folder with hosts
//! has two answers (keep them, promoting to the parent, or take them
//! with it), and both have to leave the tree consistent. Sub-groups
//! nest to any depth, so a parent change is a re-parent of a subtree.

use super::*;

impl Oryxis {
    pub(super) fn handle_tabs_groups(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            TabsMessage::EditGroup(gid) => {
                self.overlay = None;
                if let Some(group) = self.groups.iter().find(|g| g.id == gid) {
                    self.group_edit.id = Some(gid);
                    self.group_edit.label = group.label.clone();
                    self.group_edit.icon = group.icon.clone().unwrap_or_default();
                    self.group_edit.color = group.color.clone().unwrap_or_default();
                    // Resolve the stored parent id back to its full
                    // breadcrumb path for the combo (what the picker
                    // displays); a dangling id (deleted parent) shows
                    // as root, matching how the grid renders it.
                    self.group_edit.parent_label = group
                        .parent_id
                        .filter(|pid| self.groups.iter().any(|g| g.id == *pid))
                        .map(|pid| oryxis_core::models::Group::path_of(&self.groups, pid))
                        .unwrap_or_default();
                    self.hydrate_group_defaults_form(gid);
                    self.group_edit.visible = true;
                    // Mutually exclusive with the other right-hand panels.
                    self.editor_flush_pending();
                    self.panels.host_panel = false;
                    // Drop what the host editor's eyes revealed.
                    self.editor_form.sweep_secrets();
                    self.panel_nav_clear();
                    self.panels.session_group_panel = false;
                }
            }
            TabsMessage::NewSubgroup(gid) => {
                self.overlay = None;
                let parent_label = Some(oryxis_core::models::Group::path_of(&self.groups, gid));
                if let Some(parent_label) = parent_label {
                    self.group_edit = crate::state::GroupEditForm {
                        visible: true,
                        id: None,
                        label: String::new(),
                        icon: String::new(),
                        color: String::new(),
                        parent_label,
                        ..Default::default()
                    };
                    // Mutually exclusive with the other right-hand panels.
                    self.editor_flush_pending();
                    self.panels.host_panel = false;
                    // Drop what the host editor's eyes revealed.
                    self.editor_form.sweep_secrets();
                    self.panel_nav_clear();
                    self.panels.session_group_panel = false;
                }
            }
            TabsMessage::NewGroup => {
                self.overlay = None;
                // Create a fresh top-level folder: empty parent = root.
                // Symmetric counterpart to "New subgroup", so an empty
                // group can be born from the add menu instead of only by
                // typing a new name in the host editor's group combo.
                self.group_edit = crate::state::GroupEditForm {
                    visible: true,
                    id: None,
                    label: String::new(),
                    icon: String::new(),
                    color: String::new(),
                    parent_label: String::new(),
                    ..Default::default()
                };
                // Mutually exclusive with the other right-hand panels.
                self.editor_flush_pending();
                self.panels.host_panel = false;
                // Drop what the host editor's eyes revealed.
                self.editor_form.sweep_secrets();
                self.panel_nav_clear();
                self.panels.session_group_panel = false;
            }
            TabsMessage::GroupEditLabelChanged(v) => {
                self.group_edit.label = v;
            }
            TabsMessage::GroupEditParentChanged(v) => {
                self.group_edit.parent_label = v;
            }
            TabsMessage::GroupEditToggleDefaults => {
                self.group_edit.defaults_open = !self.group_edit.defaults_open;
            }
            TabsMessage::GroupEditDefaultUsername(v) => {
                self.group_edit.username = v;
            }
            TabsMessage::GroupEditDefaultPort(v) => {
                // Digits only, same guard the host editor's port field
                // uses: a typo must not be storable.
                self.group_edit.port = v.chars().filter(|c| c.is_ascii_digit()).collect();
            }
            TabsMessage::GroupEditDefaultIdentity(v) => {
                self.group_edit.identity_label = v;
            }
            TabsMessage::GroupEditDefaultProxyIdentity(v) => {
                self.group_edit.proxy_identity_label = v;
            }
            TabsMessage::GroupEditDefaultTheme(v) => {
                self.group_edit.terminal_theme = v;
            }
            TabsMessage::GroupEditDefaultSnippet(v) => {
                self.group_edit.startup_snippet_label = v;
            }
            TabsMessage::GroupEditEnvAdd => {
                self.group_edit.env_vars.push(
                    oryxis_core::models::connection::EnvVar {
                        key: String::new(),
                        value: String::new(),
                    },
                );
            }
            TabsMessage::GroupEditEnvRemove(idx) => {
                if idx < self.group_edit.env_vars.len() {
                    self.group_edit.env_vars.remove(idx);
                }
            }
            TabsMessage::GroupEditEnvKey(idx, v) => {
                if let Some(var) = self.group_edit.env_vars.get_mut(idx) {
                    var.key = v;
                }
            }
            TabsMessage::GroupEditEnvValue(idx, v) => {
                if let Some(var) = self.group_edit.env_vars.get_mut(idx) {
                    var.value = v;
                }
            }
            TabsMessage::ShowGroupEditIconPicker => {
                self.icon_picker.icon = if self.group_edit.icon.is_empty() {
                    None
                } else {
                    Some(self.group_edit.icon.clone())
                };
                self.icon_picker.color = if self.group_edit.color.is_empty() {
                    None
                } else {
                    Some(self.group_edit.color.clone())
                };
                self.icon_picker.hex_input = self.group_edit.color.clone();
                self.icon_picker.for_id = None;
                self.icon_picker.for_session_group = false;
                self.icon_picker.for_group_edit = true;
                self.icon_picker.for_local_terminal = false;
                self.panels.icon_picker = true;
            }
            TabsMessage::SaveGroupEdit => {
                let trimmed = self.group_edit.label.trim().to_string();
                if !trimmed.is_empty() {
                    // Resolve the parent combo by label, mirroring the
                    // dynamic-group editor: empty / unmatched = root and
                    // only a manual folder qualifies as a container. The
                    // edited group's own subtree is excluded so a save
                    // can never mint a parent cycle (nesting a folder
                    // under its own descendant would orphan the subtree).
                    let excluded = self
                        .group_edit
                        .id
                        .map(|gid| oryxis_core::models::Group::subtree_ids(&self.groups, gid))
                        .unwrap_or_default();
                    // Full-path match first (what the picker fills in),
                    // bare label as the typed-by-hand fallback.
                    let parent_id = oryxis_core::models::Group::resolve_path_or_label(
                        &self.groups,
                        &self.group_edit.parent_label,
                        &excluded,
                    );
                    let icon = if self.group_edit.icon.is_empty() {
                        None
                    } else {
                        Some(self.group_edit.icon.clone())
                    };
                    let color = if self.group_edit.color.is_empty() {
                        None
                    } else {
                        Some(self.group_edit.color.clone())
                    };
                    // Labels resolve to ids HERE, against the lists as
                    // they are at save time, so a picker left pointing
                    // at something the user deleted meanwhile stores
                    // nothing rather than a dangling id.
                    let defaults = self.group_edit_defaults();
                    if let Some(gid) = self.group_edit.id {
                        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
                            group.label = trimmed;
                            group.icon = icon;
                            group.color = color;
                            group.parent_id = parent_id;
                            group.defaults = defaults;
                            group.updated_at = chrono::Utc::now();
                            if let Some(vault) = &self.vault {
                                let _ = vault.save_group(group);
                            }
                        }
                    } else {
                        // Create mode (the folder kebab's "New subgroup").
                        // Reuse an existing manual folder with the same
                        // (label, parent) instead of minting a
                        // byte-identical duplicate: two "New subgroup"s
                        // with the same name under one parent would
                        // otherwise produce two groups with identical
                        // breadcrumb paths, the second unselectable
                        // (combos resolve first-match-wins) and an
                        // indistinguishable duplicate card. Reuse mirrors
                        // the host editor's find-or-create semantics; the
                        // user's icon / colour edits are folded onto the
                        // existing folder so the save isn't a silent
                        // no-op. Navigation is intentionally left alone,
                        // matching the fresh-create branch below.
                        let dup = self
                            .groups
                            .iter()
                            .find(|g| g.parent_id == parent_id && g.label == trimmed)
                            .map(|g| g.id);
                        if let Some(gid) = dup {
                            if let Some(group) =
                                self.groups.iter_mut().find(|g| g.id == gid)
                            {
                                group.icon = icon;
                                group.color = color;
                                group.defaults = defaults;
                                group.updated_at = chrono::Utc::now();
                                if let Some(vault) = &self.vault {
                                    let _ = vault.save_group(group);
                                }
                            }
                        } else {
                            let mut group = oryxis_core::models::Group::new(trimmed);
                            group.icon = icon;
                            group.color = color;
                            group.parent_id = parent_id;
                            group.defaults = defaults;
                            if let Some(vault) = &self.vault {
                                let _ = vault.save_group(&group);
                            }
                            self.groups.push(group);
                        }
                    }
                }
                self.group_edit.visible = false;
                self.group_edit.id = None;
            }
            TabsMessage::CancelGroupEdit => {
                self.group_edit.visible = false;
                self.group_edit.id = None;
            }
            TabsMessage::StartDeleteFolder(gid) => {
                self.overlay = None;
                self.folder_delete = Some(gid);
            }
            TabsMessage::DeleteFolderKeepHosts => {
                if let Some(gid) = self.folder_delete {
                    // Deleting a folder promotes its contents one level
                    // up (to the deleted folder's own parent; root for a
                    // top-level folder, which preserves the pre-subgroup
                    // behavior).
                    let new_parent = self
                        .groups
                        .iter()
                        .find(|g| g.id == gid)
                        .and_then(|g| g.parent_id);
                    // Track any vault write failure across the re-home
                    // passes. A silently dropped Result here could leave
                    // a host / subgroup pointing at a group we then
                    // tombstone, stranding it (renders nowhere at root).
                    // So we surface failures and skip the final delete
                    // unless every child was re-homed successfully.
                    let mut write_failed = false;
                    for conn in self.connections.iter_mut() {
                        if conn.group_id == Some(gid) {
                            conn.group_id = new_parent;
                            conn.updated_at = chrono::Utc::now();
                            if let Some(vault) = &self.vault
                                && let Err(e) = vault.save_connection(conn, None)
                            {
                                tracing::error!(
                                    "delete folder {gid}: failed to re-home host {}: {e}",
                                    conn.id
                                );
                                write_failed = true;
                            }
                        }
                    }
                    // Re-home nested sub-groups (manual subgroups and
                    // ECS / K8s dynamic groups alike), so they don't
                    // dangle off the deleted parent and vanish from
                    // every view.
                    for g in self.groups.iter_mut() {
                        if g.parent_id == Some(gid) {
                            g.parent_id = new_parent;
                            g.updated_at = chrono::Utc::now();
                            if let Some(vault) = &self.vault
                                && let Err(e) = vault.save_group(g)
                            {
                                tracing::error!(
                                    "delete folder {gid}: failed to re-home subgroup {}: {e}",
                                    g.id
                                );
                                write_failed = true;
                            }
                        }
                    }
                    // Only tombstone the folder once every child was
                    // re-homed. On failure, abort with a toast and leave
                    // the folder in place; the user can retry rather than
                    // be left with orphaned hosts.
                    let mut removed = false;
                    if write_failed {
                        self.set_toast(crate::i18n::t("folder_delete_failed").to_string());
                    } else if let Some(vault) = &self.vault {
                        if let Err(e) = vault.delete_group(&gid) {
                            tracing::error!("delete folder {gid}: failed to delete group: {e}");
                            self.set_toast(
                                crate::i18n::t("folder_delete_failed").to_string(),
                            );
                        } else {
                            removed = true;
                        }
                    } else {
                        // No vault (should not happen for a saved
                        // folder), keep the in-memory removal consistent.
                        removed = true;
                    }
                    if removed {
                        self.groups.retain(|g| g.id != gid);
                        if self.active_group == Some(gid) {
                            self.active_group = new_parent;
                        }
                        // Don't leave the editor panel open on a deleted row.
                        if self.group_edit.id == Some(gid) {
                            self.group_edit.visible = false;
                            self.group_edit.id = None;
                        }
                    }
                    self.close_modal(crate::state::Modal::FolderDelete);
                }
            }
            TabsMessage::DeleteFolderWithHosts => {
                if let Some(gid) = self.folder_delete {
                    // Drop every host inside the folder, then the folder.
                    let to_drop: Vec<_> = self
                        .connections
                        .iter()
                        .filter(|c| c.group_id == Some(gid))
                        .map(|c| c.id)
                        .collect();
                    // Nested sub-groups (manual subgroups and dynamic
                    // ECS / K8s groups) aren't "hosts": promote them to
                    // the deleted folder's own parent rather than
                    // deleting them with the folder, so an import isn't
                    // silently lost and nothing dangles off the removed
                    // parent.
                    let new_parent = self
                        .groups
                        .iter()
                        .find(|g| g.id == gid)
                        .and_then(|g| g.parent_id);
                    // Track vault write failures across the re-home and
                    // host-drop passes. A silently dropped Result could
                    // leave a subgroup (re-home failed) or a still-live
                    // host (delete failed) pointing at a group we then
                    // tombstone, stranding it at root. Skip the final
                    // group delete unless every write succeeded.
                    let mut write_failed = false;
                    for g in self.groups.iter_mut() {
                        if g.parent_id == Some(gid) {
                            g.parent_id = new_parent;
                            g.updated_at = chrono::Utc::now();
                            if let Some(vault) = &self.vault
                                && let Err(e) = vault.save_group(g)
                            {
                                tracing::error!(
                                    "delete folder {gid}: failed to re-home subgroup {}: {e}",
                                    g.id
                                );
                                write_failed = true;
                            }
                        }
                    }
                    let mut dropped: Vec<uuid::Uuid> = Vec::new();
                    if let Some(vault) = &self.vault {
                        for cid in &to_drop {
                            if let Err(e) = vault.delete_connection(cid) {
                                tracing::error!(
                                    "delete folder {gid}: failed to delete host {cid}: {e}"
                                );
                                write_failed = true;
                            } else {
                                // Saved AI conversations reference the host
                                // by id; sweep them with it.
                                let _ = vault.delete_chat_conversations_for_connection(cid);
                                dropped.push(*cid);
                            }
                        }
                    } else {
                        dropped = to_drop.clone();
                    }
                    // Drop from memory only the hosts actually removed
                    // from the vault, so a failed delete doesn't vanish
                    // the row while its record survives on disk.
                    self.connections.retain(|c| !dropped.contains(&c.id));
                    // Only tombstone the folder once every child write
                    // landed. On failure, abort with a toast and keep the
                    // folder so nothing is stranded; the user can retry.
                    let mut removed = false;
                    if write_failed {
                        self.set_toast(crate::i18n::t("folder_delete_failed").to_string());
                    } else if let Some(vault) = &self.vault {
                        if let Err(e) = vault.delete_group(&gid) {
                            tracing::error!("delete folder {gid}: failed to delete group: {e}");
                            self.set_toast(
                                crate::i18n::t("folder_delete_failed").to_string(),
                            );
                        } else {
                            removed = true;
                        }
                    } else {
                        removed = true;
                    }
                    if removed {
                        self.groups.retain(|g| g.id != gid);
                        if self.active_group == Some(gid) {
                            self.active_group = new_parent;
                        }
                        // Don't leave the editor panel open on a deleted row.
                        if self.group_edit.id == Some(gid) {
                            self.group_edit.visible = false;
                            self.group_edit.id = None;
                        }
                    }
                    self.close_modal(crate::state::Modal::FolderDelete);
                }
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
