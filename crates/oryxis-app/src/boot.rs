//! `Oryxis::boot`, vault data hydration, and the `persist_setting`
//! best-effort writer. Pulled out of `app.rs` so the remaining file is
//! mostly the message dispatch + view plumbing.

use iced::keyboard;
use iced::widget::{svg, text_editor};
use iced::{Point, Task};

use oryxis_vault::VaultStore;

use crate::app::{Message, Oryxis, AUTO_CONNECT, AUTO_PASSWORD};
use crate::state::{ConnectionForm, SettingsSection, VaultState, View};

impl Oryxis {
    pub fn boot() -> (Self, Task<Message>) {
        // CLI hand-off: if the parent process started us with `--connect
        // <uuid>` (the path "Duplicate in New Window" takes), capture that
        // ID now and dispatch a `ConnectSsh` once the vault is open.
        let pending_auto_connect = AUTO_CONNECT.get().copied();
        // Inherited master password from the parent's stdin pipe, used
        // to silently unlock the vault below so the user doesn't have to
        // re-type for the spawned window.
        let inherited_password = AUTO_PASSWORD.get().cloned();

        let mut vault = VaultStore::open_default().ok();
        let mut vault_state = VaultState::Loading;
        let mut vault_has_user_password = false;

        if let Some(v) = &mut vault {
            if !v.is_initialized() {
                // Brand new vault, show setup screen
                vault_state = VaultState::NeedSetup;
            } else {
                // Consult the plaintext `has_user_password` flag before
                // running `open_without_password`. That helper attempts
                // `unlock("")`, which runs a full Argon2id KDF against
                // the empty password just to find out the vault is
                // locked, ~17 ms wasted on every cold boot for users
                // with a master password. The flag is written by
                // `set_user_password` / `remove_user_password` /
                // `set_master_password`, and is backfilled here for
                // legacy vaults that pre-date the flag.
                let flag = v.get_setting("has_user_password").ok().flatten();
                let known_user_pw = matches!(flag.as_deref(), Some("1"));
                if known_user_pw {
                    // Skip empty-pw KDF entirely. Try the inherited
                    // password (from `--inherit-vault` stdin) before
                    // falling back to the lock screen.
                    let unlocked = inherited_password
                        .as_ref()
                        .is_some_and(|pw| v.unlock(pw).is_ok());
                    vault_state = if unlocked {
                        VaultState::Unlocked
                    } else {
                        VaultState::Locked
                    };
                    vault_has_user_password = true;
                } else {
                    // Either the flag says "0" or it's missing (legacy
                    // vault). Either way we attempt the empty unlock,
                    // and opportunistically backfill the flag if it
                    // wasn't there.
                    match v.open_without_password() {
                        Ok(()) => {
                            vault_state = VaultState::Unlocked;
                            vault_has_user_password = false;
                            if flag.is_none() {
                                let _ = v.set_setting("has_user_password", "0");
                            }
                        }
                        Err(_) => {
                            let unlocked = inherited_password
                                .as_ref()
                                .is_some_and(|pw| v.unlock(pw).is_ok());
                            vault_state = if unlocked {
                                VaultState::Unlocked
                            } else {
                                VaultState::Locked
                            };
                            vault_has_user_password = true;
                            if flag.is_none() {
                                let _ = v.set_setting("has_user_password", "1");
                            }
                        }
                    }
                }
            }
            // Theme + language live in the plaintext `settings` table,
            // not behind the encryption key, so we can hydrate them
            // *before* the unlock so the lock screen / setup screen
            // already render in the user's chosen theme + language
            // instead of falling back to the defaults until they type
            // the password.
            if let Ok(Some(name)) = v.get_setting("app_theme") {
                use crate::theme::AppTheme;
                AppTheme::set_active(AppTheme::from_name(&name));
            }
            if let Ok(Some(code)) = v.get_setting("language") {
                use crate::i18n::Language;
                Language::set_active(Language::from_code(&code));
            }
            if let Ok(Some(code)) = v.get_setting("layout_direction") {
                use crate::i18n::LayoutDirection;
                LayoutDirection::set_active(LayoutDirection::from_code(&code));
            }
        }

        // Plugin providers are kept twice: once as `Arc<dyn CloudProvider>`
        // inside the registry (used by every CloudProvider call site)
        // and once as `Arc<PluginProvider>` in `plugin_providers` (so
        // the install path can call rebind after `cache::set_current`).
        // Both fields point at the SAME Arc so a rebind through the
        // concrete map propagates to the registered trait object.
        let aws_provider =
            std::sync::Arc::new(crate::plugins::PluginProvider::new("aws"));
        let k8s_provider =
            std::sync::Arc::new(crate::plugins::PluginProvider::new("k8s"));
        let plugin_providers = {
            let mut m: std::collections::HashMap<
                String,
                std::sync::Arc<crate::plugins::PluginProvider>,
            > = std::collections::HashMap::new();
            m.insert("aws".to_string(), aws_provider.clone());
            m.insert("k8s".to_string(), k8s_provider.clone());
            m
        };
        let cloud_provider_registry = {
            let mut reg = oryxis_cloud::CloudProviderRegistry::new();
            reg.register(aws_provider.clone());
            reg.register(k8s_provider.clone());
            std::sync::Arc::new(reg)
        };

        let (mut app, task) = (
            Self {
                vault,
                vault_state,
                vault_password_input: String::new(),
                vault_password_visible: false,
                vault_error: None,
                // Vector logo: rendered through iced's SVG (resvg) path so
                // it stays crisp at any scale and avoids the wgpu image-atlas
                // corruption seen on GNOME Wayland fractional scaling. Both
                // handles share the one asset; the SVG scales to each call
                // site's box.
                logo_handle: svg::Handle::from_memory(include_bytes!("../../../resources/logo.svg").as_slice()),
                connections: Vec::new(),
                groups: Vec::new(),
                session_groups: Vec::new(),
                active_view: View::Dashboard,
                active_group: None,
                host_search: String::new(),
                quick_host_input: String::new(),
                tabs: Vec::new(),
                pending_pane_split: None,
                split_menu_hovered: false,
                active_tab: None,
                last_terminal_tab: None,
                hovered_tab: None,
                show_new_tab_picker: false,
                new_tab_picker_search: String::new(),
                new_tab_picker_group: None,
                show_tab_jump: false,
                tab_jump_search: String::new(),
                show_burger_menu: false,
                show_subnav_overflow: false,
                show_icon_picker: false,
                icon_picker_for: None,
                icon_picker_for_group_form: false,
                icon_picker_for_session_group: false,
                icon_picker_for_group_edit: false,
                icon_picker_icon: None,
                icon_picker_color: None,
                icon_picker_hex_input: String::new(),
                icon_picker_icon_search: String::new(),
                icon_color_popover: None,
                show_theme_picker: false,
                show_chain_editor: false,
                chain_editor_adding: false,
                chain_editor_search: String::new(),
                connecting: None,
                connect_anim_tick: 0,
                last_window_press_at: None,
                pending_host_key: None,
                host_key_response_tx: None,
                active_host_key_tx: None,
                pending_kbi_prompt: None,
                kbi_inputs: Vec::new(),
                kbi_response_tx: None,
                show_host_panel: false,
                editor_form: ConnectionForm::default(),
                editor_initial_command: text_editor::Content::new(),
                host_panel_error: None,
                show_session_group_panel: false,
                editor_session_group: crate::state::SessionGroupForm::default(),
                session_group_script_editor: text_editor::Content::new(),
                session_group_panel_error: None,
                hovered_session_group_card: None,
                pane_script_overrides: std::collections::HashMap::new(),
                hovered_card: None,
                selected_nav: None,
                dashboard_nav: std::cell::RefCell::new(Vec::new()),
                hovered_folder_card: None,
                hovered_key_card: None,
                hovered_identity_card: None,
                hovered_snippet_card: None,
                snippet_context_menu: None,
                card_context_menu: None,
                overlay: None,
                folder_rename: None,
                group_edit_visible: false,
                group_edit_id: None,
                group_edit_label: String::new(),
                group_edit_icon: String::new(),
                group_edit_color: String::new(),
                folder_delete: None,
                pending_auto_connect,
                // Keep the inherited password in memory only when the
                // unlock above actually succeeded, otherwise the user is
                // about to type their own at the lock screen.
                master_password: if vault_state == VaultState::Unlocked {
                    inherited_password
                } else {
                    None
                },
                sftp: crate::state::SftpState {
                    left: crate::state::PaneState {
                        is_remote: false,
                        local_path: std::env::var_os("HOME")
                            .or_else(|| std::env::var_os("USERPROFILE"))
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|| std::path::PathBuf::from("/")),
                        ..Default::default()
                    },
                    right: crate::state::PaneState {
                        is_remote: true,
                        ..Default::default()
                    },
                    // picker_open defaults to false. It must NOT start open:
                    // `any_modal_blocks_input()` treats an open SFTP picker as
                    // a focus-owning modal and swallows every terminal
                    // keystroke, so a stale boot-true silently kills all input
                    // until the picker is opened and closed once.
                    ..Default::default()
                },
                sftp_tabs: Vec::new(),
                active_sftp: None,
                tab_order: Vec::new(),
                routing_sftp: None,
                hovered_sftp_tab: None,
                pending_sftp_close: None,
                mouse_position: Point::ORIGIN,
                window_size: iced::Size::new(1200.0, 750.0),
                window_focused: true,
                ssm_keepalive_base: None,
                window_maximized: false,
                window_fullscreen: false,
                fullscreen_hint_visible: false,
                hotkey_bindings: crate::hotkeys::default_bindings(),
                editing_hotkey: None,
                modifiers: keyboard::Modifiers::default(),
                #[cfg(target_os = "windows")]
                last_printscreen: None,
                keys: Vec::new(),
                show_key_panel: false,
                key_import_label: String::new(),
                key_import_content: text_editor::Content::new(),
                key_import_pem: String::new(),
                key_import_passphrase: String::new(),
                key_import_passphrase_required: false,
                key_import_passphrase_visible: false,
                key_error: None,
                key_success: None,
                key_context_menu: None,
                editing_key_id: None,
                key_search: String::new(),
                snippet_search: String::new(),
                history_search: String::new(),
                identities: Vec::new(),
                identities_with_password: std::collections::HashSet::new(),
                show_identity_panel: false,
                identity_form_label: String::new(),
                identity_form_username: String::new(),
                identity_form_password: String::new(),
                identity_form_key: None,
                identity_form_password_visible: false,
                identity_form_password_touched: false,
                identity_form_has_existing_password: false,
                editing_identity_id: None,
                identity_context_menu: None,
                show_keychain_add_menu: false,
                hosts_sort: crate::state::ListSort::default(),
                keys_sort: crate::state::ListSort::default(),
                snippets_sort: crate::state::ListSort::default(),
                proxy_identities: Vec::new(),
                proxy_identity_form_visible: false,
                proxy_identity_form_label: String::new(),
                proxy_identity_form_kind: crate::state::ProxyKind::Socks5,
                proxy_identity_form_host: String::new(),
                proxy_identity_form_port: String::new(),
                proxy_identity_form_username: String::new(),
                proxy_identity_form_password: String::new(),
                proxy_identity_form_password_visible: false,
                proxy_identity_form_password_touched: false,
                proxy_identity_form_has_existing_password: false,
                editing_proxy_identity_id: None,
                proxy_identity_form_error: None,
                cloud_profiles: Vec::new(),
                cloud_form_visible: false,
                cloud_form_label: String::new(),
                cloud_form_provider: crate::state::CloudProviderChoice::Aws,
                cloud_form_auth_kind: crate::state::CloudAuthChoice::Profile,
                cloud_form_aws_profile_name: String::new(),
                cloud_form_aws_regions: Vec::new(),
                cloud_form_aws_region_draft: String::new(),
                cloud_form_aws_access_key_id: String::new(),
                cloud_form_aws_access_key_secret: String::new(),
                cloud_form_aws_access_key_secret_touched: false,
                cloud_form_aws_access_key_secret_visible: false,
                cloud_form_aws_access_key_session_token: String::new(),
                cloud_form_aws_has_existing_secret: false,
                cloud_form_aws_sso_start_url: String::new(),
                cloud_form_aws_sso_region: String::new(),
                cloud_form_aws_sso_account_id: String::new(),
                cloud_form_aws_sso_role_name: String::new(),
                cloud_form_kubeconfig_path: String::new(),
                cloud_form_context: String::new(),
                editing_cloud_profile_id: None,
                cloud_form_error: None,
                cloud_form_test_state: crate::state::CloudTestState::Idle,
                cloud_discover_visible: false,
                cloud_discover_profile_id: None,
                cloud_discover_state: crate::state::CloudDiscoverState::Idle,
                cloud_discover_selected_ec2: std::collections::HashSet::new(),
                cloud_discover_selected_ecs: std::collections::HashSet::new(),
                cloud_discover_selected_k8s: std::collections::HashSet::new(),
                cloud_discover_filter: String::new(),
                cloud_discover_collapsed: std::collections::HashSet::new(),
                cloud_discover_default_transport:
                    oryxis_core::models::cloud::TransportKind::Ssh,
                cloud_discover_default_group_name: String::new(),
                editor_parent_combo: iced::widget::combo_box::State::new(Vec::new()),
                editor_startup_combo: iced::widget::combo_box::State::new(Vec::new()),
                editor_key_combo: iced::widget::combo_box::State::new(Vec::new()),
                cloud_discover_default_group_picker_open: false,
                cloud_discover_default_group_picker_search: String::new(),
                cloud_discover_default_group_combo_bounds: crate::widgets::new_bounds_cell(),
                group_picker_search: String::new(),
                editor_startup_choice: crate::state::StartupChoice::None,
                dynamic_form_parent_combo_bounds: crate::widgets::new_bounds_cell(),
                session_group_folder_combo_bounds: crate::widgets::new_bounds_cell(),
                plus_btn_bounds: crate::widgets::new_bounds_cell(),
                host_filter_cloud_profile: None,
                cloud_import_confirm_visible: false,
                cloud_dynamic_group_state: std::collections::HashMap::new(),
                cloud_dynamic_form_visible: false,
                cloud_dynamic_form_group_id: None,
                cloud_dynamic_form_username: String::new(),
                cloud_dynamic_form_initial_command: String::new(),
                cloud_dynamic_form_transport:
                    oryxis_core::models::cloud::TransportKind::EcsExec,
                cloud_dynamic_form_selected_key: None,
                cloud_dynamic_form_selected_identity: None,
                cloud_dynamic_form_label: String::new(),
                cloud_dynamic_form_color: String::new(),
                cloud_dynamic_form_icon: String::new(),
                cloud_dynamic_form_parent_label: String::new(),
                cloud_dynamic_form_is_k8s: false,
                cloud_dynamic_form_k8s_context: String::new(),
                cloud_dynamic_form_namespace: String::new(),
                cloud_dynamic_form_k8s_selector_kind: crate::state::K8sSelectorKind::Labels,
                cloud_dynamic_form_k8s_selector_value: String::new(),
                cloud_dynamic_form_cluster: String::new(),
                cloud_dynamic_form_service: String::new(),
                cloud_dynamic_form_container: String::new(),
                hovered_dynamic_group_card: None,
                hovered_cloud_card: None,
                // Provider registry seeded once at boot. AWS runs as a
                // plugin subprocess via `PluginProvider`; K8s lands in
                // a follow-up PR. The Arc lets us hand the registry to
                // async tasks without locking.
                cloud_provider_registry,
                plugin_providers,
                // Plugins panel state, the defaults here are replaced
                // by `load_data_from_vault` once the vault is unlocked
                // (settings + on-disk plugin cache).
                plugins_auto_update_global: true,
                plugins: Vec::new(),
                plugin_install_modal: None,
                snippets: Vec::new(),
                custom_terminal_themes: Vec::new(),
                custom_ui_themes: Vec::new(),
                theme_editor: None,
                hovered_theme_card: None,
                theme_color_popover: None,
                show_theme_import: false,
                theme_import_content: text_editor::Content::new(),
                theme_import_name: String::new(),
                theme_import_error: None,
                ui_theme_editor: None,
                ui_color_popover: None,
                hovered_ui_theme_card: None,
                active_app_theme_name: "Oryxis Dark".to_string(),
                known_hosts: Vec::new(),
                logs: Vec::new(),
                logs_page: 0,
                logs_total: 0,
                clear_history_confirm: false,
                session_logs: Vec::new(),
                session_logs_page: 0,
                session_logs_total: 0,
                viewing_session_log: None,
                hovered_log_row: None,
                show_snippet_panel: false,
                snippet_label: String::new(),
                snippet_command: text_editor::Content::new(),
                snippet_editing_id: None,
                snippet_error: None,
                port_forward_rules: Vec::new(),
                active_forwards: std::collections::HashMap::new(),
                port_forward_starting: std::collections::HashSet::new(),
                show_port_forward_panel: false,
                pf_label: String::new(),
                pf_kind: oryxis_core::models::port_forward_rule::ForwardKind::Local,
                pf_host_id: None,
                pf_listen_host: "127.0.0.1".into(),
                pf_listen_port: String::new(),
                pf_target_host: String::new(),
                pf_target_port: String::new(),
                pf_auto_start: false,
                pf_editing_id: None,
                pf_error: None,
                hovered_port_forward_card: None,
                port_forward_search: String::new(),
                cloud_search: String::new(),
                proxy_search: String::new(),
                terminal_palette: oryxis_terminal::TerminalPalette::default(),
                terminal_theme_override: None,
                terminal_font_size: 14.0,
                terminal_font_name: "SauceCodePro Nerd Font".to_string(),
                settings_section: SettingsSection::Interface,
                setting_renderer_backend: "auto".to_string(),
                renderer_active: None,
                setting_copy_on_select: true,
                setting_right_click_copy: false,
                setting_bold_is_bright: true,
                setting_keyword_highlight: true,
                setting_smart_contrast: true,
                setting_show_status_bar: true,
                setting_host_list_view: false,
                setting_card_accent_glass: true,
                setting_show_host_address: false,
                setting_close_to_tray: false,
                setting_minimize_to_tray: false,
                tray_menu_signature: 0,
                is_window_hidden: false,
                ipc_state_signature: 0,
                setting_tab_close_button_side: "left".into(),
                setting_pinned_tab_style: "compact".into(),
                pin_next_plugin_tab: None,
                pending_ecs_autoconnect: None,
                tab_drag: None,
                setting_show_tab_status_dot: true,
                setting_tab_accent_line: true,
                setting_tab_accent_wash: true,
                setting_tab_fill_style: "gradient".into(),
                sftp_enabled: true,
                // Workspace is the v0.7 default. Existing users who
                // never persisted `layout_mode` also fall through to
                // this default on next launch (no migration row
                // Vault nav orientation: horizontal pill strip by default.
                setting_nav_orientation: "horizontal".into(),
                setting_nav_rail_expanded: false,
                setting_default_host_icon: "circular".into(),
                setting_keepalive_interval: "30".into(),
                setting_cloud_auto_refresh_enabled: false,
                setting_cloud_auto_refresh_interval_minutes: "30".into(),
                setting_cloud_auto_archive_orphans: false,
                setting_cloud_orphan_archive_days: "7".into(),
                setting_scrollback_rows: "10000".into(),
                setting_word_delimiters: oryxis_terminal::DEFAULT_WORD_DELIMITERS.into(),
                revealed_secrets: std::collections::HashSet::new(),
                hint_link_click_used: false,
                setting_sftp_concurrency: "2".into(),
                setting_sftp_connect_timeout: "15".into(),
                setting_sftp_auth_timeout: "30".into(),
                setting_sftp_session_timeout: "10".into(),
                setting_sftp_op_timeout: "30".into(),
                setting_auto_reconnect: true,
                setting_max_reconnect_attempts: "5".into(),
                setting_os_detection: true,
                setting_session_logging: false,
                setting_connection_history: false,
                setting_logs_retention: "off".into(),
                setting_auto_check_updates: true,
                setting_update_channel: crate::update::UpdateChannel::default(),
                pending_update: None,
                update_downloading: false,
                update_progress: 0.0,
                update_error: None,
                update_check_status: None,
                reconnect_counters: std::collections::HashMap::new(),
                ai_enabled: false,
                ai_provider: "anthropic".into(),
                ai_model: "claude-sonnet-4-20250514".into(),
                ai_api_key: String::new(),
                ai_api_key_set: false,
                ai_api_url: String::new(),
                ai_system_prompt: text_editor::Content::new(),
                vault_has_user_password,
                vault_new_password: String::new(),
                vault_confirm_password: String::new(),
                vault_password_error: None,
                vault_destroy_confirm: false,
                toast: None,
                loaded_cjk_fonts: std::collections::HashSet::new(),
                error_dialog: None,
                local_shells: None,
                local_shell_picker_open: false,
                chat_input: text_editor::Content::new(),
                chat_loading: false,
                chat_task: None,
                chat_scroll_at_bottom: true,
                terminal_sidebar_tab: crate::state::TerminalSidebarTab::default(),
                sidebar_snippet_search: String::new(),
                sidebar_sort_open: false,
                sidebar_search_open: false,
                chat_sidebar_width: 350.0,
                chat_sidebar_drag: None,
                sftp_split_ratio: 0.5,
                sftp_split_drag: None,
                sftp_log_drag: None,
                sftp_columns_template: crate::state::SftpColumnState::default(),
                sftp_col_resize: None,
                sftp_col_drag: None,
                sftp_hovered_col: None,
                mcp_server_enabled: false,
                show_mcp_info: false,
                mcp_config_copied: false,
                mcp_install_status: None,
                mcp_server_token: String::new(),
                mcp_token_visible: false,
                mcp_target_wsl: false,
                sync_enabled: false,
                sync_mode: "manual".into(),
                sync_passwords: false,
                flatten_hosts: true,
                sync_device_name: String::new(),
                // `signaling_url` is now `Option<String>` on the
                // engine config; the app state uses a plain `String`
                // (empty == not set) so a Settings text input can
                // drive it.
                sync_signaling_url: oryxis_sync::SyncConfig::default()
                    .signaling_url
                    .unwrap_or_default(),
                sync_signaling_token: oryxis_sync::SyncConfig::default()
                    .signaling_token
                    .unwrap_or_default(),
                sync_relay_url: String::new(),
                sync_listen_port: "0".into(),
                sync_peers: Vec::new(),
                sync_pairing_code: None,
                sync_status: None,
                sync_runtime: None,
                sync_engine_running: false,
                sync_pairing_state: crate::state::SyncPairingState::Idle,
                sync_join_code_input: String::new(),
                sync_join_target_input: String::new(),
                sync_pairing_link: None,
                sync_join_link_input: String::new(),
                sync_discovered: Vec::new(),
                sync_in_progress: false,
                sync_abort_tx: None,
                sync_signaling_tick: 0,
                show_export_dialog: false,
                export_password: String::new(),
                export_include_keys: true,
                export_selection: oryxis_vault::ExportSelection::all(),
                export_status: None,
                show_import_dialog: false,
                import_password: String::new(),
                import_file_data: None,
                import_summary: None,
                import_selection: oryxis_vault::ExportSelection::all(),
                import_status: None,
                ssh_config_import_status: None,
                show_share_dialog: false,
                share_password: String::new(),
                share_include_keys: false,
                share_filter: None,
                share_status: None,
                share_suggested_name: None,
            },
            Task::none(),
        );

