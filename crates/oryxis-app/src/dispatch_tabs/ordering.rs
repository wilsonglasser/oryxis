//! Tab-strip ordering helpers split out of `dispatch_tabs`:
//! reconcile / replace-id / live-slide reorder, and the connect-
//! progress re-anchor after a bulk tab filter.

use crate::app::Oryxis;

impl Oryxis {
    /// Give the Settings surface a strip entry (issue #120), so leaving it
    /// and coming back is one click instead of a hunt through the toolbar.
    /// Idempotent, and called from `ChangeView(Settings)`, which is the
    /// single door every entry point (gear, burger menu, hotkey, command
    /// palette, the strip entry itself) already goes through. That is why
    /// there is at most one: nothing else can mint a second.
    ///
    /// Modelled on `ensure_sftp_tab`, which does the same for `View::Sftp`.
    pub(crate) fn ensure_settings_tab(&mut self) {
        if self.settings_tab_open {
            return;
        }
        self.settings_tab_open = true;
        if !self.tab_order.contains(&crate::state::TabRef::Settings) {
            self.tab_order.push(crate::state::TabRef::Settings);
        }
    }

    /// Close the Settings tab. When Settings is the surface on screen the
    /// close has to take you somewhere, so it lands on the previously
    /// focused tab if there still is one, and on the Home dashboard
    /// otherwise. Closing it from another surface only removes the chip.
    pub(crate) fn close_settings_tab(&mut self) -> iced::Task<crate::app::Message> {
        self.settings_tab_open = false;
        self.tab_order.retain(|r| !matches!(r, crate::state::TabRef::Settings));
        self.settings_scroll.clear();
        if !(self.active_tab.is_none() && self.active_view == crate::state::View::Settings) {
            return iced::Task::none();
        }
        // Most-recently-used first, skipping the entry we just dropped.
        let fallback = self
            .tab_mru
            .iter()
            .find(|r| !matches!(r, crate::state::TabRef::Settings))
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
            // Not backed by a storage vec: `settings_tab_open` is the
            // whole existence test.
            TabRef::Settings => self.settings_tab_open,
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
    /// `connecting.tab_idx` / `pending_pane_split` index goes stale.
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
                crate::state::TabRef::Settings => false,
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
                crate::state::TabRef::Settings => false,
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

#[cfg(test)]
mod tests {
    use super::placement_index;
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
                TabRef::Settings,
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
    fn settings_can_be_the_source_of_a_placement() {
        // Not reachable from Duplicate today (Settings has no duplicate
        // action), but the strip id is synthetic and the lookup must not
        // silently miss it if one ever appears.
        let (order, ..) = strip();
        assert_eq!(
            placement_index(
                &order,
                TabPlacement::NextToOriginal,
                crate::state::SETTINGS_TAB_ID
            ),
            Some(4)
        );
    }

    #[test]
    fn an_empty_strip_appends_whatever_was_asked() {
        let empty: Vec<TabRef> = Vec::new();
        let id = Uuid::from_u128(1);
        assert_eq!(placement_index(&empty, TabPlacement::NextToOriginal, id), None);
        assert_eq!(placement_index(&empty, TabPlacement::Start, id), Some(0));
    }
}

