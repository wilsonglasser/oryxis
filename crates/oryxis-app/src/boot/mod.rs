//! `Oryxis::boot`, vault data hydration, and the `persist_setting`
//! best-effort writer. Pulled out of `app.rs` so the remaining file is
//! mostly the message dispatch + view plumbing.

use iced::keyboard;
use iced::widget::{svg, text_editor};
use iced::{Point, Task};

use oryxis_vault::VaultStore;

use crate::app::{TabsMessage, SshMessage, UpdateMessage, Message, Oryxis, AUTO_CONNECT, AUTO_PASSWORD};
use crate::state::{ConnectionForm, SettingsSection, VaultState, View};

mod load;
mod migrate;
mod persist;

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
        // Biometric-unlock state, hydrated pre-unlock (like theme /
        // language) so the lock screen can offer the affordance without
        // waiting for the master password. `load_data_from_vault` no
        // longer re-probes availability (it is stable per session).
        let mut biometric_unlock_enabled = false;
        let mut biometric_available = false;
        // Window geometry persisted by `persist_window_geometry`. main()
        // already applied these to the OS window before iced booted; the
        // state fields must agree so the custom chrome renders the right
        // maximize/restore glyph and border width from the first frame.
        let mut restored_window_size = iced::Size::new(1200.0, 750.0);
        let mut restored_window_pos: Option<iced::Point> = None;
        let mut restored_maximized = false;
        let mut restored_fullscreen = false;
        // Update-check settings, hydrated pre-unlock: the boot
        // `CheckForUpdate` fires while the vault can still be locked, so
        // reading these only in `load_data_from_vault` made a locked-vault
        // boot check run on the default channel (Stable) and ignore a
        // disabled auto-check. A nightly binary checked on Stable is
        // always offered the latest stable (the un-strand rule in
        // `update::check_stable`), which nagged nightly users with a
        // same-version "update" on every boot.
        let mut auto_check_updates = true;
        let mut update_channel = crate::update::UpdateChannel::default();

        // Language baseline before any vault read: follow the OS locale
        // (English when unsupported), so the very first boot, the setup
        // screen and even a vault that failed to open render localized
        // from the first frame. A persisted concrete choice overrides
        // it below; the persisted "auto" keeps it.
        crate::i18n::Language::set_active(crate::i18n::detect_os_language());

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
            {
                use crate::i18n::Language;
                // "auto" keeps the OS-locale baseline set above; a
                // concrete code is an explicit user choice. A missing
                // row (first run, or a pre-0.10 vault that never
                // touched the picker) reads as Auto and is made
                // explicit so the Settings picker shows the saved
                // "Auto (OS)" state.
                match v.get_setting("language").ok().flatten() {
                    Some(code) if code != "auto" => {
                        Language::set_active(Language::from_code(&code));
                    }
                    Some(_) => {}
                    None => {
                        let _ = v.set_setting("language", "auto");
                    }
                }
            }
            if let Ok(Some(code)) = v.get_setting("layout_direction") {
                use crate::i18n::LayoutDirection;
                LayoutDirection::set_active(LayoutDirection::from_code(&code));
            }
            // Biometric unlock: read the opt-in and probe platform support
            // now, so the lock screen can show the affordance before the
            // vault is unlocked. Availability is a provider-level check
            // (no vault account needed) and stable for the session.
            if let Ok(Some(flag)) = v.get_setting("biometric_unlock_enabled") {
                biometric_unlock_enabled = flag == "true";
            }
            biometric_available = oryxis_biometric::default_provider().is_available();
            if let Ok(Some(flag)) = v.get_setting("auto_check_updates") {
                auto_check_updates = flag == "true";
            }
            if let Ok(Some(c)) = v.get_setting("update_channel") {
                update_channel = crate::update::UpdateChannel::from_setting(&c);
            }
            // Same clamp as main(): a corrupt row must not produce a
            // degenerate size (it feeds terminal layout math via
            // `window_size` before the first Resized event lands).
            if let (Ok(Some(w)), Ok(Some(h))) = (
                v.get_setting("window_width"),
                v.get_setting("window_height"),
            ) && let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>())
                && w.is_finite()
                && h.is_finite()
            {
                restored_window_size =
                    iced::Size::new(w.clamp(800.0, 16384.0), h.clamp(500.0, 16384.0));
            }
            if let (Ok(Some(x)), Ok(Some(y))) = (
                v.get_setting("window_pos_x"),
                v.get_setting("window_pos_y"),
            ) && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
                && x.is_finite()
                && y.is_finite()
            {
                restored_window_pos = Some(iced::Point::new(x, y));
            }
            restored_maximized = matches!(
                v.get_setting("window_maximized").ok().flatten().as_deref(),
                Some("true")
            );
            restored_fullscreen = matches!(
                v.get_setting("window_fullscreen").ok().flatten().as_deref(),
                Some("true")
            );
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
        let gcp_provider =
            std::sync::Arc::new(crate::plugins::PluginProvider::new("gcp"));
        let azure_provider =
            std::sync::Arc::new(crate::plugins::PluginProvider::new("azure"));
        let plugin_providers = {
            let mut m: std::collections::HashMap<
                String,
                std::sync::Arc<crate::plugins::PluginProvider>,
            > = std::collections::HashMap::new();
            m.insert("aws".to_string(), aws_provider.clone());
            m.insert("k8s".to_string(), k8s_provider.clone());
            m.insert("gcp".to_string(), gcp_provider.clone());
            m.insert("azure".to_string(), azure_provider.clone());
            m
        };
        let cloud_provider_registry = {
            let mut reg = oryxis_cloud::CloudProviderRegistry::new();
            reg.register(aws_provider.clone());
            reg.register(k8s_provider.clone());
            reg.register(gcp_provider.clone());
            reg.register(azure_provider.clone());
            std::sync::Arc::new(reg)
        };

        let (mut app, task) = (
            Self {
                vault,
                vault_ui: crate::state::VaultUi {
                    state: vault_state,
                    has_user_password: vault_has_user_password,
                    // Pre-check the onboarding form's biometric opt-in
                    // whenever the platform can service it.
                    setup_enable_biometric: biometric_available,
                    ..Default::default()
                },
                // Vector logo: rendered through iced's SVG (resvg) path so
                // it stays crisp at any scale and avoids the wgpu image-atlas
                // corruption seen on GNOME Wayland fractional scaling. Both
                // handles share the one asset; the SVG scales to each call
                // site's box.
                logo_handle: svg::Handle::from_memory(include_bytes!("../../../../resources/logo.svg").as_slice()),
                connections: Vec::new(),
                quick_connects: std::collections::HashMap::new(),
                groups: Vec::new(),
                session_groups: Vec::new(),
                active_view: View::Dashboard,
                active_group: None,
                host_search: String::new(),
                quick_host_input: String::new(),
                tabs: Vec::new(),
                next_tab_insert_at: None,
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
                palette: crate::state::PaletteState::default(),
                show_burger_menu: false,
                show_subnav_overflow: false,
                show_icon_picker: false,
                icon_picker: crate::state::IconPickerState::default(),
                icon_color_popover: None,
                show_theme_picker: false,
                onboarding_slide: 0,
                show_chain_editor: false,
                chain_editor_adding: false,
                chain_editor_search: String::new(),
                connecting: None,
                connect_anim_tick: 0,
                last_window_press_at: None,
                pending_legacy_algo: None,
                pending_host_key: None,
                host_key_response_tx: None,
                active_host_key_tx: None,
                pending_kbi_prompt: None,
                pending_kbi_quick: None,
                pending_auth_switch: None,
                pending_edit_cancel: false,
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
                keynav: crate::keynav::KeyNavState::default(),
                hovered_folder_card: None,
                hovered_key_card: None,
                hovered_identity_card: None,
                hovered_snippet_card: None,
                hovered_history_card: None,
                hovered_files_row: None,
                sftp_open_at_path: None,
                sftp_click_gen: 0,
                sftp_edit_reopen: None,
                command_history: Vec::new(),
                command_history_host: None,
                cmd_history_search: String::new(),
                snippet_context_menu: None,
                card_context_menu: None,
                overlay: None,
                folder_rename: None,
                tab_rename: None,
                pending_paste: None,
                pending_terminal_drops: Vec::new(),
                group_edit: crate::state::GroupEditForm::default(),
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
                hybrid_sftp_owner: None,
                tab_order: Vec::new(),
                tab_mru: Vec::new(),
                tab_cycle: None,
                routing_sftp: None,
                hovered_sftp_tab: None,
                pending_sftp_close: None,
                mouse_position: Point::ORIGIN,
                window_size: restored_window_size,
                window_windowed_size: restored_window_size,
                window_windowed_pos: restored_window_pos,
                window_focused: true,
                ssm_keepalive_base: None,
                window_maximized: restored_maximized,
                window_fullscreen: restored_fullscreen,
                // Restoring straight into fullscreen re-shows the
                // "Press F11 to exit" hint (auto-hide task below), so
                // the user is never trapped in a chromeless window.
                fullscreen_hint_visible: restored_fullscreen,
                hotkey_bindings: crate::hotkeys::default_bindings(),
                editing_hotkey: None,
                modifiers: keyboard::Modifiers::default(),
                alt_sides: crate::key_encode::OptionSides::default(),
                #[cfg(target_os = "windows")]
                last_printscreen: None,
                keys: Vec::new(),
                show_key_panel: false,
                key_import_content: text_editor::Content::new(),
                key_import_public_content: text_editor::Content::new(),
                key_import_cert_content: text_editor::Content::new(),
                key_import_form: crate::state::KeyImportForm::default(),
                cert_viewer: None,
                key_generate_form: crate::state::KeyGenerateForm::default(),
                show_key_generate_panel: false,
                key_error: None,
                key_success: None,
                key_context_menu: None,
                key_search: String::new(),
                snippet_search: String::new(),
                history_search: String::new(),
                history_search_content: false,
                history_content: crate::state::HistoryContentSearch::default(),
                history_filter_tags: Vec::new(),
                identities: Vec::new(),
                identities_with_password: std::collections::HashSet::new(),
                show_identity_panel: false,
                identity_form: crate::state::IdentityForm::default(),
                identity_context_menu: None,
                show_keychain_add_menu: false,
                hosts_sort: crate::state::ListSort::default(),
                keys_sort: crate::state::ListSort::default(),
                snippets_sort: crate::state::ListSort::default(),
                proxy_identities: Vec::new(),
                proxy_identity_form: crate::state::ProxyIdentityForm::default(),
                cloud_profiles: Vec::new(),
                cloud_form: crate::state::CloudForm::default(),
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
                group_edit_parent_combo_bounds: crate::widgets::new_bounds_cell(),
                plus_btn_bounds: crate::widgets::new_bounds_cell(),
                host_tag_filter_btn_bounds: crate::widgets::new_bounds_cell(),
                snippet_tag_filter_btn_bounds: crate::widgets::new_bounds_cell(),
                history_tag_filter_btn_bounds: crate::widgets::new_bounds_cell(),
                toolbar_split_btn_bounds: crate::widgets::new_bounds_cell(),
                toolbar_sort_btn_bounds: crate::widgets::new_bounds_cell(),
                toolbar_overflow_btn_bounds: crate::widgets::new_bounds_cell(),
                host_filter_cloud_profile: None,
                host_filter_tags: Vec::new(),
                cloud_import_confirm_visible: false,
                cloud_dynamic_group_state: std::collections::HashMap::new(),
                cloud_dynamic_form: crate::state::CloudDynamicForm::default(),
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
                hovered_builtin_theme_card: None,
                theme_color_popover: None,
                show_theme_import: false,
                theme_import_content: text_editor::Content::new(),
                theme_import_name: String::new(),
                theme_import_error: None,
                monitor: Default::default(),
                monitor_stamp: 0,
                monitor_error: None,
                monitor_ports_open: false,
                monitor_disks_open: true,
                setting_monitor_interval: "5".to_string(),
                setting_sftp_default_editor: String::new(),
                setting_sftp_edit_autosave: false,
                sftp_edit_upload_all: false,
                show_ui_theme_import: false,
                ui_theme_import_content: text_editor::Content::new(),
                ui_theme_import_name: String::new(),
                ui_theme_import_error: None,
                ui_theme_editor: None,
                ui_color_popover: None,
                hovered_ui_theme_card: None,
                hovered_builtin_ui_theme_card: None,
                active_app_theme_name: "Oryxis Dark".to_string(),
                known_hosts: Vec::new(),
                logs: Vec::new(),
                logs_page: 0,
                logs_total: 0,
                clear_history_confirm: false,
                session_logs: Vec::new(),
                chat_conversations: Vec::new(),
                chat_viewer: None,
                session_logs_page: 0,
                session_logs_total: 0,
                viewing_session_log: None,
                session_player: None,
                gif_export: crate::state::GifExportState::default(),
                hovered_log_row: None,
                show_snippet_panel: false,
                snippet_label: String::new(),
                snippet_command: text_editor::Content::new(),
                snippet_group: String::new(),
                snippet_group_combo: iced::widget::combo_box::State::new(Vec::new()),
                snippet_tags_input: String::new(),
                snippet_editing_id: None,
                pending_snippet_vars: None,
                snippet_hotkey: None,
                snippet_hotkey_capturing: false,
                snippet_error: None,
                port_forward_rules: Vec::new(),
                active_forwards: std::collections::HashMap::new(),
                remote_desktop_forwards: std::collections::HashMap::new(),
                remote_desktop_seq: 0,
                remote_desktop_enabled: false,
                port_forward_starting: std::collections::HashSet::new(),
                port_forward_retry: std::collections::HashMap::new(),
                port_forward_agent_watch: None,
                show_port_forward_panel: false,
                port_forward_form: crate::state::PortForwardRuleForm::default(),
                hovered_port_forward_card: None,
                port_forward_context_menu: None,
                port_forward_search: String::new(),
                cloud_search: String::new(),
                proxy_search: String::new(),
                terminal_palette: oryxis_terminal::TerminalPalette::default(),
                terminal_theme_override: None,
                local_terminal_theme: None,
                terminal_font_size: 14.0,
                terminal_font_name: "SauceCodePro Nerd Font".to_string(),
                settings_section: SettingsSection::Interface,
                settings_tab_open: false,
                hovered_settings_tab: false,
                settings_scroll: std::collections::HashMap::new(),
                settings_search: String::new(),
                settings_active_match: 0,
                setting_renderer_backend: "auto".to_string(),
                renderer_active: None,
                setting_copy_on_select: true,
                setting_careful_paste: true,
                setting_paste_guard: true,
                setting_right_click_copy: false,
                setting_terminal_right_click: crate::util::RightClickMode::default(),
                setting_scrollback_reset_keypress: true,
                setting_scrollback_reset_output: false,
                setting_bold_is_bright: true,
                setting_pane_border_inactive: true,
                setting_pane_gap: "0".to_string(),
                show_terminal_theme_gallery: false,
                show_ui_theme_gallery: false,
                files_recent_folders: std::collections::HashMap::new(),
                setting_keyword_highlight: true,
                setting_command_history: true,
                setting_command_history_file: false,
                setting_snippet_tag_filter: false,
                snippet_filter_tags: Vec::new(),
                active_snippet_group: None,
                sidebar_snippet_group: None,
                setting_command_history_file_dir: None,
                setting_zmodem_download_dir: String::new(),
                setting_performance_mode: false,
                setting_perf_overlay: false,
                pending_perf_mode_toast: false,
                setting_smart_contrast: true,
                setting_bell_mode: crate::util::BellMode::default(),
                setting_clipboard_access: crate::util::ClipboardAccess::default(),
                setting_notification_mode: crate::util::NotificationMode::default(),
                setting_smart_tabs: true,
                setting_smart_long_secs: 10,
                setting_show_status_bar: true,
                setting_status_show_version: true,
                setting_status_show_connection: true,
                setting_status_bar_align_left: false,
                setting_status_show_latency: false,
                setting_status_show_dimensions: false,
                setting_status_show_cwd: false,
                setting_terminal_sidebar_left: false,
                setting_sidebar_auto_open: false,
                setting_sidebar_default_tab: None,
                setting_monitor_status_bar: false,
                setting_host_list_view: false,
                setting_card_accent_glass: true,
                setting_show_host_address: false,
                setting_show_tab_host_address: false,
                privacy: crate::state::PrivacyState::default(),
                setting_debug_logging: false,
                download_mirror: Default::default(),
                agent: crate::state::AgentState::default(),
                setting_close_to_tray: false,
                setting_minimize_to_tray: false,
                tray_menu_signature: 0,
                jumplist_signature: 0,
                jumplist_window_tagged: false,
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
                setting_tab_accent_text: true,
                setting_tab_accent_color: "host".into(),
                setting_tab_fill_style: "gradient".into(),
                setting_tab_bar_position: "top".into(),
                setting_inactive_tab_style: "none".into(),
                setting_tab_width_mode: "adaptive".into(),
                setting_tab_uniform_size: "medium".into(),
                setting_pinned_tabs_top_bar: false,
                setting_side_hide_top_bar: false,
                setting_side_full_height: false,
                setting_host_monitoring: false,
                setting_host_monitoring_seeded: false,
                setting_monitor_all_hosts: false,
                sftp_enabled: true,
                // Workspace is the v0.7 default. Existing users who
                // never persisted `layout_mode` also fall through to
                // this default on next launch (no migration row
                // Vault nav orientation: horizontal pill strip by default.
                setting_nav_orientation: "horizontal".into(),
                setting_language_choice: "auto".into(),
                setting_nav_rail_expanded: false,
                setting_default_host_icon: "circular".into(),
                setting_keepalive_interval: "30".into(),
                setting_default_agent_forwarding: false,
                setting_default_port: "22".into(),
                setting_default_keepalive: String::new(),
                setting_default_terminal_type: "xterm-256color".into(),
                setting_default_username: String::new(),
                setting_default_auth_method:
                    oryxis_core::models::connection::AuthMethod::Auto,
                setting_default_identity_id: None,
                setting_default_key_id: None,
                setting_default_group_id: None,
                setting_default_proxy_identity_id: None,
                // Mirror `Connection::new` / `ConnectionForm::default`, which
                // expose new hosts via MCP by default.
                setting_default_mcp_enabled: true,
                setting_default_encoding: None,
                setting_default_env_vars: Vec::new(),
                setting_defaults_collapsed: false,
                setting_cloud_auto_refresh_enabled: false,
                setting_cloud_auto_refresh_interval_minutes: "30".into(),
                setting_cloud_auto_archive_orphans: false,
                setting_cloud_orphan_archive_days: "7".into(),
                setting_scrollback_rows: "10000".into(),
                setting_word_delimiters: oryxis_terminal::DEFAULT_WORD_DELIMITERS.into(),
                revealed_secrets: std::collections::HashSet::new(),
                setting_hint_mode: crate::util::HintMode::default(),
                setting_sftp_concurrency: "2".into(),
                setting_sftp_force_osc7: false,
                setting_sftp_ask_download_dir: false,
                setting_sftp_connect_timeout: "15".into(),
                setting_sftp_auth_timeout: "30".into(),
                setting_sftp_session_timeout: "10".into(),
                setting_sftp_op_timeout: "30".into(),
                setting_auto_reconnect: true,
                setting_max_reconnect_attempts: "5".into(),
                setting_auto_lock_minutes: "0".into(),
                last_user_activity: std::time::Instant::now(),
                setting_biometric_unlock_enabled: biometric_unlock_enabled,
                biometric_available,
                setting_os_detection: true,
                setting_session_logging: false,
                setting_session_log_full: true,
                setting_session_log_compress: true,
                setting_connection_history: false,
                setting_logs_retention: "off".into(),
                setting_auto_check_updates: auto_check_updates,
                setting_update_channel: update_channel,
                pending_update: None,
                update_downloading: false,
                update_progress: 0.0,
                update_error: None,
                update_check_status: None,
                reconnect_counters: std::collections::HashMap::new(),
                ai: crate::state::AiState::default(),
                toast: None,
                toast_deadline: None,
                loaded_cjk_fonts: std::collections::HashSet::new(),
                error_dialog: None,
                local_terminals: None,
                local_terminal_default: None,
                local_terminal_form: crate::state::LocalTerminalForm::default(),
                local_terminal_add_open: false,
                hovered_local_terminal_card: None,
                local_shell_picker_open: false,
                chat_input: text_editor::Content::new(),
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
                mcp: crate::state::McpState::default(),
                sync: crate::state::SyncState::default(),
                flatten_hosts: true,
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
                sftp_backup: crate::state::SftpBackupForm::default(),
                ssh_config_import_status: None,
                show_share_dialog: false,
                share: crate::state::ShareForm::default(),
                ssh_import_hosts: Vec::new(),
                ssh_import_selected: Vec::new(),
                ssh_import_existing: Vec::new(),
                show_ssh_import_dialog: false,
            },
            Task::none(),
        );

        // If auto-unlocked (no user password), load data immediately
        if app.vault_ui.state == VaultState::Unlocked {
            app.load_data_from_vault();
        }

        // If we were launched with `--connect <uuid>` AND the vault is
        // already open (no master password), kick off the connect right
        // after boot. When the vault is locked, we defer until VaultUnlock
        // succeeds (handled in that branch).
        let mut tasks = vec![task, Task::done(Message::Update(UpdateMessage::CheckForUpdate))];
        // A Windows nightly self-replace that fails after the app has
        // exited has no UI left to report to; the helper leaves a marker
        // in TEMP instead. Surface it here so a failed swap is never
        // silent (the boot check above re-offers the same build, so the
        // user can just try again).
        if let Some(detail) = crate::update::take_update_failure() {
            tracing::warn!(
                target = "oryxis::update",
                detail = %detail,
                "previous self-update failed after exit",
            );
            app.set_toast(crate::i18n::t("update_replace_failed").to_string());
        }
        if app.vault_ui.state == VaultState::Unlocked
            && let Some(connect_id) = app.pending_auto_connect.take()
            && let Some(idx) = app
                .connections
                .iter()
                .position(|c| c.id == connect_id)
        {
            tasks.push(Task::done(Message::Ssh(SshMessage::ConnectSsh(idx))));
        }
        // Bring the sync engine up if the vault is already open and the
        // user left sync enabled. When the vault is locked we defer to
        // the `VaultUnlock` handler, same as `--connect`.
        // Only the P2P transport runs a background engine; the SFTP
        // transport reconciles on the iced cadence subscription instead.
        if app.vault_ui.state == VaultState::Unlocked
            && app.sync.enabled
            && app.sync.transport != "sftp"
        {
            tasks.push(app.start_sync_engine());
        }

        // Auto-start port forward rules marked `auto_start`. Deferred to
        // `VaultUnlock` when the vault is locked, same as sync / --connect.
        if app.vault_ui.state == VaultState::Unlocked {
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
        if app.vault_ui.state == VaultState::Unlocked {
            tasks.extend(app.spawn_plugin_unlock_tasks());
        }
        // One-time performance-mode auto-enable notice, for the
        // auto-unlocked (no-password) vault. The password path shows it
        // from the `VaultUnlock` handler instead.
        tasks.push(app.take_perf_mode_toast_task());

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

        // A restored position may reference a monitor that is gone
        // (undocked laptop, unplugged display). Verify shortly after the
        // window is up and pull it back on-screen if so. The delay lets
        // the WM finish placing the window first; skipped entirely when
        // nothing was restored (Default position is always visible).
        // Not needed while maximized / fullscreen: the OS resolves those
        // against a real monitor on its own, and the check would compare
        // the *windowed* rectangle nobody is looking at.
        if app.window_windowed_pos.is_some()
            && !app.window_maximized
            && !app.window_fullscreen
        {
            tasks.push(Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                },
                |_| Message::Tabs(TabsMessage::WindowEnsureOnScreen),
            ));
        }

        // The boot constructor set the F11 hint visible when restoring
        // into fullscreen; schedule the same 3 s auto-hide the F11
        // toggle handler uses.
        if app.window_fullscreen {
            tasks.push(Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                },
                |_| Message::Tabs(TabsMessage::FullscreenHintHide),
            ));
        }

        // Populate the unified strip order from the restored (dormant pinned)
        // tabs before the first render; subsequent messages keep it in sync via
        // `reconcile_tab_order` at the end of `update`.
        app.reconcile_tab_order();
        // Booting straight onto the lock screen: put the keyboard in
        // the master-password field (same auto-focus as LockVault /
        // AutoLockVault, so the password is typeable without a click).
        if app.vault_ui.state == crate::state::VaultState::Locked {
            tasks.push(iced::widget::operation::focus(iced::widget::Id::new(
                "vault-unlock-password",
            )));
        }
        let boot_task = Task::batch(tasks);
        (app, boot_task)
    }

    /// If [`load_data_from_vault`](Self::load_data_from_vault) just
    /// auto-enabled performance mode for this GPU stack, raise the
    /// one-time explaining toast and return its auto-clear task.
    /// Returns [`Task::none`] otherwise. Called from every unlock path
    /// so whichever one the user hits shows the notice once.
    pub(crate) fn take_perf_mode_toast_task(&mut self) -> Task<Message> {
        if !std::mem::take(&mut self.pending_perf_mode_toast) {
            return Task::none();
        }
        self.set_toast(crate::i18n::t("perf_mode_auto_toast").to_string());
        Task::perform(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(4000)).await;
            },
            |_| Message::ToastClear,
        )
    }
}
