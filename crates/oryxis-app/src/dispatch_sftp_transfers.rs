//! `Oryxis::handle_sftp_transfers`, match arms for the SFTP transfer
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
    do_relay_item, do_upload_item, parent_path, remote_cp, remote_join, transfer_item_label,
    unique_name_in_local_dir, unique_name_in_remote_dir, walk_local_for_duplicate,
    walk_local_for_upload, walk_remote_for_download, walk_remote_for_relay, UploadOutcome,
    UploadStepOutcome,
};
use crate::state::SftpPaneSide;

impl Oryxis {
    pub(crate) fn handle_sftp_transfers(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        // The remote destination/source side for upload/download. Both
        // paths only run with exactly one remote pane, so this resolves
        // unambiguously. Default to Right for state mutations when no
        // remote pane exists (the early returns below short-circuit).
        let remote_side = self.sftp.remote_side().unwrap_or(SftpPaneSide::Right);
        let local_side = self.sftp.local_side().unwrap_or(SftpPaneSide::Left);
        // Owning SFTP tab for any continuation message this handler emits. For
        // a user-initiated transfer this is the focused tab; for a routed
        // continuation (`route_sftp_async`) it is the originating tab. Captured
        // by the async result closures so the chain stays pinned to one tab.
        let owner = self.current_sftp_owner();
        match message {
            Message::SftpUpload(local_path) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(remote_side).remote_path.clone());
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
                        let target = remote_join(&remote_dir, &basename);
                        client
                            .upload_from(&local_path, &target)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(UploadOutcome::Done(remote_dir))
                    },
                    move |result| match result {
                        Ok(UploadOutcome::Done(reload)) => {
                            Message::SftpNavigateRemote(remote_side, reload)
                        }
                        Ok(UploadOutcome::Conflict(prompt)) => Message::SftpAskOverwrite(prompt),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
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
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
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
                            // Resume the worker pool, set paused false
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
                            vec![Task::done(Message::SftpTransferItemDone(owner, slot))];
                        for _ in 1..slot_count {
                            tasks.push(Task::done(Message::SftpTransferNext(owner)));
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
                            Ok(()) => Message::SftpTransferItemDone(owner, slot),
                            Err(e) => Message::SftpTransferError(owner, e, slot),
                        },
                    )];
                    // Resume the other slots that exited on pause.
                    for _ in 1..slot_count {
                        tasks.push(Task::done(Message::SftpTransferNext(owner)));
                    }
                    return Ok(Task::batch(tasks));
                }
                let reload = prompt.dst_dir.clone();
                return Ok(match action {
                    crate::state::OverwriteAction::Cancel => Task::none(),
                    crate::state::OverwriteAction::ReplaceIfDifferent
                        if prompt.src_size == prompt.dst_size =>
                    {
                        // Same size, assume identical, no-op. The user
                        // explicitly opted into this lazy comparison so
                        // we don't need to hash to be sure.
                        Task::none()
                    }
                    crate::state::OverwriteAction::Replace
                    | crate::state::OverwriteAction::ReplaceIfDifferent => {
                        let target = remote_join(&prompt.dst_dir, &prompt.basename);
                        Task::perform(
                            async move {
                                client
                                    .upload_from(&prompt.src, &target)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                Ok::<String, String>(reload)
                            },
                            move |r| match r {
                                Ok(reload) => Message::SftpNavigateRemote(remote_side, reload),
                                Err(e) => Message::SftpOpResult(remote_side, e, true),
                            },
                        )
                    }
                    crate::state::OverwriteAction::Duplicate => Task::perform(
                        async move {
                            let unique = unique_name_in_remote_dir(
                                &client,
                                &prompt.dst_dir,
                                &prompt.basename,
                            )
                            .await?;
                            let target = remote_join(&prompt.dst_dir, &unique);
                            client
                                .upload_from(&prompt.src, &target)
                                .await
                                .map_err(|e| e.to_string())?;
                            Ok::<String, String>(reload)
                        },
                        move |r| match r {
                            Ok(reload) => Message::SftpNavigateRemote(remote_side, reload),
                            Err(e) => Message::SftpOpResult(remote_side, e, true),
                        },
                    ),
                });
            }
            Message::SftpDownload(remote_path) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(local_side).local_path.clone());
                return Ok(Task::perform(
                    async move {
                        let basename = remote_path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&remote_path)
                            .to_string();
                        let unique = unique_name_in_local_dir(&local_dir, &basename);
                        let target = local_dir.join(&unique);
                        client
                            // Single file: one extra stat is negligible.
                            .download_to(&remote_path, &target, None)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok::<(), String>(())
                    },
                    move |result| match result {
                        Ok(()) => Message::SftpRefreshLocal(local_side),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
                    },
                ));
            }
            Message::SftpDuplicate(side, path) => {
                self.sftp.row_menu = None;
                if !self.sftp.pane(side).is_remote {
                        let src = std::path::PathBuf::from(&path);
                        let parent = match src.parent() {
                            Some(p) => p.to_path_buf(),
                            None => {
                                self.sftp.pane_mut(side).error = Some("Cannot duplicate root".into());
                                return Ok(Task::none());
                            }
                        };
                        let basename = src
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("untitled")
                            .to_string();
                        let unique = unique_name_in_local_dir(&parent, &basename);
                        let dest = parent.join(&unique);
                        // The copy can be multi-GB; run it off the event
                        // loop instead of freezing update() for the
                        // duration, mirroring the remote branch below.
                        return Ok(Task::perform(
                            tokio::task::spawn_blocking(move || std::fs::copy(&src, &dest)),
                            move |res| match res {
                                Ok(Ok(_)) => Message::SftpRefreshLocal(side),
                                Ok(Err(e)) => Message::SftpOpResult(side, format!("copy: {e}"), true),
                                Err(e) => Message::SftpOpResult(side, format!("copy: {e}"), true),
                            },
                        ));
                } else {
                        let Some(client) = self.sftp.pane(side).client.clone() else {
                            return Ok(Task::none());
                        };
                        let parent = parent_path(&path);
                        let basename = path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&path)
                            .to_string();
                        let reload = self.sftp.pane(side).remote_path.clone();
                        let src = path.clone();
                        return Ok(Task::perform(
                            async move {
                                let unique =
                                    unique_name_in_remote_dir(&client, &parent, &basename)
                                        .await?;
                                let dest = remote_join(&parent, &unique);
                                // `cp -- src dst`, same exec channel trick
                                // we used for `rm -rf`. Using -- prevents
                                // dashes in names from being parsed as flags.
                                remote_cp(&client, &src, &dest, false).await?;
                                Ok::<String, String>(reload)
                            },
                            move |result| match result {
                                Ok(reload) => Message::SftpNavigateRemote(side, reload),
                                Err(e) => Message::SftpOpResult(side, e, true),
                            },
                        ));
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
                // hovered row is on the remote pane AND a folder.
                let target_folder = self
                    .sftp
                    .hovered_row
                    .as_ref()
                    .filter(|(s, _, is_dir)| *s == remote_side && *is_dir)
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
                if self.sftp.pane(remote_side).client.is_none() {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
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
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(remote_side).remote_path.clone());
                let concurrency = self.sftp_concurrency();
                return Ok(Task::perform(
                    async move {
                        let basename = local_root
                            .file_name()
                            .and_then(|s| s.to_str())
                            .ok_or_else(|| "invalid folder name".to_string())?
                            .to_string();
                        let unique =
                            unique_name_in_remote_dir(&client, &remote_dir, &basename).await?;
                        let target_root = remote_join(&remote_dir, &unique);
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: local_root.to_string_lossy().into_owned(),
                            dst: target_root.clone(),
                            is_dir: true,
                            size: None,
                        });
                        walk_local_for_upload(&local_root, &target_root, &mut queue)
                            .map_err(|e| e.to_string())?;
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Upload,
                            unique,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(owner, state),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
                    },
                ));
            }
            Message::SftpDownloadFolder(remote_root) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(local_side).local_path.clone());
                let concurrency = self.sftp_concurrency();
                return Ok(Task::perform(
                    async move {
                        let basename = remote_root
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&remote_root)
                            .to_string();
                        // Pick a non-colliding local name.
                        let unique = unique_name_in_local_dir(&local_dir, &basename);
                        let target_root = local_dir.join(&unique);
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: remote_root.clone(),
                            dst: target_root.to_string_lossy().into_owned(),
                            is_dir: true,
                            size: None,
                        });
                        walk_remote_for_download(&client, &remote_root, &target_root, &mut queue)
                            .await?;
                        let clients = build_client_pool(client, concurrency).await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Download,
                            unique,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(owner, state),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
                    },
                ));
            }
            Message::SftpDuplicateFolder(side, path) => {
                self.sftp.row_menu = None;
                if !self.sftp.pane(side).is_remote {
                        let src = std::path::PathBuf::from(&path);
                        let parent = match src.parent() {
                            Some(p) => p.to_path_buf(),
                            None => {
                                self.sftp.pane_mut(side).error = Some("Cannot duplicate root".into());
                                return Ok(Task::none());
                            }
                        };
                        let basename = src
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("untitled")
                            .to_string();
                        let unique = unique_name_in_local_dir(&parent, &basename);
                        let target_root = parent.join(&unique);
                        // Build the queue synchronously, no client needed
                        // for a local-only walk + copy.
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: src.to_string_lossy().into_owned(),
                            dst: target_root.to_string_lossy().into_owned(),
                            is_dir: true,
                            size: None,
                        });
                        if let Err(e) = walk_local_for_duplicate(&src, &target_root, &mut queue) {
                            self.sftp.pane_mut(side).error = Some(e);
                            return Ok(Task::none());
                        }
                        // Local duplicate uses sync std::fs::copy in
                        // the queue runner, no SFTP channels needed,
                        // so the client pool stays empty. Concurrency
                        // is fixed at 1 for the same reason: spawning
                        // multiple sync workers wouldn't help (they'd
                        // hammer the OS file cache from the same
                        // thread).
                        let state = crate::state::TransferState::new(
                            crate::state::TransferKind::DuplicateLocal,
                            unique,
                            queue,
                            Vec::new(),
                            None,
                            None,
                            1,
                        );
                        return Ok(Task::done(Message::SftpTransferQueueReady(owner, state)));
                } else {
                        let Some(client) = self.sftp.pane(side).client.clone() else {
                            return Ok(Task::none());
                        };
                        let parent = parent_path(&path);
                        let basename = path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&path)
                            .to_string();
                        let reload = self.sftp.pane(side).remote_path.clone();
                        let src = path.clone();
                        // `cp -r --`, single fast call, no progress bar
                        // needed since the user can't usefully observe
                        // partial recursive copy progress over SSH anyway.
                        return Ok(Task::perform(
                            async move {
                                let unique =
                                    unique_name_in_remote_dir(&client, &parent, &basename)
                                        .await?;
                                let dest = remote_join(&parent, &unique);
                                remote_cp(&client, &src, &dest, true).await?;
                                Ok::<String, String>(reload)
                            },
                            move |result| match result {
                                Ok(reload) => Message::SftpNavigateRemote(side, reload),
                                Err(e) => Message::SftpOpResult(side, e, true),
                            },
                        ));
                }
            }
            Message::SftpTransferQueueReady(_, state) => {
                let slot_count = state.busy_slots.len().max(1);
                // Fresh transfer: reset the per-file panel log + collapse it.
                self.sftp.transfer_done_log.clear();
                self.sftp.transfer_panel_open = false;
                // Live byte progress: total = sum of known item sizes (0 if
                // unknown, bar falls back to item counts). Use a *fresh*
                // counter rather than resetting the old one, so a lingering
                // worker from a previous/cancelled transfer (whose task may
                // still be draining) can't keep incrementing this transfer's
                // counter and spike the bar to 100% before its first byte.
                self.sftp.transfer_bytes_total =
                    state.queue.iter().filter_map(|i| i.size).sum();
                self.sftp.transfer_bytes_done =
                    std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                self.sftp.transfer = Some(state);
                // Kick off one Next per slot so the worker pool fills
                // up immediately. Each completion will dispatch its
                // own Next to keep the chain going.
                let initial: Vec<Task<Message>> = (0..slot_count)
                    .map(|_| Task::done(Message::SftpTransferNext(owner)))
                    .collect();
                return Ok(Task::batch(initial));
            }
            Message::SftpTransferNext(_) => {
                let Some(transfer) = self.sftp.transfer.as_mut() else {
                    return Ok(Task::none());
                };
                if transfer.paused {
                    // Modal is up, workers idle until the user picks
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
                    // All slots busy, Next dispatch by ItemDone is
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
                        // Relay refreshes its actual destination pane,
                        // which may be the left pane (right-to-left relay),
                        // not the canonical remote (`remote_side`).
                        let relay_dest = transfer.dest_side;
                        self.sftp.transfer = None;
                        return Ok(match kind {
                            crate::state::TransferKind::Relay => {
                                let dst = relay_dest.unwrap_or(remote_side);
                                Task::done(Message::SftpNavigateRemote(
                                    dst,
                                    self.sftp.pane(dst).remote_path.clone(),
                                ))
                            }
                            crate::state::TransferKind::Upload => Task::done(
                                Message::SftpNavigateRemote(
                                    remote_side,
                                    self.sftp.pane(remote_side).remote_path.clone(),
                                ),
                            ),
                            crate::state::TransferKind::Download
                            | crate::state::TransferKind::DuplicateLocal => {
                                self.refresh_sftp_local(local_side);
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
                // Shared live-byte counter the worker increments as chunks
                // move; the tick subscription polls it for the bar.
                let bytes_done = self.sftp.transfer_bytes_done.clone();
                match kind {
                    crate::state::TransferKind::Upload => {
                        let client = transfer.clients[slot as usize].clone();
                        return Ok(Task::perform(
                            do_upload_item(client, item, overwrite_default, multi, Some(bytes_done)),
                            move |r| match r {
                                Ok(UploadStepOutcome::Done) => {
                                    Message::SftpTransferItemDone(owner, slot)
                                }
                                Ok(UploadStepOutcome::Conflict { prompt, item }) => {
                                    Message::SftpTransferConflict(owner, prompt, item, slot)
                                }
                                Err(e) => Message::SftpTransferError(owner, e, slot),
                            },
                        ));
                    }
                    crate::state::TransferKind::Download => {
                        let client = transfer.clients[slot as usize].clone();
                        return Ok(Task::perform(
                            do_download_item(client, item, Some(bytes_done)),
                            move |r| match r {
                                Ok(()) => Message::SftpTransferItemDone(owner, slot),
                                Err(e) => Message::SftpTransferError(owner, e, slot),
                            },
                        ));
                    }
                    crate::state::TransferKind::Relay => {
                        // Source client for the slot, plus the single
                        // dest-host client (relay runs at concurrency 1).
                        let src_client = transfer.clients[slot as usize].clone();
                        let Some(dst_client) = transfer.dest_client.clone() else {
                            return Ok(Task::done(Message::SftpTransferError(
                                owner,
                                "relay: missing destination client".into(),
                                slot,
                            )));
                        };
                        return Ok(Task::perform(
                            do_relay_item(src_client, dst_client, item, Some(bytes_done)),
                            move |r| match r {
                                Ok(()) => Message::SftpTransferItemDone(owner, slot),
                                Err(e) => Message::SftpTransferError(owner, e, slot),
                            },
                        ));
                    }
                    crate::state::TransferKind::DuplicateLocal => {
                        // Sync, no need for an async task.
                        return Ok(match do_local_duplicate_item(&item) {
                            Ok(()) => Task::done(Message::SftpTransferItemDone(owner, slot)),
                            Err(e) => Task::done(Message::SftpTransferError(owner, e, slot)),
                        });
                    }
                }
            }
            Message::SftpTransferItemDone(_, slot) => {
                // Record the finished item's label for the per-file panel.
                // `current` is the label set when this item was dispatched
                // (exact at the relay's concurrency of 1; an approximation
                // at higher concurrency, good enough for a status list).
                let finished = self.sftp.transfer.as_ref().and_then(|t| t.current.clone());
                if let Some(transfer) = self.sftp.transfer.as_mut() {
                    transfer.completed += 1;
                    transfer.current = None;
                    if (slot as usize) < transfer.busy_slots.len() {
                        transfer.busy_slots[slot as usize] = false;
                    }
                }
                if let Some(label) = finished {
                    self.sftp.transfer_done_log.push(label);
                }
                return Ok(Task::done(Message::SftpTransferNext(owner)));
            }
            Message::SftpToggleTransferPanel => {
                self.sftp.transfer_panel_open = !self.sftp.transfer_panel_open;
            }
            // No-op: the redraw it triggers is the point (the bar reads the
            // shared byte counter during view()).
            Message::SftpTransferTick => {}
            Message::SftpTransferConflict(_, prompt, item, slot) => {
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
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(remote_side).remote_path.clone());
                let concurrency = self.sftp_concurrency();
                return Ok(Task::perform(
                    async move {
                        let mut queue = std::collections::VecDeque::new();
                        // Each top-level path goes in as-is; folders
                        // expand recursively. Names aren't pre-uniqued
                        //, the per-item conflict check at the queue
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
                                    size: None,
                                });
                                walk_local_for_upload(path, &target, &mut queue)
                                    .map_err(|e| e.to_string())?;
                            } else {
                                queue.push_back(crate::state::TransferItem {
                                    src: path.to_string_lossy().into_owned(),
                                    dst: target,
                                    is_dir: false,
                                    // Byte size up front so the total is known
                                    // and the bar advances by bytes.
                                    size: path.metadata().map(|m| m.len()).ok(),
                                });
                            }
                        }
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
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Upload,
                            label,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(owner, state),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
                    },
                ));
            }
            Message::SftpUploadSelection => {
                self.sftp.row_menu = None;
                let paths: Vec<std::path::PathBuf> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| !self.sftp.pane(*s).is_remote)
                    .map(|(_, p)| std::path::PathBuf::from(p))
                    .collect();
                if paths.is_empty() {
                    return Ok(Task::none());
                }
                return Ok(Task::done(Message::SftpUploadBatch(paths)));
            }
            Message::SftpDownloadSelection => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
                let remote_items: Vec<(String, bool)> = self
                    .sftp
                    .selected_rows
                    .iter()
                    .filter(|(s, _)| self.sftp.pane(*s).is_remote)
                    .map(|(s, p)| (p.clone(), self.row_is_dir_in_pane(*s, p)))
                    .collect();
                if remote_items.is_empty() {
                    return Ok(Task::none());
                }
                let local_dir = self
                    .sftp
                    .download_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(local_side).local_path.clone());
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
                                    size: None,
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
                                    size: None,
                                });
                            }
                        }
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
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Download,
                            label,
                            queue,
                            clients,
                            None,
                            None,
                            concurrency,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(owner, state),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
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
            Message::SftpTransferError(_, e, _slot) => {
                // Errors abort the whole transfer, the in-flight item
                // failed and we don't try to be clever about retrying
                // siblings (a network blip is likely to nuke them all).
                let kind = self.sftp.transfer.as_ref().map(|t| t.kind);
                let relay_dest = self.sftp.transfer.as_ref().and_then(|t| t.dest_side);
                self.sftp.transfer = None;
                match kind {
                    Some(crate::state::TransferKind::DuplicateLocal) => {
                        self.sftp.pane_mut(local_side).error = Some(e);
                        self.refresh_sftp_local(local_side);
                    }
                    Some(crate::state::TransferKind::Relay) => {
                        let dst = relay_dest.unwrap_or(remote_side);
                        self.sftp.pane_mut(dst).error = Some(e);
                    }
                    _ => {
                        self.sftp.pane_mut(remote_side).error = Some(e);
                    }
                }
            }
            Message::SftpCancelTransfer => {
                let kind = self.sftp.transfer.as_ref().map(|t| t.kind);
                let relay_dest = self.sftp.transfer.as_ref().and_then(|t| t.dest_side);
                self.sftp.transfer = None;
                // The in-flight item can't be aborted mid-byte (russh-sftp
                // doesn't expose a cancel token), but no further items
                // will run, and the user can refresh to see the partial
                // result.
                match kind {
                    Some(crate::state::TransferKind::Relay) => {
                        let dst = relay_dest.unwrap_or(remote_side);
                        return Ok(Task::done(Message::SftpNavigateRemote(
                            dst,
                            self.sftp.pane(dst).remote_path.clone(),
                        )));
                    }
                    Some(crate::state::TransferKind::Upload) => {
                        return Ok(Task::done(Message::SftpNavigateRemote(
                            remote_side,
                            self.sftp.pane(remote_side).remote_path.clone(),
                        )));
                    }
                    Some(_) => {
                        self.refresh_sftp_local(local_side);
                    }
                    None => {}
                }
            }
            Message::SftpRelay(from, src_path) => {
                // Server-to-server single-file transfer: source pane is
                // `from`, destination is the other (also remote) pane.
                self.sftp.row_menu = None;
                let dest_side = if from == SftpPaneSide::Left {
                    SftpPaneSide::Right
                } else {
                    SftpPaneSide::Left
                };
                let (Some(src_client), Some(dst_client)) = (
                    self.sftp.pane(from).client.clone(),
                    self.sftp.pane(dest_side).client.clone(),
                ) else {
                    self.sftp.pane_mut(from).error = Some("Both panes must be connected".into());
                    return Ok(Task::none());
                };
                let dest_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(dest_side).remote_path.clone());
                return Ok(Task::perform(
                    async move {
                        let basename = src_path
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&src_path)
                            .to_string();
                        // Pick a non-colliding name on the destination so
                        // a relay never silently clobbers an existing
                        // file with the same name.
                        let unique =
                            unique_name_in_remote_dir(&dst_client, &dest_dir, &basename).await?;
                        let target = remote_join(&dest_dir, &unique);
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: src_path,
                            dst: target,
                            is_dir: false,
                            size: None,
                        });
                        // Relay runs at concurrency 1: one source client
                        // slot plus the single dest client.
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Relay,
                            basename,
                            queue,
                            vec![src_client],
                            Some(dst_client),
                            Some(dest_side),
                            1,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(owner, state),
                        Err(e) => Message::SftpOpResult(from, e, true),
                    },
                ));
            }
            Message::SftpRelayFolder(from, src_root) => {
                self.sftp.row_menu = None;
                let dest_side = if from == SftpPaneSide::Left {
                    SftpPaneSide::Right
                } else {
                    SftpPaneSide::Left
                };
                let (Some(src_client), Some(dst_client)) = (
                    self.sftp.pane(from).client.clone(),
                    self.sftp.pane(dest_side).client.clone(),
                ) else {
                    self.sftp.pane_mut(from).error = Some("Both panes must be connected".into());
                    return Ok(Task::none());
                };
                let dest_dir = self
                    .sftp
                    .upload_dest_override
                    .take()
                    .unwrap_or_else(|| self.sftp.pane(dest_side).remote_path.clone());
                return Ok(Task::perform(
                    async move {
                        let basename = src_root
                            .rsplit('/')
                            .find(|s| !s.is_empty())
                            .unwrap_or(&src_root)
                            .to_string();
                        let unique =
                            unique_name_in_remote_dir(&dst_client, &dest_dir, &basename).await?;
                        let target_root = remote_join(&dest_dir, &unique);
                        let mut queue = std::collections::VecDeque::new();
                        queue.push_back(crate::state::TransferItem {
                            src: src_root.clone(),
                            dst: target_root.clone(),
                            is_dir: true,
                            size: None,
                        });
                        // Walk the SOURCE remote tree, mapping each entry
                        // onto a destination POSIX path under target_root.
                        walk_remote_for_relay(&src_client, &src_root, &target_root, &mut queue)
                            .await?;
                        Ok::<crate::state::TransferState, String>(crate::state::TransferState::new(
                            crate::state::TransferKind::Relay,
                            unique,
                            queue,
                            vec![src_client],
                            Some(dst_client),
                            Some(dest_side),
                            1,
                        ))
                    },
                    move |result| match result {
                        Ok(state) => Message::SftpTransferQueueReady(owner, state),
                        Err(e) => Message::SftpOpResult(from, e, true),
                    },
                ));
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
