//! `Oryxis::handle_sftp_transfers` — match arms for the SFTP transfer
//! pipeline: single + batch + folder uploads/downloads/duplicates,
//! conflict resolution, OS-level file drop, queue lifecycle (slots,
//! retry, error reporting, cancel). Pulled out of `dispatch_sftp.rs`
//! since the queue runner is genuinely a different subsystem from the
//! navigation/listing arms.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{Message, Oryxis};
use crate::sftp_helpers::{
    apply_overwrite_for_item, build_client_pool, do_download_item, do_local_duplicate_item,
    do_upload_item, parent_path, remote_join, transfer_item_label, unique_entry_name,
    walk_local_for_duplicate, walk_local_for_upload, walk_remote_for_download, UploadOutcome,
    UploadStepOutcome,
};

impl Oryxis {
    pub(crate) fn handle_sftp_transfers(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::SftpUpload(local_path) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.client.clone() else {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.remote_path.clone());
                return Ok(Task::perform(
                    async move {
                        let basename = local_path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .ok_or_else(|| "invalid filename".to_string())?
                            .to_string();
                        let entries = client
                            .list_dir(&remote_dir)
                            .await
                            .map_err(|e| e.to_string())?;
                        // Existence check up front: hand back to the
                        // user via overwrite modal if the name is taken,
                        // otherwise stream the file and finish silently.
                        let conflict = entries.iter().find(|e| e.name == basename);
                        if let Some(existing) = conflict {
                            let src_size = tokio::fs::metadata(&local_path)
                                .await
                                .map(|m| m.len())
                                .unwrap_or(0);
                            return Ok::<UploadOutcome, String>(UploadOutcome::Conflict(
                                crate::state::OverwritePrompt {
                                    src: local_path,
                                    dst_dir: remote_dir,
                                    basename,
                                    src_size,
                                    dst_size: existing.size,
                                    multi: false,
                                    apply_to_all: false,
                                },
                            ));
                        }
                        let bytes = tokio::fs::read(&local_path)
                            .await
                            .map_err(|e| format!("read local: {e}"))?;
                        let target = remote_join(&remote_dir, &basename);
                        client
                            .write_file(&target, &bytes)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(UploadOutcome::Done(remote_dir))
                    },
                    |result| match result {
                        Ok(UploadOutcome::Done(reload)) => Message::SftpNavigateRemote(reload),
                        Ok(UploadOutcome::Conflict(prompt)) => Message::SftpAskOverwrite(prompt),
                        Err(e) => Message::SftpOpResult(e, true),
                    },
                ));
            }
            Message::SftpAskOverwrite(prompt) => {
                self.sftp.overwrite_prompt = Some(prompt);
            }
            Message::SftpToggleApplyToAll => {
                if let Some(p) = self.sftp.overwrite_prompt.as_mut() {
                    p.apply_to_all = !p.apply_to_all;
                }
            }
            Message::SftpResolveOverwrite(action) => {
                let Some(prompt) = self.sftp.overwrite_prompt.take() else {
                    return Ok(Task::none());
                };
                let apply_to_all = prompt.apply_to_all;
                let Some(client) = self.sftp.client.clone() else {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                // Pull a parked transfer item if this prompt fired from
                // inside a queue runner. Two distinct flows hang off
                // here: standalone single-file conflict, and in-transfer
                // multi-file conflict with sticky decisions.
                let (pending_item, pending_slot, slot_count) =
                    self.sftp.transfer.as_mut().map_or(
                        (None, None, 0usize),
                        |t| {
                            if apply_to_all {
                                t.overwrite_default = Some(action);
                            }
                            // Resume the worker pool — set paused false
                            // so the resume Next dispatches succeed.
                            t.paused = false;
                            (
                                t.pending_conflict_item.take(),
                                t.pending_conflict_slot.take(),
                                t.busy_slots.len(),
                            )
                        },
                    );
                if let Some(item) = pending_item {
                    if matches!(action, crate::state::OverwriteAction::Cancel) {
                        // Cancel skips this item; with apply-to-all it
                        // also drops the rest of the queue so the user
                        // doesn't keep getting prompted.
                        if apply_to_all
                            && let Some(t) = self.sftp.transfer.as_mut()
                        {
                            t.queue.clear();
                        }
                        let slot = pending_slot.unwrap_or(0);
                        // Free slot bookkeeping handled by ItemDone.
                        // Also kick a Next per other slot so the rest
                        // of the workers resume from pause.
                        let mut tasks =
                            vec![Task::done(Message::SftpTransferItemDone(slot))];
                        for _ in 1..slot_count {
                            tasks.push(Task::done(Message::SftpTransferNext));
                        }
                        return Ok(Task::batch(tasks));
                    }
                    let slot = pending_slot.unwrap_or(0);
                    // Use the slot's own SFTP client for the apply
                    // step; falls back to the original navigation
                    // client only if the slot index is somehow stale.
                    let client = self
                        .sftp
                        .transfer
                        .as_ref()
                        .and_then(|t| t.clients.get(slot as usize).cloned())
                        .unwrap_or(client);
                    if let Some(t) = self.sftp.transfer.as_mut()
                        && (slot as usize) < t.busy_slots.len()
                    {
                        t.busy_slots[slot as usize] = true;
                    }
                    let mut tasks = vec![Task::perform(
                        apply_overwrite_for_item(client, item, action),
                        move |r| match r {
                            Ok(()) => Message::SftpTransferItemDone(slot),
                            Err(e) => Message::SftpTransferError(e, slot),
                        },
                    )];
                    // Resume the other slots that exited on pause.
                    for _ in 1..slot_count {
                        tasks.push(Task::done(Message::SftpTransferNext));
                    }
                    return Ok(Task::batch(tasks));
                }
                let reload = prompt.dst_dir.clone();
                return Ok(match action {
                    crate::state::OverwriteAction::Cancel => Task::none(),
                    crate::state::OverwriteAction::ReplaceIfDifferent
                        if prompt.src_size == prompt.dst_size =>
                    {
                        // Same size — assume identical, no-op. The user
                        // explicitly opted into this lazy comparison so
                        // we don't need to hash to be sure.
                        Task::none()
                    }
                    crate::state::OverwriteAction::Replace
                    | crate::state::OverwriteAction::ReplaceIfDifferent => {
                        let target = remote_join(&prompt.dst_dir, &prompt.basename);
                        Task::perform(
                            async move {
                                let bytes = tokio::fs::read(&prompt.src)
                                    .await
                                    .map_err(|e| format!("read local: {e}"))?;
                                client
                                    .write_file(&target, &bytes)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                Ok::<String, String>(reload)
                            },
                            |r| match r {
                                Ok(reload) => Message::SftpNavigateRemote(reload),
                                Err(e) => Message::SftpOpResult(e, true),
                            },
                        )
                    }
                    crate::state::OverwriteAction::Duplicate => Task::perform(
                        async move {
                            let entries = client
                                .list_dir(&prompt.dst_dir)
                                .await
                                .map_err(|e| e.to_string())?;
                            let names: std::collections::HashSet<String> =
                                entries.into_iter().map(|e| e.name).collect();
                            let unique =
                                unique_entry_name(&prompt.basename, |n| !names.contains(n));
                            let target = remote_join(&prompt.dst_dir, &unique);
                            let bytes = tokio::fs::read(&prompt.src)
                                .await
                                .map_err(|e| format!("read local: {e}"))?;
                            client
                                .write_file(&target, &bytes)
                                .await
                                .map_err(|e| e.to_string())?;
                            Ok::<String, String>(reload)
                        },
                        |r| match r {
                            Ok(reload) => Message::SftpNavigateRemote(reload),
                            Err(e) => Message::SftpOpResult(e, true),
                        },
                    ),
                });
            }
            Message::SftpDownload(remote_path) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.client.clone() else {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.local_path.clone());
                return Ok(Task::perform(
                    async move {
                        let basename = remote_path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&remote_path)
                            .to_string();
                        let bytes = client
                            .read_file(&remote_path)
                            .await
                            .map_err(|e| e.to_string())?;
                        let mut listing = Vec::new();
                        if let Ok(rd) = std::fs::read_dir(&local_dir) {
                            for entry in rd.flatten() {
                                if let Some(n) = entry.file_name().to_str() {
                                    listing.push(n.to_string());
                                }
                            }
                        }
                        let names: std::collections::HashSet<String> =
                            listing.into_iter().collect();
                        let unique = unique_entry_name(&basename, |n| !names.contains(n));
                        let target = local_dir.join(&unique);
                        tokio::fs::write(&target, &bytes)
                            .await
                            .map_err(|e| format!("write local: {e}"))?;
                        Ok::<(), String>(())
                    },
                    |result| match result {
                        Ok(()) => Message::SftpRefreshLocal,
                        Err(e) => Message::SftpOpResult(e, true),
                    },
                ));
            }
            Message::SftpDuplicate(side, path) => {
                self.sftp.row_menu = None;
                match side {
                    crate::state::SftpPaneSide::Local => {
                        let src = std::path::PathBuf::from(&path);
                        let parent = match src.parent() {
                            Some(p) => p.to_path_buf(),
                            None => {
                                self.sftp.local_error = Some("Cannot duplicate root".into());
                                return Ok(Task::none());
                            }
                        };
                        let basename = src
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("untitled")
                            .to_string();
                        let mut listing = std::collections::HashSet::new();
                        if let Ok(rd) = std::fs::read_dir(&parent) {
                            for entry in rd.flatten() {
                                if let Some(n) = entry.file_name().to_str() {
                                    listing.insert(n.to_string());
                                }
                            }
                        }
                        let unique = unique_entry_name(&basename, |n| !listing.contains(n));
                        let dest = parent.join(&unique);
                        if let Err(e) = std::fs::copy(&src, &dest) {
                            self.sftp.local_error = Some(format!("copy: {e}"));
                        }
                        self.refresh_sftp_local();
                    }
                    crate::state::SftpPaneSide::Remote => {
                        let Some(client) = self.sftp.client.clone() else {
                            return Ok(Task::none());
                        };
                        let parent = parent_path(&path);
                        let basename = path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&path)
                            .to_string();
                        let reload = self.sftp.remote_path.clone();
                        let src = path.clone();
                        return Ok(Task::perform(
                            async move {
                                let entries = client
                                    .list_dir(&parent)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                let names: std::collections::HashSet<String> =
                                    entries.into_iter().map(|e| e.name).collect();
                                let unique =
                                    unique_entry_name(&basename, |n| !names.contains(n));
                                let dest = if parent == "/" {
                                    format!("/{}", unique)
                                } else {
                                    format!("{}/{}", parent.trim_end_matches('/'), unique)
                                };
                                // `cp -- src dst` — same exec channel trick
                                // we used for `rm -rf`. Using -- prevents
                                // dashes in names from being parsed as flags.
                                let escaped_src = src.replace('\'', "'\\''");
                                let escaped_dst = dest.replace('\'', "'\\''");
                                let cmd =
                                    format!("cp -- '{}' '{}'", escaped_src, escaped_dst);
                                let (code, _out, err) =
                                    client.exec(&cmd).await.map_err(|e| e.to_string())?;
                                if code == 0 {
                                    Ok::<String, String>(reload)
                                } else {
                                    let msg = err.trim();
                                    Err(if msg.is_empty() {
                                        format!("cp exited {code}")
                                    } else {
                                        msg.to_string()
                                    })
                                }
                            },
                            |result| match result {
                                Ok(reload) => Message::SftpNavigateRemote(reload),
                                Err(e) => Message::SftpOpResult(e, true),
                            },
                        ));
                    }
                }
            }
            Message::SftpFileHovered => {
                self.sftp.drop_active = true;
            }
            Message::SftpFilesHoveredLeft => {
                self.sftp.drop_active = false;
            }
            Message::SftpFileDropped(path) => {
                let was_active = self.sftp.drop_active;
                // OS drops only land in a remote folder when the
                // hovered row is on the remote side AND a folder.
                let target_folder = self
                    .sftp
                    .hovered_row
                    .as_ref()
                    .filter(|(s, _, is_dir)| *s == crate::state::SftpPaneSide::Remote && *is_dir)
                    .map(|(_, p, _)| p.clone());
                self.sftp.drop_active = false;
                if !was_active || self.active_view != crate::state::View::Sftp {
                    return Ok(Task::none());
                }
                let in_remote_pane =
                    target_folder.is_some() || self.is_cursor_over_remote_pane();
                if !in_remote_pane {
                    return Ok(Task::none());
                }
                if self.sftp.client.is_none() {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                }
                // The upload handlers below consume `upload_dest_override`
                // before falling back to `remote_path`. Drops onto a
                // specific folder row land inside that folder; otherwise
                // the active remote dir is used.
                self.sftp.upload_dest_override = target_folder;
                return Ok(if path.is_dir() {
                    Task::done(Message::SftpUploadFolder(path))
                } else {
                    Task::done(Message::SftpUpload(path))
                });
            }
            Message::SftpUploadFolder(local_root) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.client.clone() else {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.remote_path.clone());
                let concurrency = self.sftp_concurrency();
                return Ok(Task::perform(
                    async move {
                        let basename = local_root
                            .file_name()
                            .and_then(|s| s.to_str())
                            .ok_or_else(|| "invalid folder name".to_string())?
                            .to_string();
                        let entries = client
                            .list_dir(&remote_dir)
                            .await
                            .map_err(|e| e.to_string())?;
                        let names: std::collections::HashSet<String> =
                            entries.into_iter().map(|e| e.name).collect();
                        let unique = unique_entry_name(&basename, |n| !names.contains(n));
                        let target_root = if remote_dir == "/" {
                            format!("/{}", unique)
                        } else {
                            format!("{}/{}", remote_dir.trim_end_matches('/'), unique)
                        };
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: local_root.to_string_lossy().into_owned(),
                            dst: target_root.clone(),
                            is_dir: true,
                        });
                        walk_local_for_upload(&local_root, &target_root, &mut queue)
                            .map_err(|e| e.to_string())?;
                        let total = queue.len();
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState {
                            kind: crate::state::TransferKind::Upload,
                            root_label: unique,
                            queue,
                            current: None,
                            completed: 0,
                            total,
                            overwrite_default: None,
                            pending_conflict_item: None,
                            pending_conflict_slot: None,
                            clients,
                            busy_slots: vec![false; concurrency as usize],
                            paused: false,
                        })
                    },
                    |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(state),
                        Err(e) => Message::SftpOpResult(e, true),
                    },
                ));
            }
            Message::SftpDownloadFolder(remote_root) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.client.clone() else {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.local_path.clone());
                let concurrency = self.sftp_concurrency();
                return Ok(Task::perform(
                    async move {
                        let basename = remote_root
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&remote_root)
                            .to_string();
                        // Pick a non-colliding local name.
                        let mut existing = std::collections::HashSet::new();
                        if let Ok(rd) = std::fs::read_dir(&local_dir) {
                            for entry in rd.flatten() {
                                if let Some(n) = entry.file_name().to_str() {
                                    existing.insert(n.to_string());
                                }
                            }
                        }
                        let unique = unique_entry_name(&basename, |n| !existing.contains(n));
                        let target_root = local_dir.join(&unique);
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: remote_root.clone(),
                            dst: target_root.to_string_lossy().into_owned(),
                            is_dir: true,
                        });
                        walk_remote_for_download(&client, &remote_root, &target_root, &mut queue)
                            .await?;
                        let total = queue.len();
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState {
                            kind: crate::state::TransferKind::Download,
                            root_label: unique,
                            queue,
                            current: None,
                            completed: 0,
                            total,
                            overwrite_default: None,
                            pending_conflict_item: None,
                            pending_conflict_slot: None,
                            clients,
                            busy_slots: vec![false; concurrency as usize],
                            paused: false,
                        })
                    },
                    |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(state),
                        Err(e) => Message::SftpOpResult(e, true),
                    },
                ));
            }
            Message::SftpDuplicateFolder(side, path) => {
                self.sftp.row_menu = None;
                match side {
                    crate::state::SftpPaneSide::Local => {
                        let src = std::path::PathBuf::from(&path);
                        let parent = match src.parent() {
                            Some(p) => p.to_path_buf(),
                            None => {
                                self.sftp.local_error = Some("Cannot duplicate root".into());
                                return Ok(Task::none());
                            }
                        };
                        let basename = src
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("untitled")
                            .to_string();
                        let mut existing = std::collections::HashSet::new();
                        if let Ok(rd) = std::fs::read_dir(&parent) {
                            for entry in rd.flatten() {
                                if let Some(n) = entry.file_name().to_str() {
                                    existing.insert(n.to_string());
                                }
                            }
                        }
                        let unique = unique_entry_name(&basename, |n| !existing.contains(n));
                        let target_root = parent.join(&unique);
                        // Build the queue synchronously — no client needed
                        // for a local-only walk + copy.
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: src.to_string_lossy().into_owned(),
                            dst: target_root.to_string_lossy().into_owned(),
                            is_dir: true,
                        });
                        if let Err(e) = walk_local_for_duplicate(&src, &target_root, &mut queue) {
                            self.sftp.local_error = Some(e);
                            return Ok(Task::none());
                        }
                        let total = queue.len();
                        // Local duplicate uses sync std::fs::copy in
                        // the queue runner — no SFTP channels needed,
                        // so the client pool stays empty. Concurrency
                        // is fixed at 1 for the same reason: spawning
                        // multiple sync workers wouldn't help (they'd
                        // hammer the OS file cache from the same
                        // thread).
                        let state = crate::state::TransferState {
                            kind: crate::state::TransferKind::DuplicateLocal,
                            root_label: unique,
                            queue,
                            current: None,
                            completed: 0,
                            total,
                            overwrite_default: None,
                            pending_conflict_item: None,
                            pending_conflict_slot: None,
                            clients: Vec::new(),
                            busy_slots: vec![false; 1],
                            paused: false,
                        };
                        return Ok(Task::done(Message::SftpTransferQueueReady(state)));
                    }
                    crate::state::SftpPaneSide::Remote => {
                        let Some(client) = self.sftp.client.clone() else {
                            return Ok(Task::none());
                        };
                        let parent = parent_path(&path);
                        let basename = path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&path)
                            .to_string();
                        let reload = self.sftp.remote_path.clone();
                        let src = path.clone();
                        // `cp -r --` — single fast call, no progress bar
                        // needed since the user can't usefully observe
                        // partial recursive copy progress over SSH anyway.
                        return Ok(Task::perform(
                            async move {
                                let entries = client
                                    .list_dir(&parent)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                let names: std::collections::HashSet<String> =
                                    entries.into_iter().map(|e| e.name).collect();
                                let unique =
                                    unique_entry_name(&basename, |n| !names.contains(n));
                                let dest = if parent == "/" {
                                    format!("/{}", unique)
                                } else {
                                    format!("{}/{}", parent.trim_end_matches('/'), unique)
                                };
                                let escaped_src = src.replace('\'', "'\\''");
                                let escaped_dst = dest.replace('\'', "'\\''");
                                let cmd = format!(
                                    "cp -r -- '{}' '{}'",
                                    escaped_src, escaped_dst
                                );
                                let (code, _out, err) =
                                    client.exec(&cmd).await.map_err(|e| e.to_string())?;
                                if code == 0 {
                                    Ok::<String, String>(reload)
                                } else {
                                    let msg = err.trim();
                                    Err(if msg.is_empty() {
                                        format!("cp -r exited {code}")
                                    } else {
                                        msg.to_string()
                                    })
                                }
                            },
                            |result| match result {
                                Ok(reload) => Message::SftpNavigateRemote(reload),
                                Err(e) => Message::SftpOpResult(e, true),
                            },
                        ));
                    }
                }
            }
            Message::SftpTransferQueueReady(state) => {
                let slot_count = state.busy_slots.len().max(1);
                self.sftp.transfer = Some(state);
                // Kick off one Next per slot so the worker pool fills
                // up immediately. Each completion will dispatch its
                // own Next to keep the chain going.
                let initial: Vec<Task<Message>> = (0..slot_count)
                    .map(|_| Task::done(Message::SftpTransferNext))
                    .collect();
                return Ok(Task::batch(initial));
            }
            Message::SftpTransferNext => {
                let Some(transfer) = self.sftp.transfer.as_mut() else {
                    return Ok(Task::none());
                };
                if transfer.paused {
                    // Modal is up — workers idle until the user picks
                    // an action. Resolve will re-dispatch Next for
                    // each slot then.
                    return Ok(Task::none());
                }
                let Some(slot) = transfer
                    .busy_slots
                    .iter()
                    .position(|b| !b)
                    .map(|i| i as u8)
                else {
                    // All slots busy — Next dispatch by ItemDone is
                    // ahead of an already-busy slot. Drop it; the
                    // next ItemDone will free a slot.
                    return Ok(Task::none());
                };
                let Some(item) = transfer.queue.pop_front() else {
                    // Queue exhausted. If every slot is idle, finalize
                    // and refresh; otherwise wait for in-flight slots
                    // to drain.
                    if transfer.busy_slots.iter().all(|b| !b) {
                        let kind = transfer.kind;
                        self.sftp.transfer = None;
                        return Ok(match kind {
                            crate::state::TransferKind::Upload => Task::done(
                                Message::SftpNavigateRemote(self.sftp.remote_path.clone()),
                            ),
                            crate::state::TransferKind::Download
                            | crate::state::TransferKind::DuplicateLocal => {
                                self.refresh_sftp_local();
                                Task::none()
                            }
                        });
                    }
                    return Ok(Task::none());
                };
                transfer.busy_slots[slot as usize] = true;
                transfer.current = Some(transfer_item_label(&item));
                let kind = transfer.kind;
                let overwrite_default = transfer.overwrite_default;
                let multi = transfer.total > 1;
                match kind {
                    crate::state::TransferKind::Upload => {
                        let client = transfer.clients[slot as usize].clone();
                        return Ok(Task::perform(
                            do_upload_item(client, item, overwrite_default, multi),
                            move |r| match r {
                                Ok(UploadStepOutcome::Done) => {
                                    Message::SftpTransferItemDone(slot)
                                }
                                Ok(UploadStepOutcome::Conflict { prompt, item }) => {
                                    Message::SftpTransferConflict(prompt, item, slot)
                                }
                                Err(e) => Message::SftpTransferError(e, slot),
                            },
                        ));
                    }
                    crate::state::TransferKind::Download => {
                        let client = transfer.clients[slot as usize].clone();
                        return Ok(Task::perform(
                            do_download_item(client, item),
                            move |r| match r {
                                Ok(()) => Message::SftpTransferItemDone(slot),
                                Err(e) => Message::SftpTransferError(e, slot),
                            },
                        ));
                    }
                    crate::state::TransferKind::DuplicateLocal => {
                        // Sync — no need for an async task.
                        return Ok(match do_local_duplicate_item(&item) {
                            Ok(()) => Task::done(Message::SftpTransferItemDone(slot)),
                            Err(e) => Task::done(Message::SftpTransferError(e, slot)),
                        });
                    }
                }
            }
            Message::SftpTransferItemDone(slot) => {
                if let Some(transfer) = self.sftp.transfer.as_mut() {
                    transfer.completed += 1;
                    transfer.current = None;
                    if (slot as usize) < transfer.busy_slots.len() {
                        transfer.busy_slots[slot as usize] = false;
                    }
                }
                return Ok(Task::done(Message::SftpTransferNext));
            }
            Message::SftpTransferConflict(prompt, item, slot) => {
                // Park the popped item alongside the prompt so the
                // resolve handler knows which destination the user is
                // about to act on. The queue stays stalled here until
                // the modal is answered.
                if let Some(transfer) = self.sftp.transfer.as_mut() {
                    transfer.pending_conflict_item = Some(item);
                    transfer.pending_conflict_slot = Some(slot);
                    transfer.paused = true;
                    if (slot as usize) < transfer.busy_slots.len() {
                        transfer.busy_slots[slot as usize] = false;
                    }
                }
                self.sftp.overwrite_prompt = Some(prompt);
            }
            Message::SftpUploadBatch(paths) => {
                self.sftp.row_menu = None;
                if paths.is_empty() {
                    return Ok(Task::none());
                }
                let Some(client) = self.sftp.client.clone() else {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.remote_path.clone());
                let concurrency = self.sftp_concurrency();
                return Ok(Task::perform(
                    async move {
                        let mut queue = std::collections::VecDeque::new();
                        // Each top-level path goes in as-is; folders
                        // expand recursively. Names aren't pre-uniqued
                        // — the per-item conflict check at the queue
                        // runner handles that with user input.
                        for path in &paths {
                            let basename = path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("file")
                                .to_string();
                            let target = if remote_dir == "/" {
                                format!("/{}", basename)
                            } else {
                                format!(
                                    "{}/{}",
                                    remote_dir.trim_end_matches('/'),
                                    basename
                                )
                            };
                            if path.is_dir() {
                                queue.push_back(crate::state::TransferItem {
                                    src: path.to_string_lossy().into_owned(),
                                    dst: target.clone(),
                                    is_dir: true,
                                });
                                walk_local_for_upload(path, &target, &mut queue)
                                    .map_err(|e| e.to_string())?;
                            } else {
                                queue.push_back(crate::state::TransferItem {
                                    src: path.to_string_lossy().into_owned(),
                                    dst: target,
                                    is_dir: false,
                                });
                            }
                        }
                        let total = queue.len();
                        let label = if paths.len() == 1 {
                            paths[0]
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("upload")
                                .to_string()
                        } else {
                            format!("{} items", paths.len())
                        };
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState {
                            kind: crate::state::TransferKind::Upload,
                            root_label: label,
                            queue,
                            current: None,
                            completed: 0,
                            total,
                            overwrite_default: None,
                            pending_conflict_item: None,
                            pending_conflict_slot: None,
                            clients,
                            busy_slots: vec![false; concurrency as usize],
                            paused: false,
                        })
                    },
                    |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(state),
                        Err(e) => Message::SftpOpResult(e, true),
                    },
                ));
            }
            Message::SftpUploadSelection => {
                self.sftp.row_menu = None;
                let paths: Vec<std::path::PathBuf> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| *s == crate::state::SftpPaneSide::Local)
                    .map(|(_, p)| std::path::PathBuf::from(p))
                    .collect();
                if paths.is_empty() {
                    return Ok(Task::none());
                }
                return Ok(Task::done(Message::SftpUploadBatch(paths)));
            }
            Message::SftpDownloadSelection => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.client.clone() else {
                    self.sftp.remote_error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_items: Vec<(String, bool)> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| *s == crate::state::SftpPaneSide::Remote)
                    .map(|(_, p)| (p.clone(), self.row_is_dir_in_pane(crate::state::SftpPaneSide::Remote, p)))
                    .collect();
                if remote_items.is_empty() {
                    return Ok(Task::none());
                }
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.local_path.clone());
                let concurrency = self.sftp_concurrency();
                return Ok(Task::perform(
                    async move {
                        let mut queue = std::collections::VecDeque::new();
                        for (remote_path, is_dir) in &remote_items {
                            let basename = remote_path
                                .rsplit('/')
                                .find(|s| !s.is_empty())
                                .unwrap_or(remote_path)
                                .to_string();
                            let target = local_dir.join(&basename);
                            if *is_dir {
                                queue.push_back(crate::state::TransferItem {
                                    src: remote_path.clone(),
                                    dst: target.to_string_lossy().into_owned(),
                                    is_dir: true,
                                });
                                walk_remote_for_download(
                                    &client,
                                    remote_path,
                                    &target,
                                    &mut queue,
                                )
                                .await?;
                            } else {
                                queue.push_back(crate::state::TransferItem {
                                    src: remote_path.clone(),
                                    dst: target.to_string_lossy().into_owned(),
                                    is_dir: false,
                                });
                            }
                        }
                        let total = queue.len();
                        let label = if remote_items.len() == 1 {
                            remote_items[0]
                                .0
                                .rsplit('/')
                                .find(|s| !s.is_empty())
                                .unwrap_or(&remote_items[0].0)
                                .to_string()
                        } else {
                            format!("{} items", remote_items.len())
                        };
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState {
                            kind: crate::state::TransferKind::Download,
                            root_label: label,
                            queue,
                            current: None,
                            completed: 0,
                            total,
                            overwrite_default: None,
                            pending_conflict_item: None,
                            pending_conflict_slot: None,
                            clients,
                            busy_slots: vec![false; concurrency as usize],
                            paused: false,
                        })
                    },
                    |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(state),
                        Err(e) => Message::SftpOpResult(e, true),
                    },
                ));
            }
            Message::SftpDuplicateSelection => {
                self.sftp.row_menu = None;
                // Fan out per-item duplicate. They run sequentially
                // anyway because the SFTP connection serializes; for
                // local-side they're independent fs::copy calls.
                let items: Vec<(crate::state::SftpPaneSide, String, bool)> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .map(|(side, path)| (*side, path.clone(), self.row_is_dir_in_pane(*side, path)))
                    .collect();
                if items.is_empty() {
                    return Ok(Task::none());
                }
                let mut tasks = Vec::with_capacity(items.len());
                for (side, path, is_dir) in items {
                    tasks.push(Task::done(if is_dir {
                        Message::SftpDuplicateFolder(side, path)
                    } else {
                        Message::SftpDuplicate(side, path)
                    }));
                }
                self.sftp.selected_rows.clear();
                return Ok(Task::batch(tasks));
            }
            Message::SftpTransferError(e, _slot) => {
                // Errors abort the whole transfer — the in-flight item
                // failed and we don't try to be clever about retrying
                // siblings (a network blip is likely to nuke them all).
                let kind = self.sftp.transfer.as_ref().map(|t| t.kind);
                self.sftp.transfer = None;
                match kind {
                    Some(crate::state::TransferKind::DuplicateLocal) => {
                        self.sftp.local_error = Some(e);
                        self.refresh_sftp_local();
                    }
                    _ => {
                        self.sftp.remote_error = Some(e);
                    }
                }
            }
            Message::SftpCancelTransfer => {
                let kind = self.sftp.transfer.as_ref().map(|t| t.kind);
                self.sftp.transfer = None;
                // The in-flight item can't be aborted mid-byte (russh-sftp
                // doesn't expose a cancel token), but no further items
                // will run, and the user can refresh to see the partial
                // result.
                match kind {
                    Some(crate::state::TransferKind::Upload) => {
                        return Ok(Task::done(Message::SftpNavigateRemote(
                            self.sftp.remote_path.clone(),
                        )));
                    }
                    Some(_) => {
                        self.refresh_sftp_local();
                    }
                    None => {}
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
