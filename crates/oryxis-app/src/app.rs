use iced::keyboard;
use iced::widget::{
    image, text_editor,
};
use iced::futures::SinkExt;
use iced::{Element, Point, Subscription, Task, Theme};

use oryxis_core::models::connection::{AuthMethod, Connection};
use oryxis_core::models::group::Group;
use oryxis_core::models::identity::Identity;
use oryxis_core::models::key::SshKey;
use oryxis_ssh::{SshEngine, SshSession};
use oryxis_terminal::widget::TerminalState;
use oryxis_vault::{VaultError, VaultStore};

use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use crate::mcp::{install_mcp_config_to_file, mcp_config_json};
use crate::state::{
    ChatMessage, ChatRole, ConnectionForm, ConnectionProgress, ConnectionStep, OverlayContent,
    OverlayState, PortForwardForm, SettingsSection, SshStreamMsg, TerminalTab, VaultState, View,
};
use crate::theme::OryxisColors;
use crate::util::{ctrl_key_bytes, key_to_named_bytes, strip_ansi};

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
    pub(crate) mouse_position: Point,
    pub(crate) window_size: iced::Size,
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
    pub(crate) setting_bell_sound: bool,
    pub(crate) setting_keyword_highlight: bool,
    pub(crate) setting_keepalive_interval: String,
    pub(crate) setting_scrollback_rows: String,
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
    pub(crate) ai_system_prompt: String,

    // Vault password settings
    pub(crate) vault_has_user_password: bool,
    pub(crate) vault_new_password: String,
    pub(crate) vault_password_error: Option<String>,
    pub(crate) vault_destroy_confirm: bool,

    // AI chat sidebar
    pub(crate) chat_input: String,
    pub(crate) chat_loading: bool,

    // MCP Server
    pub(crate) mcp_server_enabled: bool,
    pub(crate) show_mcp_info: bool,
    pub(crate) mcp_config_copied: bool,
    pub(crate) mcp_install_status: Option<Result<String, String>>,

    // Sync
    pub(crate) sync_enabled: bool,
    pub(crate) sync_mode: String,
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

    // Share
    pub(crate) show_share_dialog: bool,
    pub(crate) share_password: String,
    pub(crate) share_include_keys: bool,
    pub(crate) share_filter: Option<oryxis_vault::ExportFilter>,
    pub(crate) share_status: Option<Result<String, String>>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    // Vault
    VaultPasswordChanged(String),
    VaultUnlock,
    VaultSetup,
    VaultSkipPassword,
    VaultDestroyConfirm,
    VaultDestroy,

    // Navigation
    ChangeView(View),
    QuickHostInput(String),
    QuickHostContinue,
    OpenGroup(Uuid),
    BackToRoot,
    HostSearchChanged(String),
    ToggleSidebar,

    // Tabs
    SelectTab(usize),
    CloseTab(usize),
    TabHovered(usize),
    TabUnhovered,
    ShowNewTabPicker,
    HideNewTabPicker,
    NewTabPickerSearchChanged(String),

    // Icon picker (custom host icon/color)
    ShowIconPicker(Uuid),
    HideIconPicker,
    IconPickerSelectIcon(String),
    IconPickerSelectColor(String),
    IconPickerHexInputChanged(String),
    IconPickerSave,
    IconPickerResetAuto,
    ShowTabMenu(usize),
    ReconnectTab(usize),
    CloseOtherTabs(usize),
    CloseAllTabs,

    // Terminal I/O
    PtyOutput(usize, Vec<u8>),  // (tab_index, bytes)
    KeyboardEvent(keyboard::Event),
    MouseMoved(Point),
    WindowResized(iced::Size),
    WindowDrag,
    WindowMinimize,
    WindowMaximizeToggle,
    WindowClose,

    // Overlay
    HideOverlayMenu,

    // Card interactions
    CardHovered(usize),
    CardUnhovered,
    ShowCardMenu(usize),
    #[allow(dead_code)]
    HideCardMenu,

    // Connection editor
    ShowNewConnection,
    EditConnection(usize),
    EditorLabelChanged(String),
    EditorHostnameChanged(String),
    EditorPortChanged(String),
    EditorUsernameChanged(String),
    EditorPasswordChanged(String),
    EditorAuthMethodChanged(String),
    EditorGroupChanged(String),
    EditorKeyChanged(String),
    EditorJumpHostChanged(String),
    EditorTogglePasswordVisibility,
    EditorSave,
    EditorCancel,
    DeleteConnection(usize),
    DuplicateConnection(usize),

    // SSH
    ConnectSsh(usize),
    SshProgress(ConnectionStep, String),
    SshConnected(usize, Arc<SshSession>),
    SshNewKnownHosts(Vec<oryxis_core::models::known_host::KnownHost>),
    SshDisconnected(usize),
    SshError(String),
    SshHostKeyVerify(oryxis_ssh::HostKeyQuery),
    SshHostKeyReject,
    SshHostKeyContinue,
    SshHostKeyAcceptAndSave,
    SshCloseProgress,
    SshEditFromProgress,
    SshRetry,

    // Snippets
    ShowSnippetPanel,
    HideSnippetPanel,
    SnippetLabelChanged(String),
    SnippetCommandChanged(String),
    SaveSnippet,
    EditSnippet(usize),
    DeleteSnippet(usize),
    RunSnippet(usize),

    // Known hosts
    DeleteKnownHost(usize),
    ClearAllKnownHosts,

    // History
    ClearLogs,
    LogsPageNext,
    LogsPagePrev,

    // Session logs
    ViewSessionLog(Uuid),
    CloseSessionLogView,
    DeleteSessionLog(usize),

    // Settings
    LockVault,
    #[allow(dead_code)]
    TerminalThemeChanged(String),
    AppThemeChanged(String),
    TerminalFontSizeIncrease,
    TerminalFontSizeDecrease,
    TerminalFontChanged(String),
    ChangeSettingsSection(SettingsSection),
    ToggleCopyOnSelect,
    ToggleBoldIsBright,
    ToggleBellSound,
    ToggleKeywordHighlight,
    SettingKeepaliveChanged(String),
    SettingScrollbackChanged(String),
    SettingToggleAutoReconnect,
    SettingMaxReconnectChanged(String),
    SettingToggleOsDetection,
    OsDetected(Uuid, Option<String>),
    SettingToggleAutoCheckUpdates,

    // Auto-update
    CheckForUpdate,
    CheckForUpdateManual,
    UpdateCheckResult(Option<crate::update::UpdateInfo>),
    UpdateSkipVersion,
    UpdateLater,
    UpdateStartDownload,
    UpdateDownloadProgress(f32),
    UpdateDownloadComplete(Result<std::path::PathBuf, String>),
    UpdateOpenRelease,
    AutoReconnectTick,
    ConnectAnimTick,

    // Language
    LanguageChanged(String),

    // Local shell
    OpenLocalShell,

    // Keys
    ShowKeyPanel,
    HideKeyPanel,
    KeyImportLabelChanged(String),
    KeyContentAction(text_editor::Action),
    BrowseKeyFile,
    KeyFileLoaded(String, String), // (filename, content)
    KeyFileBrowseError(String),
    ImportKey,
    DeleteKey(usize),
    ShowKeyMenu(usize),
    #[allow(dead_code)]
    HideKeyMenu,
    EditKey(usize),
    KeySearchChanged(String),

    // Identities
    ShowIdentityPanel,
    HideIdentityPanel,
    IdentityLabelChanged(String),
    IdentityUsernameChanged(String),
    IdentityPasswordChanged(String),
    IdentityKeyChanged(String),
    IdentityTogglePasswordVisibility,
    SaveIdentity,
    EditIdentity(usize),
    DeleteIdentity(usize),
    ShowIdentityMenu(usize),
    ToggleKeychainAddMenu,

    // Connection identity
    EditorIdentityChanged(String),

    // AI settings
    ToggleAiEnabled,
    AiProviderChanged(String),
    AiModelChanged(String),
    AiApiKeyChanged(String),
    AiApiUrlChanged(String),
    AiSystemPromptChanged(String),
    SaveAiApiKey,

    // Vault password management
    ToggleVaultPassword,
    VaultNewPasswordChanged(String),
    SetVaultPassword,

    // AI chat sidebar
    ToggleChatSidebar,
    ChatInputChanged(String),
    SendChat,
    ChatResponse(String),
    ChatToolExec(String),
    #[allow(dead_code)]
    ChatToolResult(String),

    // Port forwarding
    EditorAddPortForward,
    EditorRemovePortForward(usize),
    EditorPortFwdLocalPortChanged(usize, String),
    EditorPortFwdRemoteHostChanged(usize, String),
    EditorPortFwdRemotePortChanged(usize, String),

    // MCP
    EditorToggleMcpEnabled,
    ToggleMcpServer,
    ShowMcpInfo,
    HideMcpInfo,
    CopyMcpConfig,
    InstallMcpConfig,
    InstallMcpConfigResult(Result<String, String>),

    // Sync
    SyncToggleEnabled,
    SyncModeChanged(String),
    SyncDeviceNameChanged(String),
    SyncSignalingUrlChanged(String),
    SyncRelayUrlChanged(String),
    SyncListenPortChanged(String),
    SyncStartPairing,
    SyncUnpairDevice(uuid::Uuid),
    SyncNow,

    // Export / Import
    ExportVault,
    ExportPasswordChanged(String),
    ExportToggleKeys,
    ExportConfirm,
    #[allow(dead_code)]
    ExportCompleted(Result<String, String>),
    ImportVault,
    #[allow(dead_code)]
    ImportFileLoaded(Vec<u8>),
    ImportPasswordChanged(String),
    ImportConfirm,
    #[allow(dead_code)]
    ImportCompleted(Result<String, String>),
    ExportImportDismiss,

    // Share
    ShareConnection(usize),
    #[allow(dead_code)]
    ShareGroup(uuid::Uuid),
    SharePasswordChanged(String),
    ShareToggleKeys,
    ShareConfirm,
    ShareDismiss,
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

