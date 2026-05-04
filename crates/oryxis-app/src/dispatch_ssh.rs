//! `Oryxis::handle_ssh` — match arms for SSH connection lifecycle
//! (connect, progress streaming, host-key prompts, disconnect, errors).
//! Pulled out of `dispatch.rs` so the master router stays small.

// Domain handlers return `Err(Message)` to pass an unclaimed message
// back up the chain. The Message enum is large (~200 bytes) but
// boxing it would force every handler-call site to allocate; the
// pattern is intentional, allow the lint.
#![allow(clippy::result_large_err)]

use iced::futures::SinkExt;
use iced::Task;

use std::sync::{Arc, Mutex};
use uuid::Uuid;

use oryxis_core::models::connection::AuthMethod;
use oryxis_ssh::{SshEngine, SshSession};
use oryxis_terminal::widget::TerminalState;

use crate::app::{Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
use crate::state::{
    ConnectionProgress, ConnectionStep, SshStreamMsg, TerminalTab, View,
};
use crate::util::open_in_browser;

impl Oryxis {
    pub(crate) fn handle_ssh(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- SSH connection --
            Message::ConnectSsh(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                // Close the new-tab picker if the connection was picked there.
                self.show_new_tab_picker = false;
                if let Some(mut conn) = self.connections.get(idx).cloned() {
                    // Resolve the effective proxy (saved identity OR inline)
                    // and hydrate its password from the encrypted vault column,
                    // then collapse onto `conn.proxy` — the engine only reads
                    // that field. A dangling `proxy_identity_id` resolves to
                    // None (warning logged inside `resolve_proxy`).
                    if let Some(vault) = self.vault.as_ref() {
                        conn.proxy = vault.resolve_proxy(&conn).ok().flatten();
                    }

                    // Resolve credentials: prefer identity if linked, otherwise inline
                    let (password, private_key) = if let Some(iid) = conn.identity_id {
                        let id_pw = self.vault.as_ref()
                            .and_then(|v| v.get_identity_password(&iid).ok().flatten());
                        let identity = self.identities.iter().find(|i| i.id == iid);
                        let id_key = identity.and_then(|i| i.key_id).and_then(|kid| {
                            self.vault.as_ref().and_then(|v| v.get_key_private(&kid).ok().flatten())
                        });
                        (id_pw, id_key)
                    } else {
                        let pw = self.vault.as_ref()
                            .and_then(|v| v.get_connection_password(&conn.id).ok().flatten());
                        let pk = if conn.auth_method == AuthMethod::Key || conn.auth_method == AuthMethod::Auto {
                            conn.key_id.and_then(|kid| {
                                self.vault.as_ref().and_then(|v| v.get_key_private(&kid).ok().flatten())
                            })
                        } else {
                            None
                        };
                        (pw, pk)
                    };

                    // Build resolver for jump hosts
                    let resolver = if !conn.jump_chain.is_empty() {
                        let mut passwords = std::collections::HashMap::new();
                        let mut keys = std::collections::HashMap::new();
                        let mut proxies = std::collections::HashMap::new();
                        for jid in &conn.jump_chain {
                            if let Some(vault) = &self.vault
                                && let Ok(Some(pw)) = vault.get_connection_password(jid) {
                                    passwords.insert(*jid, pw);
                                }
                            // Get jump host's key if it uses key auth
                            if let Some(jconn) = self.connections.iter().find(|c| c.id == *jid)
                                && let Some(kid) = jconn.key_id
                                    && let Some(vault) = &self.vault
                                        && let Ok(Some(pk)) = vault.get_key_private(&kid) {
                                            keys.insert(*jid, pk);
                                        }
                            // Resolve effective proxy (inline or identity-based)
                            // for this jump host. Only the first jump's entry
                            // matters at connect-time, but we hydrate all of
                            // them so the resolver is self-contained.
                            if let Some(jconn) = self.connections.iter().find(|c| c.id == *jid)
                                && let Some(vault) = &self.vault
                                && let Ok(Some(p)) = vault.resolve_proxy(jconn)
                            {
                                proxies.insert(*jid, p);
                            }
                        }
                        Some(oryxis_ssh::ConnectionResolver {
                            connections: self.connections.clone(),
                            passwords,
                            private_keys: keys,
                            proxies,
                        })
                    } else {
                        None
                    };

                    match TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16) {
                        Ok(mut state) => {
                            state.palette = self.terminal_theme.palette();
                            let label = conn.label.clone();
                            let hostname = format!("SSH {}:{}", conn.hostname, conn.port);
                            let terminal = Arc::new(Mutex::new(state));
                            let tab_idx = self.tabs.len();

                            // Create session log for terminal recording
                            let session_log_id = if let Some(vault) = &self.vault {
                                let log_id = Uuid::new_v4();
                                let _ = vault.create_session_log(&log_id, &conn.id, &conn.label);
                                Some(log_id)
                            } else {
                                None
                            };

                            let mut new_tab = TerminalTab::new_single(
                                label.clone(),
                                Arc::clone(&terminal),
                            );
                            new_tab.panes[0].session_log_id = session_log_id;
                            self.tabs.push(new_tab);

                            // Show progress view instead of terminal
                            self.connecting = Some(ConnectionProgress {
                                label: label.clone(),
                                hostname: hostname.clone(),
                                step: ConnectionStep::Connecting,
                                logs: vec![(ConnectionStep::Connecting, format!("Connecting to {}...", conn.hostname))],
                                failed: false,
                                connection_idx: idx,
                                tab_idx,
                            });
                            self.active_tab = Some(tab_idx);

                            // Host key verification: check callback + ask channel
                            let known_hosts_snapshot: Arc<Mutex<Vec<oryxis_core::models::known_host::KnownHost>>> =
                                Arc::new(Mutex::new(self.known_hosts.clone()));
                            let kh_ref = known_hosts_snapshot.clone();
                            let host_key_check: oryxis_ssh::HostKeyCheckCallback = Arc::new(move |host, port, _key_type, fingerprint| {
                                // Tolerate a poisoned mutex (some other lock-holder panicked)
                                // by recovering the inner data rather than panicking the SSH
                                // verification callback — better to fall back to "Unknown" and
                                // re-prompt the user than to crash mid-connect.
                                let hosts = match kh_ref.lock() {
                                    Ok(guard) => guard,
                                    Err(poison) => poison.into_inner(),
                                };
                                if let Some(existing) = hosts.iter().find(|h| h.hostname == host && h.port == port) {
                                    if existing.fingerprint != fingerprint {
                                        return oryxis_ssh::HostKeyStatus::Changed {
                                            old_fingerprint: existing.fingerprint.clone(),
                                        };
                                    }
                                    return oryxis_ssh::HostKeyStatus::Known;
                                }
                                oryxis_ssh::HostKeyStatus::Unknown
                            });

                            // Channel for the SSH engine to ask the UI about host keys
                            let (hk_ask_tx, mut hk_ask_rx) = tokio::sync::mpsc::channel::<(oryxis_ssh::HostKeyQuery, tokio::sync::oneshot::Sender<bool>)>(1);
                            // Channel for the UI to send responses back
                            let (hk_resp_tx, mut hk_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
                            self.host_key_response_tx = Some(hk_resp_tx);

                            let conn_host = conn.hostname.clone();
                            let conn_port = conn.port;
                            let username = conn.username.clone()
                                .or_else(|| {
                                    conn.identity_id.and_then(|iid| {
                                        self.identities.iter().find(|i| i.id == iid)
                                            .and_then(|i| i.username.clone())
                                    })
                                })
                                .unwrap_or_else(|| "root".into());
                            let auth_method_label = format!("{:?}", conn.auth_method);
                            let keepalive_secs: u64 = self
                                .setting_keepalive_interval
                                .parse()
                                .unwrap_or(0);
                            let keepalive = (keepalive_secs > 0)
                                .then(|| std::time::Duration::from_secs(keepalive_secs));
                            let agent_forwarding = conn.agent_forwarding;
                            let stream = iced::stream::channel::<SshStreamMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<SshStreamMsg>| {
                                async move {
                                    let engine = SshEngine::new()
                                        .with_host_key_check(host_key_check)
                                        .with_host_key_ask(hk_ask_tx)
                                        .with_keepalive(keepalive)
                                        .with_agent_forwarding(agent_forwarding);

                                    // Spawn a bridge task: receives host key queries from the SSH engine,
                                    // forwards to iced stream, and waits for UI response
                                    let mut sender_clone = sender.clone();
                                    let _hk_bridge = tokio::spawn(async move {
                                        while let Some((query, resp_tx)) = hk_ask_rx.recv().await {
                                            // Send query to iced UI
                                            let _ = sender_clone.send(SshStreamMsg::HostKeyVerify(query)).await;
                                            // Wait for UI response
                                            let accepted = hk_resp_rx.recv().await.unwrap_or(false);
                                            let _ = resp_tx.send(accepted);
                                        }
                                    });

                                    // Step 1: TCP connection + SSH handshake + host key verification
                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::Connecting,
                                        format!("Connecting to {}:{}...", conn_host, conn_port),
                                    )).await;

                                    let mut handle = match engine.establish_transport(&conn, resolver.as_ref()).await {
                                        Ok(h) => {
                                            let _ = sender.send(SshStreamMsg::Progress(
                                                ConnectionStep::Handshake,
                                                format!("Connected to {}:{} — handshake OK", conn_host, conn_port),
                                            )).await;
                                            h
                                        }
                                        Err(e) => {
                                            let _ = sender.send(SshStreamMsg::Error(
                                                format!("Connection to {}:{} failed: {}", conn_host, conn_port, e),
                                            )).await;
                                            return;
                                        }
                                    };

                                    // Step 2: Authentication
                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::Authenticating,
                                        format!("Authenticating as \"{}\" ({})...", username, auth_method_label),
                                    )).await;

                                    if let Err(e) = engine.do_authenticate(&mut handle, &conn, password.as_deref(), private_key.as_deref()).await {
                                        let _ = sender.send(SshStreamMsg::Error(
                                            format!("Authentication failed for \"{}\": {}", username, e),
                                        )).await;
                                        return;
                                    }

                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::Authenticating,
                                        format!("Authenticated as \"{}\"", username),
                                    )).await;

                                    // Step 3: Open PTY session (+ port forwards)
                                    if !conn.port_forwards.is_empty() {
                                        let fwd_summary: Vec<String> = conn.port_forwards.iter()
                                            .map(|pf| format!("{}:{}:{}", pf.local_port, pf.remote_host, pf.remote_port))
                                            .collect();
                                        let _ = sender.send(SshStreamMsg::Progress(
                                            ConnectionStep::Authenticating,
                                            format!("Port forwards: {}", fwd_summary.join(", ")),
                                        )).await;
                                    }
                                    match engine.open_session(handle, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS, &conn.port_forwards).await {
                                        Ok((session, mut rx)) => {
                                            let session = Arc::new(session);
                                            let _ = sender.send(SshStreamMsg::Connected(session.clone())).await;
                                            while let Some(data) = rx.recv().await {
                                                if sender.send(SshStreamMsg::Data(data)).await.is_err() {
                                                    break;
                                                }
                                            }
                                            let _ = sender.send(SshStreamMsg::Disconnected).await;
                                        }
                                        Err(e) => {
                                            let _ = sender.send(SshStreamMsg::Error(
                                                format!("Session setup failed: {}", e),
                                            )).await;
                                        }
                                    }
                                }
                            });

                            return Ok(Task::batch(vec![
                                self.tab_scroll_to_active(),
                                Task::stream(stream).map(move |msg| match msg {
                                    SshStreamMsg::Progress(step, log) => {
                                        Message::SshProgress(step, log)
                                    }
                                    SshStreamMsg::Connected(session) => {
                                        Message::SshConnected(tab_idx, session)
                                    }
                                    SshStreamMsg::NewKnownHosts(hosts) => {
                                        Message::SshNewKnownHosts(hosts)
                                    }
                                    SshStreamMsg::HostKeyVerify(query) => {
                                        Message::SshHostKeyVerify(query)
                                    }
                                    SshStreamMsg::Data(data) => {
                                        Message::PtyOutput(tab_idx, data)
                                    }
                                    SshStreamMsg::Error(err) => Message::SshError(err),
                                    SshStreamMsg::Disconnected => {
                                        Message::SshDisconnected(tab_idx)
                                    }
                                }),
                            ]));
                        }
                        Err(e) => {
                            tracing::error!("Failed to create terminal state: {}", e);
                        }
                    }
                }
            }
            Message::SshProgress(step, log) => {
                if let Some(ref mut progress) = self.connecting {
                    progress.step = step;
                    progress.logs.push((step, log));
                }
            }
            Message::SshNewKnownHosts(hosts) => {
                if let Some(vault) = &self.vault {
                    for kh in &hosts {
                        let _ = vault.save_known_host(kh);
                    }
                    self.known_hosts = vault.list_known_hosts().unwrap_or_default();
                }
            }
            Message::SshHostKeyVerify(query) => {
                self.pending_host_key = Some(query);
            }
            Message::SshHostKeyReject => {
                self.pending_host_key = None;
                if let Some(ref tx) = self.host_key_response_tx {
                    let _ = tx.try_send(false);
                }
            }
            Message::SshHostKeyContinue => {
                // Accept for this session but don't save to known hosts
                self.pending_host_key = None;
                if let Some(ref tx) = self.host_key_response_tx {
                    let _ = tx.try_send(true);
                }
            }
            Message::SshHostKeyAcceptAndSave => {
                // Accept and save to known hosts
                if let (Some(query), Some(vault)) = (&self.pending_host_key, &self.vault) {
                    let kh = oryxis_core::models::known_host::KnownHost::new(
                        &query.hostname, query.port, &query.key_type, &query.fingerprint,
                    );
                    let _ = vault.save_known_host(&kh);
                    self.known_hosts = vault.list_known_hosts().unwrap_or_default();
                }
                self.pending_host_key = None;
                if let Some(ref tx) = self.host_key_response_tx {
                    let _ = tx.try_send(true);
                }
            }
            Message::SshConnected(tab_idx, session) => {
                let mut detect_for: Option<(Uuid, Arc<SshSession>)> = None;
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    tab.active_mut().ssh_session = Some(session.clone());
                    // Forward future viewport resizes to the SSH server so
                    // remote `top`/`vim` re-layout instead of overflowing
                    // into our local grid.
                    if let Ok(mut state) = tab.active().terminal.lock() {
                        state.set_remote_resize_sender(session.resize_sender());
                        // Also kick off a resize for the current grid in
                        // case the canvas is already smaller than the
                        // initial PTY size negotiated at session open.
                        session.resize(state.cols(), state.rows());
                    }
                    let label = tab.label.clone();
                    tracing::info!("SSH connected: {}", label);
                    if let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Connected, "Session established",
                        );
                        let _ = vault.add_log(&entry);
                    }
                    // Reset the auto-reconnect counter for this connection.
                    if let Some(conn) = self.connections.iter().find(|c| c.label == label) {
                        self.reconnect_counters.remove(&conn.id);
                        // Queue silent OS detection only if:
                        //   - the feature is enabled,
                        //   - we haven't detected this host before (runs once),
                        //   - and the user hasn't set a custom icon override.
                        let has_custom =
                            conn.custom_icon.is_some() || conn.custom_color.is_some();
                        if self.setting_os_detection && conn.detected_os.is_none() && !has_custom {
                            detect_for = Some((conn.id, session));
                        }
                    }
                }
                // Clear progress, show terminal
                self.connecting = None;

                if let Some((conn_id, sess)) = detect_for {
                    return Ok(Task::perform(
                        async move { (conn_id, sess.detect_os().await) },
                        |(id, os)| Message::OsDetected(id, os),
                    ));
                }
            }
            Message::OsDetected(conn_id, os) => {
                // Persist + update in-memory list so the icon refreshes.
                if let Some(vault) = &self.vault {
                    let _ = vault.set_detected_os(&conn_id, os.as_deref());
                }
                if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                    conn.detected_os = os.clone();
                }
                tracing::info!("OS detected for {}: {:?}", conn_id, os);
            }
            Message::SettingToggleOsDetection => {
                self.setting_os_detection = !self.setting_os_detection;
                self.persist_setting(
                    "os_detection",
                    if self.setting_os_detection { "true" } else { "false" },
                );
            }
            Message::SettingToggleAutoCheckUpdates => {
                self.setting_auto_check_updates = !self.setting_auto_check_updates;
                self.persist_setting(
                    "auto_check_updates",
                    if self.setting_auto_check_updates { "true" } else { "false" },
                );
            }
            Message::CheckForUpdate => {
                if !self.setting_auto_check_updates {
                    return Ok(Task::none());
                }
                // Also respect a persisted "skip this version" so we never
                // nag about the same tag twice.
                let skipped = self
                    .vault
                    .as_ref()
                    .and_then(|v| v.get_setting("skipped_update_version").ok().flatten());
                return Ok(Task::perform(
                    crate::update::check_latest_release(),
                    move |opt| {
                        match opt {
                            Some(info) if Some(&info.version) != skipped.as_ref() => {
                                Message::UpdateCheckResult(Some(info))
                            }
                            _ => Message::UpdateCheckResult(None),
                        }
                    },
                ));
            }
            Message::CheckForUpdateManual => {
                // Manual trigger from the settings button — runs regardless
                // of the auto-check preference. Clears prior skipped version
                // so the user can resurface a previously-dismissed prompt.
                self.update_error = None;
                self.update_check_status = Some("Checking…".into());
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", "");
                }
                return Ok(Task::perform(
                    crate::update::check_latest_release(),
                    |info| match info {
                        Some(i) => Message::UpdateCheckResult(Some(i)),
                        None => Message::UpdateCheckResult(None),
                    },
                ));
            }
            Message::UpdateCheckResult(info) => {
                match info {
                    Some(i) => {
                        self.pending_update = Some(i);
                        self.update_check_status = None;
                    }
                    None => {
                        // Only surface the "up to date" message if a manual
                        // check is in flight (status was set to "Checking…").
                        // A silent boot check that finds nothing should not
                        // change the settings UI.
                        if self.update_check_status.is_some() {
                            self.update_check_status = Some(format!(
                                "You're running the latest version ({}).",
                                env!("CARGO_PKG_VERSION"),
                            ));
                        }
                    }
                }
            }
            Message::UpdateSkipVersion => {
                if let Some(info) = self.pending_update.take()
                    && let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", &info.version);
                }
            }
            Message::UpdateLater => {
                self.pending_update = None;
            }
            Message::UpdateOpenRelease => {
                if let Some(info) = &self.pending_update {
                    let _ = open_in_browser(&info.html_url);
                }
            }
            Message::UpdateStartDownload => {
                let Some(info) = self.pending_update.clone() else {
                    return Ok(Task::none());
                };
                let Some(url) = info.installer_url.clone() else {
                    self.update_error = Some("No installer asset for this platform".into());
                    return Ok(Task::none());
                };
                let name = info
                    .installer_name
                    .clone()
                    .unwrap_or_else(|| format!("oryxis-update-{}", info.version));
                self.update_downloading = true;
                self.update_progress = 0.0;
                self.update_error = None;
                return Ok(Task::perform(
                    async move {
                        crate::update::download_installer(&url, &name, |_| {}).await
                    },
                    Message::UpdateDownloadComplete,
                ));
            }
            Message::UpdateDownloadProgress(p) => {
                self.update_progress = p;
            }
            Message::UpdateDownloadComplete(result) => {
                self.update_downloading = false;
                match result {
                    Ok(path) => {
                        if let Err(e) = crate::update::launch_installer(&path) {
                            self.update_error = Some(e);
                        } else {
                            // Installer launched — exit app so it can write
                            // over our binary. Graceful quit via window close.
                            self.pending_update = None;
                            return Ok(iced::window::latest().then(|id_opt| match id_opt {
                                Some(id) => iced::window::close(id),
                                None => Task::none(),
                            }));
                        }
                    }
                    Err(e) => self.update_error = Some(e),
                }
            }
            Message::SshDisconnected(tab_idx) => {
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    let label = tab.label.replace(" (disconnected)", "");
                    // End session log
                    if let Some(log_id) = tab.active().session_log_id
                        && let Some(vault) = &self.vault {
                            let _ = vault.end_session_log(&log_id);
                    }
                    // Log
                    if let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Disconnected, "Session ended",
                        );
                        let _ = vault.add_log(&entry);
                    }
                    tab.label = format!("{} (disconnected)", label);
                    tab.active_mut().ssh_session = None;
                    // Refresh session logs list
                    if let Some(vault) = &self.vault {
                        self.session_logs = vault.list_session_logs().unwrap_or_default();
                    }
                }
            }
            Message::SshCloseProgress => {
                // Close connection progress, remove the tab
                if let Some(ref progress) = self.connecting {
                    let tab_idx = progress.tab_idx;
                    if tab_idx < self.tabs.len() {
                        self.tabs.remove(tab_idx);
                    }
                }
                self.connecting = None;
                self.active_tab = None;
                self.active_view = View::Dashboard;
            }
            Message::SshEditFromProgress => {
                if let Some(ref progress) = self.connecting {
                    let idx = progress.connection_idx;
                    let tab_idx = progress.tab_idx;
                    self.connecting = None;
                    if tab_idx < self.tabs.len() {
                        self.tabs.remove(tab_idx);
                    }
                    self.active_tab = None;
                    self.active_view = View::Dashboard;
                    return Ok(self.update(Message::EditConnection(idx)));
                }
            }
            Message::SshRetry => {
                if let Some(ref progress) = self.connecting {
                    let idx = progress.connection_idx;
                    let tab_idx = progress.tab_idx;
                    self.connecting = None;
                    if tab_idx < self.tabs.len() {
                        self.tabs.remove(tab_idx);
                    }
                    self.active_tab = None;
                    return Ok(self.update(Message::ConnectSsh(idx)));
                }
            }
            Message::SshError(err) => {
                tracing::error!("SSH error: {}", err);
                if let Some(vault) = &self.vault {
                    let label = self.connecting.as_ref().map(|p| p.label.as_str()).unwrap_or("unknown");
                    let entry = oryxis_core::models::log_entry::LogEntry::new(
                        label, label, oryxis_core::models::log_entry::LogEvent::Error, &err,
                    );
                    let _ = vault.add_log(&entry);
                }
                // Mark progress as failed (keep the view open with logs)
                if let Some(ref mut progress) = self.connecting {
                    progress.failed = true;
                    progress.logs.push((progress.step, format!("Error: {}", err)));
                } else {
                    self.host_panel_error = Some(format!("SSH: {}", err));
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
