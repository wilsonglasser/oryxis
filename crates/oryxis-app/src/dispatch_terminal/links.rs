//! Opening a link the terminal printed.
//!
//! Ctrl+click used to hand the URL straight to the OS from inside the
//! widget. Two things it could not do from there, both of which need the
//! PANE:
//!
//! - **Ask first.** The bytes came from a remote host, and an OSC 8 link
//!   can put any label it likes over any target. A prompt naming the
//!   host and the real target is the same gate VS Code puts in front of
//!   a link opened out of a remote window.
//! - **Tunnel the callback.** A CLI login (`aws sso login` and every
//!   other OAuth client) starts a listener on ITS OWN loopback and puts
//!   `redirect_uri=http://127.0.0.1:<port>/...` in the URL it prints.
//!   Over SSH that listener is on the remote machine, so the browser
//!   here follows the redirect to a port on THIS machine and the login
//!   dies at the last step. Binding that same port locally and forwarding
//!   it down the pane's existing SSH connection is what makes the round
//!   trip close, and it is what VS Code's auto-forwarded ports do for a
//!   remote window.
//!
//! The tunnel is deliberately short-lived: it serves the one callback and
//! closes (see `AutoClose`), because it exists for a redirect that
//! happens once, not as a forward the user asked for and can see.

use std::sync::Arc;

use iced::Task;
use uuid::Uuid;

use super::*;
use crate::terminal_link::{display_target, loopback_callback, LoopbackCallback};

/// Idle grace once the callback has been served. The redirect is a
/// single short request; a couple of seconds covers a browser that
/// re-issues it (a favicon fetch, a retry) before the tunnel goes.
const CALLBACK_IDLE_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// How long a callback tunnel waits for a browser that never arrives.
/// An abandoned login (the tab was closed, the user gave up) must not
/// leave a local port bound for the rest of the session. Comfortably
/// longer than an interactive SSO round trip, comfortably shorter than
/// the device-authorization lifetimes providers use.
const CALLBACK_UNUSED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Characters of URL shown in the confirmation. An authorize URL runs to
/// several hundred; the head and tail are what a person actually reads.
const URL_DISPLAY_CHARS: usize = 180;

/// A pending "open this link?" question.
#[derive(Debug, Clone)]
pub(crate) struct LinkConfirmCard {
    /// The pane the link was clicked in, and the only session its
    /// callback may be tunnelled through.
    pub pane_id: Uuid,
    /// The real target, handed to the OS if the user agrees. Shown in
    /// full-ish form (elided in the middle) rather than behind a label,
    /// because agreeing to a name you cannot check is not consent.
    pub url: String,
    /// `url` cut to something a dialog can hold.
    pub display: String,
    /// The pane's label, naming who printed this.
    pub host_label: String,
    /// The loopback callback the link carries, when it has one.
    pub callback: Option<LoopbackCallback>,
    /// Whether that callback can actually be tunnelled: false when the
    /// pane's transport is not SSH (telnet, serial, a session carried
    /// over mosh), where there is no connection to open a channel on.
    /// The dialog says so instead of implying a working login.
    pub tunnelable: bool,
}

impl Oryxis {
    /// Ctrl+click landed on a link in `pane_id`.
    ///
    /// Local panes keep the old behaviour (the URL came from a program
    /// running as the user, and asking about every `cargo doc` link
    /// would be noise). A remote pane asks first when the setting is on,
    /// which it is by default.
    ///
    /// A link that is about to OPEN A PORT on this machine asks whatever
    /// that setting says. The two are not the same question: the setting
    /// governs a warning about where a browser is being sent, while the
    /// dialog's callback line is the only place the forward is ever
    /// described, and the code below is what makes it happen. Consent
    /// belongs to the half with the consequence, so turning the warning
    /// off cannot silently take it with it.
    pub(crate) fn activate_terminal_link(&mut self, pane_id: Uuid, url: String) -> Task<Message> {
        // The gesture landed, so the "hold Ctrl and click" hint has done
        // its job for this pane (HintMode::Once). In-memory only, exactly
        // like the widget-driven path it replaces.
        if let Some(pane) = self.pane_by_id_mut(pane_id) {
            pane.link_hint_shown = true;
        }
        let Some(pane) = self.pane_by_id(pane_id) else {
            return Task::none();
        };
        let Some(transport) = pane.session.as_ref() else {
            // Local shell: no remote author, no callback to tunnel.
            return self.launch_link(url);
        };
        let tunnelable = transport.ssh().is_some();
        let host_label = pane.label.clone();
        let callback = self.link_callback(&url);
        // A callback with nowhere to be tunnelled (telnet, serial, mosh)
        // opens no port, so it has nothing of its own to consent to and
        // follows the setting like any other link.
        if !self.prefs.terminal_link_confirm && !(callback.is_some() && tunnelable) {
            return self.open_terminal_link(pane_id, url);
        }
        self.link_confirm = Some(LinkConfirmCard {
            pane_id,
            display: display_target(&url, URL_DISPLAY_CHARS),
            callback,
            url,
            host_label,
            tunnelable,
        });
        Task::none()
    }

