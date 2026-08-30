//! Unified keyboard navigation ("focus zones") for the vault area.
//!
//! The vault surface is modeled as four zones cycled with Tab /
//! Shift+Tab: Search (iced's real text_input focus), the sub-nav
//! pills (or the vertical rail), the per-view toolbar cluster and
//! the content grid/list. Arrow keys move within a zone, Enter
//! activates, Esc returns to idle.
//!
//! Search is "zone zero" and is represented by `focus == None`: iced
//! gives no way to observe a text_input's focus from app state, so
//! idle and search-focused are deliberately the same state (the same
//! trick the old dashboard-only model used). Entering the search zone
//! focuses the view's search input; leaving it blurs via the
//! nonexistent-id trick (`"__keynav_blur__"`).
//!
//! Views record their navigable items into `KeyNavState`'s RefCells
//! during `view()` (render order, post filter/sort), so the key
//! router always moves across exactly what is on screen. Items are
//! semantic ids (not positions): a re-render can't strand the
//! selection on the wrong element because each keypress re-finds the
//! item in the freshly recorded lists. The router lives in
//! `dispatch_keynav.rs`; this module holds the types and the pure
//! movement math (`movement.rs`).

pub(crate) mod movement;
pub(crate) mod slots;
#[cfg(test)]
mod tests;

use std::cell::RefCell;

pub(crate) use slots::{ModalNavState, ModalSurface, RowAction, SidebarRow, SETTINGS_SCROLL_TARGET_ID};

/// The vault-area focus zones, in Tab-cycle order. Search is "zone
/// zero" and is represented by `KeyNavState::focus == None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusZone {
    SubNav,
    Toolbar,
    Content,
}

/// A keyboard-navigable item on the dashboard. Groups (host folders +
/// session groups) come first, then hosts, mirroring the on-screen
/// order. Enter opens a group / connects a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DashNavItem {
    /// Host folder; Enter → `OpenGroup`.
    Group(uuid::Uuid),
    /// Saved session group (index into `session_groups`); Enter → `OpenSessionGroup`.
    SessionGroup(usize),
    /// Host (index into `connections`); Enter → `ConnectSsh`.
    Host(usize),
}

/// Position-independent ids for the per-view toolbar action cluster.
/// Views record only the buttons they actually render, so the folded
/// (narrow-window) state naturally exposes just the remnants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolbarItem {
    /// Grid/list layout toggle (dashboard only).
    ViewToggle,
    /// Pause / resume the fleet polling (Monitoring only).
    MonitorPause,
    /// One sample from every machine on the board (Monitoring only).
    MonitorRefresh,
    /// Host tag-filter dropdown trigger (dashboard only).
    TagFilter,
    /// Sort-menu trigger.
    Sort,
    /// The primary action button ("+ HOST", "+ ADD", "Clear all", ...).
    Primary,
    /// History pager: previous page.
    PagerPrev,
    /// History pager: next page.
    PagerNext,
    /// Privacy-mode reveal eye (History / Known Hosts).
    PrivacyReveal,
    /// The "⋯" button when the cluster is folded at narrow widths.
    Overflow,
    /// The search icon when the search field is collapsed.
    SearchIcon,
    /// The "search in session content" toggle chip inside the History
    /// search field (rendered whenever that field is on screen, inline
    /// or in the collapsed-search overlay).
    SearchContent,
}

/// A keyboard-selectable item, recorded by the views during render.
/// Semantic ids, not positions, so a re-render (filter change, resize
/// re-chunking) can't strand the selection on the wrong element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavItem {
    /// Sub-nav zone: one pill / rail entry per vault view.
    SubNav(crate::state::View),
    /// Toolbar zone.
    Toolbar(ToolbarItem),
    /// Hosts grid (groups + session groups + hosts).
    Dash(DashNavItem),
    /// Keychain: index into the sorted/filtered keys as rendered.
    Key(usize),
    /// Keychain: index into the identities as rendered.
    Identity(usize),
    /// Snippet card (vault index).
    Snippet(usize),
    /// Snippet group folder card (index into
    /// `Oryxis::snippet_group_names()`).
    SnippetGroup(usize),
    /// Port-forward rule card (vault index).
    PortForward(usize),
    /// History row (session-log id, current page only).
    HistoryLog(uuid::Uuid),
    /// Proxy-identity row.
    Proxy(uuid::Uuid),
    /// Known-host row (index into the rendered list).
    KnownHost(usize),
    /// Settings: one sidebar section entry (the SubNav zone there).
    SettingsSection(crate::state::SettingsSection),
    /// Settings: one actionable content row, index into the
    /// per-frame `settings_row_actions` recording (rows are long and
    /// heterogeneous; index + clamp beats a giant semantic enum).
    SettingsRow(usize),
    /// Generic content row carrying its own recorded `RowAction`
    /// (index into `content_actions`). Used by content surfaces whose
    /// rows fire arbitrary messages; same index+clamp model as
    /// `SettingsRow`.
    ContentAction(usize),
}

