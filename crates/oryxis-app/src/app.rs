use iced::keyboard;
use iced::widget::{image, text_editor};
use iced::{Point, Theme};

use oryxis_core::models::connection::Connection;
use oryxis_core::models::group::Group;
use oryxis_core::models::identity::Identity;
use oryxis_core::models::key::SshKey;
use oryxis_vault::VaultStore;

use std::sync::OnceLock;
use uuid::Uuid;

/// Cross-thread hand-off for `--connect <uuid>` CLI argument. Set by
/// `main.rs` before `iced::application` starts; read inside `Oryxis::boot`.
/// Using a `OnceLock` (instead of passing through `boot`) lets us keep
/// iced's zero-arg boot signature.
pub static AUTO_CONNECT: OnceLock<Uuid> = OnceLock::new();

/// Inherited vault master password, populated by `main.rs` when the
/// parent process spawned us with `--inherit-vault` and piped the
/// password through stdin. Used at boot to silently unlock the vault so
/// the user doesn't have to re-type for "Duplicate in New Window".
pub static AUTO_PASSWORD: OnceLock<String> = OnceLock::new();

use crate::state::{
    ConnectionForm, ConnectionProgress, OverlayState, SettingsSection, TerminalTab, VaultState,
    View,
};
use crate::theme::OryxisColors;

// `Message` lives in its own module; re-export so call sites that
// import `crate::app::Message` keep working.
pub use crate::messages::Message;

// Layout constants
pub(crate) const DEFAULT_TERM_COLS: u32 = 120;
pub(crate) const DEFAULT_TERM_ROWS: u32 = 40;
pub(crate) const PANEL_WIDTH: f32 = 420.0;
pub(crate) const SIDEBAR_WIDTH: f32 = 180.0;
pub(crate) const SIDEBAR_WIDTH_COLLAPSED: f32 = 56.0;
pub(crate) const CARD_WIDTH: f32 = 280.0;

/// Fallback monospace font names offered when the system enumeration
/// returns nothing (boot-time scan still in flight, fontdb error, or
/// a stripped-down system with no installed monospace fonts beyond the
/// bundled Source Code Pro).
///
/// `Source Code Pro` is bundled with the binary (see `main.rs`). The rest
/// are looked up from the OS fontconfig; if a name doesn't resolve,
/// cosmic-text falls back gracefully to the system default monospace.
const TERMINAL_FONT_FALLBACK: &[&str] = &[
    "Source Code Pro",
    "Source Code Pro Medium",
    "JetBrains Mono",
    "Fira Code",
    "Fira Mono",
    "Cascadia Code",
    "Ubuntu Mono",
    "DejaVu Sans Mono",
    "Droid Sans Mono",
    "PT Mono",
    "Andale Mono",
    "Anonymous Pro",
    "Inconsolata",
    "Inconsolata-g",
    "Meslo",
    "Operator Mono Book",
    "Operator Mono Medium",
    "Menlo",
    "Monaco",
    "Consolas",
];

