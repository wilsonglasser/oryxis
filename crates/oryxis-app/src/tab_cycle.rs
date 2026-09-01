//! Ctrl+Tab "switch by last use" (MRU) tab cycling, modelled on the OS
//! Alt+Tab: repeated single presses toggle the two most-recently-used tabs,
//! and holding Ctrl while pressing Tab several times walks further back
//! through the recency stack. The previewed tab is only promoted to the
//! front of the MRU when Ctrl is released, so a held run of presses walks a
//! stable snapshot instead of reshuffling the order under itself (which is
//! what makes a single repeated press bounce between just the two most
//! recent, yet two presses reach the previous-previous tab).
//!
//! Contrast with Alt+Left/Right (`CycleTabs` in `shortcuts.rs`), which steps
//! positionally through the visible strip with Home included. Ctrl+Tab is
//! MRU and covers open tabs only; Home stays reachable via Ctrl+1 and
//! Alt+arrow.

use iced::Task;

use crate::app::{Message, Oryxis};
use crate::state::TabRef;

/// An in-progress Ctrl+Tab run: a snapshot of the MRU order taken at the
/// first press plus the cursor each subsequent press advances. The live
/// `Oryxis::tab_mru` is only reordered on commit (Ctrl released), so the
/// snapshot the run walks never shifts beneath it.
#[derive(Debug, Clone)]
pub(crate) struct TabCycle {
    /// MRU-ordered tabs captured when this run started.
    order: Vec<TabRef>,
    /// Index into `order` of the currently-previewed tab.
    cursor: usize,
}

/// Initial cursor for a fresh cycle. `start` is the active tab's index in
/// the MRU order, or `None` when the current surface isn't a tab
/// (Home/Settings/...). Returns `None` when there's nothing to switch to
/// (no tabs, or the only tab is already active). Pure so the toggle /
/// walk-back behaviour is unit-testable without the keyboard plumbing.
fn cycle_start_cursor(len: usize, start: Option<usize>, forward: bool) -> Option<usize> {
    match start {
        _ if len == 0 => None,
        // Already on the only tab: nowhere to go.
        Some(_) if len == 1 => None,
        // Step off the current tab.
        Some(i) => Some(cycle_step_cursor(len, i, forward)),
        // Not currently on a tab (Home/Settings/...): jump straight to the
        // most-recently-used tab (forward) or the least (backward).
        None => Some(if forward { 0 } else { len - 1 }),
    }
}

/// Advance an existing cursor one step, wrapping around `len`. Pure.
fn cycle_step_cursor(len: usize, cursor: usize, forward: bool) -> usize {
    if forward {
        (cursor + 1) % len
    } else {
        (cursor + len - 1) % len
    }
}

/// Move `chosen` to the front of `mru`, preserving the order of the rest.
/// The commit half of a cycle, and the reorder used when a tab is focused
/// outside a cycle. Pure.
fn mru_promote(mru: &mut Vec<TabRef>, chosen: TabRef) {
    mru.retain(|x| *x != chosen);
    mru.insert(0, chosen);
}

impl Oryxis {
    /// Whether a tab reference still names a live tab (used to prune the MRU
    /// when tabs close). Distinct from `tab_ref_select_msg`, which also
    /// returns `None` for a live-but-disabled SFTP tab; such a tab should
    /// stay in the MRU (it reappears if SFTP is re-enabled). Only truly
    /// closed tabs are dropped.
    fn tab_ref_alive(&self, r: &TabRef) -> bool {
        match r {
            TabRef::Terminal(id) => self.tabs.iter().any(|t| t._id == *id),
            TabRef::Sftp(id) => self.sftp_tabs.iter().any(|t| t.id == *id),
            TabRef::Panel(kind) => self.panel_tab_open(*kind),
        }
    }

