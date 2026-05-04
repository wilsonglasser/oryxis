//! `Oryxis::boot`, vault data hydration, and the `persist_setting`
//! best-effort writer. Pulled out of `app.rs` so the remaining file is
//! mostly the message dispatch + view plumbing.

use iced::keyboard;
use iced::widget::{image, text_editor};
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
        // Inherited master password from the parent's stdin pipe — used
        // to silently unlock the vault below so the user doesn't have to
        // re-type for the spawned window.
        let inherited_password = AUTO_PASSWORD.get().cloned();

        let mut vault = VaultStore::open_default().ok();
        let mut vault_state = VaultState::Loading;
        let mut vault_has_user_password = false;

        if let Some(v) = &mut vault {
            if !v.is_initialized() {
                // Brand new vault — show setup screen
                vault_state = VaultState::NeedSetup;
            } else {
                // Vault exists — try opening without password first
                match v.open_without_password() {
                    Ok(()) => {
                        vault_state = VaultState::Unlocked;
                        vault_has_user_password = false;
                    }
                    Err(_) => {
                        // Has a real password. Try the inherited password
                        // (from `--inherit-vault` stdin) before falling
                        // back to the lock screen.
                        let unlocked = inherited_password
                            .as_ref()
                            .is_some_and(|pw| v.unlock(pw).is_ok());
                        if unlocked {
                            vault_state = VaultState::Unlocked;
                        } else {
                            vault_state = VaultState::Locked;
                        }
                        vault_has_user_password = true;
                    }
                }
            }
            // Theme + language live in the plaintext `settings` table,
            // not behind the encryption key — so we can hydrate them
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
        }

        let (mut app, task) = (
            Self {
                vault,
                vault_state,
                vault_password_input: String::new(),
                vault_error: None,
                logo_handle: image::Handle::from_bytes(include_bytes!("../../../resources/logo_128.png").as_slice()),
                logo_small_handle: image::Handle::from_bytes(include_bytes!("../../../resources/logo_64.png").as_slice()),
                connections: Vec::new(),
                groups: Vec::new(),
                active_view: View::Dashboard,
                active_group: None,
                host_search: String::new(),
                quick_host_input: String::new(),
                sidebar_collapsed: false,
                tabs: Vec::new(),
                active_tab: None,
                hovered_tab: None,
                show_new_tab_picker: false,
                new_tab_picker_search: String::new(),
                show_tab_jump: false,
                tab_jump_search: String::new(),
                show_icon_picker: false,
                icon_picker_for: None,
                icon_picker_icon: None,
                icon_picker_color: None,
                icon_picker_hex_input: String::new(),
                connecting: None,
                connect_anim_tick: 0,
                last_window_press_at: None,
                pending_host_key: None,
                host_key_response_tx: None,
                show_host_panel: false,
                editor_form: ConnectionForm::default(),
                host_panel_error: None,
                hovered_card: None,
                card_context_menu: None,
                overlay: None,
                folder_rename: None,
                folder_delete: None,
                pending_auto_connect,
                // Keep the inherited password in memory only when the
                // unlock above actually succeeded — otherwise the user is
                // about to type their own at the lock screen.
                master_password: if vault_state == VaultState::Unlocked {
                    inherited_password
                } else {
                    None
                },
                sftp: crate::state::SftpState {
                    local_path: std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::path::PathBuf::from("/")),
                    picker_open: true,
                    ..Default::default()
                },
                mouse_position: Point::ORIGIN,
                window_size: iced::Size::new(1200.0, 750.0),
                window_maximized: false,
                modifiers: keyboard::Modifiers::default(),
                keys: Vec::new(),
                show_key_panel: false,
                key_import_label: String::new(),
                key_import_content: text_editor::Content::new(),
                key_import_pem: String::new(),
                key_error: None,
                key_success: None,
                key_context_menu: None,
                editing_key_id: None,
                key_search: String::new(),
                identities: Vec::new(),
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
                snippets: Vec::new(),
                known_hosts: Vec::new(),
                logs: Vec::new(),
                logs_page: 0,
                logs_total: 0,
                session_logs: Vec::new(),
                viewing_session_log: None,
                show_snippet_panel: false,
                snippet_label: String::new(),
                snippet_command: String::new(),
                snippet_editing_id: None,
                snippet_error: None,
                terminal_theme: oryxis_terminal::TerminalTheme::OryxisDark,
                terminal_font_size: 14.0,
                terminal_font_name: "Source Code Pro".to_string(),
                settings_section: SettingsSection::Terminal,
                setting_copy_on_select: true,
                setting_bold_is_bright: true,
                setting_keyword_highlight: true,
                setting_smart_contrast: true,
                setting_keepalive_interval: "0".into(),
                setting_scrollback_rows: "10000".into(),
                setting_sftp_concurrency: "2".into(),
                setting_sftp_connect_timeout: "15".into(),
                setting_sftp_auth_timeout: "30".into(),
                setting_sftp_session_timeout: "10".into(),
                setting_sftp_op_timeout: "30".into(),
                setting_auto_reconnect: true,
                setting_max_reconnect_attempts: "5".into(),
                setting_os_detection: true,
                setting_auto_check_updates: true,
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
                vault_password_error: None,
                vault_destroy_confirm: false,
                toast: None,
                local_shells: None,
                local_shell_picker_open: false,
                chat_input: text_editor::Content::new(),
                chat_loading: false,
                chat_scroll_at_bottom: true,
                chat_sidebar_width: 350.0,
                chat_sidebar_drag: None,
                mcp_server_enabled: false,
                show_mcp_info: false,
                mcp_config_copied: false,
                mcp_install_status: None,
                sync_enabled: false,
                sync_mode: "manual".into(),
                sync_passwords: false,
                sync_device_name: String::new(),
                sync_signaling_url: oryxis_sync::SyncConfig::default().signaling_url,
                sync_relay_url: String::new(),
                sync_listen_port: "0".into(),
                sync_peers: Vec::new(),
                sync_pairing_code: None,
                sync_status: None,
                show_export_dialog: false,
                export_password: String::new(),
                export_include_keys: true,
                export_status: None,
                show_import_dialog: false,
                import_password: String::new(),
                import_file_data: None,
                import_status: None,
                ssh_config_import_status: None,
                show_share_dialog: false,
                share_password: String::new(),
                share_include_keys: false,
                share_filter: None,
                share_status: None,
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
        let boot_task = Task::batch(tasks);
        (app, boot_task)
    }

    pub(crate) fn load_data_from_vault(&mut self) {
        if let Some(vault) = &self.vault {
            self.connections = vault.list_connections().unwrap_or_default();
            self.groups = vault.list_groups().unwrap_or_default();
            self.keys = vault.list_keys().unwrap_or_default();
            self.identities = vault.list_identities().unwrap_or_default();
            self.proxy_identities = vault.list_proxy_identities().unwrap_or_default();
            self.snippets = vault.list_snippets().unwrap_or_default();
            self.known_hosts = vault.list_known_hosts().unwrap_or_default();
            self.logs_total = vault.count_logs().unwrap_or(0);
            self.logs = vault.list_logs_page(self.logs_page * 50, 50).unwrap_or_default();
            self.session_logs = vault.list_session_logs().unwrap_or_default();

            // Language
            if let Ok(Some(v)) = vault.get_setting("language") {
                use crate::i18n::Language;
                Language::set_active(Language::from_code(&v));
            }

            // App theme — re-hydrate by display name. Unknown values
            // fall back to the default in `AppTheme::from_name`, so a
            // renamed theme can never wedge the app on boot.
            if let Ok(Some(v)) = vault.get_setting("app_theme") {
                use crate::theme::AppTheme;
                AppTheme::set_active(AppTheme::from_name(&v));
            }

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
            if let Ok(Some(v)) = vault.get_setting("sync_device_name") {
                self.sync_device_name = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_signaling_url") {
                self.sync_signaling_url = v;
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

            // Terminal / SFTP / connection settings — load whatever
            // the user previously typed, fall back to defaults silently
            // when the key is missing (first-run or new key in update).
            if let Ok(Some(v)) = vault.get_setting("copy_on_select") {
                self.setting_copy_on_select = v == "true";
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
            if let Ok(Some(v)) = vault.get_setting("keepalive_interval") {
                self.setting_keepalive_interval = v;
            }
            if let Ok(Some(v)) = vault.get_setting("scrollback_rows") {
                self.setting_scrollback_rows = v;
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
            if let Ok(Some(v)) = vault.get_setting("auto_check_updates") {
                self.setting_auto_check_updates = v == "true";
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
    }

    /// Best-effort persist a key/value pair to the vault. Logs failures
    /// instead of bubbling them up so a flaky disk doesn't take the
    /// whole settings panel down — the worst case is the user has to
    /// re-type on next boot.
    pub(crate) fn persist_setting(&self, key: &str, value: &str) {
        if let Some(vault) = &self.vault
            && let Err(e) = vault.set_setting(key, value)
        {
            tracing::warn!("failed to persist setting {key}: {e}");
        }
    }
}
