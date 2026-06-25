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
                    mode: None,
                    uid: None,
                    gid: None,
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
            // POSIX mode / owner are Unix-only; on Windows these stay
            // `None` and the Permissions / Owner columns render a dash.
            #[cfg(unix)]
            let (mode, uid, gid) = {
                use std::os::unix::fs::MetadataExt;
                (Some(metadata.mode()), Some(metadata.uid()), Some(metadata.gid()))
            };
            #[cfg(not(unix))]
            let (mode, uid, gid) = (None, None, None);
            pane.local_entries.push(crate::state::LocalEntry {
                name,
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                modified: metadata.modified().ok(),
                mode,
                uid,
                gid,
            });
        }
        sort_local_entries(&mut pane.local_entries, sort);
    }

    /// Append one line to the active SFTP tab's message log, stamping it
    /// with the current local time and capping the buffer to the most
    /// recent [`crate::state::SFTP_LOG_CAP`] entries. The log is in-memory
    /// only (per tab) and surfaced by the toggleable log panel.
    pub(crate) fn push_sftp_log(
        &mut self,
        level: crate::state::SftpLogLevel,
        text: impl Into<String>,
    ) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        let log = &mut self.sftp.log;
        log.push(crate::state::SftpLogEntry {
            time,
            level,
            text: text.into(),
        });
        let cap = crate::state::SFTP_LOG_CAP;
        if log.len() > cap {
            let drop = log.len() - cap;
            log.drain(0..drop);
        }
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

    /// Whether the given pane shows a `..` (parent) row, matching the view's
    /// own condition: any local path with a parent, or any remote path that
    /// isn't the root. The `..` row is a virtual first entry the keyboard
    /// cursor can land on (Enter / Right / Left there navigate up).
    pub(crate) fn sftp_pane_has_parent(&self, side: crate::state::SftpPaneSide) -> bool {
        let pane = self.sftp.pane(side);
        if pane.is_remote {
            pane.remote_path != "/" && !pane.remote_path.is_empty()
        } else {
            pane.local_path.parent().is_some()
        }
    }

    /// The per-directory scrollable id for a pane's file list. Must match
    /// the id the view builds so scroll operations target the right widget.
    fn sftp_list_scroll_id(&self, side: crate::state::SftpPaneSide) -> String {
        let pane = self.sftp.pane(side);
        let cur_path = if pane.is_remote {
            pane.remote_path.clone()
        } else {
            pane.local_path.to_string_lossy().into_owned()
        };
        let side_key = match side {
            crate::state::SftpPaneSide::Left => "left",
            crate::state::SftpPaneSide::Right => "right",
        };
        format!("sftp-list-{side_key}-{cur_path}")
    }

    /// Snap a pane's list to a relative vertical position (0.0 top .. 1.0
    /// bottom). Fallback used when the viewport height isn't known yet.
    fn sftp_snap_ratio(
        &self,
        side: crate::state::SftpPaneSide,
        idx: usize,
        total: usize,
    ) -> Task<Message> {
        let ratio = if total > 1 {
            idx as f32 / (total - 1) as f32
        } else {
            0.0
        };
        iced::widget::operation::snap_to(
            iced::widget::Id::from(self.sftp_list_scroll_id(side)),
            iced::widget::scrollable::RelativeOffset {
                x: None,
                y: Some(ratio),
            },
        )
    }

    /// Bring the row at `idx` (of `total`, `..` included) into view, but
    /// only scroll when it would otherwise sit outside the viewport: above
    /// the top edge (scroll up so it's the first visible row) or below the
    /// bottom edge (scroll down so it's the last). A row already fully
    /// visible doesn't move the list. Falls back to proportional snapping
    /// until the first `on_scroll` reports the viewport height.
    fn sftp_scroll_row_into_view(
        &mut self,
        side: crate::state::SftpPaneSide,
        idx: usize,
        total: usize,
    ) -> Task<Message> {
        use crate::views::sftp::ROW_HEIGHT;
        let viewport_h = self.sftp.pane(side).list_viewport_h;
        if viewport_h <= 0.0 {
            return self.sftp_snap_ratio(side, idx, total);
        }
        let content_h = total as f32 * ROW_HEIGHT;
        let max_scroll = (content_h - viewport_h).max(0.0);
        if max_scroll <= 0.0 {
            // Whole list fits; nothing to scroll.
            return Task::none();
        }
        let offset = self.sftp.pane(side).list_scroll_y.clamp(0.0, max_scroll);
        let row_top = idx as f32 * ROW_HEIGHT;
        let row_bottom = row_top + ROW_HEIGHT;
        let new_offset = if row_top < offset {
            row_top
        } else if row_bottom > offset + viewport_h {
            row_bottom - viewport_h
        } else {
            // Already fully visible: leave the scroll position untouched.
            return Task::none();
        };
        let new_offset = new_offset.clamp(0.0, max_scroll);
        // Optimistically record the offset so a burst of key presses before
        // the next `on_scroll` arrives still computes against fresh state.
        self.sftp.pane_mut(side).list_scroll_y = new_offset;
        iced::widget::operation::snap_to(
            iced::widget::Id::from(self.sftp_list_scroll_id(side)),
            iced::widget::scrollable::RelativeOffset {
                x: None,
                y: Some(new_offset / max_scroll),
            },
        )
    }

    /// Park the keyboard cursor on the focused pane's `..` row: clear the
    /// real-row selection and flag `parent_cursor`. Pins the list to the
    /// top unconditionally (not the edge-based "only if off-screen" path)
    /// so `..` is always visible even when iced restored a previous scroll
    /// position for this directory's scrollable id.
    fn sftp_focus_parent_row(
        &mut self,
        side: crate::state::SftpPaneSide,
        total: usize,
    ) -> Task<Message> {
        self.sftp.parent_cursor = true;
        self.sftp.selected_rows.clear();
        self.sftp.selection_anchor = None;
        self.sftp.pane_mut(side).list_scroll_y = 0.0;
        self.sftp_snap_ratio(side, 0, total)
    }

    /// Move the keyboard cursor one row up (`down == false`) or down within
    /// the focused pane, replacing any multi-row selection with the single
    /// new row and scrolling it into view. The `..` parent row, when shown,
    /// is the virtual first entry, so moving up from the first real row
    /// lands on it. With no current cursor, the first (down) or last (up)
    /// row is selected.
    pub(crate) fn sftp_move_focus(&mut self, down: bool) -> Task<Message> {
        let side = self.sftp.focused_side;
        let entries = self.visible_entry_paths_in_pane(side);
        let has_parent = self.sftp_pane_has_parent(side);
        let offset = usize::from(has_parent);
        let total = entries.len() + offset;
        if total == 0 {
            return Task::none();
        }
        // Virtual index over [".." , entries...]; ".." is index 0 when shown.
        let cur_virtual = if self.sftp.parent_cursor && has_parent {
            Some(0)
        } else {
            self.sftp
                .selected_rows
                .last()
                .filter(|(s, _)| *s == side)
                .and_then(|(_, p)| entries.iter().position(|e| e == p))
                .map(|i| i + offset)
        };
        let new_v = match cur_virtual {
            Some(i) if down => (i + 1).min(total - 1),
            Some(i) => i.saturating_sub(1),
            None if down => 0,
            None => total - 1,
        };
        if has_parent && new_v == 0 {
            return self.sftp_focus_parent_row(side, total);
        }
        self.sftp.parent_cursor = false;
        let full = entries[new_v - offset].clone();
        self.sftp.selected_rows = vec![(side, full.clone())];
        self.sftp.selection_anchor = Some((side, full));
        self.sftp_scroll_row_into_view(side, new_v, total)
    }

    /// If a pending cursor target is queued for `side`, consume it and
    /// return the task that lands the cursor. Called from the directory-load
    /// handlers once the new listing is in.
    pub(crate) fn sftp_take_pending_focus(
        &mut self,
        side: crate::state::SftpPaneSide,
    ) -> Option<Task<Message>> {
        match self.sftp.pending_focus.clone() {
            Some((s, target)) if s == side => {
                self.sftp.pending_focus = None;
                Some(self.sftp_apply_pending_focus(side, target))
            }
            _ => None,
        }
    }

    /// Land the keyboard cursor on a freshly loaded directory according to
    /// `target`: the first entry, the `..` row, or a specific path (with
    /// graceful fallback to the first entry / `..` when it isn't shown).
    pub(crate) fn sftp_apply_pending_focus(
        &mut self,
        side: crate::state::SftpPaneSide,
        target: crate::state::SftpPendingFocus,
    ) -> Task<Message> {
        use crate::state::SftpPendingFocus;
        self.sftp.focused_side = side;
        let entries = self.visible_entry_paths_in_pane(side);
        let has_parent = self.sftp_pane_has_parent(side);
        let offset = usize::from(has_parent);
        let total = entries.len() + offset;
        // Resolve the index into `entries`, or None to fall back to "..".
        let entry_idx: Option<usize> = match target {
            SftpPendingFocus::Parent => None,
            SftpPendingFocus::Path(p) => entries
                .iter()
                .position(|e| *e == p)
                .or_else(|| (!entries.is_empty()).then_some(0)),
        };
        if let Some(i) = entry_idx {
            self.sftp.parent_cursor = false;
            let full = entries[i].clone();
            self.sftp.selected_rows = vec![(side, full.clone())];
            self.sftp.selection_anchor = Some((side, full));
            self.sftp_scroll_row_into_view(side, i + offset, total)
        } else if has_parent {
            self.sftp_focus_parent_row(side, total)
        } else {
            self.sftp.selected_rows.clear();
            self.sftp.selection_anchor = None;
            self.sftp.parent_cursor = false;
            Task::none()
        }
    }

    /// Activate the focused row. Both Enter (`open_files == true`) and Right
    /// (`false`) descend into a folder and land the keyboard cursor on the
    /// opened folder's `..` row. Enter additionally opens a *file* (local via
    /// the OS handler, remote via edit-in-place); Right does nothing on a
    /// file. The `..` cursor navigates to the parent either way. No-op when
    /// nothing is focused.
    pub(crate) fn sftp_activate_focused(&mut self, open_files: bool) -> Task<Message> {
        use crate::state::SftpPendingFocus;
        let side = self.sftp.focused_side;
        if self.sftp.parent_cursor {
            return Task::done(Message::SftpUp(side));
        }
        let Some((side, path)) = self
            .sftp
            .selected_rows
            .last()
            .filter(|(s, _)| *s == side)
            .cloned()
        else {
            return Task::none();
        };
        let is_dir = self.row_is_dir_in_pane(side, &path);
        let is_remote = self.sftp.pane(side).is_remote;
        if is_dir {
            self.sftp.selected_rows.clear();
            self.sftp.selection_anchor = None;
            self.sftp.parent_cursor = false;
            // Descend and keep the cursor on the new folder's ".." row once
            // its listing loads (same for Enter and Right).
            self.sftp.pending_focus = Some((side, SftpPendingFocus::Parent));
            if is_remote {
                Task::done(Message::SftpNavigateRemote(side, path))
            } else {
                Task::done(Message::SftpNavigateLocal(
                    side,
                    std::path::PathBuf::from(path),
                ))
            }
        } else if !open_files {
            // Right arrow on a file: nothing to descend into.
            Task::none()
        } else if is_remote {
            Task::done(Message::SftpStartEdit(path))
        } else {
            Task::done(Message::SftpOpenLocal(std::path::PathBuf::from(path)))
        }
    }

    /// Move keyboard focus to the parent directory of the focused pane
    /// (Left arrow). Just dispatches the existing up-navigation.
    pub(crate) fn sftp_focus_parent(&mut self) -> Task<Message> {
        Task::done(Message::SftpUp(self.sftp.focused_side))
    }

    /// Toggle keyboard focus between the two panes (Tab). Lands the cursor
    /// on a row in the newly focused pane: a prior selection there if it's
    /// still visible, else its first row, else the `..` row.
    pub(crate) fn sftp_toggle_pane_focus(&mut self) -> Task<Message> {
        use crate::state::SftpPaneSide::{Left, Right};
        let new_side = match self.sftp.focused_side {
            Left => Right,
            Right => Left,
        };
        self.sftp.focused_side = new_side;
        self.sftp.parent_cursor = false;
        self.sftp.selection_anchor = None;
        let entries = self.visible_entry_paths_in_pane(new_side);
        let has_parent = self.sftp_pane_has_parent(new_side);
        let offset = usize::from(has_parent);
        let total = entries.len() + offset;
        // Preserve a prior selection in the target pane if it's still shown.
        let prior = self
            .sftp
            .selected_rows
            .iter()
            .find(|(s, _)| *s == new_side)
            .map(|(_, p)| p.clone())
            .filter(|p| entries.iter().any(|e| e == p));
        if let Some(p) = prior {
            let idx = entries.iter().position(|e| e == &p).unwrap_or(0);
            self.sftp.selected_rows = vec![(new_side, p.clone())];
            self.sftp.selection_anchor = Some((new_side, p));
            return self.sftp_scroll_row_into_view(new_side, idx + offset, total);
        }
        if let Some(first) = entries.first().cloned() {
            self.sftp.selected_rows = vec![(new_side, first.clone())];
            self.sftp.selection_anchor = Some((new_side, first));
            return self.sftp_scroll_row_into_view(new_side, offset, total);
        }
        // Empty pane: park on ".." when present, otherwise just clear.
        self.sftp.selected_rows.clear();
        if has_parent {
            return self.sftp_focus_parent_row(new_side, total);
        }
        Task::none()
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
        // Honor the user's resizable split, not a fixed 50/50, so the
        // boundary matches the actual divider position.
        let split = sidebar + content_w * self.sftp_split_ratio;
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
        let split = sidebar + content_w * self.sftp_split_ratio;
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
        // The drop lands on the destination pane if either the cursor sits
        // over that pane (geometry, the reliable signal during a button-hold)
        // or it's hovering one of its rows. `hovered_row` is what also feeds
        // `target_folder` above (drop *into* a hovered subfolder); that part
        // still depends on row-hover firing, so subfolder targeting can be
        // less reliable than the pane-level drop.
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

    // --- SFTP tab buffer (swap-on-focus) -----------------------------------
    //
    // The active SFTP tab's live state lives in `self.sftp`; inactive tabs
    // park their state in `SftpTab::state`. `active_sftp` is the index of the
    // tab currently loaded in `self.sftp` (the buffer owner). See the
    // invariant in `SFTP_TABS_PLAN.md`.

    /// Id of the SFTP tab currently loaded in `self.sftp`, if any.
    pub(crate) fn active_sftp_id(&self) -> Option<uuid::Uuid> {
        self.active_sftp
            .and_then(|i| self.sftp_tabs.get(i))
            .map(|t| t.id)
    }

    /// Ensure at least one SFTP tab exists, adopting the existing top-level
    /// `self.sftp` as the first tab's live buffer (no swap). Called when the
    /// SFTP surface is first reached so the single-tab case behaves exactly
    /// as before.
    pub(crate) fn ensure_sftp_tab(&mut self) {
        if self.sftp_tabs.is_empty() {
            let tab = crate::state::SftpTab::new(crate::i18n::t("sftp").to_string());
            let id = tab.id;
            self.sftp_tabs.push(tab);
            self.active_sftp = Some(0);
            self.tab_order.push(crate::state::TabRef::Sftp(id));
        }
    }

    /// Create a fresh SFTP tab (Local-left / remote-right, HOME on the left),
    /// append it to the strip, focus it and switch the surface to SFTP. The
    /// caller opens the host picker.
    pub(crate) fn open_new_sftp_tab(&mut self) {
        let mut tab = crate::state::SftpTab::new(crate::i18n::t("sftp").to_string());
        tab.state.left.local_path = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        // Seed both panes' columns from the persisted template.
        tab.state.left.columns = self.sftp_columns_template.clone();
        tab.state.right.columns = self.sftp_columns_template.clone();
        let id = tab.id;
        self.sftp_tabs.push(tab);
        let idx = self.sftp_tabs.len() - 1;
        self.tab_order.push(crate::state::TabRef::Sftp(id));
        self.focus_sftp_tab(idx);
        self.active_tab = None;
        self.active_view = crate::state::View::Sftp;
        self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
    }

    /// Build the persisted pin spec for the SFTP tab at `idx`, capturing both
    /// panes (Local vs which saved connection). Reads the live buffer for the
    /// active tab, the parked slot otherwise; a still-dormant tab returns its
    /// existing spec unchanged. `None` for an out-of-range index.
    pub(crate) fn sftp_pin_spec(&self, idx: usize) -> Option<crate::state::PinnedTabSpec> {
        let tab = self.sftp_tabs.get(idx)?;
        if let Some(spec) = &tab.pending_reopen {
            return Some(spec.clone());
        }
        let st = if self.active_sftp == Some(idx) {
            &self.sftp
        } else {
            &tab.state
        };
        let pane_spec = |p: &crate::state::PaneState| -> crate::state::SftpPaneSpec {
            if p.is_remote {
                p.host_label
                    .as_ref()
                    .and_then(|l| self.connections.iter().find(|c| &c.label == l))
                    .map(|c| crate::state::SftpPaneSpec::Remote(c.id))
                    .unwrap_or(crate::state::SftpPaneSpec::Local)
            } else {
                crate::state::SftpPaneSpec::Local
            }
        };
        Some(crate::state::PinnedTabSpec::Sftp {
            left: pane_spec(&st.left),
            right: pane_spec(&st.right),
            label: tab.label.clone(),
        })
    }

    /// Whether the SFTP tab at `idx` has unsaved work worth a close-guard:
    /// an in-flight transfer or a dirty edit-session. Reads the live buffer
    /// for the active tab, the parked slot otherwise.
    pub(crate) fn sftp_tab_has_unsaved(&self, idx: usize) -> bool {
        let st = if self.active_sftp == Some(idx) {
            &self.sftp
        } else {
            match self.sftp_tabs.get(idx) {
                Some(t) => &t.state,
                None => return false,
            }
        };
        st.transfer.is_some() || st.edit_session.as_ref().is_some_and(|e| e.dirty)
    }

    /// Close the SFTP tab at `idx`. Removes it from the strip, reindexes
    /// `active_sftp`, adopts the next remaining tab into the live buffer if the
    /// closed one owned it, and navigates away when the SFTP surface is left
    /// empty. Returns any follow-up navigation task.
    pub(crate) fn close_sftp_tab(&mut self, idx: usize) -> Task<Message> {
        if idx >= self.sftp_tabs.len() {
            return Task::none();
        }
        let id = self.sftp_tabs[idx].id;
        let was_owner = self.active_sftp == Some(idx);
        let was_focused_surface =
            was_owner && self.active_tab.is_none() && self.active_view == crate::state::View::Sftp;
        self.sftp_tabs.remove(idx);
        self.tab_order
            .retain(|r| !matches!(r, crate::state::TabRef::Sftp(x) if *x == id));
        // Reindex the buffer-owner pointer around the removed slot.
        self.active_sftp = match self.active_sftp {
            Some(a) if a == idx => None,
            Some(a) if a > idx => Some(a - 1),
            other => other,
        };
        // The closed tab owned the live buffer: `self.sftp` now holds its
        // (discarded) state. Adopt the nearest remaining tab, else reset.
        if was_owner {
            if !self.sftp_tabs.is_empty() {
                let next = idx.min(self.sftp_tabs.len() - 1);
                self.sftp = std::mem::take(&mut self.sftp_tabs[next].state);
                self.active_sftp = Some(next);
            } else {
                self.sftp = crate::state::SftpState::default();
            }
        }
        if was_focused_surface {
            if self.sftp_tabs.is_empty() {
                return Task::done(Message::ChangeView(crate::state::View::Dashboard));
            }
            self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
            self.refresh_sftp_local(crate::state::SftpPaneSide::Right);
        }
        Task::none()
    }

    /// Close every SFTP tab except the one at `keep_idx`. Reuses
    /// `close_sftp_tab` per dropped tab so `active_sftp`, buffer adoption and
    /// `tab_order` stay consistent (instead of a hand-rolled `retain` that
    /// hard-codes `active_sftp = Some(0)`).
    pub(crate) fn close_other_sftp_tabs(&mut self, keep_idx: usize) -> Task<Message> {
        let Some(keep_id) = self.sftp_tabs.get(keep_idx).map(|t| t.id) else {
            return Task::none();
        };
        let mut task = Task::none();
        while let Some(idx) = self.sftp_tabs.iter().position(|t| t.id != keep_id) {
            task = self.close_sftp_tab(idx);
        }
        task
    }

    /// Whether closing every tab but `keep_idx` would drop unsaved work, i.e.
    /// any other tab has an in-flight transfer or a dirty edit-session.
    pub(crate) fn other_sftp_tabs_have_unsaved(&self, keep_idx: usize) -> bool {
        (0..self.sftp_tabs.len()).any(|i| i != keep_idx && self.sftp_tab_has_unsaved(i))
    }

    /// Owning tab id to stamp on an SFTP async-continuation message: the tab
    /// currently being routed (mid-`route_sftp_async`) if any, else the
    /// focused tab. User-initiated transfers (not mid-route) get the focused
    /// tab; chained continuations get the originating tab. `None` when no
    /// SFTP tab is active, in which case there is nothing to route to (a
    /// nil-UUID stamp would just be dropped by `route_sftp_async`).
    pub(crate) fn current_sftp_owner(&self) -> Option<uuid::Uuid> {
        self.routing_sftp.or_else(|| self.active_sftp_id())
    }

    /// Dispatch an SFTP async-continuation message against its owning tab.
    /// Temporarily swaps that tab's parked state into `self.sftp` (no-op if it
    /// is already the focused tab), runs the normal handler chain, then swaps
    /// back. Drops the message if the owning tab was closed meanwhile.
    pub(crate) fn route_sftp_async(&mut self, id: uuid::Uuid, message: Message) -> Task<Message> {
        let Some(idx) = self.sftp_tabs.iter().position(|t| t.id == id) else {
            return Task::none();
        };
        let is_active = self.active_sftp == Some(idx);
        if !is_active {
            std::mem::swap(&mut self.sftp, &mut self.sftp_tabs[idx].state);
        }
        let prev = self.routing_sftp.replace(id);
        let task = self.dispatch_message(message);
        self.routing_sftp = prev;
        if !is_active {
            std::mem::swap(&mut self.sftp, &mut self.sftp_tabs[idx].state);
        }
        task
    }

    /// Make `idx` the focused SFTP tab: park the currently-loaded tab's state
    /// into its slot and hoist `idx`'s state into `self.sftp`. No-op if `idx`
    /// is already loaded or out of range. Does not touch `active_view` /
    /// `active_tab`; the caller drives the surface switch.
    pub(crate) fn focus_sftp_tab(&mut self, idx: usize) {
        if idx >= self.sftp_tabs.len() || self.active_sftp == Some(idx) {
            return;
        }
        if let Some(old) = self.active_sftp
            && let Some(tab) = self.sftp_tabs.get_mut(old)
        {
            // Park outgoing: live buffer -> its slot.
            std::mem::swap(&mut self.sftp, &mut tab.state);
        }
        if let Some(tab) = self.sftp_tabs.get_mut(idx) {
            // Load incoming: its slot -> live buffer.
            std::mem::swap(&mut self.sftp, &mut tab.state);
        }
        self.active_sftp = Some(idx);
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