/// Grouped keyboard-navigation state, one field on `Oryxis`.
///
/// The item lists are RefCells because they are recorded during
/// `view()` (which takes `&self`), mirroring the old `dashboard_nav`
/// pattern. Each owning builder clears its list at the top of its
/// render pass so items from a previous view never linger.
#[derive(Default)]
pub(crate) struct KeyNavState {
    /// Active zone + selected item. `None` = idle, which also covers
    /// "the search field holds iced's real focus" (indistinguishable
    /// from app state, by design).
    pub(crate) focus: Option<(FocusZone, NavItem)>,
    /// Sub-nav destinations in full logical order (inline + overflow).
    pub(crate) subnav_items: RefCell<Vec<NavItem>>,
    /// Toolbar cluster items actually rendered, in render order.
    pub(crate) toolbar_items: RefCell<Vec<NavItem>>,
    /// Content as visual rows: chunked to the column count for card
    /// grids, one item per row for 1-D lists.
    pub(crate) content_rows: RefCell<Vec<Vec<NavItem>>>,
    /// Row indices where each content SECTION starts (Groups then
    /// Hosts on the dashboard, Keys then Identities on the keychain).
    /// Tab steps between sections before leaving the content zone;
    /// arrows still move continuously across all rows. Single-section
    /// views record `[0]`.
    pub(crate) content_section_starts: RefCell<Vec<usize>>,
    /// Set by keynav's own ChangeView dispatches (SubNav Enter,
    /// Ctrl+PageUp/Down section cycling) so the ChangeView handler
    /// keeps the zone focus instead of clearing it.
    pub(crate) keep_focus_through_change_view: bool,
    /// Modal / overlay-menu layer (iteration 2): index-based row
    /// selection over per-frame recorded `RowAction`s. See `slots.rs`.
    pub(crate) modal: ModalNavState,
    /// Settings content: the `RowAction` behind each recorded
    /// `NavItem::SettingsRow`, parallel to `content_rows`.
    pub(crate) settings_row_actions: RefCell<Vec<RowAction>>,
    /// Settings search: parallel to `settings_row_actions`, `true` for
    /// each row whose label matches the active sidebar-search query.
    /// A matched row draws the persistent amber highlight (JetBrains
    /// style) in `settings_nav_ring_at`. Recorded per-frame.
    pub(crate) settings_row_highlight: RefCell<Vec<bool>>,
    /// Settings search: the visible labels of the rows that match the
    /// active query IN THE ACTIVE SECTION, filled by the sidebar
    /// render (which runs before the content). The content helpers
    /// test their label against this to decide the highlight.
    pub(crate) settings_match_labels: RefCell<Vec<&'static str>>,
    /// The visible label of the ACTIVE match (find-next cursor),
    /// filled by the sidebar render from `settings_active_match`. The
    /// content row whose label equals this gets the accent "current"
    /// ring + becomes the scroll-into-view anchor. `None` outside search.
    pub(crate) settings_active_label: std::cell::Cell<Option<&'static str>>,
    /// Index into `settings_row_actions` of the active-match row this
    /// frame, so `settings_nav_ring_at` marks exactly that row (with
    /// `report_container_id`) as the scroll-into-view anchor.
    pub(crate) settings_first_match_idx: std::cell::Cell<Option<usize>>,
    /// Generic content actions: the `RowAction` behind each recorded
    /// `NavItem::ContentAction`.
    pub(crate) content_actions: RefCell<Vec<RowAction>>,
    /// Side-panel row mode (host editor first): selected row index
    /// into `panel_items`, or `None` while iced's input focus owns
    /// the keyboard ("form mode"). The XOR invariant: entering row
    /// mode blurs, entering an input clears the selection.
    pub(crate) panel_selected: Option<usize>,
    /// Actionable panel rows recorded during view(), render order.
    pub(crate) panel_items: RefCell<Vec<RowAction>>,
    /// Last row-mode position, restored when Up/Down re-enter row
    /// mode after an input excursion (clamped against the fresh
    /// recording).
    pub(crate) panel_last_row: std::cell::Cell<Option<usize>>,
    /// On-screen rect of the currently ringed content card, written
    /// every draw by `keynav_ring_content` (a `bounds_reporter`
    /// wrap). Lets the Menu key anchor the context menu at the card
    /// instead of wherever the mouse happens to be.
    pub(crate) ring_bounds: crate::widgets::BoundsCell,
    /// One-shot anchor override for the next kebab-menu open, set by
    /// the Menu key from `ring_bounds` and consumed (take) by the
    /// Show*Menu handlers; mouse opens are unaffected (None).
    pub(crate) menu_anchor: Option<(f32, f32)>,
    /// True while a pick_list dropdown is open (fed by the widgets'
    /// on_open / on_close). The global key subscription still sees
    /// every key the focused pick_list handles itself, so this flag
    /// gates the app-side routers: while a dropdown is open,
    /// Enter/Space/Esc/Up/Down belong to the widget alone (Esc must
    /// close the dropdown, not the panel behind it).
    pub(crate) pick_open: bool,
    /// Terminal-sidebar list layer (iteration 3): ring over the
    /// Snippets / History rows, tagged by the sidebar tab that owns
    /// it so a tab switch drops the selection for free. The tab also
    /// names the REGION (its dock side), so one selection can never
    /// straddle the two regions (issue #102). Engaged by the
    /// FocusSidebarList hotkey or by Up/Down while the cursor is
    /// over a region; Esc disengages and gives the keyboard back
    /// to the terminal.
    pub(crate) sidebar_selected: Option<(crate::state::TerminalSidebarTab, usize)>,
    /// Actionable sidebar list rows recorded during view(), display
    /// order (History: frequent shortlist first, then recents), one
    /// list per region indexed by `SidebarSide::idx()` (issue #102:
    /// both regions can render in the same frame, and a shared list
    /// would interleave their indices).
    pub(crate) sidebar_items: [RefCell<Vec<SidebarRow>>; 2],
}

