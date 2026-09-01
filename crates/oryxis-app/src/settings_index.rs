//! Settings search index: one entry per individual setting row, the
//! backing data for the Settings sidebar search and the command
//! palette's per-setting rows.
//!
//! The index is a hand-maintained catalog (the rows are built inline
//! during `view()`, so there is nothing to derive it from), with two
//! guard rails: the tests below assert every `label_key` resolves in
//! the English i18n table, and the project convention (CLAUDE.md) is
//! that a new setting lands with its index entry in the same change,
//! exactly like i18n keys and keynav wiring.
//!
//! Matching (`substring_match`): every query token must appear as a
//! CONTIGUOUS substring of the ACTIVE-language label, the ENGLISH label
//! (so English terms keep working in any UI language) or the entry's
//! static English keywords (ranked below label hits). Substring, NOT
//! fuzzy subsequence: on a short settings vocabulary fuzzy false-
//! positives constantly ("font" is a subsequence of "Warn beFore
//! multi-liNe pasTe"). Section visibility follows
//! `settings_section_items()` verbatim, so feature-gated sections never
//! leak results.
//!
//! Presentation (JetBrains model, driven from the sidebar render and
//! the `keynav` state): the section tree stays put, non-matching
//! sections dim, matching ones get a hit-count badge, and the open
//! section highlights EVERY matching row in place (amber outline). The
//! active find-next match (Enter / Shift+Enter cycle them in document
//! order via [`Self::settings_ordered_matches`]) gets the accent ring
//! and is scrolled into view. An entry whose row is conditionally
//! hidden (e.g. the privacy mask classes while Privacy mode is off)
//! still counts + opens the section, it just can't highlight a row
//! that isn't on screen.

use crate::state::SettingsSection;

/// One searchable setting.
pub(crate) struct SettingsIndexEntry {
    pub(crate) section: SettingsSection,
    /// i18n key of the row's visible label. Doubles as the reveal
    /// target: the section view compares `t(label_key)` against its
    /// row labels while rendering.
    pub(crate) label_key: &'static str,
    /// Static English fuzzy fodder: synonyms a user would type that
    /// the label itself doesn't contain.
    pub(crate) keywords: &'static str,
}

const fn e(
    section: SettingsSection,
    label_key: &'static str,
    keywords: &'static str,
) -> SettingsIndexEntry {
    SettingsIndexEntry { section, label_key, keywords }
}

use SettingsSection as S;

