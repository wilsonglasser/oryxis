//! Mount / connect arms split out of `dispatch_sftp`: host picking
//! with the terminal session-reuse scan, the fresh SFTP dial (host-key
//! verification + TOTP wiring), remount / retry / cancel, remote-error
//! surfacing and the host picker. Called from `handle_sftp`.

#![allow(clippy::result_large_err)]

use iced::futures::SinkExt;
use iced::Task;

use std::sync::Arc;
use std::time::Duration;

use oryxis_ssh::SshEngine;

use super::initial_remote_listing;
use crate::app::{SshMessage, Message, Oryxis, SftpMessage};
use crate::sftp_helpers::sort_remote_entries;
use crate::state::SftpPaneSide;

/// How long a transient error toast stays on screen before auto-clearing.
const TOAST_DURATION: Duration = Duration::from_millis(2600);

/// Stream events from a fresh SFTP connect. `HostKey` surfaces an
/// unknown/changed server key to the shared verification modal mid-connect
/// (the connect blocks until the user answers); `Done` carries the final
/// mounted session or the error.
enum SftpConnectMsg {
    HostKey(oryxis_ssh::HostKeyQuery),
    ProxyCommand(oryxis_ssh::ProxyCommandQuery),
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
    /// Write (or clear) the SFTP landing folder of the host mounted in
    /// `side`, both in memory and in the vault, and confirm with a toast.
    /// The pane is matched by its host label, the same key the remount path
    /// uses; a pane showing Local (or a host that isn't a saved connection,
    /// e.g. a cloud-discovered one that was never imported) simply has
    /// nothing to store, and says so.
    fn store_sftp_initial_path(
        &mut self,
        side: SftpPaneSide,
        path: Option<String>,
    ) -> Task<Message> {
        let Some(label) = self
            .sftp
            .pane(side)
            .host_label
            .clone()
            .filter(|_| self.sftp.pane(side).is_remote)
        else {
            return Task::none();
        };
        let value = path
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let Some(conn) = self.connections.iter_mut().find(|c| c.label == label) else {
            return self.show_toast_secs(
                crate::i18n::t("sftp_initial_path_no_host").to_string(),
                4,
            );
        };
        conn.sftp_initial_path = value.clone();
        conn.updated_at = chrono::Utc::now();
        // Full save (like the icon picker's): the row must persist without
        // clobbering the host's other columns.
        if let Some(vault) = &self.vault
            && let Err(e) = vault.save_connection(conn, None)
        {
            return self.show_toast_secs(format!("{e}"), 5);
        }
        match value {
            Some(p) => self.show_toast_secs(
                crate::i18n::t("sftp_initial_path_saved").replacen("{path}", &p, 1),
                3,
            ),
            None => self.show_toast_secs(
                crate::i18n::t("sftp_initial_path_cleared").to_string(),
                3,
            ),
        }
    }

