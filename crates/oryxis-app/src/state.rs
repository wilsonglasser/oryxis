//! Pure state types used by the Oryxis application.
//!
//! Everything here is standalone data — no references to the top-level `Oryxis`
//! struct. Split out of `app.rs` to keep that file focused on the state machine.

use std::sync::{Arc, Mutex};

use iced::widget::text_editor;
use oryxis_core::models::connection::AuthMethod;
use oryxis_ssh::{SftpClient, SftpEntry, SshSession};
use oryxis_terminal::widget::TerminalState;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// SFTP view state
// ---------------------------------------------------------------------------

/// State for the SFTP browser. One session at a time for v0.3 — the user
/// picks a host, we connect (reusing the SSH connect flow), open the SFTP
/// subsystem, and browse the remote tree side-by-side with a local one.
#[derive(Default)]
pub(crate) struct SftpState {
    /// Currently mounted SSH session, if any. Cloned from the source host
    /// when the user picks one from the connection list.
    pub session: Option<Arc<SshSession>>,
    /// Active SFTP client (one channel per session).
    pub client: Option<SftpClient>,
    /// Label of the currently mounted host — shown in the breadcrumb.
    pub host_label: Option<String>,
    pub remote_path: String,
    pub remote_entries: Vec<SftpEntry>,
    pub remote_loading: bool,
    pub remote_error: Option<String>,
    pub remote_filter: String,
    pub local_path: std::path::PathBuf,
    pub local_entries: Vec<LocalEntry>,
    pub local_error: Option<String>,
    pub local_filter: String,
    /// When false (default), entries whose name starts with `.` are
    /// hidden — matches `ls` / Finder / Explorer convention. Toggleable
    /// from each pane's Actions menu independently so the user can show
    /// hidden remote files without exposing all the local dotfiles.
    pub local_show_hidden: bool,
    pub remote_show_hidden: bool,
    /// Actions popover anchored to one of the pane headers.
    pub local_actions_open: bool,
    pub remote_actions_open: bool,
    /// Whether the Windows-style drive picker dropdown is open. Only
    /// rendered on Windows hosts.
    pub local_drives_open: bool,
    /// When `Some`, the breadcrumb is replaced by a text input the user
    /// can type a full path into. The string is the in-progress edit.
    pub local_path_editing: Option<String>,
    pub remote_path_editing: Option<String>,
    /// Sort column + direction per pane.
    pub local_sort: SftpSort,
    pub remote_sort: SftpSort,
    /// True while the host picker overlay is visible (default at boot,
    /// hidden once a host is chosen).
    pub picker_open: bool,
    /// Search filter applied to the host picker.
    pub picker_search: String,
    /// Right-click row context menu — anchored to the click location
    /// and operating on a specific entry.
    pub row_menu: Option<SftpRowMenu>,
    /// Inline rename editor — replaces the row visually with a text
    /// input until the user commits or cancels.
    pub rename: Option<SftpRename>,
    /// Pending destructive action — surfaces a confirmation modal.
    /// `Vec` (instead of `Option`) so the same modal handles both single
    /// right-click delete and bulk delete from a multi-selection — the
    /// modal copy adapts to the count.
    pub delete_confirm: Vec<SftpDeleteTarget>,
    /// New file / new folder modal — kind + in-progress name input.
    pub new_entry: Option<SftpNewEntry>,
    /// True while OS files are being dragged over the window. Drives the
    /// remote-pane drop highlight; cleared on `FilesHoveredLeft` or
    /// `FileDropped`.
    pub drop_active: bool,
    /// Currently hovered row across both panes. Updated continuously
    /// from MouseArea on_enter / on_exit on every visible row, and
    /// consumed by both the OS drop target picker and the internal
    /// drag-drop release handler.
    pub hovered_row: Option<(SftpPaneSide, String, bool)>,
    /// In-progress internal drag (file/folder being dragged from one
    /// pane to the other). Spans the press → drop window.
    pub drag: Option<SftpInternalDrag>,
    /// Folder transfer in progress (upload / download / local duplicate).
    /// Drives the bottom-of-view progress bar and serializes the queue
    /// of per-item operations so the SFTP connection isn't slammed.
    pub transfer: Option<TransferState>,
    /// One-shot destination override for the next upload — set by the
    /// drag-and-drop handler when the cursor lands on a specific remote
    /// folder, consumed by `SftpUpload` / `SftpUploadFolder`.
    pub upload_dest_override: Option<String>,
    /// Same idea for downloads — set when an internal drag from the
    /// remote pane lands on a specific local folder. Consumed by
    /// `SftpDownload` / `SftpDownloadFolder`.
    pub download_dest_override: Option<std::path::PathBuf>,
    /// Multi-row selection across both panes. Plain click on a file
    /// replaces this with a single entry; ctrl-click toggles; shift-click
    /// extends from `selection_anchor` within the same pane. Cleared
    /// whenever either pane navigates away.
    pub selected_rows: Vec<(SftpPaneSide, String)>,
    /// Last clicked row — origin point for shift-click range extension.
    /// Stays put across ctrl-click toggles so the range pivots from the
    /// initial selection point rather than the most recent toggle.
    pub selection_anchor: Option<(SftpPaneSide, String)>,
    /// Active edit-in-place session — a remote file downloaded to an OS
    /// temp path and opened in the user's default editor. Persists until
    /// the user clicks Save Back or Discard.
    pub edit_session: Option<EditSession>,
    /// Pending overwrite confirmation — set when the user uploads a file
    /// whose name already exists in the destination. Cleared when the
    /// user picks an action.
    pub overwrite_prompt: Option<OverwritePrompt>,
    /// Open Properties modal for a single row. Carries the snapshot
    /// of the current metadata + the user's in-progress edits to the
    /// permission bits so Apply can diff.
    pub properties: Option<PropertiesView>,
}

