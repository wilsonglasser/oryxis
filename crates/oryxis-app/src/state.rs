//! Pure state types used by the Oryxis application.
//!
//! Everything here is standalone data, no references to the top-level `Oryxis`
//! struct. Split out of `app.rs` to keep that file focused on the state machine.

use std::sync::{Arc, Mutex};

use iced::widget::pane_grid;
use oryxis_core::models::connection::AuthMethod;
use oryxis_ssh::{SftpClient, SftpEntry, SshSession};
use oryxis_terminal::widget::TerminalState;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// SFTP view state
// ---------------------------------------------------------------------------

/// Per-pane state for the SFTP browser. Each pane is either Local
/// (`is_remote == false`, browsing the OS filesystem) or a mounted
/// remote host (`is_remote == true`, browsing via SFTP). The two panes
/// of [`SftpState`] are positional (Left / Right); their *nature* is
/// this `is_remote` flag, so either pane can be Local or remote
/// (with the rule that Local is only ever offered on the left).
#[derive(Default)]
pub(crate) struct PaneState {
    /// false = Local pane, true = Remote pane.
    pub is_remote: bool,
    // Remote connection (Some only when `is_remote` and connected).
    /// Currently mounted SSH session, if any. Cloned from the source host
    /// when the user picks one from the connection list.
    pub session: Option<Arc<SshSession>>,
    /// Active SFTP client (one channel per session).
    pub client: Option<SftpClient>,
    /// Label of the currently mounted host, shown in the breadcrumb.
    pub host_label: Option<String>,
    pub remote_path: String,
    pub remote_entries: Vec<SftpEntry>,
    pub remote_loading: bool,
    // Local (used when `!is_remote`).
    pub local_path: std::path::PathBuf,
    pub local_entries: Vec<LocalEntry>,
    /// Whether the Windows-style drive picker dropdown is open. Only
    /// rendered on Windows hosts.
    pub drives_open: bool,
    // Shared per-pane UI.
    pub error: Option<String>,
    pub filter: String,
    /// Sort column + direction for this pane.
    pub sort: SftpSort,
    /// When false (default), entries whose name starts with `.` are
    /// hidden, matches `ls` / Finder / Explorer convention. Toggleable
    /// from each pane's Actions menu independently so the user can show
    /// hidden remote files without exposing all the local dotfiles.
    pub show_hidden: bool,
    /// When `Some`, the breadcrumb is replaced by a text input the user
    /// can type a full path into. The string is the in-progress edit.
    pub path_editing: Option<String>,
    /// Actions popover anchored to this pane's header.
    pub actions_open: bool,
}

/// State for the SFTP browser. Two panes, side-by-side: the left pane is
/// Local by default but can be switched to any host; the right pane is
/// always a remote host. When both panes are remote, a transfer between
/// them uses the server-to-server relay primitive instead of
/// upload/download.
pub(crate) struct SftpState {
    /// Left pane, Local by default.
    pub left: PaneState,
    /// Right pane, a remote host (never Local).
    pub right: PaneState,
    /// True while the host picker overlay is visible (default at boot,
    /// hidden once a host is chosen).
    pub picker_open: bool,
    /// Which pane the currently open picker is choosing a host for.
    pub picker_target: SftpPaneSide,
    /// Search filter applied to the host picker.
    pub picker_search: String,
    /// Right-click row context menu, anchored to the click location
    /// and operating on a specific entry.
    pub row_menu: Option<SftpRowMenu>,
    /// Inline rename editor, replaces the row visually with a text
    /// input until the user commits or cancels.
    pub rename: Option<SftpRename>,
    /// Pending destructive action, surfaces a confirmation modal.
    /// `Vec` (instead of `Option`) so the same modal handles both single
    /// right-click delete and bulk delete from a multi-selection, the
    /// modal copy adapts to the count.
    pub delete_confirm: Vec<SftpDeleteTarget>,
    /// New file / new folder modal, kind + in-progress name input.
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
    /// One-shot destination override for the next upload, set by the
    /// drag-and-drop handler when the cursor lands on a specific remote
    /// folder, consumed by `SftpUpload` / `SftpUploadFolder`.
    pub upload_dest_override: Option<String>,
    /// Same idea for downloads, set when an internal drag from the
    /// remote pane lands on a specific local folder. Consumed by
    /// `SftpDownload` / `SftpDownloadFolder`.
    pub download_dest_override: Option<std::path::PathBuf>,
    /// Multi-row selection across both panes. Plain click on a file
    /// replaces this with a single entry; ctrl-click toggles; shift-click
    /// extends from `selection_anchor` within the same pane. Cleared
    /// whenever either pane navigates away.
    pub selected_rows: Vec<(SftpPaneSide, String)>,
    /// Last clicked row, origin point for shift-click range extension.
    /// Stays put across ctrl-click toggles so the range pivots from the
    /// initial selection point rather than the most recent toggle.
    pub selection_anchor: Option<(SftpPaneSide, String)>,
    /// Active edit-in-place session, a remote file downloaded to an OS
    /// temp path and opened in the user's default editor. Persists until
    /// the user clicks Save Back or Discard.
    pub edit_session: Option<EditSession>,
    /// Pending overwrite confirmation, set when the user uploads a file
    /// whose name already exists in the destination. Cleared when the
    /// user picks an action.
    pub overwrite_prompt: Option<OverwritePrompt>,
    /// Open Properties modal for a single row. Carries the snapshot
    /// of the current metadata + the user's in-progress edits to the
    /// permission bits so Apply can diff.
    pub properties: Option<PropertiesView>,
    /// True when the per-file progress panel (a dropdown above the
    /// transfer strip) is expanded. Toggled by clicking the strip.
    pub transfer_panel_open: bool,
    /// Labels of the items finished so far in the active transfer, for
    /// the per-file panel. Cleared when a new transfer starts.
    pub transfer_done_log: Vec<String>,
    /// Type-ahead search buffer: characters typed while a row is selected,
    /// used to jump the selection to the first matching entry. Reset after
    /// a short pause between keystrokes.
    pub type_ahead: String,
    /// Instant of the last type-ahead keystroke, for the reset timeout.
    pub type_ahead_at: Option<std::time::Instant>,
    /// The previous completed type-ahead sequence. When the user re-types
    /// the same string (after a pause), the search advances to the next
    /// match instead of restarting, so repeated typing cycles results.
    pub type_ahead_committed: String,
    /// Last plain row click `(side, path, when)`, used to detect a
    /// double-click (single click selects a folder, double click opens it).
    pub last_click: Option<(SftpPaneSide, String, std::time::Instant)>,
    /// Generation counter for the debounced type-ahead search. Each
    /// keystroke bumps it and schedules a deferred fire; only the fire
    /// whose generation still matches runs, so fast typing searches once
    /// (with the full buffer) instead of jumping on every key.
    pub type_ahead_gen: u64,
    /// Bytes transferred so far in the active transfer, incremented by the
    /// SFTP engine as chunks move. Drives the live progress bar (polled by
    /// a tick subscription while a transfer runs).
    pub transfer_bytes_done: Arc<std::sync::atomic::AtomicU64>,
    /// Total bytes the active transfer will move (sum of file sizes), for
    /// the bar's denominator. 0 when unknown (falls back to item counts).
    pub transfer_bytes_total: u64,
}

