//! Turning a freshly connected SSH session into a mosh one.
//!
//! This is the whole of the handover, and it lives at the point where
//! EVERY dial path converges rather than at the three that mint an SSH
//! transport (`start_ssh_tab`, `spawn_reused_session`,
//! `spawn_ssh_for_pane_conn`). The same reason `expand_jump_chain`
//! lives inside `make_jump_resolver`: a fourth dial site added later
//! inherits this for free, and cannot be written without it.
//!
//! Landing here also means the whole SSH connect experience is reused
//! as it stands, prompts and host keys and proxy consent and all. A
//! mosh host IS an SSH host right up to this line.
//!
//! **The SSH session is closed on the way out**, which is what mosh's
//! own wrapper does and is not a saving. SSH is TCP: it dies the moment
//! the address changes, which is the moment mosh exists for. Keeping it
//! would give the tab two lifetimes and let half of it break exactly
//! when the other half proved its worth, and the user would find out at
//! the worst possible moment. What needs SSH opens its own, visibly, in
//! a tab of its own.

use std::sync::Arc;

use oryxis_core::models::mosh::MoshOptions;
use oryxis_mosh::{BootstrapError, ServerCommand};
use oryxis_ssh::SshSession;

use crate::app::Oryxis;
use crate::messages::{Message, SshMessage, TerminalMessage};
use crate::state::TerminalTransport;

/// How long the far end gets to start `mosh-server` and say so.
///
/// Generous: this is one process spawn, but it happens after a login
/// shell has run whatever a host puts in its profile, and a host that
/// prints a slow banner is not a host that is failing.
const HANDOVER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

impl Oryxis {
    /// The mosh options for the connection behind `pane_id`, if it asks
    /// to be carried over mosh.
    ///
    /// Read from the pane's connection rather than passed down the dial,
    /// because the dial paths do not agree on what they carry and this
    /// has to be the same answer for all of them.
    pub(crate) fn pane_mosh_options(&self, pane_id: uuid::Uuid) -> Option<MoshOptions> {
        self.pane_connection(pane_id)?
            .mosh
            .clone()
            .filter(MoshOptions::is_enabled)
    }

