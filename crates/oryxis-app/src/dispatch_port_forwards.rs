//! `Oryxis::handle_port_forwards`, match arms for the standalone port
//! forward entity: CRUD on `PortForwardRule`, and the runtime on/off
//! toggle that opens / tears down a dedicated PTY-less SSH session.
//!
//! Kept separate from `dispatch_ssh.rs` (terminal sessions) so the two
//! lifecycles don't tangle. A forward holds its connection open with no
//! shell; turning it off drops the `ForwardSession`, which cancels the
//! tunnel.

// Domain handlers return `Err(Message)` to pass an unclaimed message
// back up the chain. See the note in `dispatch_ssh.rs`.
#![allow(clippy::result_large_err)]

use std::sync::{Arc, Mutex};

use iced::futures::SinkExt;
use iced::Task;
use uuid::Uuid;

use oryxis_core::models::connection::{AuthMethod, Connection};
use oryxis_core::models::port_forward_rule::PortForwardRule;
use oryxis_ssh::{ConnectionResolver, ForwardSession, HostKeyQuery, SshEngine};

use crate::app::{Message, Oryxis};

/// Items streamed out of an interactive (manual-toggle) forward connect:
/// either a host-key question for the UI modal, or the final result.
enum PfStreamMsg {
    HostKey(HostKeyQuery),
    Done(Result<Arc<ForwardSession>, String>),
}

