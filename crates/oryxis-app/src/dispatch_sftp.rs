//! `Oryxis::handle_sftp`, match arms for the SFTP pane: navigation,
//! filtering, transfers (upload/download/duplicate), property edits,
//! row interactions, drag-and-drop, edit-in-place. The single biggest
//! domain in the dispatch table.
//!
//! Pane operations are side-addressed: a `SftpPaneSide` (Left / Right)
//! names which pane, and the handler branches on `pane(side).is_remote`
//! to choose filesystem vs SFTP behaviour, so either pane can be Local
//! or a remote host.

#![allow(clippy::result_large_err)]

use iced::futures::SinkExt;
use iced::Task;

use std::sync::Arc;

use oryxis_ssh::SshEngine;

use crate::app::{Message, Oryxis};
use crate::sftp_helpers::{file_basename, parent_path, sort_local_entries, sort_remote_entries};
use crate::state::SftpPaneSide;

/// Stream events from a fresh SFTP connect. `HostKey` surfaces an
/// unknown/changed server key to the shared verification modal mid-connect
/// (the connect blocks until the user answers); `Done` carries the final
/// mounted session or the error.
enum SftpConnectMsg {
    HostKey(oryxis_ssh::HostKeyQuery),
    Done(
        Result<
            (
                Arc<oryxis_ssh::SshSession>,
                oryxis_ssh::SftpClient,
                String,
                Vec<oryxis_ssh::SftpEntry>,
            ),
            String,
        >,
    ),
}

