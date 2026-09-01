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

mod agent;
mod ai;
mod forms;
mod mcp;
mod modal;
mod modes;
mod monitor_dash;
mod net_tools;
mod overlay;
mod palette;
mod player;
mod privacy;
mod theme_ui;
mod vault_import;
mod chat_ui;
mod hover;
mod panels;
mod cloud_discover;
mod snippet_form;
mod keys_ui;
mod sftp_chrome;
mod prefs;
mod sftp;
mod sync;
mod tabs;
mod theme_editor;
mod vault;

pub(crate) use agent::{AgentConfirmCard, AgentSnippetKind, AgentState};
pub(crate) use ai::*;
pub(crate) use forms::*;
pub(crate) use mcp::*;
pub(crate) use modal::*;
pub(crate) use modes::*;
pub(crate) use monitor_dash::*;
pub(crate) use net_tools::NetToolsState;
pub(crate) use overlay::*;
pub(crate) use palette::*;
pub(crate) use player::*;
pub(crate) use privacy::*;
pub(crate) use theme_ui::ThemeEditorUi;
pub(crate) use vault_import::VaultImportState;
pub(crate) use chat_ui::ChatUi;
pub(crate) use hover::HoverState;
pub(crate) use panels::PanelsOpen;
pub(crate) use cloud_discover::CloudDiscoverUi;
pub(crate) use snippet_form::SnippetForm;
pub(crate) use keys_ui::KeysUi;
pub(crate) use sftp_chrome::SftpChrome;
pub(crate) use prefs::{resolve_download_dir, AppPrefs};
pub(crate) use sftp::*;
pub(crate) use sync::*;
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

/// A local host's startup command, waiting for its pane's output to go
/// quiet (see `Oryxis::pending_local_startup`).
#[derive(Debug, Clone)]
pub(crate) struct PendingLocalStartup {
    /// The line to send, newline included at write time.
    pub command: String,
    /// How many output batches this pane has produced. A timer armed at
    /// batch N fires only if the count is still N, so a shell that is
    /// still printing pushes the send further out instead of landing in
    /// the middle of its banner.
    pub batches: u64,
}

/// A snippet run/paste parked while its `{name}` placeholders are
/// filled in (the snippet-variables modal).
#[derive(Debug, Clone)]
pub(crate) struct PendingSnippetVars {
    /// The tab this snippet was fired at, captured when the modal opens.
    ///
    /// Resolving the target on CONFIRM instead would read whatever tab is
    /// active by then, and the modal blocks the user's input, not the
    /// app's messages: a cloud plugin answering, a background connect
    /// landing or a session group opening all activate their new tab
    /// while the user is still filling in the variables. The snippet
    /// would then run on a host nobody chose, which for a snippet is a
    /// command actually executed.
    pub target_tab: uuid::Uuid,
    /// Raw snippet body, substituted on confirm.
    pub command: String,
    /// `true` = run (+ Enter); `false` = paste only.
    pub run: bool,
    /// `Some(snippet id)` when the snippet is an install script (issue
    /// #147): the vars confirm parks the careful-paste confirmation
    /// (full body on screen) instead of sending, and a confirmed run
    /// records into the host's install memory.
    pub install: Option<uuid::Uuid>,
    /// (name, current value) per distinct placeholder, defaults
    /// pre-filled; edited in place by the modal inputs.
    pub vars: Vec<(String, String)>,
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
    /// Free-form tags, same semantics as host tags: they feed the
    /// snippet sidebar's filter-by-tags toggle so a local pane can
    /// surface its own runbook. Machine-local like the rest of the
    /// entry (never synced or exported).
    #[serde(default)]
    pub tags: Vec<String>,
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
    System, // informational notes (declines, "not connected", ...)
    /// Provider/network error, rendered as a red banner with a Retry
    /// button instead of looking like a normal assistant response.
    Error,
    /// AI requested a `risky` tool call. `content` carries the proposed
    /// command verbatim. The view renders RUN / ALWAYS RUN / DENY
    /// buttons; clicking RUN or ALWAYS RUN converts this into the
    /// regular tool-execution flow. Safe commands skip this state and
    /// run immediately.
    PendingTool,
    /// A tool execution: the command that ran and (once captured) its
    /// output. The structured data lives on `ChatMessage.tool`; the
    /// message builder turns a completed exchange into native
    /// `tool_use` + `tool_result` blocks for the provider. While the
    /// output is still `None` (command running) it is sent as flat text,
    /// so an in-flight exchange can never leave a dangling `tool_use`.
    Tool,
}