impl Oryxis {
    pub fn boot() -> (Self, Task<Message>) {
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
                        // Has a real password — show unlock screen
                        vault_state = VaultState::Locked;
                        vault_has_user_password = true;
                    }
                }
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
                show_icon_picker: false,
                icon_picker_for: None,
                icon_picker_icon: None,
                icon_picker_color: None,
                icon_picker_hex_input: String::new(),
                connecting: None,
                connect_anim_tick: 0,
                pending_host_key: None,
                host_key_response_tx: None,
                show_host_panel: false,
                editor_form: ConnectionForm::default(),
                host_panel_error: None,
                hovered_card: None,
                card_context_menu: None,
                overlay: None,
                mouse_position: Point::ORIGIN,
                window_size: iced::Size::new(1024.0, 768.0),
                window_maximized: false,
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
                setting_bell_sound: false,
                setting_keyword_highlight: true,
                setting_keepalive_interval: "0".into(),
                setting_scrollback_rows: "10000".into(),
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
                ai_system_prompt: String::new(),
                vault_has_user_password,
                vault_new_password: String::new(),
                vault_password_error: None,
                vault_destroy_confirm: false,
                chat_input: String::new(),
                chat_loading: false,
                mcp_server_enabled: false,
                show_mcp_info: false,
                mcp_config_copied: false,
                mcp_install_status: None,
                sync_enabled: false,
                sync_mode: "manual".into(),
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

        // Kick off an update check in the background. The result flows back
        // via Message::UpdateCheckResult → optionally sets `pending_update`
        // which the main view turns into a modal.
        let boot_task = Task::batch([task, Task::done(Message::CheckForUpdate)]);
        (app, boot_task)
    }

    fn load_data_from_vault(&mut self) {
        if let Some(vault) = &self.vault {
            self.connections = vault.list_connections().unwrap_or_default();
            self.groups = vault.list_groups().unwrap_or_default();
            self.keys = vault.list_keys().unwrap_or_default();
            self.identities = vault.list_identities().unwrap_or_default();
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
                self.ai_system_prompt = v;
            }
        }
    }

    pub fn title(&self) -> String {
        "Oryxis".into()
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            String::from("Oryxis Dark"),
            iced::theme::Palette {
                background: OryxisColors::t().bg_primary,
                text: OryxisColors::t().text_primary,
                primary: OryxisColors::t().accent,
                success: OryxisColors::t().success,
                warning: OryxisColors::t().warning,
                danger: OryxisColors::t().error,
            },
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // -- Vault --
            Message::VaultPasswordChanged(pw) => {
                self.vault_password_input = pw;
            }
            Message::VaultSetup => {
                if self.vault_password_input.len() < 4 {
                    self.vault_error = Some("Password must be at least 4 characters".into());
                    return Task::none();
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_master_password(&self.vault_password_input) {
                        Ok(()) => {
                            let _ = vault.set_setting("has_user_password", "1");
                            self.vault_has_user_password = true;
                            self.vault_state = VaultState::Unlocked;
                            self.vault_error = None;
                            self.vault_password_input.clear();
                            self.load_data_from_vault();
                        }
                        Err(e) => {
                            self.vault_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::VaultSkipPassword => {
                if let Some(vault) = &mut self.vault {
                    match vault.open_without_password() {
                        Ok(()) => {
                            self.vault_state = VaultState::Unlocked;
                            self.vault_error = None;
                            self.load_data_from_vault();
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_error = Some(
                                "This vault already has a password. Enter it above to unlock.".into()
                            );
                        }
                        Err(e) => {
                            self.vault_error = Some(format!("Failed to create vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultDestroyConfirm => {
                self.vault_destroy_confirm = !self.vault_destroy_confirm;
            }
            Message::VaultDestroy => {
                if let Some(vault) = &mut self.vault {
                    match vault.destroy_and_recreate() {
                        Ok(()) => {
                            self.vault_state = VaultState::NeedSetup;
                            self.vault_error = None;
                            self.vault_destroy_confirm = false;
                            self.vault_password_input.clear();
                        }
                        Err(e) => {
                            self.vault_error = Some(format!("Failed to reset vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultUnlock => {
                if let Some(vault) = &mut self.vault {
                    match vault.unlock(&self.vault_password_input) {
                        Ok(()) => {
                            self.vault_state = VaultState::Unlocked;
                            self.vault_error = None;
                            self.vault_password_input.clear();
                            self.load_data_from_vault();
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_error = Some("Invalid password".into());
                        }
                        Err(e) => {
                            self.vault_error = Some(e.to_string());
                        }
                    }
                }
            }

            // -- Navigation --
            Message::ChangeView(view) => {
                self.active_view = view;
                self.active_tab = None;
            }
            Message::QuickHostInput(v) => {
                self.quick_host_input = v;
            }
            Message::OpenGroup(gid) => {
                self.active_group = Some(gid);
                self.host_search.clear();
            }
            Message::BackToRoot => {
                self.active_group = None;
                self.host_search.clear();
            }
            Message::HostSearchChanged(v) => {
                self.host_search = v;
            }
            Message::ToggleSidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
            }
            Message::QuickHostContinue => {
                if !self.quick_host_input.is_empty() {
                    self.editor_form = ConnectionForm::default();
                    self.editor_form.hostname = self.quick_host_input.clone();
                    self.show_host_panel = true;
                    self.host_panel_error = None;
                }
            }

            // -- Card interactions --
            Message::CardHovered(idx) => {
                self.hovered_card = Some(idx);
            }
            Message::CardUnhovered => {
                self.hovered_card = None;
            }
            Message::MouseMoved(pos) => {
                self.mouse_position = pos;
            }
            Message::WindowResized(size) => {
                self.window_size = size;
            }
            Message::WindowDrag => {
                return iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::drag(id),
                    None => Task::none(),
                });
            }
            Message::WindowMinimize => {
                return iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::minimize(id, true),
                    None => Task::none(),
                });
            }
            Message::WindowMaximizeToggle => {
                self.window_maximized = !self.window_maximized;
                return iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::toggle_maximize(id),
                    None => Task::none(),
                });
            }
            Message::WindowClose => {
                return iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::close(id),
                    None => Task::none(),
                });
            }
            Message::HideOverlayMenu => {
                self.overlay = None;
                self.card_context_menu = None;
                self.key_context_menu = None;
                self.identity_context_menu = None;
                self.show_keychain_add_menu = false;
            }
            Message::ShowCardMenu(idx) => {
                if self.card_context_menu == Some(idx) {
                    self.card_context_menu = None;
                    self.overlay = None;
                } else {
                    self.card_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::HostActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::HideCardMenu => {
                self.card_context_menu = None;
                self.overlay = None;
            }

            // -- Tabs --
            Message::SelectTab(idx) => {
                if idx < self.tabs.len() {
                    self.active_tab = Some(idx);
                    self.active_view = View::Terminal;
                }
            }
            Message::TabHovered(idx) => {
                self.hovered_tab = Some(idx);
            }
            Message::TabUnhovered => {
                self.hovered_tab = None;
            }
            Message::ShowNewTabPicker => {
                self.show_new_tab_picker = true;
                self.new_tab_picker_search.clear();
            }
            Message::HideNewTabPicker => {
                self.show_new_tab_picker = false;
            }
            Message::NewTabPickerSearchChanged(v) => {
                self.new_tab_picker_search = v;
            }
            Message::ShowIconPicker(conn_id) => {
                // Pre-fill the picker with whatever the connection currently
                // has (custom > detected). The user either confirms, edits,
                // or clicks "Reset to auto" to drop the override entirely.
                if let Some(conn) = self.connections.iter().find(|c| c.id == conn_id) {
                    self.icon_picker_icon = conn
                        .custom_icon
                        .clone()
                        .or_else(|| Some("server".to_string()));
                    self.icon_picker_color = conn.custom_color.clone();
                    self.icon_picker_hex_input = conn.custom_color.clone().unwrap_or_default();
                }
                self.icon_picker_for = Some(conn_id);
                self.show_icon_picker = true;
            }
            Message::HideIconPicker => {
                self.show_icon_picker = false;
                self.icon_picker_for = None;
            }
            Message::IconPickerSelectIcon(name) => {
                self.icon_picker_icon = Some(name);
            }
            Message::IconPickerSelectColor(hex) => {
                self.icon_picker_hex_input = hex.clone();
                self.icon_picker_color = Some(hex);
            }
            Message::IconPickerHexInputChanged(v) => {
                self.icon_picker_hex_input = v.clone();
                // Validate + commit only on well-formed #RRGGBB.
                let trimmed = v.trim().trim_start_matches('#');
                if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    self.icon_picker_color = Some(format!("#{}", trimmed.to_uppercase()));
                }
            }
            Message::IconPickerSave => {
                if let Some(conn_id) = self.icon_picker_for {
                    let icon = self.icon_picker_icon.clone();
                    let color = self.icon_picker_color.clone();
                    if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                        conn.custom_icon = icon.clone();
                        conn.custom_color = color.clone();
                        // Full save so the row persists (and other fields
                        // aren't accidentally overwritten).
                        if let Some(vault) = &self.vault {
                            let _ = vault.save_connection(conn, None);
                        }
                    }
                }
                self.show_icon_picker = false;
                self.icon_picker_for = None;
            }
            Message::IconPickerResetAuto => {
                // Clears the override, letting the OS-detection result (if any)
                // drive the icon again. Does not trigger re-detection — that
                // happens on the next connect if the OS is still unknown.
                if let Some(conn_id) = self.icon_picker_for
                    && let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                    conn.custom_icon = None;
                    conn.custom_color = None;
                    if let Some(vault) = &self.vault {
                        let _ = vault.save_connection(conn, None);
                    }
                }
                self.show_icon_picker = false;
                self.icon_picker_for = None;
            }
            Message::CloseTab(idx) => {
                // Also dismiss any open context menu so the menu doesn't linger
                // after the user clicks Close from it.
                self.overlay = None;
                if idx < self.tabs.len() {
                    self.tabs.remove(idx);
                    if self.tabs.is_empty() {
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        self.active_tab = Some(idx.min(self.tabs.len() - 1));
                    }
                }
            }
            Message::ShowTabMenu(idx) => {
                self.overlay = Some(OverlayState {
                    content: OverlayContent::TabActions(idx),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::ReconnectTab(idx) => {
                self.overlay = None;
                // Find the connection matching this tab's label; close the tab and
                // dispatch ConnectSsh for that connection index. Dead tabs (no matching
                // connection) are just closed.
                if let Some(tab) = self.tabs.get(idx) {
                    let base_label = tab.label.trim_end_matches(" (disconnected)").to_string();
                    let conn_idx = self.connections.iter().position(|c| c.label == base_label);
                    self.tabs.remove(idx);
                    if self.tabs.is_empty() {
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        self.active_tab = Some(idx.min(self.tabs.len() - 1));
                    }
                    if let Some(ci) = conn_idx {
                        return Task::done(Message::ConnectSsh(ci));
                    }
                }
            }
            Message::CloseOtherTabs(idx) => {
                self.overlay = None;
                if idx < self.tabs.len() {
                    let keep = self.tabs.remove(idx);
                    self.tabs.clear();
                    self.tabs.push(keep);
                    self.active_tab = Some(0);
                }
            }
            Message::CloseAllTabs => {
                self.overlay = None;
                self.tabs.clear();
                self.active_tab = None;
                self.active_view = View::Dashboard;
            }

            // -- Terminal I/O --
            Message::PtyOutput(tab_idx, bytes) => {
                if let Some(tab) = self.tabs.get(tab_idx) {
                    if let Ok(mut state) = tab.terminal.lock() {
                        state.process(&bytes);
                    }
                    // Append to session log for terminal recording
                    if let Some(log_id) = tab.session_log_id
                        && let Some(vault) = &self.vault {
                            let _ = vault.append_session_data(&log_id, &bytes);
                    }
                }
            }
            Message::KeyboardEvent(event) => {
                // Global shortcut: Ctrl+K opens the new-tab picker regardless
                // of which screen or tab is active. Handled before the
                // tab-specific routing so it works on Dashboard / Settings /
                // inside a terminal alike.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("k")
                {
                    self.show_new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    return Task::none();
                }
                if let Some(tab_idx) = self.active_tab
                    && self.connecting.is_none()
                    && let keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        text: text_opt,
                        ..
                    } = event
                    {
                        // Ctrl+V → paste from clipboard (not raw Ctrl+V byte)
                        if modifiers.control() && !modifiers.shift() {
                            if let keyboard::Key::Character(ref c) = key {
                                if c.as_str().eq_ignore_ascii_case("v") {
                                    if let Ok(mut clip) = arboard::Clipboard::new()
                                        && let Ok(text) = clip.get_text()
                                        && let Some(tab) = self.tabs.get(tab_idx)
                                    {
                                        if let Some(ref ssh) = tab.ssh_session {
                                            let _ = ssh.write(text.as_bytes());
                                        } else if let Ok(mut state) = tab.terminal.lock() {
                                            state.write(text.as_bytes());
                                        }
                                    }
                                    // Don't fall through to the normal key handler
                                } else if c.as_str().eq_ignore_ascii_case("c") {
                                    // Ctrl+C → send interrupt (byte 3)
                                    if let Some(tab) = self.tabs.get(tab_idx) {
                                        if let Some(ref ssh) = tab.ssh_session {
                                            let _ = ssh.write(&[3]);
                                        } else if let Ok(mut state) = tab.terminal.lock() {
                                            state.write(&[3]);
                                        }
                                    }
                                } else if let Some(bytes) = ctrl_key_bytes(&key) {
                                    // Other Ctrl+key combinations
                                    if let Some(tab) = self.tabs.get(tab_idx) {
                                        if let Some(ref ssh) = tab.ssh_session {
                                            let _ = ssh.write(&bytes);
                                        } else if let Ok(mut state) = tab.terminal.lock() {
                                            state.write(&bytes);
                                        }
                                    }
                                }
                            } else if let Some(bytes) = key_to_named_bytes(&key, &modifiers) {
                                // Ctrl + named key (e.g. Ctrl+Home)
                                if let Some(tab) = self.tabs.get(tab_idx) {
                                    if let Some(ref ssh) = tab.ssh_session {
                                        let _ = ssh.write(&bytes);
                                    } else if let Ok(mut state) = tab.terminal.lock() {
                                        state.write(&bytes);
                                    }
                                }
                            }
                        } else if modifiers.shift() && modifiers.control() {
                            // Ctrl+Shift+V → paste from clipboard into SSH or local PTY.
                            // Copy (Ctrl+Shift+C) stays in the terminal widget since it
                            // owns the selection state.
                            if let keyboard::Key::Character(ref c) = key
                                && c.as_str().eq_ignore_ascii_case("v")
                                && let Ok(mut clip) = arboard::Clipboard::new()
                                && let Ok(text) = clip.get_text()
                                && let Some(tab) = self.tabs.get(tab_idx)
                            {
                                if let Some(ref ssh) = tab.ssh_session {
                                    let _ = ssh.write(text.as_bytes());
                                } else if let Ok(mut state) = tab.terminal.lock() {
                                    state.write(text.as_bytes());
                                }
                            }
                        } else {
                            // Normal keys (no Ctrl)
                            let bytes = key_to_named_bytes(&key, &modifiers).or_else(|| {
                                text_opt.map(|t| t.as_bytes().to_vec())
                            });

                            if let Some(bytes) = bytes
                                && !bytes.is_empty()
                                && let Some(tab) = self.tabs.get(tab_idx)
                            {
                                if let Some(ref ssh) = tab.ssh_session {
                                    let _ = ssh.write(&bytes);
                                } else if let Ok(mut state) = tab.terminal.lock() {
                                    state.write(&bytes);
                                }
                            }
                        }
                    }
            }
            // -- Connection editor --
            Message::ShowNewConnection => {
                self.show_host_panel = true;
                self.editor_form = ConnectionForm::default();
                self.host_panel_error = None;
            }
            Message::EditConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    self.show_host_panel = true;
                    self.host_panel_error = None;
                    let has_pw = self.vault.as_ref()
                        .and_then(|v| v.get_connection_password(&conn.id).ok())
                        .flatten()
                        .is_some();
                    self.editor_form = ConnectionForm {
                        label: conn.label.clone(),
                        hostname: conn.hostname.clone(),
                        port: conn.port.to_string(),
                        username: conn.username.clone().unwrap_or_default(),
                        password: String::new(),
                        auth_method: conn.auth_method.clone(),
                        group_name: conn
                            .group_id
                            .and_then(|gid| {
                                self.groups.iter().find(|g| g.id == gid).map(|g| g.label.clone())
                            })
                            .unwrap_or_default(),
                        selected_key: conn.key_id.and_then(|kid| {
                            self.keys.iter().find(|k| k.id == kid).map(|k| k.label.clone())
                        }),
                        jump_host: conn.jump_chain.first().and_then(|jid| {
                            self.connections.iter().find(|c| c.id == *jid).map(|c| c.label.clone())
                        }),
                        selected_identity: conn.identity_id.and_then(|iid| {
                            self.identities.iter().find(|i| i.id == iid).map(|i| i.label.clone())
                        }),
                        editing_id: Some(conn.id),
                        has_existing_password: has_pw,
                        password_touched: false,
                        password_visible: false,
                        username_focused: false,
                        port_forwards: conn.port_forwards.iter().map(|pf| PortForwardForm {
                            local_port: pf.local_port.to_string(),
                            remote_host: pf.remote_host.clone(),
                            remote_port: pf.remote_port.to_string(),
                        }).collect(),
                        mcp_enabled: conn.mcp_enabled,
                    };
                }
            }
            Message::EditorLabelChanged(v) => { self.editor_form.label = v; self.editor_form.username_focused = false; }
            Message::EditorHostnameChanged(v) => { self.editor_form.hostname = v; self.editor_form.username_focused = false; }
            Message::EditorPortChanged(v) => { self.editor_form.port = v; self.editor_form.username_focused = false; }
            Message::EditorUsernameChanged(v) => {
                self.editor_form.username = v;
                self.editor_form.username_focused = true;
            }
            Message::EditorPasswordChanged(v) => {
                self.editor_form.password_touched = true;
                self.editor_form.username_focused = false;
                self.editor_form.password = v;
            }
            Message::EditorTogglePasswordVisibility => {
                self.editor_form.password_visible = !self.editor_form.password_visible;
            }
            Message::EditorAuthMethodChanged(v) => {
                self.editor_form.auth_method = match v.as_str() {
                    "Password" => AuthMethod::Password,
                    "Key" => AuthMethod::Key,
                    "Agent" => AuthMethod::Agent,
                    "Interactive" => AuthMethod::Interactive,
                    _ => AuthMethod::Auto,
                };
            }
            Message::EditorGroupChanged(v) => self.editor_form.group_name = v,
            Message::EditorKeyChanged(v) => {
                self.editor_form.selected_key = if v == "(none)" { None } else { Some(v) };
            }
            Message::EditorJumpHostChanged(v) => {
                self.editor_form.jump_host = if v == "(none)" { None } else { Some(v) };
            }
            Message::EditorSave => {
                if self.editor_form.label.is_empty() || self.editor_form.hostname.is_empty() {
                    self.host_panel_error = Some("Label and hostname are required".into());
                    return Task::none();
                }
                let port: u16 = self.editor_form.port.parse().unwrap_or(22);

                // Find or create group
                let group_id = if !self.editor_form.group_name.is_empty() {
                    let existing = self
                        .groups
                        .iter()
                        .find(|g| g.label == self.editor_form.group_name);
                    match existing {
                        Some(g) => Some(g.id),
                        None => {
                            let g = Group::new(&self.editor_form.group_name);
                            let gid = g.id;
                            if let Some(vault) = &self.vault {
                                let _ = vault.save_group(&g);
                            }
                            self.groups.push(g);
                            Some(gid)
                        }
                    }
                } else {
                    None
                };

                let mut conn = if let Some(id) = self.editor_form.editing_id {
                    // Editing existing
                    self.connections
                        .iter()
                        .find(|c| c.id == id)
                        .cloned()
                        .unwrap_or_else(|| Connection::new("", ""))
                } else {
                    Connection::new("", "")
                };

                conn.label = self.editor_form.label.clone();
                conn.hostname = self.editor_form.hostname.clone();
                conn.port = port;
                conn.username = if self.editor_form.username.is_empty() {
                    None
                } else {
                    Some(self.editor_form.username.clone())
                };
                conn.auth_method = self.editor_form.auth_method.clone();
                conn.group_id = group_id;
                conn.key_id = self.editor_form.selected_key.as_ref().and_then(|label| {
                    self.keys.iter().find(|k| k.label == *label).map(|k| k.id)
                });
                conn.identity_id = self.editor_form.selected_identity.as_ref().and_then(|label| {
                    self.identities.iter().find(|i| i.label == *label).map(|i| i.id)
                });
                conn.jump_chain = self.editor_form.jump_host.as_ref()
                    .and_then(|label| {
                        self.connections.iter().find(|c| c.label == *label).map(|c| vec![c.id])
                    })
                    .unwrap_or_default();
                conn.port_forwards = self.editor_form.port_forwards.iter().filter_map(|pf| {
                    let local_port = pf.local_port.parse::<u16>().ok()?;
                    let remote_port = pf.remote_port.parse::<u16>().ok()?;
                    if pf.remote_host.is_empty() { return None; }
                    Some(oryxis_core::models::connection::PortForward {
                        local_port,
                        remote_host: pf.remote_host.clone(),
                        remote_port,
                    })
                }).collect();
                conn.mcp_enabled = self.editor_form.mcp_enabled;
                conn.updated_at = chrono::Utc::now();

                let password = if !self.editor_form.password_touched {
                    None // User didn't touch the field — preserve existing password
                } else if self.editor_form.password.is_empty() {
                    Some("") // User cleared the password — remove it
                } else {
                    Some(self.editor_form.password.as_str())
                };

                if let Some(vault) = &self.vault {
                    match vault.save_connection(&conn, password) {
                        Ok(()) => {
                            self.show_host_panel = false;
                            self.host_panel_error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => {
                            self.host_panel_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::EditorCancel => {
                self.show_host_panel = false;
                self.host_panel_error = None;
            }
            Message::DeleteConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    let id = conn.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_connection(&id);
                        self.show_host_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }
            Message::DuplicateConnection(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx).cloned() {
                    let mut dup = Connection::new(
                        format!("{} (copy)", conn.label),
                        &conn.hostname,
                    );
                    dup.port = conn.port;
                    dup.username = conn.username.clone();
                    dup.auth_method = conn.auth_method.clone();
                    dup.key_id = conn.key_id;
                    dup.group_id = conn.group_id;
                    dup.jump_chain = conn.jump_chain.clone();
                    dup.port_forwards = conn.port_forwards.clone();
                    dup.proxy = conn.proxy.clone();
                    dup.tags = conn.tags.clone();
                    dup.notes = conn.notes.clone();
                    dup.color = conn.color.clone();
                    if let Some(vault) = &self.vault {
                        // Copy password too
                        let pw = vault.get_connection_password(&conn.id).ok().flatten();
                        let _ = vault.save_connection(&dup, pw.as_deref());
                        self.load_data_from_vault();
                    }
                }
            }
            // -- SSH connection --
            Message::ConnectSsh(idx) => {
                self.card_context_menu = None;
                self.overlay = None;
                // Close the new-tab picker if the connection was picked there.
                self.show_new_tab_picker = false;
                if let Some(conn) = self.connections.get(idx).cloned() {
                    // Resolve credentials: prefer identity if linked, otherwise inline
                    let (password, private_key) = if let Some(iid) = conn.identity_id {
                        let id_pw = self.vault.as_ref()
                            .and_then(|v| v.get_identity_password(&iid).ok().flatten());
                        let identity = self.identities.iter().find(|i| i.id == iid);
                        let id_key = identity.and_then(|i| i.key_id).and_then(|kid| {
                            self.vault.as_ref().and_then(|v| v.get_key_private(&kid).ok().flatten())
                        });
                        (id_pw, id_key)
                    } else {
                        let pw = self.vault.as_ref()
                            .and_then(|v| v.get_connection_password(&conn.id).ok().flatten());
                        let pk = if conn.auth_method == AuthMethod::Key || conn.auth_method == AuthMethod::Auto {
                            conn.key_id.and_then(|kid| {
                                self.vault.as_ref().and_then(|v| v.get_key_private(&kid).ok().flatten())
                            })
                        } else {
                            None
                        };
                        (pw, pk)
                    };

                    // Build resolver for jump hosts
                    let resolver = if !conn.jump_chain.is_empty() {
                        let mut passwords = std::collections::HashMap::new();
                        let mut keys = std::collections::HashMap::new();
                        for jid in &conn.jump_chain {
                            if let Some(vault) = &self.vault
                                && let Ok(Some(pw)) = vault.get_connection_password(jid) {
                                    passwords.insert(*jid, pw);
                                }
                            // Get jump host's key if it uses key auth
                            if let Some(jconn) = self.connections.iter().find(|c| c.id == *jid)
                                && let Some(kid) = jconn.key_id
                                    && let Some(vault) = &self.vault
                                        && let Ok(Some(pk)) = vault.get_key_private(&kid) {
                                            keys.insert(*jid, pk);
                                        }
                        }
                        Some(oryxis_ssh::ConnectionResolver {
                            connections: self.connections.clone(),
                            passwords,
                            private_keys: keys,
                        })
                    } else {
                        None
                    };

                    match TerminalState::new_no_pty(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16) {
                        Ok(mut state) => {
                            state.palette = self.terminal_theme.palette();
                            let label = conn.label.clone();
                            let hostname = format!("SSH {}:{}", conn.hostname, conn.port);
                            let terminal = Arc::new(Mutex::new(state));
                            let tab_idx = self.tabs.len();

                            // Create session log for terminal recording
                            let session_log_id = if let Some(vault) = &self.vault {
                                let log_id = Uuid::new_v4();
                                let _ = vault.create_session_log(&log_id, &conn.id, &conn.label);
                                Some(log_id)
                            } else {
                                None
                            };

                            self.tabs.push(TerminalTab {
                                _id: Uuid::new_v4(),
                                label: label.clone(),
                                terminal: Arc::clone(&terminal),
                                ssh_session: None,
                                session_log_id,
                                chat_history: Vec::new(),
                                chat_visible: false,
                            });

                            // Show progress view instead of terminal
                            self.connecting = Some(ConnectionProgress {
                                label: label.clone(),
                                hostname: hostname.clone(),
                                step: ConnectionStep::Connecting,
                                logs: vec![(ConnectionStep::Connecting, format!("Connecting to {}...", conn.hostname))],
                                failed: false,
                                connection_idx: idx,
                                tab_idx,
                            });
                            self.active_tab = Some(tab_idx);

                            // Host key verification: check callback + ask channel
                            let known_hosts_snapshot: Arc<Mutex<Vec<oryxis_core::models::known_host::KnownHost>>> =
                                Arc::new(Mutex::new(self.known_hosts.clone()));
                            let kh_ref = known_hosts_snapshot.clone();
                            let host_key_check: oryxis_ssh::HostKeyCheckCallback = Arc::new(move |host, port, _key_type, fingerprint| {
                                let hosts = kh_ref.lock().unwrap();
                                if let Some(existing) = hosts.iter().find(|h| h.hostname == host && h.port == port) {
                                    if existing.fingerprint != fingerprint {
                                        return oryxis_ssh::HostKeyStatus::Changed {
                                            old_fingerprint: existing.fingerprint.clone(),
                                        };
                                    }
                                    return oryxis_ssh::HostKeyStatus::Known;
                                }
                                oryxis_ssh::HostKeyStatus::Unknown
                            });

                            // Channel for the SSH engine to ask the UI about host keys
                            let (hk_ask_tx, mut hk_ask_rx) = tokio::sync::mpsc::channel::<(oryxis_ssh::HostKeyQuery, tokio::sync::oneshot::Sender<bool>)>(1);
                            // Channel for the UI to send responses back
                            let (hk_resp_tx, mut hk_resp_rx) = tokio::sync::mpsc::channel::<bool>(1);
                            self.host_key_response_tx = Some(hk_resp_tx);

                            let conn_host = conn.hostname.clone();
                            let conn_port = conn.port;
                            let username = conn.username.clone()
                                .or_else(|| {
                                    conn.identity_id.and_then(|iid| {
                                        self.identities.iter().find(|i| i.id == iid)
                                            .and_then(|i| i.username.clone())
                                    })
                                })
                                .unwrap_or_else(|| "root".into());
                            let auth_method_label = format!("{:?}", conn.auth_method);
                            let stream = iced::stream::channel::<SshStreamMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<SshStreamMsg>| {
                                async move {
                                    let engine = SshEngine::new()
                                        .with_host_key_check(host_key_check)
                                        .with_host_key_ask(hk_ask_tx);

                                    // Spawn a bridge task: receives host key queries from the SSH engine,
                                    // forwards to iced stream, and waits for UI response
                                    let mut sender_clone = sender.clone();
                                    let _hk_bridge = tokio::spawn(async move {
                                        while let Some((query, resp_tx)) = hk_ask_rx.recv().await {
                                            // Send query to iced UI
                                            let _ = sender_clone.send(SshStreamMsg::HostKeyVerify(query)).await;
                                            // Wait for UI response
                                            let accepted = hk_resp_rx.recv().await.unwrap_or(false);
                                            let _ = resp_tx.send(accepted);
                                        }
                                    });

                                    // Step 1: TCP connection + SSH handshake + host key verification
                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::Connecting,
                                        format!("Connecting to {}:{}...", conn_host, conn_port),
                                    )).await;

                                    let mut handle = match engine.establish_transport(&conn, resolver.as_ref()).await {
                                        Ok(h) => {
                                            let _ = sender.send(SshStreamMsg::Progress(
                                                ConnectionStep::Handshake,
                                                format!("Connected to {}:{} — handshake OK", conn_host, conn_port),
                                            )).await;
                                            h
                                        }
                                        Err(e) => {
                                            let _ = sender.send(SshStreamMsg::Error(
                                                format!("Connection to {}:{} failed: {}", conn_host, conn_port, e),
                                            )).await;
                                            return;
                                        }
                                    };

                                    // Step 2: Authentication
                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::Authenticating,
                                        format!("Authenticating as \"{}\" ({})...", username, auth_method_label),
                                    )).await;

                                    if let Err(e) = engine.do_authenticate(&mut handle, &conn, password.as_deref(), private_key.as_deref()).await {
                                        let _ = sender.send(SshStreamMsg::Error(
                                            format!("Authentication failed for \"{}\": {}", username, e),
                                        )).await;
                                        return;
                                    }

                                    let _ = sender.send(SshStreamMsg::Progress(
                                        ConnectionStep::Authenticating,
                                        format!("Authenticated as \"{}\"", username),
                                    )).await;

                                    // Step 3: Open PTY session (+ port forwards)
                                    if !conn.port_forwards.is_empty() {
                                        let fwd_summary: Vec<String> = conn.port_forwards.iter()
                                            .map(|pf| format!("{}:{}:{}", pf.local_port, pf.remote_host, pf.remote_port))
                                            .collect();
                                        let _ = sender.send(SshStreamMsg::Progress(
                                            ConnectionStep::Authenticating,
                                            format!("Port forwards: {}", fwd_summary.join(", ")),
                                        )).await;
                                    }
                                    match engine.open_session(handle, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS, &conn.port_forwards).await {
                                        Ok((session, mut rx)) => {
                                            let session = Arc::new(session);
                                            let _ = sender.send(SshStreamMsg::Connected(session.clone())).await;
                                            while let Some(data) = rx.recv().await {
                                                if sender.send(SshStreamMsg::Data(data)).await.is_err() {
                                                    break;
                                                }
                                            }
                                            let _ = sender.send(SshStreamMsg::Disconnected).await;
                                        }
                                        Err(e) => {
                                            let _ = sender.send(SshStreamMsg::Error(
                                                format!("Session setup failed: {}", e),
                                            )).await;
                                        }
                                    }
                                }
                            });

                            return Task::stream(stream).map(move |msg| match msg {
                                SshStreamMsg::Progress(step, log) => Message::SshProgress(step, log),
                                SshStreamMsg::Connected(session) => Message::SshConnected(tab_idx, session),
                                SshStreamMsg::NewKnownHosts(hosts) => Message::SshNewKnownHosts(hosts),
                                SshStreamMsg::HostKeyVerify(query) => Message::SshHostKeyVerify(query),
                                SshStreamMsg::Data(data) => Message::PtyOutput(tab_idx, data),
                                SshStreamMsg::Error(err) => Message::SshError(err),
                                SshStreamMsg::Disconnected => Message::SshDisconnected(tab_idx),
                            });
                        }
                        Err(e) => {
                            tracing::error!("Failed to create terminal state: {}", e);
                        }
                    }
                }
            }
            Message::SshProgress(step, log) => {
                if let Some(ref mut progress) = self.connecting {
                    progress.step = step;
                    progress.logs.push((step, log));
                }
            }
            Message::SshNewKnownHosts(hosts) => {
                if let Some(vault) = &self.vault {
                    for kh in &hosts {
                        let _ = vault.save_known_host(kh);
                    }
                    self.known_hosts = vault.list_known_hosts().unwrap_or_default();
                }
            }
            Message::SshHostKeyVerify(query) => {
                self.pending_host_key = Some(query);
            }
            Message::SshHostKeyReject => {
                self.pending_host_key = None;
                if let Some(ref tx) = self.host_key_response_tx {
                    let _ = tx.try_send(false);
                }
            }
            Message::SshHostKeyContinue => {
                // Accept for this session but don't save to known hosts
                self.pending_host_key = None;
                if let Some(ref tx) = self.host_key_response_tx {
                    let _ = tx.try_send(true);
                }
            }
            Message::SshHostKeyAcceptAndSave => {
                // Accept and save to known hosts
                if let (Some(query), Some(vault)) = (&self.pending_host_key, &self.vault) {
                    let kh = oryxis_core::models::known_host::KnownHost::new(
                        &query.hostname, query.port, &query.key_type, &query.fingerprint,
                    );
                    let _ = vault.save_known_host(&kh);
                    self.known_hosts = vault.list_known_hosts().unwrap_or_default();
                }
                self.pending_host_key = None;
                if let Some(ref tx) = self.host_key_response_tx {
                    let _ = tx.try_send(true);
                }
            }
            Message::SshConnected(tab_idx, session) => {
                let mut detect_for: Option<(Uuid, Arc<SshSession>)> = None;
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    tab.ssh_session = Some(session.clone());
                    let label = tab.label.clone();
                    tracing::info!("SSH connected: {}", label);
                    if let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Connected, "Session established",
                        );
                        let _ = vault.add_log(&entry);
                    }
                    // Reset the auto-reconnect counter for this connection.
                    if let Some(conn) = self.connections.iter().find(|c| c.label == label) {
                        self.reconnect_counters.remove(&conn.id);
                        // Queue silent OS detection only if:
                        //   - the feature is enabled,
                        //   - we haven't detected this host before (runs once),
                        //   - and the user hasn't set a custom icon override.
                        let has_custom =
                            conn.custom_icon.is_some() || conn.custom_color.is_some();
                        if self.setting_os_detection && conn.detected_os.is_none() && !has_custom {
                            detect_for = Some((conn.id, session));
                        }
                    }
                }
                // Clear progress, show terminal
                self.connecting = None;

                if let Some((conn_id, sess)) = detect_for {
                    return Task::perform(
                        async move { (conn_id, sess.detect_os().await) },
                        |(id, os)| Message::OsDetected(id, os),
                    );
                }
            }
            Message::OsDetected(conn_id, os) => {
                // Persist + update in-memory list so the icon refreshes.
                if let Some(vault) = &self.vault {
                    let _ = vault.set_detected_os(&conn_id, os.as_deref());
                }
                if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                    conn.detected_os = os.clone();
                }
                tracing::info!("OS detected for {}: {:?}", conn_id, os);
            }
            Message::SettingToggleOsDetection => {
                self.setting_os_detection = !self.setting_os_detection;
            }
            Message::SettingToggleAutoCheckUpdates => {
                self.setting_auto_check_updates = !self.setting_auto_check_updates;
            }
            Message::CheckForUpdate => {
                if !self.setting_auto_check_updates {
                    return Task::none();
                }
                // Also respect a persisted "skip this version" so we never
                // nag about the same tag twice.
                let skipped = self
                    .vault
                    .as_ref()
                    .and_then(|v| v.get_setting("skipped_update_version").ok().flatten());
                return Task::perform(
                    crate::update::check_latest_release(),
                    move |opt| {
                        match opt {
                            Some(info) if Some(&info.version) != skipped.as_ref() => {
                                Message::UpdateCheckResult(Some(info))
                            }
                            _ => Message::UpdateCheckResult(None),
                        }
                    },
                );
            }
            Message::CheckForUpdateManual => {
                // Manual trigger from the settings button — runs regardless
                // of the auto-check preference. Clears prior skipped version
                // so the user can resurface a previously-dismissed prompt.
                self.update_error = None;
                self.update_check_status = Some("Checking…".into());
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", "");
                }
                return Task::perform(
                    crate::update::check_latest_release(),
                    |info| match info {
                        Some(i) => Message::UpdateCheckResult(Some(i)),
                        None => Message::UpdateCheckResult(None),
                    },
                );
            }
            Message::UpdateCheckResult(info) => {
                match info {
                    Some(i) => {
                        self.pending_update = Some(i);
                        self.update_check_status = None;
                    }
                    None => {
                        // Only surface the "up to date" message if a manual
                        // check is in flight (status was set to "Checking…").
                        // A silent boot check that finds nothing should not
                        // change the settings UI.
                        if self.update_check_status.is_some() {
                            self.update_check_status = Some(format!(
                                "You're running the latest version ({}).",
                                env!("CARGO_PKG_VERSION"),
                            ));
                        }
                    }
                }
            }
            Message::UpdateSkipVersion => {
                if let Some(info) = self.pending_update.take()
                    && let Some(vault) = &self.vault {
                    let _ = vault.set_setting("skipped_update_version", &info.version);
                }
            }
            Message::UpdateLater => {
                self.pending_update = None;
            }
            Message::UpdateOpenRelease => {
                if let Some(info) = &self.pending_update {
                    let _ = open_in_browser(&info.html_url);
                }
            }
            Message::UpdateStartDownload => {
                let Some(info) = self.pending_update.clone() else {
                    return Task::none();
                };
                let Some(url) = info.installer_url.clone() else {
                    self.update_error = Some("No installer asset for this platform".into());
                    return Task::none();
                };
                let name = info
                    .installer_name
                    .clone()
                    .unwrap_or_else(|| format!("oryxis-update-{}", info.version));
                self.update_downloading = true;
                self.update_progress = 0.0;
                self.update_error = None;
                return Task::perform(
                    async move {
                        crate::update::download_installer(&url, &name, |_| {}).await
                    },
                    Message::UpdateDownloadComplete,
                );
            }
            Message::UpdateDownloadProgress(p) => {
                self.update_progress = p;
            }
            Message::UpdateDownloadComplete(result) => {
                self.update_downloading = false;
                match result {
                    Ok(path) => {
                        if let Err(e) = crate::update::launch_installer(&path) {
                            self.update_error = Some(e);
                        } else {
                            // Installer launched — exit app so it can write
                            // over our binary. Graceful quit via window close.
                            self.pending_update = None;
                            return iced::window::latest().then(|id_opt| match id_opt {
                                Some(id) => iced::window::close(id),
                                None => Task::none(),
                            });
                        }
                    }
                    Err(e) => self.update_error = Some(e),
                }
            }
            Message::SshDisconnected(tab_idx) => {
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    let label = tab.label.replace(" (disconnected)", "");
                    // End session log
                    if let Some(log_id) = tab.session_log_id
                        && let Some(vault) = &self.vault {
                            let _ = vault.end_session_log(&log_id);
                    }
                    // Log
                    if let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Disconnected, "Session ended",
                        );
                        let _ = vault.add_log(&entry);
                    }
                    tab.label = format!("{} (disconnected)", label);
                    tab.ssh_session = None;
                    // Refresh session logs list
                    if let Some(vault) = &self.vault {
                        self.session_logs = vault.list_session_logs().unwrap_or_default();
                    }
                }
            }
            Message::SshCloseProgress => {
                // Close connection progress, remove the tab
                if let Some(ref progress) = self.connecting {
                    let tab_idx = progress.tab_idx;
                    if tab_idx < self.tabs.len() {
                        self.tabs.remove(tab_idx);
                    }
                }
                self.connecting = None;
                self.active_tab = None;
                self.active_view = View::Dashboard;
            }
            Message::SshEditFromProgress => {
                if let Some(ref progress) = self.connecting {
                    let idx = progress.connection_idx;
                    let tab_idx = progress.tab_idx;
                    self.connecting = None;
                    if tab_idx < self.tabs.len() {
                        self.tabs.remove(tab_idx);
                    }
                    self.active_tab = None;
                    self.active_view = View::Dashboard;
                    return self.update(Message::EditConnection(idx));
                }
            }
            Message::SshRetry => {
                if let Some(ref progress) = self.connecting {
                    let idx = progress.connection_idx;
                    let tab_idx = progress.tab_idx;
                    self.connecting = None;
                    if tab_idx < self.tabs.len() {
                        self.tabs.remove(tab_idx);
                    }
                    self.active_tab = None;
                    return self.update(Message::ConnectSsh(idx));
                }
            }
            Message::SshError(err) => {
                tracing::error!("SSH error: {}", err);
                if let Some(vault) = &self.vault {
                    let label = self.connecting.as_ref().map(|p| p.label.as_str()).unwrap_or("unknown");
                    let entry = oryxis_core::models::log_entry::LogEntry::new(
                        label, label, oryxis_core::models::log_entry::LogEvent::Error, &err,
                    );
                    let _ = vault.add_log(&entry);
                }
                // Mark progress as failed (keep the view open with logs)
                if let Some(ref mut progress) = self.connecting {
                    progress.failed = true;
                    progress.logs.push((progress.step, format!("Error: {}", err)));
                } else {
                    self.host_panel_error = Some(format!("SSH: {}", err));
                }
            }

            // -- Local shell --
            // -- Snippets --
            Message::ShowSnippetPanel => {
                self.show_snippet_panel = true;
                self.snippet_label.clear();
                self.snippet_command.clear();
                self.snippet_editing_id = None;
                self.snippet_error = None;
            }
            Message::HideSnippetPanel => {
                self.show_snippet_panel = false;
            }
            Message::SnippetLabelChanged(v) => self.snippet_label = v,
            Message::SnippetCommandChanged(v) => self.snippet_command = v,
            Message::EditSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    self.show_snippet_panel = true;
                    self.snippet_label = snip.label.clone();
                    self.snippet_command = snip.command.clone();
                    self.snippet_editing_id = Some(snip.id);
                    self.snippet_error = None;
                }
            }
            Message::SaveSnippet => {
                if self.snippet_label.is_empty() || self.snippet_command.is_empty() {
                    self.snippet_error = Some("Label and command are required".into());
                    return Task::none();
                }
                let mut snip = if let Some(id) = self.snippet_editing_id {
                    self.snippets.iter().find(|s| s.id == id).cloned()
                        .unwrap_or_else(|| oryxis_core::models::snippet::Snippet::new("", ""))
                } else {
                    oryxis_core::models::snippet::Snippet::new("", "")
                };
                snip.label = self.snippet_label.clone();
                snip.command = self.snippet_command.clone();
                if let Some(vault) = &self.vault {
                    match vault.save_snippet(&snip) {
                        Ok(()) => {
                            self.show_snippet_panel = false;
                            self.snippet_error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => self.snippet_error = Some(e.to_string()),
                    }
                }
            }
            Message::DeleteSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let id = snip.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_snippet(&id);
                        self.show_snippet_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }
            Message::RunSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let cmd = format!("{}\n", snip.command);
                    if let Some(tab_idx) = self.active_tab
                        && let Some(tab) = self.tabs.get(tab_idx) {
                            if let Some(ref ssh) = tab.ssh_session {
                                let _ = ssh.write(cmd.as_bytes());
                            } else if let Ok(mut state) = tab.terminal.lock() {
                                state.write(cmd.as_bytes());
                            }
                        }
                }
            }

            // -- Known hosts --
            Message::DeleteKnownHost(idx) => {
                if let Some(kh) = self.known_hosts.get(idx) {
                    let id = kh.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_known_host(&id);
                        self.load_data_from_vault();
                    }
                }
            }
            Message::ClearAllKnownHosts => {
                if let Some(vault) = &self.vault {
                    for kh in self.known_hosts.clone() {
                        let _ = vault.delete_known_host(&kh.id);
                    }
                    self.load_data_from_vault();
                }
            }

            // -- History --
            Message::ClearLogs => {
                if let Some(vault) = &self.vault {
                    let _ = vault.clear_logs();
                    self.logs_page = 0;
                    self.load_data_from_vault();
                }
            }
            Message::LogsPageNext => {
                let max_page = (self.logs_total.saturating_sub(1)) / 50;
                if self.logs_page < max_page {
                    self.logs_page += 1;
                    if let Some(vault) = &self.vault {
                        self.logs = vault
                            .list_logs_page(self.logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            Message::LogsPagePrev => {
                if self.logs_page > 0 {
                    self.logs_page -= 1;
                    if let Some(vault) = &self.vault {
                        self.logs = vault
                            .list_logs_page(self.logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            Message::ViewSessionLog(log_id) => {
                if let Some(vault) = &self.vault
                    && let Ok(Some(data)) = vault.get_session_data(&log_id) {
                        let rendered = strip_ansi(&data);
                        self.viewing_session_log = Some((log_id, rendered));
                }
            }
            Message::CloseSessionLogView => {
                self.viewing_session_log = None;
            }
            Message::DeleteSessionLog(idx) => {
                if let Some(entry) = self.session_logs.get(idx) {
                    let id = entry.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_session_log(&id);
                        self.session_logs = vault.list_session_logs().unwrap_or_default();
                    }
                }
                // Close viewer if we deleted the one being viewed
                if let Some((viewed_id, _)) = &self.viewing_session_log
                    && self.session_logs.iter().all(|s| s.id != *viewed_id) {
                        self.viewing_session_log = None;
                }
            }

            // -- Settings --
            Message::TerminalThemeChanged(name) => {
                if let Some(theme) = oryxis_terminal::TerminalTheme::ALL.iter().find(|t| t.name() == name) {
                    self.terminal_theme = *theme;
                    // Apply to all open terminals
                    for tab in &self.tabs {
                        if let Ok(mut state) = tab.terminal.lock() {
                            state.palette = theme.palette();
                        }
                    }
                }
            }
            Message::LanguageChanged(name) => {
                use crate::i18n::Language;
                if let Some(lang) = Language::ALL.iter().find(|l| l.name() == name) {
                    Language::set_active(*lang);
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("language", lang.code());
                    }
                }
            }
            Message::AppThemeChanged(name) => {
                use crate::theme::AppTheme;
                if let Some(theme) = AppTheme::ALL.iter().find(|t| t.name() == name) {
                    AppTheme::set_active(*theme);
                    // Map app theme to terminal palette
                    let term_theme = match theme {
                        AppTheme::OryxisDark => oryxis_terminal::TerminalTheme::OryxisDark,
                        AppTheme::OryxisLight => oryxis_terminal::TerminalTheme::OryxisDark,
                        AppTheme::Termius => oryxis_terminal::TerminalTheme::OryxisDark,
                        AppTheme::Darcula => oryxis_terminal::TerminalTheme::Dracula,
                        AppTheme::IslandsDark => oryxis_terminal::TerminalTheme::Dracula,
                        AppTheme::Dracula => oryxis_terminal::TerminalTheme::Dracula,
                        AppTheme::Monokai => oryxis_terminal::TerminalTheme::Monokai,
                        AppTheme::HackerGreen => oryxis_terminal::TerminalTheme::HackerGreen,
                        AppTheme::Nord => oryxis_terminal::TerminalTheme::Nord,
                        AppTheme::NordLight => oryxis_terminal::TerminalTheme::Nord,
                        AppTheme::SolarizedLight => oryxis_terminal::TerminalTheme::SolarizedDark,
                        AppTheme::PaperLight => oryxis_terminal::TerminalTheme::OryxisDark,
                    };
                    self.terminal_theme = term_theme;
                    for tab in &self.tabs {
                        if let Ok(mut state) = tab.terminal.lock() {
                            state.palette = term_theme.palette();
                        }
                    }
                }
            }
            Message::TerminalFontSizeIncrease => {
                self.terminal_font_size = (self.terminal_font_size + 1.0).min(24.0);
            }
            Message::TerminalFontSizeDecrease => {
                self.terminal_font_size = (self.terminal_font_size - 1.0).max(10.0);
            }
            Message::TerminalFontChanged(name) => {
                self.terminal_font_name = name;
            }
            Message::ChangeSettingsSection(section) => {
                self.settings_section = section;
            }
            Message::ToggleCopyOnSelect => {
                self.setting_copy_on_select = !self.setting_copy_on_select;
            }
            Message::ToggleBoldIsBright => {
                self.setting_bold_is_bright = !self.setting_bold_is_bright;
            }
            Message::ToggleBellSound => {
                self.setting_bell_sound = !self.setting_bell_sound;
            }
            Message::ToggleKeywordHighlight => {
                self.setting_keyword_highlight = !self.setting_keyword_highlight;
            }
            Message::SettingKeepaliveChanged(val) => {
                self.setting_keepalive_interval = val;
            }
            Message::SettingScrollbackChanged(val) => {
                self.setting_scrollback_rows = val;
            }
            Message::SettingToggleAutoReconnect => {
                self.setting_auto_reconnect = !self.setting_auto_reconnect;
            }
            Message::SettingMaxReconnectChanged(val) => {
                self.setting_max_reconnect_attempts = val;
            }
            Message::ConnectAnimTick => {
                self.connect_anim_tick = self.connect_anim_tick.wrapping_add(1);
            }
            Message::AutoReconnectTick => {
                if !self.setting_auto_reconnect {
                    // fall through, nothing to do
                } else {
                    let max_attempts: u32 =
                        self.setting_max_reconnect_attempts.parse().unwrap_or(5);
                    // Find the first disconnected SSH tab whose counter is under the limit.
                    // Only reconnect one per tick to avoid thrashing; next tick picks up
                    // the next candidate.
                    let candidate: Option<usize> = (0..self.tabs.len()).find(|&i| {
                        let tab = &self.tabs[i];
                        if !tab.label.ends_with(" (disconnected)") {
                            return false;
                        }
                        let base = tab.label.trim_end_matches(" (disconnected)");
                        let Some(conn) = self.connections.iter().find(|c| c.label == base) else {
                            return false;
                        };
                        let attempts = self.reconnect_counters.get(&conn.id).copied().unwrap_or(0);
                        attempts < max_attempts
                    });
                    if let Some(tab_idx) = candidate {
                        let base = self.tabs[tab_idx]
                            .label
                            .trim_end_matches(" (disconnected)")
                            .to_string();
                        if let Some(conn) = self.connections.iter().find(|c| c.label == base) {
                            let entry = self.reconnect_counters.entry(conn.id).or_insert(0);
                            *entry += 1;
                        }
                        return Task::done(Message::ReconnectTab(tab_idx));
                    }
                }
            }
            Message::LockVault => {
                if let Some(vault) = &mut self.vault {
                    vault.lock();
                    if self.vault_has_user_password {
                        self.vault_state = VaultState::Locked;
                        self.connections.clear();
                        self.keys.clear();
                        self.snippets.clear();
                        self.groups.clear();
                        self.tabs.clear();
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        // No user password: re-open immediately
                        let _ = vault.open_without_password();
                    }
                }
            }

            Message::OpenLocalShell => {
                self.connecting = None; // Clear any pending SSH connection progress
                match TerminalState::new(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16) {
                    Ok((mut state, rx)) => {
                        state.palette = self.terminal_theme.palette();
                        let tab_idx = self.tabs.len();
                        self.tabs.push(TerminalTab {
                            _id: Uuid::new_v4(),
                            label: "Local Shell".into(),
                            terminal: Arc::new(Mutex::new(state)),
                            ssh_session: None,
                            session_log_id: None,
                            chat_history: Vec::new(),
                            chat_visible: false,
                        });
                        self.active_tab = Some(tab_idx);
                        self.active_view = View::Terminal;

                        let stream = UnboundedReceiverStream::new(rx);
                        return Task::stream(stream).map(move |bytes| Message::PtyOutput(tab_idx, bytes));
                    }
                    Err(e) => {
                        tracing::error!("Failed to spawn local shell: {}", e);
                    }
                }
            }
            // -- Keys --
            Message::ShowKeyPanel => {
                // Also navigate to the Keys screen — the import panel is rendered
                // inside view_keys(), so the user needs to be there to see it
                // (e.g. when they click "+ Key" from the host editor).
                self.active_view = View::Keys;
                self.active_tab = None;
                self.show_key_panel = true;
                self.key_import_label.clear();
                self.key_import_content = text_editor::Content::new();
                self.key_import_pem.clear();
                self.key_error = None;
                self.key_success = None;
                self.editing_key_id = None;
                self.key_context_menu = None;
                self.overlay = None;
            }
            Message::HideKeyPanel => {
                self.show_key_panel = false;
                self.editing_key_id = None;
            }
            Message::KeyImportLabelChanged(v) => self.key_import_label = v,
            Message::KeyContentAction(action) => {
                self.key_import_content.perform(action);
                self.key_import_pem = self.key_import_content.text();
            }
            Message::BrowseKeyFile => {
                return Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let file = rfd::FileDialog::new()
                            .set_title("Select SSH Private Key")
                            .pick_file();
                        match file {
                            Some(path) => {
                                let filename = path
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "imported-key".into());
                                let content = std::fs::read_to_string(&path)
                                    .map_err(|e| format!("Failed to read: {}", e))?;
                                Ok((filename, content))
                            }
                            None => Err("cancelled".to_string()),
                        }
                    }),
                    |result| match result {
                        Ok(Ok((filename, content))) => Message::KeyFileLoaded(filename, content),
                        Ok(Err(e)) => Message::KeyFileBrowseError(e),
                        Err(e) => Message::KeyFileBrowseError(format!("Thread error: {}", e)),
                    },
                );
            }
            Message::KeyFileLoaded(filename, content) => {
                if self.key_import_label.is_empty() {
                    self.key_import_label = filename;
                }
                self.key_import_content = text_editor::Content::with_text(&content);
                self.key_import_pem = content;
                self.show_key_panel = true;
                self.key_error = None;
                self.key_success = Some("Key file loaded".into());
            }
            Message::KeyFileBrowseError(err) => {
                if !err.contains("cancelled") {
                    self.key_error = Some(err);
                }
            }
            Message::ImportKey => {
                if self.key_import_pem.is_empty() {
                    self.key_error = Some("Select a key file first".into());
                    return Task::none();
                }
                let label = if self.key_import_label.is_empty() {
                    "imported-key".to_string()
                } else {
                    self.key_import_label.clone()
                };
                match oryxis_vault::import_key(&label, &self.key_import_pem) {
                    Ok(mut generated) => {
                        // If editing an existing key, preserve its ID
                        if let Some(existing_id) = self.editing_key_id {
                            generated.key.id = existing_id;
                        }
                        if let Some(vault) = &self.vault {
                            match vault.save_key(&generated.key, Some(&generated.private_pem)) {
                                Ok(()) => {
                                    let verb = if self.editing_key_id.is_some() { "updated" } else { "imported" };
                                    self.key_error = None;
                                    self.key_success = Some(format!("Key '{}' {}", label, verb));
                                    self.key_import_label.clear();
                                    self.key_import_content = text_editor::Content::new();
                                    self.key_import_pem.clear();
                                    self.show_key_panel = false;
                                    self.editing_key_id = None;
                                    self.load_data_from_vault();
                                }
                                Err(e) => self.key_error = Some(e.to_string()),
                            }
                        }
                    }
                    Err(e) => self.key_error = Some(format!("Import failed: {}", e)),
                }
            }
            Message::DeleteKey(idx) => {
                if let Some(key) = self.keys.get(idx) {
                    let id = key.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_key(&id);
                        self.load_data_from_vault();
                        self.key_success = Some("Key deleted".into());
                    }
                }
                self.key_context_menu = None;
                self.overlay = None;
            }
            Message::ShowKeyMenu(idx) => {
                if self.key_context_menu == Some(idx) {
                    self.key_context_menu = None;
                    self.overlay = None;
                } else {
                    self.key_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::KeyActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::HideKeyMenu => {
                self.key_context_menu = None;
                self.identity_context_menu = None;
                self.show_keychain_add_menu = false;
                self.overlay = None;
            }
            Message::EditKey(idx) => {
                if let Some(key) = self.keys.get(idx) {
                    self.editing_key_id = Some(key.id);
                    self.key_import_label = key.label.clone();
                    // Load existing private key PEM from vault
                    let pem = self.vault.as_ref()
                        .and_then(|v| v.get_key_private(&key.id).ok().flatten())
                        .unwrap_or_default();
                    self.key_import_content = text_editor::Content::with_text(&pem);
                    self.key_import_pem = pem;
                    self.show_key_panel = true;
                    self.key_error = None;
                    self.key_success = None;
                    self.key_context_menu = None;
                    self.overlay = None;
                }
            }
            Message::KeySearchChanged(v) => {
                self.key_search = v;
            }

            // ── Identities ──
            Message::ShowIdentityPanel => {
                self.show_identity_panel = true;
                self.identity_form_label.clear();
                self.identity_form_username.clear();
                self.identity_form_password.clear();
                self.identity_form_key = None;
                self.identity_form_password_visible = false;
                self.identity_form_password_touched = false;
                self.identity_form_has_existing_password = false;
                self.editing_identity_id = None;
                self.show_keychain_add_menu = false;
                self.identity_context_menu = None;
                self.overlay = None;
            }
            Message::HideIdentityPanel => {
                self.show_identity_panel = false;
            }
            Message::IdentityLabelChanged(v) => {
                self.identity_form_label = v;
            }
            Message::IdentityUsernameChanged(v) => {
                self.identity_form_username = v;
            }
            Message::IdentityPasswordChanged(v) => {
                self.identity_form_password_touched = true;
                self.identity_form_password = v;
            }
            Message::IdentityTogglePasswordVisibility => {
                self.identity_form_password_visible = !self.identity_form_password_visible;
            }
            Message::IdentityKeyChanged(v) => {
                self.identity_form_key = if v == "(none)" { None } else { Some(v) };
            }
            Message::SaveIdentity => {
                if self.identity_form_label.trim().is_empty() {
                    return Task::none();
                }
                let mut identity = if let Some(id) = self.editing_identity_id {
                    self.identities.iter().find(|i| i.id == id).cloned()
                        .unwrap_or_else(|| Identity::new(""))
                } else {
                    Identity::new("")
                };
                identity.label = self.identity_form_label.clone();
                identity.username = if self.identity_form_username.is_empty() {
                    None
                } else {
                    Some(self.identity_form_username.clone())
                };
                identity.key_id = self.identity_form_key.as_ref().and_then(|label| {
                    self.keys.iter().find(|k| k.label == *label).map(|k| k.id)
                });
                identity.updated_at = chrono::Utc::now();

                let password = if !self.identity_form_password_touched {
                    None
                } else if self.identity_form_password.is_empty() {
                    Some("")
                } else {
                    Some(self.identity_form_password.as_str())
                };

                if let Some(vault) = &self.vault {
                    let _ = vault.save_identity(&identity, password);
                    self.load_data_from_vault();
                }
                self.show_identity_panel = false;
            }
            Message::EditIdentity(idx) => {
                if let Some(identity) = self.identities.get(idx) {
                    self.editing_identity_id = Some(identity.id);
                    self.identity_form_label = identity.label.clone();
                    self.identity_form_username = identity.username.clone().unwrap_or_default();
                    self.identity_form_password.clear();
                    self.identity_form_password_touched = false;
                    self.identity_form_password_visible = false;
                    self.identity_form_has_existing_password = self.vault.as_ref()
                        .and_then(|v| v.get_identity_password(&identity.id).ok().flatten())
                        .is_some();
                    self.identity_form_key = identity.key_id.and_then(|kid| {
                        self.keys.iter().find(|k| k.id == kid).map(|k| k.label.clone())
                    });
                    self.show_identity_panel = true;
                    self.identity_context_menu = None;
                    self.overlay = None;
                }
            }
            Message::DeleteIdentity(idx) => {
                if let Some(identity) = self.identities.get(idx) {
                    let id = identity.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_identity(&id);
                        self.load_data_from_vault();
                    }
                }
                self.identity_context_menu = None;
                self.overlay = None;
            }
            Message::ShowIdentityMenu(idx) => {
                if self.identity_context_menu == Some(idx) {
                    self.identity_context_menu = None;
                    self.overlay = None;
                } else {
                    self.identity_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::IdentityActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::ToggleKeychainAddMenu => {
                if self.show_keychain_add_menu {
                    self.show_keychain_add_menu = false;
                    self.overlay = None;
                } else {
                    self.show_keychain_add_menu = true;
                    // Push the menu a bit below the click point so it appears
                    // under the button instead of overlapping it. Also nudge
                    // left so the menu's left edge roughly aligns with the
                    // left half of the split button (rather than the cursor).
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::KeychainAdd,
                        x: (self.mouse_position.x - 60.0).max(0.0),
                        y: self.mouse_position.y + 16.0,
                    });
                }
            }

            // ── Connection identity ──
            Message::EditorIdentityChanged(v) => {
                self.editor_form.username_focused = false;
                if v == "(none)" {
                    self.editor_form.selected_identity = None;
                } else {
                    self.editor_form.selected_identity = Some(v);
                }
            }

            // ── AI settings ──
            Message::ToggleAiEnabled => {
                self.ai_enabled = !self.ai_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_enabled", if self.ai_enabled { "true" } else { "false" });
                }
            }
            Message::AiProviderChanged(provider) => {
                // Map display name to internal name
                self.ai_provider = match provider.as_str() {
                    "Anthropic" => "anthropic",
                    "OpenAI" => "openai",
                    "Google Gemini" => "gemini",
                    "Custom" => "custom",
                    other => other,
                }.to_lowercase();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_provider", &self.ai_provider);
                }
                // Suggest default model when provider changes
                match self.ai_provider.as_str() {
                    "anthropic" => {
                        self.ai_model = "claude-sonnet-4-20250514".into();
                        if let Some(vault) = &self.vault {
                            let _ = vault.set_setting("ai_model", &self.ai_model);
                        }
                    }
                    "openai" => {
                        self.ai_model = "gpt-4o".into();
                        if let Some(vault) = &self.vault {
                            let _ = vault.set_setting("ai_model", &self.ai_model);
                        }
                    }
                    "gemini" => {
                        self.ai_model = "gemini-2.5-flash".into();
                        if let Some(vault) = &self.vault {
                            let _ = vault.set_setting("ai_model", &self.ai_model);
                        }
                    }
                    _ => {} // custom: keep current model
                }
            }
            Message::AiModelChanged(model) => {
                self.ai_model = model;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_model", &self.ai_model);
                }
            }
            Message::AiApiKeyChanged(key) => {
                self.ai_api_key = key;
            }
            Message::AiApiUrlChanged(url) => {
                self.ai_api_url = url;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_api_url", &self.ai_api_url);
                }
            }
            Message::AiSystemPromptChanged(prompt) => {
                self.ai_system_prompt = prompt;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_system_prompt", &self.ai_system_prompt);
                }
            }
            Message::SaveAiApiKey => {
                if !self.ai_api_key.is_empty()
                    && let Some(vault) = &self.vault
                    && vault.set_ai_api_key(&self.ai_api_key).is_ok() {
                        self.ai_api_key.clear();
                        self.ai_api_key_set = true;
                }
            }

            // ── Vault password management ──
            Message::ToggleVaultPassword => {
                if self.vault_has_user_password {
                    // Remove password
                    if let Some(vault) = &mut self.vault {
                        match vault.remove_user_password() {
                            Ok(()) => {
                                self.vault_has_user_password = false;
                                self.vault_password_error = None;
                                self.vault_new_password.clear();
                            }
                            Err(e) => {
                                self.vault_password_error = Some(e.to_string());
                            }
                        }
                    }
                } else {
                    // Show password input (don't do anything yet, user needs to type and confirm)
                    self.vault_new_password.clear();
                    self.vault_password_error = None;
                }
            }
            Message::VaultNewPasswordChanged(pw) => {
                self.vault_new_password = pw;
            }
            Message::SetVaultPassword => {
                if self.vault_new_password.len() < 4 {
                    self.vault_password_error = Some("Password must be at least 4 characters".into());
                    return Task::none();
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_user_password(&self.vault_new_password) {
                        Ok(()) => {
                            self.vault_has_user_password = true;
                            self.vault_password_error = None;
                            self.vault_new_password.clear();
                        }
                        Err(e) => {
                            self.vault_password_error = Some(e.to_string());
                        }
                    }
                }
            }

            // ── AI chat sidebar ──
            Message::ToggleChatSidebar => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_visible = !tab.chat_visible;
                }
            }
            Message::ChatInputChanged(val) => {
                self.chat_input = val;
            }
            Message::SendChat => {
                let input = self.chat_input.trim().to_string();
                if input.is_empty() || !self.ai_enabled {
                    return Task::none();
                }
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::User,
                            content: input,
                            timestamp: chrono::Utc::now(),
                        });
                        self.chat_input.clear();
                        self.chat_loading = true;

                        // Build AI config
                        let api_key = self.vault.as_ref()
                            .and_then(|v| v.get_ai_api_key().ok().flatten())
                            .unwrap_or_default();

                        // Get additional system prompt from settings
                        let extra_prompt = self.vault.as_ref()
                            .and_then(|v| v.get_setting("ai_system_prompt").ok().flatten());

                        let config = crate::ai::AiConfig {
                            provider: self.ai_provider.clone(),
                            model: self.ai_model.clone(),
                            api_key,
                            api_url: if self.ai_api_url.is_empty() {
                                None
                            } else {
                                Some(self.ai_api_url.clone())
                            },
                            system_prompt: extra_prompt,
                        };

                        // Get last ~50 lines of terminal output for context
                        let terminal_context = if let Ok(state) = tab.terminal.lock() {
                            let term = &state.backend.term;
                            let content = term.renderable_content();
                            let mut lines: Vec<String> = Vec::new();
                            let mut current_line = String::new();
                            let mut last_row = 0i32;
                            for item in content.display_iter {
                                let row = item.point.line.0;
                                if row != last_row && !current_line.is_empty() {
                                    lines.push(std::mem::take(&mut current_line));
                                    last_row = row;
                                }
                                let c = item.cell.c;
                                if c != '\0' {
                                    current_line.push(c);
                                }
                            }
                            if !current_line.is_empty() {
                                lines.push(current_line);
                            }
                            // Take last 50 lines
                            let start = lines.len().saturating_sub(50);
                            lines[start..].join("\n")
                        } else {
                            String::new()
                        };

                        // Build messages: inject terminal context as first user message
                        let mut messages: Vec<crate::ai::ChatMsg> = Vec::new();
                        if !terminal_context.is_empty() {
                            messages.push(crate::ai::ChatMsg {
                                role: "user".into(),
                                content: serde_json::Value::String(format!(
                                    "[Current terminal output (last ~50 lines)]\n```\n{}\n```",
                                    terminal_context
                                )),
                            });
                            messages.push(crate::ai::ChatMsg {
                                role: "assistant".into(),
                                content: serde_json::Value::String(
                                    "I can see the terminal output. How can I help?".into()
                                ),
                            });
                        }
                        // Add chat history
                        messages.extend(tab.chat_history.iter().map(|m| crate::ai::ChatMsg {
                            role: match m.role {
                                ChatRole::User => "user".into(),
                                ChatRole::Assistant => "assistant".into(),
                                ChatRole::System => "user".into(),
                            },
                            content: serde_json::Value::String(m.content.clone()),
                        }));

                        return Task::perform(
                            async move {
                                crate::ai::send_chat(&config, &messages).await
                            },
                            |result| match result {
                                Ok(crate::ai::AiResponse::Text(text)) => {
                                    Message::ChatResponse(text)
                                }
                                Ok(crate::ai::AiResponse::ToolUse {
                                    command, ..
                                }) => Message::ChatToolExec(command),
                                Err(e) => {
                                    Message::ChatResponse(format!("Error: {}", e))
                                }
                            },
                        );
                }
            }
            Message::ChatResponse(response) => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::Assistant,
                            content: response,
                            timestamp: chrono::Utc::now(),
                        });
                }
                self.chat_loading = false;
            }
            Message::ChatToolExec(command) => {
                // AI requested to execute a command in the terminal
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::System,
                            content: format!("$ {}", command),
                            timestamp: chrono::Utc::now(),
                        });

                        // Write the command to the terminal
                        let cmd_bytes = format!("{}\n", command);
                        if let Some(ref ssh) = tab.ssh_session {
                            let _ = ssh.write(cmd_bytes.as_bytes());
                        } else if let Ok(mut state) = tab.terminal.lock() {
                            state.write(cmd_bytes.as_bytes());
                        }

                        // Wait 1.5s for output, then capture terminal and send back to AI
                        let terminal = Arc::clone(&tab.terminal);
                        let api_key = self.vault.as_ref()
                            .and_then(|v| v.get_ai_api_key().ok().flatten())
                            .unwrap_or_default();
                        let extra_prompt = self.vault.as_ref()
                            .and_then(|v| v.get_setting("ai_system_prompt").ok().flatten());

                        let config = crate::ai::AiConfig {
                            provider: self.ai_provider.clone(),
                            model: self.ai_model.clone(),
                            api_key,
                            api_url: if self.ai_api_url.is_empty() { None } else { Some(self.ai_api_url.clone()) },
                            system_prompt: extra_prompt,
                        };

                        // Build message history including the tool result
                        let mut messages: Vec<crate::ai::ChatMsg> = tab.chat_history.iter().map(|m| crate::ai::ChatMsg {
                            role: match m.role {
                                ChatRole::User => "user".into(),
                                ChatRole::Assistant => "assistant".into(),
                                ChatRole::System => "user".into(),
                            },
                            content: serde_json::Value::String(m.content.clone()),
                        }).collect();

                        let cmd_clone = command.clone();

                        return Task::perform(
                            async move {
                                // Poll terminal until output stabilizes (no change for 800ms)
                                // or timeout after 15s
                                let poll_interval = std::time::Duration::from_millis(300);
                                let stable_threshold = std::time::Duration::from_millis(800);
                                let max_wait = std::time::Duration::from_secs(15);

                                let start_time = std::time::Instant::now();
                                let mut last_snapshot = String::new();
                                let mut stable_since = std::time::Instant::now();

                                // Initial delay to let command start
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                                loop {
                                    let snapshot = if let Ok(state) = terminal.lock() {
                                        let term = &state.backend.term;
                                        let content = term.renderable_content();
                                        let mut lines: Vec<String> = Vec::new();
                                        let mut current_line = String::new();
                                        let mut last_row = 0i32;
                                        for item in content.display_iter {
                                            let row = item.point.line.0;
                                            if row != last_row && !current_line.is_empty() {
                                                lines.push(std::mem::take(&mut current_line));
                                                last_row = row;
                                            }
                                            let c = item.cell.c;
                                            if c != '\0' { current_line.push(c); }
                                        }
                                        if !current_line.is_empty() { lines.push(current_line); }
                                        let start = lines.len().saturating_sub(40);
                                        lines[start..].join("\n")
                                    } else {
                                        break;
                                    };

                                    if snapshot != last_snapshot {
                                        last_snapshot = snapshot;
                                        stable_since = std::time::Instant::now();
                                    } else if stable_since.elapsed() >= stable_threshold {
                                        // Output stable for 800ms — command likely finished
                                        break;
                                    }

                                    if start_time.elapsed() >= max_wait {
                                        break; // Timeout
                                    }

                                    tokio::time::sleep(poll_interval).await;
                                }

                                // Send tool result back to AI
                                messages.push(crate::ai::ChatMsg {
                                    role: "user".into(),
                                    content: serde_json::Value::String(format!(
                                        "[Command executed: `{}`]\nOutput:\n```\n{}\n```\nPlease analyze the output and respond.",
                                        cmd_clone, last_snapshot
                                    )),
                                });

                                crate::ai::send_chat(&config, &messages).await
                            },
                            |result| match result {
                                Ok(crate::ai::AiResponse::Text(text)) => Message::ChatResponse(text),
                                Ok(crate::ai::AiResponse::ToolUse { command, .. }) => Message::ChatToolExec(command),
                                Err(e) => Message::ChatResponse(format!("Error: {}", e)),
                            },
                        );
                }
            }
            Message::ChatToolResult(output) => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::System,
                            content: output,
                            timestamp: chrono::Utc::now(),
                        });
                }
            }

            // ── MCP ──
            Message::EditorToggleMcpEnabled => {
                self.editor_form.mcp_enabled = !self.editor_form.mcp_enabled;
            }
            Message::EditorAddPortForward => {
                self.editor_form.port_forwards.push(PortForwardForm::default());
            }
            Message::EditorRemovePortForward(i) => {
                if i < self.editor_form.port_forwards.len() {
                    self.editor_form.port_forwards.remove(i);
                }
            }
            Message::EditorPortFwdLocalPortChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.local_port = v;
                }
            }
            Message::EditorPortFwdRemoteHostChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.remote_host = v;
                }
            }
            Message::EditorPortFwdRemotePortChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.remote_port = v;
                }
            }
            Message::ToggleMcpServer => {
                self.mcp_server_enabled = !self.mcp_server_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("mcp_server_enabled", if self.mcp_server_enabled { "true" } else { "false" });
                }
            }
            Message::ShowMcpInfo => {
                self.show_mcp_info = true;
                self.mcp_config_copied = false;
            }
            Message::HideMcpInfo => {
                self.show_mcp_info = false;
                self.mcp_config_copied = false;
            }
            Message::CopyMcpConfig => {
                self.mcp_config_copied = true;
                return iced::clipboard::write(mcp_config_json());
            }
            Message::InstallMcpConfig => {
                self.mcp_install_status = None;
                return Task::perform(
                    async { install_mcp_config_to_file() },
                    Message::InstallMcpConfigResult,
                );
            }
            Message::InstallMcpConfigResult(result) => {
                self.mcp_install_status = Some(result);
            }

            // ── Sync ──
            Message::SyncToggleEnabled => {
                self.sync_enabled = !self.sync_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_enabled", if self.sync_enabled { "true" } else { "false" });
                }
            }
            Message::SyncModeChanged(v) => {
                self.sync_mode = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_mode", &v);
                }
            }
            Message::SyncDeviceNameChanged(v) => {
                self.sync_device_name = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_device_name", &v);
                }
            }
            Message::SyncSignalingUrlChanged(v) => {
                self.sync_signaling_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_url", &v);
                }
            }
            Message::SyncRelayUrlChanged(v) => {
                self.sync_relay_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_relay_url", &v);
                }
            }
            Message::SyncListenPortChanged(v) => {
                self.sync_listen_port = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_listen_port", &v);
                }
            }
            Message::SyncStartPairing => {
                let code = oryxis_sync::crypto::generate_pairing_code();
                self.sync_pairing_code = Some(code);
            }
            Message::SyncUnpairDevice(peer_id) => {
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_sync_peer(&peer_id);
                    self.sync_peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            Message::SyncNow => {
                self.sync_status = Some("Sync triggered...".into());
            }

            // ── Export / Import ──
            Message::ExportVault => {
                self.show_export_dialog = true;
                self.export_password = String::new();
                self.export_include_keys = true;
                self.export_status = None;
            }
            Message::ExportPasswordChanged(v) => {
                self.export_password = v;
            }
            Message::ExportToggleKeys => {
                self.export_include_keys = !self.export_include_keys;
            }
            Message::ExportConfirm => {
                if self.export_password.is_empty() {
                    self.export_status = Some(Err("Password is required".into()));
                    return Task::none();
                }
                if let Some(vault) = &self.vault {
                    let options = oryxis_vault::ExportOptions {
                        include_private_keys: self.export_include_keys,
                        filter: oryxis_vault::ExportFilter::All,
                    };
                    match oryxis_vault::export_vault(vault, &self.export_password, options) {
                        Ok(data) => {
                            // Open save dialog
                            let dialog = rfd::FileDialog::new()
                                .set_title("Export Vault")
                                .add_filter("Oryxis Export", &["oryxis"])
                                .set_file_name("vault.oryxis")
                                .save_file();
                            if let Some(path) = dialog {
                                match std::fs::write(&path, &data) {
                                    Ok(()) => {
                                        self.export_status = Some(Ok(format!("Exported to {}", path.display())));
                                    }
                                    Err(e) => {
                                        self.export_status = Some(Err(format!("Write failed: {}", e)));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.export_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            Message::ExportCompleted(result) => {
                self.export_status = Some(result);
            }
            Message::ImportVault => {
                self.import_status = None;
                self.import_password = String::new();
                self.import_file_data = None;
                // Open file picker
                let dialog = rfd::FileDialog::new()
                    .set_title("Import Vault")
                    .add_filter("Oryxis Export", &["oryxis"])
                    .pick_file();
                if let Some(path) = dialog {
                    match std::fs::read(&path) {
                        Ok(data) => {
                            if oryxis_vault::is_valid_export(&data) {
                                self.import_file_data = Some(data);
                                self.show_import_dialog = true;
                            } else {
                                self.import_status = Some(Err("Invalid export file".into()));
                            }
                        }
                        Err(e) => {
                            self.import_status = Some(Err(format!("Read failed: {}", e)));
                        }
                    }
                }
            }
            Message::ImportFileLoaded(data) => {
                self.import_file_data = Some(data);
                self.show_import_dialog = true;
            }
            Message::ImportPasswordChanged(v) => {
                self.import_password = v;
            }
            Message::ImportConfirm => {
                if self.import_password.is_empty() {
                    self.import_status = Some(Err("Password is required".into()));
                    return Task::none();
                }
                if let (Some(vault), Some(data)) = (&self.vault, &self.import_file_data) {
                    match oryxis_vault::import_vault(vault, data, &self.import_password) {
                        Ok(result) => {
                            let msg = format!(
                                "Imported: {} connections, {} keys, {} groups, {} identities, {} snippets, {} known hosts",
                                result.connections_added + result.connections_updated,
                                result.keys_added,
                                result.groups_added,
                                result.identities_added + result.identities_updated,
                                result.snippets_added,
                                result.known_hosts_added,
                            );
                            self.import_status = Some(Ok(msg));
                            self.show_import_dialog = false;
                            self.import_file_data = None;
                            self.load_data_from_vault();
                        }
                        Err(oryxis_vault::VaultError::InvalidPassword) => {
                            self.import_status = Some(Err("Wrong password".into()));
                        }
                        Err(e) => {
                            self.import_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            Message::ImportCompleted(result) => {
                self.import_status = Some(result);
                if self.import_status.as_ref().is_some_and(|r| r.is_ok()) {
                    self.show_import_dialog = false;
                    self.import_file_data = None;
                    self.load_data_from_vault();
                }
            }
            Message::ExportImportDismiss => {
                self.show_export_dialog = false;
                self.show_import_dialog = false;
                self.export_status = None;
                self.import_status = None;
                self.import_file_data = None;
            }

            // ── Share ──
            Message::ShareConnection(idx) => {
                self.overlay = None;
                if let Some(conn) = self.connections.get(idx) {
                    self.share_filter = Some(oryxis_vault::ExportFilter::Hosts(vec![conn.id]));
                    self.show_share_dialog = true;
                    self.share_password = String::new();
                    self.share_include_keys = false;
                    self.share_status = None;
                }
            }
            Message::ShareGroup(group_id) => {
                self.overlay = None;
                self.share_filter = Some(oryxis_vault::ExportFilter::Group(group_id));
                self.show_share_dialog = true;
                self.share_password = String::new();
                self.share_include_keys = false;
                self.share_status = None;
            }
            Message::SharePasswordChanged(v) => {
                self.share_password = v;
            }
            Message::ShareToggleKeys => {
                self.share_include_keys = !self.share_include_keys;
            }
            Message::ShareConfirm => {
                if self.share_password.is_empty() {
                    self.share_status = Some(Err("Password is required".into()));
                    return Task::none();
                }
                if let (Some(vault), Some(filter)) = (&self.vault, &self.share_filter) {
                    let options = oryxis_vault::ExportOptions {
                        include_private_keys: self.share_include_keys,
                        filter: filter.clone(),
                    };
                    match oryxis_vault::export_vault(vault, &self.share_password, options) {
                        Ok(data) => {
                            let dialog = rfd::FileDialog::new()
                                .set_title("Share")
                                .add_filter("Oryxis Export", &["oryxis"])
                                .set_file_name("shared.oryxis")
                                .save_file();
                            if let Some(path) = dialog {
                                match std::fs::write(&path, &data) {
                                    Ok(()) => {
                                        self.share_status = Some(Ok(format!("Saved to {}", path.display())));
                                        self.show_share_dialog = false;
                                    }
                                    Err(e) => {
                                        self.share_status = Some(Err(format!("Write failed: {}", e)));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.share_status = Some(Err(e.to_string()));
                        }
                    }
                }
            }
            Message::ShareDismiss => {
                self.show_share_dialog = false;
                self.share_filter = None;
                self.share_status = None;
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let events = iced::event::listen_with(|event, _status, _window| {
            match event {
                iced::event::Event::Keyboard(ke) => Some(Message::KeyboardEvent(ke)),
                iced::event::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::MouseMoved(position))
                }
                iced::event::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size))
                }
                _ => None,
            }
        });
        // 30-second poll for silent auto-reconnect of disconnected SSH tabs.
        let auto_reconnect = iced::time::every(std::time::Duration::from_secs(30))
            .map(|_| Message::AutoReconnectTick);

        // 100 ms tick that drives the pulsing "loading" ring on the active
        // connection step. Only runs while a connection is in progress and
        // hasn't failed — no perpetual re-renders on idle.
        let mut subs = vec![events, auto_reconnect];
        let is_connecting = self
            .connecting
            .as_ref()
            .map(|p| !p.failed)
            .unwrap_or(false);
        if is_connecting {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(100))
                    .map(|_| Message::ConnectAnimTick),
            );
        }
        Subscription::batch(subs)
    }

    // =======================================================================
    // View
    // =======================================================================

    pub fn view(&self) -> Element<'_, Message> {
        let base = match self.vault_state {
            VaultState::Loading => self.view_vault_error("Failed to open vault database"),
            VaultState::NeedSetup => self.view_vault_setup(),
            VaultState::Locked => self.view_vault_unlock(),
            VaultState::Unlocked => self.view_main(),
        };

        // Auto-update modal is application-level so it surfaces on the lock
        // screen too. Rendered via Stack with a scrim that carves out the
        // top 28 px so the window chrome stays draggable.
        if self.pending_update.is_some() {
            use iced::widget::{column, container, Space, Stack};
            use iced::{Background, Color, Length};
            let modal = self.view_update_modal();
            let scrim: Element<'_, Message> = column![
                // Reserve the chrome bar area so drag / min / max / close
                // still land on the underlying buttons.
                Space::new().height(Length::Fixed(28.0)),
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                        ..Default::default()
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
            return Stack::new()
                .push(base)
                .push(scrim)
                .push(modal)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }
        base
    }

    // -- Vault screens --









}

/// Open an external URL in the user's default browser. Best-effort — errors
/// are ignored, since we fall back to copying the URL to the clipboard in
/// the UI if opening fails is something the user notices.
fn open_in_browser(url: &str) -> Result<(), std::io::Error> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}