impl Oryxis {
    pub(crate) fn handle_port_forwards(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Editor panel --
            Message::ShowPortForwardPanel => {
                self.show_port_forward_panel = true;
                self.pf_editing_id = None;
                self.pf_label.clear();
                self.pf_kind = oryxis_core::models::port_forward_rule::ForwardKind::Local;
                // Default the host to the first connection so the picker
                // isn't empty on a fresh rule.
                self.pf_host_id = self.connections.first().map(|c| c.id);
                self.pf_listen_host = "127.0.0.1".into();
                self.pf_listen_port.clear();
                self.pf_target_host.clear();
                self.pf_target_port.clear();
                self.pf_auto_start = false;
                self.pf_error = None;
            }
            Message::HidePortForwardPanel => {
                self.show_port_forward_panel = false;
            }
            Message::PfLabelChanged(v) => self.pf_label = v,
            Message::PfKindChanged(k) => self.pf_kind = k,
            Message::PfHostChanged(id) => self.pf_host_id = Some(id),
            Message::PfListenHostChanged(v) => self.pf_listen_host = v,
            Message::PfListenPortChanged(v) => {
                self.pf_listen_port = v.chars().filter(|c| c.is_ascii_digit()).collect();
            }
            Message::PfTargetHostChanged(v) => self.pf_target_host = v,
            Message::PfTargetPortChanged(v) => {
                self.pf_target_port = v.chars().filter(|c| c.is_ascii_digit()).collect();
            }
            Message::PfAutoStartToggled(v) => self.pf_auto_start = v,
            Message::EditPortForwardRule(idx) => {
                if let Some(rule) = self.port_forward_rules.get(idx) {
                    self.show_port_forward_panel = true;
                    self.pf_editing_id = Some(rule.id);
                    self.pf_label = rule.label.clone();
                    self.pf_kind = rule.kind;
                    self.pf_host_id = Some(rule.host_id);
                    self.pf_listen_host = rule.listen_host.clone();
                    self.pf_listen_port = rule.listen_port.to_string();
                    self.pf_target_host = rule.target_host.clone();
                    self.pf_target_port = rule.target_port.to_string();
                    self.pf_auto_start = rule.auto_start;
                    self.pf_error = None;
                }
            }
            Message::SavePortForwardRule => {
                if let Some(err) = self.save_port_forward_rule() {
                    self.pf_error = Some(err);
                } else {
                    self.show_port_forward_panel = false;
                    self.pf_error = None;
                    self.load_data_from_vault();
                }
            }
            Message::DeletePortForwardRule(idx) => {
                if let Some(rule) = self.port_forward_rules.get(idx) {
                    let id = rule.id;
                    // Tear down a live forward before the rule disappears.
                    self.active_forwards.remove(&id);
                    self.port_forward_starting.remove(&id);
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_port_forward_rule(&id);
                        self.show_port_forward_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }

            // -- Runtime on/off --
            Message::StartPortForward(id) => {
                return Ok(self.start_port_forward(id, false));
            }
            Message::StopPortForward(id) => {
                self.port_forward_starting.remove(&id);
                // Await `cancel()` so a remote (`-R`) forward also releases
                // its server-side listener via `cancel_tcpip_forward`, not
                // just the local tasks that Drop would stop. Dropping the
                // last `Arc` afterwards tears the rest down.
                if let Some(session) = self.active_forwards.remove(&id) {
                    return Ok(Task::perform(
                        async move { session.cancel().await },
                        |_| Message::PortForwardLivenessTick,
                    ));
                }
            }
            Message::PortForwardStarted(id, res) => {
                // `remove` returns false when StopPortForward already pulled
                // this id from the in-flight set, i.e. the user toggled the
                // rule off while the connect was still running. In that case
                // honor the stop and drop the freshly-made session rather than
                // silently re-activating a forward the user turned off.
                let was_starting = self.port_forward_starting.remove(&id);
                match res {
                    Ok(session) => {
                        // Guard against a delete or stop that landed while the
                        // connect was in flight: if the rule is gone, or a stop
                        // was requested, drop the session so it doesn't linger
                        // with no UI to stop (or against the user's intent).
                        if was_starting && self.port_forward_rules.iter().any(|r| r.id == id) {
                            self.active_forwards.insert(id, session);
                            self.pf_error = None;
                        } else {
                            drop(session);
                        }
                    }
                    Err(e) => {
                        // Toggle bounces back to off and the error surfaces.
                        self.pf_error = Some(e);
                    }
                }
            }
            Message::PortForwardLivenessTick => {
                // Drop forwards whose underlying connection has died so the
                // per-row toggle reflects reality instead of lying "on".
                let dead: Vec<Uuid> = self
                    .active_forwards
                    .iter()
                    .filter(|(_, s)| !s.is_alive())
                    .map(|(id, _)| *id)
                    .collect();
                for id in dead {
                    self.active_forwards.remove(&id);
                    tracing::info!("port forward {id} connection dropped, toggled off");
                }
            }
            Message::PortForwardCardHovered(idx) => {
                self.hovered_port_forward_card = Some(idx);
            }
            Message::PortForwardCardUnhovered => {
                self.hovered_port_forward_card = None;
            }
            Message::PortForwardSearchChanged(v) => self.port_forward_search = v,

            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// Validate the editor draft and persist it. Returns `Some(error)` on
    /// a validation failure (left in the panel), `None` on success.
    fn save_port_forward_rule(&mut self) -> Option<String> {
        let label = self.pf_label.trim();
        if label.is_empty() {
            return Some(crate::i18n::t("pf_err_required").to_string());
        }
        let Some(host_id) = self.pf_host_id else {
            return Some(crate::i18n::t("pf_err_host").to_string());
        };
        if !self.connections.iter().any(|c| c.id == host_id) {
            return Some(crate::i18n::t("pf_err_host").to_string());
        }
        let Some(listen_port) = parse_port(&self.pf_listen_port) else {
            return Some(crate::i18n::t("pf_err_port").to_string());
        };
        let (target_host, target_port) = if self.pf_kind.has_target() {
            let th = self.pf_target_host.trim();
            if th.is_empty() {
                return Some(crate::i18n::t("pf_err_required").to_string());
            }
            let Some(tp) = parse_port(&self.pf_target_port) else {
                return Some(crate::i18n::t("pf_err_port").to_string());
            };
            (th.to_string(), tp)
        } else {
            (String::new(), 0)
        };

        let mut rule = if let Some(id) = self.pf_editing_id {
            self.port_forward_rules
                .iter()
                .find(|r| r.id == id)
                .cloned()
                .unwrap_or_else(|| PortForwardRule::new("", self.pf_kind, host_id))
        } else {
            PortForwardRule::new("", self.pf_kind, host_id)
        };
        rule.label = label.to_string();
        rule.kind = self.pf_kind;
        rule.host_id = host_id;
        rule.listen_host = self.pf_listen_host.trim().to_string();
        rule.listen_port = listen_port;
        rule.target_host = target_host;
        rule.target_port = target_port;
        rule.auto_start = self.pf_auto_start;
        rule.updated_at = chrono::Utc::now();

        let vault = self.vault.as_ref()?;
        match vault.save_port_forward_rule(&rule) {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        }
    }

    /// Open a dedicated PTY-less SSH session for the rule and bind its
    /// listener.
    ///
    /// Host-key policy splits on `boot_auto_start`: a boot/unlock auto-start
    /// runs **known-only** (strict, silent), so a host whose key isn't
    /// already trusted just fails to off instead of popping a modal storm
    /// before the window is even ready. A manual toggle, by contrast, wires
    /// the same host-key prompt the terminal uses, so the user can trust a
    /// new key on the spot.
    pub(crate) fn start_port_forward(&mut self, id: Uuid, boot_auto_start: bool) -> Task<Message> {
        if self.active_forwards.contains_key(&id) || self.port_forward_starting.contains(&id) {
            return Task::none();
        }
        let Some(rule) = self.port_forward_rules.iter().find(|r| r.id == id).cloned() else {
            return Task::none();
        };
        let Some(mut conn) = self
            .connections
            .iter()
            .find(|c| c.id == rule.host_id)
            .cloned()
        else {
            self.pf_error = Some(crate::i18n::t("pf_err_host").to_string());
            return Task::none();
        };

        // Resolve the effective proxy onto `conn.proxy` (engine reads only
        // that field), mirroring the terminal connect path.
        if let Some(vault) = self.vault.as_ref() {
            conn.proxy = vault.resolve_proxy(&conn).ok().flatten();
        }
        let (password, private_key) = self.resolve_forward_credentials(&conn);
        let resolver = self.build_jump_resolver(&conn);
        let host_key_check = self.build_host_key_check();
        let keepalive = self.effective_keepalive(&conn);
        self.port_forward_starting.insert(id);

        if boot_auto_start {
            tracing::info!("auto-starting port forward {} ({})", rule.label, id);
            return Task::perform(
                async move {
                    let engine = SshEngine::new()
                        .with_host_key_check(host_key_check)
                        .with_strict_host_key(true)
                        .with_keepalive(keepalive);
                    engine
                        .connect_forward(
                            &conn,
                            password.as_deref(),
                            private_key.as_deref(),
                            &rule,
                            resolver.as_ref(),
                        )
                        .await
                        .map(Arc::new)
                        .map_err(|e| e.to_string())
                },
                move |res| Message::PortForwardStarted(id, res),
            );
        }

        // Manual toggle: reuse the terminal's host-key ask machinery. The
        // engine sends unknown/changed keys to `hk_ask`; the bridge forwards
        // them to the shared host-key modal and waits for the user's answer
        // on `hk_resp` (driven by the existing SshHostKey* handlers).
        let (hk_ask_tx, mut hk_ask_rx) = tokio::sync::mpsc::channel::<(
            HostKeyQuery,
            tokio::sync::oneshot::Sender<bool>,
        )>(1);
        let (hk_resp_tx, mut hk_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
        self.host_key_response_tx = Some(hk_resp_tx);

        let stream = iced::stream::channel::<PfStreamMsg>(8, move |mut sender: iced::futures::channel::mpsc::Sender<PfStreamMsg>| async move {
            let engine = SshEngine::new()
                .with_host_key_check(host_key_check)
                .with_host_key_ask(hk_ask_tx)
                .with_keepalive(keepalive);

            let mut sender_clone = sender.clone();
            let _bridge = tokio::spawn(async move {
                while let Some((query, resp_tx)) = hk_ask_rx.recv().await {
                    let _ = sender_clone.send(PfStreamMsg::HostKey(query)).await;
                    let accepted = hk_resp_rx.recv().await.unwrap_or(false);
                    let _ = resp_tx.send(accepted);
                }
            });

            let result = engine
                .connect_forward(
                    &conn,
                    password.as_deref(),
                    private_key.as_deref(),
                    &rule,
                    resolver.as_ref(),
                )
                .await
                .map(Arc::new)
                .map_err(|e| e.to_string());
            let _ = sender.send(PfStreamMsg::Done(result)).await;
        });

        Task::stream(stream).map(move |m| match m {
            PfStreamMsg::HostKey(q) => Message::SshHostKeyVerify(q),
            PfStreamMsg::Done(r) => Message::PortForwardStarted(id, r),
        })
    }

    /// Start every rule marked `auto_start`. Called once after the vault is
    /// unlocked (boot or `VaultUnlock`). Returns the connect tasks to batch
    /// into the caller's task list.
    pub(crate) fn auto_start_port_forwards(&mut self) -> Vec<Task<Message>> {
        let ids: Vec<Uuid> = self
            .port_forward_rules
            .iter()
            .filter(|r| r.auto_start)
            .map(|r| r.id)
            .collect();
        ids.into_iter()
            .map(|id| self.start_port_forward(id, true))
            .collect()
    }

    /// Resolve password + private key for a connection, preferring a linked
    /// identity over inline fields. Mirrors the terminal connect path in
    /// `dispatch_ssh.rs`.
    pub(crate) fn resolve_forward_credentials(
        &self,
        conn: &Connection,
    ) -> (Option<String>, Option<String>) {
        if let Some(iid) = conn.identity_id {
            let id_pw = self
                .vault
                .as_ref()
                .and_then(|v| v.get_identity_password(&iid).ok().flatten());
            let id_key = self
                .identities
                .iter()
                .find(|i| i.id == iid)
                .and_then(|i| i.key_id)
                .and_then(|kid| {
                    self.vault
                        .as_ref()
                        .and_then(|v| v.get_key_private(&kid).ok().flatten())
                });
            (id_pw, id_key)
        } else {
            let pw = self
                .vault
                .as_ref()
                .and_then(|v| v.get_connection_password(&conn.id).ok().flatten());
            let pk = if conn.auth_method == AuthMethod::Key || conn.auth_method == AuthMethod::Auto
            {
                conn.key_id.and_then(|kid| {
                    self.vault
                        .as_ref()
                        .and_then(|v| v.get_key_private(&kid).ok().flatten())
                })
            } else {
                None
            };
            (pw, pk)
        }
    }

    /// Build the jump-host resolver (hydrated passwords / keys / proxies)
    /// for a connection, or `None` when it has no jump chain.
    pub(crate) fn build_jump_resolver(&self, conn: &Connection) -> Option<ConnectionResolver> {
        if conn.jump_chain.is_empty() {
            return None;
        }
        let mut passwords = std::collections::HashMap::new();
        let mut keys = std::collections::HashMap::new();
        let mut proxies = std::collections::HashMap::new();
        for jid in &conn.jump_chain {
            if let Some(vault) = &self.vault
                && let Ok(Some(pw)) = vault.get_connection_password(jid)
            {
                passwords.insert(*jid, pw);
            }
            if let Some(jconn) = self.connections.iter().find(|c| c.id == *jid) {
                if let Some(kid) = jconn.key_id
                    && let Some(vault) = &self.vault
                    && let Ok(Some(pk)) = vault.get_key_private(&kid)
                {
                    keys.insert(*jid, pk);
                }
                if let Some(vault) = &self.vault
                    && let Ok(Some(p)) = vault.resolve_proxy(jconn)
                {
                    proxies.insert(*jid, p);
                }
            }
        }
        Some(ConnectionResolver {
            connections: self.connections.clone(),
            passwords,
            private_keys: keys,
            proxies,
        })
    }

    /// Build a host-key check callback over a snapshot of known hosts.
    /// Forwards run it strict (unknown / changed → reject) since there is
    /// no terminal modal to prompt; the user trusts a host by connecting a
    /// terminal to it first.
    pub(crate) fn build_host_key_check(&self) -> oryxis_ssh::HostKeyCheckCallback {
        let snapshot = Arc::new(Mutex::new(self.known_hosts.clone()));
        Arc::new(move |host: &str, port: u16, _key_type: &str, fingerprint: &str| {
            let hosts = match snapshot.lock() {
                Ok(g) => g,
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
        })
    }
}

/// Parse a 1..=65535 port from the editor's digit-filtered string.
fn parse_port(s: &str) -> Option<u16> {
    match s.trim().parse::<u16>() {
        Ok(p) if p > 0 => Some(p),
        _ => None,
    }
}
