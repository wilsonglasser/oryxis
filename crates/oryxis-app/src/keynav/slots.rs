//! Recording slots for the modal / settings / side-panel keyboard
//! layers (iteration 2 of the focus-zone framework).
//!
//! These surfaces fire arbitrary `Message`s per row and `Message` is
//! `Clone` but not `PartialEq`, so unlike the vault zones (semantic
//! `NavItem` ids) the selection here is INDEX-based: each surface
//! records its actionable rows in render order every frame, and the
//! routers clamp a stale index instead of chasing identity. A
//! `ModalSurface` tag on the selection makes a surface swap (menu
//! closes, another opens) drop the selection for free.

use std::cell::RefCell;

use crate::app::{Message, NavigationMessage};

/// Widget id tagged onto the ACTIVE (find-next cursor) matched settings
/// row so the scroll-into-view operation (`scroll_into_view_task`) can
/// locate it even when it is scrolled off-screen.
pub(crate) const SETTINGS_SCROLL_TARGET_ID: &str = "settings-scroll-target";

/// One keyboard-actionable row/button recorded during view().
///
/// `activate`: dispatched by Enter / Space. `prev` / `next`: fired by
/// Left / Right on picker rows (the on_select message the dropdown
/// would produce for the neighboring option). `focus`: Enter focuses
/// this text input instead of dispatching (row mode hands the
/// keyboard back to iced's real focus).
#[derive(Default, Clone)]
pub(crate) struct RowAction {
    pub(crate) activate: Option<Message>,
    pub(crate) prev: Option<Message>,
    pub(crate) next: Option<Message>,
    pub(crate) focus: Option<iced::widget::Id>,
}

impl RowAction {
    /// A plain button / toggle / menu row.
    pub(crate) fn activate(msg: Message) -> Self {
        Self { activate: Some(msg), ..Default::default() }
    }

    /// A pick_list row: Left/Right cycle, Enter is a consumed no-op.
    pub(crate) fn picker(prev: Option<Message>, next: Option<Message>) -> Self {
        Self { prev, next, ..Default::default() }
    }

    /// A text-input row: Enter focuses the input.
    pub(crate) fn input(id: iced::widget::Id) -> Self {
        Self { focus: Some(id), ..Default::default() }
    }
}

/// One keyboard-actionable terminal-sidebar row. The sidebar walks
/// the same `RowAction` vocabulary as the side panels (activate /
/// picker prev+next / input focus), so buttons, selects, toggles,
/// search fields and the chat editor all join the Tab walk. List
/// rows (snippets / history commands) add two extra verbs on top:
/// Shift+Enter pastes without the newline (`paste`; plain Enter runs
/// via `action.activate`) and Delete removes (`delete`).
#[derive(Clone)]
pub(crate) struct SidebarRow {
    pub(crate) action: RowAction,
    pub(crate) paste: Option<Message>,
    pub(crate) delete: Option<Message>,
    /// Context menu the Menu key opens on this row, the keyboard half
    /// of its right-click (the vault layer's `ContextMenu` handling,
    /// brought to the sidebar for the Monitor tab's port rows, issue
    /// #96). `None` = the row has no menu.
    pub(crate) menu: Option<Message>,
    /// Whether this row belongs to the tab's LIST body (a snippet,
    /// history entry, file row, group card) rather than header chrome
    /// (path, search, sort, the strip's Close). The arrow hover-entry
    /// lands on the first/last list row, never on chrome: a ring
    /// popping up on a header control reads as "nothing happened"
    /// (live QA: the ringed Files path row looked like a plain
    /// focused text input).
    pub(crate) list: bool,
    /// Whether this row is the surface's current mouse selection
    /// (the Files tab's click-select). The arrow hover-entry lands
    /// on the anchored row instead of the list edge, so keyboard
    /// navigation picks up where the mouse left off.
    pub(crate) anchor: bool,
    /// Whether this row is the strip's own header chrome (the Close X,
    /// Chat's Reset). A fresh Tab walk must never LAND on chrome: the
    /// header's row count varies per tab (Chat records Reset before
    /// Close), so an index guess lands wrong; walking onto it is fine.
    pub(crate) chrome: bool,
}