/// Every searchable setting, grouped by section in sidebar order.
/// Per-data-item rows (each custom theme, each paired device, ...)
/// and modal-internal fields are deliberately not indexed: the index
/// targets what a user would search for, not every clickable element.
pub(crate) static SETTINGS_INDEX: &[SettingsIndexEntry] = &[
    // ── Interface ──────────────────────────────────────────────────
    e(S::Interface, "language", "language locale translation idioma"),
    e(S::Interface, "layout_direction", "layout direction rtl ltr right left"),
    e(S::Interface, "nav_orientation", "navigation orientation horizontal vertical rail pills"),
    e(S::Interface, "flatten_hosts_label", "flatten hosts root groups folders dashboard"),
    e(S::Interface, "show_host_address_label", "host address ip show card dashboard"),
    e(S::Interface, "card_accent_glass_label", "glass accent card translucent dashboard"),
    e(S::Interface, "default_host_icon", "default host icon avatar style"),
    e(S::Interface, "close_button_position", "tab close button position left right"),
    e(S::Interface, "pinned_tab_style", "pinned tab style compact icon"),
    e(S::Interface, "tab_number_style", "tab number index numbering prefix badge icon slot ctrl digit"),
    e(S::Interface, "tab_number_slot_align", "align shortcuts ctrl digit home slot offset first tab"),
    e(S::Interface, "duplicate_tab_position", "duplicate tab position next beside end start append copy"),
    e(S::Interface, "show_tab_host_address_label", "tab host address ip port second line show"),
    e(S::Interface, "show_tab_status_dot", "tab status dot connection indicator"),
    e(S::Interface, "tab_accent_text", "tab text accent color tint"),
    e(S::Interface, "tab_accent_color", "tab accent color host app source"),
    e(S::Interface, "tab_fill_style", "tab fill gradient solid background"),
    e(S::Interface, "tab_bar_position", "tab bar position top bottom left right vertical"),
    e(S::Interface, "inactive_tab_style", "inactive tab style border underline separator none"),
    e(S::Interface, "tab_width_mode", "tab width uniform fixed equal same size adaptive ellipsis truncate reflow"),
    e(S::Interface, "tab_uniform_size", "tab width uniform small medium large ceiling cap maximum"),
    e(S::Interface, "pinned_tabs_top_bar", "pinned tabs top bar side dock"),
    e(S::Interface, "side_hide_top_bar", "hide top bar side vertical tabs"),
    e(S::Interface, "side_full_height", "full height tab strip side vertical"),
    e(S::Interface, "tab_accent_line", "tab underline accent line hairline"),
    e(S::Interface, "tab_accent_wash", "top bar wash accent tint background"),
    e(S::Interface, "show_status_bar", "status bar show bottom"),
    e(S::Interface, "status_show_connection", "status bar connection segment"),
    e(S::Interface, "status_show_version", "status bar version segment"),
    // Named for the SSH reading it started as, but the segment answers
    // for whichever transport the pane holds: on mosh it reports how
    // long the link has been quiet, since there is no round trip to
    // report. The keywords carry that, so searching "link" or "mosh"
    // finds the toggle without renaming a setting people already know.
    e(S::Interface, "status_show_latency", "status bar latency ping rtt segment link mosh quiet contact"),
    e(S::Interface, "status_show_dimensions", "status bar terminal size dimensions rows columns"),
    e(S::Interface, "status_show_cwd", "status bar current directory path cwd"),
    e(S::Interface, "status_bar_align_left", "status bar align content left leading edge alignment position"),
    e(S::Interface, "theme_new_custom", "create custom app ui theme color"),
    e(S::Interface, "theme_import", "import app ui theme json load"),
    e(S::Interface, "renderer_backend", "renderer graphics backend gpu vulkan opengl software"),
    e(S::Interface, "performance_mode", "performance mode fps battery speed"),
    e(S::Interface, "terminal_hints", "hints tips toast teaching links help"),
    // ── Terminal ───────────────────────────────────────────────────
    e(S::Terminal, "copy_on_select", "copy select clipboard mouse selection"),
    e(S::Terminal, "terminal_right_click", "right click paste context menu mouse"),
    e(S::Terminal, "copy_requires_right_click", "copy right click clipboard guard"),
    e(S::Terminal, "middle_click_paste", "middle click paste mouse x11"),
    e(S::Terminal, "careful_paste_label", "paste warn multiline confirm guard"),
    e(S::Terminal, "paste_guard_label", "paste guard suspicious content security homograph"),
    e(S::Terminal, "word_delimiters", "word delimiters double click selection boundary"),
    e(S::Terminal, "scrollback", "scrollback lines history buffer rows limit"),
    e(S::Terminal, "scrollback_reset_keypress", "scrollback reset keypress jump bottom"),
    e(S::Terminal, "terminal_password_autofill", "password autofill sudo suggest credential prompt"),
    e(S::Terminal, "scrollback_reset_output", "scrollback reset output activity jump bottom"),
    e(S::Terminal, "bold_bright", "bold bright colors text intensity"),
    e(S::Terminal, "pane_border_inactive", "pane border outline split inactive unfocused separator divider"),
    e(S::Terminal, "pane_gap", "pane gap gutter spacing split padding between panes"),
    e(S::Terminal, "keyword_highlight", "keyword highlight color url ip path automatic"),
    e(S::Terminal, "highlight_rules", "highlight rules keyword pattern regex color trigger action notify beep sound snippet watch alert error warn"),
    e(S::Terminal, "hl_rule_add", "add highlight rule new pattern watch"),
    e(S::Terminal, "command_history_capture", "command history capture record log"),
    e(S::Terminal, "shell_integration_copy", "shell integration snippet osc 633 tmux key nonce bashrc zshrc"),
    e(S::Terminal, "shell_integration_rotate", "shell integration rotate key nonce regenerate revoke"),
    e(S::Terminal, "cmd_history_file", "command history text file log append export"),
    e(S::Terminal, "smart_contrast", "smart contrast readability colors legibility blue"),
    e(S::Terminal, "terminal_opacity", "opacity transparency transparent translucent background see through blur glass"),
    e(S::Terminal, "terminal_bg_image", "background image picture wallpaper photo backdrop"),
    e(S::Terminal, "terminal_bg_fit", "background image fit cover contain stretch tile scale"),
    e(S::Terminal, "terminal_bg_dim", "background image dim fade opacity readability contrast"),
    e(S::Terminal, "terminal_auto_title", "auto title tab osc window name"),
    e(S::Terminal, "terminal_bell", "bell sound alert audio visual notification"),
    e(S::Terminal, "terminal_clipboard", "clipboard osc 52 copy access allow"),
    e(S::Terminal, "terminal_notification", "notification osc 9 desktop toast"),
    e(S::Terminal, "smart_tabs", "smart tabs long command detection rename"),
    e(S::Terminal, "smart_tabs_threshold", "long command threshold alert duration seconds"),
    e(S::Terminal, "sidebar_tab_locations", "sidebar left right dock position side region tabs location"),
    e(S::Terminal, "tab_tip_hosts", "hosts tree sidebar left right location dock"),
    e(S::Terminal, "sidebar_auto_open", "sidebar auto open connect show"),
    e(S::Terminal, "sidebar_default_tab", "sidebar default tab chat snippets files monitor history last opened"),
    e(S::Terminal, "terminal_font_size", "font size zoom text scale points"),
    e(S::Terminal, "terminal_font", "font family typeface monospace nerd pack download jetbrains cascadia caskaydia"),
    e(S::Terminal, "terminal_font_weight", "font weight bold medium semibold regular thin thick heavier stroke"),
    e(S::Terminal, "terminal_text_thickness", "text thickness stroke smoothing thin faint antialiasing sharpness contrast"),
    e(S::Terminal, "theme_new_custom", "create custom terminal theme color scheme"),
    e(S::Terminal, "theme_import", "import terminal theme scheme iterm base16"),
    e(S::Terminal, "rescan_terminals", "rescan detect local terminals shells refresh"),
    e(S::Terminal, "add_terminal", "add local terminal shell profile"),
    e(S::Terminal, "default_terminal_behavior", "default local terminal open behavior picker ask"),
    // ── Connection ─────────────────────────────────────────────────
    e(S::Connection, "new_connection_defaults", "new connection defaults host template"),
    e(S::Connection, "forward_ssh_agent", "forward ssh agent forwarding default"),
    e(S::Connection, "setting_connection_reuse", "reuse share ssh connection second tab controlmaster multiplex"),
    e(S::Connection, "port", "default port ssh 22 number"),
    e(S::Connection, "host_keepalive", "keepalive override default interval seconds"),
    e(S::Connection, "host_terminal_type", "terminal type term xterm environment"),
    e(S::Connection, "username", "default username login user ssh"),
    e(S::Connection, "auth_method", "auth method password key agent authentication default"),
    e(S::Connection, "identity", "identity default credential login"),
    e(S::Connection, "ssh_key", "ssh key default private keypair"),
    e(S::Connection, "parent_group", "parent group folder default organize"),
    e(S::Connection, "default_proxy", "proxy jump bastion default gateway socks"),
    e(S::Connection, "expose_to_mcp", "expose mcp ai assistant default access"),
    e(S::Connection, "host_encoding", "encoding charset utf-8 default terminal"),
    e(S::Connection, "env_vars", "environment variables env default"),
    e(S::Connection, "keepalive_interval", "keepalive interval seconds connection alive ping"),
    e(S::Connection, "auto_reconnect", "auto reconnect disconnect retry drop"),
    e(S::Connection, "max_reconnect_attempts", "max reconnect attempts retries limit"),
    e(S::Connection, "os_detection", "os detection detect remote operating system probe"),
    e(S::Connection, "login_scripts", "login automation script bastion jumpserver expect send prompt jump box"),
    // ── Security ───────────────────────────────────────────────────
    e(S::Security, "vault_password", "vault password master encryption protect"),
    e(S::Security, "lock_vault", "lock vault now secure"),
    e(S::Security, "update_password", "change master password rotate update"),
    e(S::Security, "auto_lock_minutes", "auto lock idle timeout vault minutes inactivity"),
    e(S::Security, "manual_lock_action", "manual lock button sleep soft hard ask sessions behavior"),
    e(S::Security, "privacy_mode_label", "privacy mode mask redact hide secrets screenshot"),
    e(S::Security, "privacy_class_public_ips", "mask public ip address privacy"),
    e(S::Security, "privacy_class_private_ips", "mask private loopback ip lan privacy"),
    e(S::Security, "privacy_class_usernames", "mask usernames privacy hide user"),
    e(S::Security, "privacy_class_hostnames", "mask saved hostnames host privacy hide"),
    e(S::Security, "privacy_always_mask_label", "always mask words custom redact list privacy"),
    e(S::Security, "privacy_never_mask_label", "never mask allowlist exclude usernames privacy"),
    e(S::Security, "session_logging", "session logging record terminal history"),
    e(S::Security, "session_log_full", "detailed recording replay full session cast"),
    e(S::Security, "session_log_compress", "compress recordings gzip session log size"),
    e(S::Security, "connection_history", "connection history log recent hosts track"),
    e(S::Security, "log_retention_label", "log retention auto delete purge age cleanup"),
    e(S::Security, "log_size_cap_label", "log size cap limit disk space quota recording storage"),
    e(S::Security, "export_vault", "export vault backup save file portable"),
    e(S::Security, "export_hosts_csv", "export hosts csv spreadsheet list share secrets-free"),
    e(S::Security, "import_vault", "import vault restore load file portable"),
    e(S::Security, "import_from_sftp", "import sftp remote restore backup"),
    e(S::Security, "import_ssh_config_btn", "import ssh config openssh hosts migrate"),
    // ── Sync ───────────────────────────────────────────────────────
    e(S::Sync, "sync_transport_field", "sync transport method p2p sftp snapshot"),
    e(S::Sync, "sync_transport_folder", "folder sync snapshot onedrive dropbox google drive icloud network share usb"),
    e(S::Sync, "sync_transport_git", "git sync snapshot repository remote history versions forge gitlab gitea"),
    e(S::Sync, "sync_transport_webdav", "webdav sync snapshot nextcloud owncloud synology server url etag"),
    e(S::Sync, "sftp_sync_host", "sftp sync backup host server select"),
    e(S::Sync, "sftp_sync_path", "sftp sync remote path directory folder"),
    e(S::Sync, "sftp_sync_passphrase", "sftp sync passphrase password encrypt"),
    e(S::Sync, "sync_mode", "sync mode auto manual automatic"),
    e(S::Sync, "sync_passwords", "sync passwords credentials devices share"),
    e(S::Sync, "sync_now", "sync now manual trigger run push pull"),
    e(S::Sync, "sync_device_name", "device name label identify sync peer"),
    e(S::Sync, "sync_host_pairing", "pair device host pairing new add"),
    e(S::Sync, "sync_join_pairing", "pair device join code connect link"),
    e(S::Sync, "sync_signaling_url", "signaling server url sync internet"),
    e(S::Sync, "sync_signaling_token", "signaling token auth secret relay"),
    e(S::Sync, "sync_relay_url", "relay url server sync nat traversal"),
    e(S::Sync, "sync_listen_port", "listen port sync p2p network bind"),
    e(S::Sync, "sync_wizard_button", "set up relay self host server wizard compose"),
    // ── AI ─────────────────────────────────────────────────────────
    e(S::AI, "provider", "ai provider openai anthropic ollama llm"),
    e(S::AI, "model", "ai model name gpt claude llama"),
    e(S::AI, "api_url", "ai custom api url endpoint base"),
    e(S::AI, "api_key", "ai api key secret token"),
    e(S::AI, "ai_reasoning", "ai reasoning thinking chain of thought deepseek gemini cost tokens"),
    e(S::AI, "ai_save_history", "ai chat conversations save history vault privacy record transcript"),
    e(S::AI, "additional_system_prompt", "ai system prompt instructions custom persona"),
    // ── MCP ────────────────────────────────────────────────────────
    e(S::Mcp, "mcp_server", "mcp server enable model context protocol port"),
    e(S::Mcp, "mcp_setup_guide", "mcp setup guide help config claude cursor"),
    e(S::Mcp, "mcp_token_regenerate", "mcp token regenerate rotate reset auth"),
    e(S::Mcp, "mcp_install_claude", "mcp install claude code config register"),
    // ── SFTP ───────────────────────────────────────────────────────
    e(S::Sftp, "setting_sftp_console_layout", "sftp console placement split pane beside below maximized zoom full tab"),
    e(S::Sftp, "setting_default_editor", "sftp default editor external open program"),
    e(S::Sftp, "setting_sftp_ask_download_dir", "sftp download destination folder ask where save prompt"),
    e(S::Sftp, "default_download_dir", "download folder directory default zmodem transfer rz sz save"),
    e(S::Sftp, "setting_sftp_upload_temp_name", "sftp upload temporary filename part scratch rename atomic partial resume"),
    e(S::Sftp, "setting_edit_autosave_toggle", "sftp autosave auto upload edited files save"),
    e(S::Sftp, "transfer_parallelism", "sftp transfer parallelism concurrency parallel streams"),
    e(S::Sftp, "connect_timeout", "sftp connect timeout seconds"),
    e(S::Sftp, "auth_timeout", "sftp auth timeout seconds login"),
    e(S::Sftp, "channel_open_timeout", "sftp channel open timeout session"),
    e(S::Sftp, "operation_timeout", "sftp operation timeout seconds stall"),
    // ── Monitoring ─────────────────────────────────────────────────
    e(S::Monitoring, "monitor_all_hosts", "monitoring all hosts enable global vitals"),
    e(S::Monitoring, "monitor_interval", "monitoring interval seconds probe poll"),
    e(S::Monitoring, "monitor_status_bar", "monitoring status bar vitals cpu memory widget"),
    // ── SSH Agent ──────────────────────────────────────────────────
    e(S::Agent, "agent_server_confirm", "ssh agent confirm each use approve signature"),
    e(S::Agent, "agent_allow_add", "ssh agent accept add keys external apps keepassxc"),
    e(S::Agent, "agent_server_copy_path", "ssh agent socket path copy pipe"),
    e(S::Agent, "agent_server_snippet_ssh_config", "ssh agent identityagent config snippet"),
    // ── Advanced ───────────────────────────────────────────────────
    e(S::Advanced, "download_mirror", "download mirror china github custom proxy project"),
    e(S::Advanced, "debug_logging", "debug logging enable log file diagnostics"),
    e(S::Advanced, "perf_overlay", "performance hud overlay fps terminal frames"),
    e(
        S::Plugins,
        "setting_network_tools",
        "network tools dns ping traceroute port test whois rbl dnsbl tls certificate utilities",
    ),
    e(S::Advanced, "copy_env_info", "copy environment info report github issue diagnostics"),
    // ── About ──────────────────────────────────────────────────────
    // (the update rows live in `update_entries()`, they don't exist in
    // a packaged build)
    // ── Shortcuts ──────────────────────────────────────────────────
    e(S::Shortcuts, "tab_slot_includes_home", "ctrl digit tab slot home first number offset shortcut"),
    e(S::Shortcuts, "hotkey_reset_all", "reset all shortcuts hotkeys defaults keybindings"),
    // ── Cloud ──────────────────────────────────────────────────────
    e(S::Cloud, "settings_cloud_auto_refresh", "cloud auto refresh profiles discover"),
    e(S::Cloud, "settings_cloud_auto_refresh_interval", "cloud refresh interval minutes"),
    e(S::Cloud, "settings_cloud_auto_archive", "cloud auto archive orphaned hosts"),
    e(S::Cloud, "settings_cloud_orphan_archive_days", "cloud orphan archive days retention"),
    // ── Features & Plugins ─────────────────────────────────────────
    e(S::Plugins, "ai_assistant", "feature ai assistant enable chat"),
    e(S::Plugins, "sftp", "feature sftp file transfer browser enable"),
    e(S::Plugins, "sync", "feature sync vault devices enable p2p"),
    e(S::Plugins, "remote_desktop", "feature remote desktop rdp vnc enable"),
    e(S::Plugins, "feature_monitoring", "feature host monitoring enable vitals"),
    e(S::Plugins, "feature_tmux", "feature tmux session manager multiplexer attach kill"),
    e(S::Plugins, "agent_server", "feature ssh agent server enable"),
    e(S::Plugins, "plugin_action_check_updates", "plugin check updates all providers"),
    e(S::Plugins, "plugins_auto_update_global", "plugin auto update all global"),
];

