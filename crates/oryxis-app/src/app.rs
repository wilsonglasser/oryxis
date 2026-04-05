use iced::border::Radius;
use iced::keyboard;
use iced::widget::{
    button, canvas, column, container, image, pick_list, row, scrollable, text, text_editor,
    text_input, Space,
};
use iced::futures::SinkExt;
use iced::{Background, Border, Color, Element, Length, Padding, Subscription, Task, Theme};
use iced::widget::button::Status as BtnStatus;

use oryxis_core::models::connection::{AuthMethod, Connection};
use oryxis_core::models::group::Group;
use oryxis_core::models::key::SshKey;
use oryxis_ssh::{SshEngine, SshSession};
use oryxis_terminal::widget::{TerminalState, TerminalView};
use oryxis_vault::{VaultError, VaultStore};

use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use crate::theme::OryxisColors;

// Bootstrap icon helpers are available via iced_fonts::bootstrap::*
// e.g. iced_fonts::bootstrap::terminal() returns a Text widget

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A terminal tab — either a local shell or an SSH session.
struct TerminalTab {
    id: Uuid,
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
    /// If editing, the connection ID.
    editing_id: Option<Uuid>,
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self {
            label: String::new(),
            hostname: String::new(),
            port: "22".into(),
            username: String::new(),
            password: String::new(),
            auth_method: AuthMethod::Password,
            group_name: String::new(),
            selected_key: None,
            jump_host: None,
            editing_id: None,
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

    // Data
    connections: Vec<Connection>,
    groups: Vec<Group>,

    // UI state
    active_view: View,
    selected_connection: Option<usize>,

    // Tabs
    tabs: Vec<TerminalTab>,
    active_tab: Option<usize>,

    // Connection editor
    show_host_panel: bool,
    editor_form: ConnectionForm,
    host_panel_error: Option<String>,

    // Connection context menu
    context_menu_open: Option<usize>,  // index of connection with open menu

    // Keys
    keys: Vec<SshKey>,
    show_key_panel: bool,
    key_import_label: String,
    key_import_content: text_editor::Content,
    key_import_pem: String,  // raw string for import
    key_error: Option<String>,
    key_success: Option<String>,

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

    // SSH engine
    ssh_engine: SshEngine,
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

#[derive(Debug, Clone)]
pub enum Message {
    // Vault
    VaultPasswordChanged(String),
    VaultUnlock,
    VaultSetup,

    // Navigation
    SelectConnection(usize),
    ChangeView(View),

    // Tabs
    SelectTab(usize),
    CloseTab(usize),

    // Terminal I/O
    PtyOutput(usize, Vec<u8>),  // (tab_index, bytes)
    KeyboardEvent(keyboard::Event),

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
    EditorSave,
    EditorCancel,
    DeleteConnection(usize),
    ToggleContextMenu(usize),

    // SSH
    ConnectSsh(usize),
    SshConnected(usize, Arc<SshSession>),
    SshNewKnownHosts(Vec<oryxis_core::models::known_host::KnownHost>),
    SshDisconnected(usize),
    SshError(String),

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
}

/// Internal message type for SSH connection streams.
enum SshStreamMsg {
    Connected(Arc<SshSession>),
    NewKnownHosts(Vec<oryxis_core::models::known_host::KnownHost>),
    Data(Vec<u8>),
    Error(String),
    Disconnected,
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
                connections: Vec::new(),
                groups: Vec::new(),
                active_view: View::Dashboard,
                selected_connection: None,
                tabs: Vec::new(),
                active_tab: None,
                show_host_panel: false,
                editor_form: ConnectionForm::default(),
                host_panel_error: None,
                context_menu_open: None,
                keys: Vec::new(),
                show_key_panel: false,
                key_import_label: String::new(),
                key_import_content: text_editor::Content::new(),
                key_import_pem: String::new(),
                key_error: None,
                key_success: None,
                snippets: Vec::new(),
                known_hosts: Vec::new(),
                logs: Vec::new(),
                show_snippet_panel: false,
                snippet_label: String::new(),
                snippet_command: String::new(),
                snippet_editing_id: None,
                snippet_error: None,
                ssh_engine: SshEngine::new(),
            },
            Task::none(),
        )
    }

    fn load_data_from_vault(&mut self) {
        if let Some(vault) = &self.vault {
            self.connections = vault.list_connections().unwrap_or_default();
            self.groups = vault.list_groups().unwrap_or_default();
            self.keys = vault.list_keys().unwrap_or_default();
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
                background: OryxisColors::BG_PRIMARY,
                text: OryxisColors::TEXT_PRIMARY,
                primary: OryxisColors::ACCENT,
                success: OryxisColors::SUCCESS,
                warning: OryxisColors::WARNING,
                danger: OryxisColors::ERROR,
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
            Message::SelectConnection(idx) => {
                self.selected_connection = Some(idx);
            }
            Message::ChangeView(view) => {
                self.active_view = view;
                self.active_tab = None; // Show grid view, not terminal
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
                if let Some(tab) = self.tabs.get(tab_idx) {
                    if let Ok(mut state) = tab.terminal.lock() {
                        state.process(&bytes);
                    }
                }
            }
            Message::KeyboardEvent(event) => {
                if let Some(tab_idx) = self.active_tab {
                    if let keyboard::Event::KeyPressed {
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

                        if let Some(bytes) = bytes {
                            if let Some(tab) = self.tabs.get(tab_idx) {
                                if let Some(ref ssh) = tab.ssh_session {
                                    let _ = ssh.write(&bytes);
                                } else if let Ok(mut state) = tab.terminal.lock() {
                                    state.write(&bytes);
                                }
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
                if let Some(conn) = self.connections.get(idx) {
                    self.show_host_panel = true;
                    self.host_panel_error = None;
                    self.context_menu_open = None;
                    self.editor_form = ConnectionForm {
                        label: conn.label.clone(),
                        hostname: conn.hostname.clone(),
                        port: conn.port.to_string(),
                        username: conn.username.clone().unwrap_or_default(),
                        password: String::new(), // Don't prefill password
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
                        editing_id: Some(conn.id),
                    };
                }
            }
            Message::EditorLabelChanged(v) => self.editor_form.label = v,
            Message::EditorHostnameChanged(v) => self.editor_form.hostname = v,
            Message::EditorPortChanged(v) => self.editor_form.port = v,
            Message::EditorUsernameChanged(v) => self.editor_form.username = v,
            Message::EditorPasswordChanged(v) => self.editor_form.password = v,
            Message::EditorAuthMethodChanged(v) => {
                self.editor_form.auth_method = match v.as_str() {
                    "Key" => AuthMethod::Key,
                    "Agent" => AuthMethod::Agent,
                    "Interactive" => AuthMethod::Interactive,
                    _ => AuthMethod::Password,
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
                conn.jump_chain = self.editor_form.jump_host.as_ref()
                    .and_then(|label| {
                        self.connections.iter().find(|c| c.label == *label).map(|c| vec![c.id])
                    })
                    .unwrap_or_default();
                conn.updated_at = chrono::Utc::now();

                let password = if self.editor_form.password.is_empty() {
                    None
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
            Message::ToggleContextMenu(idx) => {
                if self.context_menu_open == Some(idx) {
                    self.context_menu_open = None;
                } else {
                    self.context_menu_open = Some(idx);
                }
            }
            Message::DeleteConnection(idx) => {
                if let Some(conn) = self.connections.get(idx) {
                    let id = conn.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_connection(&id);
                        self.context_menu_open = None;
                        self.show_host_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }

            // -- SSH connection --
            Message::ConnectSsh(idx) => {
                if let Some(conn) = self.connections.get(idx).cloned() {
                    let password = self
                        .vault
                        .as_ref()
                        .and_then(|v| v.get_connection_password(&conn.id).ok().flatten());

                    let private_key = if conn.auth_method == AuthMethod::Key {
                        conn.key_id.and_then(|kid| {
                            self.vault.as_ref().and_then(|v| v.get_key_private(&kid).ok().flatten())
                        })
                    } else {
                        None
                    };

                    // Build resolver for jump hosts
                    let resolver = if !conn.jump_chain.is_empty() {
                        let mut passwords = std::collections::HashMap::new();
                        let mut keys = std::collections::HashMap::new();
                        for jid in &conn.jump_chain {
                            if let Some(vault) = &self.vault {
                                if let Ok(Some(pw)) = vault.get_connection_password(jid) {
                                    passwords.insert(*jid, pw);
                                }
                            }
                            // Get jump host's key if it uses key auth
                            if let Some(jconn) = self.connections.iter().find(|c| c.id == *jid) {
                                if let Some(kid) = jconn.key_id {
                                    if let Some(vault) = &self.vault {
                                        if let Ok(Some(pk)) = vault.get_key_private(&kid) {
                                            keys.insert(*jid, pk);
                                        }
                                    }
                                }
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

                    match TerminalState::new_no_pty(120, 40) {
                        Ok(state) => {
                            let label = conn.label.clone();
                            let terminal = Arc::new(Mutex::new(state));
                            let tab_idx = self.tabs.len();

                            self.tabs.push(TerminalTab {
                                id: Uuid::new_v4(),
                                label: format!("{} (connecting...)", label),
                                terminal: Arc::clone(&terminal),
                                ssh_session: None,
                            });
                            self.active_tab = Some(tab_idx);
                            self.active_view = View::Terminal;

                            // Build TOFU callback with known hosts snapshot
                            // TOFU: verify/record host keys
                            let known_hosts_snapshot: Arc<Mutex<Vec<oryxis_core::models::known_host::KnownHost>>> =
                                Arc::new(Mutex::new(self.known_hosts.clone()));
                            let new_hosts: Arc<Mutex<Vec<oryxis_core::models::known_host::KnownHost>>> =
                                Arc::new(Mutex::new(Vec::new()));
                            let kh_ref = known_hosts_snapshot.clone();
                            let new_ref = new_hosts.clone();
                            let host_key_cb: oryxis_ssh::engine::HostKeyCallback = Arc::new(move |hostname: &str, port: u16, key_type: &str, fingerprint: &str| {
                                let hosts = kh_ref.lock().unwrap();
                                if let Some(existing) = hosts.iter().find(|h| h.hostname == hostname && h.port == port) {
                                    if existing.fingerprint != fingerprint {
                                        tracing::warn!(
                                            "HOST KEY CHANGED for {}:{} — expected {}, got {}",
                                            hostname, port, existing.fingerprint, fingerprint
                                        );
                                        return false;
                                    }
                                    return true; // Known, matches
                                }
                                // New host — record it
                                tracing::info!("New host key for {}:{} — {}", hostname, port, fingerprint);
                                let kh = oryxis_core::models::known_host::KnownHost::new(hostname, port, key_type, fingerprint);
                                new_ref.lock().unwrap().push(kh);
                                true
                            });

                            let stream = iced::stream::channel::<SshStreamMsg>(128, move |mut sender: iced::futures::channel::mpsc::Sender<SshStreamMsg>| {
                                async move {
                                    let engine = SshEngine::new().with_host_key_cb(host_key_cb);
                                    match engine
                                        .connect_with_resolver(&conn, password.as_deref(), private_key.as_deref(), 120, 40, resolver.as_ref())
                                        .await
                                    {
                                        Ok((session, mut rx)) => {
                                            let session = Arc::new(session);
                                            let _ = sender
                                                .send(SshStreamMsg::Connected(session.clone()))
                                                .await;
                                            // Send new known hosts for saving
                                            let new_kh = new_hosts.lock().unwrap().drain(..).collect::<Vec<_>>();
                                            if !new_kh.is_empty() {
                                                let _ = sender.send(SshStreamMsg::NewKnownHosts(new_kh)).await;
                                            }
                                            // Forward data
                                            while let Some(data) = rx.recv().await {
                                                if sender
                                                    .send(SshStreamMsg::Data(data))
                                                    .await
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                            let _ = sender.send(SshStreamMsg::Disconnected).await;
                                        }
                                        Err(e) => {
                                            let _ = sender
                                                .send(SshStreamMsg::Error(e.to_string()))
                                                .await;
                                        }
                                    }
                                }
                            });

                            return Task::stream(stream).map(move |msg| match msg {
                                SshStreamMsg::Connected(session) => {
                                    Message::SshConnected(tab_idx, session)
                                }
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
                    let label = tab.label.replace(" (connecting...)", "");
                    tab.label = label.clone();
                    tab.ssh_session = Some(session);
                    tracing::info!("SSH connected: {}", tab.label);
                    // Log
                    if let Some(vault) = &self.vault {
                        let entry = oryxis_core::models::log_entry::LogEntry::new(
                            &label, &label, oryxis_core::models::log_entry::LogEvent::Connected, "Session established",
                        );
                        let _ = vault.add_log(&entry);
                    }
                }
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
            Message::SshError(err) => {
                tracing::error!("SSH error: {}", err);
                // Log
                if let Some(vault) = &self.vault {
                    let entry = oryxis_core::models::log_entry::LogEntry::new(
                        "unknown", "unknown", oryxis_core::models::log_entry::LogEvent::Error, &err,
                    );
                    let _ = vault.add_log(&entry);
                }
                if let Some(last) = self.tabs.last() {
                    if last.label.contains("connecting") {
                        self.tabs.pop();
                        if self.tabs.is_empty() {
                            self.active_tab = None;
                            self.active_view = View::Dashboard;
                        } else {
                            self.active_tab = Some(self.tabs.len() - 1);
                        }
                    }
                }
                self.host_panel_error = Some(format!("SSH: {}", err));
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
                    if let Some(tab_idx) = self.active_tab {
                        if let Some(tab) = self.tabs.get(tab_idx) {
                            if let Some(ref ssh) = tab.ssh_session {
                                let _ = ssh.write(cmd.as_bytes());
                            } else if let Ok(mut state) = tab.terminal.lock() {
                                state.write(cmd.as_bytes());
                            }
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
                match TerminalState::new(120, 40) {
                    Ok((state, rx)) => {
                        let tab_idx = self.tabs.len();
                        self.tabs.push(TerminalTab {
                            id: Uuid::new_v4(),
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
            }
            Message::HideKeyPanel => {
                self.show_key_panel = false;
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
                    Ok(generated) => {
                        if let Some(vault) = &self.vault {
                            match vault.save_key(&generated.key, Some(&generated.private_pem)) {
                                Ok(()) => {
                                    self.key_error = None;
                                    self.key_success = Some(format!("Key '{}' imported", label));
                                    self.key_import_label.clear();
                                    self.key_import_content = text_editor::Content::new();
                                    self.key_import_pem.clear();
                                    self.show_key_panel = false;
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
        let logo = image(image::Handle::from_path("resources/logo_128.png"))
            .width(64)
            .height(64);
        let title = text("Welcome to Oryxis").size(28).color(OryxisColors::TEXT_PRIMARY);
        let subtitle = text("Create a master password to secure your vault.")
            .size(14)
            .color(OryxisColors::TEXT_SECONDARY);

        let input = text_input("Master password...", &self.vault_password_input)
            .on_input(Message::VaultPasswordChanged)
            .on_submit(Message::VaultSetup)
            .secure(true)
            .padding(12)
            .width(300);

        let btn = styled_button("Create Vault", Message::VaultSetup, OryxisColors::ACCENT);

        let error = if let Some(err) = &self.vault_error {
            Element::from(text(err.clone()).size(13).color(OryxisColors::ERROR))
        } else {
            Space::new().height(0).into()
        };

        container(
            column![logo, Space::new().height(16), title, Space::new().height(8), subtitle, Space::new().height(24), input, Space::new().height(12), btn, Space::new().height(8), error]
                .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::BG_PRIMARY)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_vault_unlock(&self) -> Element<'_, Message> {
        let logo = image(image::Handle::from_path("resources/logo_128.png"))
            .width(64)
            .height(64);
        let title = text("Oryxis").size(28).color(OryxisColors::ACCENT);
        let subtitle = text("Enter your master password to unlock.")
            .size(14)
            .color(OryxisColors::TEXT_SECONDARY);

        let input = text_input("Master password...", &self.vault_password_input)
            .on_input(Message::VaultPasswordChanged)
            .on_submit(Message::VaultUnlock)
            .secure(true)
            .padding(12)
            .width(300);

        let btn = styled_button("Unlock", Message::VaultUnlock, OryxisColors::ACCENT);

        let error = if let Some(err) = &self.vault_error {
            Element::from(text(err.clone()).size(13).color(OryxisColors::ERROR))
        } else {
            Space::new().height(0).into()
        };

        container(
            column![logo, Space::new().height(16), title, Space::new().height(8), subtitle, Space::new().height(24), input, Space::new().height(12), btn, Space::new().height(8), error]
                .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::BG_PRIMARY)),
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_vault_error(&self, msg: &str) -> Element<'_, Message> {
        let msg = msg.to_string();
        container(
            text(msg).size(16).color(OryxisColors::ERROR),
        )
        .center(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::BG_PRIMARY)),
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
                background: Some(Background::Color(OryxisColors::BG_PRIMARY)),
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
        if !nav_label.is_empty() && self.active_tab.is_none() {
            let tab = container(
                text(nav_label).size(12).color(OryxisColors::ACCENT),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SURFACE)),
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
            let tab_bg = if is_active { OryxisColors::BG_SURFACE } else { Color::TRANSPARENT };
            let tab_fg = if is_active { OryxisColors::TEXT_PRIMARY } else { OryxisColors::TEXT_MUTED };

            let close_btn = button(text("x").size(10).color(OryxisColors::TEXT_MUTED))
                .on_press(Message::CloseTab(idx))
                .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });

            let tab_btn = button(
                container(
                    row![
                        text(&tab.label).size(12).color(tab_fg),
                        Space::new().width(8),
                        close_btn,
                    ].align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 14.0 }),
            )
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

        container(row(items).align_y(iced::Alignment::End))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SIDEBAR)),
                ..Default::default()
            })
            .into()
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        // Logo
        let logo = image(image::Handle::from_path("resources/logo_64.png"))
            .width(28)
            .height(28);
        let header = container(
            row![
                logo,
                Space::new().width(10),
                text("ORYXIS").size(16).color(OryxisColors::ACCENT),
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
                    text("+").size(13).color(OryxisColors::TEXT_MUTED),
                    Space::new().width(10),
                    text("Local Shell").size(12).color(OryxisColors::TEXT_MUTED),
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
            .width(180)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SIDEBAR)),
                ..Default::default()
            })
            .into()
    }

    fn view_content(&self) -> Element<'_, Message> {
        // If a terminal tab is active, show terminal
        // Otherwise show the grid view for the current nav item
        let content: Element<'_, Message> = if self.active_tab.is_some() {
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
                background: Some(Background::Color(OryxisColors::BG_PRIMARY)),
                ..Default::default()
            })
            .into()
    }

    fn view_dashboard(&self) -> Element<'_, Message> {
        // ── Toolbar ──
        let toolbar = container(
            row![
                text("Hosts").size(20).color(OryxisColors::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(
                    container(
                        row![
                            text("+").size(14).color(OryxisColors::TEXT_PRIMARY),
                            Space::new().width(6),
                            text("HOST").size(12).color(OryxisColors::TEXT_PRIMARY),
                        ].align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 }),
                )
                .on_press(Message::ShowNewConnection)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::ACCENT)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Status ──
        let status: Element<'_, Message> = if let Some(err) = &self.host_panel_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::ERROR)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        // ── Host cards grid ──
        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if self.connections.is_empty() {
            let empty = container(
                column![
                    iced_fonts::bootstrap::hdd_network().size(28).color(OryxisColors::TEXT_MUTED),
                    Space::new().height(8),
                    text("No hosts yet").size(13).color(OryxisColors::TEXT_MUTED),
                    Space::new().height(4),
                    text("Click + HOST to add one").size(11).color(OryxisColors::TEXT_MUTED),
                ].align_x(iced::Alignment::Center),
            )
            .padding(24)
            .width(220)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SURFACE)),
                border: Border { radius: Radius::from(10.0), color: OryxisColors::BORDER, width: 1.0 },
                ..Default::default()
            });
            cards.push(empty.into());
        }

        // Group connections — only show folder headers for grouped connections
        let mut current_group: Option<Option<Uuid>> = None;
        for (idx, conn) in self.connections.iter().enumerate() {
            if current_group.as_ref() != Some(&conn.group_id) {
                current_group = Some(conn.group_id);
                // Only show group header if the connection has a group
                if let Some(gid) = conn.group_id {
                    if let Some(group) = self.groups.iter().find(|g| g.id == gid) {
                        // Pad current row before header
                        if !cards.is_empty() && cards.len() % 3 != 0 {
                            while cards.len() % 3 != 0 {
                                cards.push(Space::new().width(220).into());
                            }
                        }
                        cards.push(
                            container(
                                row![
                                    iced_fonts::bootstrap::folder().size(13).color(OryxisColors::TEXT_MUTED),
                                    Space::new().width(8),
                                    text(&group.label).size(13).color(OryxisColors::TEXT_MUTED),
                                ].align_y(iced::Alignment::Center),
                            )
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 4.0, left: 0.0 })
                            .width(220 * 3 + 12 * 2)
                            .into(),
                        );
                    }
                }
            }
            let is_connected = self.tabs.iter().any(|t| t.label == conn.label);
            let auth_label = match conn.auth_method {
                AuthMethod::Password => "Password",
                AuthMethod::Key => "Key",
                AuthMethod::Agent => "Agent",
                AuthMethod::Interactive => "Interactive",
            };
            let subtitle = format!("{}@{}:{} · {}", conn.username.as_deref().unwrap_or("root"), conn.hostname, conn.port, auth_label);

            // Host icon box
            let icon_color = if is_connected { OryxisColors::SUCCESS } else { OryxisColors::ACCENT };
            let icon_box = container(iced_fonts::bootstrap::hdd_network().size(14).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
                .style(move |_| container::Style {
                    background: Some(Background::Color(icon_color)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            // Edit button
            let edit_btn = button(text("...").size(12).color(OryxisColors::TEXT_MUTED))
                .on_press(Message::EditConnection(idx))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });

            let card_content = row![
                icon_box,
                Space::new().width(12),
                column![
                    text(&conn.label).size(13).color(OryxisColors::TEXT_PRIMARY),
                    Space::new().height(2),
                    text(subtitle).size(10).color(OryxisColors::TEXT_MUTED),
                ].width(Length::Fill),
                edit_btn,
            ].align_y(iced::Alignment::Center);

            let card = button(
                container(card_content).padding(16),
            )
            .on_press(Message::ConnectSsh(idx))
            .width(220)
            .style(move |_, status| {
                let (bg, border_color, border_w) = match status {
                    BtnStatus::Hovered => (OryxisColors::BG_HOVER, OryxisColors::ACCENT, 1.5),
                    BtnStatus::Pressed => (OryxisColors::BG_SELECTED, OryxisColors::ACCENT, 2.0),
                    _ => (OryxisColors::BG_SURFACE, OryxisColors::BORDER, 1.0),
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(10.0), color: border_color, width: border_w },
                    ..Default::default()
                }
            });

            cards.push(card.into());
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
                current_row.push(Space::new().width(220).into());
            }
            grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        let grid = scrollable(
            column(grid_rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        // ── Main + side panel ──
        let main_content = column![toolbar, status, grid]
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

    fn view_terminal(&self) -> Element<'_, Message> {
        let terminal_area: Element<'_, Message> = if let Some(tab_idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(tab_idx) {
                let view = TerminalView::new(Arc::clone(&tab.terminal));
                canvas(view)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            } else {
                container(text("No active session").size(14).color(OryxisColors::TEXT_MUTED))
                    .center(Length::Fill).into()
            }
        } else {
            container(text("No active session").size(14).color(OryxisColors::TEXT_MUTED))
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
        let toolbar = container(
            row![
                text("Keychain").size(20).color(OryxisColors::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(
                    container(
                        row![
                            text("+").size(14).color(OryxisColors::TEXT_PRIMARY),
                            Space::new().width(6),
                            text("KEY").size(12).color(OryxisColors::TEXT_PRIMARY),
                        ]
                        .align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 }),
                )
                .on_press(Message::ShowKeyPanel)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::ACCENT)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Status message ──
        let status: Element<'_, Message> = if let Some(err) = &self.key_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::ERROR)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 })
                .into()
        } else if let Some(ok) = &self.key_success {
            container(Element::from(text(ok.clone()).size(12).color(OryxisColors::SUCCESS)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Keys grid ──
        let section_title = container(
            text("Keys").size(14).color(OryxisColors::TEXT_MUTED),
        )
        .padding(Padding { top: 4.0, right: 24.0, bottom: 8.0, left: 24.0 });

        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if self.keys.is_empty() {
            let empty_card = container(
                column![
                    iced_fonts::bootstrap::key().size(28).color(OryxisColors::TEXT_MUTED),
                    Space::new().height(8),
                    text("No keys yet").size(13).color(OryxisColors::TEXT_MUTED),
                    Space::new().height(4),
                    text("Click + KEY to add one").size(11).color(OryxisColors::TEXT_MUTED),
                ]
                .align_x(iced::Alignment::Center),
            )
            .padding(24)
            .width(220)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SURFACE)),
                border: Border { radius: Radius::from(10.0), color: OryxisColors::BORDER, width: 1.0 },
                ..Default::default()
            });
            cards.push(empty_card.into());
        }

        for (idx, key) in self.keys.iter().enumerate() {
            let algo = format!("Type {}", key.algorithm);
            let icon_box = container(iced_fonts::bootstrap::key().size(18).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::ACCENT)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });
            let del_btn = button(text("×").size(14).color(OryxisColors::TEXT_MUTED))
                .on_press(Message::DeleteKey(idx))
                .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 4.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });
            let card = container(
                row![
                    icon_box,
                    Space::new().width(12),
                    column![
                        text(&key.label).size(13).color(OryxisColors::TEXT_PRIMARY),
                        Space::new().height(2),
                        text(algo).size(11).color(OryxisColors::TEXT_MUTED),
                    ].width(Length::Fill),
                    del_btn,
                ].align_y(iced::Alignment::Center),
            )
            .padding(16)
            .width(220)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SURFACE)),
                border: Border { radius: Radius::from(10.0), color: OryxisColors::BORDER, width: 1.0 },
                ..Default::default()
            });
            cards.push(card.into());
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
                current_row.push(Space::new().width(220).into());
            }
            grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        let grid = scrollable(
            column(grid_rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill);

        // ── Main content (grid) ──
        let main_content = column![toolbar, status, section_title, grid]
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Side panel (import key) ──
        if self.show_key_panel {
            let panel = self.view_key_import_panel();
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

        // Panel header
        let panel_header = container(
            row![
                text("Add Key").size(18).color(OryxisColors::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::TEXT_MUTED))
                    .on_press(Message::HideKeyPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::BG_SURFACE)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Name field
        let name_field = column![
            text("Name").size(12).color(OryxisColors::TEXT_SECONDARY),
            Space::new().height(6),
            text_input("my-server-key", &self.key_import_label)
                .on_input(Message::KeyImportLabelChanged)
                .padding(10),
        ];

        // File selector button
        let browse_btn = button(
            container(
                row![
                    text("Select File...").size(13).color(OryxisColors::TEXT_PRIMARY),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .on_press(Message::BrowseKeyFile)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::ACCENT)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        // Status indicator
        let file_status: Element<'_, Message> = if has_content {
            container(
                row![
                    text("V").size(12).color(OryxisColors::SUCCESS),
                    Space::new().width(6),
                    text(format!("Loaded ({} bytes)", self.key_import_pem.len()))
                        .size(11).color(OryxisColors::SUCCESS),
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
            Element::from(text(err.clone()).size(11).color(OryxisColors::ERROR))
        } else {
            Space::new().height(0).into()
        };

        // Save button
        let save_btn = button(
            container(text("Save Key").size(13).color(OryxisColors::TEXT_PRIMARY))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::ImportKey)
        .width(Length::Fill)
        .style(move |_, _| {
            let bg = if has_content { OryxisColors::ACCENT } else { OryxisColors::BG_SURFACE };
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
                    text("Private Key").size(12).color(OryxisColors::TEXT_SECONDARY),
                    Space::new().height(6),
                    browse_btn,
                    Space::new().height(8),
                    file_status,
                    Space::new().height(8),
                    text("Key Content").size(12).color(OryxisColors::TEXT_SECONDARY),
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
            .width(340)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SIDEBAR)),
                border: Border { color: OryxisColors::BORDER, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }

    fn view_snippets(&self) -> Element<'_, Message> {
        let toolbar = container(
            row![
                text("Snippets").size(20).color(OryxisColors::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(
                    container(
                        row![
                            text("+").size(14).color(OryxisColors::TEXT_PRIMARY),
                            Space::new().width(6),
                            text("SNIPPET").size(12).color(OryxisColors::TEXT_PRIMARY),
                        ].align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 }),
                )
                .on_press(Message::ShowSnippetPanel)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::ACCENT)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let status: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::ERROR)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        let section_title = container(
            text("Commands").size(14).color(OryxisColors::TEXT_MUTED),
        )
        .padding(Padding { top: 4.0, right: 24.0, bottom: 8.0, left: 24.0 });

        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if self.snippets.is_empty() {
            let empty = container(
                column![
                    iced_fonts::bootstrap::code_square().size(28).color(OryxisColors::TEXT_MUTED),
                    Space::new().height(8),
                    text("No snippets yet").size(13).color(OryxisColors::TEXT_MUTED),
                    Space::new().height(4),
                    text("Click + SNIPPET to add one").size(11).color(OryxisColors::TEXT_MUTED),
                ].align_x(iced::Alignment::Center),
            )
            .padding(24)
            .width(220)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SURFACE)),
                border: Border { radius: Radius::from(10.0), color: OryxisColors::BORDER, width: 1.0 },
                ..Default::default()
            });
            cards.push(empty.into());
        }

        for (idx, snip) in self.snippets.iter().enumerate() {
            let icon_box = container(iced_fonts::bootstrap::code_square().size(14).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::ACCENT)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            let edit_btn = button(text("...").size(12).color(OryxisColors::TEXT_MUTED))
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
                            text(&snip.label).size(13).color(OryxisColors::TEXT_PRIMARY),
                            Space::new().height(2),
                            text(cmd_preview).size(10).color(OryxisColors::TEXT_MUTED).font(iced::Font::MONOSPACE),
                        ].width(Length::Fill),
                        edit_btn,
                    ].align_y(iced::Alignment::Center),
                )
                .padding(16),
            )
            .on_press(Message::RunSnippet(idx))
            .width(220)
            .style(move |_, status| {
                let (bg, bc, bw) = match status {
                    BtnStatus::Hovered => (OryxisColors::BG_HOVER, OryxisColors::ACCENT, 1.5),
                    BtnStatus::Pressed => (OryxisColors::BG_SELECTED, OryxisColors::ACCENT, 2.0),
                    _ => (OryxisColors::BG_SURFACE, OryxisColors::BORDER, 1.0),
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
                current_row.push(Space::new().width(220).into());
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
                text(title).size(18).color(OryxisColors::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::TEXT_MUTED))
                    .on_press(Message::HideSnippetPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::BG_SURFACE)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        let form = column![
            text("Name").size(12).color(OryxisColors::TEXT_SECONDARY),
            Space::new().height(4),
            text_input("restart-nginx", &self.snippet_label)
                .on_input(Message::SnippetLabelChanged)
                .padding(10),
            Space::new().height(14),
            text("Command").size(12).color(OryxisColors::TEXT_SECONDARY),
            Space::new().height(4),
            text_input("sudo systemctl restart nginx", &self.snippet_command)
                .on_input(Message::SnippetCommandChanged)
                .padding(10),
        ];

        let panel_error: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            Element::from(text(err.clone()).size(11).color(OryxisColors::ERROR))
        } else {
            Space::new().height(0).into()
        };

        let save_btn = button(
            container(text("Save").size(13).color(OryxisColors::TEXT_PRIMARY))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill).center_x(Length::Fill),
        )
        .on_press(Message::SaveSnippet)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::ACCENT)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let mut bottom = column![save_btn];
        if let Some(edit_id) = self.snippet_editing_id {
            if let Some(idx) = self.snippets.iter().position(|s| s.id == edit_id) {
                let del_btn = button(
                    container(text("Delete").size(13).color(OryxisColors::ERROR))
                        .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                        .width(Length::Fill).center_x(Length::Fill),
                )
                .on_press(Message::DeleteSnippet(idx))
                .width(Length::Fill)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::ERROR, width: 1.0 },
                    ..Default::default()
                });
                bottom = bottom.push(Space::new().height(8));
                bottom = bottom.push(del_btn);
            }
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
            .width(340)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SIDEBAR)),
                border: Border { color: OryxisColors::BORDER, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }

    fn view_known_hosts(&self) -> Element<'_, Message> {
        let toolbar = container(
            text("Known Hosts").size(20).color(OryxisColors::TEXT_PRIMARY),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();

        if self.known_hosts.is_empty() {
            rows.push(
                container(
                    text("No known hosts yet. They will be added automatically when you connect to servers.")
                        .size(13).color(OryxisColors::TEXT_MUTED),
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

            let del_btn = button(text("x").size(11).color(OryxisColors::TEXT_MUTED))
                .on_press(Message::DeleteKnownHost(idx))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });

            let entry = container(
                row![
                    iced_fonts::bootstrap::shield_check().size(14).color(OryxisColors::SUCCESS),
                    Space::new().width(12),
                    column![
                        text(format!("{}:{}", kh.hostname, kh.port)).size(13).color(OryxisColors::TEXT_PRIMARY),
                        Space::new().height(2),
                        text(format!("{} · {}", kh.key_type, fp_short)).size(10).color(OryxisColors::TEXT_MUTED).font(iced::Font::MONOSPACE),
                        Space::new().height(2),
                        text(format!("Last seen: {}", seen)).size(10).color(OryxisColors::TEXT_MUTED),
                    ].width(Length::Fill),
                    del_btn,
                ].align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 10.0, right: 16.0, bottom: 10.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SURFACE)),
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
                text("History").size(20).color(OryxisColors::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(
                    container(text("Clear").size(12).color(OryxisColors::TEXT_MUTED))
                        .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 }),
                )
                .on_press(Message::ClearLogs)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => Color::from_rgba(0.92, 0.33, 0.38, 0.15),
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(8.0), color: OryxisColors::BORDER, width: 1.0 },
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
                        .size(13).color(OryxisColors::TEXT_MUTED),
                )
                .padding(16)
                .into(),
            );
        }

        for entry in &self.logs {
            let (event_icon, event_color) = match entry.event {
                oryxis_core::models::log_entry::LogEvent::Connected => {
                    (iced_fonts::bootstrap::check_circle(), OryxisColors::SUCCESS)
                }
                oryxis_core::models::log_entry::LogEvent::Disconnected => {
                    (iced_fonts::bootstrap::dash_circle(), OryxisColors::TEXT_MUTED)
                }
                oryxis_core::models::log_entry::LogEvent::AuthFailed => {
                    (iced_fonts::bootstrap::x_circle(), OryxisColors::WARNING)
                }
                oryxis_core::models::log_entry::LogEvent::Error => {
                    (iced_fonts::bootstrap::exclamation_circle(), OryxisColors::ERROR)
                }
            };

            let ts = entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();

            let log_row = container(
                row![
                    event_icon.size(14).color(event_color),
                    Space::new().width(12),
                    column![
                        row![
                            text(&entry.connection_label).size(13).color(OryxisColors::TEXT_PRIMARY),
                            Space::new().width(8),
                            text(format!("{}", entry.event)).size(11).color(event_color),
                        ].align_y(iced::Alignment::Center),
                        Space::new().height(2),
                        text(&entry.message).size(11).color(OryxisColors::TEXT_MUTED),
                    ].width(Length::Fill),
                    text(ts).size(10).color(OryxisColors::TEXT_MUTED),
                ].align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::BG_SURFACE)),
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
        let toolbar = container(
            text("Settings").size(20).color(OryxisColors::TEXT_PRIMARY),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let stats = column![
            text("Vault").size(14).color(OryxisColors::TEXT_MUTED),
            Space::new().height(8),
            settings_row("Hosts", self.connections.len().to_string()),
            Space::new().height(6),
            settings_row("Keys", self.keys.len().to_string()),
            Space::new().height(6),
            settings_row("Snippets", self.snippets.len().to_string()),
            Space::new().height(6),
            settings_row("Groups", self.groups.len().to_string()),
            Space::new().height(24),
            text("Security").size(14).color(OryxisColors::TEXT_MUTED),
            Space::new().height(8),
            button(
                container(
                    row![
                        iced_fonts::bootstrap::lock().size(14).color(OryxisColors::WARNING),
                        Space::new().width(10),
                        text("Lock Vault").size(13).color(OryxisColors::WARNING),
                    ].align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 }),
            )
            .on_press(Message::LockVault)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Color::from_rgba(0.95, 0.73, 0.25, 0.15),
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::WARNING, width: 1.0 },
                    ..Default::default()
                }
            }),
            Space::new().height(24),
            text("About").size(14).color(OryxisColors::TEXT_MUTED),
            Space::new().height(8),
            text("Oryxis v0.1.0").size(13).color(OryxisColors::TEXT_SECONDARY),
            text("A modern SSH client built in Rust").size(12).color(OryxisColors::TEXT_MUTED),
            Space::new().height(4),
            text("Iced + russh + alacritty_terminal").size(11).color(OryxisColors::TEXT_MUTED),
        ];

        container(
            column![
                toolbar,
                scrollable(
                    container(stats)
                        .padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                ).height(Length::Fill),
            ]
            .width(Length::Fill),
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
            OryxisColors::SUCCESS
        } else {
            OryxisColors::TEXT_MUTED
        };

        container(
            row![
                text(status_text).size(12).color(status_color),
                Space::new().width(Length::Fill),
                text("Oryxis v0.1.0").size(12).color(OryxisColors::TEXT_MUTED),
            ]
            .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 }),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::BG_SIDEBAR)),
            border: Border { color: OryxisColors::BORDER, width: 1.0, radius: Radius::from(0.0) },
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
                text(title).size(16).color(OryxisColors::TEXT_PRIMARY),
                Space::new().width(Length::Fill),
                button(iced_fonts::bootstrap::arrow_bar_right().size(14).color(OryxisColors::TEXT_MUTED))
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
                    background: Some(Background::Color(OryxisColors::ACCENT)),
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
        let port_text = format!("SSH on port");
        let ssh_section = panel_section(column![
            // SSH on [port] port
            row![
                text(port_text).size(13).color(OryxisColors::TEXT_SECONDARY),
                Space::new().width(8),
                text_input("22", &self.editor_form.port)
                    .on_input(Message::EditorPortChanged)
                    .padding(6)
                    .width(60),
            ].align_y(iced::Alignment::Center),
            Space::new().height(12),
            text("Credentials").size(12).color(OryxisColors::TEXT_MUTED),
            Space::new().height(8),
            // Username
            row![
                iced_fonts::bootstrap::person().size(13).color(OryxisColors::TEXT_MUTED),
                Space::new().width(10),
                text_input("Username", &self.editor_form.username)
                    .on_input(Message::EditorUsernameChanged)
                    .padding(10),
            ].align_y(iced::Alignment::Center),
            Space::new().height(8),
            // Password
            row![
                iced_fonts::bootstrap::keyboard().size(13).color(OryxisColors::TEXT_MUTED),
                Space::new().width(10),
                text_input("Password", &self.editor_form.password)
                    .on_input(Message::EditorPasswordChanged)
                    .secure(true)
                    .padding(10),
            ].align_y(iced::Alignment::Center),
            Space::new().height(8),
            // Key / Auth selector
            row![
                text("+ Key").size(12).color(OryxisColors::ACCENT),
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
                vec!["Password".into(), "Key".into(), "Agent".into(), "Interactive".into()],
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
            container(Element::from(text(err.clone()).size(11).color(OryxisColors::ERROR)))
                .padding(Padding { top: 4.0, right: 16.0, bottom: 4.0, left: 16.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Connect / Save button (fixed bottom) ──
        let btn_label = if is_editing { "Save" } else { "Connect" };
        let btn_bg = if has_address { OryxisColors::ACCENT } else { OryxisColors::BG_SURFACE };
        let connect_btn = button(
            container(text(btn_label).size(14).color(OryxisColors::TEXT_PRIMARY))
                .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::EditorSave)
        .width(Length::Fill)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(btn_bg)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let mut bottom = column![connect_btn];
        if let Some(edit_id) = self.editor_form.editing_id {
            if let Some(idx) = self.connections.iter().position(|c| c.id == edit_id) {
                let del_btn = button(
                    container(text("Delete Host").size(12).color(OryxisColors::ERROR))
                        .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                )
                .on_press(Message::DeleteConnection(idx))
                .width(Length::Fill)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });
                bottom = bottom.push(Space::new().height(4));
                bottom = bottom.push(del_btn);
            }
        }

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
            .width(420)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.10, 0.11, 0.15))),
                border: Border { color: OryxisColors::BORDER, width: 1.0, radius: Radius::from(0.0) },
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
        Color::from_rgba(0.30, 0.56, 1.0, 0.15)
    } else {
        Color::TRANSPARENT
    };
    let fg = if is_active {
        OryxisColors::ACCENT
    } else {
        OryxisColors::TEXT_SECONDARY
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
                BtnStatus::Pressed => Color::from_rgba(0.30, 0.56, 1.0, 0.25),
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
            background: Some(Background::Color(Color::from_rgb(0.12, 0.13, 0.17))),
            border: Border { radius: Radius::from(8.0), color: Color::from_rgb(0.18, 0.19, 0.25), width: 1.0 },
            ..Default::default()
        })
        .into()
}

