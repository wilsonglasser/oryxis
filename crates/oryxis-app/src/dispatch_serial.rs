//! Serial connect paths (full tab + split pane), the transport sibling
//! of `dispatch_telnet.rs`. Even thinner: no credentials, no network,
//! no negotiation. Opening the port is synchronous (`SerialSession::open`
//! is not async), so the stream producer opens it up front and then
//! pumps output. Reuses the SSH lifecycle messages (`SshConnected` /
//! `PtyOutput` / `SshError` / `SshDisconnected` / `PaneConnectError`)
//! so recording, reconnect, history and the tab strip ride unchanged.

use iced::Task;
use iced::futures::SinkExt;

use std::sync::{Arc, Mutex};
use uuid::Uuid;

use oryxis_serial::{SerialConfig, SerialSession};
use oryxis_terminal::widget::TerminalState;

use crate::app::{TerminalMessage, SshMessage, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS, Message, Oryxis};
use crate::state::{ConnectionProgress, ConnectionStep, TerminalTab, TerminalTransport};

impl Oryxis {
    /// Build the engine config for one serial line: the port path is
    /// `hostname`, the line parameters are `Connection.serial` (falling
    /// back to defaults, so a host that predates the field still opens).
    fn serial_config(conn: &oryxis_core::models::Connection) -> SerialConfig {
        SerialConfig {
            path: conn.hostname.clone(),
            params: conn.serial.unwrap_or_default(),
        }
    }

    /// Open a new tab on a serial line. Counterpart of `start_ssh_tab` /
    /// `start_telnet_tab`, reached whenever `conn.protocol == Serial`.
    pub(crate) fn start_serial_tab(
        &mut self,
        conn: oryxis_core::models::Connection,
        origin: crate::state::ProgressOrigin,
    ) -> Task<Message> {
        let config = Self::serial_config(&conn);

        let Ok(mut state) =
            TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16)
        else {
            tracing::error!("Failed to create terminal state for serial tab");
            return Task::none();
        };
        state.palette = self.resolve_terminal_palette_for_connection(&conn);
        let label = conn.label.clone();
        let hostname = format!("Serial {} @ {}", conn.hostname, config.params.baud);
        let terminal = Arc::new(Mutex::new(state));

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
        // C5: serial consoles get the same per-host quirks (their core
        // audience: legacy line disciplines, appliance keyboards).
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
                format!("Opening {} @ {} baud...", conn.hostname, config.params.baud),
            )],
            failed: false,
            origin,
            tab_idx,
            pane_id,
            banner: None,
        });
        self.active_tab = Some(tab_idx);
        self.remember_terminal_tab_focus(tab_idx);

        let path = conn.hostname.clone();
        let stream = iced::stream::channel::<Message>(
            128,
            move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                match SerialSession::open(config) {
                    Ok((session, mut rx)) => {
                        let transport = TerminalTransport::Serial(Arc::new(session));
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
                            .send(Message::Ssh(SshMessage::SshError(format!("Serial {path}: {e}"))))
                            .await;
                    }
                }
            },
        );

        Task::batch(vec![self.tab_scroll_to_active(), Task::stream(stream)])
    }

    /// Open a serial line into an existing split pane (or an in-place
    /// reconnect). Counterpart of `spawn_ssh_for_pane_conn`.
    pub(crate) fn spawn_serial_for_pane_conn(
        &mut self,
        conn: oryxis_core::models::Connection,
        tab_idx: usize,
        pane_id: Uuid,
    ) -> Task<Message> {
        let config = Self::serial_config(&conn);

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

        let path = conn.hostname.clone();
        let stream = iced::stream::channel::<Message>(
            128,
            move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                match SerialSession::open(config) {
                    Ok((session, mut rx)) => {
                        let transport = TerminalTransport::Serial(Arc::new(session));
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
                            .send(Message::Ssh(SshMessage::PaneConnectError(pane_id, format!("Serial {path}: {e}"))))
                            .await;
                    }
                }
            },
        );

        Task::stream(stream)
    }
}
