//! Every settings key the workspace reads or writes is CLASSIFIED: a
//! device key (`oryxis_vault::device::is_device_key`, lives in
//! `device.json`, one per machine) or a vault key (listed here, lives in
//! each vault's own table). The routing inside `VaultStore` decides by
//! that classification, so a new key that lands without a decision is
//! silently a vault key; with several vaults it would be one vault
//! showing a value another one set, or the reverse. This test makes the
//! decision explicit: add the key to `VAULT_KEYS` below, or to
//! `DEVICE_KEYS` / `DEVICE_PREFIXES` in `oryxis-vault/src/device.rs`.

use std::collections::BTreeSet;
use std::path::Path;

/// Settings that belong to a vault (CLAUDE.md § Settings table: per
/// vault unless the process or the machine can only have one of it).
const VAULT_KEYS: &[&str] = &[
    "ai_api_key", "ai_api_url", "ai_default_mode", "ai_enabled", "ai_model", "ai_provider",
    "ai_reasoning", "ai_save_history", "ai_system_prompt", "app_theme", "auto_lock_minutes",
    "auto_reconnect", "biometric_unlock_enabled", "bold_is_bright", "card_accent_glass",
    "careful_paste", "cloud_auto_archive_orphans", "cloud_auto_refresh_enabled",
    "cloud_auto_refresh_interval_minutes", "cloud_orphan_archive_days", "command_history",
    "command_history_file", "confirm_close_session_tab", "connection_history", "copy_on_select",
    "default_agent_forwarding", "default_auth_method", "default_encoding", "default_env_vars",
    "default_group_id", "default_host_icon", "default_identity_id", "default_keepalive",
    "default_key_id", "default_mcp_enabled", "default_port", "default_proxy_identity_id",
    "default_terminal_type", "default_username", "defaults_collapsed", "duplicate_tab_position",
    "files_recent_folders", "flatten_hosts", "folder_sync_device_tag", "has_user_password",
    "hint_link_click_used", "hint_privacy_mask", "host_list_view", "host_monitoring_enabled",
    "host_monitoring_seeded", "host_view_mode", "hosts_sort", "inactive_tab_style",
    "install_presets_seeded", "keepalive_default_v2_applied", "keepalive_interval", "keys_sort",
    "keyword_highlight", "layout_mode", "logs_retention", "manual_lock_action",
    "max_reconnect_attempts", "mcp_config_vault_pw", "mcp_server_allow_transfers", "mcp_server_allow_writes",
    "mcp_server_enabled", "mcp_server_token", "monitor_all_hosts", "monitor_dash_list_view",
    "monitor_dash_live_only", "monitor_interval_seconds", "monitor_status_bar", "nav_orientation",
    "nav_rail_expanded", "network_tools_enabled", "open_tabs", "open_tabs_active", "os_detection",
    "pane_border_inactive", "pane_end_action", "pane_gap", "pane_headers", "paste_guard",
    "performance_mode", "pinned_tab_style", "pinned_tabs", "pinned_tabs_top_bar",
    "port_forwards_migrated", "privacy_always_mask", "privacy_default_off_applied",
    "privacy_mask_hostnames", "privacy_mask_private_ips", "privacy_mask_public_ips",
    "privacy_mask_usernames", "privacy_mode", "privacy_never_mask", "remote_desktop_enabled",
    "restore_last_active_tab", "restore_tabs_connect", "restore_tabs_on_launch",
    "right_click_copy", "scrollback_reset_keypress", "scrollback_reset_output", "scrollback_rows",
    "session_log_compress", "session_log_file", "session_log_full", "session_log_max_bytes",
    "session_logging", "sftp_ask_download_dir", "sftp_auth_timeout", "sftp_col_order",
    "sftp_col_widths", "sftp_columns", "sftp_concurrency", "sftp_connect_timeout",
    "sftp_console_layout", "sftp_dir_double_click", "sftp_edit_autosave", "sftp_enabled",
    "sftp_file_double_click", "sftp_log_height", "sftp_op_timeout", "sftp_session_timeout",
    "sftp_split_ratio", "sftp_sync_device_tag", "sftp_upload_temp_name", "shell_integration_nonce",
    "show_host_address", "show_status_bar", "show_tab_host_address", "show_tab_status_dot",
    "side_full_height", "side_hide_top_bar", "side_panel_width", "sidebar_auto_open",
    "sidebar_default_tab", "sidebar_tab_sides", "smart_contrast", "smart_tabs",
    "smart_tabs_long_seconds", "snippet_tag_filter", "snippets_sort", "ssh_connection_reuse",
    "ssh_session_timeout", "status_bar_align_left", "status_show_connection", "status_show_cwd",
    "status_show_dimensions", "status_show_latency", "status_show_version",
    "sync_device_identity", "sync_enabled", "sync_folder_path", "sync_git_remote",
    "sync_hosted_migrated", "sync_key_private_migrated", "sync_mode", "sync_passwords",
    "sync_relay_url", "sync_sftp_host_id", "sync_sftp_passphrase", "sync_sftp_remote_path",
    "sync_signaling_token", "sync_signaling_url", "sync_transport", "sync_webdav_password",
    "sync_webdav_url", "sync_webdav_user", "tab_accent_color", "tab_accent_line",
    "tab_accent_text", "tab_accent_wash", "tab_bar_position", "tab_close_button_side",
    "tab_double_click", "tab_fill_style", "tab_number_style", "tab_slot_includes_home", "tab_slots_home_migrated", "tab_switch_order",
    "tab_uniform_size", "tab_width_mode", "terminal_auto_title", "terminal_bell_mode",
    "terminal_bg_dim", "terminal_bg_fit", "terminal_bg_image", "terminal_clipboard_access",
    "terminal_cursor_blink", "terminal_cursor_shape", "terminal_font_name", "terminal_font_size", "terminal_font_weight",
    "terminal_highlight_rules", "terminal_history_suggest", "terminal_hint_mode", "terminal_link_confirm",
    "terminal_link_tunnel", "terminal_notification", "terminal_opacity",
    "terminal_password_autofill", "terminal_right_click", "terminal_sidebar_side",
    "terminal_text_thickness", "terminal_theme", "terminal_theme_override",
    "tmux_manager_enabled", "word_delimiters", "zmodem_ask_download_dir",
];