/// The self-update rows, present only in unpackaged builds. Inside an
/// MSIX package the Microsoft Store services the app and Settings >
/// About renders no update panel at all, so a search hit here would
/// open a section whose row can never appear. Same rule as
/// [`platform_entries`], gated at runtime instead of at compile time.
fn update_entries() -> &'static [SettingsIndexEntry] {
    static UPDATE: &[SettingsIndexEntry] = &[
        e(S::About, "auto_check_updates", "update auto check startup"),
        e(S::About, "update_channel", "update channel stable nightly"),
        e(S::About, "check_for_updates_now", "check for updates now manual version"),
    ];
    if crate::packaged::is_packaged() {
        &[]
    } else {
        UPDATE
    }
}

/// Platform-gated entries appended to the base index: rows that only
/// exist in some builds, so a search on the other platforms can't
/// surface a result whose row can never render.
fn platform_entries() -> &'static [SettingsIndexEntry] {
    #[cfg(target_os = "windows")]
    {
        static WIN: &[SettingsIndexEntry] = &[
            e(S::Interface, "close_to_tray", "close tray notification area background"),
            e(S::Interface, "minimize_to_tray", "minimize tray notification area hide"),
            e(S::Security, "biometric_setting_windows", "biometric fingerprint windows hello unlock face"),
            e(S::Agent, "agent_openssh_pipe", "ssh agent openssh pipe standard alias"),
        ];
        WIN
    }
    #[cfg(target_os = "macos")]
    {
        static MAC: &[SettingsIndexEntry] = &[
            e(S::Security, "biometric_setting_macos", "biometric fingerprint touch id unlock"),
            e(S::Agent, "agent_server_snippet_env", "ssh agent auth sock env snippet shell"),
        ];
        MAC
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        static LINUX: &[SettingsIndexEntry] = &[
            e(S::Security, "biometric_setting_linux", "biometric keyring login unlock secret service"),
            e(S::Agent, "agent_server_snippet_env", "ssh agent auth sock env snippet shell"),
        ];
        LINUX
    }
}