impl Default for SftpState {
    fn default() -> Self {
        // The derived `Default` for `PaneState` gives `is_remote == false`
        // for both panes; the right pane is always remote, so it has to be
        // constructed explicitly. Hand-writing this guarantees every
        // default site (tests, resets, boot) gets a correctly-natured
        // right pane.
        Self {
            left: PaneState {
                is_remote: false,
                ..Default::default()
            },
            right: PaneState {
                is_remote: true,
                ..Default::default()
            },
            picker_open: false,
            picker_target: SftpPaneSide::Right,
            picker_search: String::new(),
            row_menu: None,
            rename: None,
            delete_confirm: Vec::new(),
            new_entry: None,
            drop_active: false,
            hovered_row: None,
            drag: None,
            transfer: None,
            upload_dest_override: None,
            download_dest_override: None,
            selected_rows: Vec::new(),
            selection_anchor: None,
            edit_session: None,
            overwrite_prompt: None,
            properties: None,
            transfer_panel_open: false,
            transfer_done_log: Vec::new(),
            type_ahead: String::new(),
            type_ahead_at: None,
            type_ahead_committed: String::new(),
            last_click: None,
            type_ahead_gen: 0,
            transfer_bytes_done: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            transfer_bytes_total: 0,
        }
    }
}

impl SftpState {
    pub(crate) fn pane(&self, side: SftpPaneSide) -> &PaneState {
        match side {
            SftpPaneSide::Left => &self.left,
            SftpPaneSide::Right => &self.right,
        }
    }

    pub(crate) fn pane_mut(&mut self, side: SftpPaneSide) -> &mut PaneState {
        match side {
            SftpPaneSide::Left => &mut self.left,
            SftpPaneSide::Right => &mut self.right,
        }
    }

    /// The side of the remote pane used as an upload destination /
    /// download source. With the current model the right pane is always
    /// remote, and the left pane can also be remote; the upload/download
    /// paths only run with exactly one remote and one local pane, so we
    /// return the first remote side, preferring the right (the canonical
    /// remote pane). Returns `None` if neither pane is remote.
    pub(crate) fn remote_side(&self) -> Option<SftpPaneSide> {
        if self.right.is_remote {
            Some(SftpPaneSide::Right)
        } else if self.left.is_remote {
            Some(SftpPaneSide::Left)
        } else {
            None
        }
    }

    /// The side of the local pane (download destination / upload source).
    /// Returns `None` if neither pane is local.
    pub(crate) fn local_side(&self) -> Option<SftpPaneSide> {
        if !self.left.is_remote {
            Some(SftpPaneSide::Left)
        } else if !self.right.is_remote {
            Some(SftpPaneSide::Right)
        } else {
            None
        }
    }
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
    /// Original mode bits, used to detect unchanged Apply (no-op) and
    /// preserve the high bits (setuid/setgid/sticky) the dialog doesn't
    /// edit.
    pub original_mode: u32,
    pub bits: PermBits,
    /// True while the chmod task is in flight, disables the Apply
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
    /// True when the prompt is part of a multi-file transfer, surfaces
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
    /// size, cheap proxy for "is it actually a different file?" without
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
    /// Display label shown in the modal, basename of the remote file.
    pub label: String,
    /// Mtime of the temp file when it was first written (right after
    /// download). The watcher tick polls this to detect saves coming
    /// from the user's editor.
    pub initial_mtime: Option<std::time::SystemTime>,
    /// True once the watcher tick observes an mtime newer than
    /// `initial_mtime`, drives the "Changes detected" copy in the
    /// modal so the user knows their save was picked up.
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferKind {
    Upload,
    Download,
    /// Local-side `cp -r` equivalent, `std::fs` doesn't expose recursive
    /// copy so we walk the tree and copy each entry ourselves.
    DuplicateLocal,
    /// Server-to-server transfer: both `src` and `dst` are remote POSIX
    /// paths, on the source pane's host and the dest pane's host
    /// respectively. The runner streams via `SftpClient::relay_to`.
    Relay,
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
    /// Remote file size, populated only for download items from the
    /// directory listing that was walked. Passed to `download_to` as a
    /// hint so each file skips an extra `stat` round trip. `None` for
    /// uploads, local duplicates, and directories.
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct TransferState {
    pub kind: TransferKind,
    /// Top-level label shown in the progress bar, e.g. "my-folder".
    pub root_label: String,
    /// Pending items, popped one at a time as each operation completes.
    pub queue: std::collections::VecDeque<TransferItem>,
    /// Name of the item currently being processed; `None` between items.
    pub current: Option<String>,
    pub completed: usize,
    pub total: usize,
    /// Sticky overwrite decision, set when the user checks "Apply to
    /// remaining" in the conflict modal. Subsequent collisions auto-
    /// resolve with this action without re-prompting.
    pub overwrite_default: Option<OverwriteAction>,
    /// When `Some`, the current item has been popped and is waiting for
    /// the user to resolve a conflict modal. The path/size info is
    /// captured here so the resolve handler can reapply the action to
    /// the right destination without re-listing.
    pub pending_conflict_item: Option<TransferItem>,
    /// Slot that hit the conflict, needed so resolve uses the same
    /// SFTP client channel for the apply step.
    pub pending_conflict_slot: Option<u8>,
    /// One SFTP client per parallel slot. Empty for `DuplicateLocal`
    /// (no SFTP needed). For `Upload`/`Download` size is `concurrency`.
    /// For `Relay` these are the *source* host's clients.
    pub clients: Vec<SftpClient>,
    /// Destination-host SFTP client, populated only for `Relay`. The
    /// relay runs at concurrency 1 (a single dest client would otherwise
    /// contend on its inner lock / raw sessions across slots), so one
    /// dest client is enough.
    pub dest_client: Option<SftpClient>,
    /// Destination pane for a `Relay`. Needed so the finalize / cancel /
    /// error arms refresh the *destination* pane: a right-to-left relay
    /// has its destination on the left, which `remote_side()` (which
    /// prefers Right) would not pick. `None` for non-relay transfers.
    pub dest_side: Option<SftpPaneSide>,
    /// Per-slot "is in flight" flag. The Next handler picks the first
    /// `false` slot to dispatch to, keeping each slot mapped 1-1 with
    /// its `clients[i]` so workers never fight for the same channel.
    pub busy_slots: Vec<bool>,
    /// True while a conflict modal is up, workers exit on Next instead
    /// of popping more items, then get re-spawned by Resolve.
    pub paused: bool,
}

/// Which pane (by position) a side-addressed SFTP message / state item
/// refers to. This is *position* only; whether a pane is Local or remote
/// is its `PaneState::is_remote` flag, not its side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SftpPaneSide {
    Left,
    #[default]
    Right,
}

/// Internal drag state, a row being dragged from one pane towards the
/// other. The press position lets us suppress short jitters; only past
/// a small threshold do we treat the press+move as a drag rather than a
/// click. Multi-row drags carry the full set so a single drop fires N
/// transfers.
#[derive(Debug, Clone)]
pub(crate) struct SftpInternalDrag {
    pub origin_side: SftpPaneSide,
    /// `(path, is_dir)` per dragged item.
    pub items: Vec<(String, bool)>,
    /// Short label shown on the floating ghost, basename or "N items".
    pub label: String,
    /// Cursor position at left-press time. Used to gate `active` on
    /// distance threshold so accidental jitter doesn't get treated as
    /// a drag and steal click handling.
    pub press_pos: iced::Point,
    /// Once the cursor moves past a few pixels we commit to the drag
    /// the ghost renders, the drop highlight kicks in, and the eventual
    /// release dispatches a transfer instead of a click.
    pub active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SftpRowMenu {
    pub side: SftpPaneSide,
    /// Stringified path, `String` for both panes since the modal /
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

/// Sort modes available for the Hosts / Keychain / Snippets card
/// grids. Persisted per-list in the `settings` table under
/// `hosts_sort` / `keys_sort` / `snippets_sort` as the value of
/// `ListSort::as_storage_str()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ListSort {
    #[default]
    LabelAsc,
    LabelDesc,
    NewestFirst,
    OldestFirst,
}

impl ListSort {
    pub fn as_storage_str(self) -> &'static str {
        match self {
            ListSort::LabelAsc => "label_asc",
            ListSort::LabelDesc => "label_desc",
            ListSort::NewestFirst => "newest_first",
            ListSort::OldestFirst => "oldest_first",
        }
    }