        // If auto-unlocked (no user password), load data immediately
        if app.vault_state == VaultState::Unlocked {
            app.load_data_from_vault();
        }

        // If we were launched with `--connect <uuid>` AND the vault is
        // already open (no master password), kick off the connect right
        // after boot. When the vault is locked, we defer until VaultUnlock
        // succeeds (handled in that branch).
        let mut tasks = vec![task, Task::done(Message::CheckForUpdate)];
        if app.vault_state == VaultState::Unlocked
            && let Some(connect_id) = app.pending_auto_connect.take()
            && let Some(idx) = app
                .connections
                .iter()
                .position(|c| c.id == connect_id)
        {
            tasks.push(Task::done(Message::ConnectSsh(idx)));
        }
        // Bring the sync engine up if the vault is already open and the
        // user left sync enabled. When the vault is locked we defer to
        // the `VaultUnlock` handler, same as `--connect`.
        if app.vault_state == VaultState::Unlocked && app.sync_enabled {
            tasks.push(app.start_sync_engine());
        }

        // Auto-start port forward rules marked `auto_start`. Deferred to
        // `VaultUnlock` when the vault is locked, same as sync / --connect.
        if app.vault_state == VaultState::Unlocked {
            tasks.extend(app.auto_start_port_forwards());
        }

