//! Reopen the last closed tab (issue #186): the capture side that every
//! user-driven close calls, and the `ReopenClosedTab` handler that brings
//! one back.
//!
//! Closing a tab is one click on a small target sitting between two other
//! small targets, and the session behind it is not recoverable by
//! undoing anything else. The browsers' answer is a stack of recently
//! closed tabs, and it works here because the app already has a
//! serializable "how do I recreate this tab" value: the pin spec. So a
//! reopen is not a second restore mechanism, it is the pin's resolution
//! (`spec_open_message`) reached from a different door.

use iced::Task;

use crate::app::{Message, Oryxis, SftpMessage};
use crate::state::ClosedTab;

/// How many closed tabs the stack keeps.
///
/// Deep enough to cover the mistake this exists for (a misclick noticed a
/// few tabs later) and the "close others" that drops a screenful at once,
/// shallow enough that it stays a recent-history list rather than a
/// session archive nobody prunes.
const CLOSED_TABS_MAX: usize = 10;

impl Oryxis {
    /// Remember the terminal tab at `idx` so it can be reopened.
    ///
    /// Called from the paths where the USER closed a tab, never from
    /// `teardown_tab_at`: the reconnect rebuild tears a tab down to put an
    /// equivalent one back in the same breath, and an entry minted there
    /// would offer the user a tab they never closed. The three sites are
    /// `close_tab_now`, `CloseOtherTabs` and `CloseAllTabs`.
    pub(crate) fn remember_closed_tab(&mut self, idx: usize) {
        let Some(tab) = self.tabs.get(idx) else { return };
        // No spec, no entry: `pin_spec` is what already answers "can this
        // tab be described well enough to recreate it", and its `None`
        // arms (quick-connect, SSM) are exactly the ones a reopen could
        // not honour either. See `ClosedTab`.
        //
        // It reads the FOCUSED pane, so a split tab comes back as a
        // single pane on that host. Same answer pinning gives a split,
        // and the honest one: the panes are separate live sessions, not
        // a layout that can be re-dialled as a unit.
        let Some(spec) = tab.pin_spec() else { return };
        let id = tab._id;
        let after_id = self.chip_to_the_left_of(id);
        self.push_closed_tab(ClosedTab { spec, after_id });
    }

    /// The SFTP half. Same rule, different storage vec: called from the
    /// user close paths in `dispatch_sftp::tabs`, not from
    /// `close_sftp_tab` itself, because the "Open terminal" morph closes
    /// the SFTP tab as its last step and that tab did not die, it became
    /// the terminal tab beside it.
    pub(crate) fn remember_closed_sftp_tab(&mut self, idx: usize) {
        let Some(spec) = self.sftp_pin_spec(idx) else { return };
        let Some(id) = self.sftp_tabs.get(idx).map(|t| t.id) else { return };
        let after_id = self.chip_to_the_left_of(id);
        self.push_closed_tab(ClosedTab { spec, after_id });
    }

    /// Strip id of the chip immediately before `id`, or `None` when it is
    /// the first one (or not in the strip at all, which reads the same:
    /// nothing to come back next to).
    fn chip_to_the_left_of(&self, id: uuid::Uuid) -> Option<uuid::Uuid> {
        let at = self.tab_order.iter().position(|r| r.strip_id() == id)?;
        self.tab_order.get(at.checked_sub(1)?).map(|r| r.strip_id())
    }

    fn push_closed_tab(&mut self, entry: ClosedTab) {
        push_closed(&mut self.closed_tabs, entry);
    }

    /// Bring back the most recently closed tab.
    ///
    /// Pops until one resolves rather than failing on the newest: a host
    /// deleted since it was closed can never be reopened, and stopping
    /// there would wedge the whole stack behind a dead entry with nothing
    /// on screen saying why. An empty stack is a no-op, like every
    /// browser's.
    pub(super) fn handle_reopen_closed_tab(&mut self) -> Task<Message> {
        use crate::state::PinnedTabSpec;
        // Four menus reach this now (the tab chip's, the SFTP chip's,
        // the strip's own and the `+` popover), and the row that fires
        // it is the last thing they have to say. The hotkey path clears
        // nothing that was open.
        self.overlay = None;
        while let Some(entry) = self.closed_tabs.pop() {
            // An SFTP tab is recreated here rather than through
            // `spec_open_message`: it lives in `sftp_tabs`, and its
            // reopen IS the dormant-pin path, a chip that re-mounts its
            // panes when it is first selected.
            //
            // This test has to stay ABOVE the `spec_open_message` call,
            // whose `Sftp` arm answers `None` (that path only ever
            // produces terminal tabs). Below it, every SFTP entry would
            // fall into the `continue` meant for a deleted host and be
            // eaten without a chip to show for it.
            if matches!(entry.spec, PinnedTabSpec::Sftp { .. }) {
                return self.reopen_closed_sftp_tab(entry);
            }
            let open = self.spec_open_message(&entry.spec);
            let Some(open) = open else { continue };
            // Where the chip goes back, through the one door every new
            // tab walks (`place_new_tab_ref`). It resolves the neighbour
            // fresh, so a neighbour that closed in the meantime degrades
            // to appending instead of guessing a stale index.
            self.arm_reopen_placement(entry.after_id);
            return self.update(open);
        }
        Task::none()
    }