    pub fn from_storage_str(s: &str) -> Self {
        match s {
            "label_desc" => ListSort::LabelDesc,
            "newest_first" => ListSort::NewestFirst,
            "oldest_first" => ListSort::OldestFirst,
            _ => ListSort::LabelAsc,
        }
    }

    /// Sort `items` in place using the row's label + creation
    /// timestamp. Labels are lowercased before comparison so case
    /// differences don't reorder rows the user thinks of as equal.
    pub fn sort_items<T, FLabel, FTime>(
        self,
        items: &mut [T],
        mut label_of: FLabel,
        mut created_at: FTime,
    ) where
        FLabel: FnMut(&T) -> String,
        FTime: FnMut(&T) -> chrono::DateTime<chrono::Utc>,
    {
        match self {
            // `sort_by_cached_key` lowercases each label once per item
            // instead of allocating two fresh Strings per comparison;
            // these sorts run on every redraw of the dashboard / keys /
            // snippets views.
            ListSort::LabelAsc => {
                items.sort_by_cached_key(|i| label_of(i).to_lowercase())
            }
            ListSort::LabelDesc => items.sort_by_cached_key(|i| {
                std::cmp::Reverse(label_of(i).to_lowercase())
            }),
            ListSort::NewestFirst => {
                items.sort_by_key(|i| std::cmp::Reverse(created_at(i)))
            }
            ListSort::OldestFirst => items.sort_by_key(|i| created_at(i)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
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
    /// User-facing label, e.g. "PowerShell", "cmd", "Ubuntu (WSL)".
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
// Terminal tab
// ---------------------------------------------------------------------------

/// What a pane reconnects to, so a saved session group can reference it.
/// This is an explicit discriminator rather than inferring "local" from a
/// missing connection id: cloud/SSM/ECS panes also lack a saved
/// `Connection`, so `None`-means-local would mis-save them. `Ephemeral`
/// covers those (and any pane we can't reference by id); they are pruned
/// when a tab is saved as a session group.
#[derive(Debug, Clone)]
pub(crate) enum PaneOrigin {
    /// Live reference to a saved Connection by id.
    Host(Uuid),
    /// A local terminal; the spec is captured so the same shell is restored.
    Local(LocalShellSpec),
    /// Cloud/SSM/ECS or otherwise non-referenceable pane.
    Ephemeral,
}

/// One terminal pane, owns its alacritty grid and (optionally) the SSH
/// session feeding it. A `TerminalTab` holds one or more panes in a
/// `pane_grid::State`, which owns their split layout.
pub(crate) struct Pane {
    /// Stable identity used to route PTY output / session events to the
    /// right pane (the `pane_grid::Pane` handle is only unique within a
    /// tab's grid, this `Uuid` is unique across all tabs).
    pub id: Uuid,
    /// This pane's own connection label ("user@host", "Local Shell", ...).
    /// The tab bar shows the *focused* pane's label + icon, so a tab split
    /// across two hosts reads as whichever pane you're in.
    pub label: String,
    pub terminal: Arc<Mutex<TerminalState>>,
    /// SSH session handle (None for local shell).
    pub ssh_session: Option<Arc<SshSession>>,
    /// Session log ID for terminal recording.
    pub session_log_id: Option<Uuid>,
    /// Recorded bytes not yet flushed to the vault. PTY output appends
    /// here; `Oryxis::flush_session_logs` drains it (size threshold, a
    /// periodic tick, disconnect, or window close). Batching keeps the
    /// vault from taking one write per SSH chunk.
    pub session_log_buf: Vec<u8>,
    /// What this pane reconnects to when restored from a saved session group.
    /// Defaults to `Ephemeral`; the creating site overrides it to `Host` or
    /// `Local` when the pane is referenceable.
    pub origin: PaneOrigin,
}

impl Pane {
    pub fn new(label: String, terminal: Arc<Mutex<TerminalState>>) -> Self {
        Self {
            id: Uuid::new_v4(),
            label,
            terminal,
            ssh_session: None,
            session_log_id: None,
            session_log_buf: Vec::new(),
            origin: PaneOrigin::Ephemeral,
        }
    }
}

/// A terminal tab. Its panes live in an iced `pane_grid::State`, which owns
/// the split layout (N-way horizontal / vertical splits) and resizing. A
/// fresh tab has exactly one pane; the user can split it.
pub(crate) struct TerminalTab {
    pub _id: Uuid,
    pub label: String,
    /// The pane tree (1+ panes). `pane_grid` owns the geometry.
    pub pane_grid: pane_grid::State<Pane>,
    /// Handle of the currently focused pane. Kept valid by the split /
    /// close / focus handlers; `active()` falls back to the first pane if
    /// it ever goes stale so we never index a closed pane.
    pub focused: pane_grid::Pane,
    /// AI chat history for this terminal session.
    pub chat_history: Vec<ChatMessage>,
    /// Whether the terminal sidebar is visible (Chat / Snippets / History
    /// tabs share this flag; the active tab is `Oryxis::terminal_sidebar_tab`).
    pub chat_visible: bool,
    /// First-token allow-list for AI tool execution. Populated when the
    /// user clicks "ALWAYS RUN" on a confirmation prompt, future tool
    /// calls whose first whitespace-delimited token matches an entry
    /// here skip the prompt and run immediately. Per-tab so an
    /// "always run rm" decision on one host doesn't leak to others.
    pub chat_always_run_commands: Vec<String>,
    /// True for cloud SSM / ECS-Exec tabs (a `session-manager-plugin`
    /// PTY). These talk SSM over a websocket whose idle timer kills the
    /// session after ~20 min of inactivity, so they get the
    /// resize-based keepalive while the window is unfocused. Plain SSH /
    /// local tabs leave this `false`.
    pub ssm_keepalive: bool,
    /// Message that re-creates this session, for "Duplicate Tab". Set
    /// only for cloud tabs that have no saved `Connection` to look up
    /// by label (ECS Exec, kubectl pod). SSH / InstanceConnect / SSM
    /// tabs are connection-backed and duplicate via label lookup
    /// instead, so they leave this `None`.
    pub relaunch: Option<Box<crate::messages::Message>>,
    /// Set when this tab was opened from a saved session group (or just
    /// saved as one). Drives the tab context menu label ("Save group" vs
    /// "Edit group") and lets the editor update the existing group in place.
    pub session_group_id: Option<Uuid>,
    /// Pinned tabs render first in the strip (compact icon chip or a
    /// bordered tab, per the `pinned_tab_style` setting) and are restored on
    /// the next launch. Toggled from the tab context menu.
    pub pinned: bool,
    /// Set on a *dormant* pinned tab recreated at boot: the tab shows in the
    /// strip but isn't connected. The first time it's selected, this spec
    /// reopens it (connect host / spawn local shell), then clears. `None` on
    /// a live tab.
    pub pending_reopen: Option<PinnedTabSpec>,
}

/// Reference to an open tab in the unified strip. Terminal and SFTP tabs
/// share one reorderable, pinnable row; identity is by `Uuid` (stable
/// across reorder / close) rather than a vec index. Reserved for the full
/// cross-type interleave / drag-reorder (deferred): SFTP tabs render grouped
/// after terminal tabs today, so `Terminal` is not yet constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TabRef {
    Terminal(Uuid),
    Sftp(Uuid),
}

/// An SFTP browser tab. Unlike terminal tabs, the **active** SFTP tab's
/// live state lives in `Oryxis::sftp` (a working buffer); this struct's
/// `state` field is a default placeholder while this tab is focused, and
/// holds the parked state while it is not. See the swap-on-focus invariant
/// in `SFTP_TABS_PLAN.md`: never read the active tab's state from the vec,
/// route by id through `Oryxis::route_sftp_async`.
pub(crate) struct SftpTab {
    pub id: Uuid,
    pub label: String,
    /// Pinned SFTP tabs render first in the strip.
    pub pinned: bool,
    /// Set on a dormant pinned SFTP tab recreated at boot: reopens (re-mounts
    /// its panes) the first time it's selected, then clears. Reserved for
    /// pin-restore-on-boot (deferred); not read yet.
    #[allow(dead_code)]
    pub pending_reopen: Option<PinnedTabSpec>,
    /// Parked state while this tab is not focused; a default placeholder while
    /// it IS the active tab (live state hoisted to `Oryxis::sftp`).
    pub state: SftpState,
}

impl SftpTab {
    pub(crate) fn new(label: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            label,
            pinned: false,
            pending_reopen: None,
            state: SftpState::default(),
        }
    }
}

/// Persisted restore spec for a pinned tab. Stored as JSON in the
/// `pinned_tabs` setting; on boot each becomes a dormant pinned tab that
/// reopens lazily on first select. Cloud / ephemeral tabs have no spec and
/// aren't persisted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PinnedTabSpec {
    /// A saved host, reopened with `ConnectSsh` (id resolved to an index
    /// fresh at reopen time, so it survives connection reordering).
    Host { id: Uuid, label: String },
    /// A local shell, reopened with the captured program / args.
    LocalShell { program: String, args: Vec<String>, label: String },
    /// An ECS Exec session, reopened with `ConnectEcsExecTask` (same
    /// mechanism the in-session reconnect uses; the task id may have
    /// recycled, in which case the reconnect re-resolves the group).
    EcsExec {
        group_id: Uuid,
        task_id: String,
        task_label: String,
        container: String,
        label: String,
    },
    /// A kubectl exec session, reopened with `ConnectKubectlExecPod`.
    KubectlExec {
        group_id: Uuid,
        namespace: String,
        pod: String,
        container: String,
        label: String,
    },
    /// A pinned SFTP browser tab. Captures both panes (Local vs which
    /// connection); reopened dormant and re-mounts its remote pane(s) on first
    /// focus.
    Sftp {
        left: SftpPaneSpec,
        right: SftpPaneSpec,
        label: String,
    },
}

/// Restore spec for one SFTP pane: Local browsing, or a remote host by saved
/// connection id (resolved fresh at reopen so it survives reordering).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum SftpPaneSpec {
    Local,
    Remote(Uuid),
}

/// In-progress drag of a tab in the strip, for reordering. Started on press
/// (`SelectTab`), promoted to `active` once the cursor moves past a small
/// threshold (so a plain click isn't a drag), committed on mouse release
/// onto the hovered tab. Reorder is restricted to within the same group
/// (pinned among pinned, normal among normal).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TabDrag {
    /// The tab being dragged, by id so it survives any reindexing (a tab
    /// closing mid-drag) and resolves to the right source at drop time.
    pub from_id: Uuid,
    /// Cursor position at press, for the move threshold.
    pub start: iced::Point,
    /// Promoted past the threshold (a real drag, not a click).
    pub active: bool,
}

