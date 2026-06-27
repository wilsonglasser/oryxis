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

use std::time::Duration;

/// How long a transient error toast stays on screen before auto-clearing.
const TOAST_DURATION: Duration = Duration::from_millis(2600);
/// Max gap between two clicks on the same folder to count as a double-click.
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);
/// A second click on an already-selected row this long after the first (i.e.
/// slower than a double-click, but still a deliberate gesture) arms an inline
/// rename, mirroring Explorer / Finder slow-click-to-rename.
const SLOW_RENAME_WINDOW: Duration = Duration::from_millis(1500);
/// Idle gap after which type-ahead starts a fresh search instead of
/// appending to the previous one.
const TYPE_AHEAD_RESET: Duration = Duration::from_millis(900);
/// Debounce before type-ahead actually searches, so fast typing resolves
/// once with the full buffer instead of on every keystroke.
const TYPE_AHEAD_DEBOUNCE: Duration = Duration::from_millis(150);

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
    NoCommonAlgo {
        category: oryxis_ssh::NegCategory,
        server_offers: Vec<String>,
    },
}

impl Oryxis {
    /// Auto-fit `col` in `side`'s pane to the widest value across every row
    /// (issue #45). Measures through the renderer's font system, sets the new
    /// width (clamped), then re-seeds + persists the column template.
    fn autofit_sftp_column(&mut self, side: SftpPaneSide, col: crate::state::SftpColumn) {
        let target = {
            let pane = self.sftp.pane(side);
            crate::views::sftp::autofit_column_width(
                pane.is_remote,
                &pane.remote_entries,
                &pane.local_entries,
                col,
            )
        };
        self.sftp.pane_mut(side).columns.width.set_autofit(col, target);
        self.sftp_columns_template = self.sftp.pane(side).columns.clone();
        self.persist_sftp_columns();
    }