        // Sweep any leftover `.old.exe` from a previous Windows MCP
        // update (no-op on Unix), before the plugin tasks below may lay
        // down a fresh launcher copy.
        crate::mcp_install::sweep_stale_launcher();
        // MCP migrate-install + plugin auto-update both need the vault
        // unlocked (they read `mcp_server_enabled` / the plugin rows
        // `load_data_from_vault` populates). When the vault is
        // password-protected it's still locked here, so these defer to
        // the `VaultUnlock` handler, which calls the same method once
        // the user's password opens it (the boot constructor can't
        // re-run). See `spawn_plugin_unlock_tasks`.
        if app.vault_state == VaultState::Unlocked {
            tasks.extend(app.spawn_plugin_unlock_tasks());
        }

        // If the saved language uses a CJK script (Korean / Chinese /
        // Japanese), fetch + load its on-demand font now so the lock
        // screen and the rest of the UI render it instead of tofu. The
        // language was already the user's choice, so this is silent (no
        // toast). A missing font degrades to the system CJK font.
        {
            let lang = crate::i18n::Language::active();
            if let Some(code) = crate::fonts::asset_code(lang) {
                app.loaded_cjk_fonts.insert(code.to_string());
                tasks.push(crate::fonts::ensure_task(lang));
            }
        }

