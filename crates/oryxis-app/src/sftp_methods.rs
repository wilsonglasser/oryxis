//! `impl Oryxis` block for SFTP pane plumbing — local refresh, timeout
//! parsing, hit-testing, and the internal drag-drop dispatcher. Split
//! out of `app.rs` so the main module stays browsable.

use iced::Task;

use crate::app::{Message, Oryxis, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED};
use crate::sftp_helpers::sort_local_entries;

impl Oryxis {
    /// Refresh the local SFTP pane from `self.sftp.local_path`. Errors
    /// (missing dir, permission denied) surface as a user-visible string
    /// instead of a panic.
    pub(crate) fn refresh_sftp_local(&mut self) {
        self.sftp.local_entries.clear();
        self.sftp.local_error = None;
        let read = match std::fs::read_dir(&self.sftp.local_path) {
            Ok(r) => r,
            Err(e) => {
                self.sftp.local_error = Some(e.to_string());
                return;
            }
        };
        for entry in read.flatten() {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            self.sftp.local_entries.push(crate::state::LocalEntry {
                name,
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                modified: metadata.modified().ok(),
            });
        }
        sort_local_entries(&mut self.sftp.local_entries, self.sftp.local_sort);
    }

    /// Parsed and clamped concurrency setting — picks how many parallel
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

    /// Build the ordered list of paths currently shown in a pane —
    /// applies the same hide-hidden + filter rules the view uses so
    /// shift-click range select can find anchor / target indices in the
    /// list the user actually sees.
    pub(crate) fn visible_entry_paths_in_pane(
        &self,
        side: crate::state::SftpPaneSide,
    ) -> Vec<String> {
        match side {
            crate::state::SftpPaneSide::Local => {
                let needle = self.sftp.local_filter.to_lowercase();
                self.sftp
                    .local_entries
                    .iter()
                    .filter(|e| {
                        if !self.sftp.local_show_hidden && e.name.starts_with('.') {
                            return false;
                        }
                        if !needle.is_empty() && !e.name.to_lowercase().contains(&needle) {
                            return false;
                        }
                        true
                    })
                    .map(|e| {
                        self.sftp
                            .local_path
                            .join(&e.name)
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect()
            }
            crate::state::SftpPaneSide::Remote => {
                let needle = self.sftp.remote_filter.to_lowercase();
                let parent = self.sftp.remote_path.trim_end_matches('/');
                self.sftp
                    .remote_entries
                    .iter()
                    .filter(|e| {
                        if !self.sftp.remote_show_hidden && e.name.starts_with('.') {
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
    }

    /// Rough hit-test for "is the cursor inside the remote pane?". Used
    /// at file-drop time to decide whether the OS drop targets the remote
    /// upload path. The panes split the content area 50/50 so we just
    /// check the right half, accounting for the left nav rail and the
    /// chat sidebar (when visible).
    pub(crate) fn is_cursor_over_remote_pane(&self) -> bool {
        let sidebar = if self.sidebar_collapsed {
            SIDEBAR_WIDTH_COLLAPSED
        } else {
            SIDEBAR_WIDTH
        };
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

    /// Mirror helper for the left pane — checks the cursor sits in the
    /// half between the sidebar and the pane split.
    pub(crate) fn is_cursor_over_local_pane(&self) -> bool {
        let sidebar = if self.sidebar_collapsed {
            SIDEBAR_WIDTH_COLLAPSED
        } else {
            SIDEBAR_WIDTH
        };
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
        match side {
            crate::state::SftpPaneSide::Local => {
                let p = std::path::Path::new(path);
                if let Some(name) = p.file_name().and_then(|n| n.to_str())
                    && let Some(entry) =
                        self.sftp.local_entries.iter().find(|e| e.name == name)
                {
                    return entry.is_dir;
                }
                p.is_dir()
            }
            crate::state::SftpPaneSide::Remote => {
                let basename = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path);
                self.sftp
                    .remote_entries
                    .iter()
                    .find(|e| e.name == basename)
                    .map(|e| e.is_dir)
                    .unwrap_or(false)
            }
        }
    }

    /// Resolve an active internal drag drop — figure out which pane the
    /// cursor is over and dispatch a transfer for each dragged item.
    /// Cross-pane drags from local → remote upload, remote → local
    /// download. Drops onto a hovered folder go *inside* that folder;
    /// otherwise they land in the target pane's current directory.
    pub(crate) fn handle_internal_drag_drop(
        &mut self,
        drag: crate::state::SftpInternalDrag,
    ) -> Task<Message> {
        let target_remote_folder = self
            .sftp
            .hovered_row
            .as_ref()
            .filter(|(s, _, is_dir)| *s == crate::state::SftpPaneSide::Remote && *is_dir)
            .map(|(_, p, _)| p.clone());
        let target_local_folder = self
            .sftp
            .hovered_row
            .as_ref()
            .filter(|(s, _, is_dir)| *s == crate::state::SftpPaneSide::Local && *is_dir)
            .map(|(_, p, _)| p.clone());
        let in_remote = target_remote_folder.is_some() || self.is_cursor_over_remote_pane();
        let in_local = target_local_folder.is_some() || self.is_cursor_over_local_pane();
        match drag.origin_side {
            crate::state::SftpPaneSide::Local if in_remote => {
                if let Some(dir) = target_remote_folder {
                    self.sftp.upload_dest_override = Some(dir);
                }
                // Single item still uses the existing single-message
                // path so the standalone single-file conflict modal
                // fires; multi flows through the batched queue runner
                // so the apply-to-all checkbox works.
                if drag.items.len() == 1 {
                    let (path, is_dir) = drag.items.into_iter().next().unwrap();
                    let pb = std::path::PathBuf::from(path);
                    return if is_dir {
                        Task::done(Message::SftpUploadFolder(pb))
                    } else {
                        Task::done(Message::SftpUpload(pb))
                    };
                }
                let paths: Vec<std::path::PathBuf> = drag
                    .items
                    .into_iter()
                    .map(|(p, _)| std::path::PathBuf::from(p))
                    .collect();
                Task::done(Message::SftpUploadBatch(paths))
            }
            crate::state::SftpPaneSide::Remote if in_local => {
                // Direct the download to the hovered local folder via
                // a one-shot override — same pattern as upload, lets
                // the message handler consume the destination without
                // mutating the pane's actual `local_path`.
                if let Some(dir) = target_local_folder {
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
            _ => Task::none(),
        }
    }
}