/// Returns the list of monospace fonts available to the picker.
///
/// Builds a fresh `fontdb::Database`, loads the system fonts on this
/// platform, and filters to families that report `monospaced`. The
/// bundled Source Code Pro is always prepended so it's the first
/// option even on systems with rich font libraries.
///
/// On error or empty enumeration we fall back to
/// `TERMINAL_FONT_FALLBACK` so the picker is never empty.
pub(crate) fn enumerate_terminal_fonts() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut names: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for face in db.faces() {
        if !face.monospaced {
            continue;
        }
        if let Some((family, _lang)) = face.families.first() {
            // Filter out empty / placeholder names defensively; some
            // systems carry symbol-only faces marked monospace.
            let trimmed = family.trim();
            if !trimmed.is_empty() {
                names.insert(trimmed.to_string());
            }
        }
    }

    if names.is_empty() {
        return TERMINAL_FONT_FALLBACK.iter().map(|s| s.to_string()).collect();
    }

    // Prepend the bundled Source Code Pro so it's always picker entry
    // #1; the rest of the names come from the dedup'd system scan.
    let mut out: Vec<String> = Vec::with_capacity(names.len() + 1);
    let bundled = "Source Code Pro".to_string();
    out.push(bundled.clone());
    for n in names {
        if n != bundled {
            out.push(n);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct Oryxis {
    // Vault
    pub(crate) vault: Option<VaultStore>,
    pub(crate) vault_state: VaultState,
    pub(crate) vault_password_input: String,
    pub(crate) vault_password_visible: bool,
    pub(crate) vault_error: Option<String>,
    pub(crate) logo_handle: image::Handle,
    pub(crate) logo_small_handle: image::Handle,

    // Data
    pub(crate) connections: Vec<Connection>,
    pub(crate) groups: Vec<Group>,

    // UI state
    pub(crate) active_view: View,
    pub(crate) active_group: Option<Uuid>,  // None = root, Some(id) = inside folder
    pub(crate) host_search: String,
    pub(crate) quick_host_input: String,
    pub(crate) sidebar_collapsed: bool,

    // Tabs
    pub(crate) tabs: Vec<TerminalTab>,
    pub(crate) active_tab: Option<usize>,
    /// Last terminal tab that had focus. Preserved when switching to nav-only
    /// views (Snippets, Keys, …) so snippet injection still targets that session.
    pub(crate) last_terminal_tab: Option<usize>,
    pub(crate) hovered_tab: Option<usize>,
    pub(crate) show_new_tab_picker: bool,
    pub(crate) new_tab_picker_search: String,
    /// Termius-style "Jump to" modal, lists all open tabs (plus Quick
    /// connect entries) for direct navigation when the bar runs out of
    /// horizontal room. Triggered by the `⋯` button in the tab bar or
    /// Ctrl+J anywhere.
    pub(crate) show_tab_jump: bool,
    pub(crate) tab_jump_search: String,
    /// Top-left burger menu visibility. Mirrors Termius's `☰` strip at
    /// the start of the tab bar: Settings / Updates / About / Exit.
    /// Toggled via the burger button or by pressing the same button
    /// again to dismiss.
    pub(crate) show_burger_menu: bool,

    // Icon/color picker (from the host editor's icon box).
    pub(crate) show_icon_picker: bool,
    pub(crate) icon_picker_for: Option<Uuid>,
    pub(crate) icon_picker_icon: Option<String>,
    pub(crate) icon_picker_color: Option<String>,
    pub(crate) icon_picker_hex_input: String,
    /// Whether the per-host terminal theme picker modal is open.
    /// Drawn on top of the host editor; the form's
    /// `terminal_theme` field is updated as soon as the user picks
    /// a card.
    pub(crate) show_theme_picker: bool,
    /// Whether the jump host picker modal is open. Opened from the
    /// "Jump Host" row in the host editor's Advanced section. Search
    /// filters by label, hostname, group, or username.
    pub(crate) show_jump_host_picker: bool,
    pub(crate) jump_host_search: String,
    pub(crate) connecting: Option<ConnectionProgress>,
    /// Counter that advances ~every 100ms while a connection is in progress.
    /// Used only to drive the pulsing "loading" ring on the active step dot.
    pub(crate) connect_anim_tick: u32,
    /// Timestamp of the last `WindowDrag` / `WindowResizeDrag` we
    /// forwarded to the OS. iced's `MouseArea` fires `on_press` on
    /// **both** clicks of a double-click (before the `on_double_click`
    /// lands), and forwarding two `iced::window::drag(...)` calls in
    /// quick succession leaves the OS in a flaky state, Windows races
    /// it with our follow-up `toggle_maximize` / `resize` and the
    /// window snaps right back. We swallow press handlers within a
    /// short window after the first one to keep the double-click path
    /// clean.
    pub(crate) last_window_press_at: Option<std::time::Instant>,

    // Host key verification dialog
    pub(crate) pending_host_key: Option<oryxis_ssh::HostKeyQuery>,
    pub(crate) host_key_response_tx: Option<tokio::sync::mpsc::Sender<bool>>,

    // Connection editor
    pub(crate) show_host_panel: bool,
    pub(crate) editor_form: ConnectionForm,
    pub(crate) host_panel_error: Option<String>,

    // Card hover & context menu
    pub(crate) hovered_card: Option<usize>,
    /// Hovered folder card on the dashboard (root view), drives the
    /// `⋮` menu visibility, mirroring `hovered_card` for hosts.
    pub(crate) hovered_folder_card: Option<Uuid>,
    /// Hovered key card / identity card in the keychain view, same
    /// hover-only-dots UX as host cards.
    pub(crate) hovered_key_card: Option<usize>,
    pub(crate) hovered_identity_card: Option<usize>,
    pub(crate) hovered_snippet_card: Option<usize>,
    pub(crate) card_context_menu: Option<usize>,

    // Floating overlay menu
    pub(crate) overlay: Option<OverlayState>,
    /// Folder rename inline editor, `Some((group_id, current_input))`
    /// while the modal is open.
    pub(crate) folder_rename: Option<(Uuid, String)>,
    /// Folder delete confirmation, group ID waiting for the user to
    /// pick "move hosts to root" / "delete with hosts" / cancel.
    pub(crate) folder_delete: Option<Uuid>,
    /// Connection ID to auto-open after the vault unlocks. Set from the
    /// `--connect` CLI flag captured at process start; cleared once the
    /// dispatch fires so a vault re-lock + unlock doesn't re-trigger it.
    pub(crate) pending_auto_connect: Option<Uuid>,
    /// Master password retained in memory for spawning child processes
    /// (Duplicate in New Window). Populated after a successful
    /// unlock / setup, cleared if the user explicitly re-locks.
    pub(crate) master_password: Option<String>,
    /// SFTP browser state. Lives at the top level so the user can pick
    /// a different host without losing their local-pane navigation.
    pub(crate) sftp: crate::state::SftpState,
    pub(crate) mouse_position: Point,
    pub(crate) window_size: iced::Size,
    /// Live keyboard modifier state, updated from `ModifiersChanged`
    /// keyboard events. Used by SFTP click logic for ctrl/shift-click
    /// selection, iced's MouseArea events don't include modifiers.
    pub(crate) modifiers: keyboard::Modifiers,
    /// Whether the OS window is currently maximized. Used by the custom
    /// chrome to swap the maximize glyph for a "restore" glyph. Toggled
    /// optimistically on `WindowMaximizeToggle` since our chrome is the only
    /// path that can change this state (native titlebar is disabled).
    pub(crate) window_maximized: bool,
    /// Whether the window is in native fullscreen mode. Flipped by F11.
    /// Same optimistic pattern as `window_maximized` because the OS-side
    /// transition is one-way from the app.
    pub(crate) window_fullscreen: bool,
    /// True for ~3 s after entering fullscreen so the "Press F11 to
    /// exit" banner renders. Cleared by a scheduled
    /// `Message::FullscreenHintHide`. Mirrors Chrome / Firefox where
    /// the on-enter hint fades on its own and the X close affordance
    /// then only shows on top-edge hover.
    pub(crate) fullscreen_hint_visible: bool,

    // Keys
    pub(crate) keys: Vec<SshKey>,
    pub(crate) show_key_panel: bool,
    pub(crate) key_import_label: String,
    pub(crate) key_import_content: text_editor::Content,
    pub(crate) key_import_pem: String,  // raw string for import
    /// Passphrase for an encrypted private key. Lives in memory only, once
    /// the key is decrypted on import, it is re-encoded unencrypted and the
    /// vault's master key takes over for at-rest protection.
    pub(crate) key_import_passphrase: String,
    /// Set when import_key returns `KeyNeedsPassphrase`. Drives the
    /// passphrase row in the import panel.
    pub(crate) key_import_passphrase_required: bool,
    pub(crate) key_import_passphrase_visible: bool,
    pub(crate) key_error: Option<String>,
    pub(crate) key_success: Option<String>,
    pub(crate) key_context_menu: Option<usize>,
    pub(crate) editing_key_id: Option<Uuid>,
    pub(crate) key_search: String,
    /// Workspace-mode contextual search backing for Snippets view.
    /// Matches against snippet label + command.
    pub(crate) snippet_search: String,
    /// Workspace-mode contextual search backing for History view.
    /// Matches against the connection label / hostname recorded in
    /// each log row.
    pub(crate) history_search: String,

    // Identities
    pub(crate) identities: Vec<Identity>,
    // Cached set of identity ids whose `password` column is non-NULL.
    // Hydrated by `load_data_from_vault`. The keychain view reads this
    // per card to decide whether to render the masked-bullets badge,
    // a per-frame `get_identity_password` decrypt would otherwise run
    // for every identity on every view() rebuild and slow the main
    // loop enough to fill iced's 100-slot subscription channel.
    pub(crate) identities_with_password: std::collections::HashSet<Uuid>,
    pub(crate) show_identity_panel: bool,
    pub(crate) identity_form_label: String,
    pub(crate) identity_form_username: String,
    pub(crate) identity_form_password: String,
    pub(crate) identity_form_key: Option<String>,
    pub(crate) identity_form_password_visible: bool,
    pub(crate) identity_form_password_touched: bool,
    pub(crate) identity_form_has_existing_password: bool,
    pub(crate) editing_identity_id: Option<Uuid>,
    pub(crate) identity_context_menu: Option<usize>,
    pub(crate) show_keychain_add_menu: bool,

    // Proxy Identities, reusable proxy configs edited inline inside
    // the Settings → Proxies section. Form state is in-memory only
    // until SaveProxyIdentity flushes to the vault.
    pub(crate) proxy_identities: Vec<oryxis_core::models::proxy_identity::ProxyIdentity>,
    pub(crate) proxy_identity_form_visible: bool,
    pub(crate) proxy_identity_form_label: String,
    pub(crate) proxy_identity_form_kind: crate::state::ProxyKind,
    pub(crate) proxy_identity_form_host: String,
    pub(crate) proxy_identity_form_port: String,
    pub(crate) proxy_identity_form_username: String,
    pub(crate) proxy_identity_form_password: String,
    pub(crate) proxy_identity_form_password_visible: bool,
    pub(crate) proxy_identity_form_password_touched: bool,
    pub(crate) proxy_identity_form_has_existing_password: bool,
    pub(crate) editing_proxy_identity_id: Option<Uuid>,
    pub(crate) proxy_identity_form_error: Option<String>,

    // Cloud Accounts, CloudProfile rows + the wizard form. Wizard is
    // intentionally minimal in v0.6 PR 3: provider + AWS profile auth
    // only. Access key + SSO + the discover-and-pick step land in
    // follow-up PRs once the foundation is exercised.
    pub(crate) cloud_profiles: Vec<oryxis_core::models::cloud_profile::CloudProfile>,
    pub(crate) cloud_form_visible: bool,
    pub(crate) cloud_form_label: String,
    pub(crate) cloud_form_provider: crate::state::CloudProviderChoice,
    pub(crate) cloud_form_auth_kind: crate::state::CloudAuthChoice,
    pub(crate) cloud_form_aws_profile_name: String,
    pub(crate) cloud_form_aws_region: String,
    /// Access Key auth fields. The secret access key follows the
    /// password-tri-state convention (`*_touched` differentiates
    /// "leave alone" from "explicitly cleared").
    pub(crate) cloud_form_aws_access_key_id: String,
    pub(crate) cloud_form_aws_access_key_secret: String,
    pub(crate) cloud_form_aws_access_key_secret_touched: bool,
    pub(crate) cloud_form_aws_access_key_secret_visible: bool,
    pub(crate) cloud_form_aws_access_key_session_token: String,
    pub(crate) cloud_form_aws_has_existing_secret: bool,
    /// SSO (IAM Identity Center) auth fields.
    pub(crate) cloud_form_aws_sso_start_url: String,
    pub(crate) cloud_form_aws_sso_region: String,
    pub(crate) cloud_form_aws_sso_account_id: String,
    pub(crate) cloud_form_aws_sso_role_name: String,
    pub(crate) editing_cloud_profile_id: Option<Uuid>,
    pub(crate) cloud_form_error: Option<String>,
    pub(crate) cloud_form_test_state: crate::state::CloudTestState,
    pub(crate) cloud_provider_registry: std::sync::Arc<oryxis_cloud::CloudProviderRegistry>,

    // Plugins panel, one row per cloud-provider plugin. Cloud
    // providers run as downloaded subprocess plugins; this is where
    // the user installs, updates, pins, and rolls them back.
    pub(crate) plugins: Vec<crate::state::PluginUiEntry>,
    /// Global default for plugin auto-update. Per-plugin overrides
    /// live on each `PluginUiEntry`.
    pub(crate) plugins_auto_update_global: bool,
    /// When `Some(provider_id)`, the first-use install opt-in modal
    /// is shown for that provider.
    pub(crate) plugin_install_modal: Option<String>,
    /// Discovery panel state, opened from a profile card or from the
    /// post-save flow. Carries the in-flight or completed result so
    /// the user picks resources without paying another API round-trip.
    pub(crate) cloud_discover_visible: bool,
    pub(crate) cloud_discover_profile_id: Option<Uuid>,
    pub(crate) cloud_discover_state: crate::state::CloudDiscoverState,
    /// EC2 instance-ids currently checked in the discovery panel.
    pub(crate) cloud_discover_selected_ec2: std::collections::HashSet<String>,
    /// ECS service identifiers checked in the discovery panel.
    /// Key format: `cluster/service/container` (the same triple a
    /// `CloudQuery::EcsTasks` carries), guarantees a stable id even
    /// when service or container names collide across clusters.
    pub(crate) cloud_discover_selected_ecs: std::collections::HashSet<String>,
    /// Live filter for the discovery panel, matches against label,
    /// instance-id, hostname, IP. Lowercased substring match.
    pub(crate) cloud_discover_filter: String,
    /// Section names currently collapsed in the discovery panel
    /// ("ec2" / "ecs" today; future K8s sections add their own keys).
    /// Persisted only in memory, re-opens default to expanded.
    pub(crate) cloud_discover_collapsed: std::collections::HashSet<String>,
    /// Default transport applied to every EC2 host imported in this
    /// discovery session. Lets the user pick "Instance Connect" once
    /// instead of editing 10 hosts after the fact. Stored at the
    /// `Oryxis` level (not on the `OverlayState`) so the choice
    /// survives discovery refreshes.
    pub(crate) cloud_discover_default_transport:
        oryxis_core::models::cloud::TransportKind,
    /// Modal that asks the user to pick the transport for the EC2
    /// hosts about to be imported. Only opened when there's at
    /// least one EC2 selected, pure-ECS imports skip straight to
    /// the import logic since dynamic groups always use ECS Exec.
    pub(crate) cloud_import_confirm_visible: bool,
    /// Per-dynamic-group resolve cache. Populated when the user opens
    /// the group (or hits Refresh inside it); reused on re-open until
    /// the user manually refreshes.
    pub(crate) cloud_dynamic_group_state:
        std::collections::HashMap<Uuid, crate::state::DynamicGroupState>,

    /// Edit-dynamic-group form. Opened from the ⋮ menu on a dynamic
    /// group card (root or nested). Edits the `cloud_query.template`
    /// fields: username, initial_command, transport, key, identity.
    pub(crate) cloud_dynamic_form_visible: bool,
    pub(crate) cloud_dynamic_form_group_id: Option<Uuid>,
    pub(crate) cloud_dynamic_form_username: String,
    pub(crate) cloud_dynamic_form_initial_command: String,
    pub(crate) cloud_dynamic_form_transport: oryxis_core::models::cloud::TransportKind,
    /// Selected key label (or `"(none)"`); resolved to a `key_id` on save.
    pub(crate) cloud_dynamic_form_selected_key: Option<String>,
    /// Selected identity label (or `"(none)"`); resolved to an `identity_id` on save.
    pub(crate) cloud_dynamic_form_selected_identity: Option<String>,

    /// Hover tracking for the kebab on dynamic-group cards (root + nested).
    pub(crate) hovered_dynamic_group_card: Option<Uuid>,

    /// Card-hover state for the kebab "..." button on cloud profile
    /// cards in Settings → Cloud, mirroring `hovered_card` /
    /// `hovered_folder_card` for hosts and folders.
    pub(crate) hovered_cloud_card: Option<Uuid>,

    // Snippets
    pub(crate) snippets: Vec<oryxis_core::models::snippet::Snippet>,
    pub(crate) show_snippet_panel: bool,
    pub(crate) snippet_label: String,
    pub(crate) snippet_command: String,
    pub(crate) snippet_editing_id: Option<Uuid>,
    pub(crate) snippet_error: Option<String>,

    // Known hosts & logs
    pub(crate) known_hosts: Vec<oryxis_core::models::known_host::KnownHost>,
    pub(crate) logs: Vec<oryxis_core::models::log_entry::LogEntry>,
    pub(crate) logs_page: usize,
    pub(crate) logs_total: usize,

    // Session logs (terminal recording)
    pub(crate) session_logs: Vec<oryxis_vault::SessionLogEntry>,
    pub(crate) session_logs_page: usize,
    pub(crate) session_logs_total: usize,
    pub(crate) viewing_session_log: Option<(Uuid, String)>, // (log_id, rendered_text)

    // Terminal theme
    /// Theme derived from the active app theme, used as the global
    /// fallback when neither `terminal_theme_override` nor a per-host
    /// override is set.
    pub(crate) terminal_theme: oryxis_terminal::TerminalTheme,
    /// User pick that overrides the app-theme-derived terminal palette.
    /// `None` means "follow the app theme" (default). Stored as the
    /// theme's display name (e.g. "Dracula") so the value survives new
    /// theme additions without a migration.
    pub(crate) terminal_theme_override: Option<String>,
    pub(crate) terminal_font_size: f32,
    pub(crate) terminal_font_name: String,

    // Settings
    pub(crate) settings_section: SettingsSection,
    pub(crate) setting_copy_on_select: bool,
    pub(crate) setting_bold_is_bright: bool,
    pub(crate) setting_keyword_highlight: bool,
    /// When the foreground and background of a cell render too close
    /// to each other (LS_COLORS' `ow` over a green palette,
    /// PowerShell's `$PSStyle.FileInfo.Directory` blue-on-blue, …),
    /// the renderer flips the foreground to a legible alternative.
    /// Off makes the renderer paint the cell exactly as the app
    /// asked, which some color-precise tools rely on.
    pub(crate) setting_smart_contrast: bool,
    /// Toggles the bottom status bar that shows current connection IP +
    /// Oryxis version. Off in `view_main` simply skips rendering it,
    /// reclaiming the row for the active content area.
    pub(crate) setting_show_status_bar: bool,
    /// `"left"` (default, Termius-style: X replaces the OS badge on
    /// hover/active) or `"right"` (badge stays left, X gets its own
    /// slot at the trailing edge of the tab). Anything else is treated
    /// as `"left"`.
    pub(crate) setting_tab_close_button_side: String,
    /// When on, each tab paints a small colored dot over its OS badge:
    /// green for an active SSH session, orange while connecting, red
    /// for a tab that lost its session. Defaults on; the user can hide
    /// it from Settings -> Interface.
    pub(crate) setting_show_tab_status_dot: bool,
    /// Toggles the SFTP feature entirely. Off hides the SFTP sidebar
    /// entry (both expanded and collapsed) so users who never transfer
    /// files don't have it taking up nav space. The SFTP settings panel
    /// still renders so the user can re-enable + tweak in one place,
    /// mirroring how `ai_enabled` works.
    pub(crate) sftp_enabled: bool,
    /// `"classic"` (current sidebar nav) or `"workspace"` (top tabs +
    /// contextual sidebar + burger, PR 6). Persisted ahead of the
    /// workspace mode landing so we can flip the default and migrate
    /// settings in a single later PR without touching boot logic again.
    pub(crate) setting_layout_mode: String,
    /// Default shape for host icons in the dashboard, sidebar tab
    /// badges and host cards: `"circular"` (default v0.7), `"square"`
    /// (legacy Termius-style), `"outline"`, or `"initials"`. Read by
    /// the host icon widget in PR 3; until then the value persists but
    /// the renderer keeps the current shape.
    pub(crate) setting_default_host_icon: String,
    pub(crate) setting_keepalive_interval: String,
    pub(crate) setting_scrollback_rows: String,
    /// Max parallel SFTP transfer slots (uploads/downloads). 1 = serial,
    /// up to 8 = aggressive. Each slot gets its own SFTP subsystem
    /// channel on the same SSH connection so they don't fight for the
    /// shared client mutex.
    pub(crate) setting_sftp_concurrency: String,
    /// TCP connect + SSH transport handshake timeout, in seconds.
    pub(crate) setting_sftp_connect_timeout: String,
    /// Authentication phase timeout, in seconds.
    pub(crate) setting_sftp_auth_timeout: String,
    /// Per-channel open timeout (PTY session, SFTP subsystem, sibling
    /// channels), in seconds.
    pub(crate) setting_sftp_session_timeout: String,
    /// Per-operation timeout for SFTP requests (list_dir, read, write,
    /// metadata). Caps the "Loading…" state so a hung server can't pin
    /// the UI forever.
    pub(crate) setting_sftp_op_timeout: String,
    pub(crate) setting_auto_reconnect: bool,
    pub(crate) setting_max_reconnect_attempts: String,
    pub(crate) setting_os_detection: bool,
    pub(crate) setting_auto_check_updates: bool,

    // Update state (set by the async GitHub check on boot)
    pub(crate) pending_update: Option<crate::update::UpdateInfo>,
    pub(crate) update_downloading: bool,
    pub(crate) update_progress: f32,
    pub(crate) update_error: Option<String>,
    /// Last manual-check outcome shown near the "Check now" button in
    /// settings. `Some("")` → in-flight; `Some("Up to date.")` → no newer
    /// release; `Some("Error: …")` on failure. `None` hides the line.
    pub(crate) update_check_status: Option<String>,
    /// Attempt counters keyed by connection UUID, persists across tab recreations.
    pub(crate) reconnect_counters: std::collections::HashMap<Uuid, u32>,

    // AI Chat settings
    pub(crate) ai_enabled: bool,
    pub(crate) ai_provider: String,
    pub(crate) ai_model: String,
    pub(crate) ai_api_key: String,
    pub(crate) ai_api_key_set: bool,
    pub(crate) ai_api_url: String,
    pub(crate) ai_system_prompt: text_editor::Content,

    // Vault password settings
    pub(crate) vault_has_user_password: bool,
    pub(crate) vault_new_password: String,
    pub(crate) vault_password_error: Option<String>,
    pub(crate) vault_destroy_confirm: bool,

    /// Transient bottom-of-chat status chip, currently used for the
    /// "Copied to clipboard" feedback after a Copy button click.
    /// `Some(text)` → render the chip; cleared after ~1.8 s by a
    /// `Task::perform`-spawned `ToastClear` round-trip.
    pub(crate) toast: Option<String>,

    /// Generic blocking error dialog. Use for cases the user must read
    /// (install instructions, fatal config errors) where a 1.8 s toast
    /// would vanish before they can act on it. `None` = no dialog.
    pub(crate) error_dialog: Option<crate::state::ErrorDialog>,

    /// Cached list of available local shells (PowerShell, cmd, WSL
    /// distros, etc.), populated lazily when the user opens the
    /// Local Shell picker so we don't pay the `wsl --list` spawn on
    /// every boot. `None` means not detected yet.
    pub(crate) local_shells: Option<Vec<crate::state::LocalShellSpec>>,
    /// True while the Local Shell picker overlay is showing. Only
    /// surfaces on Windows where there's a real choice between cmd /
    /// PowerShell / WSL distros, non-Windows just spawns the
    /// default shell directly.
    pub(crate) local_shell_picker_open: bool,

    // AI chat sidebar
    pub(crate) chat_input: text_editor::Content,
    pub(crate) chat_loading: bool,
    /// True when the user's scroll is anchored at (or very near) the bottom
    /// of the chat history, used to decide whether new assistant messages
    /// should auto-scroll. If the user has scrolled up to read older
    /// content, we leave them where they are.
    pub(crate) chat_scroll_at_bottom: bool,
    /// User-resizable width of the chat sidebar in pixels.
    pub(crate) chat_sidebar_width: f32,
    /// Some((cursor_x_at_drag_start, sidebar_width_at_drag_start)) while
    /// the user is dragging the resize handle on the sidebar's left edge.
    pub(crate) chat_sidebar_drag: Option<(f32, f32)>,

    // MCP Server
    pub(crate) mcp_server_enabled: bool,
    pub(crate) show_mcp_info: bool,
    pub(crate) mcp_config_copied: bool,
    pub(crate) mcp_install_status: Option<Result<String, String>>,
    /// Token MCP clients must present (via `ORYXIS_MCP_TOKEN` env)
    /// to talk to the server. Empty disables auth (backward-compat).
    pub(crate) mcp_server_token: String,
    /// When true, the token is rendered as plain text in the panel;
    /// otherwise as a row of bullets. Sensitive enough to keep masked
    /// by default, the user opts in to seeing it.
    pub(crate) mcp_token_visible: bool,

    // Sync
    pub(crate) sync_enabled: bool,
    pub(crate) sync_mode: String,
    /// When on, sync wraps connection / identity / proxy-identity
    /// payloads with their decrypted passwords so peers can mirror
    /// them. Off by default, passwords stay device-local until the
    /// user explicitly opts in via Settings → Sync.
    pub(crate) sync_passwords: bool,
    /// When on, the dashboard root shows two sections, Groups (manual
    /// folder cards) and Hosts (a flat list of every connection,
    /// including those that live inside a group). When off, root
    /// matches the legacy behaviour: groups at top, only ungrouped
    /// hosts beneath. Default: on.
    pub(crate) flatten_hosts: bool,
    pub(crate) sync_device_name: String,
    pub(crate) sync_signaling_url: String,
    /// Bearer token for the signaling endpoint. Sent on every
    /// `POST /register` / `GET /lookup`. Empty == not configured.
    pub(crate) sync_signaling_token: String,
    pub(crate) sync_relay_url: String,
    pub(crate) sync_listen_port: String,
    pub(crate) sync_peers: Vec<oryxis_vault::SyncPeerRow>,
    pub(crate) sync_pairing_code: Option<String>,
    pub(crate) sync_status: Option<String>,
    /// Live P2P sync engine, present only while sync is enabled. Holds
    /// a dedicated vault handle plus the QUIC / mDNS background tasks.
    pub(crate) sync_runtime: Option<crate::sync_runtime::SyncRuntime>,
    /// Mirrors `sync_runtime.is_some()` for cheap UI checks.
    pub(crate) sync_engine_running: bool,
    /// Which pairing sub-view the Sync settings panel shows.
    pub(crate) sync_pairing_state: crate::state::SyncPairingState,
    /// The 6-digit code typed in when joining another device's pairing.
    pub(crate) sync_join_code_input: String,
    /// The host address (`ip:port`) typed in when joining a pairing.
    pub(crate) sync_join_target_input: String,
    /// Shareable `oryxis://pair/...` link for the currently-hosted
    /// pairing code, cleared on cancel / complete.
    pub(crate) sync_pairing_link: Option<String>,
    /// `oryxis://pair/...` link pasted in by the joiner as an
    /// alternative to typing code + `ip:port`. Resolved via signaling.
    pub(crate) sync_join_link_input: String,
    /// Live mDNS-discovered peers on the LAN. Deduped by `device_id`.
    pub(crate) sync_discovered: Vec<crate::state::DiscoveredPeerInfo>,
    /// `Sync Now` in flight. Drives the Cancel button + suppresses
    /// re-clicks on Sync Now while a sync is already running.
    pub(crate) sync_in_progress: bool,
    /// One-shot abort channel for the in-flight `Sync Now` Task. The
    /// task races `sync_now().await` against this receiver, so
    /// `Cancel` immediately drops the QUIC connection.
    pub(crate) sync_abort_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Visible heartbeat counter for signaling re-registers. Bumps on
    /// every successful `SignalingRegistered` event so the user can
    /// confirm the heartbeat is alive (otherwise re-registers on the
    /// same IP look identical and the status freezes).
    pub(crate) sync_signaling_tick: u32,

    // Export/Import
    pub(crate) show_export_dialog: bool,
    pub(crate) export_password: String,
    pub(crate) export_include_keys: bool,
    pub(crate) export_status: Option<Result<String, String>>,
    pub(crate) show_import_dialog: bool,
    pub(crate) import_password: String,
    pub(crate) import_file_data: Option<Vec<u8>>,
    pub(crate) import_status: Option<Result<String, String>>,
    /// Latest result of an `~/.ssh/config` import, `Ok(message)` is
    /// rendered as a green banner, `Err` as red, in the Security
    /// section's import card.
    pub(crate) ssh_config_import_status: Option<Result<String, String>>,

    // Share
    pub(crate) show_share_dialog: bool,
    pub(crate) share_password: String,
    pub(crate) share_include_keys: bool,
    pub(crate) share_filter: Option<oryxis_vault::ExportFilter>,
    pub(crate) share_status: Option<Result<String, String>>,
}


// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

// `boot`, `load_data_from_vault`, `persist_setting` live in `crate::boot`.

impl Oryxis {
    /// Vertical offset (px) that toolbar dropdown anchors should use
    /// to land below the toolbar buttons on the dashboard, regardless
    /// of layout mode. Stack of contributions, top to bottom:
    /// tab bar (40) + hairline (2) + sub-nav (~36, Workspace vault
    /// only) + toolbar top (20) + button + gap (32) = ~94 (Classic)
    /// or ~130 (Workspace vault).
    ///
    /// The previous hardcoded 56 lined up against an older toolbar
    /// geometry; with the v0.7 sub-nav the menus were dropping over
    /// the trigger button. New values measured against the current
    /// toolbar and verified by user feedback.
    pub(crate) fn dashboard_dropdown_anchor_y(&self) -> f32 {
        use crate::state::View;
        // Toolbar geometry (top to bottom):
        //   tab_bar(40) + hairline(2) + toolbar_top_pad(20)
        //   + button(24) + gap(4) = 90
        // Add sub-nav (~40) on top when Workspace+vault renders it.
        // Classic was getting the Workspace value before, so the menu
        // hung well below the trigger button.
        const BASE_Y: f32 = 90.0;
        const SUBNAV_HEIGHT: f32 = 40.0;
        let in_workspace_vault = self.setting_layout_mode == "workspace"
            && self.active_tab.is_none()
            && matches!(
                self.active_view,
                View::Dashboard | View::Keys | View::Snippets | View::History
            );
        if in_workspace_vault { BASE_Y + SUBNAV_HEIGHT } else { BASE_Y }
    }

    pub(crate) fn snippet_injection_tab(&self) -> Option<usize> {
        let idx = self.active_tab.or(self.last_terminal_tab)?;
        (idx < self.tabs.len()).then_some(idx)
    }

    pub(crate) fn remember_terminal_tab_focus(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.last_terminal_tab = Some(idx);
        }
    }

    pub(crate) fn adjust_last_terminal_tab_after_remove(&mut self, removed_idx: usize) {
        if self.tabs.is_empty() {
            self.last_terminal_tab = None;
            return;
        }
        match self.last_terminal_tab {
            Some(l) if l == removed_idx => {
                self.last_terminal_tab = Some(removed_idx.min(self.tabs.len() - 1));
            }
            Some(l) if l > removed_idx => {
                self.last_terminal_tab = Some(l - 1);
            }
            _ => {}
        }
    }

    pub(crate) fn clear_terminal_tab_memory(&mut self) {
        self.last_terminal_tab = None;
    }

    pub fn title(&self) -> String {
        "Oryxis".into()
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            String::from("Oryxis Dark"),
            iced::theme::palette::Seed {
                background: OryxisColors::t().bg_primary,
                text: OryxisColors::t().text_primary,
                primary: OryxisColors::t().accent,
                success: OryxisColors::t().success,
                warning: OryxisColors::t().warning,
                danger: OryxisColors::t().error,
            },
        )
    }


}

// `update`, `boot`, `subscription`, `view`, and the connect / SFTP
// helpers each live in their own sibling module. This file now only
// holds the struct definition, the `Message` re-export, layout
// constants, and the trivial `title` / `theme` accessors.

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
