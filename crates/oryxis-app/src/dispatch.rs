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
use crate::state::{EnvVarForm, PortForwardForm, VaultState, View};

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

        match message {
            // -- Vault --
            Message::VaultPasswordChanged(pw) => {
                self.vault_ui.password_input = pw;
            }
            Message::VaultTogglePasswordVisibility => {
                self.vault_ui.password_visible = !self.vault_ui.password_visible;
            }
            Message::VaultSetup => {
                if self.vault_ui.password_input.len() < 4 {
                    self.vault_ui.error =
                        Some(crate::i18n::t("password_too_short").to_string());
                    return Task::none();
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_master_password(&self.vault_ui.password_input) {
                        Ok(()) => {
                            let _ = vault.set_setting("has_user_password", "1");
                            self.vault_ui.has_user_password = true;
                            self.vault_ui.state = VaultState::Unlocked;
                            self.vault_ui.error = None;
                            // Cache for child-window spawn.
                            self.master_password = Some(self.vault_ui.password_input.clone());
                            self.vault_ui.password_input.clear();
                            self.vault_ui.password_visible = false;
                            self.load_data_from_vault();
                            return iced::widget::operation::focus(iced::widget::Id::new(
                                "search-dashboard",
                            ));
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::VaultSkipPassword => {
                if let Some(vault) = &mut self.vault {
                    match vault.open_without_password() {
                        Ok(()) => {
                            self.vault_ui.state = VaultState::Unlocked;
                            self.vault_ui.error = None;
                            self.load_data_from_vault();
                            return iced::widget::operation::focus(iced::widget::Id::new(
                                "search-dashboard",
                            ));
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_ui.error = Some(
                                crate::i18n::t("vault_already_has_password").to_string(),
                            );
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(format!("Failed to create vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultDestroyConfirm => {
                self.vault_ui.destroy_confirm = !self.vault_ui.destroy_confirm;
            }
            Message::VaultDestroy => {
                if let Some(vault) = &mut self.vault {
                    match vault.destroy_and_recreate() {
                        Ok(()) => {
                            self.vault_ui.state = VaultState::NeedSetup;
                            self.vault_ui.error = None;
                            self.vault_ui.destroy_confirm = false;
                            self.vault_ui.password_input.clear();
                            self.vault_ui.password_visible = false;
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(format!("Failed to reset vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultUnlock => {
                // Ignore the submit when no password was typed (pressing
                // Enter on an empty field or clicking Unlock with it blank
                // shouldn't run a doomed unlock attempt or surface an error).
                if self.vault_ui.password_input.is_empty() {
                    return Task::none();
                }
                if let Some(vault) = &mut self.vault {
                    match vault.unlock(&self.vault_ui.password_input) {
                        Ok(()) => {
                            self.vault_ui.state = VaultState::Unlocked;
                            self.vault_ui.error = None;
                            // Retain the password in memory so we can spawn
                            // child windows with it via stdin pipe.
                            self.master_password = Some(self.vault_ui.password_input.clone());
                            self.vault_ui.password_input.clear();
                            self.vault_ui.password_visible = false;
                            self.load_data_from_vault();
                            // Bring the sync engine up now that the
                            // vault is open, if the user left it on. Only
                            // the P2P transport has a background engine;
                            // SFTP reconciles on the cadence subscription.
                            let sync_task = if self.sync.enabled
                                && self.sync.transport != "sftp"
                            {
                                self.start_sync_engine()
                            } else {
                                Task::none()
                            };
                            // Auto-start port forward rules now that the
                            // vault (and its credentials) is open.
                            let mut unlock_tasks = vec![sync_task];
                            unlock_tasks.extend(self.auto_start_port_forwards());
                            // Plugin migrate-install + auto-update: for a
                            // password vault these are deferred from boot
                            // to here, now that the plugin rows are loaded
                            // (boot saw a locked vault with no rows).
                            unlock_tasks.extend(self.spawn_plugin_unlock_tasks());
                            // After a manual unlock, fire any deferred
                            // `--connect <uuid>` from the launch CLI args.
                            if let Some(connect_id) = self.pending_auto_connect.take()
                                && let Some(idx) = self
                                    .connections
                                    .iter()
                                    .position(|c| c.id == connect_id)
                            {
                                unlock_tasks.push(Task::done(Message::ConnectSsh(idx)));
                            } else {
                                // Land on Home with the host search focused
                                // so the user can type / keyboard-navigate
                                // immediately (matches ChangeView behavior).
                                unlock_tasks.push(iced::widget::operation::focus(
                                    iced::widget::Id::new("search-dashboard"),
                                ));
                            }
                            return Task::batch(unlock_tasks);
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_ui.error = Some("Invalid password".into());
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(e.to_string());
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
                // Leaving the Logs view re-arms Privacy Mode masking so a
                // revealed timeline doesn't stay exposed on the next visit.
                self.privacy_revealed = false;
                self.active_view = view;
                self.active_tab = None;
                // Drop any keyboard host selection when leaving / changing
                // the surface so a stale highlight doesn't linger.
                self.selected_nav = None;
                // Navigating to the host list (Home tab / Hosts pill)
                // returns to the root, not whichever group was last open.
                if view == View::Dashboard {
                    self.active_group = None;
                }
                // Burger menu auto-dismisses on navigation: the user
                // picked a destination, leaving the overlay open is
                // visual noise.
                self.show_burger_menu = false;
                self.show_subnav_overflow = false;
                // Lazy-load the local SFTP pane when the user first lands
                // on the view (or returns to it after the underlying dir
                // changed). Cheap enough to redo unconditionally.
                if view == View::Sftp {
                    // Back the SFTP surface with a tab entry (adopts the
                    // existing top-level `self.sftp` as the first tab). The
                    // single-tab case behaves exactly as before.
                    self.ensure_sftp_tab();
                    // Refresh whichever pane(s) are Local; remote panes
                    // ignore this (refresh_sftp_local early-returns).
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Right);
                }
                // Entering Logs re-reads the timeline from the vault:
                // rows created since boot (a session that just started
                // recording, fresh connection events) only exist in the
                // tables, not in the cached page state.
                if view == View::History
                    && let Some(vault) = &self.vault
                {
                    self.logs_total = vault.count_logs().unwrap_or(0);
                    self.logs = vault
                        .list_logs_page(self.logs_page * 50, 50)
                        .unwrap_or_default();
                    self.session_logs_total = vault.count_session_logs().unwrap_or(0);
                    self.session_logs = vault
                        .list_session_logs_page(self.session_logs_page * 50, 50)
                        .unwrap_or_default();
                }
                // Land on the view with its search field focused so the
                // user can start typing immediately (same ids as Ctrl+F).
                if let Some(id) = self.active_view_search_id() {
                    return iced::widget::operation::focus(id);
                }
                // Opening Settings directly on the (default) Interface
                // section never goes through ChangeSettingsSection, so
                // fetch the renderer readout here too.
                if view == View::Settings {
                    return self.renderer_info_task();
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
            Message::HostSearchChanged(v) => {
                self.host_search = v;
                // The filtered set just changed; drop the keyboard
                // selection so it can't point at a now-hidden host. Enter
                // still connects the top result while a search is active.
                self.selected_nav = None;
            }
            Message::HostFilterByCloudProfile(maybe_pid) => {
                self.host_filter_cloud_profile = maybe_pid;
                // Filter changed the visible set; drop the keyboard
                // selection so Enter can't connect a now-hidden host.
                self.selected_nav = None;
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
                    GroupPickerTarget::DynamicFormParent => {
                        self.cloud_dynamic_form.parent_label = label;
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
                        tracing::warn!("failed to persist sort setting {key}: {e}");
                    }
                }
                self.overlay = None;
            }
            Message::ToggleToolbarSearch => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::ToolbarSearch)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor the floating field over the toolbar's search
                    // zone: at the leading edge under LTR, by its trailing
                    // edge under RTL (the render path subtracts the width).
                    let menu_w = self.toolbar_search_width();
                    let pad = 24.0_f32;
                    let panel = if self.vault_panel_open() {
                        crate::app::PANEL_WIDTH
                    } else {
                        0.0
                    };
                    let x = if crate::i18n::is_rtl_layout() {
                        (self.window_size.width - panel - pad).max(menu_w)
                    } else {
                        self.vault_rail_width() + pad
                    };
                    // Sit over the toolbar row itself (the shared anchor is
                    // tuned for dropdowns *below* the button row; back out
                    // the button height + gap to land on the row).
                    let y = (self.dashboard_dropdown_anchor_y() - 42.0).max(0.0);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::ToolbarSearch,
                        x,
                        y,
                    });
                    if let Some(id) = self.active_view_search_id() {
                        return iced::widget::operation::focus(id);
                    }
                }
            }
            Message::ToggleToolbarOverflow => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::ToolbarOverflow)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Trailing-edge anchor, mirroring the sort menu so the
                    // `…` dropdown lands under the toolbar's right cluster.
                    let menu_width = self.overlay_menu_width(&OverlayState {
                        content: OverlayContent::ToolbarOverflow,
                        x: 0.0,
                        y: 0.0,
                    });
                    let pad = 24.0_f32;
                    let panel = if self.vault_panel_open() {
                        crate::app::PANEL_WIDTH
                    } else {
                        0.0
                    };
                    let x = if crate::i18n::is_rtl_layout() {
                        panel + pad + menu_width
                    } else {
                        (self.window_size.width - panel - pad - menu_width).max(0.0)
                    };
                    let y = self.dashboard_dropdown_anchor_y();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::ToolbarOverflow,
                        x,
                        y,
                    });
                }
            }
            Message::QuickHostContinue => {
                if !self.quick_host_input.is_empty() {
                    self.editor_form = self.new_connection_form();
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
                self.overlay = None;
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
            Message::RequestDeleteSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let name = snip.label.clone();
                    self.confirm_remove(name, Message::DeleteSnippet(idx));
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


            // -- History --
            // Clear now wipes both feeds the unified History timeline
            // mixes (failed-connect log rows + recorded session rows)
            // so the user gets a true "empty list" instead of seeing
            // every previously recorded session reappear after the
            // wipe finishes.
            Message::RequestClearHistory => {
                // Close the `…` overflow menu before the confirm dialog
                // rises (no-op when triggered from the inline button).
                self.overlay = None;
                self.clear_history_confirm = true;
            }
            Message::CancelClearHistory => {
                self.clear_history_confirm = false;
            }
            Message::ClearLogs => {
                self.clear_history_confirm = false;
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
                // Flush buffered output first so viewing a still-active
                // session shows everything recorded up to this moment,
                // not just what was last persisted.
                self.flush_session_logs_final();
                if let Some(vault) = &self.vault
                    && let Ok(Some(data)) = vault.get_session_data(&log_id) {
                        let palette = self.resolve_global_terminal_palette();
                        let spans = crate::ansi_render::render(&data, &palette);
                        self.viewing_session_log = Some((log_id, spans));
                }
            }
            Message::CloseSessionLogView => {
                self.viewing_session_log = None;
            }
            Message::RequestDeleteSessionLog(idx) => {
                let label = self
                    .session_logs
                    .get(idx)
                    .map(|e| e.label.clone())
                    .unwrap_or_default();
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("log_delete_confirm_title").to_string(),
                    body: format!(
                        "{label}: {}",
                        crate::i18n::t("log_delete_confirm_body")
                    ),
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("delete").to_string(),
                        message: Box::new(Message::DeleteSessionLog(idx)),
                        danger: true,
                    }),
                });
            }
            Message::TogglePrivacyReveal => {
                self.privacy_revealed = !self.privacy_revealed;
            }
            Message::LogRowHovered(id) => {
                self.hovered_log_row = Some(id);
            }
            Message::LogRowUnhovered => {
                self.hovered_log_row = None;
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
            Message::ErrorDialogRunAction => {
                if let Some(dialog) = self.error_dialog.take()
                    && let Some(action) = dialog.action
                {
                    return self.update(*action.message);
                }
            }
            Message::ErrorDialogDismiss => {
                self.error_dialog = None;
            }

            // ── Vault password management ──
            Message::ToggleVaultPassword => {
                if self.vault_ui.has_user_password {
                    // Remove password
                    if let Some(vault) = &mut self.vault {
                        match vault.remove_user_password() {
                            Ok(()) => {
                                self.vault_ui.has_user_password = false;
                                self.vault_ui.password_error = None;
                                self.vault_ui.new_password.clear();
                                self.vault_ui.confirm_password.clear();
                            }
                            Err(e) => {
                                self.vault_ui.password_error = Some(e.to_string());
                            }
                        }
                    }
                } else {
                    // Show password input (don't do anything yet, user needs to type and confirm)
                    self.vault_ui.new_password.clear();
                    self.vault_ui.confirm_password.clear();
                    self.vault_ui.password_error = None;
                }
            }
            Message::VaultNewPasswordChanged(pw) => {
                self.vault_ui.new_password = pw;
            }
            Message::VaultConfirmPasswordChanged(pw) => {
                self.vault_ui.confirm_password = pw;
            }
            Message::SetVaultPassword => {
                if self.vault_ui.new_password.len() < 4 {
                    self.vault_ui.password_error =
                        Some(crate::i18n::t("password_too_short").to_string());
                    return Task::none();
                }
                // Both fields are hidden, so a typo would otherwise be
                // invisible until the next unlock (when it's too late).
                if self.vault_ui.new_password != self.vault_ui.confirm_password {
                    self.vault_ui.password_error =
                        Some(crate::i18n::t("passwords_do_not_match").to_string());
                    return Task::none();
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_user_password(&self.vault_ui.new_password) {
                        Ok(()) => {
                            self.vault_ui.has_user_password = true;
                            self.vault_ui.password_error = None;
                            self.vault_ui.new_password.clear();
                            self.vault_ui.confirm_password.clear();
                        }
                        Err(e) => {
                            self.vault_ui.password_error = Some(e.to_string());
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
            // Cycle the per-host recording override: Default (inherit the
            // global setting) -> On -> Off -> Default.
            Message::EditorCycleSessionLogging => {
                self.editor_form.session_logging = match self.editor_form.session_logging {
                    None => Some(true),
                    Some(true) => Some(false),
                    Some(false) => None,
                };
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
                self.mcp.server_enabled = !self.mcp.server_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("mcp_server_enabled", if self.mcp.server_enabled { "true" } else { "false" });
                }
                // MCP ships as a plugin (~5 MB binary external clients
                // like Claude Desktop spawn). First-time enable triggers
                // the install modal; an already-installed plugin or a
                // dev binary on the side both make this a no-op.
                if self.mcp.server_enabled
                    && !crate::mcp_install::is_installed()
                    && !crate::dispatch_plugins::dev_binary_present("mcp")
                {
                    return Task::done(Message::ShowPluginInstallModal(
                        "mcp".to_string(),
                    ));
                }
            }
            Message::ShowMcpInfo => {
                self.mcp.show_info = true;
                self.mcp.config_copied = false;
            }
            Message::HideMcpInfo => {
                self.mcp.show_info = false;
                self.mcp.config_copied = false;
            }
            Message::CopyMcpConfig => {
                self.mcp.config_copied = true;
                let json = if self.mcp.target_wsl {
                    mcp_config_json_wsl(&self.mcp.server_token)
                } else {
                    mcp_config_json(&self.mcp.server_token)
                };
                return iced::clipboard::write(json).discard();
            }
            Message::InstallMcpConfig => {
                self.mcp.install_status = None;
                let token = self.mcp.server_token.clone();
                let wsl = self.mcp.target_wsl;
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
                self.mcp.target_wsl = is_wsl;
                // The Copy / Install feedback from the previous target no
                // longer reflects what's on screen.
                self.mcp.config_copied = false;
                self.mcp.install_status = None;
            }
            Message::InstallMcpConfigResult(result) => {
                self.mcp.install_status = Some(result);
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
                self.mcp.server_token = token;
                // Reveal once after regenerating so the user can copy
                // it without an extra click; flip it back to masked
                // explicitly via `ToggleMcpTokenVisibility`.
                self.mcp.token_visible = true;
                // The Claude config on disk still carries the old
                // token, prompt the user to re-install.
                self.mcp.install_status = None;
            }
            Message::ToggleMcpTokenVisibility => {
                self.mcp.token_visible = !self.mcp.token_visible;
            }
            Message::CopyMcpToken => {
                return iced::clipboard::write(self.mcp.server_token.clone()).discard();
            }

            // ── Sync ──
            Message::SyncToggleEnabled => {
                self.sync.enabled = !self.sync.enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_enabled", if self.sync.enabled { "true" } else { "false" });
                }
                // SFTP transport has no background engine: enabling just
                // persists the flag (the cadence subscription picks it up);
                // disabling clears any stale status.
                if self.sync.transport == "sftp" {
                    self.sync.status = Some(
                        crate::i18n::t(if self.sync.enabled {
                            "sync_status_enabled"
                        } else {
                            "sync_status_stopped"
                        })
                        .to_string(),
                    );
                } else if self.sync.enabled {
                    return self.start_sync_engine();
                } else {
                    self.stop_sync_engine();
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_stopped").to_string());
                }
            }
            Message::SyncTogglePasswords => {
                self.sync.passwords = !self.sync.passwords;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting(
                        "sync_passwords",
                        if self.sync.passwords { "true" } else { "false" },
                    );
                }
            }
            Message::SyncModeChanged(v) => {
                self.sync.mode = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_mode", &v);
                }
            }
            Message::SyncDeviceNameChanged(v) => {
                self.sync.device_name = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_device_name", &v);
                }
            }
            Message::SyncSignalingUrlChanged(v) => {
                self.sync.signaling_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_url", &v);
                }
            }
            Message::SyncSignalingTokenChanged(v) => {
                self.sync.signaling_token = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_token", &v);
                }
            }
            Message::SyncRelayUrlChanged(v) => {
                self.sync.relay_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_relay_url", &v);
                }
            }
            Message::SyncListenPortChanged(v) => {
                self.sync.listen_port = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_listen_port", &v);
                }
            }
            Message::SyncStartPairing => {
                // Host a real pairing code on the engine. The engine
                // also emits `PairingCodeGenerated`, but we set the
                // code + state here directly so the UI flips instantly.
                if let Some(runtime) = &self.sync.runtime {
                    let handle = runtime.handle();
                    let code = handle.start_hosting_pairing();
                    let link = handle.pairing_link(&code);
                    self.sync.pairing.link = Some(link);
                    self.sync.pairing.code = Some(code);
                    self.sync.pairing.state = crate::state::SyncPairingState::Hosting;
                } else {
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                }
            }
            Message::SyncCancelHostingPairing => {
                if let Some(runtime) = &self.sync.runtime {
                    runtime.handle().cancel_hosting_pairing();
                }
                self.sync.pairing.code = None;
                self.sync.pairing.link = None;
                self.sync.pairing.state = crate::state::SyncPairingState::Idle;
            }
            Message::SyncJoinPairingRequested => {
                self.sync.pairing.state = crate::state::SyncPairingState::Joining;
                self.sync.pairing.join_code_input.clear();
                self.sync.pairing.join_target_input.clear();
                self.sync.pairing.join_link_input.clear();
            }
            Message::SyncJoinCodeChanged(v) => {
                self.sync.pairing.join_code_input = v;
            }
            Message::SyncJoinTargetChanged(v) => {
                self.sync.pairing.join_target_input = v;
            }
            Message::SyncJoinLinkChanged(v) => {
                self.sync.pairing.join_link_input = v;
            }
            Message::SyncJoinPairingCancel => {
                self.sync.pairing.state = crate::state::SyncPairingState::Idle;
            }
            Message::SyncPairWithDiscovered(device_id) => {
                if let Some(peer) = self
                    .sync.discovered
                    .iter()
                    .find(|p| p.device_id == device_id)
                {
                    self.sync.pairing.state = crate::state::SyncPairingState::Joining;
                    self.sync.pairing.join_code_input.clear();
                    self.sync.pairing.join_link_input.clear();
                    self.sync.pairing.join_target_input = peer.addr.to_string();
                }
            }
            Message::SyncJoinPairingByLink => {
                let Some(runtime) = &self.sync.runtime else {
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                    return Task::none();
                };
                let link = self.sync.pairing.join_link_input.trim().to_string();
                if oryxis_sync::parse_pairing_link(&link).is_none() {
                    self.sync.status = Some(
                        crate::i18n::t("sync_pairing_bad_link").to_string(),
                    );
                    return Task::none();
                }
                let handle = runtime.handle();
                // Keep at Joining so the inline status + form stay
                // visible; the PairingCompleted / PairingFailed event
                // handler decides whether to drop back to Idle.
                self.sync.status =
                    Some(crate::i18n::t("sync_pairing_connecting").to_string());
                return Task::perform(
                    async move {
                        let _ = handle.join_pairing_remote(&link).await;
                    },
                    |()| Message::NoOp,
                );
            }
            Message::SyncJoinPairingConnect => {
                let Some(runtime) = &self.sync.runtime else {
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                    return Task::none();
                };
                let code = self.sync.pairing.join_code_input.trim().to_string();
                if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
                    self.sync.status =
                        Some(crate::i18n::t("sync_pairing_invalid_code").to_string());
                    return Task::none();
                }
                let addr: std::net::SocketAddr =
                    match self.sync.pairing.join_target_input.trim().parse() {
                        Ok(a) => a,
                        Err(_) => {
                            self.sync.status = Some(
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
                self.sync.status =
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
                    self.sync.peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            Message::SyncNow => {
                // SFTP transport: a manual round goes through the
                // snapshot path, not the P2P engine.
                if self.sync.transport == "sftp" {
                    return self.run_sftp_sync_round();
                }
                if self.sync.in_progress {
                    // Defensive: shouldn't fire because the UI swaps
                    // Sync Now for Cancel while a sync is running,
                    // but if a stray click does land, ignore it.
                    return Task::none();
                }
                if let Some(runtime) = &self.sync.runtime {
                    let handle = runtime.handle();
                    let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
                    self.sync.abort_tx = Some(abort_tx);
                    self.sync.in_progress = true;
                    self.sync.status =
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
                self.sync.status =
                    Some(crate::i18n::t("sync_status_disabled").to_string());
            }
            Message::SyncCancelInProgress => {
                if let Some(tx) = self.sync.abort_tx.take() {
                    let _ = tx.send(());
                }
                // Don't clear `sync_in_progress` here: the Task lands
                // back as `SyncNowFinished(Err("__cancelled__"))` and
                // clears it there, so the Cancel button stays visible
                // until the cancellation actually settles.
            }
            Message::SyncTransportChanged(v) => {
                if v != self.sync.transport {
                    // Leaving P2P: tear the engine down so QUIC/mDNS stop.
                    // Entering P2P (and enabled): bring it up.
                    self.sync.transport = v.clone();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("sync_transport", &v);
                    }
                    self.sync.status = None;
                    self.sync.sftp.status = None;
                    if v == "sftp" {
                        self.stop_sync_engine();
                    } else if self.sync.enabled {
                        return self.start_sync_engine();
                    }
                }
            }
            Message::SyncSftpHostChanged(id) => {
                self.sync.sftp.host_id = Some(id);
                self.sync.sftp.picker_open = false;
                self.sync.sftp.picker_search.clear();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_sftp_host_id", &id.to_string());
                }
            }
            Message::SyncSftpOpenPicker => {
                self.sync.sftp.picker_open = true;
                self.sync.sftp.picker_search.clear();
            }
            Message::SyncSftpClosePicker => {
                self.sync.sftp.picker_open = false;
                self.sync.sftp.picker_search.clear();
            }
            Message::SyncSftpPickerSearch(v) => {
                self.sync.sftp.picker_search = v;
            }
            Message::SyncSftpPathChanged(v) => {
                self.sync.sftp.remote_path = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_sftp_remote_path", &v);
                }
            }
            Message::SyncSftpPassphraseChanged(v) => {
                self.sync.sftp.passphrase = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_sync_sftp_passphrase(&v);
                }
            }
            Message::SftpSyncTick => {
                // Auto-cadence tick. Only act in SFTP+enabled+auto and
                // when no round is already running; otherwise the tick
                // is a no-op (the subscription keeps firing regardless).
                if self.sync.transport == "sftp"
                    && self.sync.enabled
                    && self.sync.mode == "auto"
                    && !self.sync.sftp.in_progress
                {
                    return self.run_sftp_sync_round();
                }
            }
            Message::SftpSyncDone(result) => {
                self.sync.sftp.in_progress = false;
                if result.is_ok() {
                    // The merge ran on a separate vault handle, so the
                    // in-memory lists are stale: reload to reflect it.
                    self.load_data_from_vault();
                }
                self.sync.sftp.status = Some(result);
            }
            Message::SyncNowFinished(result) => {
                self.sync.in_progress = false;
                self.sync.abort_tx = None;
                match result {
                    Ok(()) => {}
                    Err(e) if e == "__cancelled__" => {
                        self.sync.status = Some(
                            crate::i18n::t("sync_status_cancelled").to_string(),
                        );
                    }
                    Err(e) if e == "__timeout__" => {
                        self.sync.status = Some(
                            crate::i18n::t("sync_status_timeout").to_string(),
                        );
                    }
                    Err(e) => {
                        self.sync.status = Some(format!(
                            "{}: {e}",
                            crate::i18n::t("sync_status_failed"),
                        ));
                    }
                }
                // Per-peer outcomes already arrived as SyncEngineEvent;
                // refresh the peer list so last_synced_at is current.
                if let Some(vault) = &self.vault {
                    self.sync.peers = vault.list_sync_peers().unwrap_or_default();
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
                            .sync.discovered
                            .iter_mut()
                            .find(|p| p.device_id == device_id)
                        {
                            *existing = info;
                        } else {
                            self.sync.discovered.push(info);
                        }
                    }
                    SyncEvent::PairingCodeGenerated { code } => {
                        self.sync.pairing.code = Some(code);
                    }
                    SyncEvent::PairingCompleted { device_name, .. } => {
                        self.sync.status = Some(format!(
                            "{} {device_name}",
                            crate::i18n::t("sync_paired_with"),
                        ));
                        // Pairing done on either side: close the modal
                        // sub-view, drop the hosted code / link / QR,
                        // and refresh the peer list.
                        self.sync.pairing.state =
                            crate::state::SyncPairingState::Idle;
                        self.sync.pairing.code = None;
                        self.sync.pairing.link = None;
                        if let Some(vault) = &self.vault {
                            self.sync.peers =
                                vault.list_sync_peers().unwrap_or_default();
                        }
                    }
                    SyncEvent::PairingFailed { reason } => {
                        self.sync.status = Some(format!(
                            "{}: {reason}",
                            crate::i18n::t("sync_pairing_failed"),
                        ));
                        // Stay in whichever sub-view triggered the
                        // pairing so the user sees the error in
                        // context and can fix + retry without
                        // re-entering everything. Host-side: clear
                        // the code/link since the single-shot was
                        // consumed even on failure.
                        if self.sync.pairing.state
                            == crate::state::SyncPairingState::Hosting
                        {
                            self.sync.pairing.code = None;
                            self.sync.pairing.link = None;
                            self.sync.pairing.state =
                                crate::state::SyncPairingState::Idle;
                        }
                    }
                    SyncEvent::SyncStarted { .. } => {
                        self.sync.status =
                            Some(crate::i18n::t("sync_status_syncing").to_string());
                    }
                    SyncEvent::SyncCompleted { pushed, pulled, .. } => {
                        self.sync.status = Some(format!(
                            "{} (+{pushed} / -{pulled})",
                            crate::i18n::t("sync_status_done"),
                        ));
                        if let Some(vault) = &self.vault {
                            self.sync.peers =
                                vault.list_sync_peers().unwrap_or_default();
                        }
                    }
                    SyncEvent::SyncFailed { error, .. } => {
                        self.sync.status = Some(format!(
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
                        self.sync.signaling_tick =
                            self.sync.signaling_tick.saturating_add(1);
                        self.sync.status = Some(format!(
                            "{} ({}): {ip}:{port}",
                            crate::i18n::t("sync_status_signaling_registered"),
                            self.sync.signaling_tick,
                        ));
                    }
                    SyncEvent::SignalingFailed { reason } => {
                        self.sync.status = Some(format!(
                            "{}: {reason}",
                            crate::i18n::t("sync_status_signaling_failed"),
                        ));
                    }
                    SyncEvent::VersionMismatch {
                        peer_version,
                        local_version,
                        ..
                    } => {
                        self.sync.status = Some(format!(
                            "{}: peer v{peer_version}, local v{local_version}",
                            crate::i18n::t("sync_status_version_mismatch"),
                        ));
                    }
                    SyncEvent::PeerStaleWarning { days_since_sync, .. } => {
                        self.sync.status = Some(format!(
                            "{} ({}d)",
                            crate::i18n::t("sync_status_peer_stale"),
                            days_since_sync,
                        ));
                    }
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