impl PinnedTabSpec {
    pub fn label(&self) -> &str {
        match self {
            PinnedTabSpec::Host { label, .. } => label,
            PinnedTabSpec::LocalShell { label, .. } => label,
            PinnedTabSpec::EcsExec { label, .. } => label,
            PinnedTabSpec::KubectlExec { label, .. } => label,
            PinnedTabSpec::Sftp { label, .. } => label,
        }
    }

    /// Identity key for de-duplicating pins. Ephemeral resource ids
    /// (ECS task, K8s pod) are excluded on purpose: a recycled task
    /// produces a spec with a different task_id but it is still the
    /// same pin, and keeping both is how duplicate chips appear.
    pub fn dedupe_key(&self) -> String {
        match self {
            PinnedTabSpec::Host { id, .. } => format!("host:{id}"),
            PinnedTabSpec::LocalShell { program, args, label } => {
                format!("local:{program}:{}:{label}", args.join("\u{1f}"))
            }
            PinnedTabSpec::EcsExec { group_id, container, .. } => {
                format!("ecs:{group_id}:{container}")
            }
            PinnedTabSpec::KubectlExec { group_id, namespace, container, .. } => {
                format!("k8s:{group_id}:{namespace}:{container}")
            }
            PinnedTabSpec::Sftp { left, right, .. } => {
                let key = |p: &SftpPaneSpec| match p {
                    SftpPaneSpec::Local => "local".to_string(),
                    SftpPaneSpec::Remote(id) => format!("remote:{id}"),
                };
                format!("sftp:{}:{}", key(left), key(right))
            }
        }
    }
}

impl TerminalTab {
    /// Build a new tab with a single pane. Split it later via
    /// `pane_grid.split(...)`.
    pub fn new_single(label: String, terminal: Arc<Mutex<TerminalState>>) -> Self {
        let (pane_grid, focused) = pane_grid::State::new(Pane::new(label.clone(), terminal));
        Self {
            _id: Uuid::new_v4(),
            label,
            pane_grid,
            focused,
            chat_history: Vec::new(),
            chat_visible: false,
            chat_always_run_commands: Vec::new(),
            ssm_keepalive: false,
            relaunch: None,
            session_group_id: None,
            pinned: false,
            pending_reopen: None,
        }
    }

