//! Pure state types used by the Oryxis application.
//!
//! Everything here is standalone data — no references to the top-level `Oryxis`
//! struct. Split out of `app.rs` to keep that file focused on the state machine.

use std::sync::{Arc, Mutex};

use iced::widget::text_editor;
use oryxis_core::models::connection::AuthMethod;
use oryxis_ssh::SshSession;
use oryxis_terminal::widget::TerminalState;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Chat (AI sidebar per terminal tab)
// ---------------------------------------------------------------------------

/// Role of a chat message in the AI sidebar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChatRole {
    User,
    Assistant,
    System, // for tool execution results
}

/// A single message in the AI chat sidebar.
#[derive(Debug, Clone)]
pub(crate) struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    #[allow(dead_code)]
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Terminal tab
// ---------------------------------------------------------------------------

/// A terminal tab — either a local shell or an SSH session.
pub(crate) struct TerminalTab {
    pub _id: Uuid,
    pub label: String,
    pub terminal: Arc<Mutex<TerminalState>>,
    /// SSH session handle (None for local shell).
    pub ssh_session: Option<Arc<SshSession>>,
    /// Session log ID for terminal recording.
    pub session_log_id: Option<Uuid>,
    /// AI chat history for this terminal session.
    pub chat_history: Vec<ChatMessage>,
    /// Whether the AI chat sidebar is visible.
    pub chat_visible: bool,
}

// ---------------------------------------------------------------------------
// Connection editor form
// ---------------------------------------------------------------------------

/// Connection editor form state.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionForm {
    pub label: String,
    pub hostname: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub auth_method: AuthMethod,
    pub group_name: String,
    pub selected_key: Option<String>,
    pub jump_host: Option<String>,  // label of jump host connection
    /// Selected identity label (if any).
    pub selected_identity: Option<String>,
    /// If editing, the connection ID.
    pub editing_id: Option<Uuid>,
    /// Whether the connection already has a password stored in the vault.
    pub has_existing_password: bool,
    /// Whether the user has modified the password field.
    pub password_touched: bool,
    /// Whether to show the password in plain text.
    pub password_visible: bool,
    /// Whether the username field is focused (shows identity autocomplete).
    pub username_focused: bool,
    /// Port forwarding rules (local -L style).
    pub port_forwards: Vec<PortForwardForm>,
    /// Whether this host is exposed via MCP.
    pub mcp_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PortForwardForm {
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
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
            username_focused: false,
            port_forwards: Vec::new(),
            mcp_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Overlay (floating context menus)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum OverlayContent {
    HostActions(usize),
    KeyActions(usize),
    IdentityActions(usize),
    KeychainAdd,
    TabActions(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct OverlayState {
    pub content: OverlayContent,
    pub x: f32,
    pub y: f32,
}

// ---------------------------------------------------------------------------
// Top-level UI modes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultState {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Terminal,
    AI,
    Theme,
    Shortcuts,
    Security,
    Sync,
    About,
}

// ---------------------------------------------------------------------------
// Connection progress (during establishment)
// ---------------------------------------------------------------------------

/// Connection progress state for the connecting tab.
#[derive(Clone)]
pub(crate) struct ConnectionProgress {
    pub label: String,
    pub hostname: String,
    pub step: ConnectionStep,
    pub logs: Vec<(ConnectionStep, String)>,
    pub failed: bool,
    pub connection_idx: usize,
    pub tab_idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStep {
    Connecting,   // step 1: TCP/proxy/jump
    Handshake,    // step 2: SSH handshake + host key
    Authenticating, // step 3: auth
}

// ---------------------------------------------------------------------------
// SSH stream (messages from the background SSH task)
// ---------------------------------------------------------------------------

/// Internal message type for SSH connection streams.
pub(crate) enum SshStreamMsg {
    Progress(ConnectionStep, String), // (step, log message)
    Connected(Arc<SshSession>),
    #[allow(dead_code)]
    NewKnownHosts(Vec<oryxis_core::models::known_host::KnownHost>),
    HostKeyVerify(oryxis_ssh::HostKeyQuery),
    Data(Vec<u8>),
    Error(String),
    Disconnected,
}

// ---------------------------------------------------------------------------
// Keep the text_editor import referenced — required for Message enum split later.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub(crate) type _EditorContent = text_editor::Content;
