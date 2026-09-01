//! Tab-strip ordering helpers split out of `dispatch_tabs`:
//! reconcile / replace-id / live-slide reorder, and the connect-
//! progress re-anchor after a bulk tab filter.

use crate::app::Oryxis;

impl Oryxis {
    /// Whether a panel currently has a chip in the strip.
    pub(crate) fn panel_tab_open(&self, kind: crate::state::PanelKind) -> bool {
        self.open_panel_tabs.contains(&kind)
    }

    /// Give a panel surface a strip entry (issue #120 gave Settings the
    /// first one), so leaving it and coming back is one click instead of
    /// a hunt through the toolbar. Idempotent, and called from
    /// `ChangeView`, which is the single door every entry point (gear,
    /// burger menu, hotkey, command palette, the strip entry itself)
    /// already goes through. That is why there is at most one per kind:
    /// nothing else can mint a second.
    ///
    /// Modelled on `ensure_sftp_tab`, which does the same for `View::Sftp`.
    pub(crate) fn ensure_panel_tab(&mut self, kind: crate::state::PanelKind) {
        if !self.open_panel_tabs.insert(kind) {
            return;
        }
        if !self.tab_order.contains(&crate::state::TabRef::Panel(kind)) {
            self.tab_order.push(crate::state::TabRef::Panel(kind));
        }
    }

    /// Close a panel tab. When the panel is the surface on screen the
    /// close has to take you somewhere, so it lands on the previously
    /// focused tab if there still is one, and on the Home dashboard
    /// otherwise. Closing it from another surface only removes the chip.
    pub(crate) fn close_panel_tab(
        &mut self,
        kind: crate::state::PanelKind,
    ) -> iced::Task<crate::app::Message> {
        self.open_panel_tabs.remove(&kind);
        self.tab_order.retain(|r| !matches!(r, crate::state::TabRef::Panel(k) if *k == kind));
        // Per-panel teardown: what a closed panel must not carry into
        // the next time it is opened.
        match kind {
            crate::state::PanelKind::Settings => self.settings_scroll.clear(),
            crate::state::PanelKind::NetTools => self.net_tools.reset(),
        }
        if !(self.active_tab.is_none() && self.active_view == kind.view()) {
            return iced::Task::none();
        }
        // Most-recently-used first, skipping the entry we just dropped.
        let fallback = self
            .tab_mru
            .iter()
            .find(|r| !matches!(r, crate::state::TabRef::Panel(k) if *k == kind))
            .copied()
            .and_then(|r| self.tab_ref_select_msg(&r));
        iced::Task::done(fallback.unwrap_or(crate::app::Message::Navigation(
            crate::app::NavigationMessage::ChangeView(crate::state::View::Dashboard),
        )))
    }

    /// Sync `tab_order` (the authoritative strip display order across terminal
    /// and SFTP tabs) with the live tabs: append refs for newly-created tabs,
    /// drop refs for closed ones, preserve the existing (drag-reordered) order.
    /// Cheap; called at the end of every `update`.
    pub(crate) fn reconcile_tab_order(&mut self) {
        use crate::state::TabRef;
        self.tab_order.retain(|r| match r {
            TabRef::Terminal(id) => self.tabs.iter().any(|t| t._id == *id),
            TabRef::Sftp(id) => self.sftp_tabs.iter().any(|t| t.id == *id),
            // Not backed by a storage vec: `open_panel_tabs` is the
            // whole existence test.
            TabRef::Panel(kind) => self.open_panel_tabs.contains(kind),
        });
        for id in self.tabs.iter().map(|t| t._id).collect::<Vec<_>>() {
            if !self.tab_order.iter().any(|r| matches!(r, TabRef::Terminal(x) if *x == id)) {
                self.place_new_tab_ref(TabRef::Terminal(id));
            }
        }
        for id in self.sftp_tabs.iter().map(|t| t.id).collect::<Vec<_>>() {
            if !self.tab_order.iter().any(|r| matches!(r, TabRef::Sftp(x) if *x == id)) {
                self.tab_order.push(TabRef::Sftp(id));
            }
        }
    }