/// Settings-search scorer: SUBSTRING matching, not fuzzy subsequence.
/// Every query token must appear as a contiguous substring of the
/// label, the English label, or the keyword fodder; a row that matches
/// no token is dropped. Fuzzy subsequence (the command palette's model)
/// is wrong here - it false-positives constantly on a short query
/// ("font" is a scattered subsequence of "Warn beFore multi-liNe pasTe"
/// and of "...file log append export"), lighting up unrelated rows.
/// JetBrains-style settings search is literal substring, and precision
/// matters more than typo tolerance on this short, known vocabulary.
///
/// Ranking per token: a label hit outranks an English-label hit, which
/// outranks a keyword-only hit; within a field an earlier position and
/// a word-boundary start score higher, and a longer token (more
/// specific) adds a little. Multi-word queries sum their tokens, so
/// order is irrelevant ("copy select" == "select copy").
fn substring_match(label: &str, en: &str, keywords: &str, needle: &str) -> Option<i32> {
    let ll = label.to_lowercase();
    let el = en.to_lowercase();
    let kl = keywords.to_lowercase();
    let mut score = 0i32;
    for tok in needle.split_whitespace() {
        let tlen = tok.chars().count() as i32;
        let hit = if let Some(pos) = ll.find(tok) {
            let boundary = pos == 0 || !ll.as_bytes()[pos - 1].is_ascii_alphanumeric();
            120 - (pos.min(60) as i32) + tlen + if boundary { 15 } else { 0 }
        } else if let Some(pos) = el.find(tok) {
            110 - (pos.min(60) as i32) + tlen
        } else if kl.contains(tok) {
            40 + tlen
        } else {
            return None;
        };
        score += hit;
    }
    Some(score)
}

