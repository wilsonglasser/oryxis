//! The SSH connect paths, split out of `dispatch_ssh`:
//! `start_ssh_tab` (full-tab connect with the progress timeline) and
//! its up-front `ConnectPlan` resolution, the split-pane /
//! quick-connect spawn paths (`spawn_ssh_for_pane_conn` and its
//! wrappers), the pane-entry helpers around them, and the advisory
//! `certificate_is_expired` check.

#![allow(clippy::result_large_err)]

use iced::futures::SinkExt;
use iced::Task;

use std::sync::{Arc, Mutex};
use uuid::Uuid;

use oryxis_core::models::cloud::TransportKind;
use oryxis_ssh::{SshEngine, SshSession};
use oryxis_terminal::widget::TerminalState;

use crate::app::{TerminalMessage, SshMessage, Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
use crate::state::{ConnectionProgress, ConnectionStep, SshStreamMsg, TerminalTab};

/// Items streamed from a per-pane SSH connect (split-into-host). Mirrors
/// `SshStreamMsg` but trimmed to what a pane needs: host-key prompts go to
/// the shared modal, data/connect/disconnect route by pane id.
enum PaneConnMsg {
    HostKey(oryxis_ssh::HostKeyQuery),
    ProxyCommand(oryxis_ssh::ProxyCommandQuery),
    Kbi(oryxis_ssh::KbiQuery),
    /// Pre-auth banner from the server (RFC 4252 §5.4).
    Banner(String),
    Connected(Arc<SshSession>),
    Data(Vec<u8>),
    Disconnected,
    Error(String),
}

/// Everything the SSH spawn tail of `start_ssh_tab` needs, resolved
/// up front by `resolve_connect_plan` so the sequential resolution
/// phases read individually instead of as one flat body.
struct ConnectPlan {
    password: Option<String>,
    private_key: Option<String>,
    certificate: Option<String>,
    pinned_agent: Option<String>,
    totp_secret: Option<String>,
    resolver: Option<oryxis_ssh::ConnectionResolver>,
}

impl Oryxis {
    /// Resolve everything the SSH spawn needs for `conn` up front:
    /// the effective proxy (collapsed onto `conn.proxy`), credentials
    /// (password / key / certificate), the pinned agent key, the TOTP
    /// secret, the quick-connect secret overlay, and the jump-host
    /// resolver. Pure resolution, no UI side effects: the advisory
    /// expired-certificate toast stays in `start_ssh_tab` (it needs
    /// `&mut self`).
    fn resolve_connect_plan(
        &self,
        conn: &mut oryxis_core::models::Connection,
        origin: crate::state::ProgressOrigin,
    ) -> ConnectPlan {
        // Group inheritance + the effective proxy, in one place
        // shared with the split / quick-connect / reconnect path.
        self.apply_group_inheritance(conn);

        // Resolve credentials: prefer identity if linked, otherwise inline
        // (shared helper, which also resolves the key's certificate for B2).
        let (password, private_key, certificate) = self.resolve_credentials(conn);
        // Agent-auth pin (B3): the referenced key's public line, offered
        // first when agent auth runs (Agent method, or the Auto ladder).
        let pinned_agent = self.pinned_agent_public(conn);

        // Per-connection TOTP secret for keyboard-interactive
        // autofill. Independent of the identity indirection
        // above, 2FA enrollment is per-host.
        let totp_secret = self
            .vault
            .as_ref()
            .and_then(|v| v.get_connection_totp_secret(&conn.id).ok().flatten());

        // Quick-connect entries have no vault rows, so every lookup above
        // missed. Overlay the credentials typed in the editor flow
        // (password / TOTP / inline-proxy password).
        let (mut password, mut totp_secret) = (password, totp_secret);
        if let crate::state::ProgressOrigin::Quick(id) = origin {
            self.apply_quick_entry_secrets(id, conn, &mut password, &mut totp_secret);
        }

        // Jump-host resolver, through the SAME builder the split /
        // reconnect path uses. Hand-rolling it here left the tab path
        // without hop inheritance (D4) and without a hop's linked
        // identity: a bastion inheriting its username from a group
        // authenticated as "root" here while the pane path resolved it.
        let resolver = self.make_jump_resolver(conn);

        ConnectPlan {
            password,
            private_key,
            certificate,
            pinned_agent,
            totp_secret,
            resolver,
        }
    }

    /// Open a new tab for `conn` and drive the SSH connect pipeline into
    /// it (progress view, host-key / KBI bridges, PTY stream). Shared by
    /// saved hosts (`ConnectSsh`, `origin = Saved(idx)`) and ad-hoc quick
    /// connects (`QuickConnect`, `origin = Quick(id)`); the origin decides
    /// the pane identity, the progress Retry / Edit resolution, and the
    /// interactive-fallback auth opt-in.
    pub(crate) fn start_ssh_tab(
        &mut self,
        mut conn: oryxis_core::models::Connection,
        origin: crate::state::ProgressOrigin,
    ) -> Task<Message> {
        let is_quick = matches!(origin, crate::state::ProgressOrigin::Quick(_));
        // SSM Session transport short-circuits the SSH
        // pipeline entirely, it goes through
        // `session-manager-plugin` instead of opening a
        // TCP+SSH connection. Punt to the dedicated
        // dispatch handler before we waste time setting up
        // the SSH-specific state below.
        if let Some(cref) = conn.cloud_ref.as_ref()
            && cref.transport_pref == TransportKind::Ssm
        {
            return self.start_ssm_session_for_connection(&conn);
        }
        // Every non-SSH protocol branches to its own (much thinner)
        // connect path: no SSH engine, no host keys, no jump chains.
        match conn.protocol {
            // Raw shares the Telnet path: same TCP dial, same tab
            // wiring, with the option layer switched off in the engine
            // config.
            oryxis_core::models::connection::ConnectionProtocol::Telnet
            | oryxis_core::models::connection::ConnectionProtocol::Raw => {
                return self.start_telnet_tab(conn, origin);
            }
            oryxis_core::models::connection::ConnectionProtocol::Serial => {
                return self.start_serial_tab(conn, origin);
            }
            oryxis_core::models::connection::ConnectionProtocol::Local => {
                return self.start_local_tab(conn, origin);
            }
            oryxis_core::models::connection::ConnectionProtocol::RemoteDesktop => {
                // Not a terminal: launch the OS-native desktop client
                // (tunnelling through the gateway SSH host if set).
                return self.launch_remote_desktop(conn);
            }
            oryxis_core::models::connection::ConnectionProtocol::Ssh => {}
        }
        // Resolve the connect plan: proxy, credentials, agent pin,
        // TOTP, quick-connect overlays, jump resolver.
        let plan = self.resolve_connect_plan(&mut conn, origin);

        // Advisory expired-certificate toast (B2). The engine still offers
        // the cert (the server clock is authoritative), but flag it here at
        // connect time so the user knows why a rejection might follow. The
        // clock check is the app's, deliberately: this is a hint, not a gate.
        if let Some(cert) = plan.certificate.as_deref()
            && certificate_is_expired(cert)
        {
            let _ = self.show_toast_secs(crate::i18n::t("cert_expired_warn").to_string(), 6);
        }

        let ConnectPlan {
            password,
            private_key,
            certificate,
            pinned_agent,
            totp_secret,
            resolver,
        } = plan;

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
                // from this tab can reconnect this pane. Quick hosts get
                // their own origin so vault-backed features opt in
                // deliberately instead of chasing a dangling id.
                new_tab.active_mut().origin = match origin {
                    crate::state::ProgressOrigin::Saved(_) => {
                        crate::state::PaneOrigin::Host(conn.id)
                    }
                    crate::state::ProgressOrigin::Quick(id) => {
                        crate::state::PaneOrigin::QuickHost(id)
                    }
                };
                // An SFTP console dials exactly like a shell and only
                // parts ways in `SshConnected`, so the intent has to
                // ride the PANE from here (issue #188). Taken rather
                // than read: a flag that outlived its request would turn
                // the next ordinary tab into a console.
                if std::mem::take(&mut self.pending_console_purpose) {
                    new_tab.active_mut().purpose = crate::state::PanePurpose::SftpConsole;
                }
                // Auto-open the terminal sidebar, decided once at tab
                // birth: per-host override wins, the global setting is
                // the default (quick hosts have no override). Toggling
                // afterwards stays entirely with the user. With two
                // regions (issue #102) the auto-open lands on the
                // region of the configured default tab; without one it
                // keeps the historical right, falling back to left only
                // when every tab is docked there.
                let auto_open = match origin {
                    crate::state::ProgressOrigin::Saved(_) => conn
                        .sidebar_auto_open
                        .unwrap_or(self.prefs.sidebar_auto_open),
                    crate::state::ProgressOrigin::Quick(_) => {
                        self.prefs.sidebar_auto_open
                    }
                };
                if auto_open
                    && let Some(side) = self.sidebar_auto_open_side()
                {
                    new_tab.sidebar_open[side.idx()] = true;
                }
                // C5: resolve this host's terminal quirks once, on the hot
                // key path + widget read from `pane.quirks` thereafter. The
                // OSC 52 override lives deep in the emulator event handler,
                // so it's pushed into the terminal state here.
                let resolved_quirks = self.resolve_quirks(&conn);
                new_tab.active_mut().quirks = resolved_quirks;
                if let Ok(term) = new_tab.active().terminal.lock() {
                    let (w, r) =
                        resolved_quirks.osc52.map(|o| o.overrides()).unwrap_or((None, None));
                    term.set_osc52_override(w, r);
                }
                if let crate::state::ProgressOrigin::Quick(id) = origin
                    && let Some(entry) = self.quick_connects.get(&id)
                {
                    // Relaunch message so Duplicate Tab can recreate this
                    // ad-hoc session. "Duplicate in New Window" auto-hides
                    // on it (a child process cannot resolve an unsaved id).
                    new_tab.relaunch =
                        Some(Box::new(Message::Ssh(SshMessage::QuickConnect(Box::new(entry.clone())))));
                }
                // Stable id of this tab's pane: PTY output and
                // session events route to it, so the right pane
                // gets the bytes even after the tab is split.
                let pane_id = new_tab.active().id;
                self.tabs.push(new_tab);

                // Show progress view instead of terminal
                self.connecting = Some(ConnectionProgress {
                    label: label.clone(),
                    hostname: hostname.clone(),
                    step: ConnectionStep::Starting,
                    logs: vec![(
                        ConnectionStep::Starting,
                        format!(
                            "Starting a new connection to \"{}\" port {}",
                            conn.hostname, conn.port
                        ),
                    )],
                    failed: false,
                    origin,
                    tab_idx,
                    pane_id,
                    banner: None,
                });
                self.active_tab = Some(tab_idx);
                self.remember_terminal_tab_focus(tab_idx);

                // Connection reuse (F2), the tab path. "Duplicate tab"
                // and a second open of the same host both land here, so
                // this is where reuse actually pays: the pane exists,
                // the progress card is up, and the live connection can
                // carry the new session without another handshake.
                //
                // The card is cleared right away because none of the
                // stages it shows (dial, kex, auth, jump hops) are
                // about to happen; leaving it up would narrate work
                // that is not being done.
                let reuse_origin = match origin {
                    crate::state::ProgressOrigin::Saved(_) => Some(conn.id),
                    crate::state::ProgressOrigin::Quick(id) => Some(id),
                };
                if let Some(o) = reuse_origin {
                    let key = crate::ssh_reuse::ReuseKey::new(o, &conn);
                    // Minted at dial time and parked for `SshConnected`
                    // to register with: recomputing at registration
                    // would key an edited row's OLD transport under its
                    // NEW resolved key.
                    self.pending_reuse_keys.insert(pane_id, key.clone());
                    if let Some(transport) = self.pooled_transport(&key) {
                        self.connecting = None;
                        self.active_view = crate::state::View::Terminal;
                        return self.spawn_reused_session(transport, conn, tab_idx, pane_id);
                    }
                }

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

                // Same ask/answer pair for command-proxy approval. The
                // user drove this connect, so an unapproved line may
                // raise the prompt (`ProxyConsentMode::Ask`).
                let (pc_ask_tx, mut pc_ask_rx) = tokio::sync::mpsc::channel::<(
                    oryxis_ssh::ProxyCommandQuery,
                    tokio::sync::oneshot::Sender<bool>,
                )>(1);
                let (pc_resp_tx, mut pc_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
                self.proxy_command_response_tx = Some(pc_resp_tx);

                // Same ask/answer pair for keyboard-interactive
                // (2FA / OTP) prompts when auth method is Interactive.
                let (kbi_ask_tx, mut kbi_ask_rx) = tokio::sync::mpsc::channel::<(oryxis_ssh::KbiQuery, tokio::sync::oneshot::Sender<Option<Vec<String>>>)>(1);
                let (kbi_resp_tx, mut kbi_resp_rx) = tokio::sync::mpsc::channel::<Option<Vec<String>>>(1);
                // Pre-auth banner sink (one-way): the engine forwards RFC
                // 4252 banners here; a bridge below surfaces them on the
                // progress card + the tab's terminal.
                let (banner_tx, mut banner_rx) =
                    tokio::sync::mpsc::unbounded_channel::<String>();
                self.kbi_response_tx = Some(kbi_resp_tx);

                let conn_host = conn.hostname.clone();
                let conn_port = conn.port;
                // What the resolver never says. A host field carrying
                // more than a host fails as a plain DNS error naming
                // the symptom ("os error 11003"), so the one thing the
                // user has to change goes unsaid (issue #171). Resolved
                // HERE, on the UI thread, so the string follows the
                // language the session is in rather than whatever it
                // becomes by the time the dial gives up.
                let host_hint = host_field_hint(&conn_host);
                // Same idea one layer down: a host whose disk key is
                // there but unusable authenticates with nothing, and the
                // engine can only report the absence ("No private key
                // selected") because the file never became key material.
                // Resolved here for the same reason as `host_hint`, and
                // only when the vault supplied no key of its own.
                let disk_hint = (private_key.is_none()).then(|| disk_key_hint(&conn)).flatten();
                let username = conn.username.clone()
                    .or_else(|| {
                        conn.identity_id.and_then(|iid| {
                            self.identities.iter().find(|i| i.id == iid)
                                .and_then(|i| i.username.clone())
                        })
                    })
                    .unwrap_or_else(|| "root".into());
                // Human wording for the "Authenticating as ... using ..."
                // log line ({:?} printed enum variant names like
                // "PasswordPrompt").
                let auth_method_label = match conn.auth_method {
                    oryxis_core::models::connection::AuthMethod::Auto => "auto-detect",
                    oryxis_core::models::connection::AuthMethod::Password => "password",
                    oryxis_core::models::connection::AuthMethod::PasswordPrompt => {
                        "prompted password"
                    }
                    oryxis_core::models::connection::AuthMethod::Key => "public key",
                    oryxis_core::models::connection::AuthMethod::Agent => "SSH agent",
                    oryxis_core::models::connection::AuthMethod::Interactive => {
                        "keyboard-interactive"
                    }
                    oryxis_core::models::connection::AuthMethod::Certificate => "certificate",
                }
                .to_string();
                let keepalive = self.effective_keepalive(&conn);
                let address_family = conn.address_family;
                let rekey_limit_mb = conn.rekey_limit_mb;
                let agent_forwarding = conn.agent_forwarding;
                let x11_forwarding = conn.x11_forwarding;
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

                // Captured for the map closure below, since `conn`
                // itself is moved into the stream producer.
                let map_conn_id = conn.id;
                // Quick-connect prompts carry their entry id so the KBI
                // modal can offer the saved identity / key selector.
                let quick_origin = match origin {
                    crate::state::ProgressOrigin::Quick(id) => Some(id),
                    crate::state::ProgressOrigin::Saved(_) => None,
                };
                // Retry action for the legacy-algorithm dialog: re-dispatch
                // the originating connect. Quick retries carry the stored
                // entry; the handler reuses it by id, so in-place mutations
                // (expanded algorithms) survive the round trip.
                let retry_msg = match origin {
                    crate::state::ProgressOrigin::Saved(id) => {
                        Message::Ssh(SshMessage::ConnectSavedHost(id))
                    }
                    crate::state::ProgressOrigin::Quick(id) => {
                        Message::Ssh(SshMessage::QuickConnect(Box::new(
                            self.quick_connects.get(&id).cloned().unwrap_or_else(
                                || crate::state::QuickConnectEntry::bare(conn.clone()),
                            ),
                        )))
                    }
                };
                let stream = iced::stream::channel::<SshStreamMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<SshStreamMsg>| {
                    async move {
                        let engine = SshEngine::new()
                            .with_host_key_check(host_key_check)
                            .with_host_key_ask(hk_ask_tx)
                            .with_proxy_command_ask(pc_ask_tx)
                            .with_kbi_ask(kbi_ask_tx)
                            .with_totp_secret(totp_secret.as_deref())
                            .with_password_prompt_labels(
                                crate::i18n::t("auth_password_prompt_title").to_string(),
                                crate::i18n::t("password").to_string(),
                            )
                            .with_keepalive(keepalive)
                            .with_address_family(address_family)
                            .with_rekey_limit_mb(rekey_limit_mb)
                            .with_agent_forwarding(agent_forwarding)
                            .with_x11_forwarding(x11_forwarding)
                            .with_env_vars(env_vars)
                            .with_encoding(encoding)
                            .with_terminal_type(terminal_type)
                            .with_algorithm_overrides(algo_ciphers, algo_kex, algo_macs, algo_host_keys)
                            .with_banner_sink(banner_tx)
                            .with_pinned_agent_key(pinned_agent.as_deref())
                        .with_auto_interactive_fallback(is_quick);

                        // One-way banner bridge (no response leg): pre-auth
                        // banners surface on the progress card + terminal.
                        let mut banner_sender = sender.clone();
                        let _banner_bridge = tokio::spawn(async move {
                            while let Some(text) = banner_rx.recv().await {
                                let _ = banner_sender.send(SshStreamMsg::Banner(text)).await;
                            }
                        });

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

                        // Same bridge for command-proxy approval. A
                        // dropped response channel resolves to `false`,
                        // i.e. the dial stops before spawning anything:
                        // the absence of an answer is never consent.
                        let mut pc_sender_clone = sender.clone();
                        let _pc_bridge = tokio::spawn(async move {
                            while let Some((query, resp_tx)) = pc_ask_rx.recv().await {
                                let _ = pc_sender_clone
                                    .send(SshStreamMsg::ProxyCommandVerify(query))
                                    .await;
                                let approved = pc_resp_rx.recv().await.unwrap_or(false);
                                let _ = resp_tx.send(approved);
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

                        // Route context up front: the dial happens inside
                        // `establish_transport`, so jump chains and proxies
                        // are announced here or never.
                        if !conn.jump_chain.is_empty() {
                            let n = conn.jump_chain.len();
                            let _ = sender.send(SshStreamMsg::Progress(
                                ConnectionStep::Connecting,
                                format!(
                                    "Routing through {} jump host{}",
                                    n,
                                    if n == 1 { "" } else { "s" }
                                ),
                            )).await;
                        }
                        if let Some(ref proxy) = conn.proxy {
                            // A command proxy's line is user-authored and can
                            // embed credentials, so only its type is logged.
                            let via = match proxy.proxy_type {
                                oryxis_core::models::connection::ProxyType::Socks5 =>
                                    format!("Using SOCKS5 proxy {}:{}", proxy.host, proxy.port),
                                oryxis_core::models::connection::ProxyType::Socks4 =>
                                    format!("Using SOCKS4 proxy {}:{}", proxy.host, proxy.port),
                                oryxis_core::models::connection::ProxyType::Http =>
                                    format!("Using HTTP proxy {}:{}", proxy.host, proxy.port),
                                oryxis_core::models::connection::ProxyType::Command(_) =>
                                    "Using command proxy".to_string(),
                            };
                            let _ = sender.send(SshStreamMsg::Progress(
                                ConnectionStep::Connecting,
                                via,
                            )).await;
                        }

                        // Step 1: TCP connection + SSH handshake + host key verification
                        let _ = sender.send(SshStreamMsg::Progress(
                            ConnectionStep::Connecting,
                            format!(
                                "Resolving address and connecting to \"{}\" port {}...",
                                conn_host, conn_port
                            ),
                        )).await;

                        let mut handle = match engine.establish_transport(&conn, resolver.as_ref()).await {
                            Ok(h) => {
                                let _ = sender.send(SshStreamMsg::Progress(
                                    ConnectionStep::Handshake,
                                    "Connection established, SSH handshake complete and host key verified".to_string(),
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
                                    // The engine error repeats the layers this
                                    // line already states ("Connection failed:"
                                    // + "host:port:"); strip them so the log
                                    // reads as one sentence, not three nested
                                    // prefixes.
                                    let raw = e.to_string();
                                    let mut root = raw.as_str();
                                    if let Some(s) = root.strip_prefix("Connection failed: ") {
                                        root = s;
                                    }
                                    let addr_prefix = format!("{}:{}: ", conn_host, conn_port);
                                    if let Some(s) = root.strip_prefix(&addr_prefix) {
                                        root = s;
                                    }
                                    let mut msg = format!("Connection to \"{}\" port {} failed: {}", conn_host, conn_port, root);
                                    // Additive, never a pre-flight block:
                                    // the row is already saved and the
                                    // dial already ran, so this reaches
                                    // the hosts poisoned before the
                                    // editor learned to split, without
                                    // any hostname we failed to foresee
                                    // becoming unconnectable.
                                    if let Some(hint) = &host_hint {
                                        msg.push('\n');
                                        msg.push_str(hint);
                                    }
                                    let _ = sender.send(SshStreamMsg::Error(msg)).await;
                                }
                                return;
                            }
                        };

                        // Step 2: Authentication
                        let _ = sender.send(SshStreamMsg::Progress(
                            ConnectionStep::Authenticating,
                            format!("Authenticating as \"{}\" using {}...", username, auth_method_label),
                        )).await;

                        let auth_material = private_key
                            .as_deref()
                            .map(|pem| oryxis_ssh::KeyMaterial::new(pem, certificate.as_deref()));
                        if let Err(e) = engine.do_authenticate(&mut handle, &conn, password.as_deref(), auth_material).await {
                            let mut msg = format!("Authentication failed for \"{}\": {}", username, e);
                            // Additive, like the host-field hint above:
                            // the engine names what was missing, this
                            // names the file that should have supplied
                            // it and why it could not.
                            if let Some(hint) = &disk_hint {
                                msg.push('\n');
                                msg.push_str(hint);
                            }
                            let _ = sender.send(SshStreamMsg::Error(msg)).await;
                            return;
                        }

                        let _ = sender.send(SshStreamMsg::Progress(
                            ConnectionStep::Authenticated,
                            format!("Authenticated as \"{}\"", username),
                        )).await;

                        // Step 3: Open PTY session (+ port forwards)
                        if !conn.port_forwards.is_empty() {
                            let fwd_summary: Vec<String> = conn.port_forwards.iter()
                                .map(|pf| format!("{}:{}:{}", pf.local_port, pf.remote_host, pf.remote_port))
                                .collect();
                            let _ = sender.send(SshStreamMsg::Progress(
                                ConnectionStep::OpeningSession,
                                format!("Port forwards: {}", fwd_summary.join(", ")),
                            )).await;
                        }
                        let _ = sender.send(SshStreamMsg::Progress(
                            ConnectionStep::OpeningSession,
                            "Opening terminal session and requesting a PTY...".to_string(),
                        )).await;
                        match engine.open_session(handle, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS, &conn.port_forwards).await {
                            Ok((session, mut rx)) => {
                                // Terminfo fallback (issue #88): the probe
                                // found the configured TERM missing on the
                                // host; log what was actually requested so
                                // the timeline explains the differing TERM.
                                if let Some(fb) = session.term_fallback() {
                                    let line = match fb.used.as_deref() {
                                        Some(used) => format!(
                                            "Host has no terminfo entry for \"{}\"; using \"{}\" for this session",
                                            fb.requested, used
                                        ),
                                        None => format!(
                                            "Host has no terminfo entry for \"{}\" and no fallback was found; full-screen apps may misbehave",
                                            fb.requested
                                        ),
                                    };
                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::OpeningSession,
                                        line,
                                    )).await;
                                }
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
                                    format!("Terminal session setup failed: {}", e),
                                )).await;
                            }
                        }
                    }
                });

                return Task::batch(vec![
                    self.tab_scroll_to_active(),
                    Task::stream(stream).map(move |msg| match msg {
                        SshStreamMsg::Progress(step, log) => {
                            Message::Ssh(SshMessage::SshProgress(pane_id, step, log))
                        }
                        SshStreamMsg::Connected(session) => Message::Ssh(SshMessage::SshConnected(
                            pane_id,
                            crate::state::TerminalTransport::Ssh(session),
                        )),
                        SshStreamMsg::HostKeyVerify(query) => {
                            Message::Ssh(SshMessage::SshHostKeyVerify(query))
                        }
                        SshStreamMsg::ProxyCommandVerify(query) => {
                            Message::Ssh(SshMessage::SshProxyCommandVerify(
                                Box::new(query),
                                crate::state::ProxyConsentMode::Ask,
                            ))
                        }
                        SshStreamMsg::KbiPrompt(query) => {
                            Message::Ssh(SshMessage::SshKbiPrompt(quick_origin, query))
                        }
                        SshStreamMsg::Data(data) => {
                            Message::Terminal(TerminalMessage::PtyOutput(pane_id, data))
                        }
                        SshStreamMsg::Banner(text) => Message::Ssh(SshMessage::SshBanner(pane_id, text)),
                        SshStreamMsg::Error(err) => Message::Ssh(SshMessage::SshError(pane_id, err)),
                        SshStreamMsg::NoCommonAlgo { category, server_offers } => {
                            Message::Ssh(SshMessage::SshNoCommonAlgo {
                                conn_id: map_conn_id,
                                category,
                                server_offers,
                                retry: Box::new(retry_msg.clone()),
                            })
                        }
                        SshStreamMsg::Disconnected => {
                            Message::Ssh(SshMessage::SshDisconnected(pane_id))
                        }
                    }),
                ]);
            }
            Err(e) => {
                tracing::error!("Failed to create terminal state: {}", e);
            }
        }
        Task::none()
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
    ///
    /// `pick` is the resolved shell (program / args / label), exactly as
    /// `spawn_local_shell` takes it; `None` means the OS default shell.
    /// Splitting used to hard-code `None`, so a user with a curated list
    /// (or an "always open X" default) still got the OS default in the new
    /// pane, which on Windows is Command Prompt (issue #108). The choice is
    /// resolved by `open_local_shell_resolved` before we get here, so both
    /// entry points honour the same setting.
    pub(crate) fn local_shell_into_pane(
        &mut self,
        tab_idx: usize,
        target: iced::widget::pane_grid::Pane,
        axis: iced::widget::pane_grid::Axis,
        pick: Option<(String, Vec<String>, String)>,
    ) -> Task<Message> {
        // Inherit the cwd of the pane we are splitting when it is itself a
        // local shell, the same rule `spawn_local_shell` applies to tabs (a
        // remote SSH cwd would not exist locally).
        let inherit_cwd = self
            .tabs
            .get(tab_idx)
            .and_then(|t| t.pane_grid.get(target))
            .filter(|p| matches!(p.origin, crate::state::PaneOrigin::Local(_)))
            .and_then(|p| p.cwd.clone());
        let result = match &pick {
            Some((program, args, _)) => TerminalState::new_with_command(
                DEFAULT_TERM_COLS as u16,
                DEFAULT_TERM_ROWS as u16,
                program,
                args,
                inherit_cwd.as_deref(),
            ),
            None => TerminalState::new(
                DEFAULT_TERM_COLS as u16,
                DEFAULT_TERM_ROWS as u16,
                inherit_cwd.as_deref(),
            ),
        };
        let Ok((mut state, rx)) = result else {
            tracing::error!(
                "Failed to spawn local shell into split pane: program={:?}",
                pick.as_ref().map(|(p, _, _)| p)
            );
            return Task::none();
        };
        state.set_palette(self.terminal_palette.clone());
        let exited = state.pty.as_mut().and_then(|p| p.take_child_exit());
        let terminal = Arc::new(Mutex::new(state));
        let label = pick
            .as_ref()
            .map(|(_, _, l)| l.clone())
            .unwrap_or_else(|| crate::i18n::t("local_shell").to_string());
        // Capture the exact shell so a saved session group restores it.
        // No pick = default OS shell (empty program).
        let origin = crate::state::PaneOrigin::Local(crate::state::LocalShellSpec {
            label: label.clone(),
            program: pick.as_ref().map(|(p, _, _)| p.clone()).unwrap_or_default(),
            args: pick.as_ref().map(|(_, a, _)| a.clone()).unwrap_or_default(),
        });
        let Some(pane_id) = self.make_split_pane(tab_idx, target, axis, label, terminal, origin)
        else {
            return Task::none();
        };
        self.active_tab = Some(tab_idx);
        // This spawn lives in the SSH module but is a LOCAL shell, which
        // is exactly how it got missed once (issue #208): it is the pane
        // a split fills from the picker, so it is also the one most
        // likely to sit next to a live sibling when its shell exits.
        self.local_pane_stream(pane_id, exited, rx)
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
        match origin {
            crate::state::PaneOrigin::Host(id) => {
                self.connections.iter().find(|c| c.id == *id)
            }
            crate::state::PaneOrigin::QuickHost(id) => {
                self.quick_connects.get(id).map(|e| &e.conn)
            }
            _ => None,
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
            return self.update(Message::Ssh(SshMessage::ConnectSsh(conn_idx)));
        }

        // Display-only terminal, fed by the SSH stream (same as a normal SSH
        // tab). Seed a "Connecting…" line for immediate feedback.
        let Ok(mut term) =
            TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16)
        else {
            return Task::none();
        };
        term.set_palette(self.resolve_terminal_palette_for_connection(&conn));
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

    /// Connect a quick-connect entry into a new split pane, mirroring
    /// `connect_ssh_into_pane` for the ad-hoc store (no cloud-transport
    /// fallback: ephemeral hosts never carry a `cloud_ref`).
    pub(crate) fn quick_connect_into_pane(
        &mut self,
        entry_id: Uuid,
        tab_idx: usize,
        target: iced::widget::pane_grid::Pane,
        axis: iced::widget::pane_grid::Axis,
    ) -> Task<Message> {
        let Some(conn) = self.quick_connects.get(&entry_id).map(|e| e.conn.clone()) else {
            return Task::none();
        };
        let Ok(mut term) =
            TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16)
        else {
            return Task::none();
        };
        term.set_palette(self.resolve_terminal_palette_for_connection(&conn));
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
            crate::state::PaneOrigin::QuickHost(entry_id),
        ) else {
            return Task::none();
        };
        self.active_tab = Some(tab_idx);
        self.spawn_ssh_for_pane_quick(entry_id, tab_idx, pane_id)
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
        let Some(conn) = self.connections.get(conn_idx).cloned() else {
            return Task::none();
        };
        self.spawn_ssh_for_pane_conn(conn, None, tab_idx, pane_id)
    }

    /// `spawn_ssh_for_pane` for a quick-connect entry (split-pane fill and
    /// in-place reconnect of ad-hoc tabs).
    pub(crate) fn spawn_ssh_for_pane_quick(
        &mut self,
        entry_id: Uuid,
        tab_idx: usize,
        pane_id: Uuid,
    ) -> Task<Message> {
        let Some(conn) = self.quick_connects.get(&entry_id).map(|e| e.conn.clone()) else {
            return Task::none();
        };
        self.spawn_ssh_for_pane_conn(conn, Some(entry_id), tab_idx, pane_id)
    }

    /// Shared body for the two wrappers above. `quick_id` marks an ad-hoc
    /// host: the typed credentials overlay the (missed) vault hydration and
    /// the engine opts into the interactive Auto auth fallback.
    /// Resolve the effective terminal quirks for a connection (C5). Today
    /// this is just the host's own `quirks` (or `DEFAULT_QUIRKS` when
    /// unset); this is the SINGLE resolution point, so the v1.0
    /// group-settings-inheritance item (d4) can extend it to walk the
    /// host -> parent-group chain -> app default without touching any
    /// call site.
    pub(crate) fn resolve_quirks(
        &self,
        conn: &oryxis_core::models::Connection,
    ) -> oryxis_core::models::terminal_quirks::TerminalQuirks {
        conn.quirks
            .unwrap_or(oryxis_core::models::terminal_quirks::DEFAULT_QUIRKS)
    }

    /// Collapse the effective proxy and the group's inherited settings
    /// (D4) onto the working copy the engine reads.
    ///
    /// ONE function for EVERY dial site, headless ones included: the tab
    /// connect, `spawn_ssh_for_pane_conn` (split panes, quick-connect,
    /// in-place reconnect), the SFTP mount / SFTP-sync / vault-backup
    /// sessions, port forwards, the monitor dashboard, the remote
    /// desktop gateway, and every jump HOP via `make_jump_resolver`.
    /// Applying it in only some of them is exactly the bug that shipped
    /// twice: a host inheriting its user connected as "root" because
    /// the path at hand resolved the proxy and nothing else. A new dial
    /// site's first line after cloning the row is a call to this.
    ///
    /// It writes to the COPY, never the vault row, so the editor still
    /// shows an inherited field as unset and clearing an override still
    /// means "go back to inheriting".
    pub(crate) fn apply_group_inheritance(&self, conn: &mut oryxis_core::models::Connection) {
        let Some(vault) = self.vault.as_ref() else {
            return;
        };
        // The collapse itself lives in the vault (`apply_effective`) so
        // the MCP server's dial resolves identically: `resolve_effective`
        // layers over `resolve_proxy` (identity over inline, dangling id
        // -> None with a warning), an inherited identity's username
        // fills an empty field, and a resolution failure falls back to
        // the host's own proxy, the pre-D4 behaviour.
        vault.apply_effective(conn, &self.groups, &self.identities);
    }

    /// Open a pane on a connection that is already up (F2 reuse).
    ///
    /// The engine is rebuilt for this host because the CHANNEL still
    /// carries per-host settings (terminal type, env vars, agent and
    /// X11 forwarding); everything negotiated per CONNECTION belongs to
    /// the transport and is whatever the original dial agreed.
    ///
    /// A failure here is not shown to the user: it means the pooled
    /// connection turned out to be unusable (half-dead, or the server
    /// is at its channel cap), and the answer is the ordinary dial, not
    /// an error. The `ReuseFailedDialFresh` handler drops the pool entry
    /// (by the dial-time key) BEFORE re-entering `spawn_ssh_for_pane_conn`,
    /// so the retry cannot pick the same dead connection and loop.
    fn spawn_reused_session(
        &mut self,
        transport: std::sync::Arc<oryxis_ssh::SshTransport>,
        conn: oryxis_core::models::Connection,
        tab_idx: usize,
        pane_id: Uuid,
    ) -> Task<Message> {
        let (cols, rows) = self
            .tabs
            .get(tab_idx)
            .and_then(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
            .and_then(|p| p.terminal.lock().ok().map(|t| (t.cols(), t.rows())))
            .unwrap_or((DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16));
        let env_vars: Vec<(String, String)> = conn
            .env_vars
            .iter()
            .filter(|e| !e.key.trim().is_empty())
            .map(|e| (e.key.clone(), e.value.clone()))
            .collect();
        let engine = oryxis_ssh::SshEngine::new()
            .with_agent_forwarding(conn.agent_forwarding)
            .with_x11_forwarding(conn.x11_forwarding)
            .with_env_vars(env_vars)
            .with_encoding(conn.encoding.clone())
            .with_terminal_type(conn.terminal_type.clone());
        tracing::info!(host = %conn.hostname, "reusing the live connection for a new session");
        // Same stream shape the dial uses, minus the dial: Connected,
        // then the bytes, then Disconnected. Reusing the shape is what
        // keeps the pane wiring (logging, cwd tracking, the terminal
        // feed) identical whether the session was dialled or reused.
        let stream = iced::stream::channel::<PaneConnMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<PaneConnMsg>| async move {
            match engine.open_session_on(transport, cols as u32, rows as u32).await {
                Ok((session, mut rx)) => {
                    let session = Arc::new(session);
                    let _ = sender.send(PaneConnMsg::Connected(session)).await;
                    while let Some(data) = rx.recv().await {
                        if sender.send(PaneConnMsg::Data(data)).await.is_err() {
                            break;
                        }
                    }
                    let _ = sender.send(PaneConnMsg::Disconnected).await;
                }
                // The pooled connection turned out to be unusable. Not
                // the user's problem: `Error` here routes to the retry
                // below, which dials for real.
                Err(e) => {
                    let _ = sender.send(PaneConnMsg::Error(e.to_string())).await;
                }
            }
        });
        Task::stream(stream).map(move |m| match m {
            PaneConnMsg::Connected(s) => Message::Ssh(SshMessage::SshConnected(
                pane_id,
                crate::state::TerminalTransport::Ssh(s),
            )),
            PaneConnMsg::Data(d) => Message::Terminal(TerminalMessage::PtyOutput(pane_id, d)),
            PaneConnMsg::Disconnected => Message::Ssh(SshMessage::SshDisconnected(pane_id)),
            // Reuse failed before the session existed: fall back to a
            // real dial. The handler drops the pool entry FIRST (by the
            // key this reuse was minted with) and recomputes the tab
            // index, so the retry cannot pick the same dead connection
            // and loop, nor index a tab that moved.
            PaneConnMsg::Error(reason) => {
                tracing::info!(%reason, "connection reuse failed, dialling fresh");
                Message::Ssh(SshMessage::ReuseFailedDialFresh(pane_id))
            }
            // A reused session never negotiates and never dials, so
            // none of these can happen: the transport (and with it any
            // proxy the original dial went through) is already up.
            PaneConnMsg::HostKey(_)
            | PaneConnMsg::ProxyCommand(_)
            | PaneConnMsg::Kbi(_)
            | PaneConnMsg::Banner(_) => Message::NoOp,
        })
    }

    pub(crate) fn spawn_ssh_for_pane_conn(
        &mut self,
        mut conn: oryxis_core::models::Connection,
        quick_id: Option<Uuid>,
        tab_idx: usize,
        pane_id: Uuid,
    ) -> Task<Message> {
        // C5: resolve quirks for this pane's host before the protocol
        // split, so SSH / Telnet / Serial panes (split, quick-connect, and
        // in-place reconnect) all read the right modes. RemoteDesktop is
        // not a terminal pane, so it doesn't matter for the early return.
        let quirks = self.resolve_quirks(&conn);
        if let Some(pane) = self
            .tabs
            .get_mut(tab_idx)
            .and_then(|t| t.pane_by_id_mut(pane_id))
        {
            pane.quirks = quirks;
            if let Ok(term) = pane.terminal.lock() {
                { let (w, r) = quirks.osc52.map(|o| o.overrides()).unwrap_or((None, None)); term.set_osc52_override(w, r); };
            }
        }
        // Telnet / Serial hosts take their own thin connect paths (no
        // SSH engine); split panes and in-place reconnects included.
        match conn.protocol {
            oryxis_core::models::connection::ConnectionProtocol::Telnet
            | oryxis_core::models::connection::ConnectionProtocol::Raw => {
                return self.spawn_telnet_for_pane_conn(conn, quick_id, tab_idx, pane_id);
            }
            oryxis_core::models::connection::ConnectionProtocol::Serial => {
                return self.spawn_serial_for_pane_conn(conn, tab_idx, pane_id);
            }
            oryxis_core::models::connection::ConnectionProtocol::Local => {
                return self.spawn_local_for_pane_conn(conn, tab_idx, pane_id);
            }
            oryxis_core::models::connection::ConnectionProtocol::RemoteDesktop => {
                // A remote desktop can't live in a split pane; just launch
                // the external client (the pane keeps its current content).
                return self.launch_remote_desktop(conn);
            }
            oryxis_core::models::connection::ConnectionProtocol::Ssh => {}
        }
        self.apply_group_inheritance(&mut conn);
        // Nested hop routes (issue #184) expand onto the working copy
        // BEFORE the reuse key is minted: the key hashes the route
        // actually dialed, so an edit to a hop's own chain re-keys
        // instead of riding a pooled transport built over the old
        // route (the full-tab path expands inside its connect plan,
        // ahead of its own key mint, for the same reason).
        self.expand_jump_chain(&mut conn);

        // Connection reuse (F2): a tab to a host that is already open
        // rides the live connection instead of paying for a handshake,
        // a key exchange, an authentication and, on a jump chain, all
        // of that per hop. Tried BEFORE any credential is resolved,
        // because a reused connection needs none of them.
        //
        // The key is built from the resolved connection, so a host that
        // inherits its user from a group keys on what it actually
        // authenticates as.
        let reuse_origin = match quick_id {
            Some(id) => Some(id),
            None => Some(conn.id),
        };
        if let Some(origin) = reuse_origin {
            let key = crate::ssh_reuse::ReuseKey::new(origin, &conn);
            // Minted at dial time and parked for `SshConnected` to
            // register with: recomputing at registration would key an
            // edited row's OLD transport under its NEW resolved key.
            self.pending_reuse_keys.insert(pane_id, key.clone());
            if let Some(transport) = self.pooled_transport(&key) {
                return self.spawn_reused_session(transport, conn, tab_idx, pane_id);
            }
        }

        let (mut password, private_key, certificate) = self.resolve_credentials(&conn);
        // Agent-auth pin (B3), same rule as the tab connect.
        let pinned_agent = self.pinned_agent_public(&conn);
        let mut totp_secret = self
            .vault
            .as_ref()
            .and_then(|v| v.get_connection_totp_secret(&conn.id).ok().flatten());
        if let Some(id) = quick_id {
            self.apply_quick_entry_secrets(id, &mut conn, &mut password, &mut totp_secret);
        }
        let is_quick = quick_id.is_some();
        let resolver = self.make_jump_resolver(&mut conn);
        let host_key_check = self.make_host_key_check();
        let keepalive = self.effective_keepalive(&conn);
        let address_family = conn.address_family;
        let rekey_limit_mb = conn.rekey_limit_mb;
        // Parity with the full-tab `ConnectSsh` path: agent forwarding, env
        // vars and a custom encoding must ride the session too, otherwise a
        // split pane (or an in-place reconnect) silently drops them.
        let agent_forwarding = conn.agent_forwarding;
        let x11_forwarding = conn.x11_forwarding;
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
            pane.start_session_log(log_id);
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

        // Command-proxy approval bridge, same staging slot and the same
        // multi-pane caveat as the two above.
        let (pc_ask_tx, mut pc_ask_rx) = tokio::sync::mpsc::channel::<(
            oryxis_ssh::ProxyCommandQuery,
            tokio::sync::oneshot::Sender<bool>,
        )>(1);
        let (pc_resp_tx, mut pc_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
        self.proxy_command_response_tx = Some(pc_resp_tx);

        // Pre-auth banner sink (one-way); a split-pane connect has no
        // progress card, so banners go straight to the pane's terminal.
        let (banner_tx, mut banner_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let stream = iced::stream::channel::<PaneConnMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<PaneConnMsg>| async move {
            let engine = SshEngine::new()
                .with_host_key_check(host_key_check)
                .with_host_key_ask(hk_ask_tx)
                .with_proxy_command_ask(pc_ask_tx)
                .with_kbi_ask(kbi_ask_tx)
                .with_totp_secret(totp_secret.as_deref())
                .with_password_prompt_labels(
                    crate::i18n::t("auth_password_prompt_title").to_string(),
                    crate::i18n::t("password").to_string(),
                )
                .with_keepalive(keepalive)
                .with_address_family(address_family)
                .with_rekey_limit_mb(rekey_limit_mb)
                .with_agent_forwarding(agent_forwarding)
                .with_x11_forwarding(x11_forwarding)
                .with_env_vars(env_vars)
                .with_encoding(encoding)
                .with_terminal_type(terminal_type)
                .with_algorithm_overrides(algo_ciphers, algo_kex, algo_macs, algo_host_keys)
                .with_banner_sink(banner_tx)
                .with_pinned_agent_key(pinned_agent.as_deref())
                .with_auto_interactive_fallback(is_quick);

            let mut sender_clone = sender.clone();
            let _bridge = tokio::spawn(async move {
                while let Some((query, resp_tx)) = hk_ask_rx.recv().await {
                    let _ = sender_clone.send(PaneConnMsg::HostKey(query)).await;
                    let accepted = hk_resp_rx.recv().await.unwrap_or(false);
                    let _ = resp_tx.send(accepted);
                }
            });

            let mut banner_sender = sender.clone();
            let _banner_bridge = tokio::spawn(async move {
                while let Some(text) = banner_rx.recv().await {
                    let _ = banner_sender.send(PaneConnMsg::Banner(text)).await;
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

            let mut pc_sender_clone = sender.clone();
            let _pc_bridge = tokio::spawn(async move {
                while let Some((query, resp_tx)) = pc_ask_rx.recv().await {
                    let _ = pc_sender_clone.send(PaneConnMsg::ProxyCommand(query)).await;
                    let approved = pc_resp_rx.recv().await.unwrap_or(false);
                    let _ = resp_tx.send(approved);
                }
            });

            match engine
                .connect_with_resolver(
                    &conn,
                    password.as_deref(),
                    private_key
                        .as_deref()
                        .map(|pem| oryxis_ssh::KeyMaterial::new(pem, certificate.as_deref())),
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
            PaneConnMsg::HostKey(q) => Message::Ssh(SshMessage::SshHostKeyVerify(q)),
            PaneConnMsg::ProxyCommand(q) => Message::Ssh(SshMessage::SshProxyCommandVerify(
                Box::new(q),
                crate::state::ProxyConsentMode::Ask,
            )),
            PaneConnMsg::Kbi(q) => Message::Ssh(SshMessage::SshKbiPrompt(quick_id, q)),
            PaneConnMsg::Banner(text) => Message::Ssh(SshMessage::SshPaneBanner(pane_id, text)),
            PaneConnMsg::Connected(s) => {
                Message::Ssh(SshMessage::SshConnected(pane_id, crate::state::TerminalTransport::Ssh(s)))
            }
            PaneConnMsg::Data(d) => Message::Terminal(TerminalMessage::PtyOutput(pane_id, d)),
            PaneConnMsg::Disconnected => Message::Ssh(SshMessage::SshDisconnected(pane_id)),
            PaneConnMsg::Error(e) => Message::Ssh(SshMessage::PaneConnectError(pane_id, e)),
        })
    }
}

/// The plain-language line a resolver failure never carries: what the
/// stored Host field holds that stopped it from being a host at all.
/// `None` for every value the dial path can legitimately resolve, so a
/// host that is merely unreachable is never told its name is wrong.
///
/// Two keys, not one per fault: the username case is the one with a
/// specific instruction (move it to the Username field), and the other
/// three share the same answer (put only a host there).
/// Why the disk key source produced nothing for this host, in the
/// user's language, or `None` when it produced a key (or was never
/// asked). Shares the host editor's wording, so the connect error and
/// the editor hint can never say different things about one file.
pub(crate) fn disk_key_hint(conn: &oryxis_core::models::Connection) -> Option<String> {
    use oryxis_vault::DiskKeyStatus as St;
    let status =
        oryxis_vault::resolve_disk_key(conn.use_disk_key, conn.identity_file.as_deref()).status();
    match status {
        // Not opted in, or it worked: the failure is about something
        // else and a line about keys would only mislead.
        St::Off | St::Ready { .. } => None,
        St::NotFound => Some(crate::i18n::t("disk_key_none").to_string()),
        St::Encrypted(path) => Some(crate::i18n::t("disk_key_locked").replace("{path}", &path)),
        St::Unreadable(path, err) | St::Unusable(path, err) => Some(
            crate::i18n::t("disk_key_problem")
                .replace("{path}", &path)
                .replace("{error}", &err),
        ),
    }
}

pub(crate) fn host_field_hint(host: &str) -> Option<String> {
    use oryxis_core::ssh_target::HostFieldFault;
    match oryxis_core::ssh_target::diagnose_host_field(host)? {
        HostFieldFault::Username => Some(crate::i18n::t("connect_host_has_user").to_string()),
        HostFieldFault::Whitespace | HostFieldFault::Scheme | HostFieldFault::Malformed => {
            Some(crate::i18n::t("connect_host_invalid").to_string())
        }
    }
}

/// Whether an attached OpenSSH certificate line is past its validity
/// window against the local clock (B2). Advisory only, drives the
/// connect-time toast; the server clock remains authoritative and the
/// engine offers the cert regardless. Unparseable certs are never
/// reported as expired; OpenSSH encodes "forever" as `u64::MAX` only,
/// so a `valid_before` of 0 is a window that ended at the epoch and
/// counts as expired.
pub(crate) fn certificate_is_expired(cert_line: &str) -> bool {
    let Ok(cert) = ssh_key::Certificate::from_openssh(cert_line.trim()) else {
        return false;
    };
    let before = cert.valid_before();
    if before == u64::MAX {
        return false;
    }
    let now = chrono::Utc::now().timestamp().max(0) as u64;
    now > before
}