        // Populate the unified strip order from the restored (dormant pinned)
        // tabs before the first render; subsequent messages keep it in sync via
        // `reconcile_tab_order` at the end of `update`.
        app.reconcile_tab_order();
        let boot_task = Task::batch(tasks);
        (app, boot_task)
    }

    pub(crate) fn load_data_from_vault(&mut self) {
        if let Some(vault) = &self.vault {
            // Auto-archive sweep: when the user has opted into the
            // cleanup, drop orphan-imported hosts whose `orphaned_at`
            // is older than the configured threshold. Runs before the
            // in-memory load so the deleted rows don't briefly appear
            // and then vanish.
            if self.setting_cloud_auto_archive_orphans {
                let days = self
                    .setting_cloud_orphan_archive_days
                    .parse::<i64>()
                    .ok()
                    .filter(|d| *d > 0)
                    .unwrap_or(7);
                let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
                if let Ok(existing) = vault.list_connections() {
                    for c in existing.iter() {
                        if let Some(cr) = c.cloud_ref.as_ref()
                            && let Some(orphaned_at) = cr.orphaned_at
                            && orphaned_at < cutoff
                        {
                            let _ = vault.delete_connection(&c.id);
                        }
                    }
                }
            }
            self.connections = vault.list_connections().unwrap_or_default();
            self.groups = vault.list_groups().unwrap_or_default();
            self.session_groups = vault.list_session_groups().unwrap_or_default();
            self.keys = vault.list_keys().unwrap_or_default();
            self.identities = vault.list_identities().unwrap_or_default();
            self.identities_with_password = vault
                .list_identity_ids_with_password()
                .unwrap_or_default();
            self.proxy_identities = vault.list_proxy_identities().unwrap_or_default();
            self.cloud_profiles = vault.list_cloud_profiles().unwrap_or_default();

            // Plugins panel: global auto-update default from settings,
            // then rebuild the per-provider rows from the on-disk
            // cache (+ per-plugin override / pin settings).
            if let Ok(Some(v)) = vault.get_setting("plugins_auto_update_global") {
                self.plugins_auto_update_global = v != "false";
            }
            self.plugins = crate::dispatch_plugins::load_plugin_entries(
                vault,
                self.plugins_auto_update_global,
            );

            // (migration runs after the rest of the load, see end of fn)
            self.snippets = vault.list_snippets().unwrap_or_default();
            self.custom_terminal_themes =
                vault.list_custom_terminal_themes().unwrap_or_default();
            self.custom_ui_themes = vault.list_custom_ui_themes().unwrap_or_default();
            self.port_forward_rules = vault.list_port_forward_rules().unwrap_or_default();
            self.known_hosts = vault.list_known_hosts().unwrap_or_default();
            // Retention: drop events + finished recordings past the
            // configured age before the lists are loaded, so the boot
            // state never shows rows that are about to disappear.
            if let Ok(Some(code)) = vault.get_setting("logs_retention")
                && let Some(days) = Self::retention_days(&code)
            {
                let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
                match vault.prune_logs_older_than(cutoff) {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("logs retention pruned {n} rows"),
                    Err(e) => tracing::warn!("logs retention prune failed: {e}"),
                }
            }
            self.logs_total = vault.count_logs().unwrap_or(0);
            self.logs = vault.list_logs_page(self.logs_page * 50, 50).unwrap_or_default();
            self.session_logs_total = vault.count_session_logs().unwrap_or(0);
            self.session_logs = vault
                .list_session_logs_page(self.session_logs_page * 50, 50)
                .unwrap_or_default();

            // Language
            if let Ok(Some(v)) = vault.get_setting("language") {
                use crate::i18n::Language;
                Language::set_active(Language::from_code(&v));
            }

            // Layout direction (Auto / LTR / RTL). Re-hydrated after
            // unlock alongside the other UI settings so the choice
            // survives restarts.
            if let Ok(Some(v)) = vault.get_setting("layout_direction") {
                use crate::i18n::LayoutDirection;
                LayoutDirection::set_active(LayoutDirection::from_code(&v));
            }

            // App theme, re-hydrate by display name (built-in or a custom
            // UI theme, now that `custom_ui_themes` is loaded). Unknown
            // values leave the early-boot default in place, so a renamed /
            // deleted theme can never wedge the app on boot.
            if let Ok(Some(v)) = vault.get_setting("app_theme")
                && self.apply_app_theme_name(&v)
            {
                self.active_app_theme_name = v;
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_theme_override")
                && !v.is_empty()
            {
                self.terminal_theme_override = Some(v);
            }
            // Refresh the global derived palette to pick up the
            // theme + override loaded above. Per-host overrides are
            // applied lazily when each tab paints.
            self.terminal_palette = self.resolve_global_terminal_palette();

            // AI settings
            if let Ok(Some(v)) = vault.get_setting("ai_enabled") {
                self.ai_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("ai_provider") {
                self.ai_provider = v;
            }
            if let Ok(Some(v)) = vault.get_setting("ai_model") {
                self.ai_model = v;
            }
            if let Ok(Some(v)) = vault.get_setting("ai_api_url") {
                self.ai_api_url = v;
            }
            self.ai_api_key_set = vault.get_ai_api_key().ok().flatten().is_some();
            if let Ok(Some(v)) = vault.get_setting("mcp_server_enabled") {
                self.mcp_server_enabled = v == "true";
            }
            // Token MCP clients must present; empty means auth is off
            // (server allows any caller as long as the global toggle is on).
            if let Ok(Some(v)) = vault.get_setting("mcp_server_token") {
                self.mcp_server_token = v;
            }

            // Sync settings
            if let Ok(Some(v)) = vault.get_setting("sync_enabled") {
                self.sync_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sync_mode") {
                self.sync_mode = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_passwords") {
                self.sync_passwords = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("flatten_hosts") {
                self.flatten_hosts = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("hosts_sort") {
                self.hosts_sort = crate::state::ListSort::from_storage_str(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("keys_sort") {
                self.keys_sort = crate::state::ListSort::from_storage_str(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("snippets_sort") {
                self.snippets_sort = crate::state::ListSort::from_storage_str(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("sync_device_name") {
                self.sync_device_name = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_signaling_url") {
                self.sync_signaling_url = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_signaling_token") {
                self.sync_signaling_token = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_relay_url") {
                self.sync_relay_url = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_listen_port") {
                self.sync_listen_port = v;
            }
            self.sync_peers = vault.list_sync_peers().unwrap_or_default();
            if let Ok(Some(v)) = vault.get_setting("ai_system_prompt") {
                self.ai_system_prompt = text_editor::Content::with_text(&v);
            }

            // Terminal / SFTP / connection settings, load whatever
            // the user previously typed, fall back to defaults silently
            // when the key is missing (first-run or new key in update).
            // Mirrors the read in `main` (which sets WGPU_BACKEND /
            // ICED_BACKEND before the runtime starts); keep this in sync
            // so the picker shows the persisted choice, not the default.
            if let Ok(Some(v)) = vault.get_setting("renderer_backend") {
                self.setting_renderer_backend = v;
            }
            if let Ok(Some(v)) = vault.get_setting("copy_on_select") {
                self.setting_copy_on_select = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("right_click_copy") {
                self.setting_right_click_copy = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("bold_is_bright") {
                self.setting_bold_is_bright = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("keyword_highlight") {
                self.setting_keyword_highlight = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("smart_contrast") {
                self.setting_smart_contrast = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("show_status_bar") {
                self.setting_show_status_bar = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("host_list_view") {
                self.setting_host_list_view = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("card_accent_glass") {
                self.setting_card_accent_glass = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("show_host_address") {
                self.setting_show_host_address = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("close_to_tray") {
                self.setting_close_to_tray = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("minimize_to_tray") {
                self.setting_minimize_to_tray = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_close_button_side")
                && (v == "left" || v == "right")
            {
                self.setting_tab_close_button_side = v;
            }
            if let Ok(Some(v)) = vault.get_setting("pinned_tab_style")
                && (v == "compact" || v == "full")
            {
                self.setting_pinned_tab_style = v;
            }
            if let Ok(Some(v)) = vault.get_setting("show_tab_status_dot") {
                self.setting_show_tab_status_dot = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_accent_line") {
                self.setting_tab_accent_line = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_accent_wash") {
                self.setting_tab_accent_wash = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_fill_style")
                && (v == "gradient" || v == "solid")
            {
                self.setting_tab_fill_style = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_enabled") {
                self.sftp_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_split_ratio")
                && let Ok(r) = v.parse::<f32>()
            {
                self.sftp_split_ratio = r.clamp(0.15, 0.85);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_log_height")
                && let Ok(h) = v.parse::<f32>()
            {
                self.sftp.log_height =
                    h.clamp(crate::state::SFTP_LOG_MIN_H, crate::state::SFTP_LOG_MAX_H);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_columns") {
                self.sftp_columns_template.apply_visibility_storage(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_col_order") {
                self.sftp_columns_template.apply_order_storage(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_col_widths") {
                self.sftp_columns_template.apply_width_storage(&v);
            }
            // Seed the initial panes from the loaded template; later tabs are
            // seeded at creation (see `seed_sftp_columns`).
            self.sftp.left.columns = self.sftp_columns_template.clone();
            self.sftp.right.columns = self.sftp_columns_template.clone();
            // Vault nav orientation. Prefer the new `nav_orientation`
            // setting; if it's absent, migrate from the legacy
            // `layout_mode` (classic → vertical rail, workspace →
            // horizontal pills) so existing users keep a familiar shape.
            if let Ok(Some(v)) = vault.get_setting("nav_orientation")
                && (v == "horizontal" || v == "vertical")
            {
                self.setting_nav_orientation = v;
            } else if let Ok(Some(v)) = vault.get_setting("layout_mode") {
                self.setting_nav_orientation = if v == "classic" {
                    "vertical".into()
                } else {
                    "horizontal".into()
                };
            }
            if let Ok(Some(v)) = vault.get_setting("nav_rail_expanded") {
                self.setting_nav_rail_expanded = v == "true";
            }
            // Hotkey overrides: each action persists under
            // `hotkey_<id>` with the canonical serialized form
            // (`"ctrl+shift+n"`). Defaults already populate
            // `hotkey_bindings`, so any missing / malformed entry
            // silently falls back to the factory binding.
            for action in crate::hotkeys::HotkeyAction::all() {
                let key = format!("hotkey_{}", action.id());
                if let Ok(Some(v)) = vault.get_setting(&key)
                    && let Some(binding) = crate::hotkeys::HotkeyBinding::parse(&v)
                {
                    self.hotkey_bindings.insert(*action, binding);
                }
            }
            if let Ok(Some(v)) = vault.get_setting("default_host_icon")
                && matches!(v.as_str(), "circular" | "square" | "rounded" | "outline" | "initials")
            {
                self.setting_default_host_icon = v;
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_font_size")
                && let Ok(parsed) = v.parse::<f32>()
            {
                self.terminal_font_size = parsed.clamp(10.0, 24.0);
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_font_name")
                && !v.is_empty()
            {
                // Migrate legacy default. v0.6 shipped Source Code Pro as
                // the bundled terminal font, v0.7 replaces it with the
                // Nerd Font-patched variant (same visual base, full PUA
                // coverage). Users who never customised the picker had
                // the literal "Source Code Pro" persisted, hop them onto
                // the new bundled family so glyphs render and the picker
                // reflects what's actually loaded.
                self.terminal_font_name = if v == "Source Code Pro" {
                    "SauceCodePro Nerd Font".to_string()
                } else {
                    v
                };
            }
            if let Ok(Some(v)) = vault.get_setting("keepalive_interval") {
                self.setting_keepalive_interval = v;
            }
            // One-shot migration: 30s is the new default in this version,
            // up from the previous "0" (off). Users sitting at the old
            // default get bumped to 30 so they pick up the better idle
            // behavior automatically. Explicit non-zero choices (e.g. a
            // user who configured 60) are preserved. The sentinel makes
            // this idempotent so a user who reverts to 0 after the
            // migration isn't bumped again on next boot.
            if let Ok(None) = vault.get_setting("keepalive_default_v2_applied") {
                if self.setting_keepalive_interval == "0"
                    || self.setting_keepalive_interval.is_empty()
                {
                    self.setting_keepalive_interval = "30".into();
                    let _ = vault.set_setting("keepalive_interval", "30");
                }
                let _ = vault.set_setting("keepalive_default_v2_applied", "true");
            }
            if let Ok(Some(v)) = vault.get_setting("scrollback_rows") {
                self.setting_scrollback_rows = v;
            }
            oryxis_terminal::set_default_scrollback(
                crate::dispatch_settings::resolve_scrollback_rows(&self.setting_scrollback_rows),
            );
            if let Ok(Some(v)) = vault.get_setting("word_delimiters") {
                self.setting_word_delimiters = v;
            }
            if let Ok(Some(v)) = vault.get_setting("hint_link_click_used") {
                self.hint_link_click_used = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_auto_refresh_enabled") {
                self.setting_cloud_auto_refresh_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_auto_refresh_interval_minutes") {
                self.setting_cloud_auto_refresh_interval_minutes = v;
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_auto_archive_orphans") {
                self.setting_cloud_auto_archive_orphans = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_orphan_archive_days") {
                self.setting_cloud_orphan_archive_days = v;
            }
            if let Ok(Some(v)) = vault.get_setting("auto_reconnect") {
                self.setting_auto_reconnect = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("max_reconnect_attempts") {
                self.setting_max_reconnect_attempts = v;
            }
            if let Ok(Some(v)) = vault.get_setting("os_detection") {
                self.setting_os_detection = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("session_logging") {
                self.setting_session_logging = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("connection_history") {
                self.setting_connection_history = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("logs_retention") {
                self.setting_logs_retention = v;
            }
            if let Ok(Some(v)) = vault.get_setting("auto_check_updates") {
                self.setting_auto_check_updates = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("update_channel") {
                self.setting_update_channel = crate::update::UpdateChannel::from_setting(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_concurrency") {
                self.setting_sftp_concurrency = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_connect_timeout") {
                self.setting_sftp_connect_timeout = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_auth_timeout") {
                self.setting_sftp_auth_timeout = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_session_timeout") {
                self.setting_sftp_session_timeout = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_op_timeout") {
                self.setting_sftp_op_timeout = v;
            }
        }

        // Run cloud-layout migration after the immutable `vault` borrow
        // ends. Idempotent; only writes rows that need fixing.
        // Take ownership of the option so we hand the migration a real
        // borrow without conflicting with `&mut self`. Restored below.
        if let Some(vault) = self.vault.take() {
            self.migrate_legacy_cloud_layout(&vault);
            self.migrate_port_forwards(&vault);
            self.vault = Some(vault);
        }

        // Recreate pinned tabs (dormant; reopen on first select).
        self.restore_pinned_tabs_dormant();
    }

    /// One-shot migration of legacy inline `Connection.port_forwards` into
    /// standalone `PortForwardRule` rows (always `Local`, `auto_start =
    /// false`). The legacy field is left intact, it still raises forwards
    /// alongside the terminal session, the new rules just make the same
    /// tunnels runnable on their own. Gated by a settings flag so it runs
    /// exactly once.
    fn migrate_port_forwards(&mut self, vault: &oryxis_vault::store::VaultStore) {
        if vault
            .get_setting("port_forwards_migrated")
            .ok()
            .flatten()
            .as_deref()
            == Some("true")
        {
            return;
        }
        let rules = legacy_forwards_to_rules(&self.connections);
        let mut created = 0usize;
        for rule in &rules {
            match vault.save_port_forward_rule(rule) {
                Ok(()) => created += 1,
                Err(e) => tracing::warn!("port-forward migration: save failed: {e}"),
            }
        }
        let _ = vault.set_setting("port_forwards_migrated", "true");
        if created > 0 {
            tracing::info!("migrated {created} legacy port forward(s) into standalone rules");
            self.port_forward_rules = vault.list_port_forward_rules().unwrap_or_default();
        }
    }

    /// Best-effort persist a key/value pair to the vault. Logs failures
    /// instead of bubbling them up so a flaky disk doesn't take the
    /// whole settings panel down, the worst case is the user has to
    /// re-type on next boot.
    pub(crate) fn persist_setting(&self, key: &str, value: &str) {
        if let Some(vault) = &self.vault
            && let Err(e) = vault.set_setting(key, value)
        {
            tracing::warn!("failed to persist setting {key}: {e}");
        }
    }

    /// Persist the current column template (visibility + order + widths) so
    /// new panes/tabs inherit it across restarts.
    pub(crate) fn persist_sftp_columns(&self) {
        self.persist_setting("sftp_columns", &self.sftp_columns_template.visibility_storage());
        self.persist_setting("sftp_col_order", &self.sftp_columns_template.order_storage());
        self.persist_setting("sftp_col_widths", &self.sftp_columns_template.width_storage());
    }

    /// Snapshot the currently-pinned tabs (those with a reopenable spec) to
    /// the `pinned_tabs` setting so they reappear, dormant, next launch.
    /// Cloud / ephemeral pinned tabs have no spec and are skipped.
    pub(crate) fn persist_pinned_tabs(&self) {
        // De-duplicate by pin identity: a dormant placeholder and its
        // freshly-reopened live tab can briefly coexist (or a missed
        // replacement can leave both around), and persisting both
        // turns into duplicate chips on the next boot.
        let mut seen = std::collections::HashSet::new();
        // Persist in `tab_order` (the drag-reorderable display order) so the
        // restored pinned sequence matches what the user arranged, across both
        // terminal and SFTP tabs.
        let mut specs: Vec<crate::state::PinnedTabSpec> = Vec::new();
        for r in &self.tab_order {
            let spec = match r {
                crate::state::TabRef::Terminal(id) => self
                    .tabs
                    .iter()
                    .find(|t| t._id == *id)
                    .filter(|t| t.pinned)
                    .and_then(|t| t.pin_spec()),
                crate::state::TabRef::Sftp(id) => self
                    .sftp_tabs
                    .iter()
                    .position(|t| t.id == *id)
                    .filter(|&i| self.sftp_tabs[i].pinned)
                    .and_then(|i| self.sftp_pin_spec(i)),
            };
            if let Some(spec) = spec
                && seen.insert(spec.dedupe_key())
            {
                specs.push(spec);
            }
        }
        let json = serde_json::to_string(&specs).unwrap_or_else(|_| "[]".into());
        self.persist_setting("pinned_tabs", &json);
    }

    /// Recreate pinned tabs as dormant placeholders at boot. They show in the
    /// strip with their saved label but hold no live session; selecting one
    /// the first time reopens it (see `reopen_dormant_tab`). Called once data
    /// is loaded so the reopen path can resolve host ids.
    pub(crate) fn restore_pinned_tabs_dormant(&mut self) {
        let json = self
            .vault
            .as_ref()
            .and_then(|v| v.get_setting("pinned_tabs").ok().flatten());
        let Some(json) = json else { return };
        let specs: Vec<crate::state::PinnedTabSpec> =
            serde_json::from_str(&json).unwrap_or_default();
        if specs.is_empty() {
            return;
        }
        // Heal any duplicates an older version persisted: one chip
        // per pin identity.
        let mut seen = std::collections::HashSet::new();
        // Pre-seed with pinned tabs already in the strip so a *re-run* of
        // `load_data_from_vault` (it fires on connection save, vault reload,
        // sync, ...) doesn't recreate dormant duplicates of live/dormant tabs
        // that already exist.
        for t in self.tabs.iter().filter(|t| t.pinned) {
            if let Some(s) = t.pin_spec() {
                seen.insert(s.dedupe_key());
            }
        }
        let existing_sftp_keys: Vec<String> = (0..self.sftp_tabs.len())
            .filter(|&i| self.sftp_tabs[i].pinned)
            .filter_map(|i| self.sftp_pin_spec(i).map(|s| s.dedupe_key()))
            .collect();
        seen.extend(existing_sftp_keys);
        for spec in specs {
            if !seen.insert(spec.dedupe_key()) {
                continue;
            }
            let label = spec.label().to_string();
            if matches!(spec, crate::state::PinnedTabSpec::Sftp { .. }) {
                // SFTP pinned tabs restore into `sftp_tabs` as dormant chips;
                // they re-mount their panes on first focus (see SelectSftpTab).
                let mut tab = crate::state::SftpTab::new(label);
                tab.pinned = true;
                tab.state.left.local_path = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("/"));
                tab.pending_reopen = Some(spec);
                // Seed `tab_order` in the persisted (interleaved terminal+SFTP)
                // order so the restored strip matches what was saved, instead of
                // reconcile grouping all terminals before all SFTP tabs.
                self.tab_order.push(crate::state::TabRef::Sftp(tab.id));
                self.sftp_tabs.push(tab);
            } else {
                let tab = crate::state::TerminalTab::new_dormant_pinned(label, spec);
                self.tab_order.push(crate::state::TabRef::Terminal(tab._id));
                self.tabs.push(tab);
            }
        }
        // The tabs sit dormant in the strip; the app still boots to its
        // default view (Hosts). We deliberately do not focus a pinned tab or
        // switch to the terminal: opening always lands on Hosts, and a
        // dormant tab only connects on an explicit select.
    }

    /// Idempotent normalization of cloud-backed groups in the vault.
    ///
    /// Two legacy issues are fixed here:
    ///   1. **Wrong icon string**, early imports stored `"cloud"` /
    ///      `"si:aws"` on the provider folder and `"si:aws"` on the
    ///      ECS dynamic group. The brand-icon registry now expects
    ///      canonical ids (`"aws"`, `"ecs"`, `"kubernetes"`).
    ///   2. **Flat layout**, early imports left dynamic groups at
    ///      root with `parent_id = None`. We now nest them under the
    ///      provider folder.
    ///
    /// This walks `self.groups` once, mutates rows that need it, and
    /// rewrites them via `save_group` (no-op if nothing changed).
    fn migrate_legacy_cloud_layout(&mut self, vault: &oryxis_vault::store::VaultStore) {
        // Snapshot so we can mutate the vault while iterating logic.
        let groups_snapshot = self.groups.clone();
        let profiles = self.cloud_profiles.clone();

        // Provider folders → canonical icon. A provider folder is any
        // group whose label matches a profile's label *and* has no
        // `cloud_query` itself (so we don't conflate it with a
        // dynamic group named the same).
        for g in &groups_snapshot {
            if g.cloud_query.is_some() {
                continue;
            }
            let Some(matching_profile) =
                profiles.iter().find(|p| p.label == g.label)
            else {
                continue;
            };
            let canonical = matching_profile.provider.as_str();
            let needs_update = g
                .icon
                .as_deref()
                .map(|cur| cur != canonical)
                .unwrap_or(true);
            if needs_update {
                let mut updated = g.clone();
                updated.icon = Some(canonical.to_string());
                let _ = vault.save_group(&updated);
            }
        }

        // Dynamic groups → canonical icon + parented under their
        // provider folder.
        for g in &groups_snapshot {
            let Some(query) = g.cloud_query.as_ref() else {
                continue;
            };
            let canonical_icon = match query.kind {
                oryxis_core::models::cloud::CloudQueryKind::EcsTasks { .. } => "ecs",
                oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. } => "kubernetes",
            };

            // Find the provider folder this dynamic group should live
            // under (= the manual folder named after the profile).
            // Re-fetch from the freshly-mutated vault list so a folder
            // we just renamed in pass 1 above still resolves.
            let parent_id = profiles
                .iter()
                .find(|p| p.id == query.profile_id)
                .and_then(|p| {
                    groups_snapshot
                        .iter()
                        .find(|gg| gg.label == p.label && gg.cloud_query.is_none())
                        .map(|gg| gg.id)
                });

            let icon_wrong = g
                .icon
                .as_deref()
                .map(|cur| cur != canonical_icon)
                .unwrap_or(true);
            let parent_wrong = parent_id.is_some() && g.parent_id != parent_id;

            if icon_wrong || parent_wrong {
                let mut updated = g.clone();
                updated.icon = Some(canonical_icon.to_string());
                if let Some(pid) = parent_id {
                    updated.parent_id = Some(pid);
                }
                let _ = vault.save_group(&updated);
            }
        }

        // Re-pull groups so the in-memory state matches what we just
        // wrote (icons + parent ids).
        self.groups = vault.list_groups().unwrap_or_default();
    }
}

/// Pure mapping from legacy inline `Connection.port_forwards` to standalone
/// `PortForwardRule`s. Every legacy forward is Local, binds `127.0.0.1` on
/// its old `local_port`, targets the old `remote_host:remote_port`, and is
/// created with `auto_start = false`. Kept separate from the vault I/O so
/// the mapping is unit-testable.
fn legacy_forwards_to_rules(
    conns: &[oryxis_core::models::connection::Connection],
) -> Vec<oryxis_core::models::port_forward_rule::PortForwardRule> {
    use oryxis_core::models::port_forward_rule::{ForwardKind, PortForwardRule};
    let mut rules = Vec::new();
    for conn in conns {
        for pf in &conn.port_forwards {
            let mut rule = PortForwardRule::new(
                format!("{} :{}", conn.label, pf.local_port),
                ForwardKind::Local,
                conn.id,
            );
            rule.listen_host = "127.0.0.1".into();
            rule.listen_port = pf.local_port;
            rule.target_host = pf.remote_host.clone();
            rule.target_port = pf.remote_port;
            rule.auto_start = false;
            rules.push(rule);
        }
    }
    rules
}

#[cfg(test)]
mod port_forward_migration_tests {
    use super::legacy_forwards_to_rules;
    use oryxis_core::models::connection::{Connection, PortForward};
    use oryxis_core::models::port_forward_rule::ForwardKind;

    #[test]
    fn maps_each_legacy_forward_to_a_local_rule() {
        let mut conn = Connection::new("db-box", "10.0.0.1");
        conn.port_forwards = vec![
            PortForward { local_port: 5432, remote_host: "127.0.0.1".into(), remote_port: 5432 },
            PortForward { local_port: 6379, remote_host: "cache.internal".into(), remote_port: 6379 },
        ];
        let other = Connection::new("no-forwards", "10.0.0.2");

        let rules = legacy_forwards_to_rules(&[conn.clone(), other]);

        // Two forwards on one connection, none on the other.
        assert_eq!(rules.len(), 2);
        for r in &rules {
            assert_eq!(r.kind, ForwardKind::Local);
            assert_eq!(r.host_id, conn.id);
            assert_eq!(r.listen_host, "127.0.0.1");
            assert!(!r.auto_start);
        }
        assert_eq!(rules[0].listen_port, 5432);
        assert_eq!(rules[0].target_host, "127.0.0.1");
        assert_eq!(rules[0].target_port, 5432);
        assert_eq!(rules[1].listen_port, 6379);
        assert_eq!(rules[1].target_host, "cache.internal");
        assert_eq!(rules[1].target_port, 6379);
    }

    #[test]
    fn no_forwards_yields_no_rules() {
        let conn = Connection::new("plain", "10.0.0.3");
        assert!(legacy_forwards_to_rules(&[conn]).is_empty());
    }
}
