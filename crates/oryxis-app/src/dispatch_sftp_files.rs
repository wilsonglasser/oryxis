//! `Oryxis::handle_sftp_files`, match arms for per-file SFTP
//! operations: chmod-style Properties dialog and edit-in-place
//! (download to temp, open in OS editor, mtime-watch + auto-upload).
//! Pulled out of `dispatch_sftp.rs` to keep that file focused on
//! navigation/listing.

#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{Message, Oryxis};

impl Oryxis {
    pub(crate) fn handle_sftp_files(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        use crate::state::SftpPaneSide;
        // Edit-in-place is a remote-pane operation; resolve the remote
        // side for error reporting / reload. Defaults to Right when no
        // remote pane exists (the early returns guard real use).
        let remote_side = self.sftp.remote_side().unwrap_or(SftpPaneSide::Right);
        let local_side = self.sftp.local_side().unwrap_or(SftpPaneSide::Left);
        match message {
            Message::SftpShowProperties(side, path, is_dir) => {
                self.sftp.row_menu = None;
                if !self.sftp.pane(side).is_remote {
                        // Local stat is sync, populate the modal in
                        // place. Permissions on Windows are coarser so
                        // Apply will refuse to chmod there (the dialog
                        // still shows file info).
                        let p = std::path::Path::new(&path);
                        let meta = match std::fs::metadata(p) {
                            Ok(m) => m,
                            Err(e) => {
                                self.sftp.pane_mut(side).error = Some(e.to_string());
                                return Ok(Task::none());
                            }
                        };
                        #[cfg(unix)]
                        let mode = {
                            use std::os::unix::fs::MetadataExt as _;
                            meta.mode()
                        };
                        #[cfg(not(unix))]
                        let mode = if meta.permissions().readonly() {
                            0o444
                        } else {
                            0o644
                        };
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as u32);
                        #[cfg(unix)]
                        let (uid, gid) = {
                            use std::os::unix::fs::MetadataExt as _;
                            (Some(meta.uid()), Some(meta.gid()))
                        };
                        #[cfg(not(unix))]
                        let (uid, gid) = (None, None);
                        let view = crate::state::PropertiesView {
                            side,
                            path,
                            is_dir,
                            size: meta.len(),
                            mtime,
                            owner_uid: uid,
                            owner_gid: gid,
                            original_mode: mode,
                            bits: crate::state::PermBits::from_mode(mode),
                            mode_input: format!("{:03o}", mode & 0o777),
                            applying: false,
                            error: None,
                        };
                        self.sftp.properties = Some(view);
                } else {
                        let Some(client) = self.sftp.pane(side).client.clone() else {
                            self.sftp.pane_mut(side).error = Some("Not connected".into());
                            return Ok(Task::none());
                        };
                        let target = path.clone();
                        return Ok(Task::perform(
                            async move {
                                client.stat(&target).await.map_err(|e| e.to_string())
                            },
                            move |result| match result {
                                Ok(stat) => {
                                    let mode = stat.permissions.unwrap_or(0o644);
                                    Message::SftpPropertiesLoaded(crate::state::PropertiesView {
                                        side,
                                        path: path.clone(),
                                        is_dir,
                                        size: stat.size,
                                        mtime: stat.mtime,
                                        owner_uid: stat.uid,
                                        owner_gid: stat.gid,
                                        original_mode: mode,
                                        bits: crate::state::PermBits::from_mode(mode),
                                        mode_input: format!("{:03o}", mode & 0o777),
                                        applying: false,
                                        error: None,
                                    })
                                }
                                Err(e) => Message::SftpOpResult(side, e, true),
                            },
                        ));
                }
            }
            Message::SftpPropertiesLoaded(view) => {
                self.sftp.properties = Some(view);
            }
            Message::SftpPropertiesToggleBit(bit) => {
                if let Some(p) = self.sftp.properties.as_mut() {
                    let b = &mut p.bits;
                    let f = match bit {
                        crate::state::PermBit::UserR => &mut b.user_r,
                        crate::state::PermBit::UserW => &mut b.user_w,
                        crate::state::PermBit::UserX => &mut b.user_x,
                        crate::state::PermBit::GroupR => &mut b.group_r,
                        crate::state::PermBit::GroupW => &mut b.group_w,
                        crate::state::PermBit::GroupX => &mut b.group_x,
                        crate::state::PermBit::OtherR => &mut b.other_r,
                        crate::state::PermBit::OtherW => &mut b.other_w,
                        crate::state::PermBit::OtherX => &mut b.other_x,
                    };
                    *f = !*f;
                    // Keep the numeric field in lockstep with the checkboxes.
                    p.mode_input = format!("{:03o}", p.bits.to_mode());
                }
            }
            Message::SftpPropertiesModeInput(s) => {
                if let Some(p) = self.sftp.properties.as_mut() {
                    // Accept only octal digits, at most 4 (a leading special-bit
                    // digit is tolerated but ignored: the dialog edits rwx only
                    // and Apply preserves setuid/setgid/sticky from the original).
                    let cleaned: String =
                        s.chars().filter(|c| ('0'..='7').contains(c)).take(4).collect();
                    p.mode_input = cleaned.clone();
                    // Rewrite the checkboxes from a parseable value; leave them
                    // untouched while the field is empty or mid-edit.
                    if let Ok(v) = u32::from_str_radix(&cleaned, 8) {
                        p.bits = crate::state::PermBits::from_mode(v & 0o777);
                    }
                }
            }
            Message::SftpPropertiesApply => {
                let Some(p) = self.sftp.properties.as_mut() else {
                    return Ok(Task::none());
                };
                if p.applying {
                    return Ok(Task::none());
                }
                // Preserve the high bits (setuid / setgid / sticky)
                // we don't expose for editing, strip rwxrwxrwx out of
                // the original and overlay our edited 9 bits.
                let new_mode = (p.original_mode & !0o777) | p.bits.to_mode();
                if new_mode == p.original_mode {
                    self.sftp.properties = None;
                    return Ok(Task::none());
                }
                p.applying = true;
                p.error = None;
                let path = p.path.clone();
                let side = p.side;
                if !self.sftp.pane(side).is_remote {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt as _;
                            let result = std::fs::set_permissions(
                                &path,
                                std::fs::Permissions::from_mode(new_mode),
                            )
                            .map_err(|e| e.to_string());
                            return Ok(Task::done(Message::SftpPropertiesDone(result)));
                        }
                        #[cfg(not(unix))]
                        {
                            return Ok(Task::done(Message::SftpPropertiesDone(Err(
                                "chmod not supported on this platform".into(),
                            ))));
                        }
                } else {
                        let Some(client) = self.sftp.pane(side).client.clone() else {
                            return Ok(Task::done(Message::SftpPropertiesDone(Err(
                                "Not connected".into(),
                            ))));
                        };
                        return Ok(Task::perform(
                            async move {
                                client.chmod(&path, new_mode).await.map_err(|e| e.to_string())
                            },
                            Message::SftpPropertiesDone,
                        ));
                }
            }
            Message::SftpPropertiesDone(result) => {
                match result {
                    Ok(()) => {
                        let side = self.sftp.properties.as_ref().map(|p| p.side);
                        self.sftp.properties = None;
                        // Refresh whichever pane we just touched so
                        // the new permissions show up immediately.
                        return Ok(match side {
                            Some(side) if !self.sftp.pane(side).is_remote => {
                                self.refresh_sftp_local(side);
                                Task::none()
                            }
                            Some(side) => Task::done(Message::SftpNavigateRemote(
                                side,
                                self.sftp.pane(side).remote_path.clone(),
                            )),
                            None => Task::none(),
                        });
                    }
                    Err(e) => {
                        if let Some(p) = self.sftp.properties.as_mut() {
                            p.applying = false;
                            p.error = Some(e);
                        }
                    }
                }
            }
            Message::SftpPropertiesClose => {
                self.sftp.properties = None;
            }
            Message::SftpOpenLocal(path) => {
                self.sftp.row_menu = None;
                if let Err(e) = open::that(&path) {
                    self.sftp.pane_mut(local_side).error = Some(format!(
                        "Failed to open {}: {e}",
                        path.display()
                    ));
                }
            }
            Message::SftpRevealInExplorer(path, is_dir) => {
                // Reachable from both the row menu and the `⋮` menu.
                self.sftp.close_menus();
                if let Err(e) = crate::util::reveal_in_file_manager(&path, is_dir) {
                    self.sftp.pane_mut(local_side).error = Some(format!(
                        "Failed to open {}: {e}",
                        path.display()
                    ));
                }
            }
            Message::SftpStartEdit(remote_path) => {
                self.sftp.row_menu = None;
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    return Ok(Task::none());
                };
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
                        // Tag the temp filename with a uuid so concurrent
                        // edits of similarly-named files don't collide.
                        let temp_path = std::env::temp_dir().join(format!(
                            "oryxis-{}-{}",
                            uuid::Uuid::new_v4(),
                            basename
                        ));
                        tokio::fs::write(&temp_path, &bytes)
                            .await
                            .map_err(|e| format!("write temp: {e}"))?;
                        // Tighten temp file perms to 0600, the file
                        // holds plaintext remote contents and shouldn't
                        // be world-readable on a shared system. Default
                        // umask often leaves files at 0644.
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt as _;
                            let _ = tokio::fs::set_permissions(
                                &temp_path,
                                std::fs::Permissions::from_mode(0o600),
                            )
                            .await;
                        }
                        // open::that returns immediately after spawning
                        // the OS handler; the editor lifecycle is then
                        // entirely owned by the user.
                        if let Err(e) = open::that(&temp_path) {
                            return Err(format!(
                                "open editor: {e} (temp at {})",
                                temp_path.display()
                            ));
                        }
                        let initial_mtime = tokio::fs::metadata(&temp_path)
                            .await
                            .ok()
                            .and_then(|m| m.modified().ok());
                        Ok::<crate::state::EditSession, String>(crate::state::EditSession {
                            remote_path,
                            temp_path,
                            label: basename,
                            initial_mtime,
                            dirty: false,
                        })
                    },
                    move |result| match result {
                        Ok(session) => Message::SftpEditReady(session),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
                    },
                ));
            }
            Message::SftpEditReady(session) => {
                self.sftp.edit_session = Some(session);
            }
            Message::SftpEditSave => {
                let Some(session) = self.sftp.edit_session.take() else {
                    return Ok(Task::none());
                };
                let Some(client) = self.sftp.pane(remote_side).client.clone() else {
                    self.sftp.pane_mut(remote_side).error = Some("Not connected to a host".into());
                    let _ = std::fs::remove_file(&session.temp_path);
                    return Ok(Task::none());
                };
                let reload = self.sftp.pane(remote_side).remote_path.clone();
                return Ok(Task::perform(
                    async move {
                        let bytes = tokio::fs::read(&session.temp_path)
                            .await
                            .map_err(|e| format!("read temp: {e}"))?;
                        client
                            .write_file(&session.remote_path, &bytes)
                            .await
                            .map_err(|e| e.to_string())?;
                        let _ = tokio::fs::remove_file(&session.temp_path).await;
                        Ok::<String, String>(reload)
                    },
                    move |result| match result {
                        Ok(reload) => Message::SftpNavigateRemote(remote_side, reload),
                        Err(e) => Message::SftpOpResult(remote_side, e, true),
                    },
                ));
            }
            Message::SftpEditDiscard => {
                if let Some(session) = self.sftp.edit_session.take() {
                    let _ = std::fs::remove_file(&session.temp_path);
                }
            }
            Message::SftpEditWatchTick => {
                // Cheap mtime poll on the temp file, once we see a
                // newer timestamp than the initial download, flag the
                // session dirty and the modal copy adapts to surface
                // the change. The watcher subscription only ticks
                // while a session is active so this isn't pinging
                // disk on idle screens.
                if let Some(session) = self.sftp.edit_session.as_mut()
                    && !session.dirty
                    && let Ok(meta) = std::fs::metadata(&session.temp_path)
                    && let Ok(mtime) = meta.modified()
                {
                    match session.initial_mtime {
                        Some(initial) if mtime > initial => session.dirty = true,
                        None => session.initial_mtime = Some(mtime),
                        _ => {}
                    }
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
