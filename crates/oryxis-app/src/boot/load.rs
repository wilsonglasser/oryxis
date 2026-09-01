use crate::app::Oryxis;
use iced::widget::text_editor;

use super::migrate::repair_group_parents;

impl Oryxis {
    pub(crate) fn load_data_from_vault(&mut self) {
        self.load_vault_entities();
        self.load_vault_plugins_and_logs();
        self.load_vault_locale_ai_sync();
        self.load_vault_terminal_settings();
        self.load_vault_hotkeys_and_defaults();

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

    /// Auto-archive sweep (uses the previously-loaded orphan setting)
    /// then the core entity lists: connections, groups (parent repair),
    /// session groups, keys, identities, proxy identities, cloud profiles.
    fn load_vault_entities(&mut self) {
        if let Some(vault) = &self.vault {
            // Auto-archive sweep: when the user has opted into the
            // cleanup, drop orphan-imported hosts whose `orphaned_at`
            // is older than the configured threshold. Runs before the
            // in-memory load so the deleted rows don't briefly appear
            // and then vanish.
            if self.prefs.cloud_auto_archive_orphans {
                let days = self
                    .prefs.cloud_orphan_archive_days
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
            // Repair invalid parent links before anything renders. Only a
            // manual folder is a container: a dynamic (cloud_query) group
            // resolves its own children and is never opened as a folder,
            // and a deleted folder leaves its children dangling. In both
            // cases the child renders nowhere (not at root since it has a
            // parent, not inside any openable folder) yet still counts as
            // "imported", so the user sees it vanish but can't re-import.
            // Re-home each such group on its nearest manual-folder ancestor
            // (or root) and persist the fix. Idempotent: a no-op once the
            // hierarchy is clean.
            repair_group_parents(&mut self.groups, vault);
            self.session_groups = vault.list_session_groups().unwrap_or_default();
            self.keys = vault.list_keys().unwrap_or_default();
            self.identities = vault.list_identities().unwrap_or_default();
            self.identities_with_password = vault
                .list_identity_ids_with_password()
                .unwrap_or_default();
            self.proxy_identities = vault.list_proxy_identities().unwrap_or_default();
            self.login_scripts = vault.list_login_scripts().unwrap_or_default();
            self.cloud_profiles = vault.list_cloud_profiles().unwrap_or_default();
        }
    }

    /// Plugin rows, content lists (snippets, custom themes, port-forward
    /// rules, known hosts) and the retention-pruned event / session logs.
    fn load_vault_plugins_and_logs(&mut self) {
        if let Some(vault) = &self.vault {

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
            // Install-script presets (issue #147), seeded ONCE per vault:
            // editable copies, so a user's edit or delete must never be
            // overwritten by a later boot. Fixed ids + fixed old
            // timestamps make two devices' seeds converge under sync
            // instead of duplicating (see `install_presets`).
            if !matches!(
                vault.get_setting("install_presets_seeded"),
                Ok(Some(ref v)) if v == "true"
            ) {
                for preset in
                    crate::install_presets::presets(&crate::shell_integration::template())
                {
                    if !self.snippets.iter().any(|s| s.id == preset.id) {
                        let _ = vault.save_snippet(&preset);
                        self.snippets.push(preset);
                    }
                }
                self.snippets.sort_by(|a, b| a.label.cmp(&b.label));
                let _ = vault.set_setting("install_presets_seeded", "true");
            }
            // Which install scripts already ran where, for the
            // "installed here" hint on the snippet surfaces.
            self.install_runs = vault
                .list_install_runs()
                .unwrap_or_default()
                .into_iter()
                .map(|(host, snippet, at)| ((host, snippet), at))
                .collect();
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
            // Saved AI conversations share the History timeline. The list is
            // metadata only (turn bodies load when one is opened), so it is
            // cheap to hold whole rather than paginate.
            self.chat_ui.conversations = vault.list_chat_conversations().unwrap_or_default();
        }
    }

    /// Language / layout / theme, the derived terminal palette, plus the
    /// AI, MCP, sync and local-terminal settings.
    fn load_vault_locale_ai_sync(&mut self) {
        if let Some(vault) = &self.vault {

            // Language: "auto" (the default, also what a missing row
            // means) follows the OS locale; a concrete code is an
            // explicit user choice. The choice string feeds the
            // Settings picker so "Auto (OS)" shows as selected instead
            // of the language it resolved to.
            {
                use crate::i18n::Language;
                let choice = vault
                    .get_setting("language")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "auto".to_string());
                let lang = if choice == "auto" {
                    crate::i18n::detect_os_language()
                } else {
                    Language::from_code(&choice)
                };
                Language::set_active(lang);
                self.prefs.language_choice = choice;
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
                self.ai.enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("ai_reasoning") {
                self.ai.reasoning = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("ai_save_history") {
                self.ai.save_history = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("ai_provider") {
                self.ai.provider = v;
            }
            if let Ok(Some(v)) = vault.get_setting("ai_model") {
                self.ai.model = v;
            }
            if let Ok(Some(v)) = vault.get_setting("ai_api_url") {
                self.ai.api_url = v;
            }
            if let Ok(Some(v)) = vault.get_setting("ai_default_mode") {
                let mode = crate::state::ChatMode::from_setting(&v);
                // Seed the process-wide default for tabs created later, and
                // apply to any tab that already exists at boot.
                crate::state::set_default_chat_mode(mode);
                for tab in &mut self.tabs {
                    tab.chat_mode = mode;
                }
            }
            self.ai.api_key_set = vault.get_ai_api_key().ok().flatten().is_some();
            if let Ok(Some(v)) = vault.get_setting("mcp_server_enabled") {
                self.mcp.server_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("remote_desktop_enabled") {
                self.remote_desktop_enabled = v == "true";
            }
            // Token MCP clients must present; empty means auth is off
            // (server allows any caller as long as the global toggle is on).
            if let Ok(Some(v)) = vault.get_setting("mcp_server_token") {
                self.mcp.server_token = v;
            }
            // Consent flag only; the password itself is read from
            // `master_password` at snippet/install time.
            if let Ok(Some(v)) = vault.get_setting("mcp_config_vault_pw") {
                self.mcp.include_vault_password = v == "true";
            }

            // Sync settings
            if let Ok(Some(v)) = vault.get_setting("sync_enabled") {
                self.sync.enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sync_mode") {
                self.sync.mode = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_passwords") {
                self.sync.passwords = v == "true";
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
            // Local terminals: curated, machine-local list persisted as JSON.
            // Presence of the key means the one-time scan already ran; a
            // corrupt / unparseable value falls back to a fresh scan (`None`).
            if let Ok(Some(v)) = vault.get_setting("local_terminals") {
                self.local_terminals = serde_json::from_str(&v).ok();
                // Legacy payloads (pre-id) deserialize with nil ids. Stamp
                // fresh ids so edit / remove / default have stable handles,
                // and re-persist once so the ids stick across restarts.
                if let Some(list) = self.local_terminals.as_mut()
                    && list.iter().any(|e| e.id.is_nil())
                {
                    for e in list.iter_mut().filter(|e| e.id.is_nil()) {
                        e.id = uuid::Uuid::new_v4();
                    }
                    self.persist_local_terminals();
                }
            }
            if let Ok(Some(v)) = vault.get_setting("local_terminal_default") {
                // Stored as the entry id; a legacy non-uuid value (old
                // program/args key) simply resolves to "always ask".
                self.local_terminal_default = uuid::Uuid::parse_str(&v).ok();
            }
            if let Ok(Some(v)) = vault.get_setting("sync_device_name") {
                self.sync.device_name = v;
            }
            // One-time grandfather of the hosted relay (v0.9 -> v0.10):
            // release builds used to bake a hosted signaling URL in as an
            // implicit default, so a syncing device could be using it with
            // an ABSENT sync_signaling_url setting. Write the baked URL
            // into the settings once, as if the user had configured it by
            // hand, then never touch it again: existing sync setups keep
            // working, while fresh installs (and vaults that never enabled
            // sync) start LAN-only with no hosted endpoint anywhere.
            // Present-but-empty means the user explicitly cleared the
            // field; that choice is respected and not migrated over.
            if vault.get_setting("sync_hosted_migrated").ok().flatten().is_none() {
                let was_syncing = matches!(
                    vault.get_setting("sync_enabled"),
                    Ok(Some(v)) if v == "true"
                );
                let url_absent =
                    matches!(vault.get_setting("sync_signaling_url"), Ok(None));
                let mut mark_done = true;
                if was_syncing && url_absent {
                    let (url, token) =
                        oryxis_sync::config::legacy_hosted_signaling();
                    if let Some(url) = url {
                        let _ = vault.set_setting("sync_signaling_url", &url);
                        if let Some(token) = token
                            && matches!(
                                vault.get_setting("sync_signaling_token"),
                                Ok(None)
                            )
                        {
                            let _ =
                                vault.set_setting("sync_signaling_token", &token);
                        }
                    } else {
                        // A user WAS syncing on the hosted default, but this
                        // binary has no baked-in hosted URL to migrate (a
                        // self-built / fork build without the CI secret).
                        // Leave the flag unset so a later official build can
                        // still complete the migration instead of silently
                        // dropping the user's internet sync.
                        mark_done = false;
                    }
                }
                if mark_done {
                    let _ = vault.set_setting("sync_hosted_migrated", "true");
                }
            }
            if let Ok(Some(v)) = vault.get_setting("sync_signaling_url") {
                self.sync.signaling_url = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_signaling_token") {
                self.sync.signaling_token = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_relay_url") {
                self.sync.relay_url = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_listen_port") {
                self.sync.listen_port = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_transport") {
                self.sync.transport = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_sftp_host_id") {
                self.sync.sftp.host_id = uuid::Uuid::parse_str(&v).ok();
            }
            if let Ok(Some(v)) = vault.get_setting("sync_sftp_remote_path") {
                self.sync.sftp.remote_path = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_webdav_url") {
                self.sync.webdav.url = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_webdav_user") {
                self.sync.webdav.user = v;
            }
            if let Ok(Some(v)) = vault.get_sync_webdav_password() {
                self.sync.webdav.password = v;
            }
            // The shared group secret: only the KNOWLEDGE flag reaches
            // state. The field starts empty and the stored value never
            // pre-fills it: a masked pre-filled passphrase turns every
            // later keystroke into an append that silently swaps the
            // group key under the existing snapshot ("Decryption failed
            // (wrong key?)" on the next round). One secret across every
            // snapshot transport, so the SFTP / folder / Git / WebDAV
            // cards share the same placeholder.
            match vault.get_sync_sftp_passphrase() {
                Ok(Some(_)) => self.sync.passphrase_known = true,
                Ok(None) => self.sync.passphrase_known = false,
                // Locked vault: leave the previous session's flag alone.
                Err(_) => {}
            }
            self.sync.peers = vault.list_sync_peers().unwrap_or_default();
            if let Ok(Some(v)) = vault.get_setting("ai_system_prompt") {
                self.ai.system_prompt = text_editor::Content::with_text(&v);
            }
        }
    }

    /// Terminal / SFTP / tab-appearance settings and the vault nav shape.
    fn load_vault_terminal_settings(&mut self) {
        if let Some(vault) = &self.vault {

            // Terminal / SFTP / connection settings, load whatever
            // the user previously typed, fall back to defaults silently
            // when the key is missing (first-run or new key in update).
            // Mirrors the read in `main` (which sets WGPU_BACKEND /
            // ICED_BACKEND before the runtime starts); keep this in sync
            // so the picker shows the persisted choice, not the default.
            if let Ok(Some(v)) = vault.get_setting("renderer_backend") {
                self.prefs.renderer_backend = v;
            }
            // Same row `main` read before the window existed: it decided
            // there whether the surface is transparent at all, this only
            // feeds the Settings picker.
            if let Ok(Some(v)) = vault.get_setting("terminal_opacity")
                && let Ok(percent) = v.parse::<u8>()
            {
                self.prefs.terminal_opacity =
                    percent.clamp(crate::theme::MIN_TERMINAL_OPACITY, 100);
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_bg_image") {
                self.prefs.terminal_bg_image = v;
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_bg_fit") {
                // Normalized through the enum so a hand-edited row can
                // only ever produce a fit the renderer knows.
                self.prefs.terminal_bg_fit =
                    oryxis_terminal::BgFit::from_str_or_default(&v).as_str().to_string();
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_bg_dim")
                && let Ok(percent) = v.parse::<u8>()
            {
                self.prefs.terminal_bg_dim = percent.min(100);
            }
            if let Ok(Some(v)) = vault.get_setting("copy_on_select") {
                self.prefs.copy_on_select = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("careful_paste") {
                self.prefs.careful_paste = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("paste_guard") {
                self.prefs.paste_guard = v == "true";
            }
            // Auto-title (OSC 0/2) lives in a process-wide gate (read at tab
            // render time); default-on, only override when explicitly stored.
            if let Ok(Some(v)) = vault.get_setting("terminal_auto_title") {
                crate::state::set_auto_title(v == "true");
            }
            if let Ok(Some(v)) = vault.get_setting("right_click_copy") {
                self.prefs.right_click_copy = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_right_click") {
                self.prefs.terminal_right_click =
                    crate::util::RightClickMode::from_code(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("scrollback_reset_keypress") {
                self.prefs.scrollback_reset_keypress = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("scrollback_reset_output") {
                self.prefs.scrollback_reset_output = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_password_autofill") {
                self.prefs.terminal_password_autofill = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("bold_is_bright") {
                self.prefs.bold_is_bright = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("pane_border_inactive") {
                self.prefs.pane_border_inactive = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("pane_gap") {
                self.prefs.pane_gap = v;
            }
            // Files-sidebar folder history, keyed by host. A malformed or
            // stale blob is dropped rather than failing the boot: it is a
            // convenience list, and losing it costs nothing.
            if let Ok(Some(v)) = vault.get_files_recent_folders()
                && let Ok(map) = serde_json::from_str::<
                    std::collections::HashMap<uuid::Uuid, Vec<String>>,
                >(&v)
            {
                self.files_recent_folders = map;
            }
            if let Ok(Some(v)) = vault.get_setting("keyword_highlight") {
                self.prefs.keyword_highlight = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting(crate::highlight_rules::SETTING_KEY) {
                self.prefs.highlight_rules = crate::highlight_rules::parse_setting(&v);
                // Compiled here rather than lazily on the first frame:
                // the first pane must be able to paint (and watch) from
                // the moment it opens.
                self.prefs.compiled_highlight_rules =
                    crate::highlight_rules::compile(&self.prefs.highlight_rules).0;
            }
            if let Ok(Some(v)) = vault.get_setting("command_history") {
                self.prefs.command_history = v == "true";
            }
            // The shell-integration key, minted on first boot and stable
            // afterwards (the user has it pasted into dotfiles on real
            // hosts, so it must not move under them). Installing it here,
            // once, is what arms the in-band capture: without it every
            // `OSC 633 ; E` is refused, which is the safe direction, since
            // the sequence carries text straight into a history row that
            // runs on one click.
            let nonce = match vault.get_setting(crate::shell_integration::SETTING) {
                Ok(Some(v)) if !v.trim().is_empty() => v,
                _ => {
                    let fresh = crate::shell_integration::generate_nonce();
                    // A vault that refuses the write leaves the key in
                    // memory for this run rather than disabling capture:
                    // the snippet the user copies now simply stops working
                    // after a restart, which they will notice, instead of
                    // capture silently doing nothing while the setting
                    // looks fine.
                    let _ = vault.set_setting(crate::shell_integration::SETTING, &fresh);
                    fresh
                }
            };
            oryxis_terminal::osc::set_global_command_nonce(Some(nonce.clone()));
            self.shell_integration_nonce = nonce;
            if let Ok(Some(v)) = vault.get_setting("command_history_file") {
                self.prefs.command_history_file = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("snippet_tag_filter") {
                self.prefs.snippet_tag_filter = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("command_history_file_dir") {
                self.prefs.command_history_file_dir =
                    (!v.trim().is_empty()).then(|| v.trim().to_string());
            }
            if let Ok(Some(v)) = vault.get_setting("zmodem_download_dir") {
                self.prefs.zmodem_download_dir = v.trim().to_string();
            }
            // Performance mode. An explicit stored choice always wins. When
            // the key is absent (first boot on this machine) and the render
            // probe redirected this GPU stack to software, auto-enable it
            // once, persist the decision so it is a stable default the user
            // can later turn off, and arm the explaining toast. Guarded so
            // it fires exactly once and never overrides a user who set it.
            match vault.get_setting("performance_mode") {
                Ok(Some(v)) => self.prefs.performance_mode = v == "true",
                _ => {
                    if crate::renderer_probe::probe_redirected() {
                        self.prefs.performance_mode = true;
                        self.pending_perf_mode_toast = true;
                        let _ = vault.set_setting("performance_mode", "true");
                    }
                }
            }
            if let Ok(Some(v)) = vault.get_setting("perf_overlay") {
                self.prefs.perf_overlay = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("network_tools_enabled") {
                self.prefs.network_tools = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("smart_contrast") {
                self.prefs.smart_contrast = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_bell_mode") {
                self.prefs.bell_mode = crate::util::BellMode::from_code(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_clipboard_access") {
                self.prefs.clipboard_access = crate::util::ClipboardAccess::from_code(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_notification") {
                self.prefs.notification_mode = crate::util::NotificationMode::from_code(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("smart_tabs") {
                self.prefs.smart_tabs = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("smart_tabs_long_seconds")
                && let Ok(n) = v.parse()
            {
                self.prefs.smart_long_secs = n;
            }
            // Apply the OSC 52 gate to the terminal backend (process-wide).
            let (cw, cr) = self.prefs.clipboard_access.flags();
            oryxis_terminal::set_clipboard_access(cw, cr);
            if let Ok(Some(v)) = vault.get_setting("show_status_bar") {
                self.prefs.show_status_bar = v == "true";
            }
            for (key, field) in [
                ("status_show_version", 0usize),
                ("status_show_connection", 1),
                ("status_show_latency", 2),
                ("status_show_dimensions", 3),
                ("status_show_cwd", 4),
            ] {
                if let Ok(Some(v)) = vault.get_setting(key) {
                    let on = v == "true";
                    match field {
                        0 => self.prefs.status_show_version = on,
                        1 => self.prefs.status_show_connection = on,
                        2 => self.prefs.status_show_latency = on,
                        3 => self.prefs.status_show_dimensions = on,
                        _ => self.prefs.status_show_cwd = on,
                    }
                }
            }
            if let Ok(Some(v)) = vault.get_setting("status_bar_align_left") {
                self.prefs.status_bar_align_left = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sidebar_tab_sides") {
                self.prefs.sidebar_tab_sides = crate::state::AppPrefs::parse_sidebar_tab_sides(&v);
            } else if let Ok(Some(v)) = vault.get_setting("terminal_sidebar_side") {
                // Pre-#102 whole-sidebar dock (issue #85): "left" moved
                // every tab at once, so the migration moves every tab
                // (HostsTree included: it joins whichever region the
                // user's sidebar lived in). Read-only migration: the
                // new key is only written when the user next touches a
                // location picker.
                if v == "left" {
                    for tab in crate::state::TerminalSidebarTab::ALL {
                        self.prefs
                            .sidebar_tab_sides
                            .insert(tab, crate::state::SidebarPlacement::Left);
                    }
                }
            }
            if let Ok(Some(v)) = vault.get_setting("sidebar_auto_open") {
                self.prefs.sidebar_auto_open = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sidebar_default_tab") {
                // Empty / "last" / any unknown code = keep the last opened
                // tab (the default); a known code pins that tab.
                self.prefs.sidebar_default_tab =
                    crate::state::TerminalSidebarTab::from_code(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("monitor_status_bar") {
                self.prefs.monitor_status_bar = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("monitor_interval_seconds") {
                self.prefs.monitor_interval = v;
            }
            if let Ok(Some(v)) = vault.get_setting("monitor_dash_list_view") {
                self.prefs.monitor_dash_list_view = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("host_view_mode") {
                if let Some(mode) = crate::state::HostViewMode::from_code(&v) {
                    self.prefs.host_view_mode = mode;
                }
            } else if let Ok(Some(v)) = vault.get_setting("host_list_view") {
                // Pre-tree bool (grid/list). Read-only migration: the
                // new key is only written when the user next cycles
                // the view.
                if v == "true" {
                    self.prefs.host_view_mode = crate::state::HostViewMode::List;
                }
            }
            if let Ok(Some(v)) = vault.get_setting("card_accent_glass") {
                self.prefs.card_accent_glass = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("show_host_address") {
                self.prefs.show_host_address = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("show_tab_host_address") {
                self.prefs.show_tab_host_address = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("privacy_mode") {
                self.privacy.mode = v == "true";
            }
            // Privacy mask lists (issue #78). The never list is seeded
            // with the generic-username defaults at struct init; a
            // stored value (even an empty string, the user clearing
            // the field is a choice) replaces it wholesale.
            if let Ok(Some(v)) = vault.get_setting("privacy_always_mask") {
                self.privacy.always_mask_editor =
                    iced::widget::text_editor::Content::with_text(&v);
                self.privacy.always_mask = v;
            }
            if let Ok(Some(v)) = vault.get_setting("privacy_never_mask") {
                self.privacy.never_mask_editor =
                    iced::widget::text_editor::Content::with_text(&v);
                self.privacy.never_mask = v;
            }
            if let Ok(Some(v)) = vault.get_setting("hint_privacy_mask") {
                self.privacy.hint_shown = v == "true";
            }
            // Per-class mask gates (issue #78 block 1): absent = on.
            if let Ok(Some(v)) = vault.get_setting("privacy_mask_public_ips") {
                self.privacy.mask_public_ips = v != "false";
            }
            if let Ok(Some(v)) = vault.get_setting("privacy_mask_private_ips") {
                self.privacy.mask_private_ips = v != "false";
            }
            if let Ok(Some(v)) = vault.get_setting("privacy_mask_usernames") {
                self.privacy.mask_usernames = v != "false";
            }
            if let Ok(Some(v)) = vault.get_setting("privacy_mask_hostnames") {
                self.privacy.mask_hostnames = v != "false";
            }
            // One-shot reset: Privacy Mode was never meant to be on by
            // default, yet some vaults carry a persisted
            // `privacy_mode = true`. Force it off once on upgrade; the
            // sentinel keeps a user who deliberately re-enables it from
            // being reset again on the next boot.
            if let Ok(None) = vault.get_setting("privacy_default_off_applied") {
                if self.privacy.mode {
                    self.privacy.mode = false;
                    let _ = vault.set_setting("privacy_mode", "false");
                }
                let _ = vault.set_setting("privacy_default_off_applied", "true");
            }
            if let Ok(Some(v)) = vault.get_setting("debug_logging")
                && v == "true"
            {
                // Normally already armed by main.rs before the tracing
                // subscriber was built; retrying here covers an earlier
                // failure so the toggle reflects the sink's real state.
                self.prefs.debug_logging = crate::logging::is_enabled()
                    || match crate::logging::enable() {
                        Ok(_) => true,
                        Err(e) => {
                            tracing::warn!("failed to enable debug logging: {e}");
                            false
                        }
                    };
            }
            if let Ok(Some(v)) = vault.get_setting("download_mirror") {
                let choice = crate::net_mirror::MirrorChoice::from_setting(&v);
                if let crate::net_mirror::MirrorChoice::Custom(url) = &choice {
                    self.download_mirror.url_input = url.clone();
                }
                self.download_mirror.choice = choice.clone();
                crate::net_mirror::set_choice(choice);
            }
            self.agent.confirm = vault
                .get_setting("agent_server_confirm")
                .ok()
                .flatten()
                .map(|v| v != "false")
                .unwrap_or(true);
            self.agent.allow_add = matches!(
                vault.get_setting("agent_server_allow_add"),
                Ok(Some(ref v)) if v == "true"
            );
            self.agent.openssh_pipe = matches!(
                vault.get_setting("agent_server_openssh_pipe"),
                Ok(Some(ref v)) if v == "true"
            );
            // The agent runtime is started after boot (needs the live
            // vault handle + master password); remember the desired
            // state so the post-unlock hook can start it.
            self.agent.enabled = matches!(
                vault.get_setting("agent_server_enabled"),
                Ok(Some(ref v)) if v == "true"
            );
            if let Ok(Some(v)) = vault.get_setting("close_to_tray") {
                self.prefs.close_to_tray = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("minimize_to_tray") {
                self.prefs.minimize_to_tray = v == "true";
            }
            // Mirror it down to the Win32 subclass proc that handles the
            // OS minimize verbs; it can't reach app state. Unconditional
            // (not inside the `if let`) so a vault without the row still
            // pushes the default.
            crate::tray::set_minimize_to_tray(self.prefs.minimize_to_tray);
            if let Ok(Some(v)) = vault.get_setting("tab_close_button_side")
                && (v == "left" || v == "right")
            {
                self.prefs.tab_close_button_side = v;
            }
            if let Ok(Some(v)) = vault.get_setting("pinned_tab_style")
                && (v == "compact" || v == "full")
            {
                self.prefs.pinned_tab_style = v;
            }
            // Ctrl+digit slots: the Home area tab used to own the first
            // one, so the Nth tab answered to Ctrl+N+1. New vaults are
            // stamped as migrated when they are created (`boot`), so an
            // unstamped vault is one that existed before the change and
            // keeps the old mapping until the user says otherwise
            // (Settings > Shortcuts, or the one-click align offered next
            // to the tab-number setting).
            if vault
                .get_setting("tab_slots_home_migrated")
                .ok()
                .flatten()
                .is_none()
            {
                self.prefs.tab_slot_includes_home = true;
                let _ = vault.set_setting("tab_slot_includes_home", "true");
                let _ = vault.set_setting("tab_slots_home_migrated", "true");
            } else if let Ok(Some(v)) = vault.get_setting("tab_slot_includes_home") {
                self.prefs.tab_slot_includes_home = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("duplicate_tab_position")
                && (v == "next" || v == "end" || v == "start")
            {
                self.prefs.duplicate_tab_position = v;
            }
            if let Ok(Some(v)) = vault.get_setting("tab_number_style")
                && (v == "off" || v == "prefix" || v == "icon")
            {
                self.prefs.tab_number_style = v;
            }
            if let Ok(Some(v)) = vault.get_setting("show_tab_status_dot") {
                self.prefs.show_tab_status_dot = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_accent_line") {
                self.prefs.tab_accent_line = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_accent_wash") {
                self.prefs.tab_accent_wash = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_accent_text") {
                self.prefs.tab_accent_text = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tab_accent_color")
                && (v == "host" || v == "app")
            {
                self.prefs.tab_accent_color = v;
            }
            if let Ok(Some(v)) = vault.get_setting("tab_fill_style")
                && (v == "gradient" || v == "solid")
            {
                self.prefs.tab_fill_style = v;
            }
            if let Ok(Some(v)) = vault.get_setting("tab_bar_position")
                && (v == "top" || v == "bottom" || v == "left" || v == "right")
            {
                crate::views::tab_bar::set_tab_bar_pos(
                    crate::views::tab_bar::TabBarPos::from_setting(&v),
                );
                self.prefs.tab_bar_position = v;
            }
            if let Ok(Some(v)) = vault.get_setting("inactive_tab_style")
                && (v == "none" || v == "border" || v == "underline")
            {
                crate::views::tab_bar::set_inactive_tab_style(
                    crate::views::tab_bar::InactiveTabStyle::from_setting(&v),
                );
                self.prefs.inactive_tab_style = v;
            }
            if let Ok(Some(v)) = vault.get_setting("tab_width_mode")
                && (v == "adaptive" || v == "uniform")
            {
                self.prefs.tab_width_mode = v;
            }
            if let Ok(Some(v)) = vault.get_setting("tab_uniform_size") {
                self.prefs.tab_uniform_size = v;
            }
            if let Ok(Some(v)) = vault.get_setting("pinned_tabs_top_bar") {
                self.prefs.pinned_tabs_top_bar = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("side_hide_top_bar") {
                self.prefs.side_hide_top_bar = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("side_full_height") {
                self.prefs.side_full_height = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_enabled") {
                self.sftp_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("host_monitoring_enabled") {
                self.prefs.host_monitoring = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("host_monitoring_seeded") {
                self.prefs.host_monitoring_seeded = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("monitor_all_hosts") {
                self.prefs.monitor_all_hosts = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("tmux_manager_enabled") {
                self.prefs.tmux_manager = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("ssh_connection_reuse") {
                self.prefs.ssh_connection_reuse = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sync_folder_path") {
                self.sync.folder.path = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sync_git_remote") {
                self.sync.git.remote = v;
            }
            if let Ok(Some(v)) = vault.get_setting("side_panel_width")
                && let Ok(w) = v.parse::<f32>()
            {
                self.panel_width =
                    w.clamp(crate::app::PANEL_WIDTH_MIN, crate::app::PANEL_WIDTH_MAX);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_split_ratio")
                && let Ok(r) = v.parse::<f32>()
            {
                self.sftp_chrome.split_ratio = r.clamp(0.15, 0.85);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_log_height")
                && let Ok(h) = v.parse::<f32>()
            {
                self.sftp.log_height =
                    h.clamp(crate::state::SFTP_LOG_MIN_H, crate::state::SFTP_LOG_MAX_H);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_columns") {
                self.sftp_chrome.columns_template.apply_visibility_storage(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_col_order") {
                self.sftp_chrome.columns_template.apply_order_storage(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_col_widths") {
                self.sftp_chrome.columns_template.apply_width_storage(&v);
            }
            // Seed the initial panes from the loaded template; later tabs are
            // seeded at creation (see `seed_sftp_columns`).
            self.sftp.left.columns = self.sftp_chrome.columns_template.clone();
            self.sftp.right.columns = self.sftp_chrome.columns_template.clone();
            // Vault nav orientation. Prefer the new `nav_orientation`
            // setting; if it's absent, migrate from the legacy
            // `layout_mode` (classic → vertical rail, workspace →
            // horizontal pills) so existing users keep a familiar shape.
            if let Ok(Some(v)) = vault.get_setting("nav_orientation")
                && (v == "horizontal" || v == "vertical")
            {
                self.prefs.nav_orientation = v;
            } else if let Ok(Some(v)) = vault.get_setting("layout_mode") {
                self.prefs.nav_orientation = if v == "classic" {
                    "vertical".into()
                } else {
                    "horizontal".into()
                };
            }
            if let Ok(Some(v)) = vault.get_setting("nav_rail_expanded") {
                self.prefs.nav_rail_expanded = v == "true";
            }
        }
    }

    /// Hotkey overrides (with factory-conflict resolution) and the
    /// new-connection defaults, including the one-shot setting migrations.
    fn load_vault_hotkeys_and_defaults(&mut self) {
        if let Some(vault) = &self.vault {
            // Hotkey overrides: each action persists under
            // `hotkey_<id>` with the canonical serialized form, chords
            // space-separated (`"ctrl+shift+v shift+ins"`). Defaults
            // already populate `hotkey_bindings`, so a missing or
            // malformed entry silently falls back to the factory
            // chords. An empty row is NOT malformed: it is a
            // deliberate unbind, and `HotkeyBindings::parse` returns an
            // empty list for it. The single-binding model could not
            // draw that distinction, so an unbound action silently
            // regained its factory chord on the next boot.
            let mut user_bound: Vec<crate::hotkeys::HotkeyAction> = Vec::new();
            for action in crate::hotkeys::HotkeyAction::all() {
                let key = format!("hotkey_{}", action.id());
                if let Ok(Some(v)) = vault.get_setting(&key)
                    && let Some(binds) = crate::hotkeys::HotkeyBindings::parse(&v)
                {
                    self.hotkey_bindings.insert(*action, binds);
                    user_bound.push(*action);
                }
            }
            // Upgrade-path conflict resolution: a release that ships a
            // NEW factory default can collide with a chord the user
            // already bound to another action (their override persists,
            // the new action has no stored row). The dispatch loop is
            // first-match-wins by enum order, which would silently
            // shadow one of the two, so the explicit user choice wins:
            // the still-factory action gives up the COLLIDING CHORD,
            // and only unbinds when that was its last one (rebindable
            // in Settings > Shortcuts as usual).
            //
            // Per chord, not per action: a factory action shipping two
            // chords keeps the one the user didn't take. That matters
            // for the clipboard pair, whose alternates (Ctrl+Insert /
            // Shift+Insert) are exactly the chords an early adopter is
            // most likely to have already bound by hand.
            let mut user_chords: Vec<crate::hotkeys::HotkeyBinding> = user_bound
                .iter()
                .filter_map(|a| self.hotkey_bindings.get(a))
                .flat_map(|binds| binds.iter().copied())
                .collect();
            // Per-snippet custom hotkeys are user choices too, recorded
            // against a table that did not yet contain the new factory
            // default (the snippet recorder refuses chords the table
            // holds, so an existing snippet chord PROVES it predates the
            // default). Binding-table dispatch runs before snippet
            // dispatch, so leaving the collision in place would silently
            // hijack the snippet's chord on upgrade; stripping it here
            // lets the table miss and the snippet keep firing. Snippets
            // load earlier in the boot sequence (`load_data_from_vault`
            // order), so the list is already populated.
            user_chords.extend(
                self.snippets
                    .iter()
                    .filter_map(|sn| sn.hotkey.as_deref())
                    .filter_map(crate::hotkeys::HotkeyBinding::parse),
            );
            let mut emptied: Vec<crate::hotkeys::HotkeyAction> = Vec::new();
            for action in crate::hotkeys::HotkeyAction::all() {
                if user_bound.contains(action) {
                    continue;
                }
                let Some(binds) = self.hotkey_bindings.get_mut(action) else {
                    continue;
                };
                for chord in &user_chords {
                    binds.remove(chord);
                }
                if binds.is_empty() {
                    emptied.push(*action);
                }
            }
            for action in emptied {
                self.hotkey_bindings.remove(&action);
            }
            // One-shot migration: middle-click paste used to be its own
            // `middle_click_paste` setting, and is now an ordinary chord
            // on `TerminalPasteSelection` (the binding table is the one
            // authority for the gesture).
            //
            // Applied to whatever list resolved ABOVE, not only to
            // vaults with no stored override: an override replaces the
            // factory list wholesale, so a user who had rebound
            // paste-selection would otherwise lose middle-click paste
            // without ever asking to. Likewise a deliberate unbind of
            // the keyboard chord never meant "and drop the mouse
            // gesture too", because the two were unrelated settings.
            if vault
                .get_setting("middle_click_paste_migrated")
                .ok()
                .flatten()
                .is_none()
            {
                let want = !matches!(
                    vault.get_setting("middle_click_paste"),
                    Ok(Some(v)) if v == "false"
                );
                let action = crate::hotkeys::HotkeyAction::TerminalPasteSelection;
                let chord = crate::hotkeys::middle_click_chord();
                let mut binds =
                    self.hotkey_bindings.get(&action).cloned().unwrap_or_default();
                let changed = if want {
                    let before = binds.len();
                    binds.push(chord);
                    binds.len() != before
                } else {
                    binds.remove(&chord)
                };
                if changed {
                    let _ = vault.set_setting(
                        &format!("hotkey_{}", action.id()),
                        &binds.serialize(),
                    );
                    if binds.is_empty() {
                        self.hotkey_bindings.remove(&action);
                    } else {
                        self.hotkey_bindings.insert(action, binds);
                    }
                }
                let _ = vault.set_setting("middle_click_paste_migrated", "true");
            }
            if let Ok(Some(v)) = vault.get_setting("default_host_icon")
                && matches!(v.as_str(), "circular" | "square" | "rounded" | "outline" | "initials")
            {
                self.prefs.default_host_icon = v;
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
            if let Ok(Some(v)) = vault.get_setting("terminal_text_thickness") {
                self.terminal_text_thickness =
                    crate::fonts::TextThickness::from_setting(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_font_weight") {
                // Unknown values read as Regular, so a row written by a
                // newer build (or a hand edit) can only cost the weight,
                // never the boot.
                self.terminal_font_weight =
                    crate::fonts::TerminalFontWeight::from_setting(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("keepalive_interval") {
                self.prefs.keepalive_interval = v;
            }
            // New-connection defaults (pre-filled into a fresh host form).
            if let Ok(Some(v)) = vault.get_setting("default_agent_forwarding") {
                self.prefs.default_agent_forwarding = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("default_port") {
                self.prefs.default_port = v;
            }
            if let Ok(Some(v)) = vault.get_setting("default_keepalive") {
                self.prefs.default_keepalive = v;
            }
            if let Ok(Some(v)) = vault.get_setting("default_terminal_type") {
                self.prefs.default_terminal_type = v;
            }
            if let Ok(Some(v)) = vault.get_setting("default_username") {
                self.prefs.default_username = v;
            }
            if let Ok(Some(v)) = vault.get_setting("default_auth_method") {
                self.prefs.default_auth_method = crate::util::auth_method_from_setting(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("default_identity_id") {
                self.prefs.default_identity_id = uuid::Uuid::parse_str(&v).ok();
            }
            if let Ok(Some(v)) = vault.get_setting("default_key_id") {
                self.prefs.default_key_id = uuid::Uuid::parse_str(&v).ok();
            }
            if let Ok(Some(v)) = vault.get_setting("default_group_id") {
                self.prefs.default_group_id = uuid::Uuid::parse_str(&v).ok();
            }
            if let Ok(Some(v)) = vault.get_setting("default_proxy_identity_id") {
                self.prefs.default_proxy_identity_id = uuid::Uuid::parse_str(&v).ok();
            }
            if let Ok(Some(v)) = vault.get_setting("default_mcp_enabled") {
                self.prefs.default_mcp_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("default_encoding") {
                self.prefs.default_encoding = if v.is_empty() { None } else { Some(v) };
            }
            if let Ok(Some(v)) = vault.get_setting("default_env_vars") {
                self.prefs.default_env_vars = crate::util::env_vars_from_setting(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("defaults_collapsed") {
                self.prefs.defaults_collapsed = v == "true";
            }
            // One-shot migration: 30s is the new default in this version,
            // up from the previous "0" (off). Users sitting at the old
            // default get bumped to 30 so they pick up the better idle
            // behavior automatically. Explicit non-zero choices (e.g. a
            // user who configured 60) are preserved. The sentinel makes
            // this idempotent so a user who reverts to 0 after the
            // migration isn't bumped again on next boot.
            if let Ok(None) = vault.get_setting("keepalive_default_v2_applied") {
                if self.prefs.keepalive_interval == "0"
                    || self.prefs.keepalive_interval.is_empty()
                {
                    self.prefs.keepalive_interval = "30".into();
                    let _ = vault.set_setting("keepalive_interval", "30");
                }
                let _ = vault.set_setting("keepalive_default_v2_applied", "true");
            }
            if let Ok(Some(v)) = vault.get_setting("scrollback_rows") {
                self.prefs.scrollback_rows = v;
            }
            oryxis_terminal::set_default_scrollback(
                crate::dispatch_settings::resolve_scrollback_rows(&self.prefs.scrollback_rows),
            );
            if let Ok(Some(v)) = vault.get_setting("word_delimiters") {
                self.prefs.word_delimiters = v;
            }
            if let Ok(Some(v)) = vault.get_setting("terminal_hint_mode") {
                self.prefs.hint_mode = crate::util::HintMode::from_code(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_auto_refresh_enabled") {
                self.prefs.cloud_auto_refresh_enabled = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_auto_refresh_interval_minutes") {
                self.prefs.cloud_auto_refresh_interval_minutes = v;
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_auto_archive_orphans") {
                self.prefs.cloud_auto_archive_orphans = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("cloud_orphan_archive_days") {
                self.prefs.cloud_orphan_archive_days = v;
            }
            if let Ok(Some(v)) = vault.get_setting("auto_reconnect") {
                self.prefs.auto_reconnect = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("max_reconnect_attempts") {
                self.prefs.max_reconnect_attempts = v;
            }
            if let Ok(Some(v)) = vault.get_setting("auto_lock_minutes") {
                self.prefs.auto_lock_minutes = v;
            }
            // Unknown tokens fall back to "ask" so a corrupted row can't
            // silently make the Lock button skip its confirm.
            if let Ok(Some(v)) = vault.get_setting("manual_lock_action") {
                self.prefs.manual_lock_action = match v.as_str() {
                    "sleep" | "lock" => v,
                    _ => "ask".into(),
                };
            }
            if let Ok(Some(v)) = vault.get_setting("biometric_unlock_enabled") {
                self.prefs.biometric_unlock_enabled = v == "true";
            }
            // Availability is probed once at boot (pre-unlock, provider
            // level) and stays stable for the session, so no re-probe here.
            if let Ok(Some(v)) = vault.get_setting("os_detection") {
                self.prefs.os_detection = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("session_logging") {
                self.prefs.session_logging = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("session_log_full") {
                self.prefs.session_log_full = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("session_log_compress") {
                self.prefs.session_log_compress = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("connection_history") {
                self.prefs.connection_history = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("logs_retention") {
                self.prefs.logs_retention = v;
            }
            if let Ok(Some(v)) = vault.get_setting("session_log_max_bytes") {
                // Anything unparseable reads as "no cap", the same
                // permissive answer a missing row gives: a bad value
                // must not silently start deleting recordings.
                self.prefs.session_log_max_bytes = v.parse::<u64>().ok().filter(|n| *n > 0);
            }
            if let Ok(Some(v)) = vault.get_setting("auto_check_updates") {
                self.prefs.auto_check_updates = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("update_channel") {
                self.prefs.update_channel = crate::update::UpdateChannel::from_setting(&v);
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_concurrency") {
                self.prefs.sftp_concurrency = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_upload_temp_name") {
                self.prefs.sftp_upload_temp_name = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_ask_download_dir") {
                self.prefs.sftp_ask_download_dir = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_console_layout")
                && let Some(layout) = crate::state::SftpConsoleLayout::from_code(&v)
            {
                // An unknown code keeps the default placement: a stored
                // value nobody can read must not decide where a console
                // lands.
                self.prefs.sftp_console_layout = layout;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_default_editor") {
                self.prefs.sftp_default_editor = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_edit_autosave") {
                self.prefs.sftp_edit_autosave = v == "true";
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_connect_timeout") {
                self.prefs.sftp_connect_timeout = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_auth_timeout") {
                self.prefs.sftp_auth_timeout = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_session_timeout") {
                self.prefs.sftp_session_timeout = v;
            }
            if let Ok(Some(v)) = vault.get_setting("sftp_op_timeout") {
                self.prefs.sftp_op_timeout = v;
            }
        }
    }
}
