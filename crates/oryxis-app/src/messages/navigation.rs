//! Vault navigation + dashboard chrome: view switch, group open, host search, tag filters, group picker, sort menus, toolbar collapse, wrapped by [`crate::messages::Message::Navigation`]. Handled by `Oryxis::handle_navigation`.

use uuid::Uuid;
use crate::state::{View};

#[derive(Debug, Clone)]
pub enum NavigationMessage {
    ChangeView(View),
    /// The Home tab (its house chip and the strip slot behind it).
    /// Leaves the current view like any `ChangeView(Dashboard)`, then
    /// re-opens the group the user was standing in, so Home comes back
    /// to the folder instead of the root. Resolved at click time, not
    /// render time, because the folder is only known then.
    GoHome,
    QuickHostInput(String),
    QuickHostContinue,
    OpenGroup(Uuid),
    HostSearchChanged(String),
    /// Continuation of a side-panel Tab press: `focused` is the widget
    /// iced actually has focused (resolved via `find_focused`), so the
    /// ring index can sync to a mouse-clicked field before walking to the
    /// next row. `None` = nothing focused (ring authoritative).
    PanelNavTabResolved {
        forward: bool,
        focused: Option<iced::widget::Id>,
    },
    /// Continuation of a Settings-content Tab / arrow press with no
    /// keynav ring active: `focused` is the widget iced actually has
    /// focused (resolved via `find_focused`), so a mouse-focused field
    /// (the export / import password, the sync passphrase) can walk the
    /// recorded rows on Tab instead of the vault-area router parking
    /// the ring on the first content row and scrolling the page away;
    /// arrows / Home / End stay with the field's own caret. `None` =
    /// nothing focused (fall back to the normal router).
    SettingsKeyResolved {
        /// The intercepted named key (Tab or a movement key).
        named: iced::keyboard::key::Named,
        /// Shift held: Shift+Tab walks the rows backward.
        shift: bool,
        focused: Option<iced::widget::Id>,
    },
    /// Continuation of a vault-area Up/Down press with no keynav ring
    /// active: `focused` is the widget iced actually has focused
    /// (resolved via `find_focused`). A focused text input that is NOT
    /// the view's search field (the empty dashboard's quick-host
    /// input) keeps the key for its own caret; the search field and
    /// the unfocused state keep today's enter-the-content-zone
    /// behavior. Issue #168: numpad typing with NumLock off delivers
    /// arrows mid-word, and the unresolved router blurred the field,
    /// scrolled the list and silently ate every following digit.
    VaultNavKeyResolved {
        named: iced::keyboard::key::Named,
        focused: Option<iced::widget::Id>,
    },
    /// Dashboard: open/close the host tag-filter dropdown.
    ShowHostTagFilterMenu,
    /// Dashboard: toggle one tag in the multi-select filter (the
    /// dropdown stays open so several can be picked in one visit).
    ToggleHostTagFilterTag(String),
    /// Dashboard: clear the tag filter entirely.
    ClearHostTagFilter,
    ToggleSortMenu(crate::state::SortMenuKind),
    SetListSort(crate::state::SortMenuKind, crate::state::ListSort),
    ToggleToolbarSearch,
    ToggleToolbarOverflow,
    ModalNavHover(usize),
    PickOpenChanged(bool),
    /// Open / close the shared group picker for a side-panel parent
    /// group input. Anchors the popover at the matching combo's
    /// measured bounds (
    /// `session_group_folder_combo_bounds`).
    ToggleGroupPicker(crate::state::GroupPickerTarget),
    /// Live filter for the shared group-picker popover.
    GroupPickerSearchChanged(String),
    /// Route a pick into the matching form field and close the
    /// popover. Existing field-change messages (`EditorGroupChanged`)
    /// still drive the write.
    GroupPickerPick(crate::state::GroupPickerTarget, String),
}
