//! `Oryxis::update`, the master message-dispatch table. ~5k lines of
//! match arms; pulled out of `app.rs` so the wiring file stays trim.
//! All `pub(crate)` helpers it relies on live in sibling modules
//! (`sftp_helpers`, `sftp_methods`, `connect_methods`, `util`,
//! `boot`, `mcp`, `state`).

#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;


use crate::app::{Message, Oryxis};

/// How long a dynamic group's resolved host list stays "fresh" before
/// re-opening the group triggers a background re-resolve. Cloud
/// resources (ECS tasks especially) recycle, so a list older than this
/// is likely to contain dead rows that fail on click. 60s balances
/// freshness against hammering the cloud API on every navigation.
pub(crate) const DYNAMIC_GROUP_CACHE_TTL_SECS: i64 = 60;

/// Chain `message` through a domain handler. If the handler claims it
/// (returns `Ok`), short-circuit and return the resulting task.
/// Otherwise, the message is handed back unchanged for the next link.
macro_rules! try_handler {
    ($self:ident, $msg:ident, $handler:ident) => {
        match $self.$handler($msg) {
            Ok(task) => return task,
            Err(m) => m,
        }
    };
}

impl Oryxis {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // SFTP async-continuation messages target a specific tab that may no
        // longer be focused. Swap the owning tab's state into `self.sftp` for
        // the duration so the (unchanged) handlers route to the right tab,
        // then swap back. See `route_sftp_async`.
        let task = if let Some(id) = message.sftp_async_owner() {
            self.route_sftp_async(id, message)
        } else {
            self.dispatch_message(message)
        };
        // Keep the unified strip order (terminal + SFTP) in sync with the live
        // tabs after every message: new tabs appended, closed ones dropped,
        // drag-reordered order preserved.
        self.reconcile_tab_order();
        task
    }

    /// Show a generic "remove this?" confirmation. Confirming dispatches
    /// `action` (the real `Delete*` message). Routes destructive removals
    /// (host, key, identity, snippet, session group) through an explicit
    /// confirm, mirroring the known-hosts / SFTP delete guards so a stray
    /// click can't silently drop an entry. Closes any open card menu first
    /// so it doesn't linger behind the dialog scrim.
    pub(crate) fn confirm_remove(&mut self, name: String, action: Message) {
        self.card_context_menu = None;
        self.snippet_context_menu = None;
        self.key_context_menu = None;
        self.identity_context_menu = None;
        self.overlay = None;
        self.error_dialog = Some(crate::state::ErrorDialog {
            title: crate::i18n::t("remove_confirm_title").to_string(),
            body: format!("\"{name}\""),
            link: None,
            action: Some(crate::state::ErrorDialogAction {
                label: crate::i18n::t("remove").to_string(),
                message: Box::new(action),
                danger: true,
            }),
        });
    }

    pub(crate) fn dispatch_message(&mut self, message: Message) -> Task<Message> {
        // Domain-specific handlers each claim a slice of `Message`
        // variants and return `Err(message)` for everything else, so
        // the chain naturally falls through to the inline match below.
        let message = try_handler!(self, message, handle_sftp_transfers);
        let message = try_handler!(self, message, handle_sftp_files);
        let message = try_handler!(self, message, handle_sftp);
        let message = try_handler!(self, message, handle_ssh);
        let message = try_handler!(self, message, handle_port_forwards);
        let message = try_handler!(self, message, handle_settings);
        let message = try_handler!(self, message, handle_keys);
        let message = try_handler!(self, message, handle_proxy_identity);
        let message = try_handler!(self, message, handle_plugins);
        let message = try_handler!(self, message, handle_cloud);
        let message = try_handler!(self, message, handle_ai);
        let message = try_handler!(self, message, handle_editor);
        let message = try_handler!(self, message, handle_session_group);
        let message = try_handler!(self, message, handle_tabs);
        let message = try_handler!(self, message, handle_terminal);
        let message = try_handler!(self, message, handle_share);
        let message = try_handler!(self, message, handle_known_hosts);
        let message = try_handler!(self, message, handle_tray);
        let message = try_handler!(self, message, handle_vault);
        let message = try_handler!(self, message, handle_snippets);
        let message = try_handler!(self, message, handle_navigation);
        let message = try_handler!(self, message, handle_history);
        let message = try_handler!(self, message, handle_mcp);
        let message = try_handler!(self, message, handle_sync);

        // Every Message variant is now claimed by one of the domain handlers
        // in the `try_handler!` chain above. Anything reaching here is an
        // unclaimed variant we forgot to wire up; treat as a no-op so we don't
        // crash on it (the handlers each fall through with `Err(message)`).
        let _ = message;
        Task::none()
    }

    /// Push the current window state (hidden + tab labels) into the
    /// tray_ipc registry so the primary's tray menu picks it up on
    /// its next scan. No-op for the primary itself (its tray rebuild
    /// reads from in-process Oryxis state directly, not via the
    /// filesystem registry).
    ///
    /// Signature-gated so 100 ms TrayPoll ticks don't churn the
    /// filesystem when nothing changed; explicit hide/show handlers
    /// also call this so the registry refreshes within one tick of
    /// the user action instead of waiting for the polling tick.
    pub(crate) fn broadcast_ipc_state_if_child(&mut self) {
        if crate::app::APP_IS_PRIMARY.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.is_window_hidden.hash(&mut h);
        self.tabs.len().hash(&mut h);
        for t in &self.tabs {
            t.label.hash(&mut h);
        }
        let sig = h.finish();
        if sig == self.ipc_state_signature {
            return;
        }
        self.ipc_state_signature = sig;
        let tabs: Vec<String> = self.tabs.iter().map(|t| t.label.clone()).collect();
        // Title: when the user has an active tab the label is what
        // they're staring at, otherwise fall back to a generic
        // "Oryxis" so the primary's submenu still has something to
        // show.
        let title = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.label.clone())
            .unwrap_or_else(|| "Oryxis".to_string());
        crate::tray_ipc::Child::write_state(crate::tray_ipc::InstanceState {
            pid: std::process::id(),
            title,
            tabs,
            is_hidden: self.is_window_hidden,
        });
    }
}
