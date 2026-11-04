//! `Oryxis::boot`, vault data hydration, and the `persist_setting`
//! best-effort writer. Pulled out of `app.rs` so the remaining file is
//! mostly the message dispatch + view plumbing.

use iced::keyboard;
use iced::widget::svg;
use iced::{Point, Task};

use oryxis_vault::VaultStore;

use crate::app::{TabsMessage, SshMessage, UpdateMessage, Message, Oryxis, AUTO_CONNECT, AUTO_PASSWORD};
use crate::state::{SettingsSection, VaultState, View};

mod load;
mod migrate;
mod persist;

impl Oryxis {
    pub fn boot() -> (Self, Task<Message>) {
        // CLI hand-off: if the parent process started us with `--connect
        // <uuid>` (the path a JumpList entry takes), capture that
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

        // A vault that cannot be opened leaves `vault_state` at `Loading`,
        // which the root view draws as the error screen: the error goes
        // with it (and to the log and stderr), since that screen is the
        // only surface left and a report used to come with the fixed
        // label alone (issue #250).
        // The vault this start opens: the last used, or the one holding
        // the host a launch names (`--connect`, a JumpList entry), so the
        // dial after its unlock finds the host. Chosen once: the id, the
        // file and the open below agree.
        let (boot_vault_id, boot_vault_path) = match oryxis_vault::device::installed() {
            Some(device) => {
                let entry = device.boot_vault_for(pending_auto_connect);
                (entry.id, Some(device.resolve(&entry)))
            }
            None => (oryxis_vault::device::FIRST_VAULT_ID, VaultStore::default_path()),
        };
        let (mut vault, vault_open_error, vault_open_read_only, vault_open_corrupt) =
            match VaultStore::open_boot_at(boot_vault_path.clone()) {
                Ok(v) => (Some(v), None, false, false),
                Err(e) => {
                    tracing::error!("vault open failed: {e}");
                    eprintln!("oryxis: vault open failed: {e}");
                    let source = match &e {
                        oryxis_vault::VaultError::Open { source, .. } => Some(&**source),
                        _ => None,
                    };
                    let read_only = matches!(source, Some(oryxis_vault::VaultError::ReadOnly));
                    let corrupt = matches!(source, Some(oryxis_vault::VaultError::Corrupt(_)));
                    (None, Some(e.to_string()), read_only, corrupt)
                }
            };
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
        let mut restored_immersive = false;
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
        // Offline mode and the download mirror, hydrated pre-unlock for
        // the same reason: the boot fetches below run while the vault
        // can still be locked, and a locked-vault boot used to dial
        // GitHub on the default mirror whatever the user had picked.
        let mut offline_mode = false;
        let mut download_mirror = crate::net_mirror::MirrorUi::default();

        // Language baseline before any vault read: follow the OS locale
        // (English when unsupported), so the very first boot, the setup
        // screen and even a vault that failed to open render localized
        // from the first frame. A persisted concrete choice overrides
        // it below; the persisted "auto" keeps it.
        crate::i18n::Language::set_active(crate::i18n::detect_os_language());

        if let Some(v) = &mut vault {
            (vault_state, vault_has_user_password) =
                crate::vault_slots::initial_lock_state(v, inherited_password.as_deref());
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
            // The settings row wins whenever it exists. Only a data dir
            // that never answered consults the offline bundle's marker
            // beside the executable (`bundle::MARKER`), and the answer
            // is written down so a marker removed later, or an ordinary
            // install put in its place, does not flip the switch back.
            offline_mode = match v.get_setting(crate::offline::SETTING_KEY) {
                Ok(Some(flag)) => flag == "true",
                _ => {
                    let seeded = crate::bundle::marker_present();
                    if seeded {
                        let _ = v.set_setting(crate::offline::SETTING_KEY, "true");
                    }
                    seeded
                }
            };
            crate::offline::set(offline_mode);
            if let Ok(Some(m)) = v.get_setting("download_mirror") {
                let choice = crate::net_mirror::MirrorChoice::from_setting(&m);
                if let crate::net_mirror::MirrorChoice::Custom(url) = &choice {
                    download_mirror.url_input = url.clone();
                }
                download_mirror.choice = choice.clone();
                crate::net_mirror::set_choice(choice);
            }
            // The offline bundle's plugin seeds go into the cache before
            // the plugin rows are built from it, so a seeded provider
            // boots as Installed. Needs no unlock; the pin is a plaintext
            // row. A seed that became the active MCP version also has to
            // reach the stable launcher path external clients spawn.
            {
                let pinned = |id: &str| {
                    v.get_setting(&format!("plugins_{id}_pinned_version"))
                        .ok()
                        .flatten()
                        .filter(|s| !s.is_empty())
                };
                for imported in crate::plugins::seed::import_bundled(&pinned) {
                    if imported.activated
                        && imported.provider_id == "mcp"
                        && let Err(e) = crate::mcp_install::sync_launcher_from_cache()
                    {
                        tracing::warn!(
                            target = "oryxis::mcp",
                            error = %e,
                            "seeded MCP plugin could not be copied to the launcher path"
                        );
                    }
                }
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
            // An absent row predates the green button (every saved
            // fullscreen came from F11), so it reads as immersive.
            restored_immersive = restored_fullscreen
                && !matches!(
                    v.get_setting("window_fullscreen_immersive").ok().flatten().as_deref(),
                    Some("false")
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
        let aliyun_provider =
            std::sync::Arc::new(crate::plugins::PluginProvider::new("aliyun"));
        let tencent_provider =
            std::sync::Arc::new(crate::plugins::PluginProvider::new("tencent"));
        let plugin_providers = {
            let mut m: std::collections::HashMap<
                String,
                std::sync::Arc<crate::plugins::PluginProvider>,
            > = std::collections::HashMap::new();
            m.insert("aws".to_string(), aws_provider.clone());
            m.insert("k8s".to_string(), k8s_provider.clone());
            m.insert("gcp".to_string(), gcp_provider.clone());
            m.insert("azure".to_string(), azure_provider.clone());
            m.insert("aliyun".to_string(), aliyun_provider.clone());
            m.insert("tencent".to_string(), tencent_provider.clone());
            m
        };
        let cloud_provider_registry = {
            let mut reg = oryxis_cloud::CloudProviderRegistry::new();
            reg.register(aws_provider.clone());
            reg.register(k8s_provider.clone());
            reg.register(gcp_provider.clone());
            reg.register(azure_provider.clone());
            reg.register(aliyun_provider.clone());
            reg.register(tencent_provider.clone());
            std::sync::Arc::new(reg)
        };

        let (mut app, task) = (
            Self {
                vaults: crate::vault_slots::VaultSlots::single({
                    let mut first = crate::vault_slots::OpenVault::new(boot_vault_id, vault);
                    // Hydrated before the unlock (the lock screen runs
                    // while the vault can still be locked), so it comes
                    // from the read above rather than the factory default.
                    first.prefs.biometric_unlock_enabled = biometric_unlock_enabled;
                    first.vault_ui = crate::state::VaultUi {
                        state: vault_state,
                        open_error: vault_open_error,
                        open_read_only: vault_open_read_only,
                        open_corrupt: vault_open_corrupt,
                        has_user_password: vault_has_user_password,
                        // Pre-check the onboarding form's biometric opt-in
                        // whenever the platform can service it.
                        setup_enable_biometric: biometric_available,
                        ..Default::default()
                    };
                    // Keep the inherited password in memory only when the
                    // unlock above actually succeeded, otherwise the user is
                    // about to type their own at the lock screen.
                    first.master_password = if vault_state == VaultState::Unlocked {
                        inherited_password
                    } else {
                        None
                    };
                    first.path = boot_vault_path;
                    first
                }),
                // Vector logo: rendered through iced's SVG (resvg) path so
                // it stays crisp at any scale and avoids the wgpu image-atlas
                // corruption seen on GNOME Wayland fractional scaling. Both
                // handles share the one asset; the SVG scales to each call
                // site's box.
                logo_handle: svg::Handle::from_memory(include_bytes!("../../../../resources/logo.svg").as_slice()),
                active_view: View::Dashboard,
                tabs: Vec::new(),
                inline_edit_anchors: Vec::new(),
                open_tabs_signature: 0,
                zmodem_swept: false,
                pending_tab_placement: None,
                pending_pane_split: None,
                quick_connect_protocol:
                    oryxis_core::models::connection::ConnectionProtocol::Ssh,
                pending_local_startup: std::collections::HashMap::new(),
                split_menu_hovered: false,
                active_tab: None,
                last_terminal_tab: None,
                new_tab_picker_search: String::new(),
                new_tab_picker_group: None,
                tab_jump_search: String::new(),
                palette: crate::state::PaletteState::default(),
                icon_color_popover: None,
                onboarding_slide: 0,
                onboarding_import_pending: false,
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
                kbi_inputs: Vec::new(),
                kbi_response_tx: None,
                host_editor_open_sections: std::collections::HashSet::new(),
                pane_script_overrides: std::collections::HashMap::new(),
                keynav: crate::keynav::KeyNavState::default(),
                sftp_open_at_path: None,
                pending_files_mode: None,
                pending_console_dir: None,
                pending_console_purpose: false,
                sftp_click_gen: 0,
                sftp_edit_reopen: None,
                edit_fetches: Vec::new(),
                edit_fetch_seq: 0,
                cmd_history_search: String::new(),
                snippet_context_menu: None,
                card_context_menu: None,
                card_drag: None,
                card_press: crate::widgets::new_press_hit_cell(),
                overlay: None,
                tab_rename: None,
                pending_paste: None,
                pending_paste_install: None,
                pending_paste_snippet: None,
                drag_out_arm: None,
                drag_out_echo: Vec::new(),
                pending_terminal_drops: Vec::new(),
                os_drop_hover: false,
                pending_auto_connect,
                pending_deep_link,
                pending_connect_target,
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
                nav: crate::window_ctx::WindowNav::default(),
                parked_views: crate::window_ctx::ParkedViews::new(),
                vault_dialog: None,
                vault_presence: std::collections::HashMap::new(),
                vault_chords: Vec::new(),
                editing_vault_hotkey: None,
                start_picker: false,
                vault_chip_bounds: crate::widgets::new_bounds_cell(),
                vault_tab_bounds: crate::widgets::new_bounds_cell(),
                vault_badge_bounds: crate::widgets::new_bounds_cell(),
                tab_order: Vec::new(),
                extra_windows: std::collections::BTreeMap::new(),
                window_ctx: None,
                pending_focus: None,
                windows_closing: Vec::new(),
                windows_revealing: Vec::new(),
                window_carry: None,
                tear_off_pending: None,
                strip_chip_bounds: std::cell::RefCell::new(std::collections::HashMap::new()),
                panel_window: None,
                input_window: None,
                float_window: None,
                keynav_scratch: crate::keynav::KeyNavState::default(),
                sftp_blank: crate::state::SftpState::default(),
                batch_dial_windows: std::collections::HashMap::new(),
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
                fullscreen_immersive: restored_immersive,
                // Restoring straight into immersive fullscreen re-shows
                // the "Press F11 to exit" hint (auto-hide task below), so
                // the user is never trapped in a chromeless window.
                fullscreen_hint_visible: restored_immersive,
                hotkey_bindings: crate::hotkeys::default_bindings(),
                editing_hotkey: None,
                modifiers: keyboard::Modifiers::default(),
                alt_sides: crate::key_encode::OptionSides::default(),
                #[cfg(target_os = "windows")]
                last_printscreen: None,
                login_script_generation: 0,
                trigger_confirm: None,
                host_editor_parent_combo_bounds: crate::widgets::new_bounds_cell(),
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
                cloud_dynamic_group_state: std::collections::HashMap::new(),
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
                monitor: Default::default(),
                tmux: Default::default(),
                ssh_transport_pool: Default::default(),
                pending_reuse_keys: Default::default(),
                monitor_dash: Default::default(),
                monitor_stamp: 0,
                monitor_error: None,
                monitor_ports_open: false,
                monitor_disks_open: true,
                sftp_chrome: crate::state::SftpChrome::default(),
                device_prefs: crate::state::DevicePrefs {
                    // Read before any vault is unlocked: the boot update
                    // check runs on the lock screen.
                    auto_check_updates,
                    update_channel,
                    offline_mode,
                    ..Default::default()
                },
                sftp_edit_upload_all: false,
                ui_color_popover: None,
                clear_history_confirm: false,
                viewing_session_log: None,
                session_player: None,
                gif_export: crate::state::GifExportState::default(),
                pending_snippet_vars: None,
                snippet_runs: Vec::new(),
                snippet_run_seq: 0,
                startup_runs_due: Vec::new(),
                active_forwards: std::collections::HashMap::new(),
                forward_conns: std::collections::HashMap::new(),
                pf_aborted_pending: std::collections::HashMap::new(),
                link_forwards: std::collections::HashMap::new(),
                link_confirm: None,
                save_output: None,
                remote_desktop_seq: 0,
                port_forward_starting: std::collections::HashSet::new(),
                port_forward_retry: std::collections::HashMap::new(),
                port_forward_agent_watch: None,
                local_terminal_theme: None,
                terminal_font_zoom: 0.0,
                settings_section: SettingsSection::Interface,
                open_panel_tabs: std::collections::BTreeSet::new(),
                net_tools: crate::state::NetToolsState::default(),
                settings_scroll: std::collections::HashMap::new(),
                settings_search: String::new(),
                settings_active_match: 0,
                renderer_active: None,
                // Replaced by the vault's own key during
                // `load_data_from_vault`; empty until then, which is also
                // what a locked vault shows, and an empty key is never
                // installed, so nothing is captured before it is known.
                sidebar_snippet_group: None,
                pending_perf_mode_toast: false,
                download_mirror,
                agent: crate::state::AgentState::default(),
                mcp_sudo: crate::state::McpSudoState::default(),
                tray_menu_signature: 0,
                jumplist_signature: 0,
                jumplist_window_tagged: false,
                is_window_hidden: false,
                ipc_state_signature: 0,
                pin_next_plugin_tab: None,
                pending_ecs_autoconnect: None,
                tab_drag: None,
                pane_drag_from: None,
                panel_resize_drag: None,
                // Workspace is the v0.7 default. Existing users who
                // never persisted `layout_mode` also fall through to
                // this default on next launch (no migration row
                // Vault nav orientation: horizontal pill strip by default.
                // Mirror `Connection::new` / `ConnectionForm::default`, which
                // expose new hosts via MCP by default.
                last_unlock: None,
                biometric_available,
                pending_update: None,
                update_downloading: false,
                update_progress: 0.0,
                update_error: None,
                update_ready: None,
                mirror_writer: None,
                session_log_mirror_failed: Vec::new(),
                update_check_status: None,
                reconnect_counters: std::collections::HashMap::new(),
                toast: None,
                os_notices: Vec::new(),
                loaded_cjk_fonts: std::collections::HashSet::new(),
                loaded_pack_fonts: std::collections::HashSet::new(),
                error_dialog: None,
                local_terminals: None,
                local_terminals_scanning: false,
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
            },
            Task::none(),
        );

        // If auto-unlocked (no user password), load data immediately;
        // unless the start asks which vault to open (Settings > Vaults,
        // several vaults): then nothing of this one loads, runs or lands
        // until it is the one picked (`pick_start_vault`).
        app.refresh_vault_presence();
        app.start_picker = app.start_picker_wanted();
        let open_now = !app.start_picker && app.vaults.cur().vault_ui.state == VaultState::Unlocked;
        if open_now {
            app.load_data_from_vault();
        }

        // If we were launched with `--connect <uuid>` AND the vault is
        // already open (no master password), kick off the connect right
        // after boot. When the vault is locked, we defer until VaultUnlock
        // succeeds (handled in that branch).
        let mut tasks = vec![task];
        // The app is a daemon: no window exists until one is opened.
        // First in the batch, so every boot task that talks to "our
        // window" finds it. The id is known synchronously; the task
        // only performs the open.
        if let Some(settings) = crate::app::WINDOW_SETTINGS.get() {
            let (id, open) = iced::window::open(settings.clone());
            crate::app::set_resident_window(id);
            tasks.insert(0, open.discard());
        }
        // The app's own fetches (release lookup, the CJK face the
        // language needs). NOT on a first run: the onboarding offers
        // offline mode a few slides in, and a request already in flight
        // by then would make the offer a lie for the first request.
        // The vault-creation arms fire the same helper once the answer
        // is known.
        if app.vaults.cur().vault_ui.state != VaultState::NeedSetup {
            tasks.extend(app.boot_fetch_tasks());
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
        // A device file that cannot be read (it does not parse, or its
        // medium went away: a portable `data` folder on a USB key) leaves
        // this start on a fresh device's defaults and vault list, every
        // write refused. Said at the start, with the file and the reason,
        // so the vaults missing from the list are not taken for lost and
        // the tree is never swapped for another in silence (issue #271).
        if let Some(problem) = oryxis_vault::device::installed().and_then(|d| d.read_problem()) {
            tracing::error!(error = %problem, "device store unreadable at start");
            if app.error_dialog.is_none() {
                app.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("device_store_unreadable_title").to_string(),
                    body: format!("{}\n\n{problem}", crate::i18n::t("device_store_unreadable_body")),
                    link: None,
                    action: None,
                });
            }
        }
        // What follows is the boot vault's work; its answers must reach
        // that vault whichever one the window is on when they arrive.
        // Boot's tasks do not go through `update`, which is what tags
        // the others, so they are tagged here (`on_vault`), apart from
        // the app's own (updates, fonts), which a closed boot vault (the
        // start picker's) must not take down with it.
        let boot_vault = app.vaults.current_id();
        let mut vault_tasks: Vec<Task<Message>> = Vec::new();
        // Whether something with a claim on where the app opens fired
        // below: the restored-strip landing (issue #229) yields to all
        // of them, the way it yields at the unlock site.
        let mut landed = false;
        if open_now
            && let Some(idx) = app.pending_auto_connect.and_then(|connect_id| {
                app.vaults.cur().connections.iter().position(|c| c.id == connect_id)
            })
        {
            app.pending_auto_connect = None;
            vault_tasks.push(Task::done(Message::Ssh(SshMessage::ConnectSsh(idx))));
            landed = true;
        }
        // Route a deep link the launch carried. `handle_deep_link`
        // re-stashes it by itself when the vault is still locked, so no
        // unlocked-state guard here (unlike `--connect`, whose index
        // lookup needs the loaded connections).
        if let Some(link) = app.pending_deep_link.take() {
            let route = app.handle_deep_link(link);
            vault_tasks.push(route);
            landed = true;
        }
        // Same for a CLI target, which `handle_connect_target` likewise
        // re-stashes while the vault is locked.
        if let Some(target) = app.pending_connect_target.take() {
            let route = app.handle_connect_target(&target);
            vault_tasks.push(route);
            landed = true;
        }
        // The restored strip's own two asks, for a vault that is open
        // at boot (a password vault answers them at `VaultUnlock`): the
        // landing on the chip that was active, and a first turn of the
        // funnel so the "connect at launch" queue starts now rather
        // than on whatever event the window raises first. The landing
        // is taken either way, so a launch that opened elsewhere does
        // not park it for a later unlock. A launch argument outranks it
        // whole: no select, and no dial beside the argument's own
        // foreground one (the tab keeps its place in the queue).
        if open_now {
            let landing = app.take_launch_landing_task(landed);
            vault_tasks.extend(landing.dial);
            vault_tasks.extend(landing.select);
            vault_tasks.extend(app.launch_dial_kick());
            // Finished downloads a previous process left in the ZMODEM
            // staging folder are OFFERED once the vault is readable (the
            // download folder is a setting); nothing is moved unasked.
            vault_tasks.extend(app.zmodem_sweep_task());
        }
        // Bring the sync engine up if the vault is already open and the
        // user left sync enabled. When the vault is locked we defer to
        // the `VaultUnlock` handler, same as `--connect`.
        // Only the P2P transport runs a background engine; every
        // snapshot transport reconciles on the iced cadence
        // subscription instead.
        if open_now
            && app.vaults.cur().sync.enabled
            && app.sync_uses_p2p()
        {
            vault_tasks.push(app.start_sync_engine());
        }
        // The git card's availability probe spawns a subprocess, so it
        // runs as a task rather than inside `view()`. Fire it whenever
        // the git transport can be on screen (also done on `VaultUnlock`
        // and on `TransportChanged`).
        if open_now && app.vaults.cur().sync.transport == "git" {
            vault_tasks.push(crate::dispatch_git_sync::git_availability_task());
        }

        // Auto-start port forward rules marked `auto_start`. Deferred to
        // `VaultUnlock` when the vault is locked, same as sync / --connect.
        if open_now {
            vault_tasks.extend(app.auto_start_port_forwards());
        }

        // Sweep any leftover `.old.exe` from a previous Windows MCP
        // update (no-op on Unix), before the plugin tasks below may lay
        // down a fresh launcher copy.
        crate::mcp_install::sweep_stale_launcher();
        // Credential Manager entries a crashed process staged for mstsc
        // and never removed (no-op on Unix, issue #251).
        crate::remote_desktop::sweep_stale_credentials();
        // Recording rows a previous process spooled under its soft lock
        // are sealed under a key that died with it (`session_spool`).
        crate::session_spool::SessionSpool::sweep_stale();
        // MCP migrate-install + plugin auto-update both need the vault
        // unlocked (they read `mcp_server_enabled` / the plugin rows
        // `load_data_from_vault` populates). When the vault is
        // password-protected it's still locked here, so these defer to
        // the `VaultUnlock` handler, which calls the same method once
        // the user's password opens it (the boot constructor can't
        // re-run). See `spawn_plugin_unlock_tasks`.
        if open_now {
            vault_tasks.extend(app.spawn_plugin_unlock_tasks());
        }
        // One-time performance-mode auto-enable notice, for the
        // auto-unlocked (no-password) vault. The password path shows it
        // from the `VaultUnlock` handler instead.
        vault_tasks.push(app.take_perf_mode_toast_task());

        // The terminal font pack rides `unlock_fetch_tasks`: the picked
        // family and weight are vault settings, so for a password vault
        // the heal used to run against the constructor's default font
        // and never against the configured one.
        if open_now {
            vault_tasks.extend(app.unlock_fetch_tasks());
        }
        tasks.push(Task::batch(vault_tasks).map(move |m| m.on_vault(boot_vault)));

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

        // winit asks for the restored maximize as a client message to the
        // root window right after it maps the window. A WM that has not
        // taken the window over yet ignores it (Openbox, issue #274), and
        // the window reopens at its floating size. Ask again once the WM
        // has placed it; where the first request already held, this is a
        // no-op. Fullscreen is not touched: it is a separate request.
        // The answer is read back and reconciled with the flag: when the
        // WM refuses the maximize, the restored flag would otherwise say
        // "maximized" over a floating window, and the edge handles (absent
        // while the flag is set) would never come back until a resize.
        if app.window_maximized && !app.window_fullscreen {
            let size = app.window_size;
            tasks.push(
                Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    },
                    |_| (),
                )
                .then(|_| crate::app::resident_window())
                .then(move |id_opt| match id_opt {
                    Some(id) => iced::window::maximize::<Message>(id, true).then(move |_| {
                        iced::window::is_maximized(id).map(move |maximized| {
                            Message::Tabs(TabsMessage::WindowStateSynced {
                                maximized,
                                fullscreen: None,
                                size,
                            })
                        })
                    }),
                    None => Task::none(),
                }),
            );
        }

        // The boot constructor set the F11 hint visible when restoring
        // into fullscreen; schedule the same 3 s auto-hide the F11
        // toggle handler uses.
        if app.fullscreen_immersive {
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
            && app.vaults.cur().connections.iter().any(|c| {
                c.protocol == oryxis_core::models::connection::ConnectionProtocol::Local
            })
        {
            // Marked in flight now, so the launch queue's first turn
            // waits on this scan instead of asking for a second one.
            app.local_terminals_scanning = true;
            tasks.push(Task::done(Message::Settings(
                crate::app::SettingsMessage::RescanLocalTerminals,
            )));
        }
        // Booting straight onto the lock screen: put the keyboard in
        // the master-password field (same auto-focus as LockVault /
        // SoftLockVault, so the password is typeable without a click).
        if app.vaults.cur().vault_ui.state == crate::state::VaultState::Locked {
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

impl Oryxis {
    /// The fetches the app makes on its own before the vault is
    /// unlocked: the release lookup and the CJK face the active
    /// language needs (the lock screen renders in it). Each is behind
    /// its own gate (`auto_check_updates`, the once-per-session font
    /// guard) and every one honors offline mode at the engine
    /// (`crate::offline`), with one deliberate asymmetry: the update
    /// check is not spawned at all while offline, since Settings > About
    /// already says why, while the font task IS, because its typed
    /// `Offline` answer is how the font that cannot heal reports itself.
    ///
    /// Boot calls this for an existing vault; a first run defers it to
    /// the vault-creation arms, after the onboarding had its chance to
    /// turn offline mode on; and the offline toggle going off calls it
    /// so nothing waits for the next launch.
    pub(crate) fn boot_fetch_tasks(&mut self) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        // The boot release lookup is for a person to act on, so an
        // emulated run skips it (`app::HARNESS_ACTIVE`) rather than
        // waiting on a network it does not control. The handler's own
        // `auto_check_updates` gate stays the user-facing switch; this
        // one is about who is watching.
        if !crate::app::harness_active() {
            tasks.push(Task::done(Message::Update(UpdateMessage::CheckForUpdate)));
        }
        // If the saved language uses a CJK script (Korean / Chinese /
        // Japanese), fetch + load its on-demand font now so the lock
        // screen and the rest of the UI render it instead of tofu. The
        // language was already the user's choice, so a cached or
        // bundled face loads silently; only a miss speaks, through the
        // `CjkFontReady` handler.
        let lang = crate::i18n::Language::active();
        if let Some(code) = crate::fonts::asset_code(lang)
            && !self.loaded_cjk_fonts.contains(code)
        {
            self.loaded_cjk_fonts.insert(code.to_string());
            tasks.push(crate::fonts::ensure_task(lang));
        }
        tasks
    }

    /// The fetches that need the vault's own settings: the terminal font
    /// pack (issue #109). Loads every already-cached pack face so a
    /// terminal picked to one of them renders right from its first
    /// frame, and fetches the face the picked family + weight needs when
    /// it isn't cached yet (settings that arrived via sync / import land
    /// on a machine without the file). Called wherever
    /// `load_data_from_vault` first runs for the session (boot for an
    /// open vault, the unlock, the two vault-creation arms) and when
    /// offline mode goes off; the guard keeps a second call cheap.
    pub(crate) fn unlock_fetch_tasks(&mut self) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        for (key, task) in crate::fonts::boot_pack_tasks(
            &self.vaults.cur().terminal_font_name,
            self.vaults.cur().terminal_font_weight,
        ) {
            if self.loaded_pack_fonts.insert(key.to_string()) {
                tasks.push(task);
            }
        }
        tasks
    }
}
