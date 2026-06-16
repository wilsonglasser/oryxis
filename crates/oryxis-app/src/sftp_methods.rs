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
