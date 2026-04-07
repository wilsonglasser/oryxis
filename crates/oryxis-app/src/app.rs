use iced::border::Radius;
use iced::keyboard;
use iced::widget::{
    button, canvas, column, container, image, pick_list, row, scrollable, text, text_editor,
    text_input, MouseArea, Space,
};
use iced::futures::SinkExt;
use iced::{Background, Border, Color, Element, Length, Padding, Subscription, Task, Theme};
use iced::widget::button::Status as BtnStatus;

use oryxis_core::models::connection::{AuthMethod, Connection};
use oryxis_core::models::group::Group;
use oryxis_core::models::identity::Identity;
use oryxis_core::models::key::SshKey;
use oryxis_ssh::{SshEngine, SshSession};
use oryxis_terminal::widget::{TerminalState, TerminalView};
use oryxis_vault::{VaultError, VaultStore};

use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use crate::theme::OryxisColors;

// Layout constants
const DEFAULT_TERM_COLS: u32 = 120;
const DEFAULT_TERM_ROWS: u32 = 40;
const PANEL_WIDTH: f32 = 420.0;
const SIDEBAR_WIDTH: f32 = 180.0;
const CARD_WIDTH: f32 = 280.0;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A terminal tab — either a local shell or an SSH session.
struct TerminalTab {
    _id: Uuid,
    label: String,
    terminal: Arc<Mutex<TerminalState>>,
    /// SSH session handle (None for local shell).
    ssh_session: Option<Arc<SshSession>>,
}