/// A completed-or-running tool execution recorded in the chat history.
/// `id` is minted locally and pairs the `tool_use` block with its
/// `tool_result` block when the message builder reconstructs the
/// provider-native request.
#[derive(Debug, Clone)]
pub(crate) struct ToolExchange {
    pub id: String,
    pub command: String,
    /// Model self-classification, "safe" | "risky".
    pub risk: String,
    /// Captured terminal output. `None` while the command is still
    /// running (rendered + sent as flat text until it resolves).
    pub output: Option<String>,
    /// Gemini's opaque per-call `thoughtSignature`, kept so the replay can
    /// echo it back; Gemini 2.5+ rejects a function call that returns
    /// without one. `None` on every other provider. See
    /// `crate::ai::ToolUseMsg::thought_signature`.
    pub thought_signature: Option<String>,
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
    /// Structured tool data, `Some` only for [`ChatRole::Tool`] messages.
    pub tool: Option<ToolExchange>,
    /// Chain-of-thought streamed alongside an assistant reply, when the
    /// provider emits one (DeepSeek thinking mode). Never rendered: it is
    /// kept because DeepSeek requires it back in the history on the next
    /// request and answers 400 without it (issue #105). Empty for every
    /// other provider and for non-assistant roles.
    pub reasoning: String,
}

impl ChatMessage {
    /// A plain text message (any role except `Tool`); `parsed_md` starts
    /// empty and is filled by the caller for assistant bubbles.
    pub(crate) fn text(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            parsed_md: Vec::new(),
            tool: None,
            reasoning: String::new(),
        }
    }
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
// Quick connect (ad-hoc hosts, never persisted)
// ---------------------------------------------------------------------------

/// One ad-hoc quick-connect host living in `Oryxis.quick_connects`.
///
/// `conn` is a full `Connection` that exists only in memory; the credential
/// fields carry what the user typed in the editor's connect-without-saving
/// flow, since there is no vault row to hydrate from (picker/search-born
/// entries keep them `None`). They live beside `conn` rather than inside it
/// so the `Connection` value that rides `Message` / relaunch never holds a
/// plaintext secret; the connect path applies them just before dialing.
/// Cleared on vault lock together with the other secret-bearing UI state.
#[derive(Clone)]
pub(crate) struct QuickConnectEntry {
    pub conn: oryxis_core::models::Connection,
    pub password: Option<String>,
    pub totp_secret: Option<String>,
    /// Password for an inline proxy typed in the editor flow (a saved
    /// proxy identity hydrates from the vault instead).
    pub proxy_password: Option<String>,
}

impl QuickConnectEntry {
    /// Entry with no typed credentials (parser-born surfaces).
    pub fn bare(conn: oryxis_core::models::Connection) -> Self {
        Self {
            conn,
            password: None,
            totp_secret: None,
            proxy_password: None,
        }
    }
}

/// What the user picked in the quick-connect "authenticate with a saved
/// identity / key instead" selector (keyboard-interactive prompt modal and
/// the failed-connect screen). Applied by mutating the ephemeral entry's
/// `Connection`, so the retry and every later reconnect carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickAuthChoice {
    /// A saved identity (username + password and/or key).
    Identity(Uuid),
    /// A saved SSH key on its own.
    Key(Uuid),
}

/// One row of the quick-auth selector: the resolved choice plus the label
/// the pick_list renders for it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QuickAuthOption {
    pub choice: QuickAuthChoice,
    pub label: String,
}

/// Manual impl so a Debug-formatted `|v| Message::Ssh(SshMessage::QuickConnect(v))` (message
/// tracing, debug log file) never prints the typed credentials.
impl std::fmt::Debug for QuickConnectEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickConnectEntry")
            .field("conn", &self.conn)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("totp_secret", &self.totp_secret.as_ref().map(|_| "***"))
            .field("proxy_password", &self.proxy_password.as_ref().map(|_| "***"))
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Connection progress (during establishment)
// ---------------------------------------------------------------------------

/// Where the connection being established came from, so the progress
/// screen's Retry / Edit actions resolve the right store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressOrigin {
    /// A saved host, BY ID. The connect can outlive the list order it
    /// started under (an auto-saved rename re-sorts, a sync apply
    /// rebuilds it), so a stored index would resolve to a different
    /// host by the time the progress screen's Retry / Edit fire.
    Saved(Uuid),
    /// Key into `Oryxis.quick_connects` (an ad-hoc host).
    Quick(Uuid),
}

