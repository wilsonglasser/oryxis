//! SFTP view state (split out of `state.rs`).

use super::*;

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

    /// Dismiss every transient overlay menu: the row/background context
    /// menu and both panes' `⋮` actions + drive-picker dropdowns. Called
    /// by any menu action so the menu always closes on click.
    pub(crate) fn close_menus(&mut self) {
        self.row_menu = None;
        self.left.actions_open = false;
        self.right.actions_open = false;
        self.left.drives_open = false;
        self.right.drives_open = false;
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

impl TransferState {
    /// Build a fresh transfer. `total` is derived from the queue and
    /// `busy_slots` gets one flag per slot (kept 1-1 with the dispatch
    /// loop's `clients`); all the progress / conflict fields start empty.
    /// `slots` is the parallel-worker count (1 for DuplicateLocal/Relay).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: TransferKind,
        root_label: String,
        queue: std::collections::VecDeque<TransferItem>,
        clients: Vec<SftpClient>,
        dest_client: Option<SftpClient>,
        dest_side: Option<SftpPaneSide>,
        slots: u8,
    ) -> Self {
        Self {
            kind,
            root_label,
            total: queue.len(),
            queue,
            current: None,
            completed: 0,
            overwrite_default: None,
            pending_conflict_item: None,
            pending_conflict_slot: None,
            clients,
            dest_client,
            dest_side,
            busy_slots: vec![false; slots as usize],
            paused: false,
        }
    }
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
    /// Set when the menu was opened by right-clicking the empty area of
    /// the pane (not a row). The view then shows only directory-level
    /// actions and `path` holds the pane's current directory.
    pub is_background: bool,
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

/// Target of the SFTP close-guard confirmation modal: either a single tab or
/// "close every tab except this one". Drives `pending_sftp_close`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PendingSftpClose {
    /// Close just the tab at this index.
    One(usize),
    /// Close every tab except the one at this index.
    Others(usize),
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
