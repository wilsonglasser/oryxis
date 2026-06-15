//! `impl Oryxis` block for SFTP pane plumbing, local refresh, timeout
//! parsing, hit-testing, and the internal drag-drop dispatcher. Split
//! out of `app.rs` so the main module stays browsable.

use iced::Task;

use crate::app::{Message, Oryxis};
use crate::sftp_helpers::sort_local_entries;

impl Oryxis {
    /// Refresh a local pane from its `local_path`. Errors (missing dir,
    /// permission denied) surface as a user-visible string instead of a
    /// panic. No-op if the pane is currently a remote host.
    pub(crate) fn refresh_sftp_local(&mut self, side: crate::state::SftpPaneSide) {
        if self.sftp.pane(side).is_remote {
            return;
        }
        let sort = self.sftp.pane(side).sort;
        let local_path = self.sftp.pane(side).local_path.clone();
        {
            let pane = self.sftp.pane_mut(side);
            pane.local_entries.clear();
            pane.error = None;
        }

        // Bare WSL UNC roots (`\\wsl$`, `\\wsl.localhost`) can't be
        // enumerated via `read_dir`, Windows treats them as servers
        // with no share and returns ERROR_PATH_NOT_FOUND. Synthesize
        // distro entries from `wsl.exe -l -q` instead so the user can
        // step into a distro just by clicking it.
        if is_wsl_root(&local_path) {
            let pane = self.sftp.pane_mut(side);
            for distro in list_wsl_distros_for_pane() {
                pane.local_entries.push(crate::state::LocalEntry {
                    name: distro,
                    is_dir: true,
                    size: 0,
                    modified: None,
                });
            }
            sort_local_entries(&mut pane.local_entries, sort);
            return;
        }

        let read = match std::fs::read_dir(&local_path) {
            Ok(r) => r,
            Err(e) => {
                self.sftp.pane_mut(side).error = Some(e.to_string());
                return;
            }
        };
        let pane = self.sftp.pane_mut(side);
        for entry in read.flatten() {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            pane.local_entries.push(crate::state::LocalEntry {
                name,
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                modified: metadata.modified().ok(),
            });
        }
        sort_local_entries(&mut pane.local_entries, sort);
    }

    /// Parsed and clamped concurrency setting, picks how many parallel
    /// SFTP transfer slots to spin up. Falls back to `2` if the user
    /// typed garbage; clamped to `[1, 8]` to keep the channel pool sane.
    pub(crate) fn sftp_concurrency(&self) -> u8 {
        self.setting_sftp_concurrency
            .parse::<u8>()
            .unwrap_or(2)
            .clamp(1, 8)
    }

    fn parse_secs(input: &str, default: u64) -> std::time::Duration {
        let v = input.parse::<u64>().unwrap_or(default).max(1);
        std::time::Duration::from_secs(v)
    }

