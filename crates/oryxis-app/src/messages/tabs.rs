//! Tabs, tab strip, tab menu, folders, icon picker, command palette, new-tab picker, window controls.

use iced::Point;
use uuid::Uuid;
use super::Message;

#[derive(Debug, Clone)]
pub enum TabsMessage {
    SelectTab(usize),
    CloseTab(usize),
    /// The strip's own close X, as opposed to `Ctrl+W`, the tab context
    /// menu or the terminal's close handling. Delegates to `CloseTab`
    /// immediately, and exists only to start the close streak that arms
    /// the next chip on arrival: after a close the strip slides the
    /// following tab under a cursor that never moved, so a mouse close
    /// is the one path where revealing the next X at once is what the
    /// user asked for.
    CloseTabFromStrip(usize),
    /// Second step of closing a GROUPED tab: the confirmation said yes,
    /// so tear it down without asking again (issue #112).
    ConfirmCloseGroupedTab(usize),
    /// Bring back the last closed tab (issue #186), terminal or SFTP.
    /// Pops `Oryxis::closed_tabs` and reopens it through the same spec
    /// resolution a dormant pinned tab uses.
    ReopenClosedTab,
    TabHovered(usize),
    /// A chip's hover dwell expired: reveal its close X, unless the
    /// pointer has moved on since. Carries the hover episode
    /// (`HoverState::tab_hover_seq`) the timer was started with, which is
    /// what tells "still resting here" from "already two tabs away".
    TabCloseDwell(u64),
    /// Cursor left the tab at this index. It carries the index because the
    /// next tab's enter can arrive FIRST (see `HoverState::leave_tab`), so
    /// the clear has to know whose exit it is.
    TabUnhovered(usize),
    /// Cursor entered the trailing drop zone (the `+` button area) during an
    /// active tab-reorder drag: slide the dragged tab to the end of its
    /// partition, the one slot the live-slide can't otherwise reach.
    TabDragToEnd,
    ShowNewTabPicker,
    HideNewTabPicker,
    NewTabPickerSearchChanged(String),
    /// Enter pressed in the picker search: quick-connect when the input
    /// parses as `user@host[:port]`, otherwise a no-op.
    NewTabPickerSubmit,
    /// Drill into a group in the new-tab picker. For a cloud-query group
    /// this also kicks off (or refreshes) the resolve so the ECS tasks /
    /// K8s pods load. `Uuid` is the group id.
    NewTabPickerOpenGroup(Uuid),
    /// Step back out of a drilled-into group to the top-level picker list.
    NewTabPickerBack,
    ShowTabJump,
    HideTabJump,
    TabJumpSearchChanged(String),
    /// Translate a vertical mouse-wheel delta over the tab bar into a
    /// horizontal scroll on the tab strip. Carries the y-pixel delta;
    /// sign flips for natural-feeling navigation (wheel-down moves
    /// later tabs into view).
    TabBarWheel(f32),
    /// Two-step dispatch: close the modal first, then fire the inner
    /// message (SelectTab, OpenLocalShell, etc). Boxed to keep the enum
    /// variant size from blowing up.
    TabJumpSelect(Box<Message>),
    /// Open the command palette (`Ctrl+Shift+P`): resets the query and
    /// focuses the search input. Refused while the vault is locked.
    ShowCommandPalette,
    HideCommandPalette,
    PaletteQueryChanged(String),
    /// Two-step dispatch like `TabJumpSelect`: close the palette, then
    /// fire the row's real message (carried, not re-derived by index).
    PaletteActivate(Box<Message>),
    /// Replay a hotkey action from a palette row (reuses the per-action
    /// context gating in `dispatch_hotkey_action`).
    RunHotkeyAction(crate::hotkeys::HotkeyAction),
    /// Navigate to a Settings section from anywhere: switches to the
    /// Settings view AND selects the section (`ChangeSettingsSection`
    /// alone only sets the section, assuming the view is already open).
    OpenSettingsSection(crate::state::SettingsSection),
    ShowIconPicker(Uuid),
    HideIconPicker,
    IconPickerSelectIcon(String),
    IconPickerSelectColor(String),
    IconPickerHexInputChanged(String),
    IconPickerIconSearchChanged(String),
    /// Open the HSV color popover, anchored at the current cursor.
    IconPickerOpenColorPopover,
    /// Dismiss the HSV color popover (click outside / pick done).
    IconPickerCloseColorPopover,
    IconPickerSave,
    IconPickerResetAuto,
    ShowTabMenu(usize),
    /// Right-click landed on the tab strip itself rather than on a chip
    /// (issue #186): open the strip's own menu at the cursor. A chip
    /// captures its right press, so this only ever fires from the empty
    /// area the strip also drags the window by.
    ShowTabBarMenu,
    ReconnectTab(usize),
    DuplicateTab(usize),
    DuplicateInNewWindow(usize),
    /// Pin / unpin a tab (from its context menu). Pinned tabs render first
    /// and are restored on the next launch.
    ToggleTabPin(usize),
    /// Open the rename dialog for a terminal tab (from its context menu).
    /// The name is transient: it lives for the tab's lifetime only and is
    /// never written back to the host or the pin spec.
    StartRenameTab(usize),
    /// Open the rename dialog for an SFTP tab (same transient semantics).
    StartRenameSftpTab(usize),
    TabRenameInput(String),
    /// Commit the rename dialog. An empty (or whitespace-only) name clears
    /// the custom name, restoring the automatic label.
    ConfirmTabRename,
    CancelTabRename,
    /// Hybrid tab (issue #61): flip the terminal tab at this index
    /// between its Terminal and Files-full (dual-pane SFTP) states.
    /// Fired by the tab context menu and the hotkey. The mode glyph and
    /// the status-bar segments send `ShowTabSurface` instead: with an
    /// SFTP console in the tab there are three surfaces, and a toggle
    /// cannot name which one it means.
    ToggleTabFilesMode(usize),
    /// Show one of the terminal tab's surfaces (Terminal / SFTP console
    /// / Files). One message across two mechanisms: Files is a tab-level
    /// mode, the other two are panes of the grid.
    ShowTabSurface(usize, crate::state::TabSurface),
    /// Promote the terminal tab's SFTP session to a standalone SFTP tab
    /// (the server-to-server surface); the hybrid state moves out.
    DetachTabSftp(usize),
    /// Close ONLY the terminal tab's SFTP session (back to a plain
    /// terminal tab): drops the browsing state + channel, the mode
    /// glyph disappears. The terminal keeps running.
    CloseTabSftpSession(usize),
    /// From an SFTP tab's context menu: focus a live terminal tab on
    /// the mounted host, or connect one.
    OpenTerminalForSftpTab(usize),
    /// Copy the focused pane's host address (`Connection.hostname`, so a
    /// DNS name or a literal IP, whichever the host is saved with) to the
    /// clipboard. From the tab context menu.
    CopyTabAddress(usize),
    ShowFolderActions(Uuid),
    StartRenameFolder(Uuid),
    FolderRenameInput(String),
    ConfirmRenameFolder,
    CancelFolderModal,
    /// Open the manual host-group editor side panel for this group.
    EditGroup(Uuid),
    /// Open the group editor in create mode with this group prefilled
    /// as the parent (folder kebab's "New subgroup").
    NewSubgroup(Uuid),
    /// Open the group editor in create mode at the vault root (no
    /// parent): the toolbar / empty-state "New group" button.
    NewGroup,
    GroupEditLabelChanged(String),
    /// Parent-group combo text (label matching, empty = root).
    GroupEditParentChanged(String),
    /// Open the icon/color picker routed to the group editor.
    ShowGroupEditIconPicker,
    /// Expand / collapse the group editor's Defaults section (D4).
    /// Collapsed by default: most groups are folders, and the
    /// inheritance fields would otherwise dominate a panel whose usual
    /// job is renaming.
    GroupEditToggleDefaults,
    /// Group default: login user hosts inherit when they name none.
    GroupEditDefaultUsername(String),
    /// Group default: port a NEW host in this group is created with.
    /// Never applied to hosts that already exist.
    GroupEditDefaultPort(String),
    /// Group defaults picked from a list: the label, or `None` for the
    /// explicit "not set" row that means "keep inheriting".
    GroupEditDefaultIdentity(Option<String>),
    GroupEditDefaultProxyIdentity(Option<String>),
    GroupEditDefaultTheme(Option<String>),
    GroupEditDefaultSnippet(Option<String>),
    /// Group default environment variables, merged by name with the
    /// host's and the other ancestors'.
    GroupEditEnvAdd,
    GroupEditEnvRemove(usize),
    GroupEditEnvKey(usize, String),
    GroupEditEnvValue(usize, String),
    SaveGroupEdit,
    CancelGroupEdit,
    StartDeleteFolder(Uuid),
    DeleteFolderKeepHosts,
    DeleteFolderWithHosts,
    CloseOtherTabs(usize),
    CloseAllTabs,
    MouseMoved(Point),
    /// A drag-out's payload finished preparing (issue #167): remote
    /// handles opened, runtime captured. It waits in the armed gesture
    /// until the cursor leaves the window, which is when the OS drag
    /// starts. Lives in the window domain rather than either browser's:
    /// both file surfaces raise it, and what escalates it is a window
    /// geometry test, not anything either browser owns.
    DragOutReady(Result<crate::drag_out::Prepared, String>),
    WindowResized(iced::Size),
    /// OS window moved; carries the new outer position in logical
    /// desktop coordinates (negative on monitors left of / above the
    /// primary). Feeds the persisted window geometry so the next launch
    /// reopens on the same monitor at the same spot.
    WindowMoved(Point),
    /// Post-boot sanity check for the restored window position: if the
    /// saved coordinates landed on a monitor that is no longer there,
    /// move the window back onto the current monitor.
    WindowEnsureOnScreen,
    /// OS window gained (`true`) or lost (`false`) focus. Gates the
    /// cloud SSM/ECS keepalive ticker: it only runs while unfocused.
    WindowFocusChanged(bool),
    /// Periodic tick (mounted only while the window is unfocused and at
    /// least one SSM/ECS tab is open) that nudges those tabs' terminal
    /// size so the SSM idle timer resets and a long alt-tab away doesn't
    /// drop the session.
    SsmKeepaliveTick,
    /// Animation tick for the strip's running-command indicator (issue
    /// #146). Mounted only while some pane has a command in flight
    /// (smart tabs on): fast while one is past the long-command
    /// threshold (it drives the marching dots), slow below it (it only
    /// has to catch the crossing).
    BusyAnimTick,
    /// Cursor entered / left a panel tab (Settings, network tools).
    /// Drives the hover-revealed close X, and the flag the press
    /// handler reads to arm a reorder drag, exactly like `TabHovered`
    /// does for a session tab.
    PanelTabHovered(crate::state::PanelKind),
    PanelTabUnhovered(crate::state::PanelKind),
    /// Close a panel tab (issue #120). Selecting one goes through
    /// `NavigationMessage::ChangeView(kind.view())` instead, so there is
    /// no matching Select variant.
    ClosePanelTab(crate::state::PanelKind),
    WindowDrag,
    WindowResizeDrag(iced::window::Direction),
    /// Press on the side-panel editor drawer's edge handle: arms a
    /// width-resize drag. `MouseMoved` tracks the cursor against the
    /// press position; the global left-release ends it (and persists
    /// the width, see `ChatSidebarResizeStop`).
    SidePanelResizeStart,
    /// Double-click on a N/S edge, fill the full monitor height while
    /// keeping horizontal position and width.
    WindowExpandVertical,
    WindowMinimize,
    WindowMaximizeToggle,
    /// OS truth about the maximized state, reconciled asynchronously
    /// after a `WindowResized`. Win+Up/Down, aero snap, the taskbar's
    /// Restore, and dragging the custom title bar down all change the
    /// OS state without firing `WindowMaximizeToggle`, so the optimistic
    /// flag would silently go stale and take the edge-resize border and
    /// `WindowResizeDrag` down with it. Carries the snapped size of the
    /// `WindowResized` that triggered the query: `window_windowed_size`
    /// is committed here, once the OS has answered, rather than in the
    /// resize handler where the stale flag could let a monitor-sized
    /// rectangle through as the "windowed" size.
    WindowMaximizedSynced(bool, iced::Size),
    WindowFullscreenToggle,
    /// Clears the "Press F11 to exit fullscreen" banner. Fired by a
    /// timed `Task::perform` 3 s after entering fullscreen.
    FullscreenHintHide,
    WindowClose,
    /// Spawn a fresh top-level Oryxis window without binding to any
    /// existing tab. Triggered by Ctrl+Shift+N and the burger menu's
    /// "New Window" entry. Inherits the vault master password the
    /// same way `DuplicateInNewWindow` does.
    SpawnNewWindow,
    /// Focus the current view's primary search/filter input. Triggered
    /// by Ctrl+F outside the terminal. No-op when the active view has
    /// no search field (Snippets, Settings, History).
    FocusViewSearch,
    /// Activate the Nth slot of the visual tab strip (0-indexed). In
    /// Workspace mode slot 0 is Hosts, slot 1 is SFTP (when enabled),
    /// followed by terminal tabs. In Classic mode the strip only
    /// holds terminal tabs. Out-of-range slots are no-ops.
    ActivateStripSlot(usize),
    HideOverlayMenu,
    CardHovered(usize),
    CardUnhovered(usize),
    FolderCardHovered(Uuid),
    FolderCardUnhovered(uuid::Uuid),
    KeyCardHovered(usize),
    KeyCardUnhovered(usize),
    IdentityCardHovered(usize),
    SnippetCardHovered(usize),
    SnippetCardUnhovered(usize),
    IdentityCardUnhovered(usize),
    ShowCardMenu(usize),
    /// Right-click / Menu-key on a host row of the sidebar Hosts tree
    /// (issue #102): the reduced card menu (no Remove, no dashboard
    /// filter), anchored at the cursor or the ringed row.
    ShowTreeHostMenu(usize),
    #[allow(dead_code)]
    HideCardMenu,
    /// Hover entered the `+` button: reveal the New-Tab / Split popover.
    /// No-op unless a terminal tab is open.
    ShowSplitMenu,
    /// Cursor entered the popover itself (keeps it open across the bridge).
    SplitMenuEnter,
    /// Cursor left the `+` button or the popover: schedule a close.
    SplitMenuLeave,
    /// Delayed close: hide the popover unless the cursor came back to it.
    SplitMenuCloseIfIdle,
    /// Picker "Local Shell" entry. Opens a local shell, into a split pane
    /// when `pending_pane_split` is set, otherwise a new tab.
    PickLocalShell,
    /// Show/hide the top-left burger menu (Settings / Updates / About /
    /// Exit). Mirrors Termius's `☰` strip at the start of the tab bar.
    ToggleBurgerMenu,
    /// Show/hide the vault sub-nav overflow ("…") menu listing the
    /// destinations that didn't fit in the pill strip.
    ToggleSubnavOverflow,
}
