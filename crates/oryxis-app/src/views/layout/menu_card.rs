//! Overlay menu builders for vault card / list kebab menus. Split out
//! of `render_overlay_menu` in views/layout/menus.rs; each method returns
//! the inner menu `items` Element that `render_overlay_menu` wraps in the
//! shared popover container. Pure relocation, no behavior change.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn build_menu_session_log_actions(&self, idx: usize) -> Element<'_, Message> {
        self.build_menu_session_log_actions_impl(idx, true)
    }

    /// The viewer-header `...` variant, shared by the static viewer
    /// and the player header: both surfaces carry their own play
    /// affordance, so the menu skips the Play row.
    pub(crate) fn build_menu_session_log_viewer_actions(
        &self,
        idx: usize,
    ) -> Element<'_, Message> {
        self.build_menu_session_log_actions_impl(idx, false)
    }

    fn build_menu_session_log_actions_impl(
        &self,
        idx: usize,
        include_play: bool,
    ) -> Element<'_, Message> {
        let log_id = self.session_logs.get(idx).map(|e| e.id);
        let mut col = column![].spacing(2);
        if let Some(log_id) = log_id {
            // Replay actions (in-app player, .cast export) pair with
            // full-detail recording; with simple logs they are hidden
            // (owner call 2026-07-04), not just degraded.
            if self.prefs.session_log_full {
                if include_play {
                    col = col.push(self.menu_item(
                        iced_fonts::lucide::play(),
                        crate::i18n::t("session_play"),
                        Message::Player(PlayerMessage::Open(log_id)),
                        OryxisColors::t().success,
                    ));
                }
                col = col.push(self.menu_item(
                    iced_fonts::lucide::film(),
                    crate::i18n::t("export_cast_tip"),
                    Message::History(HistoryMessage::ExportSessionCast(log_id)),
                    OryxisColors::t().text_secondary,
                ));
                // Renders through the downloaded oryxis-gif plugin;
                // the handler opens the install modal on first use.
                col = col.push(self.menu_item(
                    iced_fonts::lucide::image(),
                    crate::i18n::t("export_gif_tip"),
                    Message::History(HistoryMessage::ExportSessionGif(log_id)),
                    OryxisColors::t().text_secondary,
                ));
            }
            col = col.push(self.menu_item(
                iced_fonts::lucide::file_text(),
                crate::i18n::t("export_transcript_tip"),
                Message::History(HistoryMessage::ExportSessionTranscript(log_id)),
                OryxisColors::t().text_secondary,
            ));
            col = col.push(self.menu_item(
                iced_fonts::lucide::keyboard(),
                crate::i18n::t("export_commands_tip"),
                Message::History(HistoryMessage::ExportSessionCommands(log_id)),
                OryxisColors::t().text_secondary,
            ));
        }
        col = col.push(self.menu_item(
            iced_fonts::lucide::trash(),
            crate::i18n::t("delete"),
            Message::History(HistoryMessage::RequestDeleteSessionLog(idx)),
            OryxisColors::t().error,
        ));
        // Honest-export caption: recordings carry the raw
        // bytes, Privacy Mode masking is display-only.
        col = col.push(
            container(
                text(crate::i18n::t("session_export_privacy_note"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding { top: 4.0, right: 12.0, bottom: 2.0, left: 12.0 })
            .width(Length::Fill),
        );
        col.into()
    }

    /// Kebab of a saved AI conversation row. Deliberately short: a saved
    /// conversation is read or deleted, never resumed (the terminal it was
    /// held against is gone), so there is no "continue" action to offer.
    pub(crate) fn build_menu_chat_conversation_actions(
        &self,
        idx: usize,
    ) -> Element<'_, Message> {
        let mut col = column![].spacing(2);
        if let Some(id) = self.chat_ui.conversations.get(idx).map(|c| c.id) {
            col = col.push(self.menu_item(
                iced_fonts::lucide::bot(),
                crate::i18n::t("chat_open"),
                Message::History(HistoryMessage::OpenChatConversation(id)),
                OryxisColors::t().text_secondary,
            ));
        }
        col = col.push(self.menu_item(
            iced_fonts::lucide::trash(),
            crate::i18n::t("delete"),
            Message::History(HistoryMessage::RequestDeleteChatConversation(idx)),
            OryxisColors::t().error,
        ));
        col.into()
    }

    pub(crate) fn build_menu_host_actions(&self, id: uuid::Uuid) -> Element<'_, Message> {
        self.build_menu_host_actions_inner(id, true)
    }

    /// The sidebar Hosts tree's reduced host menu (issue #102): the
    /// same actions as the card menu minus Remove and the dashboard
    /// filter entry.
    pub(crate) fn build_menu_tree_host_actions(&self, id: uuid::Uuid) -> Element<'_, Message> {
        self.build_menu_host_actions_inner(id, false)
    }

    /// `dashboard` gates the entries that only make sense on the
    /// dashboard surface: Remove (the tree is navigate-and-connect,
    /// destruction keeps its confirm over the card list).
    /// Row count of `build_menu_host_actions_inner` for the SAME host +
    /// surface, feeding `overlay_menu_height`. Kept next to the builder
    /// so a new entry can't ship without its height: the old fixed
    /// estimates clipped the menu whenever every conditional entry
    /// applied at once (WoL + SSH URL on the tree = 7 rows, not 6).
    pub(crate) fn host_actions_menu_rows(&self, id: uuid::Uuid, dashboard: bool) -> f32 {
        use oryxis_core::models::connection::ConnectionProtocol;
        let conn = self.connections.iter().find(|c| c.id == id);
        let protocol = conn.map(|c| c.protocol).unwrap_or(ConnectionProtocol::Ssh);
        let mut rows = 3.0; // Connect + Edit + Duplicate
        if protocol == ConnectionProtocol::Ssh {
            rows += 1.0; // Share
            if self.sftp_enabled {
                rows += 1.0; // Open SFTP tab
                if conn.is_some_and(Oryxis::host_can_console) {
                    rows += 1.0; // Open SFTP console
                }
            }
        }
        if matches!(
            protocol,
            ConnectionProtocol::Ssh | ConnectionProtocol::Telnet | ConnectionProtocol::Raw
        ) {
            rows += 1.0; // Copy SSH URL
        }
        if conn.and_then(|c| c.mac_address.as_deref()).is_some_and(|m| !m.is_empty()) {
            rows += 1.0; // Wake on LAN
        }
        if protocol == ConnectionProtocol::RemoteDesktop
            && conn.is_some_and(|c| self.remote_desktop_forwards.contains_key(&c.id))
        {
            rows += 1.0; // Stop remote desktop
        }
        if dashboard {
            rows += 1.0; // Remove
        }
        rows
    }

    fn build_menu_host_actions_inner(
        &self,
        id: uuid::Uuid,
        dashboard: bool,
    ) -> Element<'_, Message> {
        // The menu is anchored to the HOST, so the index every
        // index-taking action still needs is resolved here, per render,
        // against the list this frame draws. A re-sort under the open
        // menu (an auto-saved rename, a sync apply) goes through
        // update() -> view(), so the rebuilt items carry the host's new
        // position rather than aiming at whoever took the old one. A
        // host that vanished leaves the actions inert.
        let idx = self.connections.iter().position(|c| c.id == id);
        let conn = idx.and_then(|i| self.connections.get(i));
        // Every index-taking action goes through this, so a vanished
        // host cannot dispatch one against a stale position.
        let by_idx = |msg: fn(usize) -> Message| idx.map_or(Message::NoOp, msg);
        // SSH-only actions (Share + SFTP mount both ride the SSH
        // subsystem) and the URL scheme depend on the protocol.
        use oryxis_core::models::connection::ConnectionProtocol;
        let protocol = conn.map(|c| c.protocol).unwrap_or(ConnectionProtocol::Ssh);
        let is_ssh_host = protocol == ConnectionProtocol::Ssh;
        let is_rd_host = protocol == ConnectionProtocol::RemoteDesktop;
        // Every protocol that names a network endpoint has a URL worth
        // copying (`ssh://`, `telnet://`, `telnets://`, `raw://`).
        // Serial and Local name none.
        let has_url = matches!(
            protocol,
            ConnectionProtocol::Ssh | ConnectionProtocol::Telnet | ConnectionProtocol::Raw
        );
        let mut items = column![
            self.menu_item(iced_fonts::lucide::play(), crate::i18n::t("connect"), by_idx(|i| Message::Ssh(SshMessage::ConnectSsh(i))), OryxisColors::t().success),
            self.menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::Editor(EditorMessage::EditConnection(id)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate"), by_idx(|i| Message::Editor(EditorMessage::DuplicateConnection(i))), OryxisColors::t().text_secondary),
        ];
        if is_ssh_host {
            items = items
                .push(self.menu_item(iced_fonts::lucide::share(), crate::i18n::t("share"), by_idx(|i| Message::Share(ShareMessage::ShareConnection(i))), OryxisColors::t().text_secondary));
            // SFTP is an optional feature: its entry hides with the
            // toggle, like every other SFTP surface.
            if self.sftp_enabled {
                items = items.push(self.menu_item(iced_fonts::lucide::folder_tree(), crate::i18n::t("open_sftp_tab"), by_idx(|i| Message::Sftp(SftpMessage::OpenSftpForConnection(i))), OryxisColors::t().text_secondary));
                // The console right beside the browser, because they are
                // two answers to the same question and the whole point of
                // issue #188 is that some people want the other one. From
                // a card there is no shell to inherit a directory from,
                // so it opens at the session's home.
                // Not offered on a mosh host: the dial would hand the
                // pane to the mosh handover instead (see
                // `host_can_console`).
                if conn.is_some_and(Oryxis::host_can_console) {
                    items = items.push(self.menu_item(iced_fonts::lucide::square_terminal(), crate::i18n::t("open_sftp_console"), Message::Sftp(SftpMessage::OpenSftpConsoleForHost(id)), OryxisColors::t().text_secondary));
                }
            }
        }
        if has_url {
            items = items.push(self.menu_item(iced_fonts::lucide::link(), crate::i18n::t("copy_ssh_url"), by_idx(|i| Message::History(HistoryMessage::CopyHostSshUrl(i))), OryxisColors::t().text_secondary));
        }
        // Wake on LAN: only hosts with a stored MAC (editor > Network).
        if conn.and_then(|c| c.mac_address.as_deref()).is_some_and(|m| !m.is_empty()) {
            items = items.push(self.menu_item(iced_fonts::lucide::zap(), crate::i18n::t("wake_on_lan"), by_idx(|i| Message::History(HistoryMessage::WakeOnLan(i))), OryxisColors::t().text_secondary));
        }
        // Remote-desktop host: Connect (above) already launches the
        // desktop; add an explicit Stop while its tunnel is live.
        if is_rd_host
            && let Some(cid) = conn.map(|c| c.id)
            && self.remote_desktop_forwards.contains_key(&cid)
        {
            items = items.push(self.menu_item(
                iced_fonts::lucide::monitor_x(),
                crate::i18n::t("stop_remote_desktop"),
                Message::RemoteDesktop(RemoteDesktopMessage::StopRemoteDesktop(cid)),
                OryxisColors::t().error,
            ));
        }
        if !dashboard {
            return items.into();
        }
        items
            .push(self.menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), by_idx(|i| Message::Editor(EditorMessage::RequestDeleteConnection(i))), OryxisColors::t().error))
            .into()
    }

    pub(crate) fn build_menu_session_group_actions(&self, idx: usize) -> Element<'_, Message> {
        column![
            self.menu_item(iced_fonts::lucide::play(), crate::i18n::t("open_session_group"), Message::SessionGroup(SessionGroupMessage::OpenSessionGroup(idx)), OryxisColors::t().success),
            self.menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::SessionGroup(SessionGroupMessage::EditSessionGroup(idx)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate"), Message::SessionGroup(SessionGroupMessage::DuplicateSessionGroup(idx)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::SessionGroup(SessionGroupMessage::RequestDeleteSessionGroup(idx)), OryxisColors::t().error),
        ]
        .into()
    }

    pub(crate) fn build_menu_key_actions(&self, idx: usize) -> Element<'_, Message> {
        let mut items = column![
            self.menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::Keys(KeysMessage::EditKey(idx)), OryxisColors::t().text_secondary),
        ];
        // Expose-via-agent toggle: only offered while the agent server
        // is running, so it stays out of the menu for users who never
        // turned the feature on. A check glyph marks the current state.
        // Security-key rows (B3) have no private half in the vault, so
        // Oryxis's own agent can never serve them; the toggle would be
        // a lie there and is hidden.
        if self.agent.enabled
            && let Some(key) = self.keys.get(idx)
            && !key.algorithm.is_security_key()
        {
            let (glyph, label) = if key.expose_via_agent {
                (iced_fonts::lucide::circle_check(), crate::i18n::t("agent_key_exposed"))
            } else {
                (iced_fonts::lucide::circle(), crate::i18n::t("agent_key_hidden"))
            };
            items = items.push(self.menu_item(
                glyph,
                label,
                Message::Agent(AgentMessage::KeyExposeViaAgentToggled(key.id)),
                OryxisColors::t().text_secondary,
            ));
        }
        // Certificate actions, only when the key carries one (B2).
        if let Some(key) = self.keys.get(idx)
            && key.certificate.is_some()
        {
            items = items.push(self.menu_item(
                iced_fonts::lucide::badge_check(),
                crate::i18n::t("cert_view"),
                Message::Keys(KeysMessage::ViewKeyCertificate(idx)),
                OryxisColors::t().text_secondary,
            ));
            items = items.push(self.menu_item(
                iced_fonts::lucide::badge_x(),
                crate::i18n::t("cert_remove"),
                Message::Keys(KeysMessage::RequestRemoveKeyCertificate(idx)),
                OryxisColors::t().text_secondary,
            ));
        }
        items = items.push(self.menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::Keys(KeysMessage::RequestDeleteKey(idx)), OryxisColors::t().error));
        items.into()
    }

    pub(crate) fn build_menu_identity_actions(&self, idx: usize) -> Element<'_, Message> {
        column![
            self.menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::Keys(KeysMessage::EditIdentity(idx)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::Keys(KeysMessage::RequestDeleteIdentity(idx)), OryxisColors::t().error),
        ].into()
    }

    pub(crate) fn build_menu_snippet_actions(&self, idx: usize) -> Element<'_, Message> {
        column![
            self.menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::Snippet(SnippetMessage::EditSnippet(idx)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::Snippet(SnippetMessage::RequestDeleteSnippet(idx)), OryxisColors::t().error),
        ].into()
    }

    /// Kebab menu on a port-forward rule card. Edit is here even though
    /// clicking the card already edits: a menu whose only entry is Delete
    /// reads like deletion is all this card can do.
    pub(crate) fn build_menu_port_forward_actions(&self, idx: usize) -> Element<'_, Message> {
        column![
            self.menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::PortForward(PortForwardMessage::EditPortForwardRule(idx)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::PortForward(PortForwardMessage::RequestDeletePortForwardRule(idx)), OryxisColors::t().error),
        ].into()
    }

    pub(crate) fn build_menu_keychain_add(&self) -> Element<'_, Message> {
        // The "+ ADD ▾" keychain menu: one row per entry of the shared
        // add catalog (`views::add_actions`), which the empty keychain
        // renders as buttons from the same list.
        let mut items = column![];
        for action in self.add_key_actions() {
            items = items.push(self.menu_item(action.icon, action.label, action.msg, action.color));
        }
        items.into()
    }

    pub(crate) fn build_menu_folder_actions(&self, gid: uuid::Uuid) -> Element<'_, Message> {
        column![
            self.menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::Tabs(TabsMessage::EditGroup(gid)), OryxisColors::t().accent),
            self.menu_item(iced_fonts::lucide::folder_plus(), crate::i18n::t("new_subgroup"), Message::Tabs(TabsMessage::NewSubgroup(gid)), OryxisColors::t().text_secondary),
            self.menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::Tabs(TabsMessage::StartDeleteFolder(gid)), OryxisColors::t().error),
        ].into()
    }

    /// Plugin-row kebab: the secondary actions the compact row doesn't
    /// carry inline. Installed rows get uninstall; a dev build only
    /// offers removing the cached downloads it shadows.
    pub(crate) fn build_menu_plugin_actions(&self, provider_id: &str) -> Element<'_, Message> {
        use crate::state::PluginUiStatus;
        let Some(entry) = self.plugins.iter().find(|p| p.provider_id == provider_id) else {
            return column![].into();
        };
        let id = entry.provider_id.clone();
        let mut items = column![];
        match &entry.status {
            PluginUiStatus::DevBuild if entry.cached_install => {
                items = items.push(self.menu_item(
                    iced_fonts::lucide::trash(),
                    crate::i18n::t("plugin_action_remove_downloads"),
                    Message::Plugin(PluginMessage::PluginUninstall(id)),
                    OryxisColors::t().error,
                ));
            }
            PluginUiStatus::Installed(_) => {
                items = items.push(self.menu_item(
                    iced_fonts::lucide::trash(),
                    crate::i18n::t("plugin_action_uninstall"),
                    Message::Plugin(PluginMessage::PluginUninstall(id)),
                    OryxisColors::t().error,
                ));
            }
            // The kebab only renders on the states above; a
            // not-installed row that somehow opens it gets nothing.
            _ => {}
        }
        items.into()
    }

}
