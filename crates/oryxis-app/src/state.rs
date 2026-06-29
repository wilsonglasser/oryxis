//! Pure state types used by the Oryxis application.
//!
//! Everything here is standalone data, no references to the top-level `Oryxis`
//! struct. Split out of `app.rs` to keep that file focused on the state machine.
//!
//! Types are grouped into sibling modules by concern (`sftp`, `tabs`, `forms`,
//! `overlay`, `modes`, `theme_editor`) and re-exported here so the rest of the
//! crate keeps using `crate::state::*` unchanged. A few small cross-cutting
//! leaves (local shell, chat, error dialog, connection progress, SSH stream)
//! stay in this root module.

pub(crate) use std::sync::{Arc, Mutex};

pub(crate) use iced::widget::pane_grid;
pub(crate) use oryxis_core::models::connection::AuthMethod;
pub(crate) use oryxis_ssh::{SftpClient, SftpEntry, SshSession};
pub(crate) use oryxis_terminal::widget::TerminalState;
pub(crate) use uuid::Uuid;

mod ai;
mod forms;
mod mcp;
mod modal;
mod modes;
mod overlay;
mod sftp;
mod tabs;
mod theme_editor;
mod vault;

pub(crate) use ai::*;
pub(crate) use forms::*;
pub(crate) use mcp::*;
pub(crate) use modal::*;
pub(crate) use modes::*;
pub(crate) use overlay::*;
pub(crate) use sftp::*;
pub(crate) use tabs::*;
pub(crate) use theme_editor::*;
pub(crate) use vault::*;

// ---------------------------------------------------------------------------
// Local shell picker
// ---------------------------------------------------------------------------

/// One row in the Local Shell picker (Windows: cmd / PowerShell / a
/// WSL distro). The launch payload: also serialized inside
/// `PaneOrigin::Local(..)` to restore a saved session group, so its
/// shape is frozen. The persisted, user-curated config lives in the
/// separate [`LocalTerminalEntry`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocalShellSpec {
    /// User-facing label, e.g. "PowerShell", "cmd", "Ubuntu (WSL)".
    pub label: String,
    /// Executable to spawn. Bare program name (resolved via `PATH`)
    /// or a full path; passed to portable-pty's `CommandBuilder`.
    pub program: String,
    /// Arguments tacked on after the program. For WSL distros this
    /// is `["-d", "<distro-name>"]`; for plain shells it's empty.
    pub args: Vec<String>,
}

/// One persisted entry in the curated local-terminal list. Machine-local
/// config (paths and WSL distros differ per host), so this is stored as a
/// JSON string in the `settings` table and deliberately kept *out* of
/// sync and portable export.
///
/// The auto-scan runs once (first time the user opens the local terminal),
/// populates this list and persists it; subsequent opens read from here
/// instead of re-scanning. Users can add/remove entries and re-scan from
/// Settings → Terminal.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct LocalTerminalEntry {
    /// Stable identity, used by the "always open X" default and by the
    /// edit / remove actions. `nil` only in legacy payloads written
    /// before ids existed; `boot` reassigns those on load.
    #[serde(default)]
    pub id: Uuid,
    /// User-facing label, e.g. "PowerShell", "Ubuntu (WSL)".
    pub label: String,
    /// Executable to spawn (bare name or full path).
    pub program: String,
    /// Arguments appended after the program.
    #[serde(default)]
    pub args: Vec<String>,
    /// `true` when the user added this entry by hand; `false` for
    /// auto-detected entries. Drives the "manual" badge in the UI and
    /// is preserved across a re-scan.
    #[serde(default)]
    pub manual: bool,
    /// Optional `#RRGGBB` accent override (icon picker). `None` falls back
    /// to the OS-hint color at render time.
    #[serde(default)]
    pub color: Option<String>,
    /// Optional icon id (icon picker). `None` falls back to the OS hint
    /// derived from the label, then a generic terminal glyph.
    #[serde(default)]
    pub icon: Option<String>,
}

impl LocalTerminalEntry {
    /// Command identity (`program` + args), used to dedup on re-scan so a
    /// detected shell already in the list isn't appended twice. Distinct
    /// from `id`, which is the user-facing stable handle for edit / remove
    /// / default and survives a program/args edit.
    pub fn cmd_key(&self) -> String {
        let mut k = self.program.clone();
        for a in &self.args {
            k.push('\u{1f}');
            k.push_str(a);
        }
        k
    }