    /// Whether a tab reference names a dormant pinned placeholder
    /// (restored at boot, never opened this session). Excluded from the
    /// Ctrl+Tab walk: MRU cycling covers OPEN tabs only, and previewing
    /// a dormant tab mid-cycle would actually reopen it (`SelectTab` /
    /// `SelectSftpTab` fire the reopen), connecting a host the user
    /// never asked for. Dormant tabs stay reachable by click, Alt+arrow
    /// and Ctrl+1..9, where activation is deliberate.
    fn tab_ref_dormant(&self, r: &TabRef) -> bool {
        match r {
            TabRef::Terminal(id) => self
                .tabs
                .iter()
                .find(|t| t._id == *id)
                .is_some_and(|t| t.pending_reopen.is_some()),
            TabRef::Sftp(id) => self
                .sftp_tabs
                .iter()
                .find(|t| t.id == *id)
                .is_some_and(|t| t.pending_reopen.is_some()),
            // Never restored at boot, so it can never be a placeholder.
            TabRef::Panel(_) => false,
        }
    }

    /// The Ctrl+Tab walk order: every activatable OPEN tab,
    /// most-recently-used first. Known tabs come from `tab_mru`; any live
    /// tab not yet recorded there (e.g. one just opened this frame) is
    /// appended in visible-strip order so it's always reachable even
    /// before the MRU has seen it. Dormant pinned placeholders are
    /// skipped in both halves (see `tab_ref_dormant`).
    fn mru_tab_order(&self) -> Vec<TabRef> {
        let mut order: Vec<TabRef> = self
            .tab_mru
            .iter()
            .copied()
            .filter(|r| {
                self.tab_ref_select_msg(r).is_some() && !self.tab_ref_dormant(r)
            })
            .collect();
        for r in self.ordered_tab_refs() {
            if self.tab_ref_select_msg(&r).is_some()
                && !self.tab_ref_dormant(&r)
                && !order.contains(&r)
            {
                order.push(r);
            }
        }
        order
    }

    /// Handle one Ctrl+Tab (`forward`) / Ctrl+Shift+Tab (`!forward`) press.
    /// Starts a run on the first press (snapshotting the MRU) and advances
    /// the cursor on each later press while Ctrl stays held. Returns the
    /// `Task` that activates the previewed tab.
    pub(crate) fn cycle_mru_step(&mut self, forward: bool) -> Task<Message> {
        let cycle = match self.tab_cycle.as_mut() {
            // Continuing a held run: advance the snapshot cursor.
            Some(cycle) => {
                cycle.cursor = cycle_step_cursor(cycle.order.len(), cycle.cursor, forward);
                cycle
            }
            // Fresh run: snapshot the MRU and pick the starting cursor.
            None => {
                let order = self.mru_tab_order();
                let start = self
                    .active_tab_ref()
                    .and_then(|a| order.iter().position(|r| *r == a));
                let Some(cursor) = cycle_start_cursor(order.len(), start, forward) else {
                    return Task::none();
                };
                self.tab_cycle.insert(TabCycle { order, cursor })
            }
        };
        let target = cycle.order[cycle.cursor];
        match self.tab_ref_select_msg(&target) {
            Some(msg) => Task::done(msg),
            None => Task::none(),
        }
    }

    /// Commit an in-progress cycle: promote the previewed tab to the front
    /// of the live MRU and end the run. Idempotent (a no-op when no cycle is
    /// active), so the release path, the focus-lost path, and the reconcile
    /// self-heal can all call it freely.
    pub(crate) fn commit_tab_cycle(&mut self) {
        if let Some(cycle) = self.tab_cycle.take()
            && let Some(chosen) = cycle.order.get(cycle.cursor).copied()
        {
            mru_promote(&mut self.tab_mru, chosen);
        }
    }

