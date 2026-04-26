//! `Oryxis::update` — the master message-dispatch table. ~5k lines of
//! match arms; pulled out of `app.rs` so the wiring file stays trim.
//! All `pub(crate)` helpers it relies on live in sibling modules
//! (`sftp_helpers`, `sftp_methods`, `connect_methods`, `util`,
//! `boot`, `mcp`, `state`).

#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;

use oryxis_vault::VaultError;

use crate::app::{Message, Oryxis};
use crate::mcp::{install_mcp_config_to_file, mcp_config_json};
use crate::state::{ConnectionForm, PortForwardForm, VaultState, View};
use crate::util::strip_ansi;

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
        // Domain-specific handlers each claim a slice of `Message`
        // variants and return `Err(message)` for everything else, so
        // the chain naturally falls through to the inline match below.
        let message = try_handler!(self, message, handle_sftp_transfers);
        let message = try_handler!(self, message, handle_sftp_files);
        let message = try_handler!(self, message, handle_sftp);
        let message = try_handler!(self, message, handle_ssh);
        let message = try_handler!(self, message, handle_settings);
        let message = try_handler!(self, message, handle_keys);
        let message = try_handler!(self, message, handle_ai);
        let message = try_handler!(self, message, handle_editor);
        let message = try_handler!(self, message, handle_tabs);
        let message = try_handler!(self, message, handle_terminal);
        let message = try_handler!(self, message, handle_share);

        match message {
            // -- Vault --
            Message::VaultPasswordChanged(pw) => {
                self.vault_password_input = pw;
            }
            Message::VaultSetup => {
                if self.vault_password_input.len() < 4 {
                    self.vault_error = Some("Password must be at least 4 characters".into());
                    return Task::none();
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_master_password(&self.vault_password_input) {
                        Ok(()) => {
                            let _ = vault.set_setting("has_user_password", "1");
                            self.vault_has_user_password = true;
                            self.vault_state = VaultState::Unlocked;
                            self.vault_error = None;
                            // Cache for child-window spawn.
                            self.master_password = Some(self.vault_password_input.clone());
                            self.vault_password_input.clear();
                            self.load_data_from_vault();
                        }
                        Err(e) => {
                            self.vault_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::VaultSkipPassword => {
                if let Some(vault) = &mut self.vault {
                    match vault.open_without_password() {
                        Ok(()) => {
                            self.vault_state = VaultState::Unlocked;
                            self.vault_error = None;
                            self.load_data_from_vault();
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_error = Some(
                                "This vault already has a password. Enter it above to unlock.".into()
                            );
                        }
                        Err(e) => {
                            self.vault_error = Some(format!("Failed to create vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultDestroyConfirm => {
                self.vault_destroy_confirm = !self.vault_destroy_confirm;
            }
            Message::VaultDestroy => {
                if let Some(vault) = &mut self.vault {
                    match vault.destroy_and_recreate() {
                        Ok(()) => {
                            self.vault_state = VaultState::NeedSetup;
                            self.vault_error = None;
                            self.vault_destroy_confirm = false;
                            self.vault_password_input.clear();
                        }
                        Err(e) => {
                            self.vault_error = Some(format!("Failed to reset vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultUnlock => {
                if let Some(vault) = &mut self.vault {
                    match vault.unlock(&self.vault_password_input) {
                        Ok(()) => {
                            self.vault_state = VaultState::Unlocked;
                            self.vault_error = None;
                            // Retain the password in memory so we can spawn
                            // child windows with it via stdin pipe.
                            self.master_password = Some(self.vault_password_input.clone());
                            self.vault_password_input.clear();
                            self.load_data_from_vault();
                            // After a manual unlock, fire any deferred
                            // `--connect <uuid>` from the launch CLI args.
                            if let Some(connect_id) = self.pending_auto_connect.take()
                                && let Some(idx) = self
                                    .connections
                                    .iter()
                                    .position(|c| c.id == connect_id)
                            {
                                return Task::done(Message::ConnectSsh(idx));
                            }
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_error = Some("Invalid password".into());
                        }
                        Err(e) => {
                            self.vault_error = Some(e.to_string());
                        }
                    }
                }
            }

            // -- Navigation --
            Message::ChangeView(view) => {
                self.active_view = view;
                self.active_tab = None;
                // Lazy-load the local SFTP pane when the user first lands
                // on the view (or returns to it after the underlying dir
                // changed). Cheap enough to redo unconditionally.
                if view == View::Sftp {
                    self.refresh_sftp_local();
                }
            }
            Message::QuickHostInput(v) => {
                self.quick_host_input = v;
            }
            Message::OpenGroup(gid) => {
                self.active_group = Some(gid);
                self.host_search.clear();
            }
            Message::BackToRoot => {
                self.active_group = None;
                self.host_search.clear();
            }
            Message::HostSearchChanged(v) => {
                self.host_search = v;
            }
            Message::ToggleSidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
            }
            Message::QuickHostContinue => {
                if !self.quick_host_input.is_empty() {
                    self.editor_form = ConnectionForm::default();
                    self.editor_form.hostname = self.quick_host_input.clone();
                    if let Some(gid) = self.active_group
                        && let Some(g) = self.groups.iter().find(|g| g.id == gid)
                    {
                        self.editor_form.group_name = g.label.clone();
                    }
                    self.show_host_panel = true;
                    self.host_panel_error = None;
                }
            }

            // -- Local shell --
            // -- Snippets --
            Message::ShowSnippetPanel => {
                self.show_snippet_panel = true;
                self.snippet_label.clear();
                self.snippet_command.clear();
                self.snippet_editing_id = None;
                self.snippet_error = None;
            }
            Message::HideSnippetPanel => {
                self.show_snippet_panel = false;
            }
            Message::SnippetLabelChanged(v) => self.snippet_label = v,
            Message::SnippetCommandChanged(v) => self.snippet_command = v,
            Message::EditSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    self.show_snippet_panel = true;
                    self.snippet_label = snip.label.clone();
                    self.snippet_command = snip.command.clone();
                    self.snippet_editing_id = Some(snip.id);
                    self.snippet_error = None;
                }
            }
            Message::SaveSnippet => {
                if self.snippet_label.is_empty() || self.snippet_command.is_empty() {
                    self.snippet_error = Some("Label and command are required".into());
                    return Task::none();
                }
                let mut snip = if let Some(id) = self.snippet_editing_id {
                    self.snippets.iter().find(|s| s.id == id).cloned()
                        .unwrap_or_else(|| oryxis_core::models::snippet::Snippet::new("", ""))
                } else {
                    oryxis_core::models::snippet::Snippet::new("", "")
                };
                snip.label = self.snippet_label.clone();
                snip.command = self.snippet_command.clone();
                if let Some(vault) = &self.vault {
                    match vault.save_snippet(&snip) {
                        Ok(()) => {
                            self.show_snippet_panel = false;
                            self.snippet_error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => self.snippet_error = Some(e.to_string()),
                    }
                }
            }
            Message::DeleteSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let id = snip.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_snippet(&id);
                        self.show_snippet_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }
            Message::RunSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let cmd = format!("{}\n", snip.command);
                    if let Some(tab_idx) = self.active_tab
                        && let Some(tab) = self.tabs.get(tab_idx) {
                            if let Some(ref ssh) = tab.active().ssh_session {
                                let _ = ssh.write(cmd.as_bytes());
                            } else if let Ok(mut state) = tab.active().terminal.lock() {
                                state.write(cmd.as_bytes());
                            }
                        }
                }
            }

            // -- Known hosts --
            Message::DeleteKnownHost(idx) => {
                if let Some(kh) = self.known_hosts.get(idx) {
                    let id = kh.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_known_host(&id);
                        self.load_data_from_vault();
                    }
                }
            }
            Message::ClearAllKnownHosts => {
                if let Some(vault) = &self.vault {
                    for kh in self.known_hosts.clone() {
                        let _ = vault.delete_known_host(&kh.id);
                    }
                    self.load_data_from_vault();
                }
            }

            // -- History --
            Message::ClearLogs => {
                if let Some(vault) = &self.vault {
                    let _ = vault.clear_logs();
                    self.logs_page = 0;
                    self.load_data_from_vault();
                }
            }
            Message::LogsPageNext => {
                let max_page = (self.logs_total.saturating_sub(1)) / 50;
                if self.logs_page < max_page {
                    self.logs_page += 1;
                    if let Some(vault) = &self.vault {
                        self.logs = vault
                            .list_logs_page(self.logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            Message::LogsPagePrev => {
                if self.logs_page > 0 {
                    self.logs_page -= 1;
                    if let Some(vault) = &self.vault {
                        self.logs = vault
                            .list_logs_page(self.logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            Message::ViewSessionLog(log_id) => {
                if let Some(vault) = &self.vault
                    && let Ok(Some(data)) = vault.get_session_data(&log_id) {
                        let rendered = strip_ansi(&data);
                        self.viewing_session_log = Some((log_id, rendered));
                }
            }
            Message::CloseSessionLogView => {
                self.viewing_session_log = None;
            }
            Message::DeleteSessionLog(idx) => {
                if let Some(entry) = self.session_logs.get(idx) {
                    let id = entry.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_session_log(&id);
                        self.session_logs = vault.list_session_logs().unwrap_or_default();
                    }
                }
                // Close viewer if we deleted the one being viewed
                if let Some((viewed_id, _)) = &self.viewing_session_log
                    && self.session_logs.iter().all(|s| s.id != *viewed_id) {
                        self.viewing_session_log = None;
                }
            }

            Message::OpenUrl(url) => {
                if let Err(e) = crate::util::open_in_browser(&url) {
                    tracing::warn!("open_in_browser({url}) failed: {e}");
                }
            }
            Message::CopyToClipboard(content) => {
                let mut ok = false;
                if let Ok(mut clip) = arboard::Clipboard::new() {
                    match clip.set_text(content) {
                        Ok(()) => ok = true,
                        Err(e) => tracing::warn!("clipboard set_text failed: {e}"),
                    }
                }
                if ok {
                    self.toast = Some(crate::i18n::t("copied_to_clipboard").to_string());
                    return Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                        },
                        |_| Message::ToastClear,
                    );
                }
            }
            Message::ToastClear => {
                self.toast = None;
            }

            // ── Vault password management ──
            Message::ToggleVaultPassword => {
                if self.vault_has_user_password {
                    // Remove password
                    if let Some(vault) = &mut self.vault {
                        match vault.remove_user_password() {
                            Ok(()) => {
                                self.vault_has_user_password = false;
                                self.vault_password_error = None;
                                self.vault_new_password.clear();
                            }
                            Err(e) => {
                                self.vault_password_error = Some(e.to_string());
                            }
                        }
                    }
                } else {
                    // Show password input (don't do anything yet, user needs to type and confirm)
                    self.vault_new_password.clear();
                    self.vault_password_error = None;
                }
            }
            Message::VaultNewPasswordChanged(pw) => {
                self.vault_new_password = pw;
            }
            Message::SetVaultPassword => {
                if self.vault_new_password.len() < 4 {
                    self.vault_password_error = Some("Password must be at least 4 characters".into());
                    return Task::none();
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_user_password(&self.vault_new_password) {
                        Ok(()) => {
                            self.vault_has_user_password = true;
                            self.vault_password_error = None;
                            self.vault_new_password.clear();
                        }
                        Err(e) => {
                            self.vault_password_error = Some(e.to_string());
                        }
                    }
                }
            }

            // ── MCP ──
            Message::EditorToggleMcpEnabled => {
                self.editor_form.mcp_enabled = !self.editor_form.mcp_enabled;
            }
            Message::EditorToggleAgentForwarding => {
                self.editor_form.agent_forwarding = !self.editor_form.agent_forwarding;
            }
            Message::EditorAddPortForward => {
                self.editor_form.port_forwards.push(PortForwardForm::default());
            }
            Message::EditorRemovePortForward(i) => {
                if i < self.editor_form.port_forwards.len() {
                    self.editor_form.port_forwards.remove(i);
                }
            }
            Message::EditorPortFwdLocalPortChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.local_port = v;
                }
            }
            Message::EditorPortFwdRemoteHostChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.remote_host = v;
                }
            }
            Message::EditorPortFwdRemotePortChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.remote_port = v;
                }
            }
            Message::ToggleMcpServer => {
                self.mcp_server_enabled = !self.mcp_server_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("mcp_server_enabled", if self.mcp_server_enabled { "true" } else { "false" });
                }
            }
            Message::ShowMcpInfo => {
                self.show_mcp_info = true;
                self.mcp_config_copied = false;
            }
            Message::HideMcpInfo => {
                self.show_mcp_info = false;
                self.mcp_config_copied = false;
            }
            Message::CopyMcpConfig => {
                self.mcp_config_copied = true;
                return iced::clipboard::write(mcp_config_json()).discard();
            }
            Message::InstallMcpConfig => {
                self.mcp_install_status = None;
                return Task::perform(
                    async { install_mcp_config_to_file() },
                    Message::InstallMcpConfigResult,
                );
            }
            Message::InstallMcpConfigResult(result) => {
                self.mcp_install_status = Some(result);
            }

            // ── Sync ──
            Message::SyncToggleEnabled => {
                self.sync_enabled = !self.sync_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_enabled", if self.sync_enabled { "true" } else { "false" });
                }
            }
            Message::SyncModeChanged(v) => {
                self.sync_mode = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_mode", &v);
                }
            }
            Message::SyncDeviceNameChanged(v) => {
                self.sync_device_name = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_device_name", &v);
                }
            }
            Message::SyncSignalingUrlChanged(v) => {
                self.sync_signaling_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_url", &v);
                }
            }
            Message::SyncRelayUrlChanged(v) => {
                self.sync_relay_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_relay_url", &v);
                }
            }
            Message::SyncListenPortChanged(v) => {
                self.sync_listen_port = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_listen_port", &v);
                }
            }
            Message::SyncStartPairing => {
                let code = oryxis_sync::crypto::generate_pairing_code();
                self.sync_pairing_code = Some(code);
            }
            Message::SyncUnpairDevice(peer_id) => {
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_sync_peer(&peer_id);
                    self.sync_peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            Message::SyncNow => {
                self.sync_status = Some("Sync triggered...".into());
            }

            // Anything not handled above was claimed by one of the
            // domain handlers in the `try_handler!` chain above. Any
            // variant reaching here means we forgot to claim it; treat
            // as a no-op so we don't crash on an unclaimed message.
            _ => {}
        }
        Task::none()
    }
}