/// Per-bit permission state shown by the Properties dialog. Maps 1-1
/// onto the POSIX rwxrwxrwx octal so Apply can rebuild a `u32` mode.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PermBits {
    pub user_r: bool,
    pub user_w: bool,
    pub user_x: bool,
    pub group_r: bool,
    pub group_w: bool,
    pub group_x: bool,
    pub other_r: bool,
    pub other_w: bool,
    pub other_x: bool,
}

impl PermBits {
    pub fn from_mode(mode: u32) -> Self {
        Self {
            user_r: mode & 0o400 != 0,
            user_w: mode & 0o200 != 0,
            user_x: mode & 0o100 != 0,
            group_r: mode & 0o040 != 0,
            group_w: mode & 0o020 != 0,
            group_x: mode & 0o010 != 0,
            other_r: mode & 0o004 != 0,
            other_w: mode & 0o002 != 0,
            other_x: mode & 0o001 != 0,
        }
    }
    pub fn to_mode(self) -> u32 {
        let mut m = 0u32;
        if self.user_r { m |= 0o400; }
        if self.user_w { m |= 0o200; }
        if self.user_x { m |= 0o100; }
        if self.group_r { m |= 0o040; }
        if self.group_w { m |= 0o020; }
        if self.group_x { m |= 0o010; }
        if self.other_r { m |= 0o004; }
        if self.other_w { m |= 0o002; }
        if self.other_x { m |= 0o001; }
        m
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermBit {
    UserR, UserW, UserX,
    GroupR, GroupW, GroupX,
    OtherR, OtherW, OtherX,
}

#[derive(Debug, Clone)]
pub(crate) struct PropertiesView {
    pub side: SftpPaneSide,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<u32>,
    pub owner_uid: Option<u32>,
    pub owner_gid: Option<u32>,
    /// Original mode bits — used to detect unchanged Apply (no-op) and
    /// preserve the high bits (setuid/setgid/sticky) the dialog doesn't
    /// edit.
    pub original_mode: u32,
    pub bits: PermBits,
    /// True while the chmod task is in flight — disables the Apply
    /// button so the user can't double-fire.
    pub applying: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct OverwritePrompt {
    pub src: std::path::PathBuf,
    pub dst_dir: String,
    pub basename: String,
    pub src_size: u64,
    pub dst_size: u64,
    /// True when the prompt is part of a multi-file transfer — surfaces
    /// the "apply to remaining" checkbox so the user doesn't have to
    /// re-answer for every collision.
    pub multi: bool,
    /// User-toggled state of the "apply to remaining" checkbox while
    /// the modal is open. Read on resolve; persisted as
    /// `TransferState.overwrite_default` if true.
    pub apply_to_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverwriteAction {
    /// Always overwrite the existing file.
    Replace,
    /// Only overwrite if the existing remote size differs from the local
    /// size — cheap proxy for "is it actually a different file?" without
    /// hashing both sides.
    ReplaceIfDifferent,
    /// Upload alongside with a "name copy" suffix instead of overwriting.
    Duplicate,
    /// Don't upload at all.
    Cancel,
}

#[derive(Debug, Clone)]
pub(crate) struct EditSession {
    pub remote_path: String,
    pub temp_path: std::path::PathBuf,
    /// Display label shown in the modal — basename of the remote file.
    pub label: String,
    /// Mtime of the temp file when it was first written (right after
    /// download). The watcher tick polls this to detect saves coming
    /// from the user's editor.
    pub initial_mtime: Option<std::time::SystemTime>,
    /// True once the watcher tick observes an mtime newer than
    /// `initial_mtime` — drives the "Changes detected" copy in the
    /// modal so the user knows their save was picked up.
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferKind {
    Upload,
    Download,
    /// Local-side `cp -r` equivalent — `std::fs` doesn't expose recursive
    /// copy so we walk the tree and copy each entry ourselves.
    DuplicateLocal,
}

#[derive(Debug, Clone)]
pub(crate) struct TransferItem {
    /// Source path. For uploads/local-duplicate this is a local path;
    /// for downloads it's a remote POSIX path.
    pub src: String,
    /// Destination path. Mirrors the side rules of `src` swapped.
    pub dst: String,
    /// Folders are processed by ensuring the destination directory exists;
    /// files are read+written.
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TransferState {
    pub kind: TransferKind,
    /// Top-level label shown in the progress bar — e.g. "my-folder".
    pub root_label: String,
    /// Pending items, popped one at a time as each operation completes.
    pub queue: std::collections::VecDeque<TransferItem>,
    /// Name of the item currently being processed; `None` between items.
    pub current: Option<String>,
    pub completed: usize,
    pub total: usize,
    /// Sticky overwrite decision — set when the user checks "Apply to
    /// remaining" in the conflict modal. Subsequent collisions auto-
    /// resolve with this action without re-prompting.
    pub overwrite_default: Option<OverwriteAction>,
    /// When `Some`, the current item has been popped and is waiting for
    /// the user to resolve a conflict modal. The path/size info is
    /// captured here so the resolve handler can reapply the action to
    /// the right destination without re-listing.
    pub pending_conflict_item: Option<TransferItem>,
    /// Slot that hit the conflict — needed so resolve uses the same
    /// SFTP client channel for the apply step.
    pub pending_conflict_slot: Option<u8>,
    /// One SFTP client per parallel slot. Empty for `DuplicateLocal`
    /// (no SFTP needed). For `Upload`/`Download` size is `concurrency`.
    pub clients: Vec<SftpClient>,
    /// Per-slot "is in flight" flag. The Next handler picks the first
    /// `false` slot to dispatch to, keeping each slot mapped 1-1 with
    /// its `clients[i]` so workers never fight for the same channel.
    pub busy_slots: Vec<bool>,
    /// True while a conflict modal is up — workers exit on Next instead
    /// of popping more items, then get re-spawned by Resolve.
    pub paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SftpPaneSide {
    Local,
    Remote,
}

/// Internal drag state — a row being dragged from one pane towards the
/// other. The press position lets us suppress short jitters; only past
/// a small threshold do we treat the press+move as a drag rather than a
/// click. Multi-row drags carry the full set so a single drop fires N
/// transfers.
#[derive(Debug, Clone)]
pub(crate) struct SftpInternalDrag {
    pub origin_side: SftpPaneSide,
    /// `(path, is_dir)` per dragged item.
    pub items: Vec<(String, bool)>,
    /// Short label shown on the floating ghost — basename or "N items".
    pub label: String,
    /// Cursor position at left-press time. Used to gate `active` on
    /// distance threshold so accidental jitter doesn't get treated as
    /// a drag and steal click handling.
    pub press_pos: iced::Point,
    /// Once the cursor moves past a few pixels we commit to the drag —
    /// the ghost renders, the drop highlight kicks in, and the eventual
    /// release dispatches a transfer instead of a click.
    pub active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SftpRowMenu {
    pub side: SftpPaneSide,
    /// Stringified path — `String` for both panes since the modal /
    /// follow-up actions accept a path verbatim.
    pub path: String,
    pub is_dir: bool,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct SftpRename {
    pub side: SftpPaneSide,
    /// Original full path; we rebuild the parent + new name on commit.
    pub original_path: String,
    pub input: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SftpDeleteTarget {
    pub side: SftpPaneSide,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SftpEntryKind {
    File,
    Folder,
}

#[derive(Debug, Clone)]
pub(crate) struct SftpNewEntry {
    pub side: SftpPaneSide,
    pub kind: SftpEntryKind,
    pub input: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SftpSortColumn {
    Name,
    Modified,
    Size,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SftpSort {
    pub column: SftpSortColumn,
    pub ascending: bool,
}

impl Default for SftpSort {
    fn default() -> Self {
        Self {
            column: SftpSortColumn::Name,
            ascending: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Reserved for upcoming sort-by-modified column; populated now so
    /// the UI can opt in without a follow-up state migration.
    #[allow(dead_code)]
    pub modified: Option<std::time::SystemTime>,
}

// ---------------------------------------------------------------------------
// Local shell picker
// ---------------------------------------------------------------------------

/// One row in the Local Shell picker (Windows: cmd / PowerShell / a
/// WSL distro). Populated lazily by `dispatch.rs::ShowLocalShellPicker`
/// the first time the user opens the menu, then cached on `Oryxis`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocalShellSpec {
    /// User-facing label — e.g. "PowerShell", "cmd", "Ubuntu (WSL)".
    pub label: String,
    /// Executable to spawn. Bare program name (resolved via `PATH`)
    /// or a full path; passed to portable-pty's `CommandBuilder`.
    pub program: String,
    /// Arguments tacked on after the program. For WSL distros this
    /// is `["-d", "<distro-name>"]`; for plain shells it's empty.
    pub args: Vec<String>,
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
    /// Provider/network error — rendered as a red banner with a Retry
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
    #[allow(dead_code)]
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Parsed Markdown items for assistant messages — cached so the view
    /// can borrow them across renders. Iced's `markdown::view` returns an
    /// Element borrowing the items slice, so we can't parse on the fly.
    pub parsed_md: Vec<iced::widget::markdown::Item>,
}

// ---------------------------------------------------------------------------
// Terminal tab
// ---------------------------------------------------------------------------

/// One terminal pane — owns its alacritty grid and (optionally) the SSH
/// session feeding it. A `TerminalTab` holds one or more panes that are
/// rendered side-by-side or stacked depending on `PaneLayout`.
pub(crate) struct Pane {
    // Used by future split-routing logic (focus, PtyOutput dispatch); kept
    // now so the field is stable across the upcoming UI work.
    #[allow(dead_code)]
    pub id: Uuid,
    pub terminal: Arc<Mutex<TerminalState>>,
    /// SSH session handle (None for local shell).
    pub ssh_session: Option<Arc<SshSession>>,
    /// Session log ID for terminal recording.
    pub session_log_id: Option<Uuid>,
}

/// How panes inside a tab are arranged on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Horizontal/Vertical wired in next phase (split UI).
pub(crate) enum PaneLayout {
    /// Single pane filling the canvas — default for fresh tabs.
    Single,
    /// Two panes side by side (first on the left, second on the right).
    Horizontal,
    /// Two panes stacked (first on top, second on the bottom).
    Vertical,
}

/// A terminal tab — either a local shell or an SSH session. Holds 1+ panes
/// (only the first one is alive when the tab is created; user can split
/// it later to create the second).
pub(crate) struct TerminalTab {
    pub _id: Uuid,
    pub label: String,
    pub panes: Vec<Pane>,
    #[allow(dead_code)] // wired in the split-UI phase
    pub layout: PaneLayout,
    /// Index into `panes` of the currently focused split (0 when there's
    /// only one pane, 0 or 1 once split).
    pub focused_pane: usize,
    /// AI chat history for this terminal session.
    pub chat_history: Vec<ChatMessage>,
    /// Whether the AI chat sidebar is visible.
    pub chat_visible: bool,
    /// First-token allow-list for AI tool execution. Populated when the
    /// user clicks "ALWAYS RUN" on a confirmation prompt — future tool
    /// calls whose first whitespace-delimited token matches an entry
    /// here skip the prompt and run immediately. Per-tab so an
    /// "always run rm" decision on one host doesn't leak to others.
    pub chat_always_run_commands: Vec<String>,
}

impl TerminalTab {
    /// Build a new tab with a single pane. Use `panes.push(...)` + a
    /// non-`Single` layout to introduce splits later.
    pub fn new_single(label: String, terminal: Arc<Mutex<TerminalState>>) -> Self {
        Self {
            _id: Uuid::new_v4(),
            label,
            panes: vec![Pane {
                id: Uuid::new_v4(),
                terminal,
                ssh_session: None,
                session_log_id: None,
            }],
            layout: PaneLayout::Single,
            focused_pane: 0,
            chat_history: Vec::new(),
            chat_visible: false,
            chat_always_run_commands: Vec::new(),
        }
    }

    /// Currently focused pane — invariant maintained by `focus_pane` and
    /// the split / close handlers.
    pub fn active(&self) -> &Pane {
        &self.panes[self.focused_pane.min(self.panes.len() - 1)]
    }

    pub fn active_mut(&mut self) -> &mut Pane {
        let idx = self.focused_pane.min(self.panes.len() - 1);
        &mut self.panes[idx]
    }
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
    /// Forward the local ssh-agent socket to the remote shell. See the
    /// matching field on `Connection`.
    pub agent_forwarding: bool,
    /// Proxy configuration editor fields
    pub proxy_type: String,
    pub proxy_host: String,
    pub proxy_port: String,
    pub proxy_username: String,
    pub proxy_password: String,
    pub proxy_command: String,
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
            agent_forwarding: false,
            proxy_type: "(none)".into(),
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_command: String::new(),
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
    FolderActions(Uuid),
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
    Sftp,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Terminal,
    Sftp,
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

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