/// Connection editor form state.
#[derive(Debug, Clone)]
struct ConnectionForm {
    label: String,
    hostname: String,
    port: String,
    username: String,
    password: String,
    auth_method: AuthMethod,
    group_name: String,
    selected_key: Option<String>,
    jump_host: Option<String>,  // label of jump host connection
    /// Selected identity label (if any).
    selected_identity: Option<String>,
    /// If editing, the connection ID.
    editing_id: Option<Uuid>,
    /// Whether the connection already has a password stored in the vault.
    has_existing_password: bool,
    /// Whether the user has modified the password field.
    password_touched: bool,
    /// Whether to show the password in plain text.
    password_visible: bool,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            label: String::new(),
            hostname: String::new(),
            port: "22".into(),
            username: String::new(),
            password: String::new(),
            auth_method: AuthMethod::Auto,
            group_name: String::new(),
            selected_key: None,
            jump_host: None,
            selected_identity: None,
            editing_id: None,
            has_existing_password: false,
            password_touched: false,
            password_visible: false,
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct Oryxis {
    // Vault
    vault: Option<VaultStore>,
    vault_state: VaultState,
    vault_password_input: String,
    vault_error: Option<String>,
    logo_handle: image::Handle,
    logo_small_handle: image::Handle,

    // Data
    connections: Vec<Connection>,
    groups: Vec<Group>,

    // UI state
    active_view: View,
    active_group: Option<Uuid>,  // None = root, Some(id) = inside folder
    host_search: String,
    quick_host_input: String,

    // Tabs
    tabs: Vec<TerminalTab>,
    active_tab: Option<usize>,
    connecting: Option<ConnectionProgress>,

    // Connection editor
    show_host_panel: bool,
    editor_form: ConnectionForm,
    host_panel_error: Option<String>,

    // Card hover & context menu
    hovered_card: Option<usize>,
    card_context_menu: Option<usize>,

    // Keys
    keys: Vec<SshKey>,
    show_key_panel: bool,
    key_import_label: String,
    key_import_content: text_editor::Content,
    key_import_pem: String,  // raw string for import
    key_error: Option<String>,
    key_success: Option<String>,
    key_context_menu: Option<usize>,
    editing_key_id: Option<Uuid>,
    key_search: String,

    // Identities
    identities: Vec<Identity>,
    show_identity_panel: bool,
    identity_form_label: String,
    identity_form_username: String,
    identity_form_password: String,
    identity_form_key: Option<String>,
    identity_form_password_visible: bool,
    identity_form_password_touched: bool,
    identity_form_has_existing_password: bool,
    editing_identity_id: Option<Uuid>,
    identity_context_menu: Option<usize>,
    show_keychain_add_menu: bool,

    // Snippets
    snippets: Vec<oryxis_core::models::snippet::Snippet>,
    show_snippet_panel: bool,
    snippet_label: String,
    snippet_command: String,
    snippet_editing_id: Option<Uuid>,
    snippet_error: Option<String>,

    // Known hosts & logs
    known_hosts: Vec<oryxis_core::models::known_host::KnownHost>,
    logs: Vec<oryxis_core::models::log_entry::LogEntry>,

    // Terminal theme
    terminal_theme: oryxis_terminal::TerminalTheme,
    terminal_font_size: f32,

    // Settings
    settings_section: SettingsSection,
    setting_copy_on_select: bool,
    setting_bold_is_bright: bool,
    setting_bell_sound: bool,
    setting_keyword_highlight: bool,
    setting_keepalive_interval: String,
    setting_scrollback_rows: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VaultState {
    Loading,
    NeedSetup,
    Locked,
    Unlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Terminal,
    Keys,
    Snippets,
    KnownHosts,
    History,
    Settings,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Terminal,
    Theme,
    About,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Vault
    VaultPasswordChanged(String),
    VaultUnlock,
    VaultSetup,

    // Navigation
    ChangeView(View),
    QuickHostInput(String),
    QuickHostContinue,
    OpenGroup(Uuid),
    BackToRoot,
    HostSearchChanged(String),

    // Tabs
    SelectTab(usize),
    CloseTab(usize),

    // Terminal I/O
    PtyOutput(usize, Vec<u8>),  // (tab_index, bytes)
    KeyboardEvent(keyboard::Event),

    // Card interactions
    CardHovered(usize),
    CardUnhovered,
    ShowCardMenu(usize),
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

    // History
    ClearLogs,

    // Settings
    LockVault,
    TerminalThemeChanged(String),
    AppThemeChanged(String),
    TerminalFontSizeIncrease,
    TerminalFontSizeDecrease,
    ChangeSettingsSection(SettingsSection),
    ToggleCopyOnSelect,
    ToggleBoldIsBright,
    ToggleBellSound,
    ToggleKeywordHighlight,
    SettingKeepaliveChanged(String),
    SettingScrollbackChanged(String),

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
    HideIdentityMenu,
    ToggleKeychainAddMenu,

    // Connection identity
    EditorIdentityChanged(String),
}

/// Internal message type for SSH connection streams.
enum SshStreamMsg {
    Progress(ConnectionStep, String), // (step, log message)
    Connected(Arc<SshSession>),
    NewKnownHosts(Vec<oryxis_core::models::known_host::KnownHost>),
    Data(Vec<u8>),
    Error(String),
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStep {
    Connecting,   // step 1: TCP/proxy/jump
    Handshake,    // step 2: SSH handshake + host key
    Authenticating, // step 3: auth
}

/// Connection progress state for the connecting tab.
#[derive(Clone)]
struct ConnectionProgress {
    label: String,
    hostname: String,
    step: ConnectionStep,
    logs: Vec<(ConnectionStep, String)>,
    failed: bool,
    connection_idx: usize,
    tab_idx: usize,
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

impl Oryxis {
    pub fn boot() -> (Self, Task<Message>) {
        let vault = VaultStore::open_default().ok();
        let vault_state = match &vault {
            None => VaultState::Loading,
            Some(v) => {
                if v.has_master_password().unwrap_or(false) {
                    VaultState::Locked
                } else {
                    VaultState::NeedSetup
                }
            }
        };

        (
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
                tabs: Vec::new(),
                active_tab: None,
                connecting: None,
                show_host_panel: false,
                editor_form: ConnectionForm::default(),
                host_panel_error: None,
                hovered_card: None,
                card_context_menu: None,
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
                show_snippet_panel: false,
                snippet_label: String::new(),
                snippet_command: String::new(),
                snippet_editing_id: None,
                snippet_error: None,
                terminal_theme: oryxis_terminal::TerminalTheme::OryxisDark,
                terminal_font_size: 14.0,
                settings_section: SettingsSection::Terminal,
                setting_copy_on_select: true,
                setting_bold_is_bright: true,
                setting_bell_sound: false,
                setting_keyword_highlight: true,
                setting_keepalive_interval: "0".into(),
                setting_scrollback_rows: "10000".into(),
            },
            Task::none(),
        )
    }

    fn load_data_from_vault(&mut self) {
        if let Some(vault) = &self.vault {
            self.connections = vault.list_connections().unwrap_or_default();
            self.groups = vault.list_groups().unwrap_or_default();
            self.keys = vault.list_keys().unwrap_or_default();
            self.identities = vault.list_identities().unwrap_or_default();
            self.snippets = vault.list_snippets().unwrap_or_default();
            self.known_hosts = vault.list_known_hosts().unwrap_or_default();
            self.logs = vault.list_logs(200).unwrap_or_default();
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
            Message::ShowCardMenu(idx) => {
                self.card_context_menu = if self.card_context_menu == Some(idx) { None } else { Some(idx) };
            }
            Message::HideCardMenu => {
                self.card_context_menu = None;
            }

            // -- Tabs --
            Message::SelectTab(idx) => {
                if idx < self.tabs.len() {
                    self.active_tab = Some(idx);
                    self.active_view = View::Terminal;
                }
            }
            Message::CloseTab(idx) => {
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

            // -- Terminal I/O --
            Message::PtyOutput(tab_idx, bytes) => {
                if let Some(tab) = self.tabs.get(tab_idx)
                    && let Ok(mut state) = tab.terminal.lock() {
                        state.process(&bytes);
                    }
            }
            Message::KeyboardEvent(event) => {
                if let Some(tab_idx) = self.active_tab
                    && let keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        text: text_opt,
                        ..
                    } = event
                    {
                        let bytes = key_to_named_bytes(&key, &modifiers).or_else(|| {
                            if modifiers.control() {
                                ctrl_key_bytes(&key)
                            } else {
                                text_opt.map(|t| t.as_bytes().to_vec())
                            }
                        });

                        if let Some(bytes) = bytes
                            && let Some(tab) = self.tabs.get(tab_idx) {
                                if let Some(ref ssh) = tab.ssh_session {
                                    let _ = ssh.write(&bytes);
                                } else if let Ok(mut state) = tab.terminal.lock() {
                                    state.write(&bytes);
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
                    };
                }
            }
            Message::EditorLabelChanged(v) => self.editor_form.label = v,
            Message::EditorHostnameChanged(v) => self.editor_form.hostname = v,
            Message::EditorPortChanged(v) => self.editor_form.port = v,
            Message::EditorUsernameChanged(v) => self.editor_form.username = v,
            Message::EditorPasswordChanged(v) => {
                self.editor_form.password_touched = true;
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

                            self.tabs.push(TerminalTab {
                                _id: Uuid::new_v4(),
                                label: label.clone(),
                                terminal: Arc::clone(&terminal),
                                ssh_session: None,
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

                            // TOFU callback
                            let known_hosts_snapshot: Arc<Mutex<Vec<oryxis_core::models::known_host::KnownHost>>> =
                                Arc::new(Mutex::new(self.known_hosts.clone()));
                            let new_hosts: Arc<Mutex<Vec<oryxis_core::models::known_host::KnownHost>>> =
                                Arc::new(Mutex::new(Vec::new()));
                            let kh_ref = known_hosts_snapshot.clone();
                            let new_ref = new_hosts.clone();
                            let host_key_cb: oryxis_ssh::HostKeyCallback = Arc::new(move |host, port, key_type, fingerprint| {
                                let hosts = kh_ref.lock().unwrap();
                                if let Some(existing) = hosts.iter().find(|h| h.hostname == host && h.port == port) {
                                    if existing.fingerprint != fingerprint {
                                        return false;
                                    }
                                    return true;
                                }
                                let kh = oryxis_core::models::known_host::KnownHost::new(host, port, key_type, fingerprint);
                                new_ref.lock().unwrap().push(kh);
                                true
                            });

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
                                    let engine = SshEngine::new().with_host_key_cb(host_key_cb);

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

                                    // Step 3: Open PTY session
                                    match engine.open_session(handle, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS).await {
                                        Ok((session, mut rx)) => {
                                            let session = Arc::new(session);
                                            let _ = sender.send(SshStreamMsg::Connected(session.clone())).await;
                                            let new_kh = new_hosts.lock().unwrap().drain(..).collect::<Vec<_>>();
                                            if !new_kh.is_empty() {
                                                let _ = sender.send(SshStreamMsg::NewKnownHosts(new_kh)).await;
                                            }
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
            Message::SshConnected(tab_idx, session) => {
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    tab.ssh_session = Some(session);
                    let label = tab.label.clone();
                    tracing::info!("SSH connected: {}", label);
                    if let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Connected, "Session established",
                        );
                        let _ = vault.add_log(&entry);
                    }
                }
                // Clear progress, show terminal
                self.connecting = None;
            }
            Message::SshDisconnected(tab_idx) => {
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    let label = tab.label.replace(" (disconnected)", "");
                    // Log
                    if let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Disconnected, "Session ended",
                        );
                        let _ = vault.add_log(&entry);
                    }
                    tab.label = format!("{} (disconnected)", label);
                    tab.ssh_session = None;
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

            // -- History --
            Message::ClearLogs => {
                if let Some(vault) = &self.vault {
                    let _ = vault.clear_logs();
                    self.load_data_from_vault();
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
            Message::AppThemeChanged(name) => {
                use crate::theme::AppTheme;
                if let Some(theme) = AppTheme::ALL.iter().find(|t| t.name() == name) {
                    AppTheme::set_active(*theme);
                    // Map app theme to terminal palette
                    let term_theme = match theme {
                        AppTheme::OryxisDark => oryxis_terminal::TerminalTheme::OryxisDark,
                        AppTheme::OryxisLight => oryxis_terminal::TerminalTheme::OryxisDark, // TODO: light terminal
                        AppTheme::Dracula => oryxis_terminal::TerminalTheme::Dracula,
                        AppTheme::Nord => oryxis_terminal::TerminalTheme::Nord,
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
            Message::LockVault => {
                if let Some(vault) = &mut self.vault {
                    vault.lock();
                    self.vault_state = VaultState::Locked;
                    self.connections.clear();
                    self.keys.clear();
                    self.snippets.clear();
                    self.groups.clear();
                    self.tabs.clear();
                    self.active_tab = None;
                    self.active_view = View::Dashboard;
                }
            }

            Message::OpenLocalShell => {
                match TerminalState::new(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16) {
                    Ok((mut state, rx)) => {
                        state.palette = self.terminal_theme.palette();
                        let tab_idx = self.tabs.len();
                        self.tabs.push(TerminalTab {
                            _id: Uuid::new_v4(),
                            label: "Local Shell".into(),
                            terminal: Arc::new(Mutex::new(state)),
                            ssh_session: None,
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
                self.show_key_panel = true;
                self.key_import_label.clear();
                self.key_import_content = text_editor::Content::new();
                self.key_import_pem.clear();
                self.key_error = None;
                self.key_success = None;
                self.editing_key_id = None;
                self.key_context_menu = None;
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
            }
            Message::ShowKeyMenu(idx) => {
                self.key_context_menu = if self.key_context_menu == Some(idx) { None } else { Some(idx) };
            }
            Message::HideKeyMenu => {
                self.key_context_menu = None;
                self.identity_context_menu = None;
                self.show_keychain_add_menu = false;
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
            }
            Message::ShowIdentityMenu(idx) => {
                self.identity_context_menu = if self.identity_context_menu == Some(idx) { None } else { Some(idx) };
            }
            Message::HideIdentityMenu => {
                self.identity_context_menu = None;
            }
            Message::ToggleKeychainAddMenu => {
                self.show_keychain_add_menu = !self.show_keychain_add_menu;
            }

            // ── Connection identity ──
            Message::EditorIdentityChanged(v) => {
                if v == "(none)" {
                    self.editor_form.selected_identity = None;
                } else {
                    self.editor_form.selected_identity = Some(v);
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().map(Message::KeyboardEvent)
    }

    // =======================================================================
    // View
    // =======================================================================

    pub fn view(&self) -> Element<'_, Message> {
        match self.vault_state {
            VaultState::Loading => self.view_vault_error("Failed to open vault database"),
            VaultState::NeedSetup => self.view_vault_setup(),
            VaultState::Locked => self.view_vault_unlock(),
            VaultState::Unlocked => self.view_main(),
        }
    }

    // -- Vault screens --

    fn view_vault_setup(&self) -> Element<'_, Message> {
        let logo = image(self.logo_handle.clone())
            .width(64)
            .height(64);
        let title = text("Welcome to Oryxis").size(28).color(OryxisColors::t().text_primary);
        let subtitle = text("Create a master password to secure your vault.")
            .size(14)
            .color(OryxisColors::t().text_secondary);

        let input = text_input("Master password...", &self.vault_password_input)
            .on_input(Message::VaultPasswordChanged)
            .on_submit(Message::VaultSetup)
            .secure(true)
            .padding(12)
            .width(300);

        let btn = styled_button("Create Vault", Message::VaultSetup, OryxisColors::t().accent);

        let error = if let Some(err) = &self.vault_error {
            Element::from(text(err.clone()).size(13).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        container(
            column![logo, Space::new().height(16), title, Space::new().height(8), subtitle, Space::new().height(24), input, Space::new().height(12), btn, Space::new().height(8), error]
                .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_vault_unlock(&self) -> Element<'_, Message> {
        let logo = image(self.logo_handle.clone())
            .width(64)
            .height(64);
        let title = text("Oryxis").size(28).color(OryxisColors::t().accent);
        let subtitle = text("Enter your master password to unlock.")
            .size(14)
            .color(OryxisColors::t().text_secondary);

        let input = text_input("Master password...", &self.vault_password_input)
            .on_input(Message::VaultPasswordChanged)
            .on_submit(Message::VaultUnlock)
            .secure(true)
            .padding(12)
            .width(300);

        let btn = styled_button("Unlock", Message::VaultUnlock, OryxisColors::t().accent);

        let error = if let Some(err) = &self.vault_error {
            Element::from(text(err.clone()).size(13).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        container(
            column![logo, Space::new().height(16), title, Space::new().height(8), subtitle, Space::new().height(24), input, Space::new().height(12), btn, Space::new().height(8), error]
                .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_vault_error(&self, msg: &str) -> Element<'_, Message> {
        let msg = msg.to_string();
        container(
            text(msg).size(16).color(OryxisColors::t().error),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    // -- Main layout --

    fn view_main(&self) -> Element<'_, Message> {
        let sidebar = self.view_sidebar();
        let tab_bar = self.view_tab_bar();
        let content = self.view_content();
        let status_bar = self.view_status_bar();

        let right_side = column![tab_bar, content].height(Length::Fill);
        let main_row = row![sidebar, right_side].height(Length::Fill);
        let layout = column![main_row, status_bar];

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            })
            .into()
    }

    fn view_tab_bar(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = Vec::new();

        // Navigation tabs (grid views)
        let nav_label = match self.active_view {
            View::Dashboard => "Hosts",
            View::Keys => "Keychain",
            View::Snippets => "Snippets",
            View::KnownHosts => "Known Hosts",
            View::History => "History",
            View::Settings => "Settings",
            View::Terminal => "",
        };
        if !nav_label.is_empty() {
            let nav_bg = if self.active_tab.is_none() { OryxisColors::t().bg_surface } else { Color::TRANSPARENT };
            let nav_fg = if self.active_tab.is_none() { OryxisColors::t().accent } else { OryxisColors::t().text_muted };
            let tab = button(
                text(nav_label).size(12).color(nav_fg),
            )
            .on_press(Message::ChangeView(self.active_view))
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
            .style(move |_, _| button::Style {
                background: Some(Background::Color(nav_bg)),
                border: Border {
                    radius: Radius { top_left: 6.0, top_right: 6.0, bottom_left: 0.0, bottom_right: 0.0 },
                    ..Default::default()
                },
                ..Default::default()
            });
            items.push(tab.into());
        }

        // Terminal session tabs
        for (idx, tab) in self.tabs.iter().enumerate() {
            let is_active = self.active_tab == Some(idx);
            let tab_bg = if is_active { OryxisColors::t().bg_surface } else { Color::TRANSPARENT };
            let tab_fg = if is_active { OryxisColors::t().text_primary } else { OryxisColors::t().text_muted };

            let close_btn = button(text("x").size(10).color(OryxisColors::t().text_muted))
                .on_press(Message::CloseTab(idx))
                .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });

            let tab_btn = button(
                row![
                    text(&tab.label).size(12).color(tab_fg),
                    Space::new().width(8),
                    close_btn,
                ].align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 14.0 })
            .on_press(Message::SelectTab(idx))
            .style(move |_, _| button::Style {
                background: Some(Background::Color(tab_bg)),
                border: Border {
                    radius: Radius { top_left: 6.0, top_right: 6.0, bottom_left: 0.0, bottom_right: 0.0 },
                    ..Default::default()
                },
                ..Default::default()
            });

            items.push(tab_btn.into());
        }

        items.push(Space::new().width(Length::Fill).into());

        container(row(items).align_y(iced::Alignment::Center))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                ..Default::default()
            })
            .into()
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        // Logo
        let logo = image(self.logo_small_handle.clone())
            .width(28)
            .height(28);
        let header = container(
            row![
                logo,
                Space::new().width(10),
                text("ORYXIS").size(16).color(OryxisColors::t().accent),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 16.0, bottom: 16.0, left: 16.0 })
        .width(Length::Fill);

        // Navigation items with pill-shaped active state
        let nav_buttons: Vec<Element<'_, Message>> = vec![
            sidebar_nav_btn(iced_fonts::bootstrap::hdd_network(), "Hosts", View::Dashboard, self.active_view == View::Dashboard && self.active_tab.is_none()),
            sidebar_nav_btn(iced_fonts::bootstrap::key(), "Keychain", View::Keys, self.active_view == View::Keys && self.active_tab.is_none()),
            sidebar_nav_btn(iced_fonts::bootstrap::code_square(), "Snippets", View::Snippets, self.active_view == View::Snippets && self.active_tab.is_none()),
            sidebar_nav_btn(iced_fonts::bootstrap::shield_check(), "Known Hosts", View::KnownHosts, self.active_view == View::KnownHosts && self.active_tab.is_none()),
            sidebar_nav_btn(iced_fonts::bootstrap::clock_history(), "History", View::History, self.active_view == View::History && self.active_tab.is_none()),
            sidebar_nav_btn(iced_fonts::bootstrap::gear(), "Settings", View::Settings, self.active_view == View::Settings && self.active_tab.is_none()),
        ];

        // Local shell shortcut at bottom
        let local_btn = button(
            container(
                row![
                    text("+").size(13).color(OryxisColors::t().text_muted),
                    Space::new().width(10),
                    text("Local Shell").size(12).color(OryxisColors::t().text_muted),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .on_press(Message::OpenLocalShell)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            border: Border { radius: Radius::from(10.0), ..Default::default() },
            ..Default::default()
        });

        let sidebar_content = column![
            header,
            column(nav_buttons),
            Space::new().height(Length::Fill),
            container(local_btn)
                .padding(Padding { top: 0.0, right: 8.0, bottom: 12.0, left: 8.0 }),
        ]
        .width(Length::Fill);

        container(sidebar_content)
            .width(SIDEBAR_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                ..Default::default()
            })
            .into()
    }

    fn view_content(&self) -> Element<'_, Message> {
        // If a terminal tab is active, show terminal
        // Otherwise show the grid view for the current nav item
        let content: Element<'_, Message> = if self.connecting.is_some() && self.active_tab.is_some() {
            self.view_connection_progress()
        } else if self.active_tab.is_some() && self.connecting.is_none() {
            self.view_terminal()
        } else {
            match self.active_view {
                View::Dashboard => self.view_dashboard(),
                View::Keys => self.view_keys(),
                View::Snippets => self.view_snippets(),
                View::KnownHosts => self.view_known_hosts(),
                View::History => self.view_history(),
                View::Settings => self.view_settings(),
                View::Terminal => self.view_terminal(),
            }
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            })
            .into()
    }

    fn view_dashboard(&self) -> Element<'_, Message> {
        // ── Toolbar ──
        let toolbar_left: Element<'_, Message> = if let Some(gid) = self.active_group {
            let group_name = self.groups.iter()
                .find(|g| g.id == gid)
                .map(|g| g.label.as_str())
                .unwrap_or("Group");
            row![
                button(
                    row![
                        iced_fonts::bootstrap::arrow_left().size(14).color(OryxisColors::t().accent),
                        Space::new().width(6),
                        text("All Hosts").size(14).color(OryxisColors::t().accent),
                    ].align_y(iced::Alignment::Center),
                )
                .on_press(Message::BackToRoot)
                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                }),
                text("/").size(16).color(OryxisColors::t().text_muted),
                Space::new().width(8),
                iced_fonts::bootstrap::folder_fill().size(16).color(OryxisColors::t().accent),
                Space::new().width(6),
                text(group_name).size(16).color(OryxisColors::t().text_primary),
            ].align_y(iced::Alignment::Center).into()
        } else {
            text("Hosts").size(20).color(OryxisColors::t().text_primary).into()
        };

        let toolbar = container(
            row![
                toolbar_left,
                Space::new().width(Length::Fill),
                button(
                    container(
                        row![
                            text("+").size(12).color(OryxisColors::t().text_primary),
                            Space::new().width(4),
                            text("HOST").size(12).color(OryxisColors::t().text_primary),
                        ].align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 6.0, right: 14.0, bottom: 7.0, left: 14.0 }),
                )
                .on_press(Message::ShowNewConnection)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Search bar ──
        let search_bar = container(
            text_input("Search hosts...", &self.host_search)
                .on_input(Message::HostSearchChanged)
                .padding(10)
                .size(13)
                .style(|_, status| text_input::Style {
                    background: Background::Color(OryxisColors::t().bg_surface),
                    border: Border {
                        radius: Radius::from(8.0),
                        width: 1.0,
                        color: match status {
                            text_input::Status::Focused { .. } => OryxisColors::t().accent,
                            _ => OryxisColors::t().border,
                        },
                    },
                    icon: OryxisColors::t().text_muted,
                    placeholder: OryxisColors::t().text_muted,
                    value: OryxisColors::t().text_primary,
                    selection: OryxisColors::t().accent,
                }),
        )
        .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
        .width(Length::Fill);

        // ── Status ──
        let status: Element<'_, Message> = if let Some(err) = &self.host_panel_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        // ── Host cards grid ──
        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if self.connections.is_empty() {
            // Termius-style empty state — centered "Create host" with input
            let has_input = !self.quick_host_input.is_empty();
            let btn_bg = if has_input { OryxisColors::t().success } else { OryxisColors::t().bg_surface };

            let empty_state = container(
                column![
                    // Icon
                    container(
                        iced_fonts::bootstrap::hdd_network().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text("Create host").size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text("Save your connection details as hosts to connect in one click.")
                        .size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    // Hostname input
                    text_input("Type IP or Hostname", &self.quick_host_input)
                        .on_input(Message::QuickHostInput)
                        .on_submit(Message::QuickHostContinue)
                        .padding(14)
                        .width(380),
                    Space::new().height(12),
                    // Continue button
                    button(
                        container(text("Continue").size(14).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                            .width(380)
                            .center_x(380),
                    )
                    .on_press(Message::QuickHostContinue)
                    .width(380)
                    .style(move |_, _| button::Style {
                        background: Some(Background::Color(btn_bg)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, search_bar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_host_panel {
                let panel = self.view_host_panel();
                return row![main_content, panel]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            } else {
                return main_content.into();
            }
        }

        if self.active_group.is_none() {
            // Root view: show folder cards for groups that have connections
            let mut shown_groups = std::collections::HashSet::new();
            for conn in &self.connections {
                if let Some(gid) = conn.group_id
                    && shown_groups.insert(gid)
                        && let Some(group) = self.groups.iter().find(|g| g.id == gid) {
                            let count = self.connections.iter().filter(|c| c.group_id == Some(gid)).count();
                            let label = group.label.clone();
                            let count_text = format!("{} host{}", count, if count != 1 { "s" } else { "" });

                            // Folder card with "stacked" effect
                            let folder_card = button(
                                container(
                                    column![
                                        row![
                                            iced_fonts::bootstrap::folder_fill().size(20).color(OryxisColors::t().accent),
                                            Space::new().width(Length::Fill),
                                            text(count_text).size(11).color(OryxisColors::t().text_muted),
                                        ].align_y(iced::Alignment::Center),
                                        Space::new().height(10),
                                        text(label).size(14).color(OryxisColors::t().text_primary),
                                    ],
                                )
                                .padding(16),
                            )
                            .on_press(Message::OpenGroup(gid))
                            .width(CARD_WIDTH)
                            .style(|_, status| {
                                let (bg, bc, bw) = match status {
                                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                                };
                                button::Style {
                                    background: Some(Background::Color(bg)),
                                    border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                                    ..Default::default()
                                }
                            });

                            cards.push(folder_card.into());
                        }
            }
        }

        // Show host cards — filtered by active group and search
        let search_lower = self.host_search.to_lowercase();
        for (idx, conn) in self.connections.iter().enumerate() {
            // Filter: at root show ungrouped only, inside folder show that group
            if let Some(gid) = self.active_group {
                if conn.group_id != Some(gid) { continue; }
            } else if conn.group_id.is_some() {
                continue; // hide grouped hosts at root (they're inside folder cards)
            }

            // Filter by search query
            if !search_lower.is_empty() {
                let label_match = conn.label.to_lowercase().contains(&search_lower);
                let host_match = conn.hostname.to_lowercase().contains(&search_lower);
                if !label_match && !host_match { continue; }
            }

            let is_connected = self.tabs.iter().any(|t| t.label == conn.label);
            let auth_label = match conn.auth_method {
                AuthMethod::Auto => "Auto",
                AuthMethod::Password => "Password",
                AuthMethod::Key => "Key",
                AuthMethod::Agent => "Agent",
                AuthMethod::Interactive => "Interactive",
            };
            let subtitle = format!("{}@{}:{} · {}", conn.username.as_deref().unwrap_or("root"), conn.hostname, conn.port, auth_label);

            let icon_color = if is_connected { OryxisColors::t().success } else { OryxisColors::t().accent };
            let icon_box = container(iced_fonts::bootstrap::hdd_network().size(14).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
                .style(move |_| container::Style {
                    background: Some(Background::Color(icon_color)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            // "..." button — only visible on hover or when menu is open
            let show_dots = self.hovered_card == Some(idx) || self.card_context_menu == Some(idx);
            let dots_btn: Element<'_, Message> = if show_dots {
                button(
                    text("···").size(14).color(OryxisColors::t().text_muted),
                )
                .on_press(Message::ShowCardMenu(idx))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .into()
            } else {
                Space::new().width(0).into()
            };

            let card_btn = button(
                container(
                    row![
                        icon_box,
                        Space::new().width(12),
                        column![
                            text(&conn.label).size(13).color(OryxisColors::t().text_primary),
                            Space::new().height(2),
                            text(subtitle).size(10).color(OryxisColors::t().text_muted),
                        ].width(Length::Fill),
                        dots_btn,
                    ].align_y(iced::Alignment::Center),
                )
                .padding(16),
            )
            .on_press(Message::ConnectSsh(idx))
            .width(CARD_WIDTH)
            .style(move |_, status| {
                let (bg, bc, bw) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                    ..Default::default()
                }
            });

            // Context menu dropdown (shown below the card)
            let card_el: Element<'_, Message> = if self.card_context_menu == Some(idx) {
                let menu = container(
                    column![
                        context_menu_item(iced_fonts::bootstrap::play_fill(), "Connect", Message::ConnectSsh(idx), OryxisColors::t().success),
                        context_menu_item(iced_fonts::bootstrap::pencil(), "Edit", Message::EditConnection(idx), OryxisColors::t().text_secondary),
                        context_menu_item(iced_fonts::bootstrap::copy(), "Duplicate", Message::DuplicateConnection(idx), OryxisColors::t().text_secondary),
                        context_menu_item(iced_fonts::bootstrap::trash(), "Remove", Message::DeleteConnection(idx), OryxisColors::t().error),
                    ],
                )
                .width(CARD_WIDTH)
                .padding(4)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                    ..Default::default()
                });

                column![card_btn, Space::new().height(4), menu]
                    .width(CARD_WIDTH)
                    .into()
            } else {
                card_btn.into()
            };

            // Wrap in MouseArea for hover tracking and right-click
            let wrapped = MouseArea::new(card_el)
                .on_enter(Message::CardHovered(idx))
                .on_exit(Message::CardUnhovered)
                .on_right_press(Message::ShowCardMenu(idx));

            cards.push(container(wrapped).width(CARD_WIDTH).into());
        }

        // Grid layout (3 cols)
        let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in cards {
            current_row.push(card);
            if current_row.len() == 3 {
                grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
                grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        let grid = scrollable(
            column(grid_rows)
                .padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        // Close context menu when clicking on empty area
        let grid = MouseArea::new(grid)
            .on_press(Message::HideCardMenu);

        // ── Main + side panel ──
        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);

        if self.show_host_panel {
            let panel = self.view_host_panel();
            row![main_content, panel]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content.into()
        }
    }

    fn view_connection_progress(&self) -> Element<'_, Message> {
        let progress = match &self.connecting {
            Some(p) => p,
            None => return Space::new().into(),
        };

        let step_num = match progress.step {
            ConnectionStep::Connecting => 1,
            ConnectionStep::Handshake => 2,
            ConnectionStep::Authenticating => 3,
        };

        let failed = progress.failed;
        let step_color = |n: u8| -> Color {
            if failed { return OryxisColors::t().error; }
            if n < step_num { OryxisColors::t().success }
            else if n == step_num { OryxisColors::t().accent }
            else { OryxisColors::t().text_muted }
        };

        // Header: host info
        let header = container(
            row![
                container(
                    iced_fonts::bootstrap::hdd_network().size(18).color(Color::WHITE),
                )
                .padding(10)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(10.0), ..Default::default() },
                    ..Default::default()
                }),
                Space::new().width(14),
                column![
                    text(&progress.label).size(16).color(OryxisColors::t().text_primary),
                    Space::new().height(2),
                    text(&progress.hostname).size(12).color(OryxisColors::t().text_muted),
                ],
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 24.0, right: 0.0, bottom: 16.0, left: 0.0 });

        // Progress dots
        let dot = |n: u8| -> Element<'_, Message> {
            let c = step_color(n);
            container(text("").size(1))
                .width(12).height(12)
                .style(move |_| container::Style {
                    background: Some(Background::Color(c)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                })
                .into()
        };
        let line = |n: u8| -> Element<'_, Message> {
            let c = step_color(n);
            container(Space::new().height(2))
                .width(80)
                .style(move |_| container::Style {
                    background: Some(Background::Color(c)),
                    ..Default::default()
                })
                .into()
        };

        let progress_bar = container(
            row![
                dot(1), line(1), dot(2), line(2), dot(3),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 0.0, right: 0.0, bottom: 16.0, left: 0.0 })
        .width(Length::Fill)
        .center_x(Length::Fill);

        // Status text
        let status_text = if failed {
            "Connection failed with connection log:"
        } else {
            "Connecting..."
        };
        let status_color = if failed { OryxisColors::t().error } else { OryxisColors::t().text_secondary };

        // Log entries
        let mut log_items: Vec<Element<'_, Message>> = Vec::new();
        for (step, msg) in &progress.logs {
            let icon_color = if msg.starts_with("Error") {
                OryxisColors::t().error
            } else {
                match step {
                    ConnectionStep::Connecting => OryxisColors::t().text_muted,
                    ConnectionStep::Handshake => OryxisColors::t().accent,
                    ConnectionStep::Authenticating => OryxisColors::t().warning,
                }
            };

            let icon = if msg.starts_with("Error") {
                iced_fonts::bootstrap::exclamation_circle()
            } else {
                iced_fonts::bootstrap::gear()
            };

            log_items.push(
                row![
                    icon.size(12).color(icon_color),
                    Space::new().width(10),
                    text(msg).size(13).color(OryxisColors::t().text_secondary),
                ]
                .align_y(iced::Alignment::Start)
                .into(),
            );
            log_items.push(Space::new().height(6).into());
        }

        let log_list = scrollable(
            column(log_items).padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .height(Length::Fill);

        let log_container = container(log_list)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { radius: Radius::from(10.0), ..Default::default() },
                ..Default::default()
            });

        // Bottom buttons
        let bottom: Element<'_, Message> = if failed {
            row![
                button(
                    container(text("Close").size(13).color(OryxisColors::t().text_primary))
                        .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                )
                .on_press(Message::SshCloseProgress)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
                Space::new().width(8),
                button(
                    container(text("Edit host").size(13).color(OryxisColors::t().text_primary))
                        .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                )
                .on_press(Message::SshEditFromProgress)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
                Space::new().width(Length::Fill),
                button(
                    container(text("Start over").size(13).color(OryxisColors::t().text_primary))
                        .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                )
                .on_press(Message::SshRetry)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().success)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            ]
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            Space::new().height(0).into()
        };

        container(
            column![
                header,
                progress_bar,
                text(status_text).size(14).color(status_color),
                Space::new().height(12),
                log_container,
                Space::new().height(12),
                bottom,
            ]
            .padding(32)
            .width(500)
            .height(Length::Fill),
        )
        .center_x(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .into()
    }

    fn view_terminal(&self) -> Element<'_, Message> {
        let terminal_area: Element<'_, Message> = if let Some(tab_idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(tab_idx) {
                let view = TerminalView::new(Arc::clone(&tab.terminal))
                    .with_font_size(self.terminal_font_size);
                canvas(view)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            } else {
                container(text("No active session").size(14).color(OryxisColors::t().text_muted))
                    .center(Length::Fill).into()
            }
        } else {
            container(text("No active session").size(14).color(OryxisColors::t().text_muted))
                .center(Length::Fill).into()
        };

        container(terminal_area)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::TERMINAL_BG)),
                ..Default::default()
            })
            .into()
    }

    fn view_keys(&self) -> Element<'_, Message> {
        // ── Header toolbar ──
        let add_btn = button(
            container(
                row![
                    text("+").size(12).color(OryxisColors::t().text_primary),
                    Space::new().width(4),
                    text("ADD").size(12).color(OryxisColors::t().text_primary),
                    Space::new().width(4),
                    text("\u{25BE}").size(10).color(OryxisColors::t().text_primary),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 6.0, right: 14.0, bottom: 7.0, left: 14.0 }),
        )
        .on_press(Message::ToggleKeychainAddMenu)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let add_area: Element<'_, Message> = if self.show_keychain_add_menu {
            let menu = container(
                column![
                    context_menu_item(iced_fonts::bootstrap::key(), "Import Key", Message::ShowKeyPanel, OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::bootstrap::person(), "New Identity", Message::ShowIdentityPanel, OryxisColors::t().text_secondary),
                ],
            )
            .width(180)
            .padding(4)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });
            column![add_btn, Space::new().height(4), menu].into()
        } else {
            add_btn.into()
        };

        let toolbar = container(
            row![
                text("Keychain").size(20).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                add_area,
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Search bar ──
        let search_bar = container(
            text_input("Search keys & identities...", &self.key_search)
                .on_input(Message::KeySearchChanged)
                .padding(10)
                .width(Length::Fill),
        )
        .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
        .width(Length::Fill);

        // ── Status message ──
        let status: Element<'_, Message> = if let Some(err) = &self.key_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 })
                .into()
        } else if let Some(ok) = &self.key_success {
            container(Element::from(text(ok.clone()).size(12).color(OryxisColors::t().success)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Keys grid ──
        let section_title = container(
            text("Keys").size(14).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 4.0, right: 24.0, bottom: 8.0, left: 24.0 });

        // Filter keys by search query
        let search_lower = self.key_search.to_lowercase();
        let filtered_keys: Vec<(usize, &SshKey)> = self.keys.iter().enumerate()
            .filter(|(_, k)| search_lower.is_empty() || k.label.to_lowercase().contains(&search_lower))
            .collect();

        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if filtered_keys.is_empty() && self.keys.is_empty() {
            let empty_state = container(
                column![
                    container(
                        iced_fonts::bootstrap::key().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text("Add a key").size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text("Import SSH keys to authenticate with your hosts.")
                        .size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    button(
                        container(text("Import Key").size(14).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                            .width(380)
                            .center_x(380),
                    )
                    .on_press(Message::ShowKeyPanel)
                    .width(380)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, search_bar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_key_panel {
                let panel = self.view_key_import_panel();
                return row![main_content, panel]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            } else if self.show_identity_panel {
                let panel = self.view_identity_panel();
                return row![main_content, panel]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
            return main_content.into();
        } else if filtered_keys.is_empty() {
            let no_results = container(
                text("No keys match your search").size(13).color(OryxisColors::t().text_muted),
            )
            .padding(24)
            .width(CARD_WIDTH);
            cards.push(no_results.into());
        }

        for (idx, key) in filtered_keys {
            let algo = format!("Type {}", key.algorithm);
            let icon_box = container(iced_fonts::bootstrap::key().size(18).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            // "..." menu button
            let dots_btn = button(
                text("···").size(14).color(OryxisColors::t().text_muted),
            )
            .on_press(Message::ShowKeyMenu(idx))
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });

            let card = button(
                row![
                    icon_box,
                    Space::new().width(12),
                    column![
                        text(&key.label).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        text(algo).size(11).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill),
                    dots_btn,
                ].align_y(iced::Alignment::Center),
            )
            .on_press(Message::EditKey(idx))
            .padding(16)
            .width(CARD_WIDTH)
            .style(|_, status| {
                let (bg, border_color, border_width) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: border_color, width: border_width },
                    ..Default::default()
                }
            });

            // Context menu dropdown (shown below the card)
            let card_el: Element<'_, Message> = if self.key_context_menu == Some(idx) {
                let menu = container(
                    column![
                        context_menu_item(iced_fonts::bootstrap::pencil(), "Edit", Message::EditKey(idx), OryxisColors::t().text_secondary),
                        context_menu_item(iced_fonts::bootstrap::trash(), "Remove", Message::DeleteKey(idx), OryxisColors::t().error),
                    ],
                )
                .width(CARD_WIDTH)
                .padding(4)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                    ..Default::default()
                });

                column![card, Space::new().height(4), menu]
                    .width(CARD_WIDTH)
                    .into()
            } else {
                card.into()
            };

            // Wrap in MouseArea for right-click
            let wrapped = MouseArea::new(card_el)
                .on_right_press(Message::ShowKeyMenu(idx));

            cards.push(container(wrapped).width(CARD_WIDTH).into());
        }

        // Key grid layout (3 cols)
        let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in cards {
            current_row.push(card);
            if current_row.len() == 3 {
                grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
                grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        // ── Identities section ──
        let identity_section_title = container(
            text("Identities").size(14).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 16.0, right: 24.0, bottom: 8.0, left: 24.0 });

        let filtered_identities: Vec<(usize, &Identity)> = self.identities.iter().enumerate()
            .filter(|(_, i)| search_lower.is_empty() || i.label.to_lowercase().contains(&search_lower))
            .collect();

        let mut identity_cards: Vec<Element<'_, Message>> = Vec::new();

        if filtered_identities.is_empty() && self.identities.is_empty() {
            let empty_hint = container(
                text("No identities yet. Create one to store reusable credentials.")
                    .size(12).color(OryxisColors::t().text_muted),
            )
            .padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 })
            .width(CARD_WIDTH);
            identity_cards.push(empty_hint.into());
        } else if filtered_identities.is_empty() {
            let no_results = container(
                text("No identities match your search").size(13).color(OryxisColors::t().text_muted),
            )
            .padding(24)
            .width(CARD_WIDTH);
            identity_cards.push(no_results.into());
        }

        for (idx, identity) in &filtered_identities {
            let idx = *idx;
            // Build subtitle describing auth methods
            let mut parts: Vec<String> = Vec::new();
            if let Some(u) = &identity.username {
                parts.push(u.clone());
            }
            let has_pw = self.vault.as_ref()
                .and_then(|v| v.get_identity_password(&identity.id).ok().flatten())
                .is_some();
            if has_pw {
                parts.push("\u{25CF}\u{25CF}\u{25CF}\u{25CF}".into());
            }
            if let Some(kid) = identity.key_id {
                if let Some(k) = self.keys.iter().find(|k| k.id == kid) {
                    parts.push(k.label.clone());
                }
            }
            let subtitle = if parts.is_empty() { "No credentials".into() } else { parts.join(", ") };

            let icon_box = container(iced_fonts::bootstrap::person().size(18).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            let dots_btn = button(
                text("···").size(14).color(OryxisColors::t().text_muted),
            )
            .on_press(Message::ShowIdentityMenu(idx))
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });

            let card = button(
                row![
                    icon_box,
                    Space::new().width(12),
                    column![
                        text(&identity.label).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        text(subtitle).size(11).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill),
                    dots_btn,
                ].align_y(iced::Alignment::Center),
            )
            .on_press(Message::EditIdentity(idx))
            .padding(16)
            .width(CARD_WIDTH)
            .style(|_, status| {
                let (bg, border_color, border_width) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: border_color, width: border_width },
                    ..Default::default()
                }
            });

            let card_el: Element<'_, Message> = if self.identity_context_menu == Some(idx) {
                let menu = container(
                    column![
                        context_menu_item(iced_fonts::bootstrap::pencil(), "Edit", Message::EditIdentity(idx), OryxisColors::t().text_secondary),
                        context_menu_item(iced_fonts::bootstrap::trash(), "Remove", Message::DeleteIdentity(idx), OryxisColors::t().error),
                    ],
                )
                .width(CARD_WIDTH)
                .padding(4)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                    ..Default::default()
                });

                column![card, Space::new().height(4), menu]
                    .width(CARD_WIDTH)
                    .into()
            } else {
                card.into()
            };

            let wrapped = MouseArea::new(card_el)
                .on_right_press(Message::ShowIdentityMenu(idx));

            identity_cards.push(container(wrapped).width(CARD_WIDTH).into());
        }

        // Identity grid layout (3 cols)
        let mut identity_grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in identity_cards {
            current_row.push(card);
            if current_row.len() == 3 {
                identity_grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
                identity_grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            identity_grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        // Combine keys and identities into one scrollable area
        let mut all_rows: Vec<Element<'_, Message>> = Vec::new();
        all_rows.push(section_title.into());
        all_rows.extend(grid_rows);
        all_rows.push(identity_section_title.into());
        all_rows.extend(identity_grid_rows);

        let grid = scrollable(
            column(all_rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill);

        // Close context menus when clicking on empty area
        let grid = MouseArea::new(grid)
            .on_press(Message::HideKeyMenu);

        // ── Main content ──
        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Side panel ──
        if self.show_key_panel {
            let panel = self.view_key_import_panel();
            row![main_content, panel]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.show_identity_panel {
            let panel = self.view_identity_panel();
            row![main_content, panel]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content.into()
        }
    }

    fn view_key_import_panel(&self) -> Element<'_, Message> {
        let has_content = !self.key_import_pem.is_empty();
        let panel_title = if self.editing_key_id.is_some() { "Edit Key" } else { "Add Key" };

        // Panel header
        let panel_header = container(
            row![
                text(panel_title).size(18).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideKeyPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Name field
        let name_field = column![
            text("Name").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            text_input("my-server-key", &self.key_import_label)
                .on_input(Message::KeyImportLabelChanged)
                .padding(10),
        ];

        // File selector button
        let browse_btn = button(
            container(
                row![
                    text("Select File...").size(13).color(OryxisColors::t().text_primary),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .on_press(Message::BrowseKeyFile)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        // Status indicator
        let file_status: Element<'_, Message> = if has_content {
            container(
                row![
                    text("V").size(12).color(OryxisColors::t().success),
                    Space::new().width(6),
                    text(format!("Loaded ({} bytes)", self.key_import_pem.len()))
                        .size(11).color(OryxisColors::t().success),
                ].align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
            .into()
        } else {
            Space::new().height(0).into()
        };

        // Editable key content (text_editor = multi-line)
        let editor = text_editor(&self.key_import_content)
            .on_action(Message::KeyContentAction)
            .padding(10)
            .height(180)
            .font(iced::Font::MONOSPACE)
            .size(11);

        // Error in panel
        let panel_error: Element<'_, Message> = if let Some(err) = &self.key_error {
            Element::from(text(err.clone()).size(11).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        // Save button
        let save_label = if self.editing_key_id.is_some() { "Update Key" } else { "Save Key" };
        let save_btn = button(
            container(text(save_label).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::ImportKey)
        .width(Length::Fill)
        .style(move |_, _| {
            let bg = if has_content { OryxisColors::t().accent } else { OryxisColors::t().bg_surface };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }
        });

        let panel_content = column![
            panel_header,
            container(
                column![
                    name_field,
                    Space::new().height(16),
                    text("Private Key").size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(6),
                    browse_btn,
                    Space::new().height(8),
                    file_status,
                    Space::new().height(8),
                    text("Key Content").size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(6),
                    editor,
                    Space::new().height(8),
                    panel_error,
                    Space::new().height(Length::Fill),
                    save_btn,
                ]
                .height(Length::Fill),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 })
            .height(Length::Fill),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }

    fn view_identity_panel(&self) -> Element<'_, Message> {
        let panel_title = if self.editing_identity_id.is_some() { "Edit Identity" } else { "New Identity" };

        // Panel header
        let panel_header = container(
            row![
                text(panel_title).size(18).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideIdentityPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Label field
        let label_field = column![
            text("Label").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            text_input("My Identity", &self.identity_form_label)
                .on_input(Message::IdentityLabelChanged)
                .padding(10),
        ];

        // Username field
        let username_field = column![
            text("Username").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            row![
                iced_fonts::bootstrap::person().size(13).color(OryxisColors::t().text_muted),
                Space::new().width(10),
                text_input("root", &self.identity_form_username)
                    .on_input(Message::IdentityUsernameChanged)
                    .padding(10),
            ].align_y(iced::Alignment::Center),
        ];

        // Password field with eye toggle
        let password_field = column![
            text("Password").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            row![
                iced_fonts::bootstrap::keyboard().size(13).color(OryxisColors::t().text_muted),
                Space::new().width(10),
                text_input(
                    if self.identity_form_has_existing_password && !self.identity_form_password_touched {
                        "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"
                    } else {
                        "Password"
                    },
                    &self.identity_form_password,
                )
                    .on_input(Message::IdentityPasswordChanged)
                    .secure(!self.identity_form_password_visible)
                    .padding(10),
                Space::new().width(6),
                button(
                    if self.identity_form_password_visible {
                        iced_fonts::bootstrap::eye_slash().size(14).color(OryxisColors::t().text_muted)
                    } else {
                        iced_fonts::bootstrap::eye().size(14).color(OryxisColors::t().text_muted)
                    }
                )
                    .on_press(Message::IdentityTogglePasswordVisibility)
                    .style(|_t, _s| button::Style::default())
                    .padding(8),
            ].align_y(iced::Alignment::Center),
        ];

        // Key selector
        let key_options = {
            let mut opts = vec!["(none)".to_string()];
            opts.extend(self.keys.iter().map(|k| k.label.clone()));
            opts
        };
        let key_field = column![
            text("SSH Key").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(6),
            row![
                text("+ Key").size(12).color(OryxisColors::t().accent),
                Space::new().width(16),
                pick_list(
                    key_options,
                    Some(self.identity_form_key.clone().unwrap_or_else(|| "(none)".into())),
                    Message::IdentityKeyChanged,
                ),
            ].align_y(iced::Alignment::Center),
        ];

        // Linked connections (only when editing)
        let linked_section: Element<'_, Message> = if let Some(editing_id) = self.editing_identity_id {
            let linked: Vec<&Connection> = self.connections.iter()
                .filter(|c| c.identity_id == Some(editing_id))
                .collect();
            if linked.is_empty() {
                column![
                    Space::new().height(16),
                    text("Linked to").size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    text("No connections using this identity").size(11).color(OryxisColors::t().text_muted),
                ].into()
            } else {
                let mut items: Vec<Element<'_, Message>> = vec![
                    Space::new().height(16).into(),
                    Element::from(text("Linked to").size(12).color(OryxisColors::t().text_muted)),
                    Space::new().height(4).into(),
                ];
                for conn in linked {
                    items.push(
                        container(
                            row![
                                iced_fonts::bootstrap::hdd_network().size(11).color(OryxisColors::t().text_muted),
                                Space::new().width(8),
                                text(&conn.label).size(12).color(OryxisColors::t().text_secondary),
                            ].align_y(iced::Alignment::Center),
                        )
                        .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
                        .into()
                    );
                }
                column(items).into()
            }
        } else {
            Space::new().height(0).into()
        };

        // Save button
        let save_label = if self.editing_identity_id.is_some() { "Update Identity" } else { "Save Identity" };
        let has_label = !self.identity_form_label.trim().is_empty();
        let save_btn = button(
            container(text(save_label).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::SaveIdentity)
        .width(Length::Fill)
        .style(move |_, _| {
            let bg = if has_label { OryxisColors::t().accent } else { OryxisColors::t().bg_surface };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }
        });

        let panel_content = column![
            panel_header,
            container(
                column![
                    label_field,
                    Space::new().height(16),
                    username_field,
                    Space::new().height(16),
                    password_field,
                    Space::new().height(16),
                    key_field,
                    linked_section,
                    Space::new().height(Length::Fill),
                    save_btn,
                ]
                .height(Length::Fill),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 })
            .height(Length::Fill),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }

    fn view_snippets(&self) -> Element<'_, Message> {
        let toolbar = container(
            row![
                text("Snippets").size(20).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(
                    container(
                        row![
                            text("+").size(12).color(OryxisColors::t().text_primary),
                            Space::new().width(4),
                            text("SNIPPET").size(12).color(OryxisColors::t().text_primary),
                        ].align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 6.0, right: 14.0, bottom: 7.0, left: 14.0 }),
                )
                .on_press(Message::ShowSnippetPanel)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let status: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        let section_title = container(
            text("Commands").size(14).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 4.0, right: 24.0, bottom: 8.0, left: 24.0 });

        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if self.snippets.is_empty() {
            let empty_state = container(
                column![
                    container(
                        iced_fonts::bootstrap::code_square().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text("Create a snippet").size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text("Save commands you use often for quick access.")
                        .size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    button(
                        container(text("New Snippet").size(14).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                            .width(380)
                            .center_x(380),
                    )
                    .on_press(Message::ShowSnippetPanel)
                    .width(380)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_snippet_panel {
                let panel = self.view_snippet_panel();
                return row![main_content, panel]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
            return main_content.into();
        }

        for (idx, snip) in self.snippets.iter().enumerate() {
            let icon_box = container(iced_fonts::bootstrap::code_square().size(14).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            let edit_btn = button(text("...").size(12).color(OryxisColors::t().text_muted))
                .on_press(Message::EditSnippet(idx))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });

            let cmd_preview = if snip.command.len() > 30 {
                format!("{}...", &snip.command[..30])
            } else {
                snip.command.clone()
            };

            let card = button(
                container(
                    row![
                        icon_box,
                        Space::new().width(12),
                        column![
                            text(&snip.label).size(13).color(OryxisColors::t().text_primary),
                            Space::new().height(2),
                            text(cmd_preview).size(10).color(OryxisColors::t().text_muted).font(iced::Font::MONOSPACE),
                        ].width(Length::Fill),
                        edit_btn,
                    ].align_y(iced::Alignment::Center),
                )
                .padding(16),
            )
            .on_press(Message::RunSnippet(idx))
            .width(CARD_WIDTH)
            .style(move |_, status| {
                let (bg, bc, bw) = match status {
                    BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                    BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                    _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                    ..Default::default()
                }
            });

            cards.push(card.into());
        }

        let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in cards {
            current_row.push(card);
            if current_row.len() == 3 {
                grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
                grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        let grid = scrollable(
            column(grid_rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        let main_content = column![toolbar, status, section_title, grid]
            .width(Length::Fill).height(Length::Fill);

        if self.show_snippet_panel {
            let panel = self.view_snippet_panel();
            row![main_content, panel].width(Length::Fill).height(Length::Fill).into()
        } else {
            main_content.into()
        }
    }

    fn view_snippet_panel(&self) -> Element<'_, Message> {
        let is_editing = self.snippet_editing_id.is_some();
        let title = if is_editing { "Edit Snippet" } else { "New Snippet" };

        let panel_header = container(
            row![
                text(title).size(18).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideSnippetPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        let form = column![
            text("Name").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("restart-nginx", &self.snippet_label)
                .on_input(Message::SnippetLabelChanged)
                .padding(10),
            Space::new().height(14),
            text("Command").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("sudo systemctl restart nginx", &self.snippet_command)
                .on_input(Message::SnippetCommandChanged)
                .padding(10),
        ];

        let panel_error: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            Element::from(text(err.clone()).size(11).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        let save_btn = button(
            container(text("Save").size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill).center_x(Length::Fill),
        )
        .on_press(Message::SaveSnippet)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let mut bottom = column![save_btn];
        if let Some(edit_id) = self.snippet_editing_id
            && let Some(idx) = self.snippets.iter().position(|s| s.id == edit_id) {
                let del_btn = button(
                    container(text("Delete").size(13).color(OryxisColors::t().error))
                        .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                        .width(Length::Fill).center_x(Length::Fill),
                )
                .on_press(Message::DeleteSnippet(idx))
                .width(Length::Fill)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().error, width: 1.0 },
                    ..Default::default()
                });
                bottom = bottom.push(Space::new().height(8));
                bottom = bottom.push(del_btn);
            }

        let panel_content = column![
            panel_header,
            container(
                column![
                    form,
                    Space::new().height(12),
                    panel_error,
                    Space::new().height(Length::Fill),
                    bottom,
                ].height(Length::Fill),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 })
            .height(Length::Fill),
        ].height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }

    fn view_known_hosts(&self) -> Element<'_, Message> {
        let toolbar = container(
            text("Known Hosts").size(20).color(OryxisColors::t().text_primary),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        if self.known_hosts.is_empty() {
            rows.push(
                container(
                    text("No known hosts yet. They will be added automatically when you connect to servers.")
                        .size(13).color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .into(),
            );
        }

        for (idx, kh) in self.known_hosts.iter().enumerate() {
            let fp_short = if kh.fingerprint.len() > 40 {
                format!("{}...", &kh.fingerprint[..40])
            } else {
                kh.fingerprint.clone()
            };
            let seen = kh.last_seen.format("%Y-%m-%d %H:%M").to_string();

            let del_btn = button(text("x").size(11).color(OryxisColors::t().text_muted))
                .on_press(Message::DeleteKnownHost(idx))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });

            let entry = container(
                row![
                    iced_fonts::bootstrap::shield_check().size(14).color(OryxisColors::t().success),
                    Space::new().width(12),
                    column![
                        text(format!("{}:{}", kh.hostname, kh.port)).size(13).color(OryxisColors::t().text_primary),
                        Space::new().height(2),
                        text(format!("{} · {}", kh.key_type, fp_short)).size(10).color(OryxisColors::t().text_muted).font(iced::Font::MONOSPACE),
                        Space::new().height(2),
                        text(format!("Last seen: {}", seen)).size(10).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill),
                    del_btn,
                ].align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 10.0, right: 16.0, bottom: 10.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

            rows.push(entry.into());
            rows.push(Space::new().height(6).into());
        }

        let list = scrollable(
            column(rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        column![toolbar, list]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_history(&self) -> Element<'_, Message> {
        let toolbar = container(
            row![
                text("History").size(20).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(
                    container(text("Clear").size(12).color(OryxisColors::t().text_muted))
                        .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 }),
                )
                .on_press(Message::ClearLogs)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().error },
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                        ..Default::default()
                    }
                }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        if self.logs.is_empty() {
            rows.push(
                container(
                    text("No activity logged yet.")
                        .size(13).color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .into(),
            );
        }

        for entry in &self.logs {
            let (event_icon, event_color) = match entry.event {
                oryxis_core::models::log_entry::LogEvent::Connected => {
                    (iced_fonts::bootstrap::check_circle(), OryxisColors::t().success)
                }
                oryxis_core::models::log_entry::LogEvent::Disconnected => {
                    (iced_fonts::bootstrap::dash_circle(), OryxisColors::t().text_muted)
                }
                oryxis_core::models::log_entry::LogEvent::AuthFailed => {
                    (iced_fonts::bootstrap::x_circle(), OryxisColors::t().warning)
                }
                oryxis_core::models::log_entry::LogEvent::Error => {
                    (iced_fonts::bootstrap::exclamation_circle(), OryxisColors::t().error)
                }
            };

            let ts = entry.timestamp.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string();

            let log_row = container(
                row![
                    event_icon.size(14).color(event_color),
                    Space::new().width(12),
                    column![
                        row![
                            text(&entry.connection_label).size(13).color(OryxisColors::t().text_primary),
                            Space::new().width(8),
                            text(format!("{}", entry.event)).size(11).color(event_color),
                        ].align_y(iced::Alignment::Center),
                        Space::new().height(2),
                        text(&entry.message).size(11).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill),
                    text(ts).size(10).color(OryxisColors::t().text_muted),
                ].align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

            rows.push(log_row.into());
            rows.push(Space::new().height(4).into());
        }

        let list = scrollable(
            column(rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        column![toolbar, list]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_settings(&self) -> Element<'_, Message> {
        // ── Settings sidebar ──
        let settings_sidebar = {
            let items: Vec<(&str, SettingsSection)> = vec![
                ("Terminal", SettingsSection::Terminal),
                ("Theme", SettingsSection::Theme),
                ("About", SettingsSection::About),
            ];
            let mut col = column![
                text("Settings").size(16).color(OryxisColors::t().text_primary),
                Space::new().height(12),
            ]
            .padding(Padding { top: 20.0, right: 8.0, bottom: 8.0, left: 8.0 });

            for (label, section) in items {
                let is_active = self.settings_section == section;
                let bg = if is_active {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else {
                    Color::TRANSPARENT
                };
                let fg = if is_active {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().text_secondary
                };
                let btn: Element<'_, Message> = button(
                    container(text(label).size(13).color(fg))
                        .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
                )
                .on_press(Message::ChangeSettingsSection(section))
                .width(Length::Fill)
                .style(move |_, status| {
                    let hover_bg = match status {
                        BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                        BtnStatus::Pressed => Color { a: 0.25, ..OryxisColors::t().accent },
                        _ => bg,
                    };
                    button::Style {
                        background: Some(Background::Color(hover_bg)),
                        border: Border { radius: Radius::from(10.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .into();
                col = col.push(btn);
            }

            container(col)
                .width(200)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                    border: Border {
                        color: OryxisColors::t().border,
                        width: 1.0,
                        radius: Radius::from(0.0),
                    },
                    ..Default::default()
                })
        };

        // ── Settings content ──
        let settings_content: Element<'_, Message> = match self.settings_section {
            SettingsSection::Terminal => {
                let toggles_section = panel_section(column![
                    toggle_row("Select text to copy & Right click to paste", self.setting_copy_on_select, Message::ToggleCopyOnSelect),
                    Space::new().height(10),
                    toggle_row("Use bright colours for bold text", self.setting_bold_is_bright, Message::ToggleBoldIsBright),
                    Space::new().height(10),
                    toggle_row("Bell sound", self.setting_bell_sound, Message::ToggleBellSound),
                    Space::new().height(10),
                    toggle_row("Keyword highlighting", self.setting_keyword_highlight, Message::ToggleKeywordHighlight),
                ]);

                let font_size_section = panel_section(column![
                    row![
                        text("Terminal Font Size").size(13).color(OryxisColors::t().text_primary),
                        Space::new().width(Length::Fill),
                        button(
                            container(text("\u{2212}").size(14).color(OryxisColors::t().text_primary))
                                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
                        )
                        .on_press(Message::TerminalFontSizeDecrease)
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => OryxisColors::t().bg_selected,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                ..Default::default()
                            }
                        }),
                        Space::new().width(8),
                        text(format!("{:.0}", self.terminal_font_size)).size(13).color(OryxisColors::t().text_primary),
                        Space::new().width(8),
                        button(
                            container(text("+").size(14).color(OryxisColors::t().text_primary))
                                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
                        )
                        .on_press(Message::TerminalFontSizeIncrease)
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => OryxisColors::t().bg_selected,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                ..Default::default()
                            }
                        }),
                    ].align_y(iced::Alignment::Center),
                ]);

                let keepalive_section = panel_section(column![
                    text("Keepalive Interval").size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("How often (in seconds) to send SSH keepalive packets. Set to 0 to disable.")
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("0", &self.setting_keepalive_interval)
                        .on_input(Message::SettingKeepaliveChanged)
                        .size(13)
                        .width(120),
                ]);

                let scrollback_section = panel_section(column![
                    text("Scrollback").size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("Limit number of terminal rows. Set to 0 for maximum.")
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("10000", &self.setting_scrollback_rows)
                        .on_input(Message::SettingScrollbackChanged)
                        .size(13)
                        .width(120),
                ]);

                let lock_btn = button(
                    container(
                        row![
                            iced_fonts::bootstrap::lock().size(14).color(OryxisColors::t().warning),
                            Space::new().width(10),
                            text("Lock Vault").size(13).color(OryxisColors::t().warning),
                        ].align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 }),
                )
                .on_press(Message::LockVault)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().warning },
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(8.0), color: OryxisColors::t().warning, width: 1.0 },
                        ..Default::default()
                    }
                });

                scrollable(
                    container(
                        column![
                            text("Terminal Settings").size(18).color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            toggles_section,
                            Space::new().height(12),
                            font_size_section,
                            Space::new().height(12),
                            keepalive_section,
                            Space::new().height(12),
                            scrollback_section,
                            Space::new().height(24),
                            text("Security").size(14).color(OryxisColors::t().text_muted),
                            Space::new().height(8),
                            lock_btn,
                            Space::new().height(24),
                        ]
                        .width(Length::Fill),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Theme => {
                use crate::theme::AppTheme;
                let active_name = AppTheme::active().name();

                let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
                let themes: Vec<&AppTheme> = AppTheme::ALL.iter().collect();

                for chunk in themes.chunks(2) {
                    let mut r = row![].spacing(12);
                    for theme in chunk {
                        let name = theme.name();
                        let is_active = name == active_name;
                        let colors = match theme {
                            AppTheme::OryxisDark => &crate::theme::ORYXIS_DARK,
                            AppTheme::OryxisLight => &crate::theme::ORYXIS_LIGHT,
                            AppTheme::Dracula => &crate::theme::DRACULA,
                            AppTheme::Nord => &crate::theme::NORD,
                        };
                        let border_color = if is_active {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().border
                        };
                        let border_width = if is_active { 2.0 } else { 1.0 };

                        let preview_bg = colors.bg_primary;
                        let accent_bar = colors.accent;
                        let success_bar = colors.success;
                        let error_bar = colors.error;

                        let preview = container(
                            column![
                                Space::new().height(20),
                                row![
                                    container(Space::new().width(30).height(4))
                                        .style(move |_| container::Style {
                                            background: Some(Background::Color(accent_bar)),
                                            border: Border { radius: Radius::from(2.0), ..Default::default() },
                                            ..Default::default()
                                        }),
                                    Space::new().width(4),
                                    container(Space::new().width(20).height(4))
                                        .style(move |_| container::Style {
                                            background: Some(Background::Color(success_bar)),
                                            border: Border { radius: Radius::from(2.0), ..Default::default() },
                                            ..Default::default()
                                        }),
                                    Space::new().width(4),
                                    container(Space::new().width(15).height(4))
                                        .style(move |_| container::Style {
                                            background: Some(Background::Color(error_bar)),
                                            border: Border { radius: Radius::from(2.0), ..Default::default() },
                                            ..Default::default()
                                        }),
                                ].padding(Padding { top: 0.0, right: 8.0, bottom: 8.0, left: 8.0 }),
                            ],
                        )
                        .width(120)
                        .style(move |_| container::Style {
                            background: Some(Background::Color(preview_bg)),
                            border: Border { radius: Radius::from(6.0), ..Default::default() },
                            ..Default::default()
                        });

                        let card: Element<'_, Message> = button(
                            container(
                                column![
                                    preview,
                                    Space::new().height(8),
                                    text(name).size(12).color(OryxisColors::t().text_primary),
                                ]
                                .align_x(iced::Alignment::Center),
                            )
                            .padding(12),
                        )
                        .on_press(Message::AppThemeChanged(name.to_string()))
                        .width(Length::FillPortion(1))
                        .style(move |_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => OryxisColors::t().bg_surface,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border {
                                    radius: Radius::from(8.0),
                                    color: border_color,
                                    width: border_width,
                                },
                                ..Default::default()
                            }
                        })
                        .into();
                        r = r.push(card);
                    }
                    // Fill remaining space if odd number
                    if chunk.len() == 1 {
                        r = r.push(Space::new().width(Length::FillPortion(1)));
                    }
                    grid_rows.push(r.into());
                }

                let mut content_col = column![
                    text("Theme").size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                ]
                .spacing(12)
                .width(Length::Fill);

                for row_el in grid_rows {
                    content_col = content_col.push(row_el);
                }

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::About => {
                let about_section = panel_section(column![
                    text("Oryxis v0.1.0").size(16).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("A modern SSH client built with Rust").size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(16),
                    settings_row("Built with", "Iced, russh, alacritty_terminal".into()),
                    Space::new().height(6),
                    settings_row("License", "AGPL-3.0".into()),
                    Space::new().height(6),
                    settings_row("GitHub", "github.com/wilsonglasser/oryxis".into()),
                ]);

                let vault_section = panel_section(column![
                    text("Vault Statistics").size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    settings_row("Hosts", self.connections.len().to_string()),
                    Space::new().height(6),
                    settings_row("Keys", self.keys.len().to_string()),
                    Space::new().height(6),
                    settings_row("Snippets", self.snippets.len().to_string()),
                    Space::new().height(6),
                    settings_row("Groups", self.groups.len().to_string()),
                ]);

                scrollable(
                    container(
                        column![
                            text("About").size(18).color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            about_section,
                            Space::new().height(12),
                            vault_section,
                            Space::new().height(24),
                        ]
                        .width(Length::Fill),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }
        };

        container(
            row![
                settings_sidebar,
                container(settings_content)
                    .width(Length::Fill)
                    .height(Length::Fill),
            ],
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_status_bar(&self) -> Element<'_, Message> {
        let status_text = if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(idx) {
                format!("● {} — connected", tab.label)
            } else {
                "No active connection".into()
            }
        } else {
            "No active connection".into()
        };

        let status_color = if self.active_tab.is_some() {
            OryxisColors::t().success
        } else {
            OryxisColors::t().text_muted
        };

        container(
            row![
                text(status_text).size(12).color(status_color),
                Space::new().width(Length::Fill),
                text("Oryxis v0.1.0").size(12).color(OryxisColors::t().text_muted),
            ]
            .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 }),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
            ..Default::default()
        })
        .into()
    }

    fn view_host_panel(&self) -> Element<'_, Message> {
        let is_editing = self.editor_form.editing_id.is_some();
        let title = if is_editing { "Edit Host" } else { "New Host" };
        let has_address = !self.editor_form.hostname.is_empty();

        // ── Header ──
        let panel_header = container(
            row![
                text(title).size(16).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(iced_fonts::bootstrap::arrow_bar_right().size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::EditorCancel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        border: Border::default(),
                        ..Default::default()
                    }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 16.0, bottom: 12.0, left: 16.0 });

        // ── Section: Address ──
        let address_section = panel_section(column![
            row![
                container(
                    iced_fonts::bootstrap::hdd_network().size(14).color(Color::WHITE),
                )
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }),
                Space::new().width(10),
                text_input("IP or Hostname", &self.editor_form.hostname)
                    .on_input(Message::EditorHostnameChanged)
                    .padding(10),
            ].align_y(iced::Alignment::Center),
        ]);

        // ── Section: General ──
        let general_section = panel_section(column![
            panel_field("Label", text_input("My Server", &self.editor_form.label)
                .on_input(Message::EditorLabelChanged).padding(10).into()),
            Space::new().height(8),
            panel_field("Parent Group", text_input("Production, Staging...", &self.editor_form.group_name)
                .on_input(Message::EditorGroupChanged).padding(10).into()),
        ]);

        // ── Section: SSH & Credentials ──
        let port_text = "SSH on port".to_string();
        let ssh_section = panel_section(column![
            // SSH on [port] port
            row![
                text(port_text).size(13).color(OryxisColors::t().text_secondary),
                Space::new().width(8),
                text_input("22", &self.editor_form.port)
                    .on_input(Message::EditorPortChanged)
                    .padding(6)
                    .width(60),
            ].align_y(iced::Alignment::Center),
            Space::new().height(12),
            text("Credentials").size(12).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            // Identity selector
            row![
                iced_fonts::bootstrap::person().size(13).color(OryxisColors::t().text_muted),
                Space::new().width(10),
                text("Identity").size(12).color(OryxisColors::t().text_secondary),
                Space::new().width(8),
                pick_list(
                    {
                        let mut opts = vec!["(none)".to_string()];
                        opts.extend(self.identities.iter().map(|i| i.label.clone()));
                        opts
                    },
                    Some(self.editor_form.selected_identity.clone().unwrap_or_else(|| "(none)".into())),
                    Message::EditorIdentityChanged,
                ),
            ].align_y(iced::Alignment::Center),
            Space::new().height(8),
            // Username
            row![
                iced_fonts::bootstrap::person().size(13).color(OryxisColors::t().text_muted),
                Space::new().width(10),
                text_input("Username", &self.editor_form.username)
                    .on_input(Message::EditorUsernameChanged)
                    .padding(10),
            ].align_y(iced::Alignment::Center),
            Space::new().height(8),
            // Password
            row![
                iced_fonts::bootstrap::keyboard().size(13).color(OryxisColors::t().text_muted),
                Space::new().width(10),
                text_input(
                    if self.editor_form.has_existing_password && !self.editor_form.password_touched {
                        "••••••••"
                    } else {
                        "Password"
                    },
                    &self.editor_form.password,
                )
                    .on_input(Message::EditorPasswordChanged)
                    .secure(!self.editor_form.password_visible)
                    .padding(10),
                Space::new().width(6),
                button(
                    if self.editor_form.password_visible {
                        iced_fonts::bootstrap::eye_slash().size(14).color(OryxisColors::t().text_muted)
                    } else {
                        iced_fonts::bootstrap::eye().size(14).color(OryxisColors::t().text_muted)
                    }
                )
                    .on_press(Message::EditorTogglePasswordVisibility)
                    .style(|_t, _s| button::Style::default())
                    .padding(8),
            ].align_y(iced::Alignment::Center),
            Space::new().height(8),
            // Key / Auth selector
            row![
                text("+ Key").size(12).color(OryxisColors::t().accent),
                Space::new().width(16),
                pick_list(
                    {
                        let mut opts = vec!["(none)".to_string()];
                        opts.extend(self.keys.iter().map(|k| k.label.clone()));
                        opts
                    },
                    Some(self.editor_form.selected_key.clone().unwrap_or_else(|| "(none)".into())),
                    Message::EditorKeyChanged,
                ),
            ].align_y(iced::Alignment::Center),
        ]);

        // ── Section: Advanced Options ──
        let jump_host_value = self.editor_form.jump_host.as_deref().unwrap_or("Disabled");
        let auth_value = match self.editor_form.auth_method {
            AuthMethod::Auto => "Auto",
            AuthMethod::Password => "Password",
            AuthMethod::Key => "Key",
            AuthMethod::Agent => "Agent",
            AuthMethod::Interactive => "Interactive",
        };

        let advanced_section = panel_section(column![
            panel_option_row(
                iced_fonts::bootstrap::link_fourfivedeg(),
                "Host Chaining",
                jump_host_value.to_string(),
            ),
            panel_divider(),
            panel_option_pick(
                iced_fonts::bootstrap::shield_lock(),
                "Auth Method",
                vec!["Auto".into(), "Password".into(), "Key".into(), "Agent".into(), "Interactive".into()],
                auth_value.to_string(),
                Message::EditorAuthMethodChanged,
            ),
            panel_divider(),
            panel_option_pick_jump(
                iced_fonts::bootstrap::diagram_three(),
                "Jump Host",
                {
                    let mut opts = vec!["(none)".to_string()];
                    for c in &self.connections {
                        if Some(c.id) != self.editor_form.editing_id {
                            opts.push(c.label.clone());
                        }
                    }
                    opts
                },
                self.editor_form.jump_host.clone().unwrap_or_else(|| "(none)".into()),
                Message::EditorJumpHostChanged,
            ),
        ]);

        // ── Error ──
        let panel_error: Element<'_, Message> = if let Some(err) = &self.host_panel_error {
            container(Element::from(text(err.clone()).size(11).color(OryxisColors::t().error)))
                .padding(Padding { top: 4.0, right: 16.0, bottom: 4.0, left: 16.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Bottom actions ──
        let save_btn_bg = if has_address { OryxisColors::t().accent } else { OryxisColors::t().bg_surface };
        let save_btn = button(
            container(text("Save").size(14).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::EditorSave)
        .width(Length::Fill)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(save_btn_bg)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let bottom = column![save_btn];
        // ── Layout ──
        let form_scroll = scrollable(
            column![
                address_section,
                Space::new().height(8),
                general_section,
                Space::new().height(8),
                ssh_section,
                Space::new().height(8),
                advanced_section,
                Space::new().height(8),
                panel_error,
            ]
            .padding(Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
        )
        .height(Length::Fill);

        let panel_content = column![
            panel_header,
            form_scroll,
            container(bottom)
                .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 }),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn sidebar_nav_btn<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    view: View,
    is_active: bool,
) -> Element<'a, Message> {
    let bg = if is_active {
        Color { a: 0.15, ..OryxisColors::t().accent }
    } else {
        Color::TRANSPARENT
    };
    let fg = if is_active {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_secondary
    };

    container(
        button(
            container(
                row![
                    icon_widget.size(14).color(fg),
                    Space::new().width(10),
                    text(label).size(13).color(fg),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .on_press(Message::ChangeView(view))
        .width(Length::Fill)
        .style(move |_, status| {
            let hover_bg = match status {
                BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                BtnStatus::Pressed => Color { a: 0.25, ..OryxisColors::t().accent },
                _ => bg,
            };
            button::Style {
                background: Some(Background::Color(hover_bg)),
                border: Border { radius: Radius::from(10.0), ..Default::default() },
                ..Default::default()
            }
        }),
    )
    .padding(Padding { top: 1.0, right: 8.0, bottom: 1.0, left: 8.0 })
    .into()
}

/// A section card with slightly lighter background.
fn panel_section<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        })
        .into()
}

/// A labeled form field inside a section.
fn panel_field<'a>(label: &'a str, input: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(label).size(12).color(OryxisColors::t().text_muted),
        Space::new().height(4),
        input,
    ]
    .into()
}

/// A divider line inside a section.
fn toggle_row<'a>(label: &'a str, value: bool, msg: Message) -> Element<'a, Message> {
    let toggle_bg = if value { OryxisColors::t().success } else { OryxisColors::t().bg_selected };
    let toggle_text = if value { "  \u{25CF}" } else { "\u{25CF}  " };
    row![
        text(label).size(13).color(OryxisColors::t().text_primary),
        Space::new().width(Length::Fill),
        button(text(toggle_text).size(12).color(Color::WHITE))
            .on_press(msg)
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(move |_, _| button::Style {
                background: Some(Background::Color(toggle_bg)),
                border: Border { radius: Radius::from(10.0), ..Default::default() },
                ..Default::default()
            }),
    ].align_y(iced::Alignment::Center)
    .into()
}