/// Scrollable id for a sidebar tab's list body. Per TAB, because the
/// two regions can each mount a list in the same frame (issue #102)
/// and duplicate scrollable ids break `snap_to`.
pub(crate) fn sidebar_scroll_id(tab: crate::state::TerminalSidebarTab) -> iced::widget::Id {
    iced::widget::Id::from(format!("sidebar-list-scroll-{}", tab.code()))
}

impl KeyNavState {
    /// The selected item when `zone` is the active zone. Views use
    /// this to decide which element gets the selection ring.
    pub(crate) fn selected_in(&self, zone: FocusZone) -> Option<NavItem> {
        match self.focus {
            Some((z, item)) if z == zone => Some(item),
            _ => None,
        }
    }
}

impl crate::app::Oryxis {
    /// Clear the recorded toolbar items. Each view's toolbar builder
    /// calls this once at the top of its render pass, then records
    /// its buttons through `keynav_toolbar_slot` in render order.
    pub(crate) fn keynav_toolbar_reset(&self) {
        self.keynav.toolbar_items.borrow_mut().clear();
    }

    /// Zero the toolbar trigger-bounds cells. Called by the toolbar
    /// builders on their `…` overflow branch (those triggers are not on
    /// screen there) and by handlers that change the toolbar's layout
    /// in the same update that anchors a menu (closing a side panel
    /// shifts every button by the panel width before the next draw):
    /// a stale rect would mis-anchor `toolbar_menu_anchor`, while an
    /// empty cell falls back to the trailing-edge estimate. Not done
    /// on every build: cells refresh on draw, and zeroing at build
    /// would blank them for the whole frame on renderers that only
    /// draw on demand.
    pub(crate) fn keynav_toolbar_zero_trigger_bounds(&self) {
        let zero = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::ZERO);
        self.toolbar_split_btn_bounds.set(zero);
        self.toolbar_sort_btn_bounds.set(zero);
        // The tag-filter dropdowns anchor on these the same way (the
        // pre-existing bounds cells); a folded or shifted toolbar must
        // not leave them stale either. Their anchor handlers fall back
        // to the cursor position on an empty cell.
        self.host_tag_filter_btn_bounds.set(zero);
        self.snippet_tag_filter_btn_bounds.set(zero);
    }

    /// Record a single-section content zone (every view except the
    /// dashboard and the keychain). Rows are visual: chunked for card
    /// grids, one item per row for 1-D lists.
    pub(crate) fn keynav_set_content_rows(&self, rows: Vec<Vec<NavItem>>) {
        *self.keynav.content_section_starts.borrow_mut() = vec![0];
        *self.keynav.content_rows.borrow_mut() = rows;
    }

    /// Record a multi-section content zone (dashboard Groups/Hosts,
    /// keychain Keys/Identities). Empty sections are dropped so Tab
    /// never lands on a heading with nothing under it.
    pub(crate) fn keynav_set_content_sections(&self, sections: Vec<Vec<Vec<NavItem>>>) {
        let mut rows: Vec<Vec<NavItem>> = Vec::new();
        let mut starts: Vec<usize> = Vec::new();
        for section in sections {
            let non_empty: Vec<Vec<NavItem>> =
                section.into_iter().filter(|r| !r.is_empty()).collect();
            if non_empty.is_empty() {
                continue;
            }
            starts.push(rows.len());
            rows.extend(non_empty);
        }
        if starts.is_empty() {
            starts.push(0);
        }
        *self.keynav.content_section_starts.borrow_mut() = starts;
        *self.keynav.content_rows.borrow_mut() = rows;
    }

    /// Clear the content zone (empty states, surfaces that aren't
    /// keyboard-navigable yet).
    pub(crate) fn keynav_clear_content(&self) {
        self.keynav.content_rows.borrow_mut().clear();
        self.keynav.content_section_starts.borrow_mut().clear();
        self.keynav.content_actions.borrow_mut().clear();
    }

    /// Record one rendered toolbar action for the keyboard router and
    /// wrap it in the focus ring when it is the selected item. Views
    /// call this only for the buttons they actually build, so the
    /// folded (narrow-window) toolbar naturally exposes just the
    /// remnants ("…" + search icon). Toolbars whose build order does
    /// not match the visual order use the split `keynav_toolbar_ring`
    /// + `keynav_toolbar_record` halves instead.
    pub(crate) fn keynav_toolbar_slot<'a>(
        &self,
        item: ToolbarItem,
        el: iced::Element<'a, crate::app::Message>,
    ) -> iced::Element<'a, crate::app::Message> {
        self.keynav_toolbar_record(item);
        self.keynav_toolbar_ring(item, el)
    }

    /// Recording half of `keynav_toolbar_slot`: append to the
    /// keyboard order without touching the element. Call in VISUAL
    /// order (logical leading-to-trailing; the router mirrors arrows
    /// under RTL).
    pub(crate) fn keynav_toolbar_record(&self, item: ToolbarItem) {
        self.keynav.toolbar_items.borrow_mut().push(NavItem::Toolbar(item));
    }

    /// Ring half of `keynav_toolbar_slot`: wrap `el` in the focus
    /// ring when it is the selected toolbar item. Safe to call on
    /// elements that end up unrecorded (they can never be selected).
    pub(crate) fn keynav_toolbar_ring<'a>(
        &self,
        item: ToolbarItem,
        el: iced::Element<'a, crate::app::Message>,
    ) -> iced::Element<'a, crate::app::Message> {
        // Always wrapped (transparent when unringed): see
        // select_ring_opt for why the wrapper must be shape-stable.
        // 6px matches the shared 24px toolbar-button radius. Contrast
        // color, not accent: most toolbar buttons are accent-filled,
        // an accent ring vanishes into them.
        let ringed = self.keynav.focus == Some((FocusZone::Toolbar, NavItem::Toolbar(item)));
        crate::widgets::select_ring_opt(
            el,
            6.0,
            ringed.then(|| crate::theme::OryxisColors::t().text_primary),
        )
    }
}