/// Connection progress state for the connecting tab.
#[derive(Clone)]
pub(crate) struct ConnectionProgress {
    pub label: String,
    pub hostname: String,
    pub step: ConnectionStep,
    pub logs: Vec<(ConnectionStep, String)>,
    pub failed: bool,
    pub origin: ProgressOrigin,
    pub tab_idx: usize,
    /// Stable id of the pane this connect is dialing. The completion
    /// (`SshConnected(pane_id, _)`, shared by SSH / Telnet / Serial) is
    /// matched against this so a split-pane or background connect, or a
    /// stale completion from a dial the user cancelled via "Edit host",
    /// can never clear an unrelated Home connect's card.
    pub pane_id: uuid::Uuid,
    /// Pre-auth banner(s) the server sent (RFC 4252 §5.4: legal
    /// notices, MFA instructions), shown on the progress card so the
    /// user reads them while answering the auth prompts. Multiple
    /// banners concatenate. Also written to the tab's terminal, where
    /// it lands in scrollback.
    pub banner: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStep {
    Starting,       // step 0: connect initiated (the seed log line)
    Connecting,     // step 1: TCP/proxy/jump dial + address resolution
    Handshake,      // step 2: SSH handshake + host key
    Authenticating, // step 3: auth in progress
    Authenticated,  // step 3 done: auth accepted
    OpeningSession, // step 4: PTY / port forwards / session setup
}


// ---------------------------------------------------------------------------
// SSH stream (messages from the background SSH task)
// ---------------------------------------------------------------------------

/// Widget id of the first keyboard-interactive prompt field, so the
/// prompt handler can land focus there on appearance (type-and-Enter for
/// OTP entry without a click).
pub(crate) const KBI_FIRST_INPUT_ID: &str = "kbi-first-input";

/// Widget id of the new-tab picker's search input, so every path that
/// opens the picker (its hotkey, the `+` button, pane splits) can land
/// focus there for immediate type-to-filter.
pub(crate) const NEW_TAB_PICKER_SEARCH_ID: &str = "new-tab-picker-search";

/// Widget id of the tab-jump modal's search input, same contract as
/// the new-tab picker's: every open path (the hotkey, the tab bar's
/// `⋯` button) lands focus here so the modal is type-to-filter from
/// the first keystroke.
pub(crate) const TAB_JUMP_SEARCH_ID: &str = "tab-jump-search";

/// Internal message type for SSH connection streams.
pub(crate) enum SshStreamMsg {
    Progress(ConnectionStep, String), // (step, log message)
    /// Pre-auth banner from the server (RFC 4252 §5.4).
    Banner(String),
    Connected(Arc<SshSession>),
    HostKeyVerify(oryxis_ssh::HostKeyQuery),
    ProxyCommandVerify(oryxis_ssh::ProxyCommandQuery),
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

/// Who is waiting behind a dial that hit a command proxy, which is what
/// decides whether the approval may be ASKED for or only looked up.
///
/// Every dial consults the same per-device approval list, so a line the
/// user already accepted never prompts twice regardless of mode. The
/// difference is what an UNAPPROVED line does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyConsentMode {
    /// The user asked for this connect and is watching it: raise the
    /// prompt.
    Ask,
    /// Nobody triggered this dial (boot auto-start forwards, snapshot
    /// sync, the monitor dashboard). Approved lines still run; anything
    /// else is refused without a modal. A dialog nobody expects is
    /// answered reflexively, which is not consent, and the whole point
    /// of the gate is that these paths are exactly where a route pushed
    /// by a sync peer would fire unattended.
    TrustedOnly,
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

/// Backing state for the History view's content search (the "search in
/// session content" toggle inside the search field): matches computed
/// asynchronously against the recorded command chunks (`kind = 'c'`),
/// the per-host command history and the decrypted output streams. The
/// maps always answer `needle`; the view ignores them whenever the
/// live query has moved past it. `generation` cancels in-flight
/// debounce timers and scan steps (a stale tick or scan result simply
/// doesn't match and is dropped).
#[derive(Default)]
pub(crate) struct HistoryContentSearch {
    pub generation: u64,
    /// Lowercased query the maps below answer.
    pub needle: String,
    /// Session log id -> matched snippet: the typed command when the
    /// hit came from a command record, or an output excerpt around
    /// the match once the output scan reaches the session.
    pub log_matches: std::collections::HashMap<Uuid, String>,
    /// Host id -> matched command from the per-host command history,
    /// covering hosts with no recorded sessions (they have no
    /// timeline row to light up, so the view surfaces them in a
    /// strip above the list).
    pub conn_matches: Vec<(Uuid, String)>,
    /// Session logs still waiting for their output scan, drained one
    /// step at a time so results land incrementally.
    pub queue: Vec<Uuid>,
    /// Matched sessions the search pulled into `Oryxis::session_logs`
    /// from beyond the loaded page window (the search covers the
    /// whole table; the timeline needs a row to light up). Dropped
    /// from the list again when the results retire.
    pub extra_logs: Vec<Uuid>,
    /// Output-scan progress: sessions scanned so far / total queued.
    pub scan_done: usize,
    pub scan_total: usize,
    /// True while an output-scan step is in flight.
    pub scanning: bool,
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