fn panel_divider<'a>() -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().border)),
            ..Default::default()
        })
        .into()
}

/// An option row: [icon] [label] ... [value]
fn panel_option_row<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    value: String,
) -> Element<'a, Message> {
    container(
        row![
            icon_widget.size(13).color(OryxisColors::t().text_muted),
            Space::new().width(10),
            text(label).size(13).color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            text(value).size(12).color(OryxisColors::t().text_muted),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
    .into()
}

fn context_menu_item<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
    color: Color,
) -> Element<'a, Message> {
    button(
        row![
            icon_widget.size(12).color(color),
            Space::new().width(8),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .width(Length::Fill)
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// An option row with a pick_list for selection.
fn panel_option_pick<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    options: Vec<String>,
    selected: String,
    on_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    container(
        row![
            icon_widget.size(13).color(OryxisColors::t().text_muted),
            Space::new().width(10),
            text(label).size(13).color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            pick_list(options, Some(selected), on_change).width(120),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
    .into()
}

/// An option row with pick_list for jump host.
fn panel_option_pick_jump<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    options: Vec<String>,
    selected: String,
    on_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    container(
        row![
            icon_widget.size(13).color(OryxisColors::t().text_muted),
            Space::new().width(10),
            text(label).size(13).color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            pick_list(options, Some(selected), on_change).width(140),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
    .into()
}

fn settings_row<'a>(label: &'static str, value: String) -> Element<'a, Message> {
    container(
        row![
            text(label).size(13).color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            text(value).size(13).color(OryxisColors::t().text_primary),
        ],
    )
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .width(300)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    })
    .into()
}