/// Every settings key (or `format!` prefix) named in the non-test source
/// of every workspace crate: literals passed to `get_setting` /
/// `set_setting` / `persist_setting`, and `const ..SETTING` / `..SETTING_KEY: &str`
/// declarations (the keys handed around by name).
fn keys_in_source() -> BTreeSet<(String, String)> {
    let call = regex::Regex::new(
        r#"(?:get_setting|set_setting|persist_setting)\(\s*&?(?:format!\(\s*)?"([^"{]*)"#,
    )
    .unwrap();
    let constant = regex::Regex::new(r#"const (?:[A-Z_]*_)?SETTING(?:_KEY)?: &str = "([^"]+)""#).unwrap();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates dir");
    let mut found = BTreeSet::new();
    let mut stack = vec![crates.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if path.is_dir() {
                // Integration tests and build output are not the app.
                if !matches!(name, "tests" | "target" | "harness") {
                    stack.push(path);
                }
                continue;
            }
            if !name.ends_with(".rs") || name.ends_with("tests.rs") || name == "tests.rs" {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read source");
            // Unit tests set throwaway keys; the app's code is above them.
            let src = src.split("#[cfg(test)]").next().unwrap_or_default();
            let rel = path.strip_prefix(crates).unwrap_or(&path).display().to_string();
            for re in [&call, &constant] {
                for cap in re.captures_iter(src) {
                    found.insert((cap[1].to_owned(), rel.clone()));
                }
            }
        }
    }
    found
}

#[test]
fn every_settings_key_is_a_device_key_or_a_vault_key() {
    let vault: BTreeSet<&str> = VAULT_KEYS.iter().copied().collect();
    let unclassified: Vec<String> = keys_in_source()
        .into_iter()
        .filter(|(key, _)| {
            !key.is_empty() && !oryxis_vault::device::is_device_key(key) && !vault.contains(key.as_str())
        })
        .map(|(key, file)| format!("{key} ({file})"))
        .collect();
    assert!(
        unclassified.is_empty(),
        "settings keys with no scope: {unclassified:?}. Add each to VAULT_KEYS in \
         settings_scope_tests.rs, or to DEVICE_KEYS / DEVICE_PREFIXES in oryxis-vault/src/device.rs"
    );
}

#[test]
fn no_key_is_both_a_device_key_and_a_vault_key() {
    let both: Vec<&str> =
        VAULT_KEYS.iter().copied().filter(|k| oryxis_vault::device::is_device_key(k)).collect();
    assert!(both.is_empty(), "classified twice: {both:?}");
}

#[test]
fn the_scan_sees_the_keys_it_is_meant_to_see() {
    // A regex that stopped matching would pass the first test vacuously.
    let keys: BTreeSet<String> = keys_in_source().into_iter().map(|(k, _)| k).collect();
    for expected in ["language", "app_theme", "hotkey_", "offline_mode", "terminal_highlight_rules"] {
        assert!(keys.contains(expected), "the scan no longer finds {expected}");
    }
}