    /// Run after every message (from `update`, right after
    /// `reconcile_tab_order`) to keep the MRU honest: prune closed tabs,
    /// self-heal a cycle stranded by lost focus, and promote the focused tab
    /// to the front whenever the user switches tabs by any means other than
    /// an in-progress cycle.
    pub(crate) fn reconcile_tab_mru(&mut self) {
        // `retain` needs `&mut self.tab_mru` while the predicate needs
        // `&self`; take the vec out so the two borrows don't overlap.
        let mut mru = std::mem::take(&mut self.tab_mru);
        mru.retain(|r| self.tab_ref_alive(r));
        self.tab_mru = mru;

        // A cycle run owns the MRU order until Ctrl is released. Normally the
        // Ctrl-release `ModifiersChanged` clears `control()` here and we
        // commit; if the window lost focus mid-hold and no release event
        // arrived, the same check still fires the moment any later message
        // sees Ctrl no longer held, so the MRU can't freeze.
        if self.tab_cycle.is_some() {
            if self.modifiers.control() {
                return;
            }
            self.commit_tab_cycle();
        }

        if let Some(cur) = self.active_tab_ref()
            && self.tab_mru.first() != Some(&cur)
        {
            mru_promote(&mut self.tab_mru, cur);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn t(n: u128) -> TabRef {
        TabRef::Terminal(Uuid::from_u128(n))
    }

    #[test]
    fn start_cursor_steps_off_current_tab() {
        // Forward from index 0 of five tabs lands on index 1.
        assert_eq!(cycle_start_cursor(5, Some(0), true), Some(1));
        // Backward wraps to the last tab.
        assert_eq!(cycle_start_cursor(5, Some(0), false), Some(4));
        // Forward from the last tab wraps to the first.
        assert_eq!(cycle_start_cursor(5, Some(4), true), Some(0));
    }

    #[test]
    fn start_cursor_no_op_cases() {
        // No tabs at all.
        assert_eq!(cycle_start_cursor(0, None, true), None);
        // The only tab is already active: nothing to switch to.
        assert_eq!(cycle_start_cursor(1, Some(0), true), None);
        assert_eq!(cycle_start_cursor(1, Some(0), false), None);
    }

    #[test]
    fn start_cursor_from_non_tab_surface() {
        // On Home/Settings (start = None): jump straight to the most-recent
        // tab forward, the least-recent backward.
        assert_eq!(cycle_start_cursor(3, None, true), Some(0));
        assert_eq!(cycle_start_cursor(3, None, false), Some(2));
        // A single open tab is still reachable from a non-tab surface.
        assert_eq!(cycle_start_cursor(1, None, true), Some(0));
    }

    #[test]
    fn step_cursor_wraps_both_ways() {
        assert_eq!(cycle_step_cursor(3, 0, true), 1);
        assert_eq!(cycle_step_cursor(3, 2, true), 0);
        assert_eq!(cycle_step_cursor(3, 0, false), 2);
        assert_eq!(cycle_step_cursor(3, 1, false), 0);
    }

    #[test]
    fn promote_moves_to_front_without_duplicating() {
        let mut mru = vec![t(1), t(2), t(3)];
        mru_promote(&mut mru, t(3));
        assert_eq!(mru, vec![t(3), t(1), t(2)]);
        // Promoting the already-front entry is a no-op ordering-wise.
        mru_promote(&mut mru, t(3));
        assert_eq!(mru, vec![t(3), t(1), t(2)]);
    }

    /// Requirement #1: releasing Ctrl between presses toggles only the two
    /// most-recently-used tabs.
    #[test]
    fn single_presses_toggle_two_most_recent() {
        let mut mru = vec![t(1), t(2), t(3), t(4), t(5)];

        // Active is the front tab; one forward press previews index 1, and
        // releasing Ctrl commits it to the front.
        let cursor = cycle_start_cursor(mru.len(), Some(0), true).unwrap();
        let chosen = mru[cursor];
        mru_promote(&mut mru, chosen);
        assert_eq!(mru[0], t(2));
        assert_eq!(mru[1], t(1));

        // Second single press: active is now t(2) at the front, so it toggles
        // straight back to t(1) rather than advancing to t(3).
        let cursor = cycle_start_cursor(mru.len(), Some(0), true).unwrap();
        let chosen = mru[cursor];
        mru_promote(&mut mru, chosen);
        assert_eq!(mru[0], t(1));
        assert_eq!(mru[1], t(2));
    }

    /// Requirement #2: holding Ctrl and pressing Tab twice reaches the
    /// previous-previous tab, walking a stable snapshot.
    #[test]
    fn held_presses_walk_further_back() {
        let mru = [t(1), t(2), t(3), t(4), t(5)];

        // First press off the active front tab.
        let mut cursor = cycle_start_cursor(mru.len(), Some(0), true).unwrap();
        assert_eq!(mru[cursor], t(2));
        // Second press (Ctrl still held) advances the same snapshot.
        cursor = cycle_step_cursor(mru.len(), cursor, true);
        assert_eq!(mru[cursor], t(3));
        // Third reaches t(4), confirming the snapshot doesn't reshuffle.
        cursor = cycle_step_cursor(mru.len(), cursor, true);
        assert_eq!(mru[cursor], t(4));
    }
}