    /// Convert to the launch payload consumed by the picker / spawn path.
    pub fn to_spec(&self) -> LocalShellSpec {
        LocalShellSpec {
            label: self.label.clone(),
            program: self.program.clone(),
            args: self.args.clone(),
        }
    }
}


// ---------------------------------------------------------------------------
// Chat (AI sidebar per terminal tab)
// ---------------------------------------------------------------------------

/// Role of a chat message in the AI sidebar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChatRole {
    User,
    Assistant,
    System, // for tool execution results
    /// Provider/network error, rendered as a red banner with a Retry
    /// button instead of looking like a normal assistant response.
    Error,
    /// AI requested a `risky` tool call. `content` carries the proposed
    /// command verbatim. The view renders RUN / ALWAYS RUN / DENY
    /// buttons; clicking RUN or ALWAYS RUN converts this into the
    /// regular tool-execution flow. Safe commands skip this state and
    /// run immediately.
    PendingTool,
}

/// A single message in the AI chat sidebar.
#[derive(Debug, Clone)]
pub(crate) struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    /// Parsed Markdown items for assistant messages, cached so the view
    /// can borrow them across renders. Iced's `markdown::view` returns an
    /// Element borrowing the items slice, so we can't parse on the fly.
    pub parsed_md: Vec<iced::widget::markdown::Item>,
}


// ---------------------------------------------------------------------------
// Generic blocking error dialog
// ---------------------------------------------------------------------------

/// Modal-style "you must read this" error. Heavier than `toast` because
/// it doesn't auto-dismiss; lighter than a full confirm modal because
/// it has a single OK action plus an optional "Open URL" button.
#[derive(Debug, Clone)]
pub(crate) struct ErrorDialog {
    pub title: String,
    pub body: String,
    /// Optional learn-more / install-instructions link. Rendered as a
    /// secondary button. `None` = no link button.
    pub link: Option<ErrorDialogLink>,
    /// Optional recovery action rendered as a primary button; pressing
    /// it dismisses the dialog and dispatches the carried message
    /// (`Message::ErrorDialogRunAction`). `None` = Close only.
    pub action: Option<ErrorDialogAction>,
}

#[derive(Debug, Clone)]
pub(crate) struct ErrorDialogLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ErrorDialogAction {
    pub label: String,
    pub message: Box<crate::app::Message>,
    /// Destructive actions (delete, uninstall) render in the error
    /// red; recovery actions keep the accent.
    pub danger: bool,
}

/// Armed when the user asked to reconnect an ECS Exec session whose
/// task is gone while the dynamic group is still resolving. Once
/// `DynamicGroupResolved` lands for `group_id`, the handler picks the
/// running task (preferring `fallback_task_id` when it survived) and
/// connects.
#[derive(Debug, Clone)]
pub(crate) struct PendingEcsAutoConnect {
    pub group_id: Uuid,
    pub container: String,
    pub fallback_task_id: String,
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

/// Widget id of the first keyboard-interactive prompt field, so the
/// prompt handler can land focus there on appearance (type-and-Enter for
/// OTP entry without a click).
pub(crate) const KBI_FIRST_INPUT_ID: &str = "kbi-first-input";

/// Internal message type for SSH connection streams.
pub(crate) enum SshStreamMsg {
    Progress(ConnectionStep, String), // (step, log message)
    Connected(Arc<SshSession>),
    HostKeyVerify(oryxis_ssh::HostKeyQuery),
    KbiPrompt(oryxis_ssh::KbiQuery),
    Data(Vec<u8>),
    Error(String),
    /// Handshake failed because the server and client share no algorithm
    /// in some category. Carries the failed category + what the server
    /// offered, so the UI can offer the legacy-algorithm fallback.
    NoCommonAlgo {
        category: oryxis_ssh::NegCategory,
        server_offers: Vec<String>,
    },
    Disconnected,
}

/// A pending "this server only speaks legacy algorithms" prompt: which
/// host failed, in which category, and what it offered. Drives the
/// legacy-fallback modal.
#[derive(Debug, Clone)]
pub(crate) struct PendingLegacyAlgo {
    pub conn_id: uuid::Uuid,
    pub category: oryxis_ssh::NegCategory,
    pub server_offers: Vec<String>,
    /// The action to re-dispatch after expanding the host's overrides, so
    /// the dialog works the same for terminal / SFTP / port-forward /
    /// backup connects (each passes its own entry message).
    pub retry: Box<crate::app::Message>,
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