impl SidebarRow {
    /// A snippet / history command row: Enter runs, Shift+Enter
    /// pastes, Delete removes (through its confirm).
    pub(crate) fn item(run: Message, paste: Message, delete: Message) -> Self {
        Self {
            action: RowAction::activate(run),
            paste: Some(paste),
            delete: Some(delete),
            menu: None,
            list: true,
            anchor: false,
            chrome: false,
        }
    }

    /// A plain button / toggle / card row: Enter activates.
    pub(crate) fn button(msg: Message) -> Self {
        Self {
            action: RowAction::activate(msg),
            paste: None,
            delete: None,
            menu: None,
            list: false,
            anchor: false,
            chrome: false,
        }
    }

    /// [`Self::button`] for rows that are LIST entries (file rows,
    /// snippet group cards): same Enter-activates contract, but the
    /// arrow hover-entry may land here.
    pub(crate) fn list_button(msg: Message) -> Self {
        Self {
            action: RowAction::activate(msg),
            paste: None,
            delete: None,
            menu: None,
            list: true,
            anchor: false,
            chrome: false,
        }
    }

    /// A text-input / focusable-widget row: Tab gives it real focus.
    pub(crate) fn input(id: iced::widget::Id) -> Self {
        Self {
            action: RowAction::input(id),
            paste: None,
            delete: None,
            menu: None,
            list: false,
            anchor: false,
            chrome: false,
        }
    }

    /// A stepper / cycling row: Left/Right fire prev/next.
    pub(crate) fn picker(prev: Option<Message>, next: Option<Message>) -> Self {
        Self {
            action: RowAction::picker(prev, next),
            paste: None,
            delete: None,
            menu: None,
            list: false,
            anchor: false,
            chrome: false,
        }
    }

    /// Attach the row's context menu, so the Menu key reaches what
    /// right-click reaches. A row whose only extra actions live in a
    /// popover is mouse-only without this.
    pub(crate) fn with_menu(mut self, msg: Message) -> Self {
        self.menu = Some(msg);
        self
    }

    /// Attach the row's delete verb, so the Delete key reaches what
    /// the row's context menu reaches (a list row whose remove action
    /// only lives behind right-click is mouse-only without this).
    pub(crate) fn with_delete(mut self, msg: Message) -> Self {
        self.delete = Some(msg);
        self
    }

    /// Mark this row as the surface's current mouse selection, the
    /// arrow hover-entry's preferred landing spot.
    pub(crate) fn with_anchor(mut self, anchor: bool) -> Self {
        self.anchor = anchor;
        self
    }

    /// Mark this row as strip header chrome (Close / Reset): a fresh
    /// Tab walk skips over it when picking where to land.
    pub(crate) fn chrome(mut self) -> Self {
        self.chrome = true;
        self
    }
}

/// Identity of the surface a modal-layer selection belongs to. A
/// selection carrying a stale tag counts as no selection, so closing
/// one menu and opening another can never dispatch a row from the
/// previous surface; no cleanup hooks needed at the ~50 open/close
/// sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalSurface {
    Modal(crate::state::Modal),
    /// Anchored dropdown/kebab menus; the discriminant distinguishes
    /// menu kinds without requiring PartialEq on the payload.
    Overlay(std::mem::Discriminant<crate::state::OverlayContent>),
    Burger,
    /// The SFTP right-click row context menu (`sftp.row_menu`), which
    /// lives outside the `overlay` / `Modal` systems.
    SftpRowMenu,
}

