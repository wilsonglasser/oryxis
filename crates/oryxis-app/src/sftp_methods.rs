//! `impl Oryxis` block for SFTP pane plumbing, local refresh, timeout
//! parsing, hit-testing, and the internal drag-drop dispatcher. Split
//! out of `app.rs` so the main module stays browsable.

use iced::Task;

use crate::app::{TabsMessage, NavigationMessage, Message, Oryxis, SftpMessage};
use crate::sftp_helpers::sort_local_entries;

/// Process-global monotonic source for `SftpPane::local_list_seq` and
/// `SftpPane::remote_list_seq`. Global (not per-pane, and shared between
/// local and remote) so a seq is unique across every SFTP surface and
/// direction, even the ones swapped in and out of the live buffer by the
/// hybrid-tab park/hoist. See `spawn_local_listing` and the remote
/// listing spawn in `dispatch_sftp`.
static NEXT_LIST_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Next value of the shared listing sequence. Both the local and remote
/// listing spawns pull from here so a stale result can never match
/// another pane's current seq.
pub(crate) fn next_list_seq() -> u64 {
    NEXT_LIST_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Oryxis {
    /// Refresh a local pane from its `local_path`. Errors (missing dir,
    /// permission denied) surface as a user-visible string instead of a
    /// panic. No-op if the pane is currently a remote host.
    pub(crate) fn refresh_sftp_local(&mut self, side: crate::state::SftpPaneSide) {
        if self.cur_sftp().pane(side).is_remote {
            return;
        }
        let sort = self.cur_sftp().pane(side).sort;
        let local_path = self.cur_sftp().pane(side).local_path.clone();
        // In-place refresh of a directory that was just listed: the OS
        // caches are warm, so the synchronous call is fine here. First
        // ENTRY into a directory goes through the async
        // `spawn_local_listing` path instead; a cold path (network
        // drive, spun-down disk) can block `read_dir` for many seconds
        // and a synchronous call here froze the whole UI until Windows
        // declared the window dead.
        match list_local_dir_blocking(&local_path) {
            Ok(mut entries) => {
                sort_local_entries(&mut entries, sort);
                let pane = self.sftp.pane_mut(side);
                pane.local_entries = entries;
                pane.error = None;
            }
            Err(e) => {
                let pane = self.sftp.pane_mut(side);
                pane.local_entries.clear();
                pane.error = Some(e);
            }
        }
    }

    /// List `path` on a blocking worker thread and deliver the outcome
    /// as `SftpLocalListed`. The entry path for NEW directories (row
    /// click, breadcrumb, drive picker, typed/pasted path): the update
    /// loop never touches the filesystem, so a cold path can't freeze
    /// the UI.
    pub(crate) fn spawn_local_listing(
        &mut self,
        side: crate::state::SftpPaneSide,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        // Stamp the pane's listing seq from a PROCESS-GLOBAL counter so a
        // slower, earlier listing (cold network path) can't overwrite this
        // one when it finally lands; SftpLocalListed drops stale seqs. The
        // counter must be global, not per-pane: `local_list_seq` lives
        // inside `SftpState`, which is swapped whole on the standalone <->
        // hybrid park/hoist, so two surfaces bumping independent per-pane
        // counters could reach the SAME value and a stale listing from one
        // surface would then match (and clobber) the live pane of the
        // other. A global monotonic seq is unique across every surface, so
        // only the pane that actually spawned a listing can match it.
        let seq = next_list_seq();
        self.sftp.pane_mut(side).local_list_seq = seq;
        let list_path = path.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    list_local_dir_blocking(&list_path)
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
            },
            move |result| Message::Sftp(SftpMessage::SftpLocalListed(side, seq, path.clone(), result)),
        )
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
        self.vaults.cur().prefs.sftp_concurrency
            .parse::<u8>()
            .unwrap_or(2)
            .clamp(1, 8)
    }

    /// Where a download lands when nothing else names a destination:
    /// the Settings > Terminal "Default download folder" row, resolved
    /// through [`crate::state::resolve_download_dir`]. Serves the ZMODEM
    /// landing dir and the sidebar browser's save dialog alike, which is
    /// what that setting promises.
    pub(crate) fn default_download_dir(&self) -> std::path::PathBuf {
        crate::state::resolve_download_dir(&self.device_prefs.zmodem_download_dir)
    }

    fn parse_secs(input: &str, default: u64) -> std::time::Duration {
        let v = input.parse::<u64>().unwrap_or(default).max(1);
        std::time::Duration::from_secs(v)
    }

    pub(crate) fn sftp_connect_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.vaults.cur().prefs.sftp_connect_timeout, 15)
    }
    pub(crate) fn sftp_auth_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.vaults.cur().prefs.sftp_auth_timeout, 30)
    }
    pub(crate) fn sftp_session_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.vaults.cur().prefs.sftp_session_timeout, 10)
    }
    pub(crate) fn sftp_op_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.vaults.cur().prefs.sftp_op_timeout, 30)
    }
    /// The terminal dials' budget for opening the shell on an
    /// authenticated connection (channel open + PTY + shell request),
    /// Settings > Connection. Fresh dials and reused connections alike.
    pub(crate) fn ssh_session_timeout(&self) -> std::time::Duration {
        Self::parse_secs(&self.vaults.cur().prefs.ssh_session_timeout, 30)
    }

    /// Build the ordered list of paths currently shown in a pane
    /// applies the same hide-hidden + filter rules the view uses so
    /// shift-click range select can find anchor / target indices in the
    /// list the user actually sees.
    pub(crate) fn visible_entry_paths_in_pane(
        &self,
        side: crate::state::SftpPaneSide,
    ) -> Vec<String> {
        let pane = self.cur_sftp().pane(side);
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
        let pane = self.cur_sftp().pane(side);
        if pane.is_remote {
            pane.remote_path != "/" && !pane.remote_path.is_empty()
        } else {
            pane.local_path.parent().is_some()
        }
    }

    /// The per-directory scrollable id for a pane's file list. Must match
    /// the id the view builds so scroll operations target the right widget.
    fn sftp_list_scroll_id(&self, side: crate::state::SftpPaneSide) -> String {
        let pane = self.cur_sftp().pane(side);
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
        iced::widget::operation::scrollable::snap_to(
            iced::widget::Id::from(self.sftp_list_scroll_id(side)),
            iced::widget::scrollable::RelativeOffset {
                x: None,
                y: Some(ratio),
            },
            iced::widget::operation::Animation::Instant,
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
        let viewport_h = self.cur_sftp().pane(side).list_viewport_h;
        if viewport_h <= 0.0 {
            return self.sftp_snap_ratio(side, idx, total);
        }
        let content_h = total as f32 * ROW_HEIGHT;
        let max_scroll = (content_h - viewport_h).max(0.0);
        if max_scroll <= 0.0 {
            // Whole list fits; nothing to scroll.
            return Task::none();
        }
        let offset = self.cur_sftp().pane(side).list_scroll_y.clamp(0.0, max_scroll);
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
        iced::widget::operation::scrollable::snap_to(
            iced::widget::Id::from(self.sftp_list_scroll_id(side)),
            iced::widget::scrollable::RelativeOffset {
                x: None,
                y: Some(new_offset / max_scroll),
            },
            iced::widget::operation::Animation::Instant,
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
        let side = self.cur_sftp().focused_side;
        let entries = self.visible_entry_paths_in_pane(side);
        let has_parent = self.sftp_pane_has_parent(side);
        let offset = usize::from(has_parent);
        let total = entries.len() + offset;
        if total == 0 {
            return Task::none();
        }
        // Virtual index over [".." , entries...]; ".." is index 0 when shown.
        let cur_virtual = if self.cur_sftp().parent_cursor && has_parent {
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
        match self.cur_sftp().pending_focus.clone() {
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

    /// Activate the focused row from the keyboard. Right (`open_files ==
    /// false`) always descends into a folder and lands the cursor on the
    /// opened folder's `..` row; on a file it does nothing. Enter
    /// (`open_files == true`) is FileZilla's activation, the same as a
    /// double click: the double-click setting decides what happens to a
    /// folder or a file (`sftp_activate_entry`), and a selection of
    /// several rows is sent to the other pane as a whole. The `..` cursor
    /// goes up either way. No-op when nothing is focused.
    pub(crate) fn sftp_activate_focused(&mut self, open_files: bool) -> Task<Message> {
        use crate::state::SftpPendingFocus;
        let side = self.cur_sftp().focused_side;
        if self.cur_sftp().parent_cursor {
            return Task::done(Message::Sftp(SftpMessage::SftpUp(side)));
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
        let opens_as_folder = self.row_opens_as_folder(side, &path);
        if !open_files {
            if self.is_remote_link_row(side, &path) {
                // Whether there is a folder to descend into is the
                // server's answer, asked first.
                return Task::done(Message::Sftp(SftpMessage::SftpOpenLink(
                    side,
                    path,
                    crate::state::SftpLinkIntent::Descend { focus_parent: true },
                )));
            }
            if !opens_as_folder {
                // Right arrow on a file: nothing to descend into.
                return Task::none();
            }
            self.sftp.selected_rows.clear();
            self.sftp.selection_anchor = None;
            self.sftp.parent_cursor = false;
            // Descend and keep the cursor on the new folder's ".." row once
            // its listing loads.
            self.sftp.pending_focus = Some((side, SftpPendingFocus::Parent));
            return self.sftp_navigate_into(side, path);
        }
        let selected_here =
            self.cur_sftp().selected_rows.iter().filter(|(s, _)| *s == side).count();
        if selected_here > 1 {
            return self.sftp_transfer_selection(side);
        }
        self.sftp_activate_entry(side, path, opens_as_folder, true)
    }

    /// FileZilla's activation of one row, shared by a double click and
    /// Enter: the double-click setting for its kind decides. A zip the
    /// browser can open counts as a folder (its "open" is browsing it);
    /// `opens_as_folder` is the row's own answer. A remote symlink is
    /// neither yet: the server is asked where it points
    /// (`SftpOpenLink`) and the answer comes back through
    /// [`Self::sftp_activate_known`]. `from_keyboard` lands the cursor on
    /// the opened folder's `..` row, the keyboard descent's rule.
    pub(crate) fn sftp_activate_entry(
        &mut self,
        side: crate::state::SftpPaneSide,
        path: String,
        opens_as_folder: bool,
        from_keyboard: bool,
    ) -> Task<Message> {
        if self.is_remote_link_row(side, &path) {
            return Task::done(Message::Sftp(SftpMessage::SftpOpenLink(
                side,
                path,
                crate::state::SftpLinkIntent::Activate { from_keyboard },
            )));
        }
        self.sftp_activate_known(side, path, opens_as_folder, from_keyboard, None)
    }

    /// The activation once what the row is, is known. `resolved_dir` is
    /// a symbolic link's answer from the server, which a transfer then
    /// follows instead of the listing (where a link is not a folder).
    pub(crate) fn sftp_activate_known(
        &mut self,
        side: crate::state::SftpPaneSide,
        path: String,
        opens_as_folder: bool,
        from_keyboard: bool,
        resolved_dir: Option<bool>,
    ) -> Task<Message> {
        use crate::state::{SftpDirDoubleClick, SftpFileDoubleClick, SftpPendingFocus};
        let browsable_zip = !opens_as_folder && self.sftp_zip_browsable(side, &path);
        if opens_as_folder || browsable_zip {
            return match self.vaults.cur().prefs.sftp_dir_double_click {
                SftpDirDoubleClick::Enter if browsable_zip => {
                    Task::done(Message::Sftp(SftpMessage::SftpZipOpen(side, path)))
                }
                SftpDirDoubleClick::Enter => {
                    self.sftp.selected_rows.clear();
                    self.sftp.selection_anchor = None;
                    self.sftp.parent_cursor = false;
                    if from_keyboard {
                        self.sftp.pending_focus = Some((side, SftpPendingFocus::Parent));
                    }
                    self.sftp_navigate_into(side, path)
                }
                SftpDirDoubleClick::Transfer => self.sftp_transfer_entry(side, path, resolved_dir),
                SftpDirDoubleClick::Nothing => Task::none(),
            };
        }
        match self.vaults.cur().prefs.sftp_file_double_click {
            SftpFileDoubleClick::Transfer => self.sftp_transfer_entry(side, path, resolved_dir),
            // A file inside a browsed archive has no "open": the listing is
            // virtual, and copying it out is the transfer.
            SftpFileDoubleClick::Open if self.cur_sftp().pane(side).zip.is_some() => Task::none(),
            SftpFileDoubleClick::Open if self.cur_sftp().pane(side).is_remote => {
                Task::done(Message::Sftp(SftpMessage::SftpStartEdit(side, path)))
            }
            SftpFileDoubleClick::Open => Task::done(Message::Sftp(SftpMessage::SftpOpenLocal(
                std::path::PathBuf::from(path),
            ))),
            SftpFileDoubleClick::Nothing => Task::none(),
        }
    }

    /// Navigate a pane into `path` (a folder of its listing, or of the
    /// archive it is browsing: the navigate arms intercept that).
    fn sftp_navigate_into(&self, side: crate::state::SftpPaneSide, path: String) -> Task<Message> {
        if self.cur_sftp().pane(side).is_remote {
            Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(side, path)))
        } else {
            Task::done(Message::Sftp(SftpMessage::SftpNavigateLocal(
                side,
                std::path::PathBuf::from(path),
            )))
        }
    }

    /// Whether a row is a zip this pane can browse in place: a `.zip`
    /// name, outside a browsed archive (a nested zip would need its outer
    /// entry decompressed first, which defeats the ranged-read model).
    fn sftp_zip_browsable(&self, side: crate::state::SftpPaneSide, path: &str) -> bool {
        use oryxis_archive::names::ArchiveKind;
        self.cur_sftp().pane(side).zip.is_none()
            && matches!(
                ArchiveKind::from_name(&crate::dispatch_sftp_archive::base_name(path)),
                Some(ArchiveKind::Zip)
            )
    }

    /// Whether a row is a symbolic link on a remote pane's real listing
    /// (a browsed archive's rows are never links), matched by the full
    /// path the rows were built with.
    pub(crate) fn is_remote_link_row(&self, side: crate::state::SftpPaneSide, path: &str) -> bool {
        let pane = self.cur_sftp().pane(side);
        if !pane.is_remote || pane.zip.is_some() {
            return false;
        }
        let base = pane.remote_path.trim_end_matches('/');
        pane.remote_entries.iter().any(|e| {
            e.is_symlink
                && if base.is_empty() {
                    path.strip_prefix('/') == Some(e.name.as_str())
                } else {
                    path.strip_prefix(base).and_then(|rest| rest.strip_prefix('/')) == Some(e.name.as_str())
                }
        })
    }

    /// Whether a row is a symbolic link on a Local pane, matched by the
    /// full path the rows were built with.
    pub(crate) fn is_local_link_row(&self, side: crate::state::SftpPaneSide, path: &str) -> bool {
        let pane = self.cur_sftp().pane(side);
        !pane.is_remote
            && pane.zip.is_none()
            && pane
                .local_entries
                .iter()
                .any(|e| e.is_symlink && pane.local_path.join(&e.name).to_string_lossy() == path)
    }

    /// Whether a row opens like a folder: a folder, or a remote symlink
    /// (the listing cannot tell where it points, so it is offered as a
    /// way in, the same answer its row and icon give; opening one asks
    /// the server first, `SftpOpenLink`).
    fn row_opens_as_folder(&self, side: crate::state::SftpPaneSide, path: &str) -> bool {
        let pane = self.cur_sftp().pane(side);
        if pane.is_remote {
            let basename = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path);
            return pane
                .remote_entries
                .iter()
                .find(|e| e.name == basename)
                .is_some_and(|e| e.is_dir || e.is_symlink);
        }
        self.row_is_dir_in_pane(side, path)
    }

    /// What the pane opposite `side` is as a transfer destination: its
    /// side, whether it is a remote one, and whether it can take a
    /// transfer right now (a remote pane needs its session; a Local pane
    /// always can). The row menu offers Upload / Download / Relay from
    /// the same answer.
    pub(crate) fn sftp_cross_pane(&self, side: crate::state::SftpPaneSide) -> SftpCrossPane {
        use crate::state::SftpPaneSide::{Left, Right};
        let other_side = if side == Left { Right } else { Left };
        let other = self.cur_sftp().pane(other_side);
        SftpCrossPane {
            other_side,
            other_is_remote: other.is_remote,
            ready: !other.is_remote || other.client.is_some(),
        }
    }

    /// Send one row to the other pane, the way the row menu would: an
    /// upload, a download or a relay by the two panes' natures, a
    /// copy-out from a browsed archive. Two Local panes have no transfer
    /// between them (FileZilla has no local-to-local either); an other
    /// pane whose session is gone says so instead of doing nothing.
    pub(crate) fn sftp_transfer_entry(
        &mut self,
        side: crate::state::SftpPaneSide,
        path: String,
        resolved_dir: Option<bool>,
    ) -> Task<Message> {
        let cross = self.sftp_cross_pane(side);
        let source_is_remote = self.cur_sftp().pane(side).is_remote;
        if !cross.ready {
            return self.show_toast(crate::i18n::t("sftp_not_connected").to_string());
        }
        // The listing's own answer, not the row's "opens as folder",
        // unless the row is a symbolic link the server was asked about:
        // then it goes as what it points at.
        let is_dir = resolved_dir.unwrap_or_else(|| self.row_is_dir_in_pane(side, &path));
        if self.cur_sftp().pane(side).zip.is_some() {
            // Out of a browsed archive, into a pane that is not browsing
            // one itself (the menu's copy-out gate).
            if self.cur_sftp().pane(cross.other_side).zip.is_some() {
                return Task::none();
            }
            return Task::done(Message::Sftp(SftpMessage::SftpZipCopyOut(side, path, is_dir)));
        }
        let msg = match (source_is_remote, cross.other_is_remote, is_dir) {
            (false, true, true) => SftpMessage::SftpUploadFolder(std::path::PathBuf::from(path)),
            (false, true, false) => {
                SftpMessage::SftpUploadBatch(vec![std::path::PathBuf::from(path)])
            }
            (true, false, true) => SftpMessage::SftpDownloadFolder(path),
            (true, false, false) => SftpMessage::SftpDownload(path),
            (true, true, true) => SftpMessage::SftpRelayFolder(side, path),
            (true, true, false) => SftpMessage::SftpRelay(side, path),
            (false, false, _) => return Task::none(),
        };
        Task::done(Message::Sftp(msg))
    }

    /// Enter on a selection of several rows: FileZilla sends the whole
    /// selection to the other pane. The batch doors exist for upload and
    /// download; a relay between two remote panes goes one row at a time
    /// in the menu too, so a multi-row Enter there does nothing.
    fn sftp_transfer_selection(&mut self, side: crate::state::SftpPaneSide) -> Task<Message> {
        let cross = self.sftp_cross_pane(side);
        let pane = self.cur_sftp().pane(side);
        if pane.zip.is_some() {
            return Task::none();
        }
        if !cross.ready {
            return self.show_toast(crate::i18n::t("sftp_not_connected").to_string());
        }
        match (pane.is_remote, cross.other_is_remote) {
            (false, true) => Task::done(Message::Sftp(SftpMessage::SftpUploadSelection)),
            (true, false) => Task::done(Message::Sftp(SftpMessage::SftpDownloadSelection)),
            _ => Task::none(),
        }
    }

    /// Move keyboard focus to the parent directory of the focused pane
    /// (Left arrow). Just dispatches the existing up-navigation.
    pub(crate) fn sftp_focus_parent(&mut self) -> Task<Message> {
        Task::done(Message::Sftp(SftpMessage::SftpUp(self.cur_sftp().focused_side)))
    }

    /// Toggle keyboard focus between the two panes (Tab). Lands the cursor
    /// on a row in the newly focused pane: a prior selection there if it's
    /// still visible, else its first row, else the `..` row.
    pub(crate) fn sftp_toggle_pane_focus(&mut self) -> Task<Message> {
        use crate::state::SftpPaneSide::{Left, Right};
        let new_side = match self.cur_sftp().focused_side {
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

    /// Open the row context menu on the keyboard-focused row (the Menu key
    /// / Shift+F10), anchored on that row's on-screen bounds rather than
    /// the mouse. The keyboard peer of a right-click: a focused entry opens
    /// its row menu, the `..` cursor opens the pane's background menu, and
    /// with neither focused (fresh pane, nothing selected) it is a no-op.
    pub(crate) fn sftp_open_focus_row_menu(&mut self) -> Task<Message> {
        let side = self.cur_sftp().focused_side;
        // Both the focused entry and the `..` row report their bounds via a
        // `bounds_reporter`, so this is the last-drawn cursor rect. It was
        // recorded during draw, which under a scrollable happens in content
        // space (the scroll translation lives in the renderer), so map it
        // back to the screen with the pane's tracked offsets. Anchor just
        // inside the leading edge and drop from the bottom so the menu
        // opens below the row, like a right-click at that spot.
        let bounds = self.cur_sftp().focus_row_bounds.get();
        let pane = self.cur_sftp().pane(side);
        let x = bounds.x + 8.0 - pane.list_pan_x.get();
        let y = bounds.y + bounds.height - pane.list_scroll_y;
        let target = self
            .sftp
            .selected_rows
            .last()
            .filter(|(s, _)| *s == side)
            .map(|(_, p)| p.clone());
        if let Some(path) = target {
            let is_dir = self.sftp_entry_is_dir(side, &path);
            self.sftp.row_menu = Some(crate::state::SftpRowMenu {
                side,
                path,
                is_dir,
                is_background: false,
                open_group: false,
                x,
                y,
            });
        } else if self.cur_sftp().parent_cursor {
            // Cursor on ".." : the pane-level (background) menu, whose
            // `path` carries the pane's current directory.
            let pane = self.cur_sftp().pane(side);
            let dir = if pane.is_remote {
                pane.remote_path.clone()
            } else {
                pane.local_path.to_string_lossy().into_owned()
            };
            self.sftp.row_menu = Some(crate::state::SftpRowMenu {
                side,
                path: dir,
                is_dir: true,
                is_background: true,
                open_group: false,
                x,
                y,
            });
        } else {
            // No entry selected and not on ".." : nothing to anchor on.
            return Task::none();
        }
        // The keypress is consumed here and never reaches the modal keynav
        // router, so prime its keyboard-visible state and default selection
        // directly: a menu opened by a key should ring row 0 at once, not
        // after a warm-up arrow press.
        self.keynav.modal.kbd.set(true);
        self.keynav.modal.selected = Some((crate::keynav::ModalSurface::SftpRowMenu, 0));
        Task::none()
    }

    /// Whether the focused-pane entry at `path` is a directory, matching
    /// the exact full-path construction the rows were built with so a
    /// keyboard-opened menu shows the same action set a right-click would.
    /// Mirrors the rows' raw `is_dir`, where a symlink reports `false`.
    fn sftp_entry_is_dir(&self, side: crate::state::SftpPaneSide, path: &str) -> bool {
        let pane = self.cur_sftp().pane(side);
        if pane.is_remote {
            let base = pane.remote_path.trim_end_matches('/');
            pane.remote_entries
                .iter()
                .find_map(|e| {
                    let full = if base.is_empty() {
                        format!("/{}", e.name)
                    } else {
                        format!("{base}/{}", e.name)
                    };
                    (full == path).then_some(e.is_dir)
                })
                .unwrap_or(false)
        } else {
            pane.local_entries
                .iter()
                .find_map(|e| {
                    (pane.local_path.join(&e.name).to_string_lossy() == path)
                        .then_some(e.is_dir)
                })
                .unwrap_or(false)
        }
    }

    /// The dual-pane half the cursor is over, by geometry: `Left` from
    /// the content's left edge to the split, `Right` from the split to
    /// the content's right edge, `None` outside that band (the rail, a
    /// sidebar region, a docked strip, outside the window). The halves
    /// are PHYSICAL (the pane row is not mirrored under RTL), and which
    /// one is remote is the panes' business, not this test's: the
    /// remote pane can sit on either side.
    pub(crate) fn cursor_pane_side(&self) -> Option<crate::state::SftpPaneSide> {
        // Side-docked tab strip (issue #87): a left dock shifts the
        // content's left edge, a right dock pulls its right edge in.
        let strip_left = self.side_strip_left_offset();
        let strip_right = self.side_strip_reserve() - strip_left;
        // The chat sidebar comes off whichever edge it is docked on
        // (issue #85), so the content band shrinks from that side.
        let (chat_left, chat_right) = self.sidebar_reserve();
        let left_edge = self.vault_rail_width() + strip_left + chat_left;
        let right_edge = self.cur_window_size().width - chat_right - strip_right;
        pane_side_at(self.cur_mouse().x, left_edge, right_edge, self.sftp_chrome.split_ratio)
    }

    /// Sidebar-region widths as a `(left, right)` reserve (issue
    /// #102: both regions can be up at once). Zero per side when that
    /// region isn't on screen for the active tab.
    fn sidebar_reserve(&self) -> (f32, f32) {
        use crate::state::SidebarSide;
        let Some(tab) = self.cur_active_tab().and_then(|i| self.tabs.get(i)) else {
            return (0.0, 0.0);
        };
        let width = |side: SidebarSide| {
            if self.sidebar_region_shown(tab, side) {
                self.vaults.cur().chat_ui.sidebar_width[side.idx()]
            } else {
                0.0
            }
        };
        (width(SidebarSide::Left), width(SidebarSide::Right))
    }

    /// Look up whether a path in the given pane points at a directory,
    /// using cached entries so this is cheap to call per row in the
    /// drag-start path collection.
    pub(crate) fn row_is_dir_in_pane(
        &self,
        side: crate::state::SftpPaneSide,
        path: &str,
    ) -> bool {
        let pane = self.cur_sftp().pane(side);
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
            || self.cursor_pane_side() == Some(dest_side);
        if !over_dest {
            return Task::none();
        }
        let src_remote = self.cur_sftp().pane(drag.origin_side).is_remote;
        let dst_remote = self.cur_sftp().pane(dest_side).is_remote;
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
                Task::done(Message::Sftp(SftpMessage::SftpUploadBatch(paths)))
            }
            // Remote -> Local: download.
            (true, false) => {
                if let Some(dir) = target_folder {
                    self.sftp.download_dest_override = Some(std::path::PathBuf::from(dir));
                }
                // One batch for the whole gesture, mirroring the upload
                // side above. Dispatching per item would raise one
                // `TransferState` per dragged entry against a runner that
                // holds exactly ONE per owner, so the last one to arrive
                // would silently replace the transfer already running.
                Task::done(Message::Sftp(SftpMessage::SftpDownloadBatch(drag.items)))
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
                        Task::done(Message::Sftp(SftpMessage::SftpRelayFolder(from, path)))
                    } else {
                        Task::done(Message::Sftp(SftpMessage::SftpRelay(from, path)))
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

    /// Whether a hybrid terminal tab has an SFTP session (its Files
    /// state carries a mounted remote pane, live or parked). Drives
    /// the tab's mode glyph / status-bar segment / menu labels: the
    /// affordances only exist once a session does (owner QA
    /// 2026-07-05: "Open SFTP session" creates it, the toggle glyph
    /// appears after).
    pub(crate) fn tab_has_sftp_session(&self, tab: &crate::state::TerminalTab) -> bool {
        if tab.files_mode {
            return true;
        }
        let st: &crate::state::SftpState = if self.hybrid_sftp_owner == Some(tab._id) {
            &self.sftp
        } else {
            &tab.files_state
        };
        st.right.host_label.is_some() || st.left.host_label.is_some()
    }

    /// Whether the full SFTP surface is what the user is looking at:
    /// the standalone SFTP view, or a hybrid terminal tab showing its
    /// Files mode. Keyboard type-ahead, row drags and OS file drops
    /// gate on this instead of `active_view == View::Sftp` so they
    /// work on both surfaces.
    pub(crate) fn sftp_surface_visible(&self) -> bool {
        if self.cur_active_tab().is_none() && self.cur_view() == crate::state::View::Sftp {
            return true;
        }
        self.cur_active_tab()
            .and_then(|i| self.tabs.get(i))
            .is_some_and(|t| t.files_mode)
    }

    /// Id of the SFTP tab currently loaded in `self.sftp`, if any.
    pub(crate) fn active_sftp_id(&self) -> Option<uuid::Uuid> {
        self.active_sftp
            .and_then(|i| self.sftp_tabs.get(i))
            .map(|t| t.id)
    }

    /// Post-update invariant repair for the hybrid ownership, called
    /// from `update()` after every message (like `reconcile_tab_order`):
    ///
    /// - a dangling owner (its tab removed by any path that bypasses
    ///   `CloseTab`: close-others, close-all, reconnect rebuilds) is
    ///   dropped and the ghost buffer reset;
    /// - an active Files-mode tab that is not the owner (activated by a
    ///   direct `active_tab = ...` assignment: the close-neighbor
    ///   fallback, `SplitTabPane`, dormant-reopen) is hoisted, so the
    ///   render invariant "Files mode visible => its state is the live
    ///   buffer" holds no matter which path focused the tab.
    ///
    /// Cheap when nothing needs repair (two comparisons).
    pub(crate) fn reconcile_hybrid_sftp(&mut self) {
        if let Some(id) = self.hybrid_sftp_owner
            && !self.tabs.iter().any(|t| t._id == id)
        {
            self.hybrid_sftp_owner = None;
            self.sftp = crate::state::SftpState::default();
        }
        if let Some(tab) = self.cur_active_tab().and_then(|i| self.tabs.get(i))
            && tab.files_mode
            && self.hybrid_sftp_owner != Some(tab._id)
        {
            let id = tab._id;
            self.hoist_hybrid_sftp(id);
        }
    }

    /// Park a hybrid tab's Files state out of the live buffer, back
    /// into its `files_state` slot. No-op when no hybrid tab owns the
    /// buffer; discards the buffer if the owning tab was closed.
    pub(crate) fn park_hybrid_sftp(&mut self) {
        if let Some(id) = self.hybrid_sftp_owner.take() {
            // Same reason as `focus_sftp_tab`: the rows a pending rename
            // was armed against are leaving the live buffer.
            self.sftp_click_gen = self.sftp_click_gen.wrapping_add(1);
            if let Some(tab) = self.tabs.iter_mut().find(|t| t._id == id) {
                *tab.files_state = std::mem::take(&mut self.sftp);
            } else {
                self.sftp = crate::state::SftpState::default();
            }
        }
    }

    /// The Files browsing a tab keeps for ONE of its panes' sessions,
    /// taken out of the tab when that pane leaves it.
    ///
    /// A tab's `files_state` is an SFTP channel multiplexed on the SSH
    /// session of the pane that was focused when Files was mounted, so
    /// it belongs with that pane's session rather than with the tab: a
    /// pane moving out takes the browsing with it, and a pane closing
    /// takes it down. Which pane that is is read off the MOUNT, the way
    /// `hybrid_sftp_remount_dead` reads it: a side whose channel rides
    /// the pane's own session handle, or (a mount whose channel already
    /// died) one mounted on the pane's host label. Only a pane holding
    /// an SSH handle can be that pane: the SFTP console beside a shell
    /// has none (its session drives the console, not the pane) and
    /// wears the shell's host label, so a label match from it would
    /// take the shell's browsing down with the console. `sftp_source()`
    /// is the pane a NEW mount would use, and in a split of two hosts
    /// that is whichever shell is focused now, not the one the browsing
    /// was opened from. Answers `None`, leaving the tab untouched, when
    /// no side of the mount is the pane's.
    ///
    /// The state comes home first (parked out of the live buffer when
    /// this tab owns it), the tab leaves Files mode, and an inherited
    /// SFTP pin stops describing it, the same three steps
    /// `close_tab_sftp_session` takes. Returns whether the tab was
    /// SHOWING Files, so a pane broken out into a tab of its own can
    /// arrive showing the same surface.
    pub(crate) fn take_tab_files_backed_by(
        &mut self,
        tab_idx: usize,
        pane_id: uuid::Uuid,
    ) -> Option<(bool, Box<crate::state::SftpState>)> {
        let tab = self.tabs.get(tab_idx)?;
        let pane = tab.pane_grid.panes.values().find(|p| p.id == pane_id)?;
        let own = pane.session.as_ref().and_then(|t| t.ssh())?;
        let label = pane.label.trim_end_matches(" (disconnected)").to_string();
        let tab_id = tab._id;
        let st: &crate::state::SftpState = if self.hybrid_sftp_owner == Some(tab_id) {
            &self.sftp
        } else {
            &tab.files_state
        };
        let backed = [&st.left, &st.right].into_iter().any(|side| {
            side.is_remote
                && match &side.session {
                    Some(mounted) => std::sync::Arc::ptr_eq(mounted, own),
                    // The channel died, or is being remounted: the host
                    // label is what still names the pane it rode.
                    None => side.host_label.as_deref() == Some(label.as_str()),
                }
        });
        if !backed {
            return None;
        }
        if self.hybrid_sftp_owner == Some(tab_id) {
            self.park_hybrid_sftp();
        }
        // A one-shot directory hint aimed at this surface dies with it.
        self.sftp_open_at_path = None;
        let tab = &mut self.tabs[tab_idx];
        let showing = std::mem::replace(&mut tab.files_mode, false);
        tab.inherited_pin = None;
        Some((showing, std::mem::take(&mut tab.files_state)))
    }

    /// Make the hybrid terminal tab `tab_id` the owner of the live
    /// `self.sftp` buffer: park whichever owner (hybrid or standalone)
    /// holds it, then hoist this tab's `files_state`. No-op when it
    /// already owns the buffer or the id is dangling.
    pub(crate) fn hoist_hybrid_sftp(&mut self, tab_id: uuid::Uuid) {
        if self.hybrid_sftp_owner == Some(tab_id) {
            return;
        }
        self.park_hybrid_sftp();
        // Park a standalone owner into its slot. Unlike the standalone
        // focus flow (where the buffer stays hoisted while the user is
        // elsewhere), here ownership actually moves, so `active_sftp`
        // clears; `SelectSftpTab` re-hoists it via `focus_sftp_tab`.
        if let Some(old) = self.active_sftp.take()
            && let Some(t) = self.sftp_tabs.get_mut(old)
        {
            t.state = std::mem::take(&mut self.sftp);
        }
        if let Some(tab) = self.tabs.iter_mut().find(|t| t._id == tab_id) {
            self.sftp = std::mem::take(&mut *tab.files_state);
            self.hybrid_sftp_owner = Some(tab_id);
        }
    }

    /// Ensure at least one SFTP tab exists, adopting the existing top-level
    /// `self.sftp` as the first tab's live buffer (no swap). Called when the
    /// SFTP surface is first reached so the single-tab case behaves exactly
    /// as before.
    pub(crate) fn ensure_sftp_tab(&mut self) {
        if self.sftp_tabs.is_empty() {
            // A hybrid owner must park first: adopting `self.sftp` while
            // a hybrid tab owns it would steal that tab's Files state
            // into the new standalone tab (double ownership). After the
            // park the buffer is a fresh default, which is exactly what
            // a first standalone tab should adopt.
            self.park_hybrid_sftp();
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
        tab.state.left.columns = self.sftp_chrome.columns_template.clone();
        tab.state.right.columns = self.sftp_chrome.columns_template.clone();
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
        let st = self.sftp_tab_state(idx)?;
        let pane_spec = |p: &crate::state::PaneState| -> crate::state::SftpPaneSpec {
            if p.is_remote {
                p.host_label
                    .as_ref()
                    .and_then(|l| self.vaults.cur().connections.iter().find(|c| &c.label == l))
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

    /// The host an "Open terminal" gesture on the SFTP tab at `idx` lands
    /// on, or `None` when the tab has not picked one yet.
    ///
    /// Two sources, and the second is the one that is easy to forget: a
    /// PINNED SFTP tab restored at boot is dormant, so neither pane carries
    /// a `host_label` until the first focus re-mounts them, but its
    /// `pending_reopen` spec names the connection outright. Reading only the
    /// mounted label hides the entry on exactly the tabs a user cared enough
    /// to pin, which is the opposite of what `79cce345` intended.
    ///
    /// One authority so the menu's visibility gate and the handler cannot
    /// disagree: an entry that draws and then does nothing when clicked is
    /// the bug that commit set out to fix.
    pub(crate) fn sftp_tab_terminal_host(&self, idx: usize) -> Option<String> {
        if let Some(st) = self.sftp_tab_state(idx)
            && let Some(mounted) = st
                .right
                .host_label
                .clone()
                .or_else(|| st.left.host_label.clone())
        {
            return Some(mounted);
        }
        // Dormant: resolve the spec's connection id to today's label
        // (the host may have been renamed since the pin was written).
        let crate::state::PinnedTabSpec::Sftp { left, right, .. } =
            self.sftp_tabs.get(idx)?.pending_reopen.as_ref()?
        else {
            return None;
        };
        [right, left].into_iter().find_map(|p| match p {
            crate::state::SftpPaneSpec::Remote(id) => self
                .vaults.cur().connections
                .iter()
                .find(|c| c.id == *id)
                .map(|c| c.label.clone()),
            crate::state::SftpPaneSpec::Local => None,
        })
    }

    /// Whether the SFTP tab at `idx` holds a live remote session: a pane
    /// mounted on an SSH connection that is still up. The question behind
    /// the "close a tab or the app with a live session" guard, which
    /// counts these tabs alongside the terminal ones.
    pub(crate) fn sftp_tab_is_live(&self, idx: usize) -> bool {
        self.sftp_tab_state(idx).is_some_and(|st| {
            [&st.left, &st.right]
                .iter()
                .any(|p| p.session.as_ref().is_some_and(|s| s.is_alive()))
        })
    }

    /// Whether the SFTP tab at `idx` has unsaved work worth a close-guard:
    /// an in-flight transfer or a dirty edit-session. Reads the live buffer
    /// for the active tab, the parked slot otherwise.
    pub(crate) fn sftp_tab_has_unsaved(&self, idx: usize) -> bool {
        match self.sftp_tab_state(idx) {
            Some(st) => sftp_state_has_unsaved(st),
            None => false,
        }
    }

    /// Tear down a hybrid terminal tab's SFTP session: park the live buffer
    /// if this tab owns it (which resets `self.sftp`), discard the parked
    /// Files state, drop Files mode, and select the tab when the close came
    /// from a background affordance. The guards (in-flight transfer decline,
    /// dirty-edit confirm) live at the call sites: the `CloseTabSftpSession`
    /// handler and the close-guard modal's confirm.
    pub(crate) fn close_tab_sftp_session(&mut self, tab_id: uuid::Uuid) -> Task<Message> {
        if self.hybrid_sftp_owner == Some(tab_id) {
            self.park_hybrid_sftp();
        }
        let Some(idx) = self.tabs.iter().position(|t| t._id == tab_id) else {
            return Task::none();
        };
        let tab = &mut self.tabs[idx];
        tab.files_mode = false;
        *tab.files_state = Default::default();
        // Same reason as the detach: with the SFTP half gone, an
        // inherited SFTP pin no longer describes this tab.
        tab.inherited_pin = None;
        // Back on the terminal surface; select the tab if it wasn't
        // already active (a background close leaves focus).
        if self.cur_active_tab() != Some(idx) {
            return self.update(Message::Tabs(TabsMessage::SelectTab(idx)));
        }
        Task::none()
    }

    /// The session behind a tab's Files browsing dropped: say so on the
    /// surface, in the slot the next operation's error would take.
    ///
    /// The mount itself is kept on purpose. It is what
    /// [`Self::hybrid_sftp_remount_dead`] lands on when the pane
    /// reconnects (the auto-reconnect of a lone pane, the restart of one
    /// in a split), at the directory the user was in, and that remount
    /// clears the notice. Matched by host label the way the remount is,
    /// and only on a mount whose channel is actually dead, so a second
    /// pane on the same host does not mark a browser it did not back.
    pub(crate) fn hybrid_sftp_mark_dead(&mut self, tab_idx: usize, pane_id: uuid::Uuid) {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        let tab_id = tab._id;
        let Some(label) = tab
            .pane_grid
            .panes
            .values()
            .find(|p| p.id == pane_id)
            .map(|p| p.label.trim_end_matches(" (disconnected)").to_string())
        else {
            return;
        };
        let hoisted = self.hybrid_sftp_owner == Some(tab_id);
        for side in [
            crate::state::SftpPaneSide::Left,
            crate::state::SftpPaneSide::Right,
        ] {
            let st: &mut crate::state::SftpState = if hoisted {
                &mut self.sftp
            } else {
                match self.tabs.get_mut(tab_idx) {
                    Some(t) => &mut t.files_state,
                    None => return,
                }
            };
            let pane = st.pane_mut(side);
            if !pane.is_remote || pane.host_label.as_deref() != Some(label.as_str()) {
                continue;
            }
            if pane.session.as_ref().is_some_and(|s| s.is_alive()) {
                continue;
            }
            pane.error = Some(crate::i18n::t("sftp_link_down").to_string());
        }
    }

    /// Remount a hybrid terminal tab's dead SFTP mounts onto a freshly
    /// connected session (issue #63: a terminal reconnect used to leave
    /// the tab's Files surface on the old, closed channel, and Retry
    /// re-listed the same dead client forever). Every Files pane still
    /// mounted on the reconnected pane's host whose session is gone or
    /// no longer alive gets a new SFTP channel multiplexed on `ssh` and
    /// is re-listed at its previous directory (home fallback when the
    /// path vanished). Panes mounted on a different host, or whose
    /// session is still alive (borrowed from another live tab), are
    /// left alone. Works on the hoisted buffer or the parked
    /// `files_state` alike; completions are stamped with the tab id so
    /// `route_sftp_async` lands them wherever the state lives by then.
    pub(crate) fn hybrid_sftp_remount_dead(
        &mut self,
        tab_idx: usize,
        pane_id: uuid::Uuid,
        ssh: &std::sync::Arc<oryxis_ssh::SshSession>,
    ) -> Task<Message> {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return Task::none();
        };
        let tab_id = tab._id;
        let Some(label) = tab
            .pane_grid
            .panes
            .values()
            .find(|p| p.id == pane_id)
            .map(|p| p.label.trim_end_matches(" (disconnected)").to_string())
        else {
            return Task::none();
        };
        let hoisted = self.hybrid_sftp_owner == Some(tab_id);
        let mut tasks: Vec<Task<Message>> = Vec::new();
        for side in [
            crate::state::SftpPaneSide::Left,
            crate::state::SftpPaneSide::Right,
        ] {
            let st: &mut crate::state::SftpState = if hoisted {
                &mut self.sftp
            } else {
                match self.tabs.get_mut(tab_idx) {
                    Some(t) => &mut t.files_state,
                    None => return Task::none(),
                }
            };
            let pane = st.pane_mut(side);
            if !pane.is_remote || pane.host_label.as_deref() != Some(label.as_str()) {
                continue;
            }
            if pane.session.as_ref().is_some_and(|s| s.is_alive()) {
                continue;
            }
            // Dead mount on the reconnected host: drop the stale channel
            // and remount on the fresh handle at the same directory.
            pane.session = None;
            pane.client = None;
            pane.remote_loading = true;
            pane.error = None;
            let hint = Some(pane.remote_path.clone()).filter(|p| !p.is_empty());
            let host = label.clone();
            let session = ssh.clone();
            let session_for_task = ssh.clone();
            tasks.push(Task::perform(
                async move {
                    let client = session_for_task
                        .open_sftp()
                        .await
                        .map_err(|e| e.to_string())?;
                    let (initial, entries) =
                        crate::dispatch_sftp::initial_remote_listing(&client, hint)
                            .await?;
                    Ok::<_, String>((client, initial, entries))
                },
                move |result| match result {
                    Ok((client, path, entries)) => Message::sftp_owned(
                        Some(tab_id),
                        SftpMessage::HostMounted(
                            side,
                            host.clone(),
                            session.clone(),
                            client,
                            path,
                            entries,
                        ),
                    ),
                    Err(e) => Message::sftp_owned(
                        Some(tab_id),
                        SftpMessage::RemoteError(side, e),
                    ),
                },
            ));
        }
        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
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
            was_owner && self.cur_active_tab().is_none() && self.cur_view() == crate::state::View::Sftp;
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
        // (discarded) state. Adopt the nearest remaining tab OF THIS
        // WINDOW, else reset: a tab in another window's strip is that
        // window's to show, and hoisting it here would put it on screen
        // in a window whose strip does not hold it.
        let next = (idx..self.sftp_tabs.len())
            .chain((0..idx.min(self.sftp_tabs.len())).rev())
            .find(|&i| self.shows_sftp_tab(i));
        if was_owner {
            match next {
                Some(next) => {
                    self.sftp = std::mem::take(&mut self.sftp_tabs[next].state);
                    self.active_sftp = Some(next);
                }
                None => self.sftp = crate::state::SftpState::default(),
            }
        }
        if was_focused_surface {
            if next.is_none() {
                // No SFTP tab left in this window. The resident window
                // goes home; an extra window shows one of its terminal
                // tabs, or its hosts screen when it has none (only a
                // MOVE that empties a window closes it).
                if self.window_ctx.is_none() {
                    return Task::done(Message::Navigation(NavigationMessage::ChangeView(crate::state::View::Dashboard)));
                }
                self.active_view = crate::state::View::Terminal;
                let first = self.tab_order.iter().find_map(|r| match r {
                    crate::state::TabRef::Terminal(t) => {
                        self.tabs.iter().position(|tab| tab._id == *t)
                    }
                    _ => None,
                });
                return match first {
                    Some(i) => Task::done(Message::Tabs(crate::app::TabsMessage::SelectTab(i))),
                    None => Task::none(),
                };
            }
            self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
            self.refresh_sftp_local(crate::state::SftpPaneSide::Right);
        }
        Task::none()
    }

    /// Close every SFTP tab of THIS window's strip except the one at
    /// `keep_idx`: another window's tabs are not on screen here to be
    /// closed from here (the terminal tabs' rule, `shows_tab`). Reuses
    /// `close_sftp_tab` per dropped tab so `active_sftp`, buffer adoption and
    /// `tab_order` stay consistent (instead of a hand-rolled `retain` that
    /// hard-codes `active_sftp = Some(0)`).
    pub(crate) fn close_other_sftp_tabs(&mut self, keep_idx: usize) -> Task<Message> {
        let Some(keep_id) = self.sftp_tabs.get(keep_idx).map(|t| t.id) else {
            return Task::none();
        };
        let mut task = Task::none();
        while let Some(idx) = (0..self.sftp_tabs.len())
            .find(|&i| self.sftp_tabs[i].id != keep_id && self.shows_sftp_tab(i))
        {
            // Each dropped tab lands on the reopen stack, same as closing
            // it on its own would (issue #186).
            self.remember_closed_sftp_tab(idx);
            task = self.close_sftp_tab(idx);
        }
        task
    }

    /// Whether "Close other tabs" has anything to close from `keep_idx`:
    /// another SFTP tab in this window's strip.
    pub(crate) fn has_other_sftp_tabs(&self, keep_idx: usize) -> bool {
        (0..self.sftp_tabs.len()).any(|i| i != keep_idx && self.shows_sftp_tab(i))
    }

    /// Whether closing every tab but `keep_idx` would drop unsaved work, i.e.
    /// any other tab of this window has an in-flight transfer or a dirty
    /// edit-session.
    pub(crate) fn other_sftp_tabs_have_unsaved(&self, keep_idx: usize) -> bool {
        (0..self.sftp_tabs.len())
            .any(|i| i != keep_idx && self.shows_sftp_tab(i) && self.sftp_tab_has_unsaved(i))
    }

    /// Owning tab id to stamp on an SFTP async-continuation message: the tab
    /// currently being routed (mid-`route_sftp_async`) if any, else the
    /// focused tab. User-initiated transfers (not mid-route) get the focused
    /// tab; chained continuations get the originating tab. `None` when no
    /// SFTP tab is active, in which case there is nothing to route to (a
    /// nil-UUID stamp would just be dropped by `route_sftp_async`).
    pub(crate) fn current_sftp_owner(&self) -> Option<uuid::Uuid> {
        // A hybrid tab and a standalone tab never own the buffer at the
        // same time (hoisting one parks the other), so the chain is
        // unambiguous. The hybrid id is validated against the live tabs:
        // reconnect flows rebuild a terminal tab without going through
        // CloseTab, which can leave a dangling owner until the next
        // park; a fresh transfer must not get stamped with it.
        self.routing_sftp
            .or_else(|| {
                self.hybrid_sftp_owner
                    .filter(|id| self.tabs.iter().any(|t| t._id == *id))
            })
            .or_else(|| self.active_sftp_id())
    }

    /// The slot a transfer's bookkeeping lives in, resolved from the id
    /// its messages already carry.
    ///
    /// Two owners run over the one queue runner: the dual-pane SFTP
    /// surface (a standalone tab or a terminal tab in Files mode), whose
    /// state `route_sftp_async` has already swapped into the live buffer
    /// by the time any handler runs, and a terminal pane's sidebar Files
    /// browser, which owns its slot outright.
    ///
    /// Resolving here rather than carrying a `TransferOwner` through four
    /// message variants and every send site is safe because tab ids and
    /// pane ids come from disjoint UUID pools: an id can only ever mean
    /// one of them.
    pub(crate) fn transfer_slot_mut(
        &mut self,
        owner: uuid::Uuid,
    ) -> Option<&mut crate::state::TransferSlot> {
        if self.is_sidebar_owner(owner) {
            return self.pane_by_id_any_tab(owner).map(|p| &mut p.files.transfer);
        }
        Some(&mut self.sftp.transfer)
    }

    /// Is this id a pane's sidebar browser rather than an SFTP surface?
    pub(crate) fn is_sidebar_owner(&self, owner: uuid::Uuid) -> bool {
        self.tabs
            .iter()
            .any(|t| t.pane_grid.panes.values().any(|p| p.id == owner))
    }

    /// Where a failed transfer's message goes, so the user still sees it
    /// after a toast would have gone. Each surface renders its own notice
    /// banner over the listing it keeps (the `error` slot would take the
    /// listing over, with a Retry that re-dials: a failed upload is not a
    /// lost connection), and writing into the wrong one is a silent drop,
    /// which is half of what the 3 GB field report actually cost.
    pub(crate) fn transfer_set_error(
        &mut self,
        owner: uuid::Uuid,
        side: crate::state::SftpPaneSide,
        message: String,
    ) {
        if self.is_sidebar_owner(owner) {
            if let Some(pane) = self.pane_by_id_any_tab(owner) {
                pane.files.notice = Some(message);
            }
            return;
        }
        self.sftp.pane_mut(side).notice = Some(message);
    }

    /// Re-list what a finished transfer changed on the sidebar side.
    /// `None` for a surface owner, whose refresh is direction-specific
    /// and stays with the rest of the dual-pane logic in the runner.
    pub(crate) fn sidebar_transfer_refresh(&mut self, owner: uuid::Uuid) -> Option<Task<Message>> {
        if !self.is_sidebar_owner(owner) {
            return None;
        }
        let pane = self.pane_by_id_any_tab(owner)?;
        let client = pane.files.client.clone()?;
        let path = pane.files.path.clone();
        pane.files.loading = true;
        pane.files.error = None;
        let seq = pane.files.next_req();
        Some(crate::dispatch_sidebar_files::list_dir_task(
            client, path, owner, seq,
        ))
    }

    /// A running transfer as the tab strip's progress border, the same
    /// affordance OSC 9;4 and ZMODEM already draw there.
    ///
    /// The point is that a transfer stays visible from OUTSIDE the tab
    /// running it: the in-tab bar answers "how far along", the strip
    /// answers "is something still going", which is the question a user
    /// who switched tabs is asking. `None` when nothing is transferring,
    /// so the border simply is not drawn.
    ///
    /// The percentage rule matches `transfer_progress_strip` exactly: by
    /// bytes when the sizes are known, by item count otherwise (an
    /// all-directory transfer has no bytes to speak of). Two surfaces
    /// disagreeing about the same transfer's progress would be worse than
    /// either being slightly wrong.
    pub(crate) fn transfer_border(slot: &crate::state::TransferSlot) -> Option<oryxis_terminal::Progress> {
        let transfer = slot.state.as_ref()?;
        let pct = if slot.bytes_total > 0 {
            let done = slot
                .bytes_done
                .load(std::sync::atomic::Ordering::Relaxed)
                .min(slot.bytes_total);
            (done as f64 / slot.bytes_total as f64) * 100.0
        } else if transfer.total > 0 {
            (transfer.completed as f64 / transfer.total as f64) * 100.0
        } else {
            0.0
        };
        Some(oryxis_terminal::Progress {
            // 1 is OSC 9;4's "normal", the determinate bar.
            state: 1,
            value: pct.clamp(0.0, 100.0) as u8,
        })
    }

    /// Transfer slot of a standalone SFTP tab, honouring the
    /// swap-on-focus invariant: the surface on screen has its state
    /// HOISTED into the live buffer, and the tab's own copy is a stale
    /// parked one. Reading the wrong side leaves a running transfer
    /// looking frozen at whatever it was when the tab lost focus.
    /// The LIVE state of a standalone SFTP tab.
    ///
    /// One authority for the swap-on-focus invariant, because getting it
    /// wrong reads another tab's host: `self.sftp` holds the state of
    /// whichever surface is hoisted, and `active_sftp` alone does not
    /// say that is this tab. A terminal tab in Files mode parks the
    /// standalone one and takes the buffer, so with a hybrid owner the
    /// tab's own slot is the current one even when `active_sftp` still
    /// points here.
    pub(crate) fn sftp_tab_state(&self, idx: usize) -> Option<&crate::state::SftpState> {
        if self.active_sftp == Some(idx) && self.hybrid_sftp_owner.is_none() {
            return Some(&self.sftp);
        }
        self.sftp_tabs.get(idx).map(|t| &t.state)
    }

    pub(crate) fn sftp_tab_slot(&self, idx: usize) -> &crate::state::TransferSlot {
        if self.active_sftp == Some(idx) && self.hybrid_sftp_owner.is_none() {
            &self.sftp.transfer
        } else {
            &self.sftp_tabs[idx].state.transfer
        }
    }

    /// Same, for a terminal tab's Files mode.
    pub(crate) fn hybrid_tab_slot<'a>(
        &'a self,
        tab: &'a crate::state::TerminalTab,
    ) -> &'a crate::state::TransferSlot {
        if self.hybrid_sftp_owner == Some(tab._id) {
            &self.sftp.transfer
        } else {
            &tab.files_state.transfer
        }
    }

    /// Dispatch an SFTP async-continuation message against its owning tab.
    /// Temporarily swaps that tab's parked state into `self.sftp` (no-op if it
    /// is already the focused tab), runs the normal handler chain, then swaps
    /// back. Drops the message if the owning tab was closed meanwhile.
    pub(crate) fn route_sftp_async(&mut self, id: uuid::Uuid, message: Message) -> Task<Message> {
        // Unwrap the owner-routing envelope (the id was already read from
        // it by `sftp_async_owner`); the inner message is what the handler
        // chain understands.
        let message = match message {
            Message::SftpFor(_, inner) => Message::Sftp(*inner),
            m => m,
        };
        if let Some(idx) = self.sftp_tabs.iter().position(|t| t.id == id) {
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
            return task;
        }
        // Hybrid terminal tab (Files mode): same swap dance against its
        // `files_state` slot.
        if self.tabs.iter().any(|t| t._id == id) {
            let is_owner = self.hybrid_sftp_owner == Some(id);
            if !is_owner
                && let Some(tab) = self.tabs.iter_mut().find(|t| t._id == id)
            {
                std::mem::swap(&mut self.sftp, &mut *tab.files_state);
            }
            let prev = self.routing_sftp.replace(id);
            let task = self.dispatch_message(message);
            self.routing_sftp = prev;
            if !is_owner
                && let Some(tab) = self.tabs.iter_mut().find(|t| t._id == id)
            {
                std::mem::swap(&mut self.sftp, &mut *tab.files_state);
            }
            return task;
        }
        // A sidebar Files browser owns its slot outright, so there is
        // nothing to swap: only the routing stamp, so `current_sftp_owner`
        // answers with this pane instead of whatever surface happens to be
        // focused, and the accessors resolve to the pane's own slot.
        if self.is_sidebar_owner(id) {
            let prev = self.routing_sftp.replace(id);
            let task = self.dispatch_message(message);
            self.routing_sftp = prev;
            return task;
        }
        // Owner closed meanwhile: drop the continuation.
        Task::none()
    }

    /// Make `idx` the focused SFTP tab: park the currently-loaded tab's state
    /// into its slot and hoist `idx`'s state into `self.sftp`. No-op if `idx`
    /// is already loaded or out of range. Does not touch `active_view` /
    /// `active_tab`; the caller drives the surface switch.
    pub(crate) fn focus_sftp_tab(&mut self, idx: usize) {
        if idx >= self.sftp_tabs.len() {
            return;
        }
        // Swapping which surface owns the live buffer retires a deferred
        // slow-click rename: its target row belongs to the surface being
        // parked, and the row list under it is about to be replaced.
        self.sftp_click_gen = self.sftp_click_gen.wrapping_add(1);
        // A hybrid tab holding the buffer parks first (its state goes
        // home); only then can a standalone tab be hoisted.
        self.park_hybrid_sftp();
        if self.active_sftp == Some(idx) {
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

/// Enumerate a local directory into `LocalEntry` rows, unsorted. Pure
/// and blocking: callers pick the thread (`spawn_blocking` for first
/// entry into a path, inline for warm in-place refreshes).
pub(crate) fn list_local_dir_blocking(
    local_path: &std::path::Path,
) -> Result<Vec<crate::state::LocalEntry>, String> {
    // Bare WSL UNC roots (`\\wsl$`, `\\wsl.localhost`) can't be
    // enumerated via `read_dir`, Windows treats them as servers
    // with no share and returns ERROR_PATH_NOT_FOUND. Synthesize
    // distro entries from `wsl.exe -l -q` instead so the user can
    // step into a distro just by clicking it.
    if is_wsl_root(local_path) {
        return Ok(list_wsl_distros_for_pane()
            .into_iter()
            .map(|distro| crate::state::LocalEntry {
                name: distro,
                is_dir: true,
                is_symlink: false,
                size: 0,
                modified: None,
                mode: None,
                uid: None,
                gid: None,
            })
            .collect());
    }
    let read = std::fs::read_dir(local_path).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for entry in read.flatten() {
        // The entry's own metadata: a symbolic link reports on itself.
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_symlink = metadata.file_type().is_symlink();
        // Whether it opens as a folder is the TARGET's answer, so a link
        // to a folder is entered like one; a dangling link is a file.
        let is_dir = if is_symlink {
            std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir())
        } else {
            metadata.is_dir()
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
        entries.push(crate::state::LocalEntry {
            name,
            is_dir,
            is_symlink,
            size: metadata.len(),
            modified: metadata.modified().ok(),
            mode,
            uid,
            gid,
        });
    }
    Ok(entries)
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
/// Whether an SFTP state carries unsaved work worth a close-guard: an
/// in-flight transfer or ANY registered edit watch. A clean watch counts
/// too: the external editor is still open, and silently dropping the
/// watch would turn its future saves into local-only writes with no
/// upload and no warning. Shared by the standalone tab close guards and
/// the hybrid Close-SFTP-session guard so the two paths can never
/// diverge on what counts as discardable.
pub(crate) fn sftp_state_has_unsaved(st: &crate::state::SftpState) -> bool {
    st.transfer.state.is_some() || !st.edit_watches.is_empty()
}

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
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
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

/// The pane opposite a row's, as a transfer destination
/// (`Oryxis::sftp_cross_pane`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SftpCrossPane {
    pub(crate) other_side: crate::state::SftpPaneSide,
    pub(crate) other_is_remote: bool,
    pub(crate) ready: bool,
}

/// The dual-pane half `x` falls in, for a content band from `left_edge`
/// to `right_edge` split at `split_ratio` of its width (the user's
/// resizable divider, not a fixed 50/50). `None` outside the band.
fn pane_side_at(x: f32, left_edge: f32, right_edge: f32, split_ratio: f32) -> Option<crate::state::SftpPaneSide> {
    let split = left_edge + (right_edge - left_edge).max(0.0) * split_ratio;
    if x > left_edge && x < split {
        Some(crate::state::SftpPaneSide::Left)
    } else if x > split && x < right_edge {
        Some(crate::state::SftpPaneSide::Right)
    } else {
        None
    }
}

#[cfg(test)]
mod pane_side_tests {
    use super::pane_side_at;
    use crate::state::SftpPaneSide::{Left, Right};

    #[test]
    fn each_half_answers_for_itself_whichever_pane_is_remote() {
        // Band 100..900 split at 30%: the divider sits at 340.
        assert_eq!(pane_side_at(200.0, 100.0, 900.0, 0.3), Some(Left));
        assert_eq!(pane_side_at(600.0, 100.0, 900.0, 0.3), Some(Right));
    }

    #[test]
    fn outside_the_band_is_no_pane() {
        assert_eq!(pane_side_at(50.0, 100.0, 900.0, 0.5), None);
        assert_eq!(pane_side_at(950.0, 100.0, 900.0, 0.5), None);
        assert_eq!(pane_side_at(-1.0, 100.0, 900.0, 0.5), None);
    }
}
