//! Overlay (floating context menus) (split out of `state.rs`).

use super::*;

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
    /// Floating search field popped from the toolbar's search icon when
    /// the window is too narrow for an inline search box. Carries no
    /// payload: the field (id, value, on_input) is resolved from the
    /// active view, exactly like the inline `vault_search_field`.
    ToolbarSearch,
    /// Overflow `…` menu folding the active view's secondary toolbar
    /// actions (sort, view toggle, history pagination) when even the
    /// icon-collapsed search can't free enough room for them inline.
    ToolbarOverflow,
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