    /// A dormant pinned tab recreated at boot: shows in the strip with the
    /// saved label but holds no live session. The placeholder pane carries a
    /// hint; selecting the tab the first time fires `spec` to reopen it.
    pub fn new_dormant_pinned(label: String, spec: PinnedTabSpec) -> Self {
        let mut term = TerminalState::new_no_pty(80, 24).unwrap();
        let hint = format!("\x1b[2m  {}\x1b[0m\r\n", crate::i18n::t("pinned_tab_dormant_hint"));
        term.process(hint.as_bytes());
        let mut tab = Self::new_single(label, Arc::new(Mutex::new(term)));
        tab.pinned = true;
        tab.pending_reopen = Some(spec);
        tab
    }

    /// Restore spec for persisting this pinned tab, or `None` if it can't be
    /// reopened (cloud / ephemeral pane with no stable reference). A dormant
    /// tab keeps the spec it was created with; a live tab derives one from
    /// its focused pane's origin.
    pub fn pin_spec(&self) -> Option<PinnedTabSpec> {
        if let Some(spec) = &self.pending_reopen {
            return Some(spec.clone());
        }
        let base = self.label.trim_end_matches(" (disconnected)").to_string();
        match &self.active().origin {
            PaneOrigin::Host(id) => Some(PinnedTabSpec::Host { id: *id, label: base }),
            PaneOrigin::Local(spec) => Some(PinnedTabSpec::LocalShell {
                program: spec.program.clone(),
                args: spec.args.clone(),
                label: spec.label.clone(),
            }),
            // Cloud exec tabs have no saved Connection, but carry the
            // relaunch message that recreates them; mirror it into a
            // serializable spec. SSM (relaunch None) and anything else stay
            // unpersisted.
            PaneOrigin::Ephemeral => match self.relaunch.as_deref() {
                Some(crate::messages::Message::ConnectEcsExecTask {
                    group_id,
                    task_id,
                    task_label,
                    container,
                }) => Some(PinnedTabSpec::EcsExec {
                    group_id: *group_id,
                    task_id: task_id.clone(),
                    task_label: task_label.clone(),
                    container: container.clone(),
                    label: base,
                }),
                Some(crate::messages::Message::ConnectKubectlExecPod {
                    group_id,
                    namespace,
                    pod,
                    container,
                }) => Some(PinnedTabSpec::KubectlExec {
                    group_id: *group_id,
                    namespace: namespace.clone(),
                    pod: pod.clone(),
                    container: container.clone(),
                    label: base,
                }),
                _ => None,
            },
        }
    }

    /// Currently focused pane. Falls back to the first pane if `focused`
    /// is stale (e.g. just after a close), so this never panics.
    pub fn active(&self) -> &Pane {
        self.pane_grid
            .get(self.focused)
            .or_else(|| self.pane_grid.panes.values().next())
            .expect("a tab always has at least one pane")
    }

    pub fn active_mut(&mut self) -> &mut Pane {
        // Resolve a valid key first (repairing `focused` if it went
        // stale), then take the mutable borrow.
        let key = if self.pane_grid.panes.contains_key(&self.focused) {
            self.focused
        } else {
            let k = *self
                .pane_grid
                .panes
                .keys()
                .next()
                .expect("a tab always has at least one pane");
            self.focused = k;
            k
        };
        self.pane_grid.get_mut(key).expect("valid pane key")
    }

    /// Look up a pane by its stable `Uuid` (for routing PTY output /
    /// session events).
    pub fn pane_by_id_mut(&mut self, id: Uuid) -> Option<&mut Pane> {
        self.pane_grid.panes.values_mut().find(|p| p.id == id)
    }

    /// Number of panes in this tab. `> 1` means the tab is split.
    pub fn pane_count(&self) -> usize {
        self.pane_grid.panes.len()
    }

    /// Label to show in the tab strip. A tab opened from (or saved as) a
    /// session group shows the group's name. Otherwise a split tab follows
    /// the *focused* pane (so a tab split across two hosts reads as whichever
    /// pane you're in); a single-pane tab uses the tab's own label, which
    /// carries the "(disconnected)" suffix the focused-pane label doesn't.
    pub fn display_label(&self) -> &str {
        if self.session_group_id.is_some() {
            &self.label
        } else if self.pane_count() > 1 {
            &self.active().label
        } else {
            &self.label
        }
    }
}

// ---------------------------------------------------------------------------
// Connection editor form
// ---------------------------------------------------------------------------

#[cfg(test)]
mod terminal_tab_tests {
    use super::*;

    fn dummy_terminal() -> Arc<Mutex<TerminalState>> {
        Arc::new(Mutex::new(TerminalState::new_no_pty(80, 24).unwrap()))
    }

    fn split(tab: &mut TerminalTab, axis: pane_grid::Axis) -> pane_grid::Pane {
        let (handle, _) = tab
            .pane_grid
            .split(axis, tab.focused, Pane::new("p".into(), dummy_terminal()))
            .expect("split");
        tab.focused = handle;
        handle
    }

    #[test]
    fn split_then_close_keeps_focused_on_a_live_pane() {
        let mut tab = TerminalTab::new_single("t".into(), dummy_terminal());
        assert_eq!(tab.pane_grid.panes.len(), 1);
        split(&mut tab, pane_grid::Axis::Vertical);
        split(&mut tab, pane_grid::Axis::Horizontal);
        assert_eq!(tab.pane_grid.panes.len(), 3);

        // Close the focused pane the way `ClosePane` does, then point
        // `focused` at the sibling that took over.
        let (_, sibling) = tab.pane_grid.close(tab.focused).expect("close");
        tab.focused = sibling;
        assert_eq!(tab.pane_grid.panes.len(), 2);

        // `active()` must resolve to one of the surviving panes, never panic.
        let active_id = tab.active().id;
        assert!(tab.pane_grid.panes.values().any(|p| p.id == active_id));
    }

    #[test]
    fn active_falls_back_when_focused_is_stale() {
        let mut tab = TerminalTab::new_single("t".into(), dummy_terminal());
        let handle = split(&mut tab, pane_grid::Axis::Vertical);
        // Close the focused pane WITHOUT repairing `focused` (simulating a
        // missed update): `active()` must still return a live pane.
        tab.pane_grid.close(handle);
        let _ = tab.active().id; // must not panic
        // `active_mut()` repairs `focused` to a valid handle.
        let id = tab.active_mut().id;
        assert!(tab.pane_grid.panes.values().any(|p| p.id == id));
    }

    #[test]
    fn pane_by_id_mut_targets_the_right_pane() {
        let mut tab = TerminalTab::new_single("t".into(), dummy_terminal());
        let id1 = tab.active().id;
        let h2 = split(&mut tab, pane_grid::Axis::Vertical);
        let id2 = tab.pane_grid.get(h2).unwrap().id;
        assert_ne!(id1, id2);
        assert_eq!(tab.pane_by_id_mut(id1).map(|p| p.id), Some(id1));
        assert_eq!(tab.pane_by_id_mut(id2).map(|p| p.id), Some(id2));
        assert!(tab.pane_by_id_mut(Uuid::new_v4()).is_none());
    }
}