/// Selection + per-frame row recording for the modal layer.
#[derive(Default)]
pub(crate) struct ModalNavState {
    /// Explicitly selected row, tagged by its owner surface.
    pub(crate) selected: Option<(ModalSurface, usize)>,
    /// Actions recorded by the active modal/menu during view().
    pub(crate) items: RefCell<Vec<RowAction>>,
    /// Row the surface marked as its default (confirm dialogs mark
    /// the action button; menus mark their first row).
    pub(crate) default: std::cell::Cell<Option<usize>>,
    /// Input-modality gate for the ring, focus-visible semantics:
    /// mouse hover still moves the selection (so Enter activates the
    /// row under the cursor and arrows continue from there), but the
    /// ring is DRAWN only when the last interaction was the keyboard.
    /// Set by the modal key router, cleared by hover. The default-row
    /// ring is exempt: on a confirm dialog it is the "Enter confirms"
    /// affordance and stays visible regardless of modality.
    pub(crate) kbd: std::cell::Cell<bool>,
}

/// prev/next messages for a picker row: the on_select message the
/// dropdown would fire for the neighboring option, wrapping at the
/// ends. Call sites know both the options and the current value, so
/// the pair is prepared at render time and stored in the RowAction.
pub(crate) fn cycle_pair<T: Clone + PartialEq>(
    options: &[T],
    current: &T,
    mk: impl Fn(T) -> Message,
) -> (Option<Message>, Option<Message>) {
    let n = options.len();
    let Some(i) = options.iter().position(|o| o == current) else {
        // Unknown current value: both arrows land on the first option
        // so the control becomes coherent instead of dead.
        let first = options.first().cloned().map(&mk);
        return (first.clone(), first);
    };
    if n < 2 {
        return (None, None);
    }
    (
        Some(mk(options[(i + n - 1) % n].clone())),
        Some(mk(options[(i + 1) % n].clone())),
    )
}

impl crate::app::Oryxis {
    /// Clear the modal-layer recording. Every navigable modal / menu
    /// view calls this first (only one such surface renders per
    /// frame, topmost-wins like `close_topmost_modal`).
    pub(crate) fn modal_nav_reset(&self) {
        self.keynav.modal.items.borrow_mut().clear();
        self.keynav.modal.default.set(None);
    }

    /// The row index the keyboard currently points at: the explicit
    /// selection when its surface tag matches and the index still
    /// exists (clamped), else the surface default.
    pub(crate) fn modal_nav_effective(&self, surface: ModalSurface) -> Option<usize> {
        use super::movement::clamp_index;
        let len = self.keynav.modal.items.borrow().len();
        match self.keynav.modal.selected {
            Some((tag, idx)) if tag == surface => clamp_index(idx, len),
            _ => self.keynav.modal.default.get().and_then(|d| clamp_index(d, len)),
        }
    }

    /// Record one modal-layer row WITHOUT wrapping an element, for
    /// call sites whose ring wrapper must be applied later than the
    /// recording (an inner slot records during construction, e.g. the
    /// password reveal eye inside its field). Pair with
    /// [`Self::modal_nav_ring_at`].
    pub(crate) fn modal_nav_record(&self, action: RowAction) -> usize {
        let mut items = self.keynav.modal.items.borrow_mut();
        items.push(action);
        items.len() - 1
    }

