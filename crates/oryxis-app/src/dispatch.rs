//! `Oryxis::update`, the master message-dispatch table. ~5k lines of
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
use crate::mcp::{
    install_mcp_config_to_file, install_mcp_config_to_wsl, mcp_config_json, mcp_config_json_wsl,
};
use crate::state::{ConnectionForm, EnvVarForm, PortForwardForm, VaultState, View};
use crate::util::strip_ansi;

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

        match message {
            // -- Vault --
            Message::VaultPasswordChanged(pw) => {
                self.vault_password_input = pw;
            }
            Message::VaultTogglePasswordVisibility => {
                self.vault_password_visible = !self.vault_password_visible;
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
                            self.vault_password_visible = false;
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
                            self.vault_password_visible = false;
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
                            self.vault_password_visible = false;
                            self.load_data_from_vault();
                            // Bring the sync engine up now that the
                            // vault is open, if the user left it on.
                            let sync_task = if self.sync_enabled {
                                self.start_sync_engine()
                            } else {
                                Task::none()
                            };
                            // Auto-start port forward rules now that the
                            // vault (and its credentials) is open.
                            let mut unlock_tasks = vec![sync_task];
                            unlock_tasks.extend(self.auto_start_port_forwards());
                            // After a manual unlock, fire any deferred
                            // `--connect <uuid>` from the launch CLI args.
                            if let Some(connect_id) = self.pending_auto_connect.take()
                                && let Some(idx) = self
                                    .connections
                                    .iter()
                                    .position(|c| c.id == connect_id)
                            {
                                unlock_tasks.push(Task::done(Message::ConnectSsh(idx)));
                            }
                            return Task::batch(unlock_tasks);
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
                // Navigating away from the Shortcuts editor cancels
                // any pending capture so the next keystroke doesn't
                // silently rebind an action from another screen.
                self.editing_hotkey = None;
                // Known Hosts moved into Settings in v0.7; rewrite the
                // request so older callers (and any persisted state)
                // land on the right place instead of an unreachable
                // top-level view.
                if view == View::KnownHosts {
                    self.active_view = View::Settings;
                    self.settings_section = crate::state::SettingsSection::KnownHosts;
                } else {
                    self.active_view = view;
                }
                self.active_tab = None;
                // Burger menu auto-dismisses on navigation: the user
                // picked a destination, leaving the overlay open is
                // visual noise.
                self.show_burger_menu = false;
                // Lazy-load the local SFTP pane when the user first lands
                // on the view (or returns to it after the underlying dir
                // changed). Cheap enough to redo unconditionally.
                if view == View::Sftp {
                    // Refresh whichever pane(s) are Local; remote panes
                    // ignore this (refresh_sftp_local early-returns).
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Right);
                }
            }
            Message::QuickHostInput(v) => {
                self.quick_host_input = v;
            }
            Message::OpenGroup(gid) => {
                self.active_group = Some(gid);
                self.host_search.clear();
                // Auto-trigger resolve when the user opens a dynamic
                // group, saves an extra click. Re-resolve when there's
                // no cache yet, or when the cached list has gone stale
                // (older than the TTL): cloud resources like ECS tasks
                // recycle, and a stale list means clicking a dead task
                // fails until a manual Refresh. A still-`Loading` or
                // `Failed` cache is left alone (don't restart in-flight
                // resolves; let the user retry a failure explicitly).
                if self.dynamic_group_needs_resolve(gid) {
                    return self
                        .handle_cloud(Message::DynamicGroupResolve(gid))
                        .unwrap_or_else(|_| Task::none());
                }
            }
            Message::BackToRoot => {
                self.active_group = None;
                self.host_search.clear();
            }
            Message::HostSearchChanged(v) => {
                self.host_search = v;
            }
            Message::HostFilterByCloudProfile(maybe_pid) => {
                self.host_filter_cloud_profile = maybe_pid;
            }
            Message::ToggleGroupPicker(target) => {
                use crate::state::{GroupPickerTarget, OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::GroupPicker(t)) if *t == target
                );
                if already_open {
                    self.overlay = None;
                } else {
                    let bounds = match target {
                        GroupPickerTarget::EditorParent => {
                            self.editor_parent_combo_bounds.get()
                        }
                        GroupPickerTarget::DynamicFormParent => {
                            self.dynamic_form_parent_combo_bounds.get()
                        }
                        GroupPickerTarget::SessionGroupFolder => {
                            self.session_group_folder_combo_bounds.get()
                        }
                    };
                    self.group_picker_search.clear();
                    // 6 px gap below the combo. Falls back to mouse
                    // coords if the cell hasn't been populated yet
                    // (first ever open before any draw pass).
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::GroupPicker(target),
                        x: if bounds.width > 0.0 {
                            bounds.x
                        } else {
                            self.mouse_position.x
                        },
                        y: if bounds.height > 0.0 {
                            bounds.y + bounds.height + 6.0
                        } else {
                            self.mouse_position.y + 26.0
                        },
                    });
                }
            }
            Message::GroupPickerSearchChanged(v) => {
                self.group_picker_search = v;
            }
            Message::GroupPickerPick(target, label) => {
                use crate::state::{GroupPickerTarget, OverlayContent};
                match target {
                    GroupPickerTarget::EditorParent => {
                        self.editor_form.group_name = label;
                    }
                    GroupPickerTarget::DynamicFormParent => {
                        self.cloud_dynamic_form_parent_label = label;
                    }
                    GroupPickerTarget::SessionGroupFolder => {
                        self.editor_session_group.group_name = label;
                    }
                }
                if matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::GroupPicker(_))
                ) {
                    self.overlay = None;
                }
            }
            Message::ToggleSortMenu(kind) => {
                use crate::state::{OverlayContent, OverlayState, SortMenuKind};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::SortMenu(k)) if *k == kind
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor the dropdown to the trailing edge of the
                    // toolbar, just under the button row, matching the
                    // keychain "+ ADD" menu geometry. Pre-compensate for
                    // the panel-on-the-right footprint per view so the
                    // menu's right edge always lands at the visible
                    // content's right edge.
                    let panel_width = match kind {
                        SortMenuKind::Hosts => {
                            if self.show_host_panel { crate::app::PANEL_WIDTH } else { 0.0 }
                        }
                        SortMenuKind::Keys => {
                            if self.show_key_panel || self.show_identity_panel {
                                crate::app::PANEL_WIDTH
                            } else {
                                0.0
                            }
                        }
                        SortMenuKind::Snippets => {
                            if self.show_snippet_panel { crate::app::PANEL_WIDTH } else { 0.0 }
                        }
                    };
                    // Must match the `OverlayContent::SortMenu` width
                    // in `views/layout.rs` so the dropdown lands under
                    // the trigger button instead of being shifted.
                    let menu_width = 220.0_f32;
                    let toolbar_padding = 24.0_f32;
                    let x = if crate::i18n::is_rtl_layout() {
                        panel_width + toolbar_padding + menu_width
                    } else {
                        self.window_size.width
                            - panel_width
                            - toolbar_padding
                            - menu_width
                    };
                    let y = self.dashboard_dropdown_anchor_y();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SortMenu(kind),
                        x: x.max(0.0),
                        y,
                    });
                }
            }
            Message::SetListSort(kind, sort) => {
                use crate::state::SortMenuKind;
                // Selecting from the sidebar's own sort popover dismisses it
                // (harmless for the workspace overlay, which closes itself).
                self.sidebar_sort_open = false;
                let key = match kind {
                    SortMenuKind::Hosts => {
                        self.hosts_sort = sort;
                        "hosts_sort"
                    }
                    SortMenuKind::Keys => {
                        self.keys_sort = sort;
                        "keys_sort"
                    }
                    SortMenuKind::Snippets => {
                        self.snippets_sort = sort;
                        "snippets_sort"
                    }
                };
                if let Some(v) = &self.vault {
                    if let Err(e) = v.set_setting(key, sort.as_storage_str()) {
                        eprintln!("[sort] failed to persist {key}: {e}");
                    }
                }
                self.overlay = None;
            }
            Message::ToggleSidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
            }
            Message::QuickHostContinue => {
                if !self.quick_host_input.is_empty() {
                    self.editor_form = ConnectionForm::default();
                    self.editor_initial_command =
                        iced::widget::text_editor::Content::new();
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
                self.snippet_command = iced::widget::text_editor::Content::new();
                self.snippet_editing_id = None;
                self.snippet_error = None;
            }
            Message::HideSnippetPanel => {
                self.show_snippet_panel = false;
            }
            Message::SnippetLabelChanged(v) => self.snippet_label = v,
            Message::SnippetCommandAction(action) => self.snippet_command.perform(action),
            Message::ShowSnippetMenu(idx) => {
                use crate::state::{OverlayContent, OverlayState};
                // Toggle: clicking the kebab again (or on the same card)
                // dismisses the popup, mirroring the host-card menu.
                if self.snippet_context_menu == Some(idx) {
                    self.snippet_context_menu = None;
                    self.overlay = None;
                } else {
                    self.snippet_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SnippetActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::EditSnippet(idx) => {
                // Reached from the card kebab menu, close the popup.
                self.snippet_context_menu = None;
                self.overlay = None;
                if let Some(snip) = self.snippets.get(idx) {
                    self.show_snippet_panel = true;
                    self.snippet_label = snip.label.clone();
                    self.snippet_command =
                        iced::widget::text_editor::Content::with_text(&snip.command);
                    self.snippet_editing_id = Some(snip.id);
                    self.snippet_error = None;
                }
            }
            Message::SaveSnippet => {
                if self.snippet_label.is_empty() || self.snippet_command.text().trim().is_empty() {
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
                snip.command = self.snippet_command.text().trim_end().to_string();
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
                // Reached from the card kebab menu or the edit panel,
                // close the popup either way.
                self.snippet_context_menu = None;
                self.overlay = None;
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
                    let cmd = snip.command.clone();
                    if let Some(tab_idx) = self.snippet_injection_tab()
                        && let Some(tab) = self.tabs.get(tab_idx)
                    {
                        // Bracket the body (so a multi-line snippet inserts as
                        // one block under bracketed paste), then append the
                        // submit newline OUTSIDE the bracket so it runs once.
                        // With the mode off this collapses to the old
                        // `command\n` raw write.
                        let bracketed = tab
                            .active()
                            .terminal
                            .lock()
                            .map(|s| s.bracketed_paste_enabled())
                            .unwrap_or(false);
                        let mut payload = oryxis_terminal::wrap_paste(&cmd, bracketed);
                        payload.push(b'\n');
                        if let Some(ref ssh) = tab.active().ssh_session {
                            let _ = ssh.write(&payload);
                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                            state.write(&payload);
                        }
                    }
                }
            }
            Message::ApplySudoPassword => {
                // Resolve the active terminal's connection by label, decrypt
                // its stored password, and type it + Enter. The password is
                // never logged (only PTY output is recorded, and sudo turns
                // echo off) nor shown in the toast.
                let toast_key = (|| {
                    let tab_idx = self.snippet_injection_tab()?;
                    let label = self.tabs.get(tab_idx)?.label.clone();
                    let conn_id = self
                        .connections
                        .iter()
                        .find(|c| c.label == label)
                        .map(|c| c.id)?;
                    let pw = self
                        .vault
                        .as_ref()
                        .and_then(|v| v.get_connection_password(&conn_id).ok().flatten())
                        .filter(|p| !p.is_empty())?;
                    let data = format!("{pw}\n");
                    if let Some(tab) = self.tabs.get(tab_idx) {
                        if let Some(ref ssh) = tab.active().ssh_session {
                            let _ = ssh.write(data.as_bytes());
                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                            state.write(data.as_bytes());
                        }
                    }
                    Some("sudo_password_sent")
                })()
                .unwrap_or("no_stored_password");
                self.toast = Some(crate::i18n::t(toast_key).to_string());
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                    },
                    |_| Message::ToastClear,
                );
            }
            Message::PasteSnippet(idx) => {
                // Same injection path as RunSnippet, but without the trailing
                // newline so the user reviews and presses Enter themselves.
                if let Some(snip) = self.snippets.get(idx) {
                    let cmd = snip.command.clone();
                    if let Some(tab_idx) = self.snippet_injection_tab()
                        && let Some(tab) = self.tabs.get(tab_idx)
                    {
                        // Wrap in bracketed paste when the focused app asked
                        // for it, so a multi-line snippet inserts as one block
                        // instead of auto-submitting on every embedded newline.
                        let bracketed = tab
                            .active()
                            .terminal
                            .lock()
                            .map(|s| s.bracketed_paste_enabled())
                            .unwrap_or(false);
                        let payload = oryxis_terminal::wrap_paste(&cmd, bracketed);
                        if let Some(ref ssh) = tab.active().ssh_session {
                            let _ = ssh.write(&payload);
                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                            state.write(&payload);
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
            // Clear now wipes both feeds the unified History timeline
            // mixes (failed-connect log rows + recorded session rows)
            // so the user gets a true "empty list" instead of seeing
            // every previously recorded session reappear after the
            // wipe finishes.
            Message::ClearLogs => {
                if let Some(vault) = &self.vault {
                    let _ = vault.clear_logs();
                    let _ = vault.clear_session_logs();
                    self.logs_page = 0;
                    self.session_logs_page = 0;
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
                        self.session_logs_total =
                            vault.count_session_logs().unwrap_or(0);
                        // Stepping a page back when the current one is now
                        // empty keeps the prev/next pair from leaving the
                        // user staring at "0 of N" with rows further back.
                        let max_page = self
                            .session_logs_total
                            .saturating_sub(1)
                            / 50;
                        if self.session_logs_page > max_page {
                            self.session_logs_page = max_page;
                        }
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
                // Close viewer if we deleted the one being viewed
                if let Some((viewed_id, _)) = &self.viewing_session_log
                    && self.session_logs.iter().all(|s| s.id != *viewed_id) {
                        self.viewing_session_log = None;
                }
            }
            Message::ClearSessionLogs => {
                if let Some(vault) = &self.vault {
                    let _ = vault.clear_session_logs();
                    self.session_logs_page = 0;
                    self.load_data_from_vault();
                }
                self.viewing_session_log = None;
            }
            Message::SessionLogsPageNext => {
                let max_page = self.session_logs_total.saturating_sub(1) / 50;
                if self.session_logs_page < max_page {
                    self.session_logs_page += 1;
                    if let Some(vault) = &self.vault {
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            Message::SessionLogsPagePrev => {
                if self.session_logs_page > 0 {
                    self.session_logs_page -= 1;
                    if let Some(vault) = &self.vault {
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
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
            Message::ErrorDialogDismiss => {
                self.error_dialog = None;
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
            Message::EditorAddEnvVar => {
                self.editor_form.env_vars.push(EnvVarForm::default());
            }
            Message::EditorRemoveEnvVar(i) => {
                if i < self.editor_form.env_vars.len() {
                    self.editor_form.env_vars.remove(i);
                }
            }
            Message::EditorEnvVarKeyChanged(i, v) => {
                if let Some(e) = self.editor_form.env_vars.get_mut(i) {
                    e.key = v;
                }
            }
            Message::EditorEnvVarValueChanged(i, v) => {
                if let Some(e) = self.editor_form.env_vars.get_mut(i) {
                    e.value = v;
                }
            }
            Message::ToggleMcpServer => {
                self.mcp_server_enabled = !self.mcp_server_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("mcp_server_enabled", if self.mcp_server_enabled { "true" } else { "false" });
                }
                // MCP ships as a plugin (~5 MB binary external clients
                // like Claude Desktop spawn). First-time enable triggers
                // the install modal; an already-installed plugin or a
                // dev binary on the side both make this a no-op.
                if self.mcp_server_enabled
                    && !crate::mcp_install::is_installed()
                    && !crate::dispatch_plugins::dev_binary_present("mcp")
                {
                    return Task::done(Message::ShowPluginInstallModal(
                        "mcp".to_string(),
                    ));
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
                let json = if self.mcp_target_wsl {
                    mcp_config_json_wsl(&self.mcp_server_token)
                } else {
                    mcp_config_json(&self.mcp_server_token)
                };
                return iced::clipboard::write(json).discard();
            }
            Message::InstallMcpConfig => {
                self.mcp_install_status = None;
                let token = self.mcp_server_token.clone();
                let wsl = self.mcp_target_wsl;
                return Task::perform(
                    async move {
                        if wsl {
                            install_mcp_config_to_wsl(&token)
                        } else {
                            install_mcp_config_to_file(&token)
                        }
                    },
                    Message::InstallMcpConfigResult,
                );
            }
            Message::SetMcpTarget(is_wsl) => {
                self.mcp_target_wsl = is_wsl;
                // The Copy / Install feedback from the previous target no
                // longer reflects what's on screen.
                self.mcp_config_copied = false;
                self.mcp_install_status = None;
            }
            Message::InstallMcpConfigResult(result) => {
                self.mcp_install_status = Some(result);
            }
            Message::RegenerateMcpToken => {
                use rand::RngCore;
                let mut bytes = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut bytes);
                let mut token = String::with_capacity(64);
                for b in bytes {
                    use std::fmt::Write as _;
                    let _ = write!(token, "{b:02x}");
                }
                self.persist_setting("mcp_server_token", &token);
                self.mcp_server_token = token;
                // Reveal once after regenerating so the user can copy
                // it without an extra click; flip it back to masked
                // explicitly via `ToggleMcpTokenVisibility`.
                self.mcp_token_visible = true;
                // The Claude config on disk still carries the old
                // token, prompt the user to re-install.
                self.mcp_install_status = None;
            }
            Message::ToggleMcpTokenVisibility => {
                self.mcp_token_visible = !self.mcp_token_visible;
            }
            Message::CopyMcpToken => {
                return iced::clipboard::write(self.mcp_server_token.clone()).discard();
            }

            // ── Sync ──
            Message::SyncToggleEnabled => {
                self.sync_enabled = !self.sync_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_enabled", if self.sync_enabled { "true" } else { "false" });
                }
                if self.sync_enabled {
                    return self.start_sync_engine();
                }
                self.stop_sync_engine();
                self.sync_status = Some(crate::i18n::t("sync_status_stopped").to_string());
            }
            Message::SyncTogglePasswords => {
                self.sync_passwords = !self.sync_passwords;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting(
                        "sync_passwords",
                        if self.sync_passwords { "true" } else { "false" },
                    );
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
            Message::SyncSignalingTokenChanged(v) => {
                self.sync_signaling_token = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_token", &v);
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
                // Host a real pairing code on the engine. The engine
                // also emits `PairingCodeGenerated`, but we set the
                // code + state here directly so the UI flips instantly.
                if let Some(runtime) = &self.sync_runtime {
                    let handle = runtime.handle();
                    let code = handle.start_hosting_pairing();
                    let link = handle.pairing_link(&code);
                    self.sync_pairing_link = Some(link);
                    self.sync_pairing_code = Some(code);
                    self.sync_pairing_state = crate::state::SyncPairingState::Hosting;
                } else {
                    self.sync_status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                }
            }
            Message::SyncCancelHostingPairing => {
                if let Some(runtime) = &self.sync_runtime {
                    runtime.handle().cancel_hosting_pairing();
                }
                self.sync_pairing_code = None;
                self.sync_pairing_link = None;
                self.sync_pairing_state = crate::state::SyncPairingState::Idle;
            }
            Message::SyncJoinPairingRequested => {
                self.sync_pairing_state = crate::state::SyncPairingState::Joining;
                self.sync_join_code_input.clear();
                self.sync_join_target_input.clear();
                self.sync_join_link_input.clear();
            }
            Message::SyncJoinCodeChanged(v) => {
                self.sync_join_code_input = v;
            }
            Message::SyncJoinTargetChanged(v) => {
                self.sync_join_target_input = v;
            }
            Message::SyncJoinLinkChanged(v) => {
                self.sync_join_link_input = v;
            }
            Message::SyncJoinPairingCancel => {
                self.sync_pairing_state = crate::state::SyncPairingState::Idle;
            }
            Message::SyncPairWithDiscovered(device_id) => {
                if let Some(peer) = self
                    .sync_discovered
                    .iter()
                    .find(|p| p.device_id == device_id)
                {
                    self.sync_pairing_state = crate::state::SyncPairingState::Joining;
                    self.sync_join_code_input.clear();
                    self.sync_join_link_input.clear();
                    self.sync_join_target_input = peer.addr.to_string();
                }
            }
            Message::SyncJoinPairingByLink => {
                let Some(runtime) = &self.sync_runtime else {
                    self.sync_status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                    return Task::none();
                };
                let link = self.sync_join_link_input.trim().to_string();
                if oryxis_sync::parse_pairing_link(&link).is_none() {
                    self.sync_status = Some(
                        crate::i18n::t("sync_pairing_bad_link").to_string(),
                    );
                    return Task::none();
                }
                let handle = runtime.handle();
                // Keep at Joining so the inline status + form stay
                // visible; the PairingCompleted / PairingFailed event
                // handler decides whether to drop back to Idle.
                self.sync_status =
                    Some(crate::i18n::t("sync_pairing_connecting").to_string());
                return Task::perform(
                    async move {
                        let _ = handle.join_pairing_remote(&link).await;
                    },
                    |()| Message::NoOp,
                );
            }
            Message::SyncJoinPairingConnect => {
                let Some(runtime) = &self.sync_runtime else {
                    self.sync_status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                    return Task::none();
                };
                let code = self.sync_join_code_input.trim().to_string();
                if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
                    self.sync_status =
                        Some(crate::i18n::t("sync_pairing_invalid_code").to_string());
                    return Task::none();
                }
                let addr: std::net::SocketAddr =
                    match self.sync_join_target_input.trim().parse() {
                        Ok(a) => a,
                        Err(_) => {
                            self.sync_status = Some(
                                crate::i18n::t("sync_pairing_bad_address").to_string(),
                            );
                            return Task::none();
                        }
                    };
                let handle = runtime.handle();
                // Keep at Joining so the inline status + form stay
                // visible while the handshake runs; the PairingCompleted
                // event flips back to Idle, PairingFailed stays put so
                // the user can fix the code/addr and retry.
                self.sync_status =
                    Some(crate::i18n::t("sync_pairing_connecting").to_string());
                // join_pairing emits PairingCompleted / PairingFailed,
                // which the SyncEngineEvent arm turns into UI state.
                return Task::perform(
                    async move {
                        let _ = handle.join_pairing(addr, code).await;
                    },
                    |()| Message::NoOp,
                );
            }
            Message::SyncUnpairDevice(peer_id) => {
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_sync_peer(&peer_id);
                    self.sync_peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            Message::SyncNow => {
                if self.sync_in_progress {
                    // Defensive: shouldn't fire because the UI swaps
                    // Sync Now for Cancel while a sync is running,
                    // but if a stray click does land, ignore it.
                    return Task::none();
                }
                if let Some(runtime) = &self.sync_runtime {
                    let handle = runtime.handle();
                    let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
                    self.sync_abort_tx = Some(abort_tx);
                    self.sync_in_progress = true;
                    self.sync_status =
                        Some(crate::i18n::t("sync_status_syncing").to_string());
                    // Race the sync against a 90s timeout AND the
                    // abort channel. Whichever fires first wins; the
                    // sync future is dropped, which closes the QUIC
                    // connection mid-handshake (quinn cleans up).
                    return Task::perform(
                        async move {
                            tokio::select! {
                                r = tokio::time::timeout(
                                    std::time::Duration::from_secs(90),
                                    handle.sync_now(),
                                ) => match r {
                                    Ok(Ok(())) => Ok(()),
                                    Ok(Err(e)) => Err(format!("{e}")),
                                    Err(_) => Err("__timeout__".into()),
                                },
                                _ = abort_rx => Err("__cancelled__".into()),
                            }
                        },
                        Message::SyncNowFinished,
                    );
                }
                self.sync_status =
                    Some(crate::i18n::t("sync_status_disabled").to_string());
            }
            Message::SyncCancelInProgress => {
                if let Some(tx) = self.sync_abort_tx.take() {
                    let _ = tx.send(());
                }
                // Don't clear `sync_in_progress` here: the Task lands
                // back as `SyncNowFinished(Err("__cancelled__"))` and
                // clears it there, so the Cancel button stays visible
                // until the cancellation actually settles.
            }
            Message::SyncNowFinished(result) => {
                self.sync_in_progress = false;
                self.sync_abort_tx = None;
                match result {
                    Ok(()) => {}
                    Err(e) if e == "__cancelled__" => {
                        self.sync_status = Some(
                            crate::i18n::t("sync_status_cancelled").to_string(),
                        );
                    }
                    Err(e) if e == "__timeout__" => {
                        self.sync_status = Some(
                            crate::i18n::t("sync_status_timeout").to_string(),
                        );
                    }
                    Err(e) => {
                        self.sync_status = Some(format!(
                            "{}: {e}",
                            crate::i18n::t("sync_status_failed"),
                        ));
                    }
                }
                // Per-peer outcomes already arrived as SyncEngineEvent;
                // refresh the peer list so last_synced_at is current.
                if let Some(vault) = &self.vault {
                    self.sync_peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            Message::SyncEngineEvent(event) => {
                use oryxis_sync::SyncEvent;
                match event {
                    SyncEvent::PeerDiscovered { device_id, device_name, addr, .. } => {
                        // Dedup by device_id: an mDNS browse can
                        // republish the same peer (different network
                        // interface, restart, etc.). Last writer wins
                        // on the address so a roaming peer's entry
                        // tracks its new ip:port.
                        let info = crate::state::DiscoveredPeerInfo {
                            device_id,
                            device_name,
                            addr,
                        };
                        if let Some(existing) = self
                            .sync_discovered
                            .iter_mut()
                            .find(|p| p.device_id == device_id)
                        {
                            *existing = info;
                        } else {
                            self.sync_discovered.push(info);
                        }
                    }
                    SyncEvent::PairingCodeGenerated { code } => {
                        self.sync_pairing_code = Some(code);
                    }
                    SyncEvent::PairingCompleted { device_name, .. } => {
                        self.sync_status = Some(format!(
                            "{} {device_name}",
                            crate::i18n::t("sync_paired_with"),
                        ));
                        // Pairing done on either side: close the modal
                        // sub-view, drop the hosted code / link / QR,
                        // and refresh the peer list.
                        self.sync_pairing_state =
                            crate::state::SyncPairingState::Idle;
                        self.sync_pairing_code = None;
                        self.sync_pairing_link = None;
                        if let Some(vault) = &self.vault {
                            self.sync_peers =
                                vault.list_sync_peers().unwrap_or_default();
                        }
                    }
                    SyncEvent::PairingFailed { reason } => {
                        self.sync_status = Some(format!(
                            "{}: {reason}",
                            crate::i18n::t("sync_pairing_failed"),
                        ));
                        // Stay in whichever sub-view triggered the
                        // pairing so the user sees the error in
                        // context and can fix + retry without
                        // re-entering everything. Host-side: clear
                        // the code/link since the single-shot was
                        // consumed even on failure.
                        if self.sync_pairing_state
                            == crate::state::SyncPairingState::Hosting
                        {
                            self.sync_pairing_code = None;
                            self.sync_pairing_link = None;
                            self.sync_pairing_state =
                                crate::state::SyncPairingState::Idle;
                        }
                    }
                    SyncEvent::SyncStarted { .. } => {
                        self.sync_status =
                            Some(crate::i18n::t("sync_status_syncing").to_string());
                    }
                    SyncEvent::SyncCompleted { pushed, pulled, .. } => {
                        self.sync_status = Some(format!(
                            "{} (+{pushed} / -{pulled})",
                            crate::i18n::t("sync_status_done"),
                        ));
                        if let Some(vault) = &self.vault {
                            self.sync_peers =
                                vault.list_sync_peers().unwrap_or_default();
                        }
                    }
                    SyncEvent::SyncFailed { error, .. } => {
                        self.sync_status = Some(format!(
                            "{}: {error}",
                            crate::i18n::t("sync_status_failed"),
                        ));
                    }
                    SyncEvent::PeerOnline { .. } | SyncEvent::PeerOffline { .. } => {}
                    SyncEvent::SignalingRegistered { ip, port } => {
                        // Confirms cross-network pairing is reachable
                        // at this address. Until this fires the host is
                        // LAN-only (or signaling failed silently). The
                        // `(n)` counter bumps on every refresh so the
                        // user sees heartbeats land even when the IP
                        // is stable.
                        self.sync_signaling_tick =
                            self.sync_signaling_tick.saturating_add(1);
                        self.sync_status = Some(format!(
                            "{} ({}): {ip}:{port}",
                            crate::i18n::t("sync_status_signaling_registered"),
                            self.sync_signaling_tick,
                        ));
                    }
                    SyncEvent::SignalingFailed { reason } => {
                        self.sync_status = Some(format!(
                            "{}: {reason}",
                            crate::i18n::t("sync_status_signaling_failed"),
                        ));
                    }
                    SyncEvent::VersionMismatch {
                        peer_version,
                        local_version,
                        ..
                    } => {
                        self.sync_status = Some(format!(
                            "{}: peer v{peer_version}, local v{local_version}",
                            crate::i18n::t("sync_status_version_mismatch"),
                        ));
                    }
                    SyncEvent::PeerStaleWarning { days_since_sync, .. } => {
                        self.sync_status = Some(format!(
                            "{} ({}d)",
                            crate::i18n::t("sync_status_peer_stale"),
                            days_since_sync,
                        ));
                    }
                }
            }

            // -- System tray --
            Message::TrayPoll => {
                // Rebuild the dynamic submenu (Active sessions +
                // Recent hosts) when the state behind it changed.
                // Signature is a hash of the tab count + connection
                // last_used times; cheap enough to recompute every
                // 100 ms and skips the actual rebuild on no-change.
                {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};

                    let mut h = DefaultHasher::new();
                    self.tabs.len().hash(&mut h);
                    for t in &self.tabs {
                        t.label.hash(&mut h);
                    }
                    self.connections.len().hash(&mut h);
                    for c in &self.connections {
                        c.id.hash(&mut h);
                        c.last_used.map(|d| d.timestamp_millis()).hash(&mut h);
                    }
                    // Fold the IPC registry into the signature so a
                    // child going hidden / changing title triggers
                    // a primary menu rebuild on the next tick. Cheap:
                    // list_instances does one dir scan + PID liveness
                    // check per entry, which on a typical setup means
                    // <5 file reads.
                    let ipc_instances = crate::tray_ipc::Primary::list_instances();
                    ipc_instances.len().hash(&mut h);
                    for inst in &ipc_instances {
                        inst.pid.hash(&mut h);
                        inst.is_hidden.hash(&mut h);
                        inst.title.hash(&mut h);
                    }
                    let sig = h.finish();
                    if sig != self.tray_menu_signature {
                        self.tray_menu_signature = sig;
                        // `&` is the Windows menu accelerator prefix:
                        // a host named "R&D" would render as "RD" with
                        // D underlined. Doubling the `&` escapes it.
                        // Capped at 20: a user with 50+ open tabs gets
                        // an unwieldy submenu otherwise; recent-hosts
                        // submenu already had a `.take(10)` for the
                        // same reason.
                        let active: Vec<(String, String)> = self
                            .tabs
                            .iter()
                            .enumerate()
                            .take(20)
                            .map(|(i, t)| (t.label.replace('&', "&&"), i.to_string()))
                            .collect();
                        // Recent hosts: top 10 by last_used desc.
                        // Hosts that were never connected drop to
                        // the bottom and get sliced off, so the
                        // menu only lists hosts the user actually
                        // touched.
                        let mut recent_pairs: Vec<&oryxis_core::models::connection::Connection> =
                            self.connections.iter().filter(|c| c.last_used.is_some()).collect();
                        recent_pairs.sort_by_key(|c| std::cmp::Reverse(c.last_used));
                        let recent: Vec<(String, String)> = recent_pairs
                            .iter()
                            .take(10)
                            .map(|c| (c.label.replace('&', "&&"), c.id.to_string()))
                            .collect();
                        // Unified "Windows" list: every window the
                        // user owns that's currently hidden, primary
                        // first (when the primary itself is hidden)
                        // then each hidden child via the IPC registry.
                        // The id-suffix is the owning process's PID;
                        // the menu click dispatcher checks self_pid
                        // to decide between local TrayShow and an
                        // IPC send_command.
                        let mut hidden: Vec<(String, String)> = Vec::new();
                        if self.is_window_hidden {
                            let primary_label = self
                                .active_tab
                                .and_then(|i| self.tabs.get(i))
                                .map(|t| t.label.clone())
                                .unwrap_or_else(|| crate::i18n::t("tray_main_window").to_string());
                            hidden.push((
                                primary_label.replace('&', "&&"),
                                std::process::id().to_string(),
                            ));
                        }
                        for inst in crate::tray_ipc::Primary::list_instances() {
                            if !inst.is_hidden {
                                continue;
                            }
                            let label = if inst.title.is_empty() || inst.title == "Oryxis" {
                                format!("{} (PID {})", crate::i18n::t("tray_main_window"), inst.pid)
                            } else {
                                inst.title.clone()
                            };
                            hidden.push((label.replace('&', "&&"), inst.pid.to_string()));
                        }
                        if let Err(e) = crate::tray::rebuild_menu(&active, &recent, &hidden) {
                            tracing::warn!("tray menu rebuild failed: {e}");
                        }
                        // Tray icon is only visible when at least
                        // one window (primary's own or any child's)
                        // is currently hidden. The "1 tray to rule
                        // them all" UX the user asked for: when
                        // everything's visible on screen there's no
                        // reason to clutter the notification area
                        // with a redundant icon.
                        let any_hidden = self.is_window_hidden || !hidden.is_empty();
                        crate::tray::set_visible(any_hidden);
                    }
                }
                // Drain whatever the tray-icon crate's event threads
                // queued since the last poll. Each menu id resolves
                // to a real Message via Task::batch so we can emit
                // more than one event per tick if the user spam-
                // clicked. On non-Windows targets both polls return
                // None immediately, so this is harmless overhead.
                let mut follow_ups: Vec<Task<Message>> = Vec::new();

                // Push our state into the tray_ipc registry so the
                // primary's "Hidden windows" menu reflects any tab
                // label edits / new sessions / etc. between explicit
                // hide/show events. No-op for the primary itself.
                self.broadcast_ipc_state_if_child();

                // Drain whatever command the primary queued for us
                // (a Show or Quit from a click in its tray menu).
                // No-op for the primary process (it never has its
                // own command file because we skip self_pid in
                // Primary::list_instances).
                let is_primary = crate::app::APP_IS_PRIMARY
                    .load(std::sync::atomic::Ordering::Relaxed);
                if !is_primary {
                    while let Some(cmd) = crate::tray_ipc::Child::poll_command() {
                        match cmd {
                            crate::tray_ipc::Command::Show => {
                                follow_ups.push(Task::done(Message::TrayShow));
                            }
                            crate::tray_ipc::Command::Quit => {
                                follow_ups.push(Task::done(Message::TrayQuit));
                            }
                        }
                    }

                    // Promotion check: if the primary process
                    // exited (mutex released) one of the surviving
                    // children needs to take over so the user
                    // doesn't end up with orphaned hidden windows
                    // and no tray to surface them. try_acquire_mutex
                    // succeeds when nobody else owns the mutex; the
                    // first child to win the race becomes the new
                    // primary, installs the tray, and unregisters
                    // its own IPC row.
                    if crate::tray::try_acquire_mutex() {
                        tracing::info!("tray IPC: promoting to primary (old primary gone)");
                        crate::app::APP_IS_PRIMARY
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        if let Err(e) = crate::tray::install() {
                            tracing::warn!("tray install on promotion: {e}");
                        }
                        crate::tray_ipc::Child::unregister();
                    }
                }

                while let Some(id) = crate::tray::poll_menu_event() {
                    let msg = match id.as_str() {
                        crate::tray::MENU_ID_SHOW => Some(Message::TrayShow),
                        crate::tray::MENU_ID_HIDE => Some(Message::TrayHide),
                        crate::tray::MENU_ID_QUIT => Some(Message::TrayQuit),
                        s if s.starts_with(crate::tray::MENU_PREFIX_SESSION) => {
                            // "oryxis-tray-session:<idx>" -> activate
                            // that open tab. The dispatcher already
                            // has TabSelect plumbed through every code
                            // path that switches the active terminal.
                            let suffix = &s[crate::tray::MENU_PREFIX_SESSION.len()..];
                            suffix.parse::<usize>().ok().and_then(|idx| {
                                if idx < self.tabs.len() {
                                    Some(Message::TrayActivateSession(idx))
                                } else {
                                    None
                                }
                            })
                        }
                        s if s.starts_with(crate::tray::MENU_PREFIX_HOST) => {
                            // "oryxis-tray-host:<uuid>" -> open a new
                            // tab against that saved connection.
                            let suffix = &s[crate::tray::MENU_PREFIX_HOST.len()..];
                            uuid::Uuid::parse_str(suffix)
                                .ok()
                                .map(Message::TrayOpenHost)
                        }
                        s if s.starts_with(crate::tray::MENU_PREFIX_HIDDEN) => {
                            // "oryxis-tray-hidden:<pid>". If pid is
                            // our own, the menu item refers to the
                            // primary's own hidden window: fire
                            // TrayShow locally. Otherwise queue an
                            // IPC Show command for the child whose
                            // TrayPoll routes it back into TrayShow
                            // on its side.
                            let suffix = &s[crate::tray::MENU_PREFIX_HIDDEN.len()..];
                            if let Ok(pid) = suffix.parse::<u32>() {
                                if pid == std::process::id() {
                                    Some(Message::TrayShow)
                                } else {
                                    crate::tray_ipc::Primary::send_command(
                                        pid,
                                        crate::tray_ipc::Command::Show,
                                    );
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(m) = msg {
                        follow_ups.push(Task::done(m));
                    }
                }
                // Left-click on the tray icon body (not the menu)
                // counts as "Show". We drain but ignore the right-
                // click event; Windows already pops the menu on its
                // own for right-clicks via the registered Menu.
                #[cfg(target_os = "windows")]
                while let Some(ev) = crate::tray::poll_icon_event() {
                    if matches!(
                        ev,
                        tray_icon::TrayIconEvent::DoubleClick { .. }
                    ) {
                        follow_ups.push(Task::done(Message::TrayShow));
                    }
                }
                #[cfg(not(target_os = "windows"))]
                while crate::tray::poll_icon_event().is_some() {}

                if !follow_ups.is_empty() {
                    return Task::batch(follow_ups);
                }
            }
            Message::TrayShow => {
                // Hop through iced::window::oldest -> window::run so
                // we get the raw window handle on the UI thread. The
                // tray hide/show helpers swallow non-Windows targets
                // (stubs return false), so this is a no-op outside
                // Windows even though the code compiles everywhere.
                // `.discard()` drops the `()` return so the chain
                // matches the dispatcher's `Task<Message>` shape.
                self.is_window_hidden = false;
                self.broadcast_ipc_state_if_child();
                return iced::window::oldest()
                    .and_then(|id| {
                        iced::window::run(id, |window| {
                            crate::tray::show_window(window);
                        })
                    })
                    .discard();
            }
            Message::TrayHide => {
                self.is_window_hidden = true;
                self.broadcast_ipc_state_if_child();
                return iced::window::oldest()
                    .and_then(|id| {
                        iced::window::run(id, |window| {
                            crate::tray::hide_window(window);
                        })
                    })
                    .discard();
            }
            Message::TrayQuit => {
                tracing::info!("tray: quit requested");
                return iced::exit();
            }
            Message::TrayActivateSession(idx) => {
                // Show first (window may be hidden) then re-emit
                // SelectTab via Task::done. Bundled together so the
                // user sees the tab swap and the window pop in the
                // same frame.
                if idx < self.tabs.len() {
                    return Task::batch(vec![
                        Task::done(Message::TrayShow),
                        Task::done(Message::SelectTab(idx)),
                    ]);
                }
            }
            Message::TrayOpenHost(uuid) => {
                if let Some(idx) =
                    self.connections.iter().position(|c| c.id == uuid)
                {
                    return Task::batch(vec![
                        Task::done(Message::TrayShow),
                        Task::done(Message::ConnectSsh(idx)),
                    ]);
                }
            }

            // Anything not handled above was claimed by one of the
            // domain handlers in the `try_handler!` chain above. Any
            // variant reaching here means we forgot to claim it; treat
            // as a no-op so we don't crash on an unclaimed message.
            _ => {}
        }
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