/// One editable row in the session-group editor: a pane's display label
/// (read-only) plus its per-pane initial script. Rows are ordered the same
/// as the layout's leaf walk, so scripts merge back by index on save.
#[derive(Debug, Clone, Default)]
pub(crate) struct PaneScriptRow {
    /// Read-only label for the pane ("user@host", "Local Shell", ...).
    pub label: String,
    /// Per-pane initial script (override-with-fallback).
    pub script: String,
}

/// Session-group editor form state. The structural `layout` is snapshotted
/// from the tab when the editor opens; `pane_rows` exposes each leaf's script
/// for editing and merges back into the layout (by leaf order) on save.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionGroupForm {
    pub label: String,
    /// Folder (Group) label, same convention as `ConnectionForm.group_name`.
    pub group_name: String,
    pub color: Option<String>,
    pub icon_style: Option<String>,
    /// Some when editing an existing session group (update in place).
    pub editing_id: Option<Uuid>,
    /// Index of the tab this group was snapshotted from, so saving can stamp
    /// its `session_group_id`.
    pub source_tab: Option<usize>,
    /// Structural snapshot of the split tree. Leaf scripts are placeholders
    /// here; the live values live in `pane_rows` and merge back on save.
    pub layout: Option<oryxis_core::models::PaneLayout>,
    pub pane_rows: Vec<PaneScriptRow>,
    /// Which pane's script is currently shown in the editor (the chevrons
    /// step this). The live multi-line buffer for it lives in
    /// `Oryxis::session_group_script_editor` (text_editor::Content isn't
    /// Clone, so it can't sit in this form struct).
    pub current_pane: usize,
}

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
    /// Ordered jump-host chain (connection ids). The session tunnels
    /// through each hop in order before reaching this host. Mirrors
    /// `Connection.jump_chain` one-to-one; edited via the chain editor.
    pub jump_chain: Vec<Uuid>,
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
    pub env_vars: Vec<EnvVarForm>,
    /// Whether this host is exposed via MCP.
    pub mcp_enabled: bool,
    /// Forward the local ssh-agent socket to the remote shell. See the
    /// matching field on `Connection`.
    pub agent_forwarding: bool,
    /// Per-host session-recording override. `None` follows the global
    /// setting; `Some(true)`/`Some(false)` force on/off. See the matching
    /// field on `Connection`.
    pub session_logging: Option<bool>,
    /// Proxy kind selection (None = disabled). The picker stores the
    /// typed enum so language switches don't break selection identity.
    pub proxy_kind: ProxyKind,
    pub proxy_host: String,
    pub proxy_port: String,
    pub proxy_username: String,
    pub proxy_password: String,
    pub proxy_command: String,
    /// Mirrors `has_existing_password` / `password_touched`: avoids
    /// pre-loading the encrypted proxy password into form state on edit
    /// and lets save distinguish "preserve" from "explicitly cleared".
    pub has_existing_proxy_password: bool,
    pub proxy_password_touched: bool,
    /// Per-host terminal palette override. `None` means "inherit the
    /// global pick"; `Some(name)` pins this host to the named palette.
    /// Mirrors `Connection.terminal_theme` while the editor is open.
    pub terminal_theme: Option<String>,
    /// Per-host SSH keepalive override (raw text). Empty string means
    /// inherit the global setting; "0" disables keepalive on this host;
    /// any positive integer overrides the global value. Stored as a
    /// string while the editor is open so the input field can show
    /// what the user typed; serialized to `Option<u32>` on save.
    pub keepalive_interval: String,
    /// Cloud-managed transport selection. Only meaningful when the
    /// connection being edited has a `cloud_ref`, the editor renders
    /// the picker conditionally. `None` here = "no cloud_ref to
    /// edit". The actual `cloud_ref.transport_pref` field is
    /// preserved when the user doesn't touch this picker.
    pub cloud_transport:
        Option<oryxis_core::models::cloud::TransportKind>,
    /// Per-host icon shape override. `None` falls back to the global
    /// `default_host_icon` setting. Mirrors `Connection.icon_style`.
    pub icon_style: Option<String>,
    pub encoding: Option<String>,
}

/// UI-side proxy kind. Includes a `None` (disabled) variant, the
/// model's `ProxyType` doesn't have a "disabled" since that's
/// represented by `Connection.proxy = None`. The `Identity(Uuid)`
/// variant points at a saved `ProxyIdentity`; when present, the
/// connection's `proxy_identity_id` is stored instead of an inline
/// `ProxyConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyKind {
    None,
    Socks5,
    Socks4,
    Http,
    Command,
    Identity(Uuid),
}

impl ProxyKind {
    /// The static (non-identity) variants, in picker display order.
    /// Used as the base of the editor's proxy picker; the host panel
    /// concatenates the user's saved proxy identities afterwards.
    pub const STATIC: &[ProxyKind] = &[
        ProxyKind::None,
        ProxyKind::Socks5,
        ProxyKind::Socks4,
        ProxyKind::Http,
        ProxyKind::Command,
    ];

    /// i18n key for the localized label rendered in the picker. `None`
    /// is returned for `Identity(_)`, saved-identity rendering uses
    /// the identity's `label`, not a static key.
    pub fn label_key(&self) -> Option<&'static str> {
        match self {
            ProxyKind::None => Some("proxy_type_none"),
            ProxyKind::Socks5 => Some("proxy_type_socks5"),
            ProxyKind::Socks4 => Some("proxy_type_socks4"),
            ProxyKind::Http => Some("proxy_type_http"),
            ProxyKind::Command => Some("proxy_type_command"),
            ProxyKind::Identity(_) => None,
        }
    }

    /// Default port for the proxy type, pre-filled when the user
    /// switches kind and the port field is still empty.
    pub fn default_port(&self) -> Option<u16> {
        match self {
            ProxyKind::Socks5 | ProxyKind::Socks4 => Some(1080),
            ProxyKind::Http => Some(8080),
            ProxyKind::None | ProxyKind::Command | ProxyKind::Identity(_) => None,
        }
    }

    /// Whether the host/port/username trio applies. `Command` runs a
    /// process directly, `None` disables the proxy, and `Identity`
    /// pulls those fields from the saved identity instead.
    pub fn needs_endpoint(&self) -> bool {
        matches!(self, ProxyKind::Socks5 | ProxyKind::Socks4 | ProxyKind::Http)
    }

    /// Whether a password field makes sense. SOCKS4 has no password
    /// concept; Command, None and Identity don't either (Identity
    /// edits its password in the saved-identity form).
    pub fn supports_password(&self) -> bool {
        matches!(self, ProxyKind::Socks5 | ProxyKind::Http)
    }
}