    /// Record one actionable row and ring it when selected. `radius`
    /// matches the row's own corner radius; `contrast` picks the
    /// text_primary ring for accent/danger-filled buttons (an accent
    /// ring vanishes into them, same rationale as the toolbar ring).
    pub(crate) fn modal_nav_slot<'a>(
        &self,
        action: RowAction,
        radius: f32,
        contrast: bool,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let idx = self.modal_nav_record(action);
        self.modal_nav_ring_at(idx, radius, contrast, el)
    }

    /// The ring/hover wrapper half of [`Self::modal_nav_slot`], keyed
    /// by an index returned from [`Self::modal_nav_record`].
    pub(crate) fn modal_nav_ring_at<'a>(
        &self,
        idx: usize,
        radius: f32,
        contrast: bool,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        // Hover converges the ring with the mouse position.
        let el: iced::Element<'a, Message> = iced::widget::MouseArea::new(el)
            .on_enter(Message::Navigation(NavigationMessage::ModalNavHover(idx)))
            .into();
        // RAW index comparison, no clamping: this runs mid-recording,
        // when the list is still partial, and clamping a selection of
        // e.g. 3 against a 1-long list would ring EVERY row on its
        // way in (each row briefly "is" the last one). The router
        // clamps when it acts; the ring only matches exact indices.
        //
        // Focus-visible: an explicit selection draws its ring only
        // when the keyboard made it (`modal.kbd`); a hover-driven one
        // stays invisible, the row's own hover background is the mouse
        // feedback. The default-row fallback (no explicit selection)
        // follows the same gate on menus/pickers, so the click that
        // clears the hover selection on PRESS can't flash the ring on
        // row 0 while the release (which closes the menu) is still in
        // flight. Confirm dialogs are exempt: their default ring is
        // the "Enter confirms" affordance and stays visible.
        let surface_family = self.modal_nav_surface();
        let surface = surface_family.map(|(s, _)| s);
        let kbd = self.keynav.modal.kbd.get();
        let default_visible = kbd
            || matches!(
                surface_family,
                Some((_, crate::dispatch_keynav_modal::SurfaceFamily::Confirm))
            );
        let explicit = match self.keynav.modal.selected {
            Some((tag, i)) if Some(tag) == surface => Some(i),
            _ => None,
        };
        let selected = match explicit {
            Some(i) => kbd && i == idx,
            None => default_visible && self.keynav.modal.default.get() == Some(idx),
        };
        // Always wrapped (transparent when unselected): a ring that
        // appears/disappears between press and release would reset the
        // row's button state and eat the click. See select_ring_opt.
        let color = selected.then(|| {
            if contrast {
                crate::theme::OryxisColors::t().text_primary
            } else {
                crate::theme::OryxisColors::t().accent
            }
        });
        crate::widgets::select_ring_opt(el, radius, color)
    }

    /// `modal_nav_slot` that also marks this row as the surface
    /// default (confirm dialogs call it on their action button).
    pub(crate) fn modal_nav_slot_default<'a>(
        &self,
        action: RowAction,
        radius: f32,
        contrast: bool,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let next_idx = self.keynav.modal.items.borrow().len();
        self.keynav.modal.default.set(Some(next_idx));
        self.modal_nav_slot(action, radius, contrast, el)
    }

    /// Clear the side-panel row recording. The panel view calls this
    /// at the top of its render pass, then records its actionable
    /// rows through `panel_nav_slot`.
    pub(crate) fn panel_nav_reset(&self) {
        self.keynav.panel_items.borrow_mut().clear();
    }

    /// Drop the panel row-mode state entirely (selection + remembered
    /// position). Called wherever the host editor opens or closes so
    /// a stale ring can never survive across editor sessions.
    pub(crate) fn panel_nav_clear(&mut self) {
        self.keynav.panel_selected = None;
        self.keynav.panel_last_row.set(None);
        // A dropdown can't survive its panel: if the panel unmounts
        // while a pick_list menu was open, the widget never gets to
        // publish on_close, so drop the flag here too.
        self.keynav.pick_open = false;
    }

    /// Record one side-panel row WITHOUT wrapping an element. For
    /// input rows whose element embeds an inner slot that records
    /// during construction (the password reveal eye): the field's row
    /// is recorded first so recording order stays display order, and
    /// since input rows never draw the panel ring, skipping the
    /// wrapper changes nothing visually.
    pub(crate) fn panel_nav_record(&self, action: RowAction) {
        self.keynav.panel_items.borrow_mut().push(action);
    }

    /// Record one actionable side-panel row and ring it when it is
    /// the current selection. Same `RowAction` vocabulary as the
    /// Settings rows (activate / picker prev+next / input focus).
    /// Input rows never draw the ring: Tab gives them real iced
    /// focus and the field's own focused border is the indicator.
    pub(crate) fn panel_nav_slot<'a>(
        &self,
        action: RowAction,
        radius: f32,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let is_input = action.focus.is_some();
        let idx = {
            let mut items = self.keynav.panel_items.borrow_mut();
            items.push(action);
            items.len() - 1
        };
        // Always wrapped (transparent when unringed): see
        // select_ring_opt for why the wrapper must be shape-stable.
        let ringed = !is_input && self.keynav.panel_selected == Some(idx);
        crate::widgets::select_ring_opt(
            el,
            radius,
            ringed.then(|| crate::theme::OryxisColors::t().accent),
        )
    }

    /// Clear both terminal-sidebar list recordings. `view_terminal`
    /// calls this once per frame BEFORE either region renders (issue
    /// #102), so a frame where a region records nothing (Chat, a
    /// closed region) can't leave a stale row list behind, and a
    /// region rendering second can't wipe the first's rows.
    pub(crate) fn sidebar_nav_reset(&self) {
        for items in &self.keynav.sidebar_items {
            items.borrow_mut().clear();
        }
    }

    /// Record one terminal-sidebar row and ring it when selected.
    /// Recording order is display order, so the recorded index always
    /// matches the row's on-screen position. Input rows never draw
    /// the ring: Tab gives them real iced focus and the widget's own
    /// focused border is the indicator (panel contract).
    pub(crate) fn sidebar_nav_slot<'a>(
        &self,
        row: SidebarRow,
        tab: crate::state::TerminalSidebarTab,
        radius: f32,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        self.sidebar_nav_slot_inner(row, tab, radius, false, el)
    }

    /// [`Self::sidebar_nav_slot`] with the text_primary contrast ring,
    /// for accent-filled controls (the "+ SNIPPET" button, the local
    /// config's save): an accent ring vanishes into them, same
    /// rationale as the toolbar / modal contrast rings.
    pub(crate) fn sidebar_nav_slot_contrast<'a>(
        &self,
        row: SidebarRow,
        tab: crate::state::TerminalSidebarTab,
        radius: f32,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        self.sidebar_nav_slot_inner(row, tab, radius, true, el)
    }

    fn sidebar_nav_slot_inner<'a>(
        &self,
        row: SidebarRow,
        tab: crate::state::TerminalSidebarTab,
        radius: f32,
        contrast: bool,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let is_input = row.action.focus.is_some();
        // The row lands in its REGION's list: the tab that records it
        // names the region via its dock side (issue #102). A hidden
        // tab never renders, so the fallback is unreachable; Right
        // keeps a misuse harmless rather than panicking mid-view.
        let side = self
            .prefs
            .sidebar_tab_side(tab)
            .unwrap_or(crate::state::SidebarSide::Right);
        let idx = {
            let mut items = self.keynav.sidebar_items[side.idx()].borrow_mut();
            items.push(row);
            items.len() - 1
        };
        // Always wrapped (transparent when unringed): see
        // select_ring_opt for why the wrapper must be shape-stable.
        let ringed = !is_input && self.keynav.sidebar_selected == Some((tab, idx));
        let el = crate::widgets::select_ring_opt(
            el,
            radius,
            ringed.then(|| {
                if contrast {
                    crate::theme::OryxisColors::t().text_primary
                } else {
                    crate::theme::OryxisColors::t().accent
                }
            }),
        );
        // Report the ringed row's rect so the Menu key can anchor a
        // row's context menu at the ROW, the same handshake
        // `keynav_ring_content` gives vault cards. Without it the
        // popover would open at wherever the mouse happens to rest,
        // which for a keyboard user is anywhere at all.
        if ringed {
            crate::widgets::bounds_reporter(el, self.keynav.ring_bounds.clone())
        } else {
            el
        }
    }

    /// Recording wrapper over `widgets::context_menu_item`: same row,
    /// registered for Up/Down + Enter in the open menu. The free fn
    /// stays for non-navigable uses (the hover-only split popover).
    pub(crate) fn menu_item<'a>(
        &self,
        icon: impl Into<crate::os_icon::BrandIcon>,
        label: &'a str,
        msg: Message,
        color: iced::Color,
    ) -> iced::Element<'a, Message> {
        self.modal_nav_slot(
            RowAction::activate(msg.clone()),
            4.0,
            false,
            crate::widgets::context_menu_item(icon, label, msg, color),
        )
    }

    /// Clear the Settings content recording. Each Settings section
    /// view calls this at the top of its render pass, then records
    /// its actionable rows through `settings_nav_slot`.
    pub(crate) fn keynav_settings_reset(&self) {
        self.keynav.settings_row_actions.borrow_mut().clear();
        self.keynav.settings_row_highlight.borrow_mut().clear();
        self.keynav.content_rows.borrow_mut().clear();
        *self.keynav.content_section_starts.borrow_mut() = vec![0];
        // Re-derived every render from the freshest recording.
        self.keynav.settings_first_match_idx.set(None);
    }

    /// Whether a settings row with this visible label matches the
    /// active sidebar-search query. The set is filled per-frame by the
    /// sidebar render (which runs before the content) from the active
    /// section's ranked results, so this is a cheap value compare.
    // `contains(&label)` doesn't type-check: the stored labels are
    // `&'static str` while `label` is a shorter-lived `&str`.
    #[allow(clippy::manual_contains)]
    pub(crate) fn settings_search_highlight(&self, label: &str) -> bool {
        self.keynav
            .settings_match_labels
            .borrow()
            .iter()
            .any(|l| *l == label)
    }

    /// Record one Settings content row, tracking whether it is a
    /// search match (drives the amber highlight) and whether it is the
    /// ACTIVE match (find-next cursor: accent ring + scroll anchor).
    /// Read-only rows are simply not recorded, so arrows only stop on
    /// things Enter/Space/Left/Right can act on.
    fn settings_nav_record_hl(&self, action: RowAction, highlight: bool, active: bool) -> usize {
        let idx = {
            let mut actions = self.keynav.settings_row_actions.borrow_mut();
            actions.push(action);
            actions.len() - 1
        };
        self.keynav.settings_row_highlight.borrow_mut().push(highlight);
        if active && self.keynav.settings_first_match_idx.get().is_none() {
            self.keynav.settings_first_match_idx.set(Some(idx));
        }
        self.keynav
            .content_rows
            .borrow_mut()
            .push(vec![super::NavItem::SettingsRow(idx)]);
        idx
    }

    /// Record one Settings content row WITHOUT wrapping an element,
    /// for call sites whose ring wrapper must be applied later than
    /// the recording (an inner slot records during construction, e.g.
    /// the password reveal eye inside its field). Pair with
    /// [`Self::settings_nav_ring_at`]. Not a search match (unlabeled).
    pub(crate) fn settings_nav_record(&self, action: RowAction) -> usize {
        self.settings_nav_record_hl(action, false, false)
    }

    /// Whether `label` is the active find-next match (accent "current"
    /// ring + scroll anchor), filled per-frame by the sidebar render.
    fn settings_is_active_match(&self, label: &str) -> bool {
        self.keynav
            .settings_active_label
            .get()
            .is_some_and(|a| a == label)
    }

    /// Record one actionable Settings content row (single-column) and
    /// ring it when selected. `radius` matches the row's own corner
    /// radius. Not a search target (see the `_labeled` variants).
    pub(crate) fn settings_nav_slot<'a>(
        &self,
        action: RowAction,
        radius: f32,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let idx = self.settings_nav_record(action);
        self.settings_nav_ring_at(idx, radius, el)
    }

    /// [`Self::settings_nav_slot`] for a row whose visible `label` is
    /// in `SETTINGS_INDEX`: the row highlights amber when it matches
    /// the active search. `nav_toggle_row` / `nav_pick_row` do this
    /// for free; inline index rows call this explicitly.
    pub(crate) fn settings_nav_slot_labeled<'a>(
        &self,
        label: &str,
        action: RowAction,
        radius: f32,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let hl = self.settings_search_highlight(label);
        let active = self.settings_is_active_match(label);
        let idx = self.settings_nav_record_hl(action, hl, active);
        self.settings_nav_ring_at(idx, radius, el)
    }

    /// [`Self::settings_nav_record`] for a labeled row (record-only
    /// half, for rows whose ring is applied separately).
    pub(crate) fn settings_nav_record_labeled(
        &self,
        label: &str,
        action: RowAction,
    ) -> usize {
        let hl = self.settings_search_highlight(label);
        let active = self.settings_is_active_match(label);
        self.settings_nav_record_hl(action, hl, active)
    }

    /// The ring wrapper half of [`Self::settings_nav_slot`], keyed by
    /// an index returned from [`Self::settings_nav_record`]. Draws the
    /// accent ring on the keynav selection / active find-next match, or
    /// the amber highlight on a plain search match; the active match
    /// also reports its rect so the search can scroll it into view.
    pub(crate) fn settings_nav_ring_at<'a>(
        &self,
        idx: usize,
        radius: f32,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let item = super::NavItem::SettingsRow(idx);
        let ringed = self.keynav.selected_in(super::FocusZone::Content) == Some(item);
        let matched = self
            .keynav
            .settings_row_highlight
            .borrow()
            .get(idx)
            .copied()
            .unwrap_or(false);
        let active = self.keynav.settings_first_match_idx.get() == Some(idx);
        // Priority: keynav selection or the ACTIVE find-next match draw
        // the accent ring (the "current" match, distinct from its amber
        // siblings); a plain match draws amber. An outset gives the box
        // breathing room on edge-to-edge rows (owner feedback).
        let color = if ringed || active {
            Some(crate::theme::OryxisColors::t().accent)
        } else if matched {
            Some(crate::theme::OryxisColors::t().warning)
        } else {
            None
        };
        let outset = if matched || active { 3.0 } else { 0.0 };
        // Always wrapped (transparent when neither): see
        // select_ring_opt for why the wrapper must be shape-stable.
        let el = crate::widgets::select_ring_opt_outset(el, radius, color, outset);
        // Tag the active-match row so the scroll-into-view operation can
        // find its layout position even when it is off-screen (operate
        // sees culled rows; draw does not).
        if active {
            crate::widgets::report_container_id(SETTINGS_SCROLL_TARGET_ID, el)
        } else {
            el
        }
    }

    /// Ring a keyboard-selected content CARD and report its on-screen
    /// rect into `keynav.ring_bounds`, so the Menu key can anchor the
    /// card's context menu at the card (kebab corner) instead of the
    /// mouse position. Only the single ringed element writes the cell
    /// per frame. Call it for EVERY card, passing `ringed`: the ring
    /// Stack must stay in the tree either way (shape-stable, see
    /// select_ring_opt) while the bounds_reporter, which is invisible
    /// to the widget tree, wraps only the ringed card.
    pub(crate) fn keynav_ring_content<'a>(
        &self,
        ringed: bool,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let el = crate::widgets::select_ring_opt(
            el,
            10.0,
            ringed.then(|| crate::theme::OryxisColors::t().accent),
        );
        if ringed {
            crate::widgets::bounds_reporter(el, self.keynav.ring_bounds.clone())
        } else {
            el
        }
    }

    /// The anchor for the next kebab-menu open: the Menu key's ring
    /// anchor when set (one-shot), else the live mouse position.
    pub(crate) fn keynav_take_menu_anchor(&mut self) -> (f32, f32) {
        self.keynav
            .menu_anchor
            .take()
            .unwrap_or((self.mouse_position.x, self.mouse_position.y))
    }

    /// Record one generic content-action row (single-column): used by
    /// content surfaces whose rows fire arbitrary messages. The caller
    /// clears via
    /// `keynav_clear_content` at the top of its render pass.
    pub(crate) fn content_action_slot<'a>(
        &self,
        action: RowAction,
        radius: f32,
        el: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let idx = {
            let mut actions = self.keynav.content_actions.borrow_mut();
            actions.push(action);
            actions.len() - 1
        };
        let item = super::NavItem::ContentAction(idx);
        self.keynav.content_rows.borrow_mut().push(vec![item]);
        // Always wrapped (transparent when unringed): see
        // select_ring_opt for why the wrapper must be shape-stable.
        let ringed = self.keynav.selected_in(super::FocusZone::Content) == Some(item);
        crate::widgets::select_ring_opt(
            el,
            radius,
            ringed.then(|| crate::theme::OryxisColors::t().accent),
        )
    }

    /// Recording toggle row for Settings content: same visual as
    /// `widgets::toggle_row`, plus Enter/Space flipping it from the
    /// keyboard.
    pub(crate) fn nav_toggle_row<'a>(
        &self,
        label: &'a str,
        value: bool,
        msg: Message,
    ) -> iced::Element<'a, Message> {
        self.settings_nav_slot_labeled(
            label,
            RowAction::activate(msg.clone()),
            8.0,
            crate::widgets::toggle_row(label, value, msg),
        )
    }

    /// Recording picker row for Settings content: the standard
    /// "label ... pick_list" line, with Left/Right cycling the
    /// options without opening the dropdown. (Settings keeps the
    /// ring-and-cycle model: Up/Down stay row navigation here, unlike
    /// the side panels where pickers are Tab-focusable inputs.)
    pub(crate) fn nav_pick_row<'a, D, F>(
        &self,
        label: &'a str,
        options: Vec<String>,
        selected: String,
        display: D,
        width: f32,
        on_change: F,
    ) -> iced::Element<'a, Message>
    where
        D: Fn(&String) -> String + 'a,
        F: Fn(String) -> Message + Clone + 'a,
    {
        let (prev, next) = cycle_pair(&options, &selected, on_change.clone());
        // The ring hugs the pick_list itself, not the whole row: it
        // marks WHICH control Left/Right act on (user feedback). The
        // label drives the search highlight.
        let picker = self.settings_nav_slot_labeled(
            label,
            RowAction::picker(prev, next),
            crate::widgets::INPUT_RADIUS,
            iced::widget::pick_list(Some(selected), options, display)
                .on_select(on_change)
                // Mouse-opened dropdowns arm the same key guard the
                // focusable panel pickers use, so Esc closes the menu
                // instead of falling through to the app routers.
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(width)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
        );
        crate::widgets::dir_row(vec![
            iced::widget::text(label)
                .size(13)
                .color(crate::theme::OryxisColors::t().text_primary)
                .into(),
            iced::widget::Space::new().width(iced::Length::Fill).into(),
            picker,
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// Recording wrapper over `widgets::sort_menu_row`.
    pub(crate) fn sort_row(
        &self,
        kind: crate::state::SortMenuKind,
        sort: crate::state::ListSort,
        icon: iced::widget::Text<'static, iced::Theme, iced::Renderer>,
        label_key: &'static str,
        is_active: bool,
    ) -> iced::Element<'static, Message> {
        self.modal_nav_slot(
            RowAction::activate(Message::Navigation(NavigationMessage::SetListSort(kind, sort))),
            4.0,
            false,
            crate::widgets::sort_menu_row(kind, sort, icon, label_key, is_active),
        )
    }
}
