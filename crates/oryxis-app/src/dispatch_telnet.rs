//! Telnet connect paths (full tab + split pane), the transport twin of
//! the SSH flows in `dispatch_ssh.rs`. Deliberately thin: no host keys,
//! no keyboard-interactive, no jump chains, no port forwards, Telnet
//! is a raw TCP dial plus option negotiation, and the engine crate
//! (`oryxis-telnet`) owns all of that. Both paths reuse the SSH
//! lifecycle messages (`SshConnected` / `PtyOutput` / `SshError` /
//! `SshDisconnected` / `PaneConnectError`), so everything downstream
//! (recording, reconnect, history, the tab strip) rides unchanged.

use iced::Task;
use iced::futures::SinkExt;

use std::sync::{Arc, Mutex};
use uuid::Uuid;

use oryxis_telnet::{TelnetConfig, TelnetSession};
use oryxis_terminal::widget::TerminalState;

use crate::app::{TerminalMessage, SshMessage, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS, Message, Oryxis};
use crate::state::{ConnectionProgress, ConnectionStep, TerminalTab, TerminalTransport};

impl Oryxis {
    /// Build the engine config for one Telnet dial: credentials resolved
    /// with the same identity-over-inline precedence as SSH (a synced
    /// host may carry an identity even though the reduced editor hides
    /// it), quick-connect secrets overlaid for ad-hoc hosts.
    fn telnet_config(
        &self,
        conn: &mut oryxis_core::models::Connection,
        quick_id: Option<Uuid>,
    ) -> TelnetConfig {
        let (username, mut password) = if let Some(iid) = conn.identity_id {
            let identity = self.identities.iter().find(|i| i.id == iid);
            (
                conn.username
                    .clone()
                    .or_else(|| identity.and_then(|i| i.username.clone())),
                self.vault
                    .as_ref()
                    .and_then(|v| v.get_identity_password(&iid).ok().flatten()),
            )
        } else {
            (
                conn.username.clone(),
                self.vault
                    .as_ref()
                    .and_then(|v| v.get_connection_password(&conn.id).ok().flatten()),
            )
        };
        if let Some(id) = quick_id {
            // TOTP has no Telnet counterpart; scratch var keeps the
            // shared overlay signature.
            let mut unused_totp = None;
            self.apply_quick_entry_secrets(id, conn, &mut password, &mut unused_totp);
        }
        TelnetConfig {
            host: conn.hostname.clone(),
            port: conn.port,
            username: username.filter(|u| !u.trim().is_empty()),
            password: password.filter(|p| !p.is_empty()),
            term: conn
                .terminal_type
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "xterm-256color".to_string()),
            encoding: conn.encoding.clone(),
            address_family: conn.address_family,
            ..TelnetConfig::default()
        }
    }

    /// Open a new tab connected to a Telnet host. The counterpart of
    /// `start_ssh_tab`, reached from the same entry points whenever
    /// `conn.protocol == Telnet`.
    pub(crate) fn start_telnet_tab(
        &mut self,
        mut conn: oryxis_core::models::Connection,
        origin: crate::state::ProgressOrigin,
    ) -> Task<Message> {
        let quick_id = match origin {
            crate::state::ProgressOrigin::Quick(id) => Some(id),
            crate::state::ProgressOrigin::Saved(_) => None,
        };
        let config = self.telnet_config(&mut conn, quick_id);

        let Ok(mut state) =
            TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16)
        else {
            tracing::error!("Failed to create terminal state for Telnet tab");
            return Task::none();
        };
        state.palette = self.resolve_terminal_palette_for_connection(&conn);
        let label = conn.label.clone();
        let hostname = format!("Telnet {}:{}", conn.hostname, conn.port);
        let terminal = Arc::new(Mutex::new(state));

        // Session recording rides the shared PtyOutput path, so a Telnet
        // session records exactly like an SSH one.
        let session_log_id = if self.should_record_session(Some(&conn)) {
            self.vault.as_ref().map(|vault| {
                let log_id = Uuid::new_v4();
                if let Err(e) = vault.create_session_log(&log_id, &conn.id, &conn.label) {
                    tracing::warn!("session log create failed: {e}");
                }
                self.session_logs_total += 1;
                log_id
            })
        } else {
            None
        };

        let mut new_tab = TerminalTab::new_single(label.clone(), Arc::clone(&terminal));
        new_tab.active_mut().session_log_id = session_log_id;
        new_tab.active_mut().origin = match origin {
            crate::state::ProgressOrigin::Saved(_) => crate::state::PaneOrigin::Host(conn.id),
            crate::state::ProgressOrigin::Quick(id) => crate::state::PaneOrigin::QuickHost(id),
        };
        // C5: Telnet hosts get the same per-host quirks as SSH (this is
        // exactly their audience: appliances / serial consoles).
        let resolved_quirks = self.resolve_quirks(&conn);
        new_tab.active_mut().quirks = resolved_quirks;
        if let Ok(term) = new_tab.active().terminal.lock() {
            let (w, r) = resolved_quirks.osc52.map(|o| o.overrides()).unwrap_or((None, None));
            term.set_osc52_override(w, r);
        }
        if let crate::state::ProgressOrigin::Quick(id) = origin
            && let Some(entry) = self.quick_connects.get(&id)
        {
            new_tab.relaunch = Some(Box::new(Message::Ssh(SshMessage::QuickConnect(Box::new(entry.clone())))));
        }
        let pane_id = new_tab.active().id;
        let tab_idx = self.push_terminal_tab(new_tab);

        self.connecting = Some(ConnectionProgress {
            label: label.clone(),
            hostname: hostname.clone(),
            step: ConnectionStep::Starting,
            logs: vec![(
                ConnectionStep::Starting,
                format!(
                    "Starting a new Telnet connection to \"{}\" port {}",
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

        let conn_host = conn.hostname.clone();
        let conn_port = conn.port;
        let stream = iced::stream::channel::<Message>(
            128,
            move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                match TelnetSession::connect(config).await {
                    Ok((session, mut rx)) => {
                        let transport = TerminalTransport::Telnet(Arc::new(session));
                        let _ = sender.send(Message::Ssh(SshMessage::SshConnected(pane_id, transport))).await;
                        while let Some(data) = rx.recv().await {
                            if sender.send(Message::Terminal(TerminalMessage::PtyOutput(pane_id, data))).await.is_err() {
                                break;
                            }
                        }
                        let _ = sender.send(Message::Ssh(SshMessage::SshDisconnected(pane_id))).await;
                    }
                    Err(e) => {
                        let _ = sender
                            .send(Message::Ssh(SshMessage::SshError(format!(
                                "Connection to {}:{} failed: {}",
                                conn_host, conn_port, e
                            ))))
                            .await;
                    }
                }
            },
        );

        Task::batch(vec![self.tab_scroll_to_active(), Task::stream(stream)])
    }

    /// Connect a Telnet host into an existing split pane (or an in-place
    /// reconnect). The counterpart of `spawn_ssh_for_pane_conn`, reached
    /// from the same wrappers whenever `conn.protocol == Telnet`.
    pub(crate) fn spawn_telnet_for_pane_conn(
        &mut self,
        mut conn: oryxis_core::models::Connection,
        quick_id: Option<Uuid>,
        tab_idx: usize,
        pane_id: Uuid,
    ) -> Task<Message> {
        let config = self.telnet_config(&mut conn, quick_id);

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
            self.session_logs_total += 1;
        }
        if let Some(log_id) = session_log_id
            && let Some(pane) = self.tabs[tab_idx].pane_by_id_mut(pane_id)
        {
            pane.start_session_log(log_id);
        }

        let stream = iced::stream::channel::<Message>(
            128,
            move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                match TelnetSession::connect(config).await {
                    Ok((session, mut rx)) => {
                        let transport = TerminalTransport::Telnet(Arc::new(session));
                        let _ = sender.send(Message::Ssh(SshMessage::SshConnected(pane_id, transport))).await;
                        while let Some(data) = rx.recv().await {
                            if sender.send(Message::Terminal(TerminalMessage::PtyOutput(pane_id, data))).await.is_err() {
                                break;
                            }
                        }
                        let _ = sender.send(Message::Ssh(SshMessage::SshDisconnected(pane_id))).await;
                    }
                    Err(e) => {
                        let _ = sender
                            .send(Message::Ssh(SshMessage::PaneConnectError(pane_id, e.to_string())))
                            .await;
                    }
                }
            },
        );

        Task::stream(stream)
    }
}