impl crate::app::Oryxis {
    /// Rank the settings index against `query`: every visible-section
    /// entry whose active-language label, English label or keywords
    /// fuzzy-match, best score first (ties: sidebar section order,
    /// then label). Returns `(entry, section label)` pairs; the
    /// section label comes from `settings_section_items()` so it is
    /// the exact sidebar wording. Empty/blank query returns nothing
    /// (the sidebar then shows the plain section list).
    pub(crate) fn settings_search_results(
        &self,
        query: &str,
    ) -> Vec<(&'static SettingsIndexEntry, &'static str)> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        // Visible sections, in sidebar order, with their labels: the
        // single source of feature gating (hidden section = no rows).
        let sections = self.settings_section_items();
        let section_pos = |s: SettingsSection| sections.iter().position(|(_, v)| *v == s);
        let mut scored: Vec<(i32, usize, &'static SettingsIndexEntry, &'static str)> =
            SETTINGS_INDEX
                .iter()
                .chain(platform_entries())
                .chain(update_entries())
                .filter_map(|entry| {
                    let pos = section_pos(entry.section)?;
                    let label = crate::i18n::t(entry.label_key);
                    let en = crate::i18n::en_lookup(entry.label_key);
                    let score =
                        substring_match(label, en, entry.keywords, &needle)?;
                    Some((score, pos, entry, sections[pos].0))
                })
                .collect();
        scored.sort_by(|(sa, pa, ea, _), (sb, pb, eb, _)| {
            sb.cmp(sa)
                .then(pa.cmp(pb))
                .then_with(|| {
                    crate::i18n::t(ea.label_key).cmp(crate::i18n::t(eb.label_key))
                })
        });
        scored.into_iter().map(|(_, _, e, l)| (e, l)).collect()
    }

    /// Matches for `query` in DOCUMENT order (sidebar section order,
    /// then `SETTINGS_INDEX` authoring order, which mirrors render
    /// order), for Enter / Shift+Enter find-next cycling. Returns
    /// `(section, resolved label)`. Same visibility gating and matcher
    /// as [`Self::settings_search_results`], only the ordering differs.
    pub(crate) fn settings_ordered_matches(
        &self,
        query: &str,
    ) -> Vec<(SettingsSection, &'static str)> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let sections = self.settings_section_items();
        let mut out: Vec<(usize, usize, SettingsSection, &'static str)> = SETTINGS_INDEX
            .iter()
            .chain(platform_entries())
            .chain(update_entries())
            .enumerate()
            .filter_map(|(i, entry)| {
                let pos = sections.iter().position(|(_, v)| *v == entry.section)?;
                let label = crate::i18n::t(entry.label_key);
                let en = crate::i18n::en_lookup(entry.label_key);
                substring_match(label, en, entry.keywords, &needle)?;
                Some((pos, i, entry.section, label))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        out.into_iter().map(|(_, _, s, l)| (s, l)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_entries() -> impl Iterator<Item = &'static SettingsIndexEntry> {
        SETTINGS_INDEX
            .iter()
            .chain(platform_entries())
            .chain(update_entries())
    }

    #[test]
    fn every_label_key_resolves_in_en() {
        for entry in all_entries() {
            assert_ne!(
                crate::i18n::en_lookup(entry.label_key),
                "???",
                "missing i18n for settings index key {:?} ({:?})",
                entry.label_key,
                entry.section,
            );
        }
    }

    #[test]
    fn no_duplicate_section_key_pairs() {
        let mut seen = std::collections::HashSet::new();
        for entry in all_entries() {
            assert!(
                seen.insert((entry.section, entry.label_key)),
                "duplicate settings index entry: {:?} / {}",
                entry.section,
                entry.label_key,
            );
        }
    }

    #[test]
    fn substring_only_no_subsequence_false_positive() {
        // The QA regression: "font" is a scattered subsequence of
        // "Warn before multi-line paste" but NOT a substring, so it
        // must not match; the literal "Terminal Font" must.
        assert!(substring_match("Terminal Font", "Terminal Font", "font family", "font").is_some());
        assert!(
            substring_match("Warn before multi-line paste", "Warn before multi-line paste", "paste warn guard", "font")
                .is_none()
        );
    }

    #[test]
    fn label_hit_outranks_keyword_hit() {
        // "font" in the visible label beats "font" only in keywords.
        let label = substring_match("Terminal Font", "Terminal Font", "", "font").unwrap();
        let kw = substring_match("Something else", "Something else", "font size zoom", "font").unwrap();
        assert!(label > kw, "{label} > {kw}");
    }

    #[test]
    fn multi_word_queries_match_order_free() {
        // Every token must be a substring somewhere; order is free.
        assert!(substring_match(
            "Select text to copy & Right click to paste",
            "Select text to copy & Right click to paste",
            "copy select clipboard",
            "select copy",
        )
        .is_some());
        // A token that matches nothing kills the row.
        assert!(substring_match("Terminal Font", "Terminal Font", "font family", "font zzz").is_none());
        // Keyword substring works even when the label doesn't contain it.
        assert!(substring_match(
            "Auto-lock vault (minutes, 0 = off)",
            "Auto-lock vault (minutes, 0 = off)",
            "auto lock idle timeout inactivity",
            "idle",
        )
        .is_some());
    }

    #[test]
    fn keywords_are_ascii_lowercase() {
        // The fuzzy matcher lowercases the haystack anyway, but the
        // keyword strings are a hand-written convention: keep them
        // lowercase ascii so diffs stay boring.
        for entry in all_entries() {
            assert!(
                entry
                    .keywords
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || " -.".contains(c)),
                "keywords not lowercase-ascii for {}: {:?}",
                entry.label_key,
                entry.keywords,
            );
        }
    }
}