    /// Give a just-born tab its strip slot: at the end like every other
    /// tab, unless a Duplicate armed a [`crate::state::PendingTabPlacement`]
    /// for it.
    ///
    /// This is the single door every new terminal tab walks through
    /// (`reconcile_tab_order` runs at the end of every `update`), which
    /// is why the placement lives here instead of at the spawn sites:
    /// SSH, Telnet, Serial, local shell, session groups and the
    /// asynchronous cloud plugins all arrive here, several updates late
    /// in the cloud case, with no per-site wiring to keep in sync.
    ///
    /// Only `tab_order` moves. `Oryxis::tabs` keeps append-only
    /// semantics, so no `active_tab` / `last_terminal_tab` /
    /// `connecting.tab_idx` index goes stale. (`pending_pane_split` was
    /// on that list until it started holding a tab id instead.)
    fn place_new_tab_ref(&mut self, r: crate::state::TabRef) {
        let at = self
            .pending_tab_placement
            .take()
            // A duplicate that never produced a tab must not reposition
            // an unrelated one opened later.
            .filter(|p| !p.is_expired())
            .and_then(|p| placement_index(&self.tab_order, p.placement, p.source_id));
        match at {
            Some(at) => self.tab_order.insert(at.min(self.tab_order.len()), r),
            None => self.tab_order.push(r),
        }
    }

    /// Replace a terminal tab's id in `tab_order` in place (same position).
    /// Used when a dormant placeholder is swapped for its freshly-connected
    /// live tab (new id) so the reopened tab keeps its strip position instead
    /// of being appended at the end by `reconcile_tab_order`.
    pub(crate) fn replace_tab_order_id(&mut self, old: uuid::Uuid, new: uuid::Uuid) {
        for r in self.tab_order.iter_mut() {
            if let crate::state::TabRef::Terminal(id) = r
                && *id == old
            {
                *id = new;
                return;
            }
        }
    }

    /// Put the reopened tab `new` back at `slot`, the index the dormant
    /// placeholder `old` held before it was dropped.
    ///
    /// [`Self::replace_tab_order_id`] cannot do this job when the caller
    /// removed the placeholder from `self.tabs` BEFORE dispatching the
    /// reopen: that dispatch is a nested `update`, whose
    /// `reconcile_tab_order` drops refs with no backing tab, so by the
    /// time the caller looks there is no `old` ref left to rename and the
    /// rename silently no-ops. The reopened tab then lands wherever
    /// `place_new_tab_ref` put it, at the end, losing the position the
    /// user arranged. Capture the index first, restore it here.
    pub(crate) fn restore_tab_order_slot(
        &mut self,
        old: uuid::Uuid,
        new: uuid::Uuid,
        slot: Option<usize>,
    ) {
        restore_order_slot(&mut self.tab_order, old, new, slot);
    }

    /// Put the terminal tab `new` into the strip slot the SFTP tab `old`
    /// occupies, for the "Open terminal" morph (H5). Cross-kind, which is
    /// why it is not [`Self::replace_tab_order_id`].
    ///
    /// Two things make this more than a rename. The morph dispatches
    /// `ConnectSsh` through `self.update`, and that is the SAME `update`
    /// (`dispatch.rs`), so `reconcile_tab_order` has ALREADY appended a
    /// `TabRef::Terminal(new)` at the end by the time we get here; renaming
    /// the old ref would leave two refs carrying one id, which renders as a
    /// duplicate chip. And the SFTP tab is still in the strip at this point
    /// (it is closed after), so its slot is a real index rather than one we
    /// have to remember across a removal.
    ///
    /// Only `tab_order` moves, per the contract on [`Self::place_new_tab_ref`]:
    /// `Oryxis::tabs` stays append-only, so no `active_tab` /
    /// `last_terminal_tab` / `connecting.tab_idx` index goes stale.
    pub(crate) fn morph_tab_order_slot(&mut self, old: uuid::Uuid, new: uuid::Uuid) {
        morph_order_slot(&mut self.tab_order, old, new);
    }