/// A labeled form field inside a section.
fn panel_field<'a>(label: &'a str, input: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(label).size(12).color(OryxisColors::TEXT_MUTED),
        Space::new().height(4),
        input,
    ]
    .into()
}

/// A divider line inside a section.
fn panel_divider<'a>() -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.18, 0.19, 0.25))),
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
            icon_widget.size(13).color(OryxisColors::TEXT_MUTED),
            Space::new().width(10),
            text(label).size(13).color(OryxisColors::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value).size(12).color(OryxisColors::TEXT_MUTED),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
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
            icon_widget.size(13).color(OryxisColors::TEXT_MUTED),
            Space::new().width(10),
            text(label).size(13).color(OryxisColors::TEXT_SECONDARY),
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
            icon_widget.size(13).color(OryxisColors::TEXT_MUTED),
            Space::new().width(10),
            text(label).size(13).color(OryxisColors::TEXT_SECONDARY),
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
            text(label).size(13).color(OryxisColors::TEXT_SECONDARY),
            Space::new().width(Length::Fill),
            text(value).size(13).color(OryxisColors::TEXT_PRIMARY),
        ],
    )
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .width(300)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::BG_SURFACE)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    })
    .into()
}

fn styled_button(label: &str, msg: Message, color: Color) -> Element<'_, Message> {
    button(
        container(text(label).size(14).color(OryxisColors::TEXT_PRIMARY))
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