    pub(crate) fn sftp_connect_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.setting_sftp_connect_timeout, 15)
    }
    pub(crate) fn sftp_auth_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.setting_sftp_auth_timeout, 30)
    }
    pub(crate) fn sftp_session_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.setting_sftp_session_timeout, 10)
    }
    pub(crate) fn sftp_op_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.setting_sftp_op_timeout, 30)
    }

    /// Build the ordered list of paths currently shown in a pane
    /// applies the same hide-hidden + filter rules the view uses so
    /// shift-click range select can find anchor / target indices in the
    /// list the user actually sees.
    pub(crate) fn visible_entry_paths_in_pane(
        &self,
        side: crate::state::SftpPaneSide,
    ) -> Vec<String> {
        let pane = self.sftp.pane(side);
        let needle = pane.filter.to_lowercase();
        if !pane.is_remote {
            pane.local_entries
                .iter()
                .filter(|e| {
                    if !pane.show_hidden && e.name.starts_with('.') {
                        return false;
                    }
                    if !needle.is_empty() && !e.name.to_lowercase().contains(&needle) {
                        return false;
                    }
                    true
                })
                .map(|e| pane.local_path.join(&e.name).to_string_lossy().into_owned())
                .collect()
        } else {
            let parent = pane.remote_path.trim_end_matches('/');
            pane.remote_entries
                .iter()
                .filter(|e| {
                    if !pane.show_hidden && e.name.starts_with('.') {
                        return false;
                    }
                    if !needle.is_empty() && !e.name.to_lowercase().contains(&needle) {
                        return false;
                    }
                    true
                })
                .map(|e| {
                    if parent.is_empty() {
                        format!("/{}", e.name)
                    } else {
                        format!("{}/{}", parent, e.name)
                    }
                })
                .collect()
        }
    }

    /// Rough hit-test for "is the cursor inside the remote pane?". Used
    /// at file-drop time to decide whether the OS drop targets the remote
    /// upload path. The panes split the content area 50/50 so we just
    /// check the right half, accounting for the left nav rail and the
    /// chat sidebar (when visible).
    pub(crate) fn is_cursor_over_remote_pane(&self) -> bool {
        let sidebar = self.vault_rail_width();
        let chat_w = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| if t.chat_visible { self.chat_sidebar_width } else { 0.0 })
            .unwrap_or(0.0);
        let content_w = (self.window_size.width - sidebar - chat_w).max(0.0);
        let split = sidebar + content_w / 2.0;
        self.mouse_position.x > split
            && self.mouse_position.x < self.window_size.width - chat_w
    }

    /// Mirror helper for the left pane, checks the cursor sits in the
    /// half between the sidebar and the pane split.
    pub(crate) fn is_cursor_over_local_pane(&self) -> bool {
        let sidebar = self.vault_rail_width();
        let chat_w = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| if t.chat_visible { self.chat_sidebar_width } else { 0.0 })
            .unwrap_or(0.0);
        let content_w = (self.window_size.width - sidebar - chat_w).max(0.0);
        let split = sidebar + content_w / 2.0;
        self.mouse_position.x > sidebar && self.mouse_position.x < split
    }

    /// Look up whether a path in the given pane points at a directory,
    /// using cached entries so this is cheap to call per row in the
    /// drag-start path collection.
    pub(crate) fn row_is_dir_in_pane(
        &self,
        side: crate::state::SftpPaneSide,
        path: &str,
    ) -> bool {
        let pane = self.sftp.pane(side);
        if !pane.is_remote {
            let p = std::path::Path::new(path);
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && let Some(entry) = pane.local_entries.iter().find(|e| e.name == name)
            {
                return entry.is_dir;
            }
            p.is_dir()
        } else {
            let basename = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path);
            pane.remote_entries
                .iter()
                .find(|e| e.name == basename)
                .map(|e| e.is_dir)
                .unwrap_or(false)
        }
    }

    /// Resolve an active internal drag drop: figure out which pane the
    /// cursor is over (the destination) and dispatch a transfer for each
    /// dragged item, routing by the source and destination pane natures:
    /// Local -> remote uploads, remote -> Local downloads, remote ->
    /// remote relays. Drops onto a hovered folder go *inside* that
    /// folder; otherwise they land in the target pane's current
    /// directory.
    pub(crate) fn handle_internal_drag_drop(
        &mut self,
        drag: crate::state::SftpInternalDrag,
    ) -> Task<Message> {
        use crate::state::SftpPaneSide::{Left, Right};
        // Destination is the pane the cursor lands on, which must be the
        // pane opposite the drag origin (same-pane drops are no-ops).
        let dest_side = if drag.origin_side == Left { Right } else { Left };
        // A folder row in the destination pane redirects the drop inside
        // that folder.
        let target_folder = self
            .sftp
            .hovered_row
            .as_ref()
            .filter(|(s, _, is_dir)| *s == dest_side && *is_dir)
            .map(|(_, p, _)| p.clone());
        // The drop lands on the destination pane if the cursor is hovering
        // one of its rows. `hovered_row` is updated by row-hover events,
        // which fire even while a button is held (unlike cursor-move on
        // WSLg), so this is the reliable cross-platform signal. The
        // cursor-over-pane geometry check is kept as a fallback for empty
        // areas on platforms that do deliver moves during the hold.
        let hovered_dest = self
            .sftp
            .hovered_row
            .as_ref()
            .is_some_and(|(s, _, _)| *s == dest_side);
        let over_dest = hovered_dest
            || target_folder.is_some()
            || match dest_side {
                Left => self.is_cursor_over_local_pane(),
                Right => self.is_cursor_over_remote_pane(),
            };
        if !over_dest {
            return Task::none();
        }
        let src_remote = self.sftp.pane(drag.origin_side).is_remote;
        let dst_remote = self.sftp.pane(dest_side).is_remote;
        match (src_remote, dst_remote) {
            // Local -> remote: upload.
            (false, true) => {
                if let Some(dir) = target_folder {
                    self.sftp.upload_dest_override = Some(dir);
                }
                // Always route through the batched queue runner, even for a
                // single file, so the transfer shows the progress strip +
                // per-file panel and refreshes the remote pane on
                // completion. (Name conflicts use the queue's overwrite
                // modal instead of the standalone single-file one.)
                let paths: Vec<std::path::PathBuf> = drag
                    .items
                    .into_iter()
                    .map(|(p, _)| std::path::PathBuf::from(p))
                    .collect();
                Task::done(Message::SftpUploadBatch(paths))
            }
            // Remote -> Local: download.
            (true, false) => {
                if let Some(dir) = target_folder {
                    self.sftp.download_dest_override = Some(std::path::PathBuf::from(dir));
                }
                let mut tasks = Vec::with_capacity(drag.items.len());
                for (path, is_dir) in drag.items {
                    tasks.push(if is_dir {
                        Task::done(Message::SftpDownloadFolder(path))
                    } else {
                        Task::done(Message::SftpDownload(path))
                    });
                }
                Task::batch(tasks)
            }
            // Remote -> remote: server-to-server relay.
            (true, true) => {
                if let Some(dir) = target_folder {
                    self.sftp.upload_dest_override = Some(dir);
                }
                let from = drag.origin_side;
                let mut tasks = Vec::with_capacity(drag.items.len());
                for (path, is_dir) in drag.items {
                    tasks.push(if is_dir {
                        Task::done(Message::SftpRelayFolder(from, path))
                    } else {
                        Task::done(Message::SftpRelay(from, path))
                    });
                }
                Task::batch(tasks)
            }
            // Local -> Local: not reachable (Local is left-only), no-op.
            (false, false) => Task::none(),
        }
    }
}

/// Detects the synthetic WSL roots (`\\wsl$` / `\\wsl.localhost`).
/// Both forms with or without a trailing slash count, since clicking
/// the dropdown entry and typing the path manually produce slightly
/// different shapes.
fn is_wsl_root(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    let normalized = s.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    normalized == r"\\wsl$" || normalized == r"\\wsl.localhost"
}

/// Lists installed WSL distros via `wsl.exe -l -q`. UTF-16LE output
/// from `wsl.exe` is decoded back to a `String`, then trimmed for
/// the empty trailing entries Windows likes to add. Returns an empty
/// list on non-Windows or if `wsl.exe` is unavailable.
fn list_wsl_distros_for_pane() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW so we don't flash a console.
        let output = match std::process::Command::new("wsl.exe")
            .args(["-l", "-q"])
            .creation_flags(0x0800_0000)
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };
        // wsl.exe emits UTF-16LE on stdout when invoked from Win32.
        let bytes = output.stdout;
        let utf16: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&utf16);
        text.lines()
            .map(|line| line.trim().trim_matches('\0').to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}
