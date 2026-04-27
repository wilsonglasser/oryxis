//! `Oryxis::handle_sftp` — match arms for the SFTP pane: navigation,
//! filtering, transfers (upload/download/duplicate), property edits,
//! row interactions, drag-and-drop, edit-in-place. The single biggest
//! domain in the dispatch table.

#![allow(clippy::result_large_err)]

use iced::Task;

use std::sync::Arc;

use oryxis_ssh::SshEngine;

use crate::app::{Message, Oryxis};
use crate::sftp_helpers::{file_basename, parent_path, sort_local_entries, sort_remote_entries};

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
                // Always close the picker so the user sees the loading
                // state (or eventual error) on the panes themselves.
                self.sftp.picker_open = false;
                self.sftp.host_label = Some(conn.label.clone());
                self.sftp.remote_loading = true;
                self.sftp.remote_error = None;
                self.sftp.remote_entries.clear();

                // Reuse an existing SSH session whenever a terminal tab is
                // already pointed at this host — saves a TCP round-trip
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
                                label.clone(),
                                session.clone(),
                                client,
                                path,
                                entries,
                            ),
                            Err(e) => Message::SftpRemoteError(e),
                        },
                    ));
                }

                // No existing tab — open a brand-new SSH session, just
                // for SFTP. Same credential pipeline as Message::ConnectSsh,
                // but without spawning a terminal tab.
                let (password, private_key) = self.resolve_credentials(&conn);
                let resolver = self.make_jump_resolver(&conn);
                let host_key_check = self.make_host_key_check();
                let keepalive_secs: u64 = self
                    .setting_keepalive_interval
                    .parse()
                    .unwrap_or(0);
                let keepalive = (keepalive_secs > 0)
                    .then(|| std::time::Duration::from_secs(keepalive_secs));

                let connect_to = self.sftp_connect_timeout();
                let auth_to = self.sftp_auth_timeout();
                let session_to = self.sftp_session_timeout();
                return Ok(Task::perform(
                    async move {
                        let engine = SshEngine::new()
                            .with_host_key_check(host_key_check)
                            .with_keepalive(keepalive)
                            .with_connect_timeout(connect_to)
                            .with_auth_timeout(auth_to)
                            .with_session_timeout(session_to);
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
                        let client = session
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
                        Ok::<_, String>((session, client, initial, entries))
                    },
                    move |result| match result {
                        Ok((session, client, path, entries)) => Message::SftpHostMounted(
                            label.clone(),
                            session,
                            client,
                            path,
                            entries,
                        ),
                        Err(e) => Message::SftpRemoteError(e),
                    },
                ));
            }
            Message::SftpHostMounted(label, session, client, path, entries) => {
                self.sftp.host_label = Some(label);
                self.sftp.session = Some(session);
                // Apply the user-configured op timeout to this fresh
                // client so list_dir/read/write calls respect it.
                client.set_op_timeout(self.sftp_op_timeout());
                self.sftp.client = Some(client);
                self.sftp.remote_path = path;
                let mut entries = entries;
                sort_remote_entries(&mut entries, self.sftp.remote_sort);
                self.sftp.remote_entries = entries;
                self.sftp.remote_loading = false;
                self.sftp.remote_error = None;
            }
            Message::SftpRemoteError(msg) => {
                self.sftp.remote_loading = false;
                self.sftp.remote_error = Some(msg);
            }
            Message::SftpCancelRemoteLoad => {
                // Drop the loading visual. The underlying Task::perform
                // can't be aborted (russh-sftp has no cancel token), so
                // a late success will still flow through SftpHostMounted
                // / SftpRemoteLoaded — but at least the user gets the
                // UI back and can retry/pick another host.
                self.sftp.remote_loading = false;
                self.sftp.remote_error = Some("Cancelled by user".into());
            }
            Message::SftpRetryRemote => {
                // Three cases the retry button has to cover:
                // 1. Session is mounted (client is Some) — just re-list
                //    the current path. Network blip / op-timeout case.
                // 2. Session lost (client is None) but the host label
                //    is still around — re-run the full pick flow for
                //    that host. Connect-failed case.
                // 3. No host label — fall back to the picker.
                if self.sftp.client.is_some() {
                    return Ok(Task::done(Message::SftpNavigateRemote(
                        self.sftp.remote_path.clone(),
                    )));
                }
                if let Some(label) = self.sftp.host_label.clone()
                    && let Some(idx) = self
                        .connections
                        .iter()
                        .position(|c| c.label == label)
                {
                    return Ok(Task::done(Message::SftpPickHost(idx)));
                }
                self.sftp.picker_open = true;
            }
            Message::SftpNavigateRemote(path) => {
                let client = match self.sftp.client.clone() {
                    Some(c) => c,
                    None => return Ok(Task::none()),
                };
                self.sftp.remote_loading = true;
                self.sftp.remote_error = None;
                let target = path.clone();
                return Ok(Task::perform(
                    async move { client.list_dir(&target).await.map_err(|e| e.to_string()) },
                    move |result| match result {
                        Ok(entries) => Message::SftpRemoteLoaded(path.clone(), entries),
                        Err(e) => Message::SftpRemoteError(e),
                    },
                ));
            }
            Message::SftpRemoteLoaded(path, entries) => {
                self.sftp.remote_path = path;
                let mut entries = entries;
                sort_remote_entries(&mut entries, self.sftp.remote_sort);
                self.sftp.remote_entries = entries;
                self.sftp.remote_loading = false;
                // Selection is path-keyed; navigation invalidates it.
                self.sftp.selected_rows.clear();
                self.sftp.selection_anchor = None;
            }
            Message::SftpRemoteUp => {
                let parent = parent_path(&self.sftp.remote_path);
                return Ok(Task::done(Message::SftpNavigateRemote(parent)));
            }
            Message::SftpNavigateLocal(path) => {
                self.sftp.local_path = path;
                self.sftp.selected_rows.clear();
                self.sftp.selection_anchor = None;
                self.sftp.local_drives_open = false;
                self.sftp.local_actions_open = false;
                self.sftp.remote_actions_open = false;
                self.refresh_sftp_local();
            }
            Message::SftpLocalUp => {
                if let Some(p) = self.sftp.local_path.parent() {
                    self.sftp.local_path = p.to_path_buf();
                    self.refresh_sftp_local();
                }
            }
            Message::SftpRefreshLocal => {
                self.refresh_sftp_local();
            }
            Message::SftpOpenPicker => {
                self.sftp.picker_open = true;
                self.sftp.picker_search.clear();
            }
            Message::SftpClosePicker => {
                self.sftp.picker_open = false;
            }
            Message::SftpPickerSearch(s) => {
                self.sftp.picker_search = s;
            }
            Message::SftpToggleLocalHidden => {
                self.sftp.local_show_hidden = !self.sftp.local_show_hidden;
            }
            Message::SftpToggleRemoteHidden => {
                self.sftp.remote_show_hidden = !self.sftp.remote_show_hidden;
            }
            Message::SftpLocalFilter(s) => {
                self.sftp.local_filter = s;
            }
            Message::SftpRemoteFilter(s) => {
                self.sftp.remote_filter = s;
            }
            Message::SftpToggleLocalActions => {
                self.sftp.local_actions_open = !self.sftp.local_actions_open;
                self.sftp.remote_actions_open = false;
                self.sftp.local_drives_open = false;
            }
            Message::SftpToggleRemoteActions => {
                self.sftp.remote_actions_open = !self.sftp.remote_actions_open;
                self.sftp.local_actions_open = false;
                self.sftp.local_drives_open = false;
            }
            Message::SftpToggleLocalDrives => {
                self.sftp.local_drives_open = !self.sftp.local_drives_open;
                self.sftp.local_actions_open = false;
                self.sftp.remote_actions_open = false;
            }
            Message::SftpCloseMenus => {
                self.sftp.local_actions_open = false;
                self.sftp.remote_actions_open = false;
                self.sftp.local_drives_open = false;
            }
            Message::SftpStartEditLocalPath => {
                self.sftp.local_path_editing =
                    Some(self.sftp.local_path.display().to_string());
            }
            Message::SftpStartEditRemotePath => {
                self.sftp.remote_path_editing = Some(self.sftp.remote_path.clone());
            }
            Message::SftpEditLocalPath(s) => {
                if self.sftp.local_path_editing.is_some() {
                    self.sftp.local_path_editing = Some(s);
                }
            }
            Message::SftpEditRemotePath(s) => {
                if self.sftp.remote_path_editing.is_some() {
                    self.sftp.remote_path_editing = Some(s);
                }
            }
            Message::SftpCommitLocalPath => {
                if let Some(input) = self.sftp.local_path_editing.take() {
                    let p = std::path::PathBuf::from(input);
                    if p.is_dir() {
                        self.sftp.local_path = p;
                        self.refresh_sftp_local();
                    } else {
                        self.sftp.local_error = Some(format!(
                            "Not a directory: {}",
                            p.display()
                        ));
                    }
                }
            }
            Message::SftpCommitRemotePath => {
                if let Some(target) = self.sftp.remote_path_editing.take() {
                    return Ok(Task::done(Message::SftpNavigateRemote(target)));
                }
            }
            Message::SftpCancelEditPath => {
                self.sftp.local_path_editing = None;
                self.sftp.remote_path_editing = None;
            }
            Message::SftpSortLocal(col) => {
                if self.sftp.local_sort.column == col {
                    self.sftp.local_sort.ascending = !self.sftp.local_sort.ascending;
                } else {
                    self.sftp.local_sort.column = col;
                    self.sftp.local_sort.ascending = true;
                }
                let sort = self.sftp.local_sort;
                sort_local_entries(&mut self.sftp.local_entries, sort);
            }
            Message::SftpSortRemote(col) => {
                if self.sftp.remote_sort.column == col {
                    self.sftp.remote_sort.ascending = !self.sftp.remote_sort.ascending;
                } else {
                    self.sftp.remote_sort.column = col;
                    self.sftp.remote_sort.ascending = true;
                }
                let sort = self.sftp.remote_sort;
                sort_remote_entries(&mut self.sftp.remote_entries, sort);
            }
            Message::SftpRowRightClick(side, path, is_dir) => {
                // If the user right-clicks a row that wasn't part of the
                // current selection, treat the right-click as a fresh
                // single-select — matches Finder/Explorer behaviour and
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
                let basename = file_basename(&path, side);
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
                match rn.side {
                    crate::state::SftpPaneSide::Local => {
                        let original = std::path::PathBuf::from(&rn.original_path);
                        let parent = original.parent().map(|p| p.to_path_buf());
                        let Some(parent) = parent else {
                            self.sftp.local_error = Some("Cannot rename root".into());
                            return Ok(Task::none());
                        };
                        let dest = parent.join(&new_name);
                        if let Err(e) = std::fs::rename(&original, &dest) {
                            self.sftp.local_error = Some(e.to_string());
                        }
                        self.refresh_sftp_local();
                    }
                    crate::state::SftpPaneSide::Remote => {
                        let Some(client) = self.sftp.client.clone() else {
                            return Ok(Task::none());
                        };
                        let parent = parent_path(&rn.original_path);
                        let dest = if parent == "/" {
                            format!("/{}", new_name)
                        } else {
                            format!("{}/{}", parent.trim_end_matches('/'), new_name)
                        };
                        let from = rn.original_path;
                        let reload_path = self.sftp.remote_path.clone();
                        return Ok(Task::perform(
                            async move {
                                client.rename(&from, &dest).await.map_err(|e| e.to_string())
                            },
                            move |result| match result {
                                Ok(()) => Message::SftpNavigateRemote(reload_path.clone()),
                                Err(e) => Message::SftpOpResult(e, true),
                            },
                        ));
                    }
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
                // Process locals synchronously, then fire one chained
                // async task that walks remote deletes in series and
                // navigates once at the end. Avoids N parallel
                // navigates racing each other after a bulk delete.
                let mut local_touched = false;
                let remote_targets: Vec<crate::state::SftpDeleteTarget> = targets
                    .into_iter()
                    .filter(|t| {
                        if t.side == crate::state::SftpPaneSide::Local {
                            let path = std::path::PathBuf::from(&t.path);
                            let result = if t.is_dir {
                                std::fs::remove_dir_all(&path)
                            } else {
                                std::fs::remove_file(&path)
                            };
                            if let Err(e) = result {
                                self.sftp.local_error = Some(e.to_string());
                            }
                            local_touched = true;
                            false
                        } else {
                            true
                        }
                    })
                    .collect();
                if local_touched {
                    self.refresh_sftp_local();
                    self.sftp.selected_rows.clear();
                }
                if !remote_targets.is_empty() {
                    let Some(client) = self.sftp.client.clone() else {
                        return Ok(Task::none());
                    };
                    let reload_path = self.sftp.remote_path.clone();
                    self.sftp.selected_rows.clear();
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
                            Ok::<String, String>(reload_path)
                        },
                        |r| match r {
                            Ok(reload) => Message::SftpNavigateRemote(reload),
                            Err(e) => Message::SftpOpResult(e, true),
                        },
                    ));
                }
            }
            Message::SftpCancelDelete => {
                self.sftp.delete_confirm.clear();
            }
            Message::SftpStartNewEntry(side, kind) => {
                self.sftp.local_actions_open = false;
                self.sftp.remote_actions_open = false;
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
                match ne.side {
                    crate::state::SftpPaneSide::Local => {
                        let target = self.sftp.local_path.join(&name);
                        let result = match ne.kind {
                            crate::state::SftpEntryKind::Folder => std::fs::create_dir(&target),
                            crate::state::SftpEntryKind::File => std::fs::File::create(&target).map(|_| ()),
                        };
                        if let Err(e) = result {
                            self.sftp.local_error = Some(e.to_string());
                        }
                        self.refresh_sftp_local();
                    }
                    crate::state::SftpPaneSide::Remote => {
                        let Some(client) = self.sftp.client.clone() else {
                            return Ok(Task::none());
                        };
                        let parent = self.sftp.remote_path.trim_end_matches('/').to_string();
                        let target = if parent.is_empty() {
                            format!("/{}", name)
                        } else {
                            format!("{}/{}", parent, name)
                        };
                        let kind = ne.kind;
                        let reload_path = self.sftp.remote_path.clone();
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
                                Ok(()) => Message::SftpNavigateRemote(reload_path.clone()),
                                Err(e) => Message::SftpOpResult(e, true),
                            },
                        ));
                    }
                }
            }
            Message::SftpNewEntryCancel => {
                self.sftp.new_entry = None;
            }
            Message::SftpRowEnter(side, path, is_dir) => {
                self.sftp.hovered_row = Some((side, path, is_dir));
            }
            Message::SftpRowExit => {
                self.sftp.hovered_row = None;
            }
            Message::SftpMouseLeftPressed => {
                // Begin a potential internal drag if the cursor is
                // currently on a row in the SFTP view. The drag stays
                // pending (active=false) until the user moves past the
                // threshold — that way plain clicks still flow to the
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
                    // Plain click on a folder still navigates — keeps
                    // the existing one-click-to-enter feel for folders
                    // while files act as selectable items.
                    self.sftp.selected_rows.clear();
                    self.sftp.selection_anchor = None;
                    return Ok(match side {
                        crate::state::SftpPaneSide::Local => Task::done(
                            Message::SftpNavigateLocal(std::path::PathBuf::from(path)),
                        ),
                        crate::state::SftpPaneSide::Remote => {
                            Task::done(Message::SftpNavigateRemote(path))
                        }
                    });
                } else {
                    self.sftp.selected_rows = vec![target.clone()];
                    self.sftp.selection_anchor = Some(target);
                }
            }
            Message::SftpOpResult(msg, is_error) => {
                if is_error {
                    self.sftp.remote_error = Some(msg);
                } else {
                    tracing::info!("sftp op: {}", msg);
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