    pub(super) fn handle_sftp_hosts(
        &mut self,
        message: SftpMessage,
    ) -> Result<Task<Message>, SftpMessage> {
        match message {
            SftpMessage::SftpPickHost(idx) => {
                let mut conn = match self.connections.get(idx).cloned() {
                    Some(c) => c,
                    None => {
                        // Bail-out must drop any one-shot initial-path hint or
                        // it would leak into the next unrelated mount.
                        self.sftp_open_at_path = None;
                        return Ok(Task::none());
                    }
                };
                // Same working copy the terminal connect dials: group
                // inheritance (D4) and the effective proxy both land on
                // the clone, so an SFTP-only mount authenticates exactly
                // like a tab to the same host would.
                self.apply_group_inheritance(&mut conn);
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

                // Reuse an existing SSH session whenever a terminal pane is
                // already pointed at this host, saves a TCP round-trip
                // and a second auth dance. Scans every pane (not just each
                // tab's focused one): a split tab hosts two servers under
                // one tab label, and the match is by the pane's own label.
                let existing = self.tabs.iter().find_map(|t| {
                    t.pane_grid.panes.values().find_map(|p| {
                        if p.label.trim_end_matches(" (disconnected)") == conn.label {
                            // SFTP multiplexes on the SSH handle; a Telnet
                            // pane to the same label can't be reused. Nor
                            // can a session that died without its pane
                            // noticing yet: mounting on it would fail
                            // with "session closed", dial fresh instead.
                            p.session
                                .as_ref()
                                .and_then(|s| s.ssh())
                                .filter(|s| s.is_alive())
                                .cloned()
                        } else {
                            None
                        }
                    })
                });
                let label = conn.label.clone();
                // Preferred directory for this mount, in precedence order:
                // the one-shot hint (a sidebar Files promotion, a remount
                // landing back where the user was) first, then the host's
                // own saved SFTP folder. Both fall back to the login
                // directory in `initial_remote_listing` when they don't
                // resolve, so a stale value can never break the mount.
                let initial_hint = self
                    .sftp_open_at_path
                    .take()
                    .or_else(|| conn.sftp_initial_path.clone())
                    .filter(|p| !p.trim().is_empty());
                // Owner of the live buffer at kickoff time (standalone SFTP
                // tab or hybrid terminal tab). The mount completion is
                // stamped with it so a park/hoist swap while the mount is in
                // flight still delivers the result to the originating
                // state, or drops it if that owner closed meanwhile.
                let owner = self.current_sftp_owner();
                if let Some(session) = existing {
                    let session_for_task = session.clone();
                    return Ok(Task::perform(
                        async move {
                            let client = session_for_task
                                .open_sftp()
                                .await
                                .map_err(|e| e.to_string())?;
                            let (initial, entries) =
                                initial_remote_listing(&client, initial_hint).await?;
                            Ok::<_, String>((client, initial, entries))
                        },
                        move |result| match result {
                            Ok((client, path, entries)) => Message::sftp_owned(
                                owner,
                                SftpMessage::HostMounted(
                                    target,
                                    label.clone(),
                                    session.clone(),
                                    client,
                                    path,
                                    entries,
                                ),
                            ),
                            Err(e) => Message::sftp_owned(
                                owner,
                                SftpMessage::RemoteError(target, e),
                            ),
                        },
                    ));
                }

                // No existing tab, open a brand-new SSH session, just
                // for SFTP. Same credential pipeline as |v| Message::Ssh(SshMessage::ConnectSsh(v)),
                // but without spawning a terminal tab.
                let (password, private_key, certificate) = self.resolve_credentials(&conn);
                // Agent-auth pin (B3), same rule as the tab connect.
                let pinned_agent = self.pinned_agent_public(&conn);
                let resolver = self.make_jump_resolver(&mut conn);
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

                // Command-proxy approval, same bridge: the user asked
                // for this mount, so an unapproved line may prompt.
                let (pc_ask_tx, mut pc_ask_rx) = tokio::sync::mpsc::channel::<(
                    oryxis_ssh::ProxyCommandQuery,
                    tokio::sync::oneshot::Sender<bool>,
                )>(1);
                let (pc_resp_tx, mut pc_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
                self.proxy_command_response_tx = Some(pc_resp_tx);

                // TOTP autofill for keyboard-interactive 2FA, same as the
                // terminal path (headless here: no modal, so the autofill is
                // the only way an OTP-gated host can mount at all).
                let totp_secret = self
                    .vault
                    .as_ref()
                    .and_then(|v| v.get_connection_totp_secret(&conn.id).ok().flatten());

                // Captured for the map closure (conn is moved into the
                // producer). The retry re-runs this same SFTP mount.
                let sftp_conn_id = conn.id;
                let stream = iced::stream::channel::<SftpConnectMsg>(
                    8,
                    move |mut sender: iced::futures::channel::mpsc::Sender<SftpConnectMsg>| async move {
                        let engine = SshEngine::new()
                            .with_host_key_check(host_key_check)
                            .with_host_key_ask(hk_ask_tx)
                            .with_proxy_command_ask(pc_ask_tx)
                            .with_totp_secret(totp_secret.as_deref())
                            .with_keepalive(keepalive)
                            .with_address_family(conn.address_family)
                            .with_rekey_limit_mb(conn.rekey_limit_mb)
                            .with_pinned_agent_key(pinned_agent.as_deref())
                            .with_algorithm_overrides(
                                conn.ciphers.clone(),
                                conn.kex.clone(),
                                conn.macs.clone(),
                                conn.host_key_algorithms.clone(),
                            )
                            .with_connect_timeout(connect_to)
                            .with_auth_timeout(auth_to)
                            .with_session_timeout(session_to);

                        let mut pc_sender = sender.clone();
                        let _pc_bridge = tokio::spawn(async move {
                            while let Some((query, resp_tx)) = pc_ask_rx.recv().await {
                                let _ = pc_sender.send(SftpConnectMsg::ProxyCommand(query)).await;
                                let approved = pc_resp_rx.recv().await.unwrap_or(false);
                                let _ = resp_tx.send(approved);
                            }
                        });

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
                                private_key
                                    .as_deref()
                                    .map(|pem| oryxis_ssh::KeyMaterial::new(pem, certificate.as_deref())),
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
                            let (initial, entries) =
                                initial_remote_listing(&client, initial_hint).await?;
                            Ok::<_, String>((session, client, initial, entries))
                        }
                        .await;
                        let _ = sender.send(SftpConnectMsg::Done(result)).await;
                    },
                );
                return Ok(Task::stream(stream).map(move |m| match m {
                    SftpConnectMsg::HostKey(q) => Message::Ssh(SshMessage::SshHostKeyVerify(q)),
                    SftpConnectMsg::ProxyCommand(q) => Message::Ssh(
                        SshMessage::SshProxyCommandVerify(
                            Box::new(q),
                            crate::state::ProxyConsentMode::Ask,
                        ),
                    ),
                    SftpConnectMsg::Done(Ok((session, client, path, entries))) => {
                        Message::sftp_owned(
                            owner,
                            SftpMessage::HostMounted(
                                target,
                                label.clone(),
                                session,
                                client,
                                path,
                                entries,
                            ),
                        )
                    }
                    SftpConnectMsg::Done(Err(e)) => {
                        Message::sftp_owned(owner, SftpMessage::RemoteError(target, e))
                    }
                    SftpConnectMsg::NoCommonAlgo { category, server_offers } => {
                        Message::Ssh(SshMessage::SshNoCommonAlgo {
                            conn_id: sftp_conn_id,
                            category,
                            server_offers,
                            retry: Box::new(Message::Sftp(SftpMessage::SftpPickHost(idx))),
                        })
                    }
                }));
            }
            SftpMessage::SftpRemountPane(side, idx) => {
                // Point the picker at this side, then reuse the full mount
                // pipeline. Dispatched once per side, so each runs in its own
                // update cycle with the correct target (no shared-field race).
                self.sftp.picker_target = side;
                return self.handle_sftp_hosts(SftpMessage::SftpPickHost(idx));
            }
            SftpMessage::SftpPickLocal => {
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
                    // The remote history belonged to the unmounted host.
                    pane.path_history.clear();
                    pane.path_history_open = false;
                    if pane.local_path.as_os_str().is_empty() {
                        pane.local_path = std::env::var_os("HOME")
                            .or_else(|| std::env::var_os("USERPROFILE"))
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| std::path::PathBuf::from("/"));
                    }
                }
                self.refresh_sftp_local(target);
            }
            SftpMessage::HostMounted(side, label, session, client, path, entries) => {
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
                // Fresh mount: any zip browse belonged to the previous
                // session, and the new host gets its own tool probe. The
                // path history is host-scoped too (issue #85): another
                // host's tree must never show up in this pane's dropdown.
                pane.path_history.clear();
                pane.path_history_open = false;
                pane.zip = None;
                pane.archive_tools = None;
                pane.archive_busy = None;
                // Inherit the mounted host's name as the tab label (last mount
                // wins when both panes are remote). Mid-route the stamped
                // owner names the tab, NOT `active_sftp` (which points at
                // whichever standalone tab holds the buffer right now); a
                // hybrid owner resolves to no position, its terminal tab
                // keeps its own label.
                let owner_idx = match self.routing_sftp {
                    Some(id) => self.sftp_tabs.iter().position(|t| t.id == id),
                    None => self.active_sftp,
                };
                if let Some(t) = owner_idx.and_then(|i| self.sftp_tabs.get_mut(i)) {
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
                // Discover which archive tools the host has (enables
                // the remote Extract / Compress menu items), one exec
                // round per mount.
                return Ok(self.spawn_archive_probe(side));
            }
            SftpMessage::RemoteError(side, msg) => {
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
                    self.set_toast(msg);
                    return Ok(Task::perform(
                        async {
                            tokio::time::sleep(TOAST_DURATION).await
                        },
                        |_| Message::ToastClear,
                    ));
                }
            }
            SftpMessage::SftpCancelRemoteLoad(side) => {
                // Drop the loading visual. The underlying Task::perform
                // can't be aborted (russh-sftp has no cancel token), so
                // a late success will still flow through SftpMessage::HostMounted
                // / SftpRemoteLoaded, but at least the user gets the
                // UI back and can retry/pick another host.
                let pane = self.sftp.pane_mut(side);
                pane.remote_loading = false;
                pane.error = Some("Cancelled by user".into());
            }
            SftpMessage::SftpRetryRemote(side) => {
                // Three cases the retry button has to cover:
                // 1. Session is mounted (client is Some) AND still
                //    alive, just re-list the current path. Network
                //    blip / op-timeout case.
                // 2. Session lost (client is None, or the SSH session
                //    under it died, issue #63: re-listing a channel on
                //    a closed session fails with "session closed"
                //    forever) but the host label is still around,
                //    re-run the full pick flow for that host, which
                //    reuses a live terminal session to the host when
                //    one exists (post-reconnect case) or dials fresh.
                // 3. No host label, fall back to the picker.
                let session_alive = self
                    .sftp
                    .pane(side)
                    .session
                    .as_ref()
                    .is_some_and(|s| s.is_alive());
                if self.sftp.pane(side).client.is_some() && session_alive {
                    return Ok(Task::done(Message::Sftp(SftpMessage::SftpNavigateRemote(
                        side,
                        self.sftp.pane(side).remote_path.clone(),
                    ))));
                }
                if !session_alive {
                    // Drop the dead channel so the remount below (or a
                    // later one) starts clean instead of ever touching
                    // the closed session again.
                    let pane = self.sftp.pane_mut(side);
                    pane.client = None;
                    pane.session = None;
                }
                if let Some(label) = self.sftp.pane(side).host_label.clone()
                    && let Some(idx) = self
                        .connections
                        .iter()
                        .position(|c| c.label == label)
                {
                    // Land the remount where the user was, not at the
                    // home directory (the pick pipeline's default); the
                    // one-shot hint falls back to home when the path is
                    // gone. An explicit pending hint keeps priority.
                    if self.sftp_open_at_path.is_none() {
                        self.sftp_open_at_path =
                            Some(self.sftp.pane(side).remote_path.clone())
                                .filter(|p| !p.is_empty());
                    }
                    self.sftp.picker_target = side;
                    return Ok(Task::done(Message::Sftp(SftpMessage::SftpPickHost(idx))));
                }
                self.sftp.picker_target = side;
                self.sftp.picker_open = true;
            }
            SftpMessage::SftpSetInitialPath(side, path) => {
                self.sftp.close_menus();
                return Ok(self.store_sftp_initial_path(side, Some(path)));
            }
            SftpMessage::SftpClearInitialPath(side) => {
                self.sftp.close_menus();
                return Ok(self.store_sftp_initial_path(side, None));
            }
            SftpMessage::SftpOpenPicker(side) => {
                self.sftp.picker_target = side;
                self.sftp.picker_open = true;
                self.sftp.picker_search.clear();
            }
            SftpMessage::SftpClosePicker => {
                self.sftp.picker_open = false;
            }
            SftpMessage::SftpPickerSearch(s) => {
                self.sftp.picker_search = s;
            }
            SftpMessage::OpenSftpForConnection(idx) => {
                // Dismiss the host-card context menu this was launched from so
                // it doesn't linger over the SFTP surface (mirrors ConnectSsh).
                self.card_context_menu = None;
                self.overlay = None;
                if self.connections.get(idx).is_none() {
                    // Bail-out must drop any one-shot initial-path hint or
                    // it would leak into the next unrelated mount.
                    self.sftp_open_at_path = None;
                    return Ok(Task::none());
                }
                // Fresh SFTP tab, then mount the host into its remote (right)
                // pane via the shared mount pipeline (reuse-or-connect).
                self.open_new_sftp_tab();
                return self.handle_sftp_hosts(SftpMessage::SftpRemountPane(SftpPaneSide::Right, idx));
            }
            SftpMessage::OpenSftpConsoleForHost(id) => {
                // Same dismissals as its browser twin above: the menu
                // this came from must not linger over the console.
                self.card_context_menu = None;
                self.overlay = None;
                let Some(conn) = self.connections.iter().find(|c| c.id == id).cloned() else {
                    return Ok(Task::none());
                };
                // From a card there is no shell whose directory to
                // inherit, so the console opens at the session's home.
                return Ok(self.open_sftp_console(conn, None));
            }
            SftpMessage::OpenSftpConsoleForTab(idx) => {
                self.overlay = None;
                let Some((conn, dir)) = self.tab_console_target(idx) else {
                    return Ok(Task::none());
                };
                // Asked for ON a session, so it lands in that session's
                // tab: as a pane beside the shell, or zoomed over it,
                // per the user's placement setting.
                return Ok(self.open_sftp_console_in_tab(idx, conn, dir));
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