    /// Answer the confirmation. Anything but an explicit yes drops the
    /// card and opens nothing.
    pub(crate) fn resolve_link_confirm(&mut self, open: bool) -> Task<Message> {
        let Some(card) = self.link_confirm.take() else {
            return Task::none();
        };
        if !open {
            return Task::none();
        }
        self.open_terminal_link(card.pane_id, card.url)
    }

    /// Copy the target instead of opening it, and close the question.
    ///
    /// A user who wants to look at a URL before following it has
    /// answered the dialog, so it should not stay open waiting for a
    /// second click on Cancel.
    pub(crate) fn copy_link_confirm(&mut self) -> Task<Message> {
        let Some(card) = self.link_confirm.take() else {
            return Task::none();
        };
        self.update(Message::CopyToClipboard(card.url))
    }

    /// The callback a link carries, or `None` when the feature is off.
    fn link_callback(&self, url: &str) -> Option<LoopbackCallback> {
        self.prefs
            .terminal_link_tunnel
            .then(|| loopback_callback(url))
            .flatten()
    }

    /// Open a link that has been agreed to (or that never needed asking),
    /// tunnelling its callback port first when there is one.
    ///
    /// The order matters: the tunnel has to be listening BEFORE the
    /// browser is launched, because the redirect can come back within a
    /// second of the user finishing at the provider.
    fn open_terminal_link(&mut self, pane_id: Uuid, url: String) -> Task<Message> {
        let Some(callback) = self.link_callback(&url) else {
            return self.launch_link(url);
        };
        let Some(ssh) = self
            .pane_by_id(pane_id)
            .and_then(|p| p.session.as_ref())
            .and_then(|t| t.ssh())
            .cloned()
        else {
            // Nothing to tunnel through. The link still opens: the user
            // asked for it, and it may be a login they intend to finish
            // by hand on the other machine.
            return self.launch_link(url);
        };
        let port = callback.port;
        // The far end is named the way the link named it, with an IPv6
        // literal's brackets off: sshd resolves it on the REMOTE side, so
        // a `localhost` callback reaches whichever loopback family the
        // CLI actually bound there, which `127.0.0.1` alone would miss on
        // a host that resolved it to `::1`.
        let target_host = callback.host.trim_matches(['[', ']']).to_string();
        // The near end takes the FAMILY the callback was written in. A
        // browser handed `http://[::1]:<port>/` dials IPv6 and nothing
        // else, so an IPv4-only listener would leave the redirect
        // reaching nothing after the tunnel reported success. A name
        // (`localhost`) stays on IPv4: every resolver has that address
        // for it, and a browser that tries `::1` first falls back.
        let listen_host = if target_host.parse::<std::net::Ipv6Addr>().is_ok() {
            "::1"
        } else {
            "127.0.0.1"
        };
        // Already forwarded for this pane (a retried login, a second
        // click on the same URL): reuse it rather than fighting our own
        // listener for the port. Both questions matter: a tunnel that has
        // served its callback cancels itself while the pane's connection
        // stays up, so "the connection is alive" alone would keep handing
        // back a tunnel whose listener is gone.
        if self
            .link_forwards
            .get(&(pane_id, port))
            .is_some_and(|f| !f.is_cancelled() && f.is_alive())
        {
            return self.launch_link(url);
        }
        self.link_forwards.remove(&(pane_id, port));
        let host_label = self
            .pane_by_id(pane_id)
            .map(|p| p.label.clone())
            .unwrap_or_default();
        let policy = oryxis_ssh::AutoClose {
            idle_grace: CALLBACK_IDLE_GRACE,
            unused_timeout: Some(CALLBACK_UNUSED_TIMEOUT),
        };
        let stream = iced::stream::channel::<Message>(
            4,
            move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                use iced::futures::SinkExt;
                // Both ends are loopback on the same port: the far end is
                // the listener the CLI opened on the remote's own
                // loopback, and the near end has to match it because the
                // authorization server was told that exact
                // `redirect_uri`.
                let outcome = ssh
                    .open_local_forward(listen_host, port, &target_host, port, policy)
                    .await
                    .map(Arc::new)
                    .map_err(|e| {
                        tracing::warn!("callback tunnel on {port} failed: {e}");
                        crate::i18n::t("terminal_link_tunnel_failed")
                            .replace("{port}", &port.to_string())
                            .replace("{host}", &host_label)
                    });
                let closed = outcome.as_ref().ok().map(|f| f.subscribe_cancel());
                let _ = sender
                    .send(Message::Terminal(TerminalMessage::TerminalLinkTunnelReady(
                        pane_id, port, url, outcome,
                    )))
                    .await;
                // Wait out the tunnel's own life so the app can drop its
                // bookkeeping when it self-closes (callback served, or
                // never used). The watch receiver holds no strong
                // reference, so this task never keeps a tunnel alive.
                if let Some(mut closed) = closed {
                    let _ = closed.wait_for(|&c| c).await;
                    let _ = sender
                        .send(Message::Terminal(TerminalMessage::TerminalLinkTunnelClosed(
                            pane_id, port,
                        )))
                        .await;
                }
            },
        );
        Task::stream(stream)
    }

    /// The tunnel attempt settled: open the browser, or explain why not.
    pub(crate) fn link_tunnel_ready(
        &mut self,
        pane_id: Uuid,
        port: u16,
        url: String,
        outcome: Result<Arc<oryxis_ssh::ForwardSession>, String>,
    ) -> Task<Message> {
        match outcome {
            Ok(forward) => {
                self.link_forwards.insert((pane_id, port), forward);
                let toast = crate::i18n::t("terminal_link_tunnel_open")
                    .replace("{port}", &port.to_string());
                self.set_toast(toast);
                self.launch_link(url)
            }
            Err(msg) => {
                // Deliberately NOT opening the link. The browser would
                // follow the provider's redirect to this machine's own
                // port, handing an authorization code to whatever local
                // process holds it. Saying so is the useful outcome.
                self.set_toast(msg);
                Task::none()
            }
        }
    }

    /// The tunnel closed on its own (callback served, or it waited out
    /// its unused timeout).
    ///
    /// Only drops the entry if the tunnel sitting there is the cancelled
    /// one: a second login on the same port may have replaced it while
    /// this message was in flight, and that one is live.
    pub(crate) fn link_tunnel_closed(&mut self, pane_id: Uuid, port: u16) {
        if self
            .link_forwards
            .get(&(pane_id, port))
            .is_some_and(|f| f.is_cancelled())
        {
            self.link_forwards.remove(&(pane_id, port));
        }
    }

    /// Hand a URL to the OS default handler.
    ///
    /// Through the terminal crate's opener, not the app's: the target is
    /// bytes a remote host printed, and that path is the one written for
    /// them (scheme allowlist, and no shell between the string and the
    /// handler on Windows). A launch that fails says so, because the
    /// alternative is a click that visibly does nothing.
    fn launch_link(&mut self, url: String) -> Task<Message> {
        if !oryxis_terminal::open_url(&url) {
            tracing::warn!("opening {url} failed");
            self.set_toast(crate::i18n::t("terminal_link_open_failed").to_string());
        }
        Task::none()
    }

    /// Drop callback tunnels whose pane is gone.
    ///
    /// Dropping the `Arc` cancels the tunnel (the app holds the only
    /// strong one), releasing the local port. Called from the pane and
    /// tab close paths; the tunnels also expire on their own, so a path
    /// that forgets to call this leaks a port for minutes, not forever.
    pub(crate) fn prune_link_forwards(&mut self) {
        if self.link_forwards.is_empty() {
            return;
        }
        let live: std::collections::HashSet<Uuid> = self
            .tabs
            .iter()
            .flat_map(|t| t.pane_grid.panes.values().map(|p| p.id))
            .collect();
        self.link_forwards.retain(|(pane_id, _), _| live.contains(pane_id));
    }
}