fn styled_button(label: &str, msg: Message, color: Color) -> Element<'_, Message> {
    button(
        container(text(label).size(14).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 8.0, right: 24.0, bottom: 8.0, left: 24.0 }),
    )
    .on_press(msg)
    .style(move |_, _| button::Style {
        background: Some(Background::Color(color)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    })
    .into()
}

fn key_to_named_bytes(key: &keyboard::Key, _modifiers: &keyboard::Modifiers) -> Option<Vec<u8>> {
    if let keyboard::Key::Named(named) = key {
        let bytes: &[u8] = match named {
            keyboard::key::Named::Enter => b"\r",
            keyboard::key::Named::Backspace => b"\x7f",
            keyboard::key::Named::Tab => b"\t",
            keyboard::key::Named::Escape => b"\x1b",
            keyboard::key::Named::ArrowUp => b"\x1b[A",
            keyboard::key::Named::ArrowDown => b"\x1b[B",
            keyboard::key::Named::ArrowRight => b"\x1b[C",
            keyboard::key::Named::ArrowLeft => b"\x1b[D",
            keyboard::key::Named::Home => b"\x1b[H",
            keyboard::key::Named::End => b"\x1b[F",
            keyboard::key::Named::PageUp => b"\x1b[5~",
            keyboard::key::Named::PageDown => b"\x1b[6~",
            keyboard::key::Named::Insert => b"\x1b[2~",
            keyboard::key::Named::Delete => b"\x1b[3~",
            keyboard::key::Named::F1 => b"\x1bOP",
            keyboard::key::Named::F2 => b"\x1bOQ",
            keyboard::key::Named::F3 => b"\x1bOR",
            keyboard::key::Named::F4 => b"\x1bOS",
            keyboard::key::Named::F5 => b"\x1b[15~",
            keyboard::key::Named::F6 => b"\x1b[17~",
            keyboard::key::Named::F7 => b"\x1b[18~",
            keyboard::key::Named::F8 => b"\x1b[19~",
            keyboard::key::Named::F9 => b"\x1b[20~",
            keyboard::key::Named::F10 => b"\x1b[21~",
            keyboard::key::Named::F11 => b"\x1b[23~",
            keyboard::key::Named::F12 => b"\x1b[24~",
            keyboard::key::Named::Space => b" ",
            _ => return None,
        };
        Some(bytes.to_vec())
    } else {
        None
    }
}

fn ctrl_key_bytes(key: &keyboard::Key) -> Option<Vec<u8>> {
    if let keyboard::Key::Character(c) = key {
        let ch = c.as_str().bytes().next()?;
        let ctrl = match ch {
            b'a'..=b'z' => ch - b'a' + 1,
            b'A'..=b'Z' => ch - b'A' + 1,
            b'[' => 27,
            b'\\' => 28,
            b']' => 29,
            b'^' => 30,
            b'_' => 31,
            _ => return None,
        };
        Some(vec![ctrl])
    } else {
        None
    }
}