    fn reopen_closed_sftp_tab(&mut self, entry: ClosedTab) -> Task<Message> {
        // Dormant, exactly like a pinned SFTP tab restored at boot: the
        // panes re-mount on the first select rather than here, so one
        // path owns the mount and the reopen cannot drift from it.
        let label = entry.spec.label().to_string();
        let tab = crate::state::SftpTab::new_dormant(label, entry.spec);
        let id = tab.id;
        self.sftp_tabs.push(tab);
        let idx = self.sftp_tabs.len() - 1;
        // Placed by hand: `reconcile_tab_order` appends SFTP refs, and
        // only terminal tabs go through the placement door. Same three
        // answers that door gives, so the two kinds come back to the same
        // place.
        let at = match entry.after_id {
            // It was the first chip, so it comes back as the first chip.
            None => 0,
            Some(a) => self
                .tab_order
                .iter()
                .position(|r| r.strip_id() == a)
                .map(|p| p + 1)
                // The neighbour closed in the meantime: the end is where
                // a new tab would have gone anyway.
                .unwrap_or(self.tab_order.len()),
        };
        self.tab_order
            .insert(at.min(self.tab_order.len()), crate::state::TabRef::Sftp(id));
        self.update(Message::Sftp(SftpMessage::SelectSftpTab(idx)))
    }

    /// Arm the strip placement for the tab a reopen is about to spawn.
    ///
    /// Not `arm_tab_placement`, which reads the `duplicate_tab_position`
    /// setting: a reopen has an answer of its own, the slot the tab held
    /// when it left.
    fn arm_reopen_placement(&mut self, after: Option<uuid::Uuid>) {
        use crate::state::{PendingTabPlacement, TabPlacement};
        let placement = match after {
            Some(_) => TabPlacement::NextToOriginal,
            // It was the first chip, so it comes back as the first chip.
            None => TabPlacement::Start,
        };
        self.pending_tab_placement = Some(PendingTabPlacement {
            // Unread under `Start`, which needs no anchor.
            source_id: after.unwrap_or_default(),
            placement,
            armed_at: std::time::Instant::now(),
        });
    }
}

/// Pure half of [`Oryxis::push_closed_tab`], so the cap is testable
/// without an `Oryxis`.
fn push_closed(stack: &mut Vec<ClosedTab>, entry: ClosedTab) {
    stack.push(entry);
    // Oldest first out, so the cap never costs the user the tab they just
    // closed.
    if stack.len() > CLOSED_TABS_MAX {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{push_closed, CLOSED_TABS_MAX};
    use crate::state::{ClosedTab, PinnedTabSpec};

    fn entry(n: u128) -> ClosedTab {
        ClosedTab {
            spec: PinnedTabSpec::Host {
                id: uuid::Uuid::from_u128(n),
                label: format!("host-{n}"),
            },
            after_id: None,
        }
    }

    /// The cap drops the OLDEST entry. Closing eleven tabs must not cost
    /// the eleventh, which is the one the user is about to ask back.
    #[test]
    fn the_cap_drops_the_oldest_not_the_newest() {
        let mut stack: Vec<ClosedTab> = Vec::new();
        for n in 0..(CLOSED_TABS_MAX as u128 + 1) {
            push_closed(&mut stack, entry(n));
        }

        assert_eq!(stack.len(), CLOSED_TABS_MAX);
        let newest = match &stack.last().unwrap().spec {
            PinnedTabSpec::Host { id, .. } => *id,
            _ => unreachable!(),
        };
        assert_eq!(newest, uuid::Uuid::from_u128(CLOSED_TABS_MAX as u128));
        let oldest = match &stack.first().unwrap().spec {
            PinnedTabSpec::Host { id, .. } => *id,
            _ => unreachable!(),
        };
        assert_eq!(oldest, uuid::Uuid::from_u128(1), "entry 0 was dropped");
    }
}
