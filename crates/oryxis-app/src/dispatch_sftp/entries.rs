//! Entry mutation arms split out of `dispatch_sftp`: inline rename,
//! delete with its confirm flow, new file / folder creation and the
//! shared op-result logging. Called from `handle_sftp`.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SftpMessage, Message, Oryxis};
use crate::sftp_helpers::file_basename;
use crate::state::SftpPaneSide;

impl Oryxis {
    pub(super) fn handle_sftp_entries(
        &mut self,
        message: SftpMessage,
    ) -> Result<Task<Message>, SftpMessage> {
        match message {
            SftpMessage::SftpStartRename(side, path) => {
                // Rows inside a browsed archive are read-only.
                if self.sftp.pane(side).zip.is_some() {
                    return Ok(Task::none());
                }
                self.sftp.row_menu = None;
                let original_path = path.clone();
                let basename = file_basename(&path, self.sftp.pane(side).is_remote);
                self.sftp.rename = Some(crate::state::SftpRename {
                    side,
                    original_path,
                    input: basename,
                });
                // Drop the keyboard straight into the inline input so the user
                // can type the new name without an extra click.
                return Ok(crate::widgets::focus_input(iced::widget::Id::new(
                    crate::views::sftp::RENAME_INPUT_ID,
                )));
            }
            SftpMessage::SftpRenameInput(s) => {
                if let Some(ref mut r) = self.sftp.rename {
                    r.input = s;
                }
            }
            SftpMessage::SftpRenameCommit => {
                // The Enter that submits this rename also reaches the global
                // keyboard subscription; swallow the row-activation it would
                // otherwise trigger (which re-opens the just-renamed file).
                // Not set on the click-to-commit path (no trailing Enter there).
                self.sftp.swallow_next_activate = true;
                return Ok(self.commit_rename());
            }
            SftpMessage::SftpRenamed(side, reload_path, new_name) => {
                self.push_sftp_log(
                    crate::state::SftpLogLevel::Ok,
                    format!("{} {}", crate::i18n::t("sftp_log_renamed"), new_name),
                );
                return Ok(Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(side, reload_path))));
            }
            SftpMessage::SftpAskDelete(side, path, is_dir) => {
                // Rows inside a browsed archive are read-only.
                if self.sftp.pane(side).zip.is_some() {
                    return Ok(Task::none());
                }
                self.sftp.row_menu = None;
                self.sftp.delete_confirm = vec![crate::state::SftpDeleteTarget {
                    side,
                    path,
                    is_dir,
                }];
            }
            SftpMessage::SftpAskDeleteSelection => {
                self.sftp.row_menu = None;
                let targets: Vec<crate::state::SftpDeleteTarget> = self
                    .sftp
                    .selected_rows
                    .iter()
                    // Rows inside a browsed archive are read-only, the same
                    // gate the single-row ask applies. Filtered rather than
                    // refused wholesale so a selection spanning both panes
                    // still deletes the half that is real, and a selection
                    // entirely inside an archive asks nothing at all
                    // (`targets` ends up empty).
                    .filter(|(side, _)| self.sftp.pane(*side).zip.is_none())
                    .map(|(side, path)| crate::state::SftpDeleteTarget {
                        side: *side,
                        path: path.clone(),
                        is_dir: self.row_is_dir_in_pane(*side, path),
                    })
                    .collect();
                if !targets.is_empty() {
                    self.sftp.delete_confirm = targets;
                }
            }
            SftpMessage::SftpConfirmDelete => {
                let targets = std::mem::take(&mut self.sftp.delete_confirm);
                if targets.is_empty() {
                    return Ok(Task::none());
                }
                // Process local-pane targets synchronously, then fire one
                // chained async task per remote pane that walks remote
                // deletes in series and navigates once at the end. Avoids
                // N parallel navigates racing after a bulk delete.
                let mut local_sides: Vec<SftpPaneSide> = Vec::new();
                let mut remote_targets: Vec<crate::state::SftpDeleteTarget> = Vec::new();
                let mut local_deleted = 0usize;
                for t in targets {
                    if self.sftp.pane(t.side).is_remote {
                        remote_targets.push(t);
                    } else {
                        let path = std::path::PathBuf::from(&t.path);
                        let result = if t.is_dir {
                            std::fs::remove_dir_all(&path)
                        } else {
                            std::fs::remove_file(&path)
                        };
                        match result {
                            Ok(()) => local_deleted += 1,
                            Err(e) => self.sftp.pane_mut(t.side).error = Some(e.to_string()),
                        }
                        if !local_sides.contains(&t.side) {
                            local_sides.push(t.side);
                        }
                    }
                }
                if local_deleted > 0 {
                    self.push_sftp_log(
                        crate::state::SftpLogLevel::Ok,
                        format!(
                            "{} {} {}",
                            crate::i18n::t("sftp_log_deleted"),
                            local_deleted,
                            crate::i18n::t("sftp_log_items"),
                        ),
                    );
                }
                for side in local_sides {
                    self.refresh_sftp_local(side);
                    self.sftp.selected_rows.clear();
                }
                if !remote_targets.is_empty() {
                    // All remote targets share a pane in practice (the
                    // selection is single-pane), so route via the first
                    // target's side.
                    let side = remote_targets[0].side;
                    let Some(client) = self.sftp.pane(side).client.clone() else {
                        return Ok(Task::none());
                    };
                    self.sftp.selected_rows.clear();
                    // Full paths of what we're deleting, so on success we
                    // can drop them from the listing in place instead of
                    // re-listing the whole directory (no network round trip,
                    // no "Loading..." flash).
                    let removed_paths: Vec<String> =
                        remote_targets.iter().map(|t| t.path.clone()).collect();
                    return Ok(Task::perform(
                        async move {
                            for tgt in remote_targets {
                                if tgt.is_dir {
                                    client
                                        .remove_dir_recursive(&tgt.path)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                } else {
                                    client
                                        .remove_file(&tgt.path)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                }
                            }
                            Ok::<(), String>(())
                        },
                        move |r| match r {
                            Ok(()) => Message::Sftp(SftpMessage::SftpEntriesRemoved(side, removed_paths.clone())),
                            Err(e) => Message::Sftp(SftpMessage::SftpOpResult(side, e, true)),
                        },
                    ));
                }
            }
            SftpMessage::SftpCancelDelete => {
                self.sftp.delete_confirm.clear();
            }
            SftpMessage::SftpEntriesRemoved(side, paths) => {
                // Drop the just-deleted entries from the listing in place,
                // keeping scroll position and skipping a re-list round trip.
                if !paths.is_empty() {
                    self.push_sftp_log(
                        crate::state::SftpLogLevel::Ok,
                        format!(
                            "{} {} {}",
                            crate::i18n::t("sftp_log_deleted"),
                            paths.len(),
                            crate::i18n::t("sftp_log_items"),
                        ),
                    );
                }
                let removed: std::collections::HashSet<String> = paths.into_iter().collect();
                let pane = self.sftp.pane_mut(side);
                let base = pane.remote_path.trim_end_matches('/').to_string();
                pane.remote_entries.retain(|e| {
                    let full = if base.is_empty() {
                        format!("/{}", e.name)
                    } else {
                        format!("{}/{}", base, e.name)
                    };
                    !removed.contains(&full)
                });
            }
            SftpMessage::SftpStartNewEntry(side, kind) => {
                // No creating entries inside a browsed archive.
                if self.sftp.pane(side).zip.is_some() {
                    return Ok(Task::none());
                }
                self.sftp.close_menus();
                self.sftp.new_entry = Some(crate::state::SftpNewEntry {
                    side,
                    kind,
                    input: String::new(),
                });
                // Drop the keyboard straight into the name field, so the
                // right-click flows into typing the name. Same courtesy the
                // inline rename above and the sidebar's new-entry row do.
                return Ok(crate::widgets::focus_input(iced::widget::Id::new(
                    crate::views::sftp::NEW_ENTRY_INPUT_ID,
                )));
            }
            SftpMessage::SftpNewEntryInput(s) => {
                if let Some(ref mut e) = self.sftp.new_entry {
                    e.input = s;
                }
            }
            SftpMessage::SftpNewEntryCommit => {
                let Some(ne) = self.sftp.new_entry.take() else {
                    return Ok(Task::none());
                };
                // See SftpRenameCommit: swallow the trailing Enter's activation.
                self.sftp.swallow_next_activate = true;
                let name = ne.input.trim().to_string();
                // One plain path component (rejects empty, ".", ".." and
                // separators), same guard as the rename commit.
                if !crate::sftp_helpers::is_safe_remote_entry_name(&name) {
                    return Ok(Task::none());
                }
                if !self.sftp.pane(ne.side).is_remote {
                    let target = self.sftp.pane(ne.side).local_path.join(&name);
                    let result = match ne.kind {
                        crate::state::SftpEntryKind::Folder => std::fs::create_dir(&target),
                        // create_new: colliding with an existing file must
                        // error out, not truncate it to zero bytes.
                        crate::state::SftpEntryKind::File => std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&target)
                            .map(|_| ()),
                    };
                    if let Err(e) = result {
                        self.sftp.pane_mut(ne.side).error =
                            if e.kind() == std::io::ErrorKind::AlreadyExists {
                                Some(
                                    crate::i18n::t("files_entry_exists")
                                        .replacen("{name}", &name, 1),
                                )
                            } else {
                                Some(e.to_string())
                            };
                    }
                    self.refresh_sftp_local(ne.side);
                } else {
                    let Some(client) = self.sftp.pane(ne.side).client.clone() else {
                        return Ok(Task::none());
                    };
                    let parent = self.sftp.pane(ne.side).remote_path.trim_end_matches('/').to_string();
                    let target = if parent.is_empty() {
                        format!("/{}", name)
                    } else {
                        format!("{}/{}", parent, name)
                    };
                    let kind = ne.kind;
                    let side = ne.side;
                    let reload_path = self.sftp.pane(side).remote_path.clone();
                    let exists_msg = crate::i18n::t("files_entry_exists")
                        .replacen("{name}", &name, 1);
                    return Ok(Task::perform(
                        async move {
                            match kind {
                                crate::state::SftpEntryKind::Folder => {
                                    client.create_dir(&target).await.map_err(|e| e.to_string())
                                }
                                // Exclusive create, so an existing file
                                // errors instead of being truncated to
                                // zero bytes; the stat pre-check turns the
                                // server's opaque EXCL failure into a
                                // readable message (EXCL still closes the
                                // check-to-create race).
                                crate::state::SftpEntryKind::File => {
                                    if client.stat(&target).await.is_ok() {
                                        return Err(exists_msg);
                                    }
                                    client
                                        .create_file_exclusive(&target)
                                        .await
                                        .map_err(|e| e.to_string())
                                }
                            }
                        },
                        move |result| match result {
                            Ok(()) => Message::Sftp(SftpMessage::SftpNavigateRemote(side, reload_path.clone())),
                            Err(e) => Message::Sftp(SftpMessage::SftpOpResult(side, e, true)),
                        },
                    ));
                }
            }
            SftpMessage::SftpNewEntryCancel => {
                self.sftp.new_entry = None;
            }
            SftpMessage::SftpOpResult(side, msg, is_error) => {
                if is_error {
                    self.push_sftp_log(
                        crate::state::SftpLogLevel::Error,
                        format!("{} {}", crate::i18n::t("sftp_log_error"), msg),
                    );
                    self.sftp.pane_mut(side).error = Some(msg);
                } else {
                    self.push_sftp_log(crate::state::SftpLogLevel::Ok, msg.clone());
                    tracing::info!("sftp op: {}", msg);
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
