//! Overlay (floating context menus) (split out of `state.rs`).

use super::*;

#[derive(Debug, Clone)]
pub(crate) enum OverlayContent {
    /// Kebab / right-click menu on a host card, BY ID. The list can
    /// re-sort while the menu is up (an auto-save rename, a sync
    /// apply), and the menu's items are rebuilt from a position on
    /// every render: a stored index would silently re-aim Duplicate,
    /// Connect and Remove at whatever host moved into that slot.
    HostActions(uuid::Uuid),
    /// Right-click / Menu-key menu on a host row of the sidebar Hosts
    /// tree (issue #102): the card menu's ACTION half. No Remove and
    /// no filter-by-profile: the tree is a navigate-and-connect
    /// surface, destruction and dashboard filters stay on the
    /// dashboard (owner call).
    TreeHostActions(uuid::Uuid),
    /// Kebab / right-click menu on a session-group card. Items: Open, Edit,
    /// Duplicate, Delete.
    SessionGroupActions(usize),
    KeyActions(usize),
    IdentityActions(usize),
    /// Kebab menu on a snippet card. Items: Edit and Delete.
    SnippetActions(usize),
    /// Kebab menu on a port-forward rule card. Items: Edit and Delete.
    PortForwardActions(usize),
    KeychainAdd,
    TabActions(usize),
    /// Right-click menu on an SFTP browser tab. Items: New SFTP tab,
    /// Pin/Unpin, Close. `usize` is the `sftp_tabs` index.
    SftpTabActions(usize),
    /// Right-click menu on a sidebar Files row. Carries the entry's
    /// full remote path + kind; items: Open (dirs), Open SFTP session
    /// here, Copy path, Copy name (files).
    SidebarFilesRow { path: String, is_dir: bool },
    /// Right-click on the sidebar Files list's empty area: directory
    /// actions for the current folder.
    SidebarFilesBackground { dir: String },
    /// Hover popover under the `+` tab button: New Tab + Split actions for
    /// the active terminal tab, plus the reopen when a tab has been
    /// closed (issue #186).
    SplitMenu,
    /// Right-click on the tab strip's own empty area (issue #186): the
    /// strip's actions rather than a chip's, the way every browser
    /// answers a right-click next to the tabs. Deliberately holds
    /// nothing destructive: that area is also the window-drag handle.
    TabBarActions,
    FolderActions(Uuid),
    /// Dropdown next to the Snippets sort button: multi-select
    /// snippet-tag filter, mirroring `HostTagFilter`.
    SnippetTagFilter,
    /// Kebab menu on a History session row: Export .cast, Export
    /// transcript, Delete. `usize` is the `session_logs` index.
    SessionLogActions(usize),
    /// Kebab menu of a saved AI conversation row, by index into
    /// `Oryxis::chat_conversations`.
    ChatConversationActions(usize),
    /// Same session-log actions, opened from the viewer's header `...`
    /// button: the viewer has a dedicated Play button, so this variant
    /// renders the menu without the Play row.
    SessionLogViewerActions(usize),
    /// Dropdown next to the dashboard sort button: pick a host tag to
    /// filter the grid by (or clear the filter).
    HostTagFilter,
    /// Dropdown under the History-toolbar tag button: multi-select
    /// host-tag filter over the timeline rows, mirroring
    /// `HostTagFilter`.
    HistoryTagFilter,
    /// Shared group-picker popover for side-panel Parent Group
    /// inputs. The target enum tells the dispatch which form field
    /// the picked value flows into so the same overlay machinery
    /// (search + list) serves both the host editor and the dynamic
    /// group editor without duplicate state.
    GroupPicker(GroupPickerTarget),
    /// Sort dropdown anchored to the toolbar sort button in one of
    /// the card-grid views (Hosts / Keychain / Snippets).
    SortMenu(SortMenuKind),
    /// Floating search field popped from the toolbar's search icon when
    /// the window is too narrow for an inline search box. Carries no
    /// payload: the field (id, value, on_input) is resolved from the
    /// active view, exactly like the inline `vault_search_field`.
    ToolbarSearch,
    /// Overflow `…` menu folding the active view's secondary toolbar
    /// actions (sort, view toggle, history pagination) when even the
    /// icon-collapsed search can't free enough room for them inline.
    ToolbarOverflow,
    /// Right-click context menu over a terminal pane (right-click scheme
    /// = Menu). Items: Copy (when a selection was live), Copy All, Paste,
    /// Clear Scrollback. Carries the pane id (so actions target the
    /// clicked pane) and the selection text captured by the widget at
    /// right-click (the app can't reach the widget's live selection).
    /// Position lives in `OverlayState.x/y` (window-absolute).
    TerminalContextMenu(Uuid, Option<String>),
    /// Right-click context menu over the session-log transcript viewer
    /// (issue #90, right-click scheme = Menu). Read-only, so the only
    /// items are Copy (when a selection was live) and Copy All; there is
    /// a single viewer at a time, so no id is carried. Selection text is
    /// captured by the widget at right-click. Position lives in
    /// `OverlayState.x/y` (window-absolute).
    SessionLogViewerContext(Option<String>),
    /// Kebab menu on a Plugins-panel row. Carries the provider id.
    /// Items depend on the row's status: check for updates, the
    /// auto-update override toggle, uninstall / remove downloads.
    PluginActions(String),
    /// Right-click menu on a Monitor-tab listening-port row (issue
    /// #96). Items: Forward this port locally (TCP only, since SSH has
    /// no UDP forwarding), Kill process, Force kill. Carries the whole
    /// socket row so the confirmation describes the socket that was
    /// pointed at, not whatever a later sample holds.
    MonitorPortActions(Box<crate::monitor::model::PortStat>),
    /// Credential suggestions for a password prompt the pane is
    /// blocking on (issue #117). Anchored at the terminal caret rather
    /// than at a widget, and non-modal: it never takes a key the user
    /// did not aim at it, and it never sends anything on its own.
    ///
    /// The entries carry only WHERE each credential lives, never the
    /// credential: the decrypt happens on the pick, like every other
    /// secret read in the app.
    PasswordSuggest {
        pane_id: Uuid,
        /// Resolved once, when the popup opened. Re-resolving per frame
        /// would let the list shift under a selection index.
        entries: Vec<PasswordSource>,
        /// `None` until the user engages with Down: an unengaged popup
        /// is a hint, and Enter must still reach the prompt.
        ///
        /// KEYBOARD-only, never set by hover: Enter picks whatever is
        /// selected, so a hover-set selection would turn "mouse brushed
        /// the popup, then Enter aimed at the prompt" into sending a
        /// secret nobody picked. Mouse users click; the row carries its
        /// own index.
        selected: Option<usize>,
        /// Window-space top edge of the caret's CELL (`ime_caret_rect`
        /// is the whole cell box, so this clears the prompt's glyphs,
        /// not just the bar drawn over them). `x`/`y` place the box
        /// BELOW the caret; this is the edge it flips over when the box
        /// does not fit there, which at a shell prompt (last row of the
        /// terminal) is the ordinary case.
        caret_top: f32,
        /// Scroll offset of the row list, which only exists once the
        /// list overflows the popup's cap. Fed by the scrollable's own
        /// `on_scroll` AND written optimistically by keyboard
        /// navigation, so a burst of arrow presses before the next
        /// event arrives still computes against a fresh position.
        scroll: f32,
    },
}

/// One offerable credential: what to show, and where to read it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PasswordSource {
    /// Row title (the host label or the identity name).
    pub label: String,
    /// Row subtitle (the username, when there is one).
    pub sublabel: String,
    pub kind: PasswordSourceKind,
}

/// Which vault row a [`PasswordSource`] decrypts from on pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasswordSourceKind {
    /// `connections.password` of a saved host.
    Connection(Uuid),
    /// `identities.password` of a saved identity.
    Identity(Uuid),
}

/// Which side-panel input the shared group picker is currently
/// driving. Each panel carries its own combo bounds cell so the
/// popover anchors precisely under the right chevron.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupPickerTarget {
    SessionGroupFolder,
    /// Parent combo in the manual group editor side panel. The list
    /// excludes the edited group's own subtree (no cycles).
    GroupEditParent,
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
