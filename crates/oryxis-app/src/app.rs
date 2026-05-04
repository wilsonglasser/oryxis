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

/// Inherited vault master password — populated by `main.rs` when the
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

/// Monospace fonts offered in the terminal font picker.
///
/// `Source Code Pro` is bundled with the binary (see `main.rs`). The rest are
/// looked up from the OS fontconfig — if not installed, cosmic-text falls back
/// gracefully to the system default monospace.
pub(crate) const TERMINAL_FONTS: &[&str] = &[
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

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct Oryxis {
    // Vault
    pub(crate) vault: Option<VaultStore>,
    pub(crate) vault_state: VaultState,
    pub(crate) vault_password_input: String,
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
    pub(crate) hovered_tab: Option<usize>,
    pub(crate) show_new_tab_picker: bool,
    pub(crate) new_tab_picker_search: String,
    /// Termius-style "Jump to" modal — lists all open tabs (plus Quick
    /// connect entries) for direct navigation when the bar runs out of
    /// horizontal room. Triggered by the `⋯` button in the tab bar or
    /// Ctrl+J anywhere.
    pub(crate) show_tab_jump: bool,
    pub(crate) tab_jump_search: String,

    // Icon/color picker (from the host editor's icon box).
    pub(crate) show_icon_picker: bool,
    pub(crate) icon_picker_for: Option<Uuid>,
    pub(crate) icon_picker_icon: Option<String>,
    pub(crate) icon_picker_color: Option<String>,
    pub(crate) icon_picker_hex_input: String,
    pub(crate) connecting: Option<ConnectionProgress>,
    /// Counter that advances ~every 100ms while a connection is in progress.
    /// Used only to drive the pulsing "loading" ring on the active step dot.
    pub(crate) connect_anim_tick: u32,
    /// Timestamp of the last `WindowDrag` / `WindowResizeDrag` we
    /// forwarded to the OS. iced's `MouseArea` fires `on_press` on
    /// **both** clicks of a double-click (before the `on_double_click`
    /// lands), and forwarding two `iced::window::drag(...)` calls in
    /// quick succession leaves the OS in a flaky state — Windows races
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
    pub(crate) card_context_menu: Option<usize>,

    // Floating overlay menu
    pub(crate) overlay: Option<OverlayState>,
    /// Folder rename inline editor — `Some((group_id, current_input))`
    /// while the modal is open.
    pub(crate) folder_rename: Option<(Uuid, String)>,
    /// Folder delete confirmation — group ID waiting for the user to
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
    /// selection — iced's MouseArea events don't include modifiers.
    pub(crate) modifiers: keyboard::Modifiers,
    /// Whether the OS window is currently maximized. Used by the custom
    /// chrome to swap the maximize glyph for a "restore" glyph. Toggled
    /// optimistically on `WindowMaximizeToggle` since our chrome is the only
    /// path that can change this state (native titlebar is disabled).
    pub(crate) window_maximized: bool,

    // Keys
    pub(crate) keys: Vec<SshKey>,
    pub(crate) show_key_panel: bool,
    pub(crate) key_import_label: String,
    pub(crate) key_import_content: text_editor::Content,
    pub(crate) key_import_pem: String,  // raw string for import
    pub(crate) key_error: Option<String>,
    pub(crate) key_success: Option<String>,
    pub(crate) key_context_menu: Option<usize>,
    pub(crate) editing_key_id: Option<Uuid>,
    pub(crate) key_search: String,

    // Identities
    pub(crate) identities: Vec<Identity>,
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

    // Proxy Identities — reusable proxy configs edited inline inside
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
    pub(crate) viewing_session_log: Option<(Uuid, String)>, // (log_id, rendered_text)

    // Terminal theme
    pub(crate) terminal_theme: oryxis_terminal::TerminalTheme,
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
    /// Attempt counters keyed by connection UUID — persists across tab recreations.
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

    /// Transient bottom-of-chat status chip — currently used for the
    /// "Copied to clipboard" feedback after a Copy button click.
    /// `Some(text)` → render the chip; cleared after ~1.8 s by a
    /// `Task::perform`-spawned `ToastClear` round-trip.
    pub(crate) toast: Option<String>,

    /// Cached list of available local shells (PowerShell, cmd, WSL
    /// distros, etc.) — populated lazily when the user opens the
    /// Local Shell picker so we don't pay the `wsl --list` spawn on
    /// every boot. `None` means not detected yet.
    pub(crate) local_shells: Option<Vec<crate::state::LocalShellSpec>>,
    /// True while the Local Shell picker overlay is showing. Only
    /// surfaces on Windows where there's a real choice between cmd /
    /// PowerShell / WSL distros — non-Windows just spawns the
    /// default shell directly.
    pub(crate) local_shell_picker_open: bool,

    // AI chat sidebar
    pub(crate) chat_input: text_editor::Content,
    pub(crate) chat_loading: bool,
    /// True when the user's scroll is anchored at (or very near) the bottom
    /// of the chat history — used to decide whether new assistant messages
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

    // Sync
    pub(crate) sync_enabled: bool,
    pub(crate) sync_mode: String,
    /// When on, sync wraps connection / identity / proxy-identity
    /// payloads with their decrypted passwords so peers can mirror
    /// them. Off by default — passwords stay device-local until the
    /// user explicitly opts in via Settings → Sync.
    pub(crate) sync_passwords: bool,
    pub(crate) sync_device_name: String,
    pub(crate) sync_signaling_url: String,
    pub(crate) sync_relay_url: String,
    pub(crate) sync_listen_port: String,
    pub(crate) sync_peers: Vec<oryxis_vault::SyncPeerRow>,
    pub(crate) sync_pairing_code: Option<String>,
    pub(crate) sync_status: Option<String>,

    // Export/Import
    pub(crate) show_export_dialog: bool,
    pub(crate) export_password: String,
    pub(crate) export_include_keys: bool,
    pub(crate) export_status: Option<Result<String, String>>,
    pub(crate) show_import_dialog: bool,
    pub(crate) import_password: String,
    pub(crate) import_file_data: Option<Vec<u8>>,
    pub(crate) import_status: Option<Result<String, String>>,
    /// Latest result of an `~/.ssh/config` import — `Ok(message)` is
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
