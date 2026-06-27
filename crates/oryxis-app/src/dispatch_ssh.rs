//! `Oryxis::handle_ssh`, match arms for SSH connection lifecycle
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

use oryxis_core::models::cloud::TransportKind;
use oryxis_core::models::connection::AuthMethod;
use oryxis_ssh::{SshEngine, SshSession};
use oryxis_terminal::widget::TerminalState;

use crate::app::{Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
use crate::state::{
    ConnectionProgress, ConnectionStep, SshStreamMsg, TerminalTab, View,
};
use crate::util::open_in_browser;

/// Items streamed from a per-pane SSH connect (split-into-host). Mirrors
/// `SshStreamMsg` but trimmed to what a pane needs: host-key prompts go to
/// the shared modal, data/connect/disconnect route by pane id.
enum PaneConnMsg {
    HostKey(oryxis_ssh::HostKeyQuery),
    Kbi(oryxis_ssh::KbiQuery),
    Connected(Arc<SshSession>),
    Data(Vec<u8>),
    Disconnected,
    Error(String),
}

impl Oryxis {
    /// Whether a new session should be recorded to the vault. A per-host
    /// `Connection.session_logging` override wins; otherwise the global
    /// `session_logging` setting decides. Panes without a saved
    /// connection (cloud / SSM / local) fall through to the global value.
    pub(crate) fn should_record_session(
        &self,
        conn: Option<&oryxis_core::models::connection::Connection>,
    ) -> bool {
        conn.and_then(|c| c.session_logging)
            .unwrap_or(self.setting_session_logging)
    }

    /// Whether connection events (connect / disconnect / auth failure /
    /// error) should be written to the vault log. Gated by the global
    /// `connection_history` setting (off by default).
    pub(crate) fn should_record_history(&self) -> bool {
        self.setting_connection_history
    }

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
                // If this pick is filling a split pane (not a new tab),
                // route to the per-pane connect path instead.
                if let Some((tab_idx, target, axis)) = self.pending_pane_split.take() {
                    return Ok(self.connect_ssh_into_pane(idx, tab_idx, target, axis));
                }
                if let Some(mut conn) = self.connections.get(idx).cloned() {
                    // SSM Session transport short-circuits the SSH
                    // pipeline entirely, it goes through
                    // `session-manager-plugin` instead of opening a
                    // TCP+SSH connection. Punt to the dedicated
                    // dispatch handler before we waste time setting up
                    // the SSH-specific state below.
                    if let Some(cref) = conn.cloud_ref.as_ref()
                        && cref.transport_pref == TransportKind::Ssm
                    {
                        return Ok(self.start_ssm_session_for_connection(&conn));
                    }
                    // Resolve the effective proxy (saved identity OR inline)
                    // and hydrate its password from the encrypted vault column,
                    // then collapse onto `conn.proxy`, the engine only reads
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
                            // Only the jump-chain hosts are looked up by
                            // the engine; cloning the full host list per
                            // connect is wasted work on large vaults.
                            connections: self
                                .connections
                                .iter()
                                .filter(|c| conn.jump_chain.contains(&c.id))
                                .cloned()
                                .collect(),
                            passwords,
                            private_keys: keys,
                            proxies,
                        })
                    } else {
                        None
                    };

                    match TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16) {
                        Ok(mut state) => {
                            // Pick the per-host override first, then
                            // the global override, then the app
                            // theme. The terminal repaints itself
                            // anyway when the user later switches
                            // themes, but starting on the right
                            // palette avoids a one-frame flash.
                            state.palette =
                                self.resolve_terminal_palette_for_connection(&conn);
                            let label = conn.label.clone();
                            let hostname = format!("SSH {}:{}", conn.hostname, conn.port);
                            let terminal = Arc::new(Mutex::new(state));
                            let tab_idx = self.tabs.len();

                            // Create session log for terminal recording,
                            // unless recording is disabled (per-host
                            // override or the global setting).
                            let session_log_id = if self.should_record_session(Some(&conn)) {
                                if let Some(vault) = &self.vault {
                                    let log_id = Uuid::new_v4();
                                    if let Err(e) =
                                        vault.create_session_log(&log_id, &conn.id, &conn.label)
                                    {
                                        tracing::warn!("session log create failed: {e}");
                                    }
                                    // Keep the in-memory count live so the
                                    // History nav stays visible if logging is
                                    // toggled off mid-session.
                                    self.session_logs_total += 1;
                                    Some(log_id)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            let mut new_tab = TerminalTab::new_single(
                                label.clone(),
                                Arc::clone(&terminal),
                            );
                            new_tab.active_mut().session_log_id = session_log_id;
                            // Referenceable by id, so a session group saved
                            // from this tab can reconnect this pane.
                            new_tab.active_mut().origin =
                                crate::state::PaneOrigin::Host(conn.id);
                            // Stable id of this tab's pane: PTY output and
                            // session events route to it, so the right pane
                            // gets the bytes even after the tab is split.
                            let pane_id = new_tab.active().id;
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
                            self.remember_terminal_tab_focus(tab_idx);

                            // Host key verification: check callback + ask channel
                            let known_hosts_snapshot: Arc<Mutex<Vec<oryxis_core::models::known_host::KnownHost>>> =
                                Arc::new(Mutex::new(self.known_hosts.clone()));
                            let kh_ref = known_hosts_snapshot.clone();
                            let host_key_check: oryxis_ssh::HostKeyCheckCallback = Arc::new(move |host, port, key_type, fingerprint| {
                                // Tolerate a poisoned mutex (some other lock-holder panicked)
                                // by recovering the inner data rather than panicking the SSH
                                // verification callback, better to fall back to "Unknown" and
                                // re-prompt the user than to crash mid-connect.
                                let hosts = match kh_ref.lock() {
                                    Ok(guard) => guard,
                                    Err(poison) => poison.into_inner(),
                                };
                                // Match on key type too: a server legitimately
                                // offering a different algorithm than the one
                                // stored is an "Unknown" (verify and accept),
                                // not a scary "Changed" MITM warning, which
                                // must stay reserved for a real fingerprint
                                // mismatch on the same key type.
                                if let Some(existing) = hosts.iter().find(|h| {
                                    h.hostname == host && h.port == port && h.key_type == key_type
                                }) {
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

                            // Same ask/answer pair for keyboard-interactive
                            // (2FA / OTP) prompts when auth method is Interactive.
                            let (kbi_ask_tx, mut kbi_ask_rx) = tokio::sync::mpsc::channel::<(oryxis_ssh::KbiQuery, tokio::sync::oneshot::Sender<Option<Vec<String>>>)>(1);
                            let (kbi_resp_tx, mut kbi_resp_rx) = tokio::sync::mpsc::channel::<Option<Vec<String>>>(1);
                            self.kbi_response_tx = Some(kbi_resp_tx);

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
                            let keepalive = self.effective_keepalive(&conn);
                            let agent_forwarding = conn.agent_forwarding;
                            let env_vars: Vec<(String, String)> = conn
                                .env_vars
                                .iter()
                                .filter(|e| !e.key.trim().is_empty())
                                .map(|e| (e.key.clone(), e.value.clone()))
                                .collect();
                            let encoding = conn.encoding.clone();
                            let terminal_type = conn.terminal_type.clone();
                            let algo_ciphers = conn.ciphers.clone();
                            let algo_kex = conn.kex.clone();
                            let algo_macs = conn.macs.clone();
                            let algo_host_keys = conn.host_key_algorithms.clone();

                            // Resolve EC2 Instance Connect pre-step
                            // when the connection's `cloud_ref` asks
                            // for it. Tri-state result so the closure
                            // can either skip silently (not asked
                            // for), run the API call (have everything),
                            // or surface a clear setup error (asked
                            // for it but missing key / profile).
                            // Box `Run` so the enum's stack size matches
                            // its smallest variant, otherwise clippy
                            // flags the variant disparity.
                            struct InstanceConnectRun {
                                provider:
                                    std::sync::Arc<dyn oryxis_cloud::CloudProvider>,
                                profile: oryxis_core::models::cloud_profile::CloudProfile,
                                region: String,
                                instance_id: String,
                                os_user: String,
                                public_key: String,
                            }
                            enum InstanceConnectPlan {
                                Skip,
                                Run(Box<InstanceConnectRun>),
                                MissingKey,
                                MissingProfile,
                                MissingRegion,
                            }
                            let instance_connect_plan: InstanceConnectPlan = (|| {
                                let Some(cref) = conn.cloud_ref.as_ref() else {
                                    return InstanceConnectPlan::Skip;
                                };
                                if cref.transport_pref != TransportKind::InstanceConnect {
                                    return InstanceConnectPlan::Skip;
                                }
                                let Some(region) = cref.region.clone() else {
                                    return InstanceConnectPlan::MissingRegion;
                                };
                                let Some(profile) = self
                                    .cloud_profiles
                                    .iter()
                                    .find(|p| p.id == cref.profile_id)
                                    .cloned()
                                else {
                                    return InstanceConnectPlan::MissingProfile;
                                };
                                // The provider is the plugin that pushes the key.
                                // It's seeded at boot and effectively always
                                // present; fold the can't-happen "not registered"
                                // case into MissingProfile rather than adding a
                                // variant (and an i18n key in 11 languages) for it.
                                let Some(provider) =
                                    self.cloud_provider_registry.get(&profile.provider)
                                else {
                                    return InstanceConnectPlan::MissingProfile;
                                };
                                let key_id = conn.key_id.or_else(|| {
                                    conn.identity_id.and_then(|iid| {
                                        self.identities
                                            .iter()
                                            .find(|i| i.id == iid)
                                            .and_then(|i| i.key_id)
                                    })
                                });
                                let Some(key_id) = key_id else {
                                    return InstanceConnectPlan::MissingKey;
                                };
                                let Some(pubkey) = self
                                    .keys
                                    .iter()
                                    .find(|k| k.id == key_id)
                                    .map(|k| k.public_key.clone())
                                else {
                                    return InstanceConnectPlan::MissingKey;
                                };
                                if pubkey.trim().is_empty() {
                                    return InstanceConnectPlan::MissingKey;
                                }
                                InstanceConnectPlan::Run(Box::new(InstanceConnectRun {
                                    provider,
                                    profile,
                                    region,
                                    instance_id: cref.resource_id.clone(),
                                    os_user: username.clone(),
                                    public_key: pubkey,
                                }))
                            })();

                            // Captured (Copy) for the map closure below, since
                            // `conn` itself is moved into the stream producer.
                            let map_conn_id = conn.id;
                            let map_idx = idx;
                            let stream = iced::stream::channel::<SshStreamMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<SshStreamMsg>| {
                                async move {
                                    let engine = SshEngine::new()
                                        .with_host_key_check(host_key_check)
                                        .with_host_key_ask(hk_ask_tx)
                                        .with_kbi_ask(kbi_ask_tx)
                                        .with_keepalive(keepalive)
                                        .with_agent_forwarding(agent_forwarding)
                                        .with_env_vars(env_vars)
                                        .with_encoding(encoding)
                                        .with_terminal_type(terminal_type)
                                        .with_algorithm_overrides(algo_ciphers, algo_kex, algo_macs, algo_host_keys);

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

                                    // Same bridge for keyboard-interactive prompts.
                                    // A dropped response channel resolves to `None`
                                    // (cancel), which the engine treats as a clean
                                    // auth abort rather than a hang.
                                    let mut kbi_sender_clone = sender.clone();
                                    let _kbi_bridge = tokio::spawn(async move {
                                        while let Some((query, resp_tx)) = kbi_ask_rx.recv().await {
                                            let _ = kbi_sender_clone.send(SshStreamMsg::KbiPrompt(query)).await;
                                            let answers = kbi_resp_rx.recv().await.unwrap_or(None);
                                            let _ = resp_tx.send(answers);
                                        }
                                    });

                                    tracing::info!(
                                        target = "oryxis::dispatch_ssh",
                                        plan = match &instance_connect_plan {
                                            InstanceConnectPlan::Skip => "skip (no cloud_ref or transport != InstanceConnect)",
                                            InstanceConnectPlan::Run(_) => "run (push key via SendSSHPublicKey)",
                                            InstanceConnectPlan::MissingKey => "abort (no SSH key linked)",
                                            InstanceConnectPlan::MissingProfile => "abort (cloud profile gone)",
                                            InstanceConnectPlan::MissingRegion => "abort (region missing on cloud_ref)",
                                        },
                                        "Instance Connect pre-step decision"
                                    );

                                    // Pre-step: EC2 Instance Connect.
                                    // AWS injects the public key into
                                    // the instance's authorized_keys
                                    // for ~60s; we have that window
                                    // to dial. Setup misconfigurations
                                    // (missing key / profile / region)
                                    // bail loudly here instead of
                                    // silently degrading to plain SSH
                                    //, that path would just confuse
                                    // the user into wondering why the
                                    // transport pick didn't take.
                                    match instance_connect_plan {
                                        InstanceConnectPlan::Skip => {}
                                        InstanceConnectPlan::Run(run) => {
                                            let InstanceConnectRun {
                                                provider,
                                                profile,
                                                region,
                                                instance_id,
                                                os_user,
                                                public_key,
                                            } = *run;
                                            let _ = sender
                                                .send(SshStreamMsg::Progress(
                                                    ConnectionStep::Connecting,
                                                    format!(
                                                        "Pushing temporary public key to {instance_id} via EC2 Instance Connect…"
                                                    ),
                                                ))
                                                .await;
                                            if let Err(e) = provider
                                                .push_instance_connect_key(
                                                    &profile,
                                                    &region,
                                                    &instance_id,
                                                    &os_user,
                                                    &public_key,
                                                )
                                                .await
                                            {
                                                let _ = sender
                                                    .send(SshStreamMsg::Error(format!(
                                                        "EC2 Instance Connect push failed: {e}"
                                                    )))
                                                    .await;
                                                return;
                                            }
                                        }
                                        InstanceConnectPlan::MissingKey => {
                                            let _ = sender
                                                .send(SshStreamMsg::Error(
                                                    crate::i18n::t("ic_err_missing_key").into(),
                                                ))
                                                .await;
                                            return;
                                        }
                                        InstanceConnectPlan::MissingProfile => {
                                            let _ = sender
                                                .send(SshStreamMsg::Error(
                                                    crate::i18n::t("ic_err_missing_profile").into(),
                                                ))
                                                .await;
                                            return;
                                        }
                                        InstanceConnectPlan::MissingRegion => {
                                            let _ = sender
                                                .send(SshStreamMsg::Error(
                                                    crate::i18n::t("ic_err_missing_region").into(),
                                                ))
                                                .await;
                                            return;
                                        }
                                    }

                                    // Step 1: TCP connection + SSH handshake + host key verification
                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::Connecting,
                                        format!("Connecting to {}:{}...", conn_host, conn_port),
                                    )).await;

                                    let mut handle = match engine.establish_transport(&conn, resolver.as_ref()).await {
                                        Ok(h) => {
                                            let _ = sender.send(SshStreamMsg::Progress(
                                                ConnectionStep::Handshake,
                                                format!("Connected to {}:{}, handshake OK", conn_host, conn_port),
                                            )).await;
                                            h
                                        }
                                        Err(e) => {
                                            // A "no common algorithm" failure becomes a
                                            // legacy-fallback offer instead of a dead end.
                                            if let Some(nf) = e.negotiation_failure() {
                                                let _ = sender.send(SshStreamMsg::NoCommonAlgo {
                                                    category: nf.category,
                                                    server_offers: nf.server_offers,
                                                }).await;
                                            } else {
                                                let _ = sender.send(SshStreamMsg::Error(
                                                    format!("Connection to {}:{} failed: {}", conn_host, conn_port, e),
                                                )).await;
                                            }
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
                                        Message::SshConnected(pane_id, session)
                                    }
                                    SshStreamMsg::HostKeyVerify(query) => {
                                        Message::SshHostKeyVerify(query)
                                    }
                                    SshStreamMsg::KbiPrompt(query) => {
                                        Message::SshKbiPrompt(query)
                                    }
                                    SshStreamMsg::Data(data) => {
                                        Message::PtyOutput(pane_id, data)
                                    }
                                    SshStreamMsg::Error(err) => Message::SshError(err),
                                    SshStreamMsg::NoCommonAlgo { category, server_offers } => {
                                        Message::SshNoCommonAlgo {
                                            conn_id: map_conn_id,
                                            category,
                                            server_offers,
                                            retry: Box::new(Message::ConnectSsh(map_idx)),
                                        }
                                    }
                                    SshStreamMsg::Disconnected => {
                                        Message::SshDisconnected(pane_id)
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
            Message::SshHostKeyVerify(query) => {
                self.pending_host_key = Some(query);
                // Bind the responder to this prompt by cloning the staging
                // slot. A connect that starts later overwrites only the
                // staging slot, so the answer the user gives here still goes
                // back to the connect whose host they actually saw. Clone
                // (not take): one connect's bridge is a loop that can raise
                // several host-key prompts (e.g. a jump chain with two
                // unknown hops), and every answer rides the same sender.
                self.active_host_key_tx = self.host_key_response_tx.clone();
            }
            Message::SshHostKeyReject => {
                self.pending_host_key = None;
                if let Some(tx) = self.active_host_key_tx.take() {
                    let _ = tx.try_send(false);
                }
            }
            Message::SshHostKeyContinue => {
                // Accept for this session but don't save to known hosts
                self.pending_host_key = None;
                if let Some(tx) = self.active_host_key_tx.take() {
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
                if let Some(tx) = self.active_host_key_tx.take() {
                    let _ = tx.try_send(true);
                }
            }
            Message::SshKbiPrompt(query) => {
                // One empty answer buffer per prompt, parallel to query.prompts.
                self.kbi_inputs = vec![String::new(); query.prompts.len()];
                self.pending_kbi_prompt = Some(query);
                // Land focus in the first prompt field so OTP entry is
                // type-and-Enter without a click.
                return Ok(iced::widget::operation::focus(iced::widget::Id::new(
                    crate::state::KBI_FIRST_INPUT_ID,
                )));
            }
            Message::SshKbiInput(idx, value) => {
                if let Some(slot) = self.kbi_inputs.get_mut(idx) {
                    *slot = value;
                }
            }
            Message::SshKbiSubmit => {
                let answers = std::mem::take(&mut self.kbi_inputs);
                self.pending_kbi_prompt = None;
                if let Some(ref tx) = self.kbi_response_tx {
                    let _ = tx.try_send(Some(answers));
                }
            }
            Message::SshKbiCancel => {
                self.pending_kbi_prompt = None;
                self.kbi_inputs.clear();
                if let Some(ref tx) = self.kbi_response_tx {
                    let _ = tx.try_send(None);
                }
            }
            Message::SshConnected(pane_id, session) => {
                let mut detect_for: Option<(Uuid, Arc<SshSession>)> = None;
                if let Some(tab_idx) = self.pane_tab_index(pane_id) {
                    let label = self.tabs[tab_idx].label.clone();
                    // Attach the session to the specific pane that connected
                    // and forward future viewport resizes to the server so
                    // remote `top`/`vim` re-layout instead of overflowing.
                    if let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id) {
                        pane.ssh_session = Some(session.clone());
                        if let Ok(mut state) = pane.terminal.lock() {
                            state.set_remote_resize_sender(session.resize_sender());
                            session.resize(state.cols(), state.rows());
                        }
                    }
                    // Startup command, fired as keystrokes right after the
                    // session is wired. The SSH channel buffers input until
                    // the shell is ready, so the line lands cleanly; the
                    // newline triggers `Enter` on the remote.
                    //
                    // A session-group per-pane script (keyed by pane_id) wins
                    // over the host's own `initial_command`. The fallback is
                    // resolved via the pane's origin rather than the tab label
                    // so it stays correct for group tabs (whose label is the
                    // group name) and for two panes sharing one host.
                    // A live snippet reference (its body, looked up now so
                    // snippet edits propagate) wins over the literal
                    // `initial_command`; a dangling snippet id resolves to
                    // nothing, never an error.
                    let (startup_snip, startup_lit) = self
                        .pane_origin_connection(pane_id)
                        .map(|c| (c.startup_snippet_id, c.initial_command.clone()))
                        .unwrap_or((None, None));
                    let fallback_cmd = match startup_snip {
                        Some(id) => self
                            .snippets
                            .iter()
                            .find(|s| s.id == id)
                            .map(|s| s.command.clone()),
                        None => startup_lit,
                    };
                    let initial = self
                        .pane_script_overrides
                        .remove(&pane_id)
                        .filter(|s| !s.trim().is_empty())
                        .or(fallback_cmd)
                        .filter(|s| !s.trim().is_empty());
                    if let Some(cmd) = initial {
                        let payload = format!("{cmd}\n");
                        if let Err(e) = session.write(payload.as_bytes()) {
                            tracing::warn!(
                                target = "oryxis::dispatch_ssh",
                                error = %e,
                                "failed to send startup command"
                            );
                        } else {
                            tracing::info!(
                                target = "oryxis::dispatch_ssh",
                                bytes = payload.len(),
                                "sent startup command after session ready"
                            );
                        }
                    }
                    tracing::info!("SSH connected: {}", label);
                    if self.should_record_history()
                        && let Some(vault) = &self.vault {
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
            Message::SettingToggleSessionLogging => {
                self.setting_session_logging = !self.setting_session_logging;
                self.persist_setting(
                    "session_logging",
                    if self.setting_session_logging { "true" } else { "false" },
                );
            }
            Message::SettingToggleConnectionHistory => {
                self.setting_connection_history = !self.setting_connection_history;
                self.persist_setting(
                    "connection_history",
                    if self.setting_connection_history { "true" } else { "false" },
                );
            }
            Message::LogsRetentionChanged(code) => {
                self.setting_logs_retention = code.to_string();
                self.persist_setting("logs_retention", code);
                // Apply right away so picking a shorter window has a
                // visible effect, then refresh the cached Logs state.
                if let Some(days) = Self::retention_days(code)
                    && let Some(vault) = &self.vault
                {
                    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
                    match vault.prune_logs_older_than(cutoff) {
                        Ok(0) => {}
                        Ok(n) => tracing::info!("logs retention pruned {n} rows"),
                        Err(e) => tracing::warn!("logs retention prune failed: {e}"),
                    }
                    self.logs_page = 0;
                    self.session_logs_page = 0;
                    self.logs_total = vault.count_logs().unwrap_or(0);
                    self.logs = vault.list_logs_page(0, 50).unwrap_or_default();
                    self.session_logs_total = vault.count_session_logs().unwrap_or(0);
                    self.session_logs =
                        vault.list_session_logs_page(0, 50).unwrap_or_default();
                }
            }
            Message::SettingToggleAutoCheckUpdates => {
                self.setting_auto_check_updates = !self.setting_auto_check_updates;
                self.persist_setting(
                    "auto_check_updates",
                    if self.setting_auto_check_updates { "true" } else { "false" },
                );
            }
            Message::SettingUpdateChannelChanged(channel) => {
                self.setting_update_channel = channel;
                self.persist_setting("update_channel", channel.as_setting());
                // A channel switch invalidates any "skip this version" so
                // the user is offered the other stream's build right away.
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", "");
                }
                // Switching channel is an explicit intent to follow that
                // stream, so re-check immediately (surfacing the same
                // "Checking…" status + toast as a manual check) instead of
                // waiting for the next boot check.
                self.update_error = None;
                self.update_check_status = Some(crate::update::UpdateStatus::Checking);
                self.toast = Some(crate::i18n::t("update_check_checking").to_string());
                return Ok(Task::perform(
                    crate::update::check_latest_release(channel),
                    |res| match res {
                        Ok(info) => Message::UpdateCheckResult(info),
                        Err(e) => Message::UpdateCheckFailed(e.to_string()),
                    },
                ));
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
                    crate::update::check_latest_release(self.setting_update_channel),
                    move |res| {
                        match res {
                            Ok(Some(info)) if Some(&info.version) != skipped.as_ref() => {
                                Message::UpdateCheckResult(Some(info))
                            }
                            // Boot check is best-effort: log the failure
                            // but never surface it in the UI.
                            Err(e) => {
                                tracing::warn!("update check failed: {e}");
                                Message::UpdateCheckResult(None)
                            }
                            _ => Message::UpdateCheckResult(None),
                        }
                    },
                ));
            }
            Message::CheckForUpdateManual => {
                // Manual trigger from the settings button OR the burger
                // menu. Navigate to Settings > About so the result
                // (up-to-date / error + retry) is on screen regardless
                // of where the check was fired from (issue #38: the
                // burger-menu path previously looked like a no-op).
                self.show_burger_menu = false;
                self.editing_hotkey = None;
                self.active_view = crate::state::View::Settings;
                self.settings_section = crate::state::SettingsSection::About;
                self.active_tab = None;
                self.update_error = None;
                self.update_check_status = Some(crate::update::UpdateStatus::Checking);
                self.toast = Some(crate::i18n::t("update_check_checking").to_string());
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", "");
                }
                return Ok(Task::perform(
                    crate::update::check_latest_release(self.setting_update_channel),
                    |res| match res {
                        Ok(info) => Message::UpdateCheckResult(info),
                        Err(e) => Message::UpdateCheckFailed(e.to_string()),
                    },
                ));
            }
            Message::UpdateCheckResult(info) => {
                match info {
                    Some(i) => {
                        // Surface the new version as a toast too so a
                        // burger-menu-triggered check confirms the
                        // result even before the update modal renders.
                        self.toast = Some(format!(
                            "{} {}",
                            crate::i18n::t("update_check_available"),
                            i.version,
                        ));
                        self.pending_update = Some(i);
                        self.update_check_status = None;
                    }
                    None => {
                        // Only surface the "up to date" message if a manual
                        // check is in flight (status was set to Checking).
                        // A silent boot check that finds nothing should not
                        // change the settings UI.
                        if self.update_check_status.is_some() {
                            self.update_check_status =
                                Some(crate::update::UpdateStatus::UpToDate);
                            self.toast = Some(format!(
                                "{} ({})",
                                crate::i18n::t("update_check_up_to_date"),
                                env!("CARGO_PKG_VERSION"),
                            ));
                        }
                    }
                }
                // Auto-dismiss the toast after the standard 1.8 s
                // window matches the existing "copied to clipboard"
                // toast cadence so users get consistent feedback timing.
                return Ok(Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
                    },
                    |_| Message::ToastClear,
                ));
            }
            Message::UpdateCheckFailed(cause) => {
                // Same gating as the up-to-date arm: only a manual check
                // (status in flight) reports; boot checks already logged.
                if self.update_check_status.is_some() {
                    self.update_check_status =
                        Some(crate::update::UpdateStatus::Failed(cause.clone()));
                    self.toast = Some(format!(
                        "{}: {}",
                        crate::i18n::t("update_check_failed"),
                        cause,
                    ));
                }
                return Ok(Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
                    },
                    |_| Message::ToastClear,
                ));
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
                // Stream so the modal's progress bar moves with the
                // download instead of jumping 0 to done. The sync
                // progress closure forwards into the async sink via an
                // unbounded channel.
                let stream = iced::stream::channel::<Message>(
                    100,
                    move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                        use iced::futures::SinkExt as _;
                        let (ptx, mut prx) = tokio::sync::mpsc::unbounded_channel::<f32>();
                        let mut dl = tokio::spawn(async move {
                            crate::update::download_installer(&url, &name, move |p| {
                                let _ = ptx.send(p);
                            })
                            .await
                        });
                        loop {
                            tokio::select! {
                                Some(p) = prx.recv() => {
                                    let _ = sender
                                        .send(Message::UpdateDownloadProgress(p))
                                        .await;
                                }
                                res = &mut dl => {
                                    let result =
                                        res.unwrap_or_else(|e| Err(e.to_string()));
                                    let _ = sender
                                        .send(Message::UpdateDownloadComplete(result))
                                        .await;
                                    break;
                                }
                            }
                        }
                    },
                );
                return Ok(Task::stream(stream));
            }
            Message::UpdateDownloadProgress(p) => {
                self.update_progress = p;
            }
            Message::UpdateDownloadComplete(result) => {
                self.update_downloading = false;
                match result {
                    Ok(path) => {
                        // Nightly ships a bare binary we swap in place;
                        // stable hands a downloaded installer to the OS.
                        let apply = match self.pending_update.as_ref().map(|i| i.artifact) {
                            Some(crate::update::UpdateArtifact::Binary) => {
                                crate::update::apply_binary_update(&path)
                            }
                            _ => crate::update::launch_installer(&path),
                        };
                        if let Err(e) = apply {
                            self.update_error = Some(e);
                        } else {
                            // Installer launched (or new binary spawned),
                            // exit so the old binary is released. Graceful
                            // quit via window close.
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
            Message::SshDisconnected(pane_id) => {
                // Persist whatever this pane recorded before we mark the
                // log ended; otherwise the tail of the session is lost.
                self.flush_session_logs_final();
                if let Some(tab_idx) = self.pane_tab_index(pane_id) {
                    let label = self.tabs[tab_idx].label.replace(" (disconnected)", "");
                    // Clear the disconnected pane's session + end its log.
                    let log_id = self.tabs[tab_idx].pane_by_id_mut(pane_id).and_then(|p| {
                        p.ssh_session = None;
                        p.session_log_id
                    });
                    if let Some(log_id) = log_id
                        && let Some(vault) = &self.vault
                    {
                        let _ = vault.end_session_log(&log_id);
                    }
                    if self.should_record_history()
                        && let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Disconnected, "Session ended",
                        );
                        let _ = vault.add_log(&entry);
                    }
                    // Refresh session logs list (count + current page)
                    if let Some(vault) = &self.vault {
                        self.session_logs_total =
                            vault.count_session_logs().unwrap_or(0);
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                    // The tab-level "(disconnected)" relabel + idle toast +
                    // auto-reconnect only make sense when the tab IS this one
                    // session. A split tab has live sibling panes, relabeling
                    // it would make `AutoReconnectTick` rebuild the whole tab
                    // (`ReconnectTab` removes it), nuking the siblings. So for
                    // a multi-pane tab we just note the disconnect inside the
                    // pane and leave the tab alone.
                    if self.tabs[tab_idx].pane_grid.panes.len() > 1 {
                        if let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id)
                            && let Ok(mut state) = pane.terminal.lock()
                        {
                            state.process(b"\r\n[disconnected]\r\n");
                        }
                        return Ok(Task::none());
                    }
                    self.tabs[tab_idx].label = format!("{} (disconnected)", label);
                    // Surface the disconnect to the user. Without this the
                    // terminal just goes silent and the silent auto-reconnect
                    // (up to 30s later) feels like the shell mysteriously
                    // reset itself. A second toast fires from `ReconnectTab`
                    // when the actual reconnect attempt starts, so the
                    // wording here is intentionally past-tense only.
                    self.toast = Some(crate::i18n::t("disconnected_idle").to_string());
                    return Ok(Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                        },
                        |_| Message::ToastClear,
                    ));
                }
            }
            Message::SshCloseProgress => {
                // Close connection progress, remove the tab
                if let Some(ref progress) = self.connecting {
                    let tab_idx = progress.tab_idx;
                    if tab_idx < self.tabs.len() {
                        self.tabs.remove(tab_idx);
                        self.adjust_last_terminal_tab_after_remove(tab_idx);
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
                        self.adjust_last_terminal_tab_after_remove(tab_idx);
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
                        self.adjust_last_terminal_tab_after_remove(tab_idx);
                    }
                    self.active_tab = None;
                    return Ok(self.update(Message::ConnectSsh(idx)));
                }
            }
            Message::PaneConnectError(pane_id, msg) => {
                // Surface the failure inside the pane that was connecting.
                if let Some(pane) = self
                    .tabs
                    .iter()
                    .flat_map(|t| t.pane_grid.panes.values())
                    .find(|p| p.id == pane_id)
                    && let Ok(mut state) = pane.terminal.lock()
                {
                    state.process(format!("\r\nConnection failed: {msg}\r\n").as_bytes());
                }
                // A failed *in-place reconnect* (single-pane tab whose label
                // matches a saved host) must fall back to the "(disconnected)"
                // state so `AutoReconnectTick` keeps retrying up to
                // `max_reconnect_attempts`. Split tabs (>1 pane) share this
                // message but stay connected via their live sibling panes;
                // session-group tabs carry the group name (no matching host),
                // so neither gets relabeled.
                if let Some(tab_idx) = self.pane_tab_index(pane_id)
                    && self.tabs[tab_idx].pane_grid.panes.len() == 1
                    && !self.tabs[tab_idx].label.ends_with(" (disconnected)")
                {
                    let label = self.tabs[tab_idx].label.clone();
                    if self.connections.iter().any(|c| c.label == label) {
                        self.tabs[tab_idx].label = format!("{label} (disconnected)");
                    }
                }
                tracing::error!("pane SSH connect failed: {msg}");
            }
            Message::SshError(err) => {
                tracing::error!("SSH error: {}", err);
                if self.should_record_history()
                    && let Some(vault) = &self.vault {
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
            Message::SshNoCommonAlgo { conn_id, category, server_offers, retry } => {
                // Only offer the fallback when the failed category is still
                // Auto. If it's already pinned (manually, or by a prior
                // accept that expanded everything) and STILL has no match,
                // the server wants an algorithm russh can't provide, so show
                // a plain error instead of looping the dialog.
                let already_pinned = self
                    .connections
                    .iter()
                    .find(|c| c.id == conn_id)
                    .map(|c| match category {
                        oryxis_ssh::NegCategory::Cipher => c.ciphers.is_some(),
                        oryxis_ssh::NegCategory::Kex => c.kex.is_some(),
                        oryxis_ssh::NegCategory::Mac => c.macs.is_some(),
                        oryxis_ssh::NegCategory::HostKey => c.host_key_algorithms.is_some(),
                    })
                    .unwrap_or(false);
                if already_pinned {
                    if let Some(ref mut progress) = self.connecting {
                        progress.failed = true;
                        progress.logs.push((
                            progress.step,
                            crate::i18n::t("legacy_algo_unsupported").into(),
                        ));
                    }
                } else {
                    // Drop any "busy" backup spinner while the dialog is up so
                    // its retry (SftpBackupConfirm) isn't blocked by the guard.
                    self.sftp_backup_busy = false;
                    self.pending_legacy_algo = Some(crate::state::PendingLegacyAlgo {
                        conn_id,
                        category,
                        server_offers,
                        retry,
                    });
                }
            }
            Message::LegacyAlgoCancel => {
                self.pending_legacy_algo = None;
                let msg = crate::i18n::t("legacy_algo_cancelled");
                if let Some(ref mut progress) = self.connecting {
                    progress.failed = true;
                    progress.logs.push((progress.step, msg.into()));
                }
                // Clear the other paths' transient connecting state so
                // cancelling the dialog never leaves a stuck "busy" backup or
                // a spinning SFTP pane.
                self.sftp_backup_busy = false;
                if self.sftp_backup_open {
                    self.sftp_backup_status = Some(Err(msg.to_string()));
                }
                for side in [
                    crate::state::SftpPaneSide::Left,
                    crate::state::SftpPaneSide::Right,
                ] {
                    let pane = self.sftp.pane_mut(side);
                    if pane.remote_loading {
                        pane.remote_loading = false;
                        pane.error = Some(msg.to_string());
                    }
                }
            }
            Message::LegacyAlgoAccept { remember } => {
                let Some(pending) = self.pending_legacy_algo.take() else {
                    return Ok(Task::none());
                };
                let Some(idx) = self.connections.iter().position(|c| c.id == pending.conn_id)
                else {
                    return Ok(Task::none());
                };
                // Expand every category to the full supported set (secure
                // names stay first, so a modern server still negotiates
                // securely). One retry then covers all legacy categories.
                let to_full = |names: Vec<&'static str>| -> Option<Vec<String>> {
                    Some(names.into_iter().map(|s| s.to_string()).collect())
                };
                {
                    let conn = &mut self.connections[idx];
                    // Secure-first order: the default safe set, then the
                    // legacy entries appended. Pinning raw `supported_*`
                    // here would demote chacha/gcm below 3des/cbc.
                    conn.ciphers = to_full(oryxis_ssh::algorithms::expanded_ciphers());
                    conn.kex = to_full(oryxis_ssh::algorithms::expanded_kex());
                    conn.macs = to_full(oryxis_ssh::algorithms::expanded_macs());
                    conn.host_key_algorithms =
                        to_full(oryxis_ssh::algorithms::expanded_host_keys());
                }
                if remember && let Some(vault) = &self.vault {
                    let _ = vault.save_connection(&self.connections[idx], None);
                }
                // Re-run the originating connect (terminal / SFTP / forward /
                // backup) now that the in-memory connection carries the
                // expanded algorithm set.
                self.pending_legacy_algo = None;
                return Ok(self.update(*pending.retry));
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// Create a new pane next to `target` in tab `tab_idx`, focus it, and
    /// return its stable id (for routing PTY output / session events).
    pub(crate) fn make_split_pane(
        &mut self,
        tab_idx: usize,
        target: iced::widget::pane_grid::Pane,
        axis: iced::widget::pane_grid::Axis,
        label: String,
        terminal: Arc<Mutex<TerminalState>>,
        origin: crate::state::PaneOrigin,
    ) -> Option<Uuid> {
        let tab = self.tabs.get_mut(tab_idx)?;
        let mut pane = crate::state::Pane::new(label, terminal);
        pane.origin = origin;
        let pane_id = pane.id;
        let (handle, _split) = tab.pane_grid.split(axis, target, pane)?;
        tab.focused = handle;
        Some(pane_id)
    }

    /// Open a local shell into a new split pane.
    pub(crate) fn local_shell_into_pane(
        &mut self,
        tab_idx: usize,
        target: iced::widget::pane_grid::Pane,
        axis: iced::widget::pane_grid::Axis,
    ) -> Task<Message> {
        let Ok((mut state, rx)) =
            TerminalState::new(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16, None)
        else {
            return Task::none();
        };
        state.palette = self.terminal_palette.clone();
        let terminal = Arc::new(Mutex::new(state));
        let label = crate::i18n::t("local_shell").to_string();
        // Default OS shell (empty program). Restored verbatim by the
        // session-group open path.
        let origin = crate::state::PaneOrigin::Local(crate::state::LocalShellSpec {
            label: label.clone(),
            program: String::new(),
            args: Vec::new(),
        });
        let Some(pane_id) = self.make_split_pane(tab_idx, target, axis, label, terminal, origin)
        else {
            return Task::none();
        };
        self.active_tab = Some(tab_idx);
        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        Task::stream(stream).map(move |bytes| Message::PtyOutput(pane_id, bytes))
    }

    /// Resolve the `Connection` a pane was opened from, via its `PaneOrigin`
    /// (not the tab label). Returns `None` for local / ephemeral panes or a
    /// dangling host reference.
    pub(crate) fn pane_origin_connection(
        &self,
        pane_id: Uuid,
    ) -> Option<&oryxis_core::models::Connection> {
        let origin = self
            .tabs
            .iter()
            .flat_map(|t| t.pane_grid.panes.values())
            .find(|p| p.id == pane_id)
            .map(|p| &p.origin)?;
        if let crate::state::PaneOrigin::Host(id) = origin {
            self.connections.iter().find(|c| c.id == *id)
        } else {
            None
        }
    }

    /// Connect a saved host into a new split pane. Uses the one-shot
    /// `connect_with_resolver` (no full progress timeline); the pane shows a
    /// "Connecting…" line until output arrives. Host-key prompts reuse the
    /// shared modal. Cloud-transport hosts fall back to a normal tab.
    pub(crate) fn connect_ssh_into_pane(
        &mut self,
        conn_idx: usize,
        tab_idx: usize,
        target: iced::widget::pane_grid::Pane,
        axis: iced::widget::pane_grid::Axis,
    ) -> Task<Message> {
        let Some(conn) = self.connections.get(conn_idx).cloned() else {
            return Task::none();
        };
        // SSM / ECS / kubectl transports need their own plugin PTY, not a
        // plain SSH session, so they can't live in this pane path yet; open
        // them as a normal tab instead.
        if conn
            .cloud_ref
            .as_ref()
            .is_some_and(|c| c.transport_pref != TransportKind::Ssh)
        {
            return self.update(Message::ConnectSsh(conn_idx));
        }

        // Display-only terminal, fed by the SSH stream (same as a normal SSH
        // tab). Seed a "Connecting…" line for immediate feedback.
        let Ok(mut term) =
            TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16)
        else {
            return Task::none();
        };
        term.palette = self.resolve_terminal_palette_for_connection(&conn);
        term.process(
            format!("Connecting to {} ({}:{})...\r\n", conn.label, conn.hostname, conn.port)
                .as_bytes(),
        );
        let terminal = Arc::new(Mutex::new(term));
        let Some(pane_id) = self.make_split_pane(
            tab_idx,
            target,
            axis,
            conn.label.clone(),
            terminal,
            crate::state::PaneOrigin::Host(conn.id),
        ) else {
            return Task::none();
        };
        self.active_tab = Some(tab_idx);
        self.spawn_ssh_for_pane(conn_idx, tab_idx, pane_id)
    }

    /// Establish an SSH session for an EXISTING pane (already created in
    /// `tab_idx`'s grid with id `pane_id`) and wire its byte stream to that
    /// pane. Split out of `connect_ssh_into_pane` so the session-group open
    /// path can build the whole splitted tab up front (via
    /// `pane_grid::State::with_configuration`) and then connect each pane.
    pub(crate) fn spawn_ssh_for_pane(
        &mut self,
        conn_idx: usize,
        tab_idx: usize,
        pane_id: Uuid,
    ) -> Task<Message> {
        let Some(mut conn) = self.connections.get(conn_idx).cloned() else {
            return Task::none();
        };
        if let Some(vault) = self.vault.as_ref() {
            conn.proxy = vault.resolve_proxy(&conn).ok().flatten();
        }
        let (password, private_key) = self.resolve_forward_credentials(&conn);
        let resolver = self.build_jump_resolver(&conn);
        let host_key_check = self.build_host_key_check();
        let keepalive = self.effective_keepalive(&conn);
        // Parity with the full-tab `ConnectSsh` path: agent forwarding, env
        // vars and a custom encoding must ride the session too, otherwise a
        // split pane (or an in-place reconnect) silently drops them.
        let agent_forwarding = conn.agent_forwarding;
        let env_vars: Vec<(String, String)> = conn
            .env_vars
            .iter()
            .filter(|e| !e.key.trim().is_empty())
            .map(|e| (e.key.clone(), e.value.clone()))
            .collect();
        let encoding = conn.encoding.clone();
        let terminal_type = conn.terminal_type.clone();
        let algo_ciphers = conn.ciphers.clone();
        let algo_kex = conn.kex.clone();
        let algo_macs = conn.macs.clone();
        let algo_host_keys = conn.host_key_algorithms.clone();

        let session_log_id = if self.should_record_session(Some(&conn)) {
            self.vault.as_ref().map(|v| {
                let id = Uuid::new_v4();
                if let Err(e) = v.create_session_log(&id, &conn.id, &conn.label) {
                    tracing::warn!("session log create failed: {e}");
                }
                id
            })
        } else {
            None
        };
        if session_log_id.is_some() {
            // Keep the count live so the History nav doesn't vanish if
            // logging is toggled off while this session is still open.
            self.session_logs_total += 1;
        }
        if let Some(log_id) = session_log_id
            && let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id)
        {
            pane.session_log_id = Some(log_id);
        }

        // Host-key bridge: the engine asks via `hk_ask`, we surface the
        // shared modal (`SshHostKeyVerify`), and the answer comes back on
        // `hk_resp` (driven by the existing SshHostKey* handlers).
        let (hk_ask_tx, mut hk_ask_rx) = tokio::sync::mpsc::channel::<(
            oryxis_ssh::HostKeyQuery,
            tokio::sync::oneshot::Sender<bool>,
        )>(1);
        let (hk_resp_tx, mut hk_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
        // NOTE: a single shared response channel. When a session group opens
        // several panes at once, each call overwrites this, so simultaneous
        // host-key prompts for multiple *first-time-unknown* hosts could
        // mis-route. Deliberate: saved-group hosts are normally already in
        // known_hosts, so no prompt fires. Revisit if batch first-connect
        // becomes common.
        self.host_key_response_tx = Some(hk_resp_tx);

        // Keyboard-interactive (2FA / OTP) bridge, mirroring the host-key one.
        // NOTE: shares the same single-response-channel limitation documented
        // above for host keys. If a session group opens several panes that
        // each hit Interactive auth at once, this `kbi_response_tx` is
        // overwritten per pane and answers could mis-route. Rare in practice
        // (Interactive 2FA + simultaneous group open); revisit if it bites.
        let (kbi_ask_tx, mut kbi_ask_rx) = tokio::sync::mpsc::channel::<(
            oryxis_ssh::KbiQuery,
            tokio::sync::oneshot::Sender<Option<Vec<String>>>,
        )>(1);
        let (kbi_resp_tx, mut kbi_resp_rx) = tokio::sync::mpsc::channel::<Option<Vec<String>>>(1);
        self.kbi_response_tx = Some(kbi_resp_tx);

        let stream = iced::stream::channel::<PaneConnMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<PaneConnMsg>| async move {
            let engine = SshEngine::new()
                .with_host_key_check(host_key_check)
                .with_host_key_ask(hk_ask_tx)
                .with_kbi_ask(kbi_ask_tx)
                .with_keepalive(keepalive)
                .with_agent_forwarding(agent_forwarding)
                .with_env_vars(env_vars)
                .with_encoding(encoding)
                .with_terminal_type(terminal_type)
                .with_algorithm_overrides(algo_ciphers, algo_kex, algo_macs, algo_host_keys);

            let mut sender_clone = sender.clone();
            let _bridge = tokio::spawn(async move {
                while let Some((query, resp_tx)) = hk_ask_rx.recv().await {
                    let _ = sender_clone.send(PaneConnMsg::HostKey(query)).await;
                    let accepted = hk_resp_rx.recv().await.unwrap_or(false);
                    let _ = resp_tx.send(accepted);
                }
            });

            let mut kbi_sender_clone = sender.clone();
            let _kbi_bridge = tokio::spawn(async move {
                while let Some((query, resp_tx)) = kbi_ask_rx.recv().await {
                    let _ = kbi_sender_clone.send(PaneConnMsg::Kbi(query)).await;
                    let answers = kbi_resp_rx.recv().await.unwrap_or(None);
                    let _ = resp_tx.send(answers);
                }
            });

            match engine
                .connect_with_resolver(
                    &conn,
                    password.as_deref(),
                    private_key.as_deref(),
                    DEFAULT_TERM_COLS,
                    DEFAULT_TERM_ROWS,
                    resolver.as_ref(),
                )
                .await
            {
                Ok((session, mut rx)) => {
                    let session = Arc::new(session);
                    let _ = sender.send(PaneConnMsg::Connected(session.clone())).await;
                    while let Some(data) = rx.recv().await {
                        if sender.send(PaneConnMsg::Data(data)).await.is_err() {
                            break;
                        }
                    }
                    let _ = sender.send(PaneConnMsg::Disconnected).await;
                }
                Err(e) => {
                    let _ = sender.send(PaneConnMsg::Error(e.to_string())).await;
                }
            }
        });

        Task::stream(stream).map(move |m| match m {
            PaneConnMsg::HostKey(q) => Message::SshHostKeyVerify(q),
            PaneConnMsg::Kbi(q) => Message::SshKbiPrompt(q),
            PaneConnMsg::Connected(s) => Message::SshConnected(pane_id, s),
            PaneConnMsg::Data(d) => Message::PtyOutput(pane_id, d),
            PaneConnMsg::Disconnected => Message::SshDisconnected(pane_id),
            PaneConnMsg::Error(e) => Message::PaneConnectError(pane_id, e),
        })
    }
}