    /// Move the tab identified by `from_id` to just before `target_id` in
    /// `tab_order`, but only within the same pin partition (can't drag an
    /// unpinned tab above a pinned one, matching the terminal behaviour). Used
    /// by the unified live-slide drag. Re-anchors nothing (the storage vecs and
    /// `active_tab` / `active_sftp` indices are untouched; only display order
    /// changes).
    pub(crate) fn slide_tab_in_order(&mut self, from_id: uuid::Uuid, target_id: uuid::Uuid) {
        let pinned_of = |r: &crate::state::TabRef| -> bool {
            match r {
                crate::state::TabRef::Terminal(id) => {
                    self.tabs.iter().find(|t| t._id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                crate::state::TabRef::Sftp(id) => {
                    self.sftp_tabs.iter().find(|t| t.id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                // Transient by design, so pinning it would promise a
                // persistence it does not have.
                crate::state::TabRef::Panel(_) => false,
            }
        };
        let id_of = |r: &crate::state::TabRef| -> uuid::Uuid { r.strip_id() };
        let Some(from_pos) = self.tab_order.iter().position(|r| id_of(r) == from_id) else { return };
        let Some(to_pos) = self.tab_order.iter().position(|r| id_of(r) == target_id) else { return };
        if from_pos == to_pos {
            return;
        }
        // Same partition only.
        if pinned_of(&self.tab_order[from_pos]) != pinned_of(&self.tab_order[to_pos]) {
            return;
        }
        let moved = self.tab_order.remove(from_pos);
        let dest = if from_pos < to_pos { to_pos - 1 } else { to_pos };
        self.tab_order.insert(dest, moved);
    }

    /// Move the tab identified by `from_id` to the very end of its own pin
    /// partition in `tab_order` (last among normal tabs, or last among pinned).
    /// Powers the trailing drop zone so a tab can reach the rightmost slot,
    /// which the before-the-target live-slide can never express. Idempotent:
    /// a no-op when the tab already sits at its partition's end, so repeated
    /// `CursorMoved`-driven calls don't thrash.
    pub(crate) fn slide_tab_to_partition_end(&mut self, from_id: uuid::Uuid) {
        let pinned_of = |r: &crate::state::TabRef| -> bool {
            match r {
                crate::state::TabRef::Terminal(id) => {
                    self.tabs.iter().find(|t| t._id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                crate::state::TabRef::Sftp(id) => {
                    self.sftp_tabs.iter().find(|t| t.id == *id).map(|t| t.pinned).unwrap_or(false)
                }
                // Transient by design, so pinning it would promise a
                // persistence it does not have.
                crate::state::TabRef::Panel(_) => false,
            }
        };
        let id_of = |r: &crate::state::TabRef| -> uuid::Uuid { r.strip_id() };
        let Some(from_pos) = self.tab_order.iter().position(|r| id_of(r) == from_id) else {
            return;
        };
        let from_pinned = pinned_of(&self.tab_order[from_pos]);
        // Last slot that belongs to the dragged tab's partition.
        let Some(last_same) = self.tab_order.iter().rposition(|r| pinned_of(r) == from_pinned)
        else {
            return;
        };
        if from_pos >= last_same {
            return;
        }
        // Removing `from_pos` shifts everything after it down one, so the old
        // `last_same` now sits at `last_same - 1`; inserting at `last_same`
        // drops the tab immediately after it (the new partition end).
        let moved = self.tab_order.remove(from_pos);
        self.tab_order.insert(last_same, moved);
    }

    /// Re-anchor (or clear) the in-flight connect progress after the tab
    /// list was filtered by close-others / close-all (both keep pinned
    /// tabs). `connecting_id` is the connecting tab's id captured *before*
    /// the filter: if that tab survived, point `tab_idx` at its new slot;
    /// if it was closed, drop the progress so a later SshRetry /
    /// SshCloseProgress can't `remove()` the wrong (surviving / pinned) tab.
    pub(super) fn reanchor_connecting_after_filter(&mut self, connecting_id: Option<uuid::Uuid>) {
        if self.connecting.is_none() {
            return;
        }
        match connecting_id.and_then(|cid| self.tabs.iter().position(|t| t._id == cid)) {
            Some(i) => {
                if let Some(p) = self.connecting.as_mut() {
                    p.tab_idx = i;
                }
            }
            None => self.connecting = None,
        }
    }

}

/// Where a new strip entry lands, given the placement a Duplicate armed.
/// `None` means "append", which is what every unarmed spawn does.
///
/// Pure (no `Oryxis`) so the ordering rules are unit-testable: this is
/// the whole behavioural difference between "the copy shows up beside
/// its original" and "the copy shows up wherever".
fn placement_index(
    order: &[crate::state::TabRef],
    placement: crate::state::TabPlacement,
    source_id: uuid::Uuid,
) -> Option<usize> {
    use crate::state::TabPlacement;
    match placement {
        TabPlacement::End => None,
        // Head of the strip. `strip_order` renders the pinned partition
        // first whatever `tab_order` says, so for the unpinned copy this
        // reads as "first unpinned tab", which is what the user sees.
        TabPlacement::Start => Some(0),
        // The original may have been closed while its copy was still
        // connecting: append rather than guess a slot.
        TabPlacement::NextToOriginal => order
            .iter()
            .position(|e| e.strip_id() == source_id)
            .map(|p| p + 1),
    }
}

/// Pure half of [`Oryxis::restore_tab_order_slot`].
fn restore_order_slot(
    order: &mut Vec<crate::state::TabRef>,
    old: uuid::Uuid,
    new: uuid::Uuid,
    slot: Option<usize>,
) {
    use crate::state::TabRef;
    // Both, because which of them is present depends on whether the
    // nested reconcile has run yet: the placeholder's ref, and the one
    // appended for the live tab.
    order.retain(|r| !matches!(r, TabRef::Terminal(x) if *x == old || *x == new));
    match slot {
        Some(at) => order.insert(at.min(order.len()), TabRef::Terminal(new)),
        // Never in the order to begin with: the end is where a new tab
        // would have gone anyway.
        None => order.push(TabRef::Terminal(new)),
    }
}

/// Pure half of [`Oryxis::morph_tab_order_slot`], so the slot arithmetic
/// is testable without an `Oryxis`.
fn morph_order_slot(order: &mut Vec<crate::state::TabRef>, old: uuid::Uuid, new: uuid::Uuid) {
    use crate::state::TabRef;
    // Drop the ref the nested reconcile appended (absent on the path that
    // reuses an existing terminal tab, where it keeps its own slot).
    order.retain(|r| !matches!(r, TabRef::Terminal(x) if *x == new));
    match order
        .iter()
        .position(|r| matches!(r, TabRef::Sftp(x) if *x == old))
    {
        Some(at) => order[at] = TabRef::Terminal(new),
        // No slot to inherit (the SFTP tab was never in the order): fall
        // back to the end, where a new tab would have landed anyway.
        None => order.push(TabRef::Terminal(new)),
    }
}

#[cfg(test)]
mod tests {
    use super::{morph_order_slot, placement_index, restore_order_slot};
    use crate::state::{TabPlacement, TabRef};
    use uuid::Uuid;

    /// A strip holding two terminal tabs around an SFTP tab, plus
    /// Settings: the placement walks `tab_order` by strip id, so the
    /// other tab kinds are just entries it has to count over.
    fn strip() -> (Vec<TabRef>, Uuid, Uuid) {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let sftp = Uuid::from_u128(3);
        (
            vec![
                TabRef::Terminal(a),
                TabRef::Sftp(sftp),
                TabRef::Terminal(b),
                TabRef::Panel(crate::state::PanelKind::Settings),
            ],
            a,
            b,
        )
    }

    #[test]
    fn next_to_original_lands_immediately_after_its_source() {
        let (order, a, b) = strip();
        assert_eq!(
            placement_index(&order, TabPlacement::NextToOriginal, a),
            Some(1),
            "a copy of the first tab goes before the SFTP tab that follows it"
        );
        assert_eq!(
            placement_index(&order, TabPlacement::NextToOriginal, b),
            Some(3),
            "a copy of the last terminal tab goes before Settings"
        );
    }

    #[test]
    fn a_closed_original_appends_instead_of_guessing() {
        let (order, ..) = strip();
        // The source was closed while its copy was still connecting.
        assert_eq!(
            placement_index(&order, TabPlacement::NextToOriginal, Uuid::from_u128(99)),
            None
        );
    }

    #[test]
    fn start_and_end_ignore_the_source() {
        let (order, a, _) = strip();
        assert_eq!(placement_index(&order, TabPlacement::Start, a), Some(0));
        assert_eq!(placement_index(&order, TabPlacement::End, a), None);
        // Even a dangling source id keeps those two honest.
        let gone = Uuid::from_u128(99);
        assert_eq!(placement_index(&order, TabPlacement::Start, gone), Some(0));
        assert_eq!(placement_index(&order, TabPlacement::End, gone), None);
    }

    #[test]
    fn a_panel_can_be_the_source_of_a_placement() {
        // Not reachable from Duplicate today (a panel has no duplicate
        // action), but the strip id is synthetic and the lookup must not
        // silently miss it if one ever appears.
        let (order, ..) = strip();
        assert_eq!(
            placement_index(
                &order,
                TabPlacement::NextToOriginal,
                crate::state::PanelKind::Settings.tab_id()
            ),
            Some(4)
        );
    }

    #[test]
    fn each_panel_kind_has_its_own_strip_id() {
        // Two panels sharing a synthetic id would drag as one chip and
        // reorder each other, which is exactly the kind of collision the
        // uuid-keyed machinery cannot detect.
        let mut ids: Vec<uuid::Uuid> =
            crate::state::PanelKind::ALL.iter().map(|k| k.tab_id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn every_panel_kind_round_trips_through_its_view() {
        // `for_view` is what `ChangeView` uses to decide which chip to
        // mint, so a kind whose view maps back to a different kind (or to
        // nothing) would open a surface with no tab.
        for kind in crate::state::PanelKind::ALL {
            assert_eq!(crate::state::PanelKind::for_view(kind.view()), Some(kind));
        }
    }

    #[test]
    fn an_empty_strip_appends_whatever_was_asked() {
        let empty: Vec<TabRef> = Vec::new();
        let id = Uuid::from_u128(1);
        assert_eq!(placement_index(&empty, TabPlacement::NextToOriginal, id), None);
        assert_eq!(placement_index(&empty, TabPlacement::Start, id), Some(0));
    }

    // -- H5: "Open terminal" morphs the SFTP tab into the pair -----------

    #[test]
    fn the_morphed_tab_inherits_the_sftp_tabs_slot() {
        let (a, sftp, b) = (Uuid::from_u128(1), Uuid::from_u128(3), Uuid::from_u128(2));
        let new = Uuid::from_u128(9);
        // What the strip looks like when the morph runs on the connect
        // path: the SFTP tab is still in place, and the nested
        // `reconcile_tab_order` has already appended the freshly
        // connected terminal tab at the end.
        let mut order = vec![
            TabRef::Terminal(a),
            TabRef::Sftp(sftp),
            TabRef::Terminal(b),
            TabRef::Terminal(new),
        ];
        morph_order_slot(&mut order, sftp, new);
        assert_eq!(
            order,
            vec![
                TabRef::Terminal(a),
                TabRef::Terminal(new),
                TabRef::Terminal(b),
            ],
            "the pair takes the SFTP tab's position, not the end of the strip"
        );
    }

    #[test]
    fn the_appended_ref_is_dropped_rather_than_duplicated() {
        // Renaming the old ref in place (what `replace_tab_order_id`
        // does) would leave two refs carrying one id, which renders as a
        // duplicate chip.
        let sftp = Uuid::from_u128(3);
        let new = Uuid::from_u128(9);
        let mut order = vec![TabRef::Sftp(sftp), TabRef::Terminal(new)];
        morph_order_slot(&mut order, sftp, new);
        assert_eq!(
            order.iter().filter(|r| r.strip_id() == new).count(),
            1,
            "exactly one entry for the morphed tab"
        );
        assert_eq!(order, vec![TabRef::Terminal(new)]);
    }

    #[test]
    fn only_a_tab_born_from_the_gesture_inherits_the_slot() {
        // Reusing a LIVE terminal must not drag it to the absorbed tab's
        // position: it already has a slot the user arranged, and moving it
        // would rewrite an arrangement for the same reason the pin rule
        // exists. That path never calls this function; closing the SFTP
        // tab drops its entry on its own. Pinning the asymmetry here so a
        // future caller does not "simplify" it into always inheriting.
        let (dest, sftp) = (Uuid::from_u128(1), Uuid::from_u128(3));
        let order = vec![
            TabRef::Sftp(sftp),
            TabRef::Terminal(Uuid::from_u128(2)),
            TabRef::Terminal(dest),
        ];
        let mut inherited = order.clone();
        morph_order_slot(&mut inherited, sftp, dest);
        assert_eq!(
            inherited.first(),
            Some(&TabRef::Terminal(dest)),
            "inheriting moves the destination to the absorbed tab's slot, \
             which is right only when the destination was just born"
        );
    }

    // -- Reopening a dormant pinned tab keeps its slot ------------------

    #[test]
    fn a_reopened_dormant_tab_returns_to_its_own_slot() {
        // The state `reopen_dormant_tab` actually faces after the nested
        // update: the placeholder's ref is ALREADY gone (its tab was
        // removed before the reopen, so `reconcile_tab_order` dropped it)
        // and the live tab's ref was appended at the end. Renaming, which
        // is what the code did before, finds nothing and leaves the tab
        // at the end of its pin partition.
        let (a, dormant, b) = (Uuid::from_u128(1), Uuid::from_u128(3), Uuid::from_u128(2));
        let live = Uuid::from_u128(9);
        let mut order = vec![
            TabRef::Terminal(a),
            TabRef::Terminal(b),
            TabRef::Terminal(live),
        ];
        // The placeholder had been at index 1, captured before the remove.
        restore_order_slot(&mut order, dormant, live, Some(1));
        assert_eq!(
            order,
            vec![
                TabRef::Terminal(a),
                TabRef::Terminal(live),
                TabRef::Terminal(b),
            ],
            "the reopened tab goes back where the user put it"
        );
    }

    #[test]
    fn restoring_a_slot_leaves_exactly_one_ref() {
        // The other order of events: the placeholder's ref is still there
        // (nothing reconciled yet). Both must not survive.
        let (dormant, live) = (Uuid::from_u128(3), Uuid::from_u128(9));
        let mut order = vec![
            TabRef::Terminal(dormant),
            TabRef::Sftp(Uuid::from_u128(5)),
            TabRef::Terminal(live),
        ];
        restore_order_slot(&mut order, dormant, live, Some(0));
        assert_eq!(
            order,
            vec![
                TabRef::Terminal(live),
                TabRef::Sftp(Uuid::from_u128(5)),
            ],
            "one entry for the reopened tab, at the placeholder's index"
        );
    }

    #[test]
    fn a_placeholder_with_no_slot_appends() {
        let (dormant, live) = (Uuid::from_u128(3), Uuid::from_u128(9));
        let mut order = vec![TabRef::Terminal(Uuid::from_u128(1))];
        restore_order_slot(&mut order, dormant, live, None);
        assert_eq!(
            order,
            vec![TabRef::Terminal(Uuid::from_u128(1)), TabRef::Terminal(live)]
        );
    }

    #[test]
    fn a_tab_that_was_never_in_the_order_appends() {
        let new = Uuid::from_u128(9);
        let mut order = vec![TabRef::Terminal(Uuid::from_u128(1))];
        morph_order_slot(&mut order, Uuid::from_u128(404), new);
        assert_eq!(
            order,
            vec![TabRef::Terminal(Uuid::from_u128(1)), TabRef::Terminal(new)],
            "no slot to inherit falls back to where a new tab lands"
        );
    }
}