    /// Arm a pending internal drag for a pressed SFTP row. Stays
    /// `active = false` until the cursor reaches the other pane, so a plain
    /// click still flows through. Called both from the global left-press
    /// (via `hovered_row`) and from the row button's own `on_press`: the
    /// latter is the reliable path for a truncated row, whose hover tooltip
    /// can drop `hovered_row` before the press lands (issue: truncated names
    /// wouldn't drag). No-op if a drag is already armed for this press.
    fn arm_sftp_row_drag(&mut self, side: SftpPaneSide, path: String, is_dir: bool) {
        if self.sftp.drag.is_some() {
            return;
        }
        // Drag the entire same-pane selection if the pressed row is part of
        // it; otherwise drag just this row.
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
        let pressed_in_selection = same_side.iter().any(|(p, _)| p == &path);
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

    /// Apply the in-progress inline rename (Enter, or a click outside the
    /// input). Logs on success; a remote rename runs async and re-lists the
    /// directory via `SftpRenamed`. No-op when nothing is being renamed or
    /// the new name is blank. Does not touch `swallow_next_activate` (the
    /// keyboard-commit path sets that itself).
    fn commit_rename(&mut self) -> Task<Message> {
        let Some(rn) = self.sftp.rename.take() else {
            return Task::none();
        };
        let new_name = rn.input.trim().to_string();
        if new_name.is_empty() {
            return Task::none();
        }
        if !self.sftp.pane(rn.side).is_remote {
            let original = std::path::PathBuf::from(&rn.original_path);
            let Some(parent) = original.parent().map(|p| p.to_path_buf()) else {
                self.sftp.pane_mut(rn.side).error = Some("Cannot rename root".into());
                return Task::none();
            };
            let dest = parent.join(&new_name);
            match std::fs::rename(&original, &dest) {
                Ok(()) => self.push_sftp_log(
                    crate::state::SftpLogLevel::Ok,
                    format!("{} {}", crate::i18n::t("sftp_log_renamed"), new_name),
                ),
                Err(e) => self.sftp.pane_mut(rn.side).error = Some(e.to_string()),
            }
            self.refresh_sftp_local(rn.side);
            Task::none()
        } else {
            let Some(client) = self.sftp.pane(rn.side).client.clone() else {
                return Task::none();
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
            Task::perform(
                async move { client.rename(&from, &dest).await.map_err(|e| e.to_string()) },
                move |result| match result {
                    Ok(()) => Message::SftpRenamed(side, reload_path.clone(), new_name.clone()),
                    Err(e) => Message::SftpOpResult(side, e, true),
                },
            )
        }
    }

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

                // Captured for the map closure (conn is moved into the
                // producer). The retry re-runs this same SFTP mount.
                let sftp_conn_id = conn.id;
                let stream = iced::stream::channel::<SftpConnectMsg>(
                    8,
                    move |mut sender: iced::futures::channel::mpsc::Sender<SftpConnectMsg>| async move {
                        let engine = SshEngine::new()
                            .with_host_key_check(host_key_check)
                            .with_host_key_ask(hk_ask_tx)
                            .with_keepalive(keepalive)
                            .with_algorithm_overrides(
                                conn.ciphers.clone(),
                                conn.kex.clone(),
                                conn.macs.clone(),
                                conn.host_key_algorithms.clone(),
                            )
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

                        // First, the transport handshake on its own so a
                        // "no common algorithm" failure routes to the legacy
                        // fallback dialog instead of a generic error string.
                        let session = match engine
                            .connect_with_resolver(
                                &conn,
                                password.as_deref(),
                                private_key.as_deref(),
                                80,
                                24,
                                resolver.as_ref(),
                            )
                            .await
                        {
                            Ok((s, _rx)) => Arc::new(s),
                            Err(e) => {
                                if let Some(nf) = e.negotiation_failure() {
                                    let _ = sender
                                        .send(SftpConnectMsg::NoCommonAlgo {
                                            category: nf.category,
                                            server_offers: nf.server_offers,
                                        })
                                        .await;
                                } else {
                                    let _ = sender
                                        .send(SftpConnectMsg::Done(Err(e.to_string())))
                                        .await;
                                }
                                return;
                            }
                        };
                        let result = async {
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
                    SftpConnectMsg::NoCommonAlgo { category, server_offers } => {
                        Message::SshNoCommonAlgo {
                            conn_id: sftp_conn_id,
                            category,
                            server_offers,
                            retry: Box::new(Message::SftpPickHost(idx)),
                        }
                    }
                }));
            }
            Message::SftpRemountPane(side, idx) => {
                // Point the picker at this side, then reuse the full mount
                // pipeline. Dispatched once per side, so each runs in its own
                // update cycle with the correct target (no shared-field race).
                self.sftp.picker_target = side;
                return self.handle_sftp(Message::SftpPickHost(idx));
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
                let tab_label = label.clone();
                let host_for_log = label.clone();
                let entry_count = entries.len();
                let path_for_log = path.clone();
                let pane = self.sftp.pane_mut(side);
                pane.is_remote = true;
                pane.host_label = Some(label);
                pane.session = Some(session);
                pane.client = Some(client);
                pane.remote_path = path;
                pane.remote_entries = entries;
                pane.remote_loading = false;
                pane.error = None;
                // Inherit the mounted host's name as the tab label (last mount
                // wins when both panes are remote).
                if let Some(i) = self.active_sftp
                    && let Some(t) = self.sftp_tabs.get_mut(i)
                {
                    t.label = tab_label;
                }
                self.push_sftp_log(
                    crate::state::SftpLogLevel::Ok,
                    format!("{} {}", crate::i18n::t("sftp_log_connected"), host_for_log),
                );
                self.push_sftp_log(
                    crate::state::SftpLogLevel::Info,
                    format!(
                        "{} {} ({} {})",
                        crate::i18n::t("sftp_log_listed"),
                        path_for_log,
                        entry_count,
                        crate::i18n::t("sftp_log_items"),
                    ),
                );
            }
            Message::SftpRemoteError(side, msg) => {
                // A failed navigation has no new listing to land the cursor on.
                if matches!(&self.sftp.pending_focus, Some((s, _)) if *s == side) {
                    self.sftp.pending_focus = None;
                }
                let had_listing = !self.sftp.pane(side).remote_entries.is_empty();
                // Hard failure (nothing to fall back on) logs as an error;
                // a soft failure that keeps the previous listing is a warning.
                self.push_sftp_log(
                    if had_listing {
                        crate::state::SftpLogLevel::Warn
                    } else {
                        crate::state::SftpLogLevel::Error
                    },
                    format!("{} {}", crate::i18n::t("sftp_log_error"), msg),
                );
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
                            tokio::time::sleep(TOAST_DURATION).await
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
                // Also dismiss any open menu (Refresh routes here).
                self.sftp.close_menus();
                let client = match self.sftp.pane(side).client.clone() {
                    Some(c) => c,
                    None => {
                        // No client to load from: drop any cursor target
                        // queued for this side so a later successful load
                        // doesn't consume a stale one.
                        if matches!(&self.sftp.pending_focus, Some((s, _)) if *s == side) {
                            self.sftp.pending_focus = None;
                        }
                        return Ok(Task::none());
                    }
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
                let entry_count = entries.len();
                let path_for_log = path.clone();
                let pane = self.sftp.pane_mut(side);
                // Only a genuine directory change resets the scroll. A
                // same-path reload (Refresh, post-op reload) keeps the
                // scrollable's id, so iced preserves the visual scroll;
                // zeroing list_scroll_y there would desync our tracked
                // offset from the widget and break edge-based scrolling.
                let changed_dir = pane.remote_path != path;
                pane.remote_path = path;
                pane.remote_entries = entries;
                pane.remote_loading = false;
                if changed_dir {
                    pane.list_scroll_y = 0.0;
                }
                // Selection is path-keyed; navigation invalidates it.
                self.sftp.selected_rows.clear();
                self.sftp.selection_anchor = None;
                self.sftp.parent_cursor = false;
                self.push_sftp_log(
                    crate::state::SftpLogLevel::Info,
                    format!(
                        "{} {} ({} {})",
                        crate::i18n::t("sftp_log_listed"),
                        path_for_log,
                        entry_count,
                        crate::i18n::t("sftp_log_items"),
                    ),
                );
                // Folder descent / back-navigation: now the listing is in,
                // drop the keyboard cursor where the move queued it.
                if let Some(task) = self.sftp_take_pending_focus(side) {
                    return Ok(task);
                }
            }
            Message::SftpUp(side) => {
                if self.sftp.pane(side).is_remote {
                    let cur = self.sftp.pane(side).remote_path.clone();
                    // Land the cursor on the folder we're leaving once the
                    // parent loads (its full path in the parent listing).
                    let child = cur.trim_end_matches('/').to_string();
                    if !child.is_empty() {
                        self.sftp.pending_focus =
                            Some((side, crate::state::SftpPendingFocus::Path(child)));
                    }
                    let parent = parent_path(&cur);
                    return Ok(Task::done(Message::SftpNavigateRemote(side, parent)));
                }
                if let Some(p) = self.sftp.pane(side).local_path.parent() {
                    let p = p.to_path_buf();
                    // The folder we're leaving, as it'll appear in the parent.
                    let child = self
                        .sftp
                        .pane(side)
                        .local_path
                        .to_string_lossy()
                        .into_owned();
                    {
                        let pane = self.sftp.pane_mut(side);
                        pane.local_path = p;
                        // New directory -> fresh scrollable starts at the top.
                        pane.list_scroll_y = 0.0;
                    }
                    self.sftp.selected_rows.clear();
                    self.sftp.selection_anchor = None;
                    self.sftp.parent_cursor = false;
                    self.refresh_sftp_local(side);
                    // Local listing is synchronous: focus the folder we left.
                    return Ok(self.sftp_apply_pending_focus(
                        side,
                        crate::state::SftpPendingFocus::Path(child),
                    ));
                }
            }
            Message::SftpNavigateLocal(side, path) => {
                {
                    let pane = self.sftp.pane_mut(side);
                    // Only a real directory change resets the scroll (see
                    // SftpRemoteLoaded): a same-path navigate keeps the
                    // scrollable id and its preserved scroll position.
                    let changed_dir = pane.local_path != path;
                    pane.local_path = path;
                    pane.drives_open = false;
                    pane.actions_open = false;
                    if changed_dir {
                        pane.list_scroll_y = 0.0;
                    }
                }
                self.sftp.left.actions_open = false;
                self.sftp.right.actions_open = false;
                self.sftp.selected_rows.clear();
                self.sftp.selection_anchor = None;
                self.sftp.parent_cursor = false;
                self.refresh_sftp_local(side);
                // Folder descent into a local folder: the (synchronous)
                // listing is populated, so land the queued cursor now.
                if let Some(task) = self.sftp_take_pending_focus(side) {
                    return Ok(task);
                }
            }
            Message::SftpRefreshLocal(side) => {
                self.sftp.close_menus();
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
            Message::SelectSftpTab(idx) => {
                if idx < self.sftp_tabs.len() {
                    self.focus_sftp_tab(idx);
                    self.active_tab = None;
                    self.active_view = crate::state::View::Sftp;
                    self.show_burger_menu = false;
                    // Dormant pinned tab (restored at boot): re-mount its remote
                    // pane on first focus. Single-remote case (the common
                    // left=Local / right=Remote); a dual-remote tab re-mounts
                    // only its right pane here.
                    let reopen = self.sftp_tabs[idx].pending_reopen.take();
                    if let Some(crate::state::PinnedTabSpec::Sftp { left, right, .. }) = reopen {
                        use crate::state::{SftpPaneSide, SftpPaneSpec};
                        self.refresh_sftp_local(SftpPaneSide::Left);
                        // Re-mount every remote pane the tab had (both, for a
                        // server-to-server tab). Each side is dispatched
                        // separately so the mount pipeline targets it correctly.
                        let mut tasks = Vec::new();
                        for (side, spec) in
                            [(SftpPaneSide::Right, &right), (SftpPaneSide::Left, &left)]
                        {
                            if let SftpPaneSpec::Remote(id) = spec
                                && let Some(ci) = self.connections.iter().position(|c| c.id == *id)
                            {
                                tasks.push(Task::done(Message::SftpRemountPane(side, ci)));
                            }
                        }
                        if !tasks.is_empty() {
                            return Ok(Task::batch(tasks));
                        }
                        return Ok(Task::none());
                    }
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Right);
                }
            }
            Message::CloseSftpTab(idx) => {
                self.overlay = None;
                // Guard: an in-flight transfer or unsaved edit-session opens a
                // confirmation modal instead of closing outright.
                if self.sftp_tab_has_unsaved(idx) {
                    self.pending_sftp_close = Some(crate::state::PendingSftpClose::One(idx));
                } else {
                    return Ok(self.close_sftp_tab(idx));
                }
            }
            Message::ConfirmCloseSftpTab => {
                match self.pending_sftp_close.take() {
                    Some(crate::state::PendingSftpClose::One(idx)) => {
                        return Ok(self.close_sftp_tab(idx));
                    }
                    Some(crate::state::PendingSftpClose::Others(idx)) => {
                        return Ok(self.close_other_sftp_tabs(idx));
                    }
                    None => {}
                }
            }
            Message::CancelCloseSftpTab => {
                self.pending_sftp_close = None;
            }
            Message::ToggleSftpTabPin(idx) => {
                if let Some(t) = self.sftp_tabs.get_mut(idx) {
                    t.pinned = !t.pinned;
                }
                self.overlay = None;
                // Persist so the pin (and its arranged order) survives a relaunch.
                self.persist_pinned_tabs();
            }
            Message::CloseOtherSftpTabs(idx) => {
                self.overlay = None;
                if idx >= self.sftp_tabs.len() {
                    return Ok(Task::none());
                }
                // Guard: if any tab we'd drop has an in-flight transfer or an
                // unsaved edit-session, confirm first (mirrors CloseSftpTab)
                // instead of silently discarding it.
                if self.other_sftp_tabs_have_unsaved(idx) {
                    self.pending_sftp_close = Some(crate::state::PendingSftpClose::Others(idx));
                } else {
                    return Ok(self.close_other_sftp_tabs(idx));
                }
            }
            Message::ShowSftpTabMenu(idx) => {
                self.overlay = Some(crate::state::OverlayState {
                    content: crate::state::OverlayContent::SftpTabActions(idx),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::SftpTabHovered(idx) => {
                self.hovered_sftp_tab = Some(idx);
                // Terminal / SFTP hover are mutually exclusive (one cursor).
                self.hovered_tab = None;
                // Live-slide: while a drag is active, entering this SFTP tab
                // slides the dragged tab (terminal or SFTP) into its slot in
                // the unified `tab_order`.
                if let Some(drag) = self.tab_drag.filter(|d| d.active)
                    && let Some(target) = self.sftp_tabs.get(idx).map(|t| t.id)
                    && drag.from_id != target
                {
                    self.slide_tab_in_order(drag.from_id, target);
                }
            }
            Message::SftpTabUnhovered => {
                self.hovered_sftp_tab = None;
            }
            Message::NewSftpTab => {
                self.overlay = None;
                // Dismiss the new-tab picker too: SFTP is selectable from it.
                self.show_new_tab_picker = false;
                // ...and the burger menu, the other entry point (its own
                // flag, so clearing `overlay` above isn't enough); without
                // this it lingers over the freshly-opened SFTP tab and the
                // host picker until an extra click.
                self.show_burger_menu = false;
                self.open_new_sftp_tab();
                // Empty tab: open the host picker for the remote pane.
                self.sftp.picker_open = true;
                self.sftp.picker_target = crate::state::SftpPaneSide::Right;
            }
            Message::SftpPickerSearch(s) => {
                self.sftp.picker_search = s;
            }
            Message::SftpToggleHidden(side) => {
                self.sftp.close_menus();
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
                self.sftp.left.filter_open = false;
                self.sftp.right.filter_open = false;
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
                self.sftp.close_menus();
            }
            Message::SftpToggleColumn(side, col) => {
                // Per-pane toggle; the actions menu stays open so the user can
                // flip several columns in one pass. The edited pane becomes the
                // new persisted template seed.
                self.sftp.pane_mut(side).columns.toggle(col);
                self.sftp_columns_template = self.sftp.pane(side).columns.clone();
                self.persist_sftp_columns();
            }
            Message::SftpColResizeStart(side, col) => {
                let start_w = self.sftp.pane(side).columns.width.get(col);
                self.sftp_col_resize = Some((side, col, self.mouse_position.x, start_w));
                self.sftp.close_menus();
            }
            Message::SftpColAutoFit(side, col) => {
                self.autofit_sftp_column(side, col);
            }
            Message::SftpColDragStart(side, col) => {
                self.sftp_col_drag = Some(crate::state::SftpColDrag {
                    side,
                    col,
                    press_x: self.mouse_position.x,
                    active: false,
                });
            }
            Message::SftpColHovered(side, col) => {
                self.sftp_hovered_col = Some((side, col));
            }
            Message::SftpColUnhovered => {
                self.sftp_hovered_col = None;
            }
            Message::SftpToggleFilterSearch(side) => {
                let now = !self.sftp.pane(side).filter_open;
                self.sftp.close_menus();
                self.sftp.pane_mut(side).filter_open = now;
                if now {
                    // Focus the popover input so the user can type immediately.
                    let id = match side {
                        SftpPaneSide::Left => "sftp-filter-pop-left",
                        SftpPaneSide::Right => "sftp-filter-pop-right",
                    };
                    return Ok(iced::widget::operation::focus(iced::widget::Id::new(id)));
                }
            }
            Message::SftpToggleLog => {
                self.sftp.log_open = !self.sftp.log_open;
            }
            Message::SftpSplitResizeStart => {
                // Capture the cursor x and current ratio; the MouseMoved
                // handler computes the delta against these.
                self.sftp_split_drag = Some((self.mouse_position.x, self.sftp_split_ratio));
            }
            Message::SftpLogResizeStart => {
                // Capture the cursor y and current log height; the MouseMoved
                // handler computes the delta against these.
                self.sftp_log_drag = Some((self.mouse_position.y, self.sftp.log_height));
            }
            Message::OpenSftpForConnection(idx) => {
                // Dismiss the host-card context menu this was launched from so
                // it doesn't linger over the SFTP surface (mirrors ConnectSsh).
                self.card_context_menu = None;
                self.overlay = None;
                if self.connections.get(idx).is_none() {
                    return Ok(Task::none());
                }
                // Fresh SFTP tab, then mount the host into its remote (right)
                // pane via the shared mount pipeline (reuse-or-connect).
                self.open_new_sftp_tab();
                return self.handle_sftp(Message::SftpRemountPane(SftpPaneSide::Right, idx));
            }
            Message::OpenSftpForTab(tab_idx) => {
                self.overlay = None;
                let Some(tab) = self.tabs.get(tab_idx) else {
                    return Ok(Task::none());
                };
                let base = tab.label.trim_end_matches(" (disconnected)").to_string();
                // Prefer a saved connection by label so the SFTP tab can
                // reconnect on its own.
                if let Some(conn_idx) = self.connections.iter().position(|c| c.label == base) {
                    return self.handle_sftp(Message::OpenSftpForConnection(conn_idx));
                }
                // No saved host (ad-hoc / cloud tab): mount the tab's live
                // SSH session directly. Nothing to do if it has no session.
                let Some(session) = tab.active().ssh_session.clone() else {
                    return Ok(Task::none());
                };
                let label = base;
                self.open_new_sftp_tab();
                {
                    let pane = self.sftp.pane_mut(SftpPaneSide::Right);
                    pane.is_remote = true;
                    pane.host_label = Some(label.clone());
                    pane.remote_loading = true;
                    pane.error = None;
                    pane.remote_entries.clear();
                }
                let target = SftpPaneSide::Right;
                let session_for_task = session.clone();
                return Ok(Task::perform(
                    async move {
                        let client =
                            session_for_task.open_sftp().await.map_err(|e| e.to_string())?;
                        let initial = client
                            .canonicalize(".")
                            .await
                            .unwrap_or_else(|_| "/".to_string());
                        let entries =
                            client.list_dir(&initial).await.map_err(|e| e.to_string())?;
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
                    is_background: false,
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::SftpBackgroundRightClick(side) => {
                // Empty-area right-click: `path` carries the pane's current
                // directory so the directory-level actions act on it.
                let pane = self.sftp.pane(side);
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
                // Drop the keyboard straight into the inline input so the user
                // can type the new name without an extra click.
                return Ok(iced::widget::operation::focus(iced::widget::Id::new(
                    crate::views::sftp::RENAME_INPUT_ID,
                )));
            }
            Message::SftpRenameInput(s) => {
                if let Some(ref mut r) = self.sftp.rename {
                    r.input = s;
                }
            }
            Message::SftpRenameCommit => {
                // The Enter that submits this rename also reaches the global
                // keyboard subscription; swallow the row-activation it would
                // otherwise trigger (which re-opens the just-renamed file).
                // Not set on the click-to-commit path (no trailing Enter there).
                self.sftp.swallow_next_activate = true;
                return Ok(self.commit_rename());
            }
            Message::SftpRenamed(side, reload_path, new_name) => {
                self.push_sftp_log(
                    crate::state::SftpLogLevel::Ok,
                    format!("{} {}", crate::i18n::t("sftp_log_renamed"), new_name),
                );
                return Ok(Task::done(Message::SftpNavigateRemote(side, reload_path)));
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
                let mut local_deleted = 0usize;
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
                        match result {
                            Ok(()) => local_deleted += 1,
                            Err(e) => self.sftp.pane_mut(t.side).error = Some(e.to_string()),
                        }
                        if !local_sides.contains(&t.side) {
                            local_sides.push(t.side);
                        }
                    }
                }
                if local_deleted > 0 {
                    self.push_sftp_log(
                        crate::state::SftpLogLevel::Ok,
                        format!(
                            "{} {} {}",
                            crate::i18n::t("sftp_log_deleted"),
                            local_deleted,
                            crate::i18n::t("sftp_log_items"),
                        ),
                    );
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
                if !paths.is_empty() {
                    self.push_sftp_log(
                        crate::state::SftpLogLevel::Ok,
                        format!(
                            "{} {} {}",
                            crate::i18n::t("sftp_log_deleted"),
                            paths.len(),
                            crate::i18n::t("sftp_log_items"),
                        ),
                    );
                }
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
                self.sftp.close_menus();
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
                // See SftpRenameCommit: swallow the trailing Enter's activation.
                self.sftp.swallow_next_activate = true;
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
                // row in the *other* pane. This is a secondary trigger: the
                // primary one is the cursor-geometry crossing in the
                // MouseMoved handler (reliable during a button-hold, same as
                // the divider drags). Row-hover can be disrupted by tooltips
                // / row gaps, so it can't be the sole signal. Activating
                // lights up the destination pane outline as drag feedback.
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
                // Any physical click leaves dashboard card-selection mode.
                self.selected_nav = None;
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
                } else if let Some(idx) = self.hovered_sftp_tab
                    && let Some(tab) = self.sftp_tabs.get(idx)
                {
                    // SFTP tabs arm the same unified reorder drag.
                    self.tab_drag = Some(crate::state::TabDrag {
                        from_id: tab.id,
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
                // A press outside the inline-rename input commits the rename
                // (click any other row, empty area, or the other pane). A
                // press *inside* the input doesn't fire SftpSelectRow and
                // keeps `hovered_row` on the rename's own row, so we leave it
                // be and let the user keep editing.
                if let Some(rn) = self.sftp.rename.as_ref() {
                    let on_rename_row = self.sftp.hovered_row.as_ref().is_some_and(
                        |(s, p, _)| *s == rn.side && *p == rn.original_path,
                    );
                    if !on_rename_row {
                        return Ok(self.commit_rename());
                    }
                }
                if let Some((side, path, is_dir)) = self.sftp.hovered_row.clone() {
                    self.arm_sftp_row_drag(side, path, is_dir);
                }
            }
            Message::SftpSelectRow(side, path, is_dir) => {
                // Arm a potential drag from the button's own press, before the
                // selection below collapses, using the exact pressed row. A
                // second arm path alongside the global left-press; no-op if
                // that already armed it. (Cross-pane activation itself happens
                // later via cursor geometry in MouseMoved.)
                self.arm_sftp_row_drag(side, path.clone(), is_dir);
                // Keyboard focus follows the mouse: a clicked row's pane
                // becomes the focused pane and the cursor leaves the ".." row.
                self.sftp.focused_side = side;
                self.sftp.parent_cursor = false;
                let target = (side, path.clone());
                let ctrl = self.modifiers.control() || self.modifiers.command();
                let shift = self.modifiers.shift();
                // Slow-click-to-rename: a second plain click on the row that is
                // already the sole selection, slower than a double-click but
                // within a deliberate window, arms an inline rename. It's only
                // committed on release (see the Left-up handler) so dragging an
                // already-selected row still works.
                let now = std::time::Instant::now();
                let already_sole = self.sftp.selected_rows.as_slice() == [target.clone()];
                let slow_second = !ctrl
                    && !shift
                    && already_sole
                    && self.sftp.last_click.as_ref().is_some_and(|(s, p, t)| {
                        *s == side
                            && p == &path
                            && now.duration_since(*t) >= DOUBLE_CLICK_WINDOW
                            && now.duration_since(*t) < SLOW_RENAME_WINDOW
                    });
                self.sftp.pending_rename =
                    slow_second.then(|| (side, path.clone()));
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
                            && now.duration_since(*t) < DOUBLE_CLICK_WINDOW
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
                    self.push_sftp_log(
                        crate::state::SftpLogLevel::Error,
                        format!("{} {}", crate::i18n::t("sftp_log_error"), msg),
                    );
                    self.sftp.pane_mut(side).error = Some(msg);
                } else {
                    self.push_sftp_log(crate::state::SftpLogLevel::Ok, msg.clone());
                    tracing::info!("sftp op: {}", msg);
                }
            }
            Message::SftpListScrolled(side, offset_y, viewport_h) => {
                let pane = self.sftp.pane_mut(side);
                pane.list_scroll_y = offset_y;
                pane.list_viewport_h = viewport_h;
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
                // Consume the activation-swallow flag on the first keyboard
                // event after an inline-input commit: the trailing Enter from
                // that submit must not activate the still-selected row.
                let swallow = std::mem::take(&mut self.sftp.swallow_next_activate);
                if let iced::keyboard::Event::KeyPressed { key, .. } = &ke {
                    // Escape cancels an in-progress inline rename / new-entry
                    // instead of falling through to the terminal handler.
                    if matches!(key, iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)) {
                        if self.sftp.rename.take().is_some() {
                            return Ok(Task::none());
                        }
                        if self.sftp.new_entry.take().is_some() {
                            return Ok(Task::none());
                        }
                    }
                    if swallow
                        && matches!(key, iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter))
                    {
                        return Ok(Task::none());
                    }
                }
                let editing = self.sftp.rename.is_some()
                    || self.sftp.new_entry.is_some()
                    || self.sftp.overwrite_prompt.is_some()
                    || !self.sftp.delete_confirm.is_empty()
                    || self.sftp.properties.is_some()
                    || self.sftp.picker_open
                    || self.sftp.left.path_editing.is_some()
                    || self.sftp.right.path_editing.is_some();
                // Arrow / Enter navigation: move the focused row or open
                // it (folder -> navigate, file -> open). These are Named
                // keys, so they never reach the type-ahead char extraction
                // below; handle them here before that returns `Err` and
                // forwards the keypress to the terminal/PTY. Suppressed
                // while a modal/input owns the keyboard, and when a
                // modifier is held (those belong to hotkeys / the PTY).
                if !editing
                    && let iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(named),
                        modifiers,
                        ..
                    } = &ke
                    && !modifiers.control()
                    && !modifiers.command()
                    && !modifiers.alt()
                {
                    use iced::keyboard::key::Named;
                    // Any of these takes the keyboard cursor over, so mute the
                    // mouse-hover highlight until the mouse moves again.
                    if matches!(
                        named,
                        Named::ArrowDown
                            | Named::ArrowUp
                            | Named::ArrowLeft
                            | Named::ArrowRight
                            | Named::Enter
                            | Named::Tab
                    ) {
                        self.sftp.suppress_hover = true;
                    }
                    match named {
                        Named::ArrowDown => return Ok(self.sftp_move_focus(true)),
                        Named::ArrowUp => return Ok(self.sftp_move_focus(false)),
                        // Right descends into a folder (or up via ".."); on a
                        // file it does nothing. Enter additionally opens files.
                        Named::ArrowRight => return Ok(self.sftp_activate_focused(false)),
                        Named::Enter => return Ok(self.sftp_activate_focused(true)),
                        // Left goes to the parent directory.
                        Named::ArrowLeft => return Ok(self.sftp_focus_parent()),
                        // Tab switches the focused pane.
                        Named::Tab => return Ok(self.sftp_toggle_pane_focus()),
                        _ => {}
                    }
                }
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
                // Type-ahead works from any keyboard cursor: a selected row
                // (the selection's pane is the focus) or the ".." parent row
                // (which clears selected_rows but sets parent_cursor on the
                // focused pane).
                if self.sftp.selected_rows.last().is_none() && !self.sftp.parent_cursor {
                    return Ok(Task::none());
                }
                let now = std::time::Instant::now();
                let elapsed = self
                    .sftp
                    .type_ahead_at
                    .map(|t| now.duration_since(t) > TYPE_AHEAD_RESET)
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
                // Type-ahead moves the keyboard cursor too; mute mouse hover.
                self.sftp.suppress_hover = true;
                // Debounce: bump the generation and search only after a short
                // pause, so fast typing ("cla") resolves once with the full
                // buffer instead of jumping on every key (c -> cl -> cla).
                self.sftp.type_ahead_gen = self.sftp.type_ahead_gen.wrapping_add(1);
                let generation = self.sftp.type_ahead_gen;
                return Ok(Task::perform(
                    async move {
                        tokio::time::sleep(TYPE_AHEAD_DEBOUNCE).await;
                    },
                    move |_| Message::SftpTypeAheadFire(generation),
                ));
            }
            Message::SftpTypeAheadFire(generation) => {
                // A newer keystroke superseded this fire: skip it.
                if generation != self.sftp.type_ahead_gen {
                    return Ok(Task::none());
                }
                // On the ".." row there's no selected row, so fall back to
                // the focused pane (type-ahead works from the parent cursor).
                let side = self
                    .sftp
                    .selected_rows
                    .last()
                    .map(|(s, _)| *s)
                    .unwrap_or(self.sftp.focused_side);
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
                self.sftp.focused_side = side;
                self.sftp.parent_cursor = false;
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