impl Oryxis {
    pub(crate) fn handle_sftp(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::SftpPickHost(idx) => {
                let conn = match self.connections.get(idx).cloned() {
                    Some(c) => c,
                    None => return Ok(Task::none()),
                };
                // The picker connects the host into whichever pane it was
                // opened for.
                let target = self.sftp.picker_target;
                // Always close the picker so the user sees the loading
                // state (or eventual error) on the panes themselves.
                self.sftp.picker_open = false;
                {
                    let pane = self.sftp.pane_mut(target);
                    pane.is_remote = true;
                    pane.host_label = Some(conn.label.clone());
                    pane.remote_loading = true;
                    pane.error = None;
                    pane.remote_entries.clear();
                }

                // Reuse an existing SSH session whenever a terminal tab is
                // already pointed at this host, saves a TCP round-trip
                // and a second auth dance.
                let existing = self.tabs.iter().find_map(|t| {
                    let base = t.label.trim_end_matches(" (disconnected)");
                    if base == conn.label {
                        t.active().ssh_session.clone()
                    } else {
                        None
                    }
                });
                let label = conn.label.clone();
                if let Some(session) = existing {
                    let session_for_task = session.clone();
                    return Ok(Task::perform(
                        async move {
                            let client = session_for_task
                                .open_sftp()
                                .await
                                .map_err(|e| e.to_string())?;
                            let initial = client
                                .canonicalize(".")
                                .await
                                .unwrap_or_else(|_| "/".to_string());
                            let entries = client
                                .list_dir(&initial)
                                .await
                                .map_err(|e| e.to_string())?;
                            Ok::<_, String>((client, initial, entries))
                        },
                        move |result| match result {
                            Ok((client, path, entries)) => Message::SftpHostMounted(
                                target,
                                label.clone(),
                                session.clone(),
                                client,
                                path,
                                entries,
                            ),
                            Err(e) => Message::SftpRemoteError(target, e),
                        },
                    ));
                }

                // No existing tab, open a brand-new SSH session, just
                // for SFTP. Same credential pipeline as Message::ConnectSsh,
                // but without spawning a terminal tab.
                let (password, private_key) = self.resolve_credentials(&conn);
                let resolver = self.make_jump_resolver(&conn);
                let host_key_check = self.make_host_key_check();
                let keepalive = self.effective_keepalive(&conn);

                let connect_to = self.sftp_connect_timeout();
                let auth_to = self.sftp_auth_timeout();
                let session_to = self.sftp_session_timeout();

                // Wire the host-key ask channel so an unknown/changed key
                // prompts the same verification modal the terminal uses
                // instead of being silently TOFU-accepted. The bridge below
                // forwards each query to the modal and waits for the user's
                // answer on `host_key_response_tx` (driven by the shared
                // SshHostKey* handlers).
                let (hk_ask_tx, mut hk_ask_rx) = tokio::sync::mpsc::channel::<(
                    oryxis_ssh::HostKeyQuery,
                    tokio::sync::oneshot::Sender<bool>,
                )>(1);
                let (hk_resp_tx, mut hk_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
                self.host_key_response_tx = Some(hk_resp_tx);

                let stream = iced::stream::channel::<SftpConnectMsg>(
                    8,
                    move |mut sender: iced::futures::channel::mpsc::Sender<SftpConnectMsg>| async move {
                        let engine = SshEngine::new()
                            .with_host_key_check(host_key_check)
                            .with_host_key_ask(hk_ask_tx)
                            .with_keepalive(keepalive)
                            .with_connect_timeout(connect_to)
                            .with_auth_timeout(auth_to)
                            .with_session_timeout(session_to);

                        let mut sender_clone = sender.clone();
                        let _bridge = tokio::spawn(async move {
                            while let Some((query, resp_tx)) = hk_ask_rx.recv().await {
                                let _ = sender_clone.send(SftpConnectMsg::HostKey(query)).await;
                                let accepted = hk_resp_rx.recv().await.unwrap_or(false);
                                let _ = resp_tx.send(accepted);
                            }
                        });

                        let result = async {
                            let (session, _rx) = engine
                                .connect_with_resolver(
                                    &conn,
                                    password.as_deref(),
                                    private_key.as_deref(),
                                    80,
                                    24,
                                    resolver.as_ref(),
                                )
                                .await
                                .map_err(|e| e.to_string())?;
                            let session = Arc::new(session);
                            let client = session.open_sftp().await.map_err(|e| e.to_string())?;
                            let initial = client
                                .canonicalize(".")
                                .await
                                .unwrap_or_else(|_| "/".to_string());
                            let entries =
                                client.list_dir(&initial).await.map_err(|e| e.to_string())?;
                            Ok::<_, String>((session, client, initial, entries))
                        }
                        .await;
                        let _ = sender.send(SftpConnectMsg::Done(result)).await;
                    },
                );
                return Ok(Task::stream(stream).map(move |m| match m {
                    SftpConnectMsg::HostKey(q) => Message::SshHostKeyVerify(q),
                    SftpConnectMsg::Done(Ok((session, client, path, entries))) => {
                        Message::SftpHostMounted(target, label.clone(), session, client, path, entries)
                    }
                    SftpConnectMsg::Done(Err(e)) => Message::SftpRemoteError(target, e),
                }));
            }
            Message::SftpPickLocal => {
                // "Local" is only offered for the left pane. Switch the
                // target pane back to local browsing and refresh.
                let target = self.sftp.picker_target;
                self.sftp.picker_open = false;
                {
                    let pane = self.sftp.pane_mut(target);
                    pane.is_remote = false;
                    pane.session = None;
                    pane.client = None;
                    pane.host_label = None;
                    pane.remote_entries.clear();
                    pane.error = None;
                    if pane.local_path.as_os_str().is_empty() {
                        pane.local_path = std::env::var_os("HOME")
                            .or_else(|| std::env::var_os("USERPROFILE"))
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| std::path::PathBuf::from("/"));
                    }
                }
                self.refresh_sftp_local(target);
            }
            Message::SftpHostMounted(side, label, session, client, path, entries) => {
                // Apply the user-configured op timeout to this fresh
                // client so list_dir/read/write calls respect it.
                client.set_op_timeout(self.sftp_op_timeout());
                let sort = self.sftp.pane(side).sort;
                let mut entries = entries;
                sort_remote_entries(&mut entries, sort);
                let pane = self.sftp.pane_mut(side);
                pane.is_remote = true;
                pane.host_label = Some(label);
                pane.session = Some(session);
                pane.client = Some(client);
                pane.remote_path = path;
                pane.remote_entries = entries;
                pane.remote_loading = false;
                pane.error = None;
            }
            Message::SftpRemoteError(side, msg) => {
                let pane = self.sftp.pane_mut(side);
                pane.remote_loading = false;
                if pane.remote_entries.is_empty() {
                    // Nothing to fall back on (initial connect / first list
                    // failed): take the pane over with the error + retry.
                    pane.error = Some(msg);
                } else {
                    // A navigation/refresh failed but the previous listing
                    // is still valid (e.g. trying to enter a symlink that
                    // points at a file). Keep it on screen and surface the
                    // error as a transient toast instead of wiping the pane.
                    self.toast = Some(msg);
                    return Ok(Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(2600)).await
                        },
                        |_| Message::ToastClear,
                    ));
                }
            }
            Message::SftpCancelRemoteLoad(side) => {
                // Drop the loading visual. The underlying Task::perform
                // can't be aborted (russh-sftp has no cancel token), so
                // a late success will still flow through SftpHostMounted
                // / SftpRemoteLoaded, but at least the user gets the
                // UI back and can retry/pick another host.
                let pane = self.sftp.pane_mut(side);
                pane.remote_loading = false;
                pane.error = Some("Cancelled by user".into());
            }
            Message::SftpRetryRemote(side) => {
                // Three cases the retry button has to cover:
                // 1. Session is mounted (client is Some), just re-list
                //    the current path. Network blip / op-timeout case.
                // 2. Session lost (client is None) but the host label
                //    is still around, re-run the full pick flow for
                //    that host. Connect-failed case.
                // 3. No host label, fall back to the picker.
                if self.sftp.pane(side).client.is_some() {
                    return Ok(Task::done(Message::SftpNavigateRemote(
                        side,
                        self.sftp.pane(side).remote_path.clone(),
                    )));
                }
                if let Some(label) = self.sftp.pane(side).host_label.clone()
                    && let Some(idx) = self
                        .connections
                        .iter()
                        .position(|c| c.label == label)
                {
                    self.sftp.picker_target = side;
                    return Ok(Task::done(Message::SftpPickHost(idx)));
                }
                self.sftp.picker_target = side;
                self.sftp.picker_open = true;
            }
            Message::SftpNavigateRemote(side, path) => {
                let client = match self.sftp.pane(side).client.clone() {
                    Some(c) => c,
                    None => return Ok(Task::none()),
                };
                {
                    let pane = self.sftp.pane_mut(side);
                    pane.remote_loading = true;
                    pane.error = None;
                }
                let target = path.clone();
                return Ok(Task::perform(
                    async move { client.list_dir(&target).await.map_err(|e| e.to_string()) },
                    move |result| match result {
                        Ok(entries) => Message::SftpRemoteLoaded(side, path.clone(), entries),
                        Err(e) => Message::SftpRemoteError(side, e),
                    },
                ));
            }
            Message::SftpRemoteLoaded(side, path, entries) => {
                let sort = self.sftp.pane(side).sort;
                let mut entries = entries;
                sort_remote_entries(&mut entries, sort);
                let pane = self.sftp.pane_mut(side);
                pane.remote_path = path;
                pane.remote_entries = entries;
                pane.remote_loading = false;
                // Selection is path-keyed; navigation invalidates it.
                self.sftp.selected_rows.clear();
                self.sftp.selection_anchor = None;
            }
            Message::SftpUp(side) => {
                if self.sftp.pane(side).is_remote {
                    let parent = parent_path(&self.sftp.pane(side).remote_path);
                    return Ok(Task::done(Message::SftpNavigateRemote(side, parent)));
                }
                if let Some(p) = self.sftp.pane(side).local_path.parent() {
                    let p = p.to_path_buf();
                    self.sftp.pane_mut(side).local_path = p;
                    self.refresh_sftp_local(side);
                }
            }
            Message::SftpNavigateLocal(side, path) => {
                {
                    let pane = self.sftp.pane_mut(side);
                    pane.local_path = path;
                    pane.drives_open = false;
                    pane.actions_open = false;
                }
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.selected_rows.clear();
                self.sftp.selection_anchor = None;
                self.refresh_sftp_local(side);
            }
            Message::SftpRefreshLocal(side) => {
                self.refresh_sftp_local(side);
            }
            Message::SftpOpenPicker(side) => {
                self.sftp.picker_target = side;
                self.sftp.picker_open = true;
                self.sftp.picker_search.clear();
            }
            Message::SftpClosePicker => {
                self.sftp.picker_open = false;
            }
            Message::SftpPickerSearch(s) => {
                self.sftp.picker_search = s;
            }
            Message::SftpToggleHidden(side) => {
                let pane = self.sftp.pane_mut(side);
                pane.show_hidden = !pane.show_hidden;
            }
            Message::SftpFilter(side, s) => {
                self.sftp.pane_mut(side).filter = s;
            }
            Message::SftpToggleActions(side) => {
                let now = !self.sftp.pane(side).actions_open;
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.left.drives_open = false;
                self.sftp.pane_mut(side).actions_open = now;
            }
            Message::SftpToggleDrives(side) => {
                let now = !self.sftp.pane(side).drives_open;
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.left.drives_open = false;
                self.sftp.right.drives_open = false;
                self.sftp.pane_mut(side).drives_open = now;
            }
            Message::SftpCloseMenus => {
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.left.drives_open = false;
                self.sftp.right.drives_open = false;
            }
            Message::SftpStartEditPath(side) => {
                let value = if self.sftp.pane(side).is_remote {
                    self.sftp.pane(side).remote_path.clone()
                } else {
                    self.sftp.pane(side).local_path.display().to_string()
                };
                self.sftp.pane_mut(side).path_editing = Some(value);
            }
            Message::SftpEditPath(side, s) => {
                if self.sftp.pane(side).path_editing.is_some() {
                    self.sftp.pane_mut(side).path_editing = Some(s);
                }
            }
            Message::SftpCommitPath(side) => {
                let Some(input) = self.sftp.pane_mut(side).path_editing.take() else {
                    return Ok(Task::none());
                };
                if self.sftp.pane(side).is_remote {
                    return Ok(Task::done(Message::SftpNavigateRemote(side, input)));
                }
                let p = std::path::PathBuf::from(input);
                if p.is_dir() {
                    self.sftp.pane_mut(side).local_path = p;
                    self.refresh_sftp_local(side);
                } else {
                    self.sftp.pane_mut(side).error =
                        Some(format!("Not a directory: {}", p.display()));
                }
            }
            Message::SftpCancelEditPath => {
                self.sftp.left.path_editing = None;
                self.sftp.right.path_editing = None;
            }
            Message::SftpSort(side, col) => {
                {
                    let pane = self.sftp.pane_mut(side);
                    if pane.sort.column == col {
                        pane.sort.ascending = !pane.sort.ascending;
                    } else {
                        pane.sort.column = col;
                        pane.sort.ascending = true;
                    }
                }
                let sort = self.sftp.pane(side).sort;
                if self.sftp.pane(side).is_remote {
                    sort_remote_entries(&mut self.sftp.pane_mut(side).remote_entries, sort);
                } else {
                    sort_local_entries(&mut self.sftp.pane_mut(side).local_entries, sort);
                }
            }
            Message::SftpRowRightClick(side, path, is_dir) => {
                // If the user right-clicks a row that wasn't part of the
                // current selection, treat the right-click as a fresh
                // single-select, matches Finder/Explorer behaviour and
                // means menu actions never silently target a different
                // set of rows than the visual selection suggests.
                let target = (side, path.clone());
                let in_selection = self.sftp.selected_rows.contains(&target);
                if !in_selection {
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                }
                self.sftp.row_menu = Some(crate::state::SftpRowMenu {
                    side,
                    path,
                    is_dir,
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::SftpRowMenuClose => {
                self.sftp.row_menu = None;
            }
            Message::SftpStartRename(side, path) => {
                self.sftp.row_menu = None;
                let original_path = path.clone();
                let basename = file_basename(&path, self.sftp.pane(side).is_remote);
                self.sftp.rename = Some(crate::state::SftpRename {
                    side,
                    original_path,
                    input: basename,
                });
            }
            Message::SftpRenameInput(s) => {
                if let Some(ref mut r) = self.sftp.rename {
                    r.input = s;
                }
            }
            Message::SftpRenameCommit => {
                let Some(rn) = self.sftp.rename.take() else {
                    return Ok(Task::none());
                };
                let new_name = rn.input.trim().to_string();
                if new_name.is_empty() {
                    return Ok(Task::none());
                }
                if !self.sftp.pane(rn.side).is_remote {
                    let original = std::path::PathBuf::from(&rn.original_path);
                    let parent = original.parent().map(|p| p.to_path_buf());
                    let Some(parent) = parent else {
                        self.sftp.pane_mut(rn.side).error = Some("Cannot rename root".into());
                        return Ok(Task::none());
                    };
                    let dest = parent.join(&new_name);
                    if let Err(e) = std::fs::rename(&original, &dest) {
                        self.sftp.pane_mut(rn.side).error = Some(e.to_string());
                    }
                    self.refresh_sftp_local(rn.side);
                } else {
                    let Some(client) = self.sftp.pane(rn.side).client.clone() else {
                        return Ok(Task::none());
                    };
                    let parent = parent_path(&rn.original_path);
                    let dest = if parent == "/" {
                        format!("/{}", new_name)
                    } else {
                        format!("{}/{}", parent.trim_end_matches('/'), new_name)
                    };
                    let from = rn.original_path;
                    let side = rn.side;
                    let reload_path = self.sftp.pane(side).remote_path.clone();
                    return Ok(Task::perform(
                        async move {
                            client.rename(&from, &dest).await.map_err(|e| e.to_string())
                        },
                        move |result| match result {
                            Ok(()) => Message::SftpNavigateRemote(side, reload_path.clone()),
                            Err(e) => Message::SftpOpResult(side, e, true),
                        },
                    ));
                }
            }
            Message::SftpAskDelete(side, path, is_dir) => {
                self.sftp.row_menu = None;
                self.sftp.delete_confirm = vec![crate::state::SftpDeleteTarget {
                    side,
                    path,
                    is_dir,
                }];
            }
            Message::SftpAskDeleteSelection => {
                self.sftp.row_menu = None;
                let targets: Vec<crate::state::SftpDeleteTarget> = self
                    .sftp
                    .selected_rows
                    .iter()
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
            Message::SftpConfirmDelete => {
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
                        if let Err(e) = result {
                            self.sftp.pane_mut(t.side).error = Some(e.to_string());
                        }
                        if !local_sides.contains(&t.side) {
                            local_sides.push(t.side);
                        }
                    }
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
                            Ok(()) => Message::SftpEntriesRemoved(side, removed_paths.clone()),
                            Err(e) => Message::SftpOpResult(side, e, true),
                        },
                    ));
                }
            }
            Message::SftpCancelDelete => {
                self.sftp.delete_confirm.clear();
            }
            Message::SftpEntriesRemoved(side, paths) => {
                // Drop the just-deleted entries from the listing in place,
                // keeping scroll position and skipping a re-list round trip.
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
            Message::SftpStartNewEntry(side, kind) => {
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.new_entry = Some(crate::state::SftpNewEntry {
                    side,
                    kind,
                    input: String::new(),
                });
            }
            Message::SftpNewEntryInput(s) => {
                if let Some(ref mut e) = self.sftp.new_entry {
                    e.input = s;
                }
            }
            Message::SftpNewEntryCommit => {
                let Some(ne) = self.sftp.new_entry.take() else {
                    return Ok(Task::none());
                };
                let name = ne.input.trim().to_string();
                if name.is_empty() {
                    return Ok(Task::none());
                }
                if !self.sftp.pane(ne.side).is_remote {
                    let target = self.sftp.pane(ne.side).local_path.join(&name);
                    let result = match ne.kind {
                        crate::state::SftpEntryKind::Folder => std::fs::create_dir(&target),
                        crate::state::SftpEntryKind::File => {
                            std::fs::File::create(&target).map(|_| ())
                        }
                    };
                    if let Err(e) = result {
                        self.sftp.pane_mut(ne.side).error = Some(e.to_string());
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
                    return Ok(Task::perform(
                        async move {
                            match kind {
                                crate::state::SftpEntryKind::Folder => {
                                    client.create_dir(&target).await.map_err(|e| e.to_string())
                                }
                                crate::state::SftpEntryKind::File => client
                                    .write_file(&target, b"")
                                    .await
                                    .map_err(|e| e.to_string()),
                            }
                        },
                        move |result| match result {
                            Ok(()) => Message::SftpNavigateRemote(side, reload_path.clone()),
                            Err(e) => Message::SftpOpResult(side, e, true),
                        },
                    ));
                }
            }
            Message::SftpNewEntryCancel => {
                self.sftp.new_entry = None;
            }
            Message::SftpRowEnter(side, path, is_dir) => {
                self.sftp.hovered_row = Some((side, path, is_dir));
                // Promote a pending drag to active once the cursor reaches a
                // row in the *other* pane. This platform doesn't deliver
                // cursor-move events while a mouse button is held, so the
                // distance-threshold promotion in the MouseMoved handler
                // never fires; the row-hover event (which does fire during
                // the hold) is what drives activation here. Activating lights
                // up the destination pane outline as drag feedback.
                if let Some(drag) = self.sftp.drag.as_mut()
                    && !drag.active
                    && drag.origin_side != side
                {
                    drag.active = true;
                }
            }
            Message::SftpRowExit => {
                self.sftp.hovered_row = None;
            }
            Message::SftpMouseLeftPressed => {
                // A physical left press over a tab arms a potential reorder
                // drag. Armed here (on the real button press) rather than in
                // SelectTab, so programmatic SelectTab dispatches (the
                // tab-jump modal, etc.) can't trigger a phantom drag.
                if let Some(idx) = self.hovered_tab
                    && let Some(tab) = self.tabs.get(idx)
                {
                    self.tab_drag = Some(crate::state::TabDrag {
                        from_id: tab._id,
                        start: self.mouse_position,
                        active: false,
                    });
                }
                // Begin a potential internal drag if the cursor is
                // currently on a row in the SFTP view. The drag stays
                // pending (active=false) until the user moves past the
                // threshold, that way plain clicks still flow to the
                // button's on_press handler.
                if self.active_view != crate::state::View::Sftp {
                    return Ok(Task::none());
                }
                let Some((side, path, is_dir)) = self.sftp.hovered_row.clone() else {
                    return Ok(Task::none());
                };
                // Drag the entire same-pane selection if the pressed row
                // is part of it; otherwise drag just this row.
                let same_side: Vec<(String, bool)> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| *s == side)
                    .map(|(_, p)| {
                        let is_dir = self.row_is_dir_in_pane(side, p);
                        (p.clone(), is_dir)
                    })
                    .collect();
                let pressed_in_selection =
                    same_side.iter().any(|(p, _)| p == &path);
                let items: Vec<(String, bool)> = if pressed_in_selection {
                    same_side
                } else {
                    vec![(path.clone(), is_dir)]
                };
                let label = if items.len() > 1 {
                    format!("{} items", items.len())
                } else {
                    path.rsplit(['/', '\\'])
                        .find(|s| !s.is_empty())
                        .unwrap_or(&path)
                        .to_string()
                };
                self.sftp.drag = Some(crate::state::SftpInternalDrag {
                    origin_side: side,
                    items,
                    label,
                    press_pos: self.mouse_position,
                    active: false,
                });
            }
            Message::SftpSelectRow(side, path, is_dir) => {
                let target = (side, path.clone());
                let ctrl = self.modifiers.control() || self.modifiers.command();
                let shift = self.modifiers.shift();
                if shift {
                    // Range select within same pane. If the anchor lives
                    // in the other pane (or doesn't exist), fall through
                    // to a single-select to avoid silent cross-pane jumps.
                    if let Some(anchor) = self.sftp.selection_anchor.clone()
                        && anchor.0 == side
                    {
                        let entries = self.visible_entry_paths_in_pane(side);
                        let a = entries.iter().position(|p| p == &anchor.1);
                        let t = entries.iter().position(|p| p == &path);
                        if let (Some(ai), Some(ti)) = (a, t) {
                            let (lo, hi) = if ai <= ti { (ai, ti) } else { (ti, ai) };
                            self.sftp.selected_rows = entries[lo..=hi]
                                .iter()
                                .map(|p| (side, p.clone()))
                                .collect();
                            return Ok(Task::none());
                        }
                    }
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                } else if ctrl {
                    // Ctrl-click toggle. Anchor follows the click so a
                    // subsequent shift-click extends from here.
                    if let Some(pos) = self
                        .sftp
                        .selected_rows
                        .iter()
                        .position(|x| x == &target)
                    {
                        self.sftp.selected_rows.remove(pos);
                    } else {
                        self.sftp.selected_rows.push(target.clone());
                    }
                    self.sftp.selection_anchor = Some(target);
                } else if is_dir {
                    // Single click selects the folder (so it can be the
                    // type-ahead focus); a quick double click on the same
                    // folder opens it.
                    let now = std::time::Instant::now();
                    let is_double = self.sftp.last_click.as_ref().is_some_and(|(s, p, t)| {
                        *s == side
                            && p == &path
                            && now.duration_since(*t) < std::time::Duration::from_millis(400)
                    });
                    if is_double {
                        self.sftp.last_click = None;
                        self.sftp.selected_rows.clear();
                        self.sftp.selection_anchor = None;
                        return Ok(if self.sftp.pane(side).is_remote {
                            Task::done(Message::SftpNavigateRemote(side, path))
                        } else {
                            Task::done(Message::SftpNavigateLocal(
                                side,
                                std::path::PathBuf::from(path),
                            ))
                        });
                    }
                    self.sftp.last_click = Some((side, path, now));
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                } else {
                    self.sftp.last_click = Some((side, path, std::time::Instant::now()));
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                }
            }
            Message::SftpOpResult(side, msg, is_error) => {
                if is_error {
                    self.sftp.pane_mut(side).error = Some(msg);
                } else {
                    tracing::info!("sftp op: {}", msg);
                }
            }
            Message::KeyboardEvent(ke) => {
                // Type-ahead: while a row is selected in the SFTP view,
                // typing letters jumps the selection to the first entry whose
                // name starts with what's been typed. Only plain printable
                // keys are intercepted here; modifiers, named keys, hotkeys,
                // and typing inside text fields all forward to the terminal
                // handler (which owns that logic) via `Err`.
                if self.active_view != crate::state::View::Sftp {
                    return Err(Message::KeyboardEvent(ke));
                }
                let editing = self.sftp.rename.is_some()
                    || self.sftp.new_entry.is_some()
                    || self.sftp.overwrite_prompt.is_some()
                    || !self.sftp.delete_confirm.is_empty()
                    || self.sftp.properties.is_some()
                    || self.sftp.picker_open
                    || self.sftp.left.path_editing.is_some()
                    || self.sftp.right.path_editing.is_some();
                let ch = if editing {
                    None
                } else if let iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Character(s),
                    modifiers,
                    ..
                } = &ke
                {
                    if modifiers.control() || modifiers.command() || modifiers.alt() {
                        None
                    } else {
                        s.chars().next().filter(|c| !c.is_control())
                    }
                } else {
                    None
                };
                let Some(ch) = ch else {
                    return Err(Message::KeyboardEvent(ke));
                };
                // Type-ahead only kicks in once a row is selected (the
                // selection's pane is the focus).
                if self.sftp.selected_rows.last().is_none() {
                    return Ok(Task::none());
                }
                let now = std::time::Instant::now();
                let elapsed = self
                    .sftp
                    .type_ahead_at
                    .map(|t| now.duration_since(t) > std::time::Duration::from_millis(900))
                    .unwrap_or(true);
                if elapsed {
                    // A pause completes the previous sequence: remember it so
                    // re-typing the same search cycles to the next match.
                    self.sftp.type_ahead_committed = std::mem::take(&mut self.sftp.type_ahead);
                }
                for lc in ch.to_lowercase() {
                    self.sftp.type_ahead.push(lc);
                }
                self.sftp.type_ahead_at = Some(now);
                // Debounce: bump the generation and search only after a short
                // pause, so fast typing ("cla") resolves once with the full
                // buffer instead of jumping on every key (c -> cl -> cla).
                self.sftp.type_ahead_gen = self.sftp.type_ahead_gen.wrapping_add(1);
                let generation = self.sftp.type_ahead_gen;
                return Ok(Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    },
                    move |_| Message::SftpTypeAheadFire(generation),
                ));
            }
            Message::SftpTypeAheadFire(generation) => {
                // A newer keystroke superseded this fire: skip it.
                if generation != self.sftp.type_ahead_gen {
                    return Ok(Task::none());
                }
                let Some(side) = self.sftp.selected_rows.last().map(|(s, _)| *s) else {
                    return Ok(Task::none());
                };
                let prefix = self.sftp.type_ahead.clone();
                if prefix.is_empty() {
                    return Ok(Task::none());
                }
                // Cycle when the sequence matches the previous one (the user
                // re-typed the same search): advance past the current
                // selection instead of restarting at the top.
                let cycle = prefix == self.sftp.type_ahead_committed;

                // Snapshot the displayed entries as (name, full_path) in
                // display order (same hidden + filter rules as the view).
                let (visible, cur_path) = {
                    let pane = self.sftp.pane(side);
                    let filter = pane.filter.to_lowercase();
                    let show_hidden = pane.show_hidden;
                    let cur_path = if pane.is_remote {
                        pane.remote_path.clone()
                    } else {
                        pane.local_path.to_string_lossy().into_owned()
                    };
                    let base_remote = cur_path.trim_end_matches('/').to_string();
                    let raw: Vec<String> = if pane.is_remote {
                        pane.remote_entries.iter().map(|e| e.name.clone()).collect()
                    } else {
                        pane.local_entries.iter().map(|e| e.name.clone()).collect()
                    };
                    let mut visible: Vec<(String, String)> = Vec::new();
                    for n in raw {
                        if !show_hidden && n.starts_with('.') {
                            continue;
                        }
                        if !filter.is_empty() && !n.to_lowercase().contains(&filter) {
                            continue;
                        }
                        let full = if pane.is_remote {
                            if base_remote.is_empty() {
                                format!("/{n}")
                            } else {
                                format!("{base_remote}/{n}")
                            }
                        } else {
                            std::path::Path::new(&cur_path)
                                .join(&n)
                                .to_string_lossy()
                                .into_owned()
                        };
                        visible.push((n, full));
                    }
                    (visible, cur_path)
                };
                let total = visible.len();
                if total == 0 {
                    return Ok(Task::none());
                }
                // Cycling starts just after the current selection; otherwise
                // from the top.
                let start = if cycle {
                    let cur = self.sftp.selected_rows.last().map(|(_, p)| p.clone());
                    cur.and_then(|c| visible.iter().position(|(_, f)| *f == c))
                        .map(|i| i + 1)
                        .unwrap_or(0)
                } else {
                    0
                };
                let Some(idx) = (0..total)
                    .map(|off| (start + off) % total)
                    .find(|&i| visible[i].0.to_lowercase().starts_with(&prefix))
                else {
                    // No match; keep the buffer so the next key extends it.
                    return Ok(Task::none());
                };
                let full = visible[idx].1.clone();
                self.sftp.selected_rows = vec![(side, full.clone())];
                self.sftp.selection_anchor = Some((side, full));
                // Scroll the match into view via the pane's per-directory
                // scroll id (must match the one the view builds).
                let side_key = match side {
                    crate::state::SftpPaneSide::Left => "left",
                    crate::state::SftpPaneSide::Right => "right",
                };
                let scroll_id = format!("sftp-list-{side_key}-{cur_path}");
                let ratio = if total > 1 {
                    idx as f32 / (total - 1) as f32
                } else {
                    0.0
                };
                return Ok(iced::widget::operation::snap_to(
                    iced::widget::Id::from(scroll_id),
                    iced::widget::scrollable::RelativeOffset {
                        x: None,
                        y: Some(ratio),
                    },
                ));
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
