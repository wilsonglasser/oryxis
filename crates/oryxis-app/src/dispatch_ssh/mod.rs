//! `Oryxis::handle_ssh`, the SSH-domain router, plus the two
//! session-recording gates every submodule consults.
//!
//! The groups follow a connection through its life:
//!
//! - `launch`   : a pick becomes a dial (saved host or typed
//!   `user@host`), including the split-pane placement.
//! - `connect`  : `start_ssh_tab` and its `ConnectPlan` resolution, the
//!   spawn paths and the pane-entry helpers around them.
//! - `hostkey`  : host-key verification prompts + the
//!   no-common-algorithm / legacy-algorithm fallback dialog.
//! - `kbi`      : keyboard-interactive (2FA / OTP) prompts + the
//!   quick-connect identity / key auth switch.
//! - `progress` : the connection progress card, its retry, and the
//!   pre-auth banners that render on it.
//! - `session`  : a live session being wired up, probed for its OS, and
//!   torn down.
//! - `errors`   : dials that failed, per pane and for the card.

// Domain handlers return `Err(Message)` to pass an unclaimed message
// back up the chain. The Message enum is large (~200 bytes) but
// boxing it would force every handler-call site to allocate; the
// pattern is intentional, allow the lint.
#![allow(clippy::result_large_err)]

mod connect;
mod errors;
mod hostkey;
mod kbi;
mod launch;
mod mosh;
mod progress;
mod session;

use iced::Task;

use std::sync::Arc;
use uuid::Uuid;

use oryxis_ssh::SshSession;

use crate::app::{EditorMessage, SshMessage, Message, Oryxis};
use crate::state::View;

impl Oryxis {
    /// Whether a new session should be recorded to the vault. A per-host
    /// `Connection.session_logging` override wins; otherwise the global
    /// `session_logging` setting decides. Panes without a saved
    /// connection (local shells) fall through to the global value.
    pub(crate) fn should_record_session(
        &self,
        conn: Option<&oryxis_core::models::connection::Connection>,
    ) -> bool {
        conn.and_then(|c| c.session_logging)
            .unwrap_or(self.prefs.session_logging)
    }

    /// Whether connection events (connect / disconnect / auth failure /
    /// error) should be written to the vault log. Gated by the global
    /// `connection_history` setting (off by default).
    pub(crate) fn should_record_history(&self) -> bool {
        self.prefs.connection_history
    }

    /// Route one SSH-lifecycle message to the submodule that owns it.
    ///
    /// Exhaustive by design: a new `SshMessage` variant does not compile
    /// until it is listed here, which is the whole point of the router.
    ///
    /// The two older submodules (`hostkey`, `kbi`) predate it and still
    /// return `Result`, so their calls end in `unwrap_or_else(unrouted)`;
    /// since the router guarantees the family, that `Err` is now a loud
    /// grouping bug rather than a fall-through. The four newer ones just
    /// return the `Task`.
    pub(crate) fn handle_ssh(
        &mut self,
        message: SshMessage,
    ) -> Task<Message> {
        match message {
            // Host-key prompt / legacy-algorithm dialog / command-proxy
            // approval -> hostkey sub; keyboard-interactive auth +
            // quick-auth switch -> kbi sub.
            // Exhaustive: a new variant fails to compile until listed.
            m @ (SshMessage::SshNoCommonAlgo{ .. }
            | SshMessage::LegacyAlgoAccept{ .. }
            | SshMessage::LegacyAlgoCancel
            | SshMessage::SshHostKeyVerify(..)
            | SshMessage::SshHostKeyReject
            | SshMessage::SshHostKeyContinue
            | SshMessage::SshHostKeyAcceptAndSave
            | SshMessage::SshProxyCommandVerify(..)
            | SshMessage::SshProxyCommandReject
            | SshMessage::SshProxyCommandOnce
            | SshMessage::SshProxyCommandAlways) => {
                self.handle_ssh_hostkey(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SshMessage::SshKbiPrompt(..)
            | SshMessage::SshKbiInput(..)
            | SshMessage::SshKbiSubmit
            | SshMessage::SshKbiCancel
            | SshMessage::QuickAuthSwitch(..)) => {
                self.handle_ssh_kbi(m).unwrap_or_else(crate::dispatch::unrouted)
            }
            m @ (SshMessage::ConnectSsh(..)
            | SshMessage::ConnectSavedHost(..)
            | SshMessage::QuickConnect(..)
            | SshMessage::QuickConnectProtocolPicked(..)) => self.handle_ssh_launch(m),
            m @ (SshMessage::SshProgress(..)
            | SshMessage::SshCloseProgress
            | SshMessage::SshEditFromProgress
            | SshMessage::SshRetry
            | SshMessage::SshBanner(..)
            | SshMessage::SshPaneBanner(..)) => self.handle_ssh_progress(m),
            m @ (SshMessage::SshConnected(..)
            | SshMessage::OsDetected(..)
            | SshMessage::ReuseFailedDialFresh(..)
            | SshMessage::SshDisconnected(..)) => self.handle_ssh_session(m),
            m @ (SshMessage::PaneConnectError(..)
            | SshMessage::SshError(..)) => self.handle_ssh_errors(m),
        }
    }
}