impl std::fmt::Display for ProxyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Localized at render time. The picker compares variants via
        // PartialEq, so language switches do not invalidate the
        // selected value. `Identity(_)` falls back to a generic label
        //, the host panel installs a custom mapper that swaps in the
        // identity's user-chosen label at render time.
        match self.label_key() {
            Some(k) => write!(f, "{}", crate::i18n::t(k)),
            None => write!(f, "{}", crate::i18n::t("proxy_type_identity_fallback")),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PortForwardForm {
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EnvVarForm {
    pub key: String,
    pub value: String,
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
            jump_chain: Vec::new(),
            selected_identity: None,
            editing_id: None,
            has_existing_password: false,
            password_touched: false,
            password_visible: false,
            username_focused: false,
            port_forwards: Vec::new(),
            env_vars: Vec::new(),
            mcp_enabled: true,
            agent_forwarding: false,
            session_logging: None,
            proxy_kind: ProxyKind::None,
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_command: String::new(),
            has_existing_proxy_password: false,
            proxy_password_touched: false,
            terminal_theme: None,
            keepalive_interval: String::new(),
            cloud_transport: None,
            icon_style: None,
            encoding: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Overlay (floating context menus)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum OverlayContent {
    HostActions(usize),
    /// Kebab / right-click menu on a session-group card. Items: Open, Edit,
    /// Duplicate, Delete.
    SessionGroupActions(usize),
    KeyActions(usize),
    IdentityActions(usize),
    /// Kebab menu on a snippet card. Items: Edit and Delete.
    SnippetActions(usize),
    KeychainAdd,
    TabActions(usize),
    /// Right-click menu on an SFTP browser tab. Items: New SFTP tab,
    /// Pin/Unpin, Close. `usize` is the `sftp_tabs` index.
    SftpTabActions(usize),
    /// Hover popover under the `+` tab button: New Tab + Split actions for
    /// the active terminal tab.
    SplitMenu,
    FolderActions(Uuid),
    CloudProfileActions(Uuid),
    /// Kebab menu on a dynamic-group card (ECS / K8s service folder).
    /// Items: Edit (template) and Delete.
    DynamicGroupActions(Uuid),
    /// Dropdown menu rendered next to "+ Host", lists every
    /// configured cloud profile so the user can launch discovery
    /// directly from the Hosts view. Only opened when at least one
    /// profile is configured (otherwise the chevron is hidden).
    CloudProviderPicker,
    /// Floating context menu for the Discover import modal's
    /// "Import into" combo. Carries a search input + the full list
    /// of user groups. Rendered through the modal's local Stack
    /// (the global overlay path is short-circuited by the modal's
    /// early return).
    CloudDiscoverGroupPicker,
    /// Shared group-picker popover for side-panel Parent Group
    /// inputs. The target enum tells the dispatch which form field
    /// the picked value flows into so the same overlay machinery
    /// (search + list) serves both the host editor and the dynamic
    /// group editor without duplicate state.
    GroupPicker(GroupPickerTarget),
    /// Sort dropdown anchored to the toolbar sort button in one of
    /// the card-grid views (Hosts / Keychain / Snippets).
    SortMenu(SortMenuKind),
}

/// Which side-panel input the shared group picker is currently
/// driving. Each panel carries its own combo bounds cell so the
/// popover anchors precisely under the right chevron.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupPickerTarget {
    DynamicFormParent,
    SessionGroupFolder,
}

/// Host editor's startup-command source. `None` runs nothing; `Snippet`
/// seeds the command from a saved snippet (snapshotted into the command
/// text on save); `Custom` is the free-text editor. On reopen the choice
/// is recovered by matching the stored command against snippet bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupChoice {
    None,
    Custom,
    Snippet(uuid::Uuid),
}

/// Which list the open sort menu controls. Drives both the dispatched
/// `Set*Sort` message and the icon shown on the trigger button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortMenuKind {
    Hosts,
    Keys,
    Snippets,
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

/// Active tab inside the terminal-side panel. `Chat` is only reachable
/// when AI is enabled; the dispatch falls back to `Snippets` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalSidebarTab {
    #[default]
    Chat,
    Snippets,
}

/// Identifies a secret text field whose reveal/eye toggle is on. One
/// shared enum + a `HashSet` in app state instead of a bool per field,
/// so adding the eye to a new password input is a one-variant change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretField {
    /// Inline proxy password in the host editor.
    ProxyPassword,
    /// Password on the Share (portable export) dialog.
    SharePassword,
    /// AI assistant API key (Settings > AI).
    AiApiKey,
    /// New master password (Settings > Security).
    VaultNewPassword,
    /// Portable export password (Settings > Security).
    ExportPassword,
    /// Portable import password (Settings > Security).
    ImportPassword,
    /// Sync signaling token (Settings > Sync).
    SyncSignalingToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Terminal,
    Keys,
    Snippets,
    PortForwarding,
    /// Cloud-account CRUD. Promoted to a top-level vault surface
    /// (sub-nav pill / sidebar entry); the Cloud Sync settings block
    /// stays behind in Settings.
    Cloud,
    /// Proxy-identity CRUD. Promoted to a top-level vault surface.
    Proxies,
    /// Known-host management. Promoted back to a top-level vault
    /// surface alongside Cloud / Proxies (was a SettingsSection in
    /// v0.7).
    KnownHosts,
    History,
    Sftp,
    Settings,
}

/// One row in the Plugins panel: a cloud-provider plugin and its
/// install / update state. Cloud providers ship as downloaded
/// subprocess plugins (see `crate::plugins`); this is the UI-side
/// view of one.
#[derive(Debug, Clone)]
pub struct PluginUiEntry {
    /// Provider id, matches `CloudProvider::id()` (`"aws"`, ...).
    pub provider_id: String,
    /// Human-readable name shown in the panel.
    pub display_name: String,
    /// Current install / update state.
    pub status: PluginUiStatus,
    /// Per-plugin auto-update override, resolved against the global
    /// default when the panel loads.
    pub auto_update: bool,
    /// User-pinned version. When set, the updater won't move off it.
    pub pinned_version: Option<String>,
    /// Downloaded binaries exist in the plugin cache (or, for MCP,
    /// the launcher copy). Lets a dev build still offer "remove
    /// downloaded files" for the cache it shadows.
    pub cached_install: bool,
    /// Last successfully fetched manifest. Drives the install modal's
    /// size / changelog. `None` until a check runs (and on every
    /// machine until the manifest host exists, see PR 6).
    pub manifest: Option<crate::plugins::PluginManifest>,
}

/// Install / update lifecycle state for a [`PluginUiEntry`].
#[derive(Debug, Clone, PartialEq)]
pub enum PluginUiStatus {
    /// No binary on disk and no dev build, the plugin must be
    /// downloaded before its provider can be used.
    NotInstalled,
    /// Running from a freshly-built `target/debug` binary (the dev
    /// loop). No version directory, no manifest involved.
    DevBuild,
    /// Installed from the cache at this version.
    Installed(String),
    /// Installed, and the manifest advertises a newer compatible
    /// version.
    UpdateAvailable { current: String, latest: String },
    /// A manifest fetch is in flight.
    Checking,
    /// A binary download + verify is in flight (indeterminate).
    Downloading,
    /// The last check / install failed; carries a user-facing message.
    Failed(String),
}

/// Cloud provider picked in the wizard. AWS authenticates via named
/// profile / access key / SSO; Kubernetes via a kubeconfig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudProviderChoice {
    #[default]
    Aws,
    K8s,
}

/// Which kind of `PodSelector` a K8s dynamic group's editor produces.
/// `Labels` takes a `k=v,k=v` string; the rest take a single resource
/// name (the resolver expands it to that workload's / pod's selector).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum K8sSelectorKind {
    #[default]
    Labels,
    Deployment,
    StatefulSet,
    Name,
}