    /// Start `mosh-server` over `ssh`, hand the session to mosh, and let
    /// the SSH connection go.
    pub(crate) fn begin_mosh_handover(
        &mut self,
        pane_id: uuid::Uuid,
        ssh: Arc<SshSession>,
        options: MoshOptions,
    ) -> iced::Task<Message> {
        let (cols, rows) = self
            .tabs
            .iter()
            .find_map(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
            .and_then(|p| p.terminal.lock().ok().map(|t| (t.cols(), t.rows())))
            .map_or((80, 24), |(c, r)| (c.max(1), r.max(1)));
        // Where the UDP session goes: the host as configured, because
        // for a direct dial that IS the address already known to reach
        // it, and `mosh-server -s` binds the one the SSH session arrived
        // on, which is the same one.
        //
        // A JUMP CHAIN breaks that pairing and cannot be repaired here.
        // The SSH connection reaches the final host FROM the last hop,
        // so `-s` binds an address facing the bastion, while this dials
        // what the user typed, from here. mosh is UDP and UDP does not
        // travel down an SSH tunnel, so there is no address that would
        // work: upstream mosh has the same limitation, for the same
        // reason. Documented as a limitation rather than guarded,
        // because a host reachable both ways works fine and refusing it
        // would take away a working setup.
        let host = self.pane_dialled_host(pane_id);
        // The same answer the pane's own emulator is given on every
        // output batch. The mosh screen decides where the server's
        // output lands and the pane draws the diff taken from it, so a
        // disagreement about how wide `│` is would put text in the pane
        // that the model never described.
        let ambiguous_width_wide = self
            .pane_connection(pane_id)
            .is_some_and(|c| c.ambiguous_width_effective());
        // PINNED on the pane, because the output funnel would otherwise
        // re-read the host's setting on every batch and a later edit
        // would flip the PANE while the screen inside the protocol keeps
        // the answer it was built with (there is no path to re-configure
        // it). On a mosh host the setting applies on the next connect,
        // the way encoding and TERM do. Cleared on disconnect.
        if let Some(pane) = self
            .tabs
            .iter_mut()
            .find_map(|t| t.pane_grid.panes.values_mut().find(|p| p.id == pane_id))
        {
            pane.mosh_ambiguous_width = Some(ambiguous_width_wide);
        }

        let command = ServerCommand {
            server_path: options.server_path.clone(),
            port_range: options.port_range.clone(),
            command: Some(options.command.clone()).filter(|c| !c.trim().is_empty()),
            ..Default::default()
        };
        let line = command.render();

        let stream = iced::stream::channel::<Message>(
            128,
            move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            use iced::futures::SinkExt as _;
            let _ = sender
                .send(Message::Ssh(SshMessage::SshProgress(
                    pane_id,
                    crate::state::ConnectionStep::OpeningSession,
                    line.clone(),
                )))
                .await;

            let said = ssh.exec_capture(&line, None, HANDOVER_TIMEOUT).await;
            let Some(result) = said else {
                let _ = sender
                    .send(Message::Ssh(SshMessage::SshError(
                        pane_id,
                        crate::i18n::t("mosh_error_no_answer").to_string(),
                    )))
                    .await;
                return;
            };
            // Both streams, because the announcement is on one and the
            // reason there is none is on the other.
            let output = format!("{}\n{}", result.stdout, result.stderr);

            let handover = match oryxis_mosh::parse(&output) {
                Ok(handover) => handover,
                Err(error) => {
                    let text = match error {
                        BootstrapError::NotInstalled => {
                            crate::i18n::t("mosh_error_not_installed").to_string()
                        }
                        other => crate::i18n::t("mosh_error_refused")
                            .replace("{reason}", &other.to_string()),
                    };
                    let _ = sender
                        .send(Message::Ssh(SshMessage::SshError(pane_id, text)))
                        .await;
                    return;
                }
            };

            let opened = oryxis_mosh::MoshSession::connect(
                &host,
                handover.port,
                &handover.key,
                cols,
                rows,
                ambiguous_width_wide,
            );
            let (session, mut rx) = match opened {
                Ok(pair) => pair,
                Err(error) => {
                    let _ = sender
                        .send(Message::Ssh(SshMessage::SshError(
                            pane_id,
                            crate::i18n::t("mosh_error_open")
                                .replace("{reason}", &error.to_string()),
                        )))
                        .await;
                    return;
                }
            };

            // The SSH connection has done its whole job. Letting it go
            // is the point: what is left is a session that survives the
            // address changing, and a TCP connection alongside it would
            // only be a second thing to die.
            ssh.close();

            // Wipe what SSH left on the pane, because mosh's model of
            // the screen starts BLANK and it only ever sends the
            // difference against that. The SSH session opened a shell of
            // its own on the way in, so the pane is already carrying a
            // login banner and a prompt that mosh will never mention
            // again: they would sit there under the real session for the
            // rest of its life. Clearing makes the pane match the model,
            // which is the whole contract the diff depends on.
            let _ = sender
                .send(Message::Terminal(TerminalMessage::PtyOutput(
                    pane_id,
                    b"\x1b[H\x1b[2J\x1b[3J\x1b[m".to_vec(),
                )))
                .await;

            let transport = TerminalTransport::Mosh(Arc::new(session));
            let _ = sender
                .send(Message::Ssh(SshMessage::SshConnected(pane_id, transport)))
                .await;
            while let Some(data) = rx.recv().await {
                if sender
                    .send(Message::Terminal(TerminalMessage::PtyOutput(pane_id, data)))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            let _ = sender
                .send(Message::Ssh(SshMessage::SshDisconnected(pane_id)))
                .await;
            },
        );

        iced::Task::stream(stream)
    }

    /// The address a pane's SSH session reached.
    fn pane_dialled_host(&self, pane_id: uuid::Uuid) -> String {
        self.pane_connection(pane_id)
            .map_or_else(|| "127.0.0.1".to_string(), |c| c.hostname.clone())
    }

    /// The saved connection behind a pane, if it has one.
    fn pane_connection(&self, pane_id: uuid::Uuid) -> Option<&oryxis_core::models::Connection> {
        let conn_id = self
            .tabs
            .iter()
            .find_map(|t| t.pane_grid.panes.values().find(|p| p.id == pane_id))
            .and_then(|p| match p.origin {
                crate::state::PaneOrigin::Host(id) => Some(id),
                _ => None,
            })?;
        self.connections.iter().find(|c| c.id == conn_id)
    }
}
