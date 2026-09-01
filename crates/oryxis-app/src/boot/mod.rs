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
        // OS `oryxis://` scheme launch with no running instance to
        // forward to: parse it now (main.rs already shape-checked) and
        // route it below once the vault state is known.
        let pending_deep_link = crate::app::PENDING_DEEP_LINK
            .get()
            .and_then(|url| crate::deep_link::parse(url));
        // `oryxis user@host` with no running instance to forward to.
        // Already shape-checked in main.rs; routed below once the vault
        // state is known, and re-stashed by its handler while locked.
        let pending_connect_target = crate::app::PENDING_CONNECT_TARGET.get().cloned();
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
                // Stamp the Ctrl+digit slot migration as already done, so
                // the boot migration in `load_data_from_vault` leaves this
                // vault on the new mapping (slot N = tab N). Only vaults
                // that predate the change reach that migration unstamped
                // and keep Home in the first slot. This is the one place
                // that can tell a fresh install from an existing one.
                let _ = v.set_setting("tab_slots_home_migrated", "true");
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
                closed_tabs: Vec::new(),
                pending_tab_placement: None,
                pending_pane_split: None,
                quick_connect_protocol:
                    oryxis_core::models::connection::ConnectionProtocol::Ssh,
                pending_local_startup: std::collections::HashMap::new(),
                split_menu_hovered: false,
                active_tab: None,
                last_terminal_tab: None,
                hover: crate::state::HoverState::default(),
                new_tab_picker_search: String::new(),
                new_tab_picker_group: None,
                tab_jump_search: String::new(),
                palette: crate::state::PaletteState::default(),
                icon_picker: crate::state::IconPickerState::default(),
                icon_color_popover: None,
                onboarding_slide: 0,
                onboarding_import_pending: false,
                chain_editor_adding: false,
                chain_editor_search: String::new(),
                connecting: None,
                connect_anim_tick: 0,
                busy_anim_tick: 0,
                last_window_press_at: None,
                pending_legacy_algo: None,
                pending_host_key: None,
                host_key_response_tx: None,
                active_host_key_tx: None,
                pending_proxy_command: None,
                proxy_command_response_tx: None,
                active_proxy_command_tx: None,
                pending_kbi_prompt: None,
                pending_kbi_quick: None,
                pending_auth_switch: None,
                pending_edit_cancel: false,
                kbi_inputs: Vec::new(),
                kbi_response_tx: None,
                editor_form: ConnectionForm::default(),
                editor_initial_command: text_editor::Content::new(),
                host_panel_error: None,
                host_editor_open_sections: std::collections::HashSet::new(),
                editor_session_group: crate::state::SessionGroupForm::default(),
                session_group_script_editor: text_editor::Content::new(),
                session_group_panel_error: None,
                pane_script_overrides: std::collections::HashMap::new(),
                keynav: crate::keynav::KeyNavState::default(),
                sftp_open_at_path: None,
                pending_files_mode: None,
                pending_console_dir: None,
                pending_console_purpose: false,
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
                pending_paste_install: None,
                drag_out_arm: None,
                drag_out_echo: Vec::new(),
                pending_terminal_drops: Vec::new(),
                os_drop_hover: false,
                group_edit: crate::state::GroupEditForm::default(),
                folder_delete: None,
                pending_auto_connect,
                pending_deep_link,
                pending_connect_target,
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
                pending_sftp_close: None,
                last_download_dir: None,
                mouse_position: Point::ORIGIN,
                window_size: restored_window_size,
                window_windowed_size: restored_window_size,
                window_windowed_pos: restored_window_pos,
                window_windowed_pos_prev: restored_window_pos,
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
                cert_viewer: None,
                snippet_search: String::new(),
                history_search: String::new(),
                history_search_content: false,
                history_content: crate::state::HistoryContentSearch::default(),
                history_filter_tags: Vec::new(),
                identities: Vec::new(),
                identities_with_password: std::collections::HashSet::new(),
                identity_form: crate::state::IdentityForm::default(),
                identity_context_menu: None,
                hosts_sort: crate::state::ListSort::default(),
                keys_sort: crate::state::ListSort::default(),
                snippets_sort: crate::state::ListSort::default(),
                proxy_identities: Vec::new(),
                proxy_identity_form: crate::state::ProxyIdentityForm::default(),
                login_scripts: Vec::new(),
                login_script_generation: 0,
                login_script_form: crate::state::LoginScriptForm::default(),
                highlight_rules_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
                highlight_rule_form: crate::state::HighlightRuleForm::default(),
                trigger_confirm: None,
                cloud_profiles: Vec::new(),
                cloud_form: crate::state::CloudForm::default(),
                editor_parent_combo: iced::widget::combo_box::State::new(Vec::new()),
                editor_startup_combo: iced::widget::combo_box::State::new(Vec::new()),
                editor_login_script_combo: iced::widget::combo_box::State::new(Vec::new()),
                editor_script_template_combo: iced::widget::combo_box::State::new(Vec::new()),
                editor_key_combo: iced::widget::combo_box::State::new(Vec::new()),
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
                install_runs: std::collections::HashMap::new(),
                custom_terminal_themes: Vec::new(),
                custom_ui_themes: Vec::new(),
                theme_ui: crate::state::ThemeEditorUi::default(),
                monitor: Default::default(),
                tmux: Default::default(),
                ssh_transport_pool: Default::default(),
                pending_reuse_keys: Default::default(),
                monitor_dash: Default::default(),
                monitor_stamp: 0,
                monitor_error: None,
                monitor_ports_open: false,
                monitor_disks_open: true,
                panels: crate::state::PanelsOpen::default(),
                cloud_discover: crate::state::CloudDiscoverUi::default(),
                snippet_form: crate::state::SnippetForm::default(),
                keys_ui: crate::state::KeysUi::default(),
                sftp_chrome: crate::state::SftpChrome::default(),
                prefs: crate::state::AppPrefs {
                    // Hydrated before the unlock (the lock screen and the
                    // boot update check both run while the vault can still
                    // be locked), so these three come from the reads above
                    // rather than from the factory defaults.
                    biometric_unlock_enabled,
                    auto_check_updates,
                    update_channel,
                    ..Default::default()
                },
                sftp_edit_upload_all: false,
                ui_theme_import_content: text_editor::Content::new(),
                ui_theme_import_name: String::new(),
                ui_theme_import_error: None,
                ui_theme_editor: None,
                ui_color_popover: None,
                active_app_theme_name: "Oryxis Dark".to_string(),
                known_hosts: Vec::new(),
                logs: Vec::new(),
                logs_page: 0,
                logs_total: 0,
                clear_history_confirm: false,
                session_logs: Vec::new(),
                chat_ui: crate::state::ChatUi::default(),
                session_logs_page: 0,
                session_logs_total: 0,
                viewing_session_log: None,
                session_player: None,
                gif_export: crate::state::GifExportState::default(),
                pending_snippet_vars: None,
                port_forward_rules: Vec::new(),
                active_forwards: std::collections::HashMap::new(),
                forward_conns: std::collections::HashMap::new(),
                pf_aborted_pending: std::collections::HashMap::new(),
                remote_desktop_forwards: std::collections::HashMap::new(),
                link_forwards: std::collections::HashMap::new(),
                link_confirm: None,
                remote_desktop_seq: 0,
                remote_desktop_enabled: false,
                port_forward_starting: std::collections::HashSet::new(),
                port_forward_retry: std::collections::HashMap::new(),
                port_forward_agent_watch: None,
                port_forward_form: crate::state::PortForwardRuleForm::default(),
                port_forward_context_menu: None,
                port_forward_search: String::new(),
                cloud_search: String::new(),
                proxy_search: String::new(),
                terminal_palette: oryxis_terminal::TerminalPalette::default(),
                terminal_theme_override: None,
                local_terminal_theme: None,
                terminal_font_size: 14.0,
                terminal_font_name: "SauceCodePro Nerd Font".to_string(),
                terminal_font_weight: crate::fonts::TerminalFontWeight::default(),
                terminal_text_thickness: crate::fonts::TextThickness::default(),
                settings_section: SettingsSection::Interface,
                open_panel_tabs: std::collections::BTreeSet::new(),
                net_tools: crate::state::NetToolsState::default(),
                settings_scroll: std::collections::HashMap::new(),
                settings_search: String::new(),
                settings_active_match: 0,
                renderer_active: None,
                files_recent_folders: std::collections::HashMap::new(),
                // Replaced by the vault's own key during
                // `load_data_from_vault`; empty until then, which is also
                // what a locked vault shows, and an empty key is never
                // installed, so nothing is captured before it is known.
                shell_integration_nonce: String::new(),
                snippet_filter_tags: Vec::new(),
                active_snippet_group: None,
                sidebar_snippet_group: None,
                pending_perf_mode_toast: false,
                privacy: crate::state::PrivacyState::default(),
                download_mirror: Default::default(),
                agent: crate::state::AgentState::default(),
                tray_menu_signature: 0,
                jumplist_signature: 0,
                jumplist_window_tagged: false,
                is_window_hidden: false,
                ipc_state_signature: 0,
                pin_next_plugin_tab: None,
                pending_ecs_autoconnect: None,
                tab_drag: None,
                panel_width: crate::app::PANEL_WIDTH,
                panel_resize_drag: None,
                editor_saved_snapshot: None,
                sftp_enabled: true,
                // Workspace is the v0.7 default. Existing users who
                // never persisted `layout_mode` also fall through to
                // this default on next launch (no migration row
                // Vault nav orientation: horizontal pill strip by default.
                // Mirror `Connection::new` / `ConnectionForm::default`, which
                // expose new hosts via MCP by default.
                revealed_secrets: std::collections::HashSet::new(),
                last_user_activity: std::time::Instant::now(),
                last_session_log_capacity_check: std::time::Instant::now(),
                last_unlock: None,
                biometric_available,
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
                loaded_pack_fonts: std::collections::HashSet::new(),
                error_dialog: None,
                local_terminals: None,
                local_terminal_default: None,
                local_terminal_form: crate::state::LocalTerminalForm::default(),
                local_terminal_add_open: false,
                local_shell_picker_open: false,
                terminal_sidebar_tab: [
                    // Remembered active tab per region. Every tab
                    // defaults to the RIGHT region, so the left seed
                    // only matters once the user docks something
                    // there (the hosts tree being the likely mover);
                    // both re-resolve against the region's actual
                    // offers on every read.
                    crate::state::TerminalSidebarTab::HostsTree,
                    crate::state::TerminalSidebarTab::default(),
                ],
                sidebar_snippet_search: String::new(),
                sidebar_sort_open: false,
                sidebar_search_open: false,
                hosts_tree_expanded: std::collections::HashSet::new(),
                hosts_tree_search: String::new(),
                mcp: crate::state::McpState::default(),
                sync: crate::state::SyncState::default(),
                flatten_hosts: true,
                export_password: String::new(),
                export_include_keys: true,
                export_selection: oryxis_vault::ExportSelection::all(),
                export_status: None,
                vault_import: crate::state::VaultImportState::default(),
                sftp_backup: crate::state::SftpBackupForm::default(),
                ssh_config_import_status: None,
                share: crate::state::ShareForm::default(),
                ssh_import_hosts: Vec::new(),
                ssh_import_direct: None,
                import_hub_error: None,
                import_hub_pending: None,
                import_hub_password: String::new(),
                ssh_import_selected: Vec::new(),
                ssh_import_existing: Vec::new(),
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
        let mut tasks = vec![task];
        // The boot release lookup is for a person to act on, so an
        // emulated run skips it (`app::HARNESS_ACTIVE`) rather than
        // waiting on a network it does not control. The handler's own
        // `auto_check_updates` gate stays the user-facing switch; this
        // one is about who is watching.
        if !crate::app::harness_active() {
            tasks.push(Task::done(Message::Update(UpdateMessage::CheckForUpdate)));
        }
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
            // A blocking dialog, not a toast: the boot check right above
            // lands 1-2s later and its "update available" toast + modal
            // would overwrite/cover a 2.5s toast, which made a failing
            // swap look exactly like a successful update that "didn't
            // stick". The body carries the helper's captured error
            // verbatim (the actual `copy`/`move` stderr), so the user
            // sees WHY instead of a generic shrug; the update modal
            // yields while this dialog is up (root_view gates on it).
            app.error_dialog = Some(crate::state::ErrorDialog {
                title: crate::i18n::t("update_replace_failed").to_string(),
                body: detail,
                // The title's own wording tells the user to open the
                // release page, so the dialog carries the button rather
                // than leaving them to find it.
                link: Some(crate::state::ErrorDialogLink {
                    label: crate::i18n::t("download").to_string(),
                    url: format!(
                        "https://github.com/{}/releases/latest",
                        crate::update::RELEASE_REPO
                    ),
                }),
                action: None,
            });
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
        // Route a deep link the launch carried. `handle_deep_link`
        // re-stashes it by itself when the vault is still locked, so no
        // unlocked-state guard here (unlike `--connect`, whose index
        // lookup needs the loaded connections).
        if let Some(link) = app.pending_deep_link.take() {
            let route = app.handle_deep_link(link);
            tasks.push(route);
        }
        // Same for a CLI target, which `handle_connect_target` likewise
        // re-stashes while the vault is locked.
        if let Some(target) = app.pending_connect_target.take() {
            let route = app.handle_connect_target(&target);
            tasks.push(route);
        }
        // Bring the sync engine up if the vault is already open and the
        // user left sync enabled. When the vault is locked we defer to
        // the `VaultUnlock` handler, same as `--connect`.
        // Only the P2P transport runs a background engine; every
        // snapshot transport reconciles on the iced cadence
        // subscription instead.
        if app.vault_ui.state == VaultState::Unlocked
            && app.sync.enabled
            && app.sync_uses_p2p()
        {
            tasks.push(app.start_sync_engine());
        }
        // The git card's availability probe spawns a subprocess, so it
        // runs as a task rather than inside `view()`. Fire it whenever
        // the git transport can be on screen (also done on `VaultUnlock`
        // and on `TransportChanged`).
        if app.vault_ui.state == VaultState::Unlocked && app.sync.transport == "git" {
            tasks.push(crate::dispatch_git_sync::git_availability_task());
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

        // Terminal font pack (issue #109): load every already-cached
        // pack face now so a terminal picked to one of them renders
        // right from its first frame, and silently fetch the face the
        // picked family + weight needs when it isn't cached yet
        // (settings that arrived via sync / import land on a machine
        // without the file). Guard semantics mirror the CJK block
        // above.
        for (key, task) in crate::fonts::boot_pack_tasks(
            &app.terminal_font_name,
            app.terminal_font_weight,
        ) {
            app.loaded_pack_fonts.insert(key.to_string());
            tasks.push(task);
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
        // A local host names a curated terminal, and that list is
        // scanned lazily (the first time the local-shell picker opens).
        // A vault that already holds one, whether saved here or arrived
        // by sync or import, needs the list before the user clicks
        // Connect, or the host would resolve to nothing on a machine
        // that simply never looked. The rescan merges and persists; it
        // never opens a shell.
        if app.local_terminals.is_none()
            && app.connections.iter().any(|c| {
                c.protocol == oryxis_core::models::connection::ConnectionProtocol::Local
            })
        {
            tasks.push(Task::done(Message::Settings(
                crate::app::SettingsMessage::RescanLocalTerminals,
            )));
        }
        // Booting straight onto the lock screen: put the keyboard in
        // the master-password field (same auto-focus as LockVault /
        // SoftLockVault, so the password is typeable without a click).
        if app.vault_ui.state == crate::state::VaultState::Locked {
            tasks.push(crate::widgets::focus_input(iced::widget::Id::new(
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