impl K8sSelectorKind {
    pub const ALL: [K8sSelectorKind; 4] = [
        K8sSelectorKind::Labels,
        K8sSelectorKind::Deployment,
        K8sSelectorKind::StatefulSet,
        K8sSelectorKind::Name,
    ];
}

impl std::fmt::Display for K8sSelectorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            K8sSelectorKind::Labels => "Labels",
            K8sSelectorKind::Deployment => "Deployment",
            K8sSelectorKind::StatefulSet => "StatefulSet",
            K8sSelectorKind::Name => "Pod name",
        })
    }
}

impl CloudProviderChoice {
    pub fn id(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::K8s => "k8s",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "k8s" => Self::K8s,
            _ => Self::Aws,
        }
    }
}

/// Auth strategy chosen in the wizard. Only `Profile` is implemented in
/// v0.6 PR 3; the other variants render disabled with a hint and route
/// to `CloudError::Unsupported` if somehow selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudAuthChoice {
    #[default]
    Profile,
    AccessKey,
    Sso,
    Kubeconfig,
}

impl CloudAuthChoice {
    pub fn id(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::AccessKey => "access_key",
            Self::Sso => "sso",
            Self::Kubeconfig => "kubeconfig",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "access_key" => Self::AccessKey,
            "sso" => Self::Sso,
            "kubeconfig" => Self::Kubeconfig,
            _ => Self::Profile,
        }
    }
}

/// Live state of the "Test credentials" button in the wizard.
#[derive(Debug, Clone, Default)]
pub enum CloudTestState {
    #[default]
    Idle,
    Running,
    Ok,
    Failed(String),
}

/// State of the wizard's "Discover & pick" panel, owns the in-flight
/// or completed discovery result so the user can scroll/select without
/// re-hitting the cloud.
#[derive(Debug, Clone, Default)]
pub enum CloudDiscoverState {
    #[default]
    Idle,
    Running,
    Loaded(oryxis_cloud::DiscoveryResult),
    Failed(String),
}


/// Per-dynamic-group resolve state. Lives in a `HashMap<group_id, _>`
/// on `Oryxis` so opening one group doesn't blow away another's
/// cached resolve. TTL handling lives on the call site.
#[derive(Debug, Clone)]
pub enum DynamicGroupState {
    Loading,
    Loaded {
        hosts: Vec<oryxis_cloud::DiscoveredHost>,
        // When this list was fetched. `OpenGroup` compares against
        // `Utc::now()` and re-resolves past the cache TTL so a recycled
        // ECS task doesn't sit as a dead row until a manual Refresh.
        fetched_at: chrono::DateTime<chrono::Utc>,
    },
    Failed(String),
}

/// One mDNS-discovered peer the user could pair with. Lives in
/// `Oryxis.sync_discovered`, deduped by `device_id`, rebuilt as
/// `SyncEngineEvent::PeerDiscovered` arrives.
#[derive(Debug, Clone)]
pub(crate) struct DiscoveredPeerInfo {
    pub device_id: Uuid,
    pub device_name: String,
    pub addr: std::net::SocketAddr,
}

/// Which pairing sub-view the Sync settings panel is showing. The
/// hosted code itself lives in `Oryxis.sync_pairing_code`; the join
/// inputs live in `sync_join_code_input` / `sync_join_target_input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SyncPairingState {
    /// Default: just the two "Host" / "Join" entry buttons.
    #[default]
    Idle,
    /// This device is hosting a code, waiting for a peer to join.
    Hosting,
    /// This device is entering another device's code + address.
    Joining,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Terminal,
    /// SSH connection behaviour shared across hosts: keepalive
    /// interval, auto-reconnect, OS detection. Split out of the
    /// Terminal section, which had grown into a grab-bag of terminal
    /// display, connection and logging knobs.
    Connection,
    Sftp,
    AI,
    /// Visual + layout preferences. Absorbs the legacy "Theme" section
    /// and adds toggles for status bar visibility and (in later PRs)
    /// layout mode, tab close button position, host icon style, etc.
    Interface,
    /// MCP server (Model Context Protocol). Was bundled into the
    /// installer in 0.6 and lived inside the Security section; in
    /// 0.7 it's distributed as a plugin and gets its own section
    /// in the Settings sidebar so the setup-guide affordances and
    /// the enable toggle aren't buried.
    Mcp,
    Shortcuts,
    Security,
    Sync,
    /// Cloud Sync preferences (auto-refresh interval, orphan
    /// auto-archive). The cloud *account* CRUD moved to the top-level
    /// `View::Cloud` surface; this section keeps only the sync knobs.
    Cloud,
    /// Cloud provider plugins management: install, update, uninstall
    /// the subprocess plugins each cloud provider runs as. Sits next
    /// to `Cloud` because every cloud account here needs a matching
    /// plugin to actually function.
    Plugins,
    About,
}

// ---------------------------------------------------------------------------
// Custom terminal theme editor
// ---------------------------------------------------------------------------

/// One editable color slot in the custom terminal theme editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeColorSlot {
    Foreground,
    Background,
    Cursor,
    Ansi(u8),
}

/// In-progress edit of a custom terminal theme. `None` for `editing_id`
/// means a brand new theme; `Some(id)` edits an existing one. Colors are
/// `"#RRGGBB"` hex strings being typed.
#[derive(Debug, Clone)]
pub(crate) struct ThemeEditorForm {
    pub editing_id: Option<uuid::Uuid>,
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub ansi: [String; 16],
    pub error: Option<String>,
}

impl ThemeEditorForm {
    pub fn from_theme(
        t: &oryxis_core::models::custom_terminal_theme::CustomTerminalTheme,
    ) -> Self {
        Self {
            editing_id: Some(t.id),
            name: t.name.clone(),
            foreground: t.foreground.clone(),
            background: t.background.clone(),
            cursor: t.cursor.clone(),
            ansi: t.ansi.clone(),
            error: None,
        }
    }


    /// Write the color string for a slot.
    pub fn set_slot(&mut self, slot: ThemeColorSlot, value: String) {
        match slot {
            ThemeColorSlot::Foreground => self.foreground = value,
            ThemeColorSlot::Background => self.background = value,
            ThemeColorSlot::Cursor => self.cursor = value,
            ThemeColorSlot::Ansi(i) => self.ansi[i as usize] = value,
        }
    }
}

/// In-progress edit of a custom UI (chrome) theme. `colors` holds the 21
/// `"#rrggbb"` strings indexed by `theme::UI_COLOR_FIELDS`.
#[derive(Debug, Clone)]
pub(crate) struct UiThemeEditorForm {
    pub editing_id: Option<uuid::Uuid>,
    pub name: String,
    pub colors: [String; 21],
    pub error: Option<String>,
}

impl UiThemeEditorForm {
    pub fn from_theme(
        t: &oryxis_core::models::custom_ui_theme::CustomUiTheme,
    ) -> Self {
        Self {
            editing_id: Some(t.id),
            name: t.name.clone(),
            colors: t.colors.clone(),
            error: None,
        }
    }

    /// New theme seeded from a base palette (the 21 hex of an existing
    /// theme), so the user starts from something that already works.
    pub fn new_from_colors(colors: [String; 21]) -> Self {
        Self { editing_id: None, name: String::new(), colors, error: None }
    }
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
    Disconnected,
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
