//! Dropping a dragged tab into the displayed tab's pane grid: the app
//! side of the split anchors (issue #112). `crate::pane_drop` answers
//! *where*; this answers *whether* and *what happens*.
//!
//! The gesture reads as "pull this session in beside the one I'm looking
//! at", and it works out because iced's `button` publishes `on_press` on
//! the RELEASE (`button.rs`, `ButtonReleased`), not the press: grabbing a
//! chip therefore does not select it, so the content area keeps showing
//! the tab that was already active. That tab is the destination, the
//! grabbed one is the source, and the whole thing needs no
//! remember-and-restore of the selection.

use iced::widget::pane_grid::{Axis, Edge, Pane as PaneHandle, Region, Target};
use iced::{Rectangle, Size};

use crate::app::Oryxis;
use crate::pane_drop::DropProposal;

impl Oryxis {
    /// Whether the cursor is inside the tab strip's band, for the strip's
    /// current dock. The strip owns the gesture while the cursor is over
    /// it (that is the reorder half of the drag), so this is what keeps
    /// the two halves from both claiming a frame.
    ///
    /// Known asymmetry on a BOTTOM dock: that band carries 40 px of slack
    /// past the strip (it has to clear the status bar, see
    /// `cursor_in_tab_strip_band`), which overlaps the grid's own bottom
    /// anchor. So on a bottom-docked strip `Target::Edge(Bottom)` is hard
    /// to reach and may not be reachable at all. Deliberately left that
    /// way: the strip winning is the safe direction (the worst case is a
    /// missing anchor, never a drop somewhere the preview didn't promise),
    /// and every other anchor, including the bottom pane's own bottom
    /// third, still works.
    pub(crate) fn cursor_in_tab_strip(&self) -> bool {
        crate::views::tab_bar::cursor_in_tab_strip_band(
            crate::views::tab_bar::tab_bar_pos(),
            self.mouse_position,
            self.window_size,
            self.prefs.pinned_tabs_top_bar && !self.prefs.side_hide_top_bar,
        )
    }

    /// The destination tab and the drop the cursor is proposing right
    /// now, or `None` when the gesture proposes nothing.
    ///
    /// Read twice per gesture from the same inputs: once by `view()` to
    /// paint the preview, once by the release handler to perform the
    /// drop. A release doesn't move the cursor, so the two agree by
    /// construction, and no extra state has to be kept in sync.
    pub(crate) fn tab_drop_proposal(&self) -> Option<(usize, DropProposal)> {
        let drag = self.tab_drag.filter(|d| d.active)?;
        if self.cursor_in_tab_strip() {
            return None;
        }
        let dest_idx = self.active_tab?;
        // The connect screen replaces the grid for its own tab, so the
        // panes' reported rects are whatever they were before it took
        // over: hit-testing them would target a grid nobody can see.
        if self.connecting.as_ref().is_some_and(|c| c.tab_idx == dest_idx) {
            return None;
        }
        let dest = self.tabs.get(dest_idx)?;
        // Files mode swaps the grid out for the SFTP surface: same
        // problem, no pane layout on screen to drop onto.
        if dest.files_mode {
            return None;
        }
        // Merging a tab into itself has no meaning: the chip already
        // stands for every pane it would "gain".
        if dest._id == drag.from_id {
            return None;
        }
        let source = self.tabs.iter().find(|t| t._id == drag.from_id)?;
        // A dormant pin is a placeholder carrying a reopen spec, not a
        // session; there is nothing to move yet.
        if source.pending_reopen.is_some() {
            return None;
        }
        // A tab mid-connect is addressed by INDEX (`connecting.tab_idx`)
        // and its pane has no session yet. Let the dial finish.
        if self
            .connecting
            .as_ref()
            .and_then(|c| self.tabs.get(c.tab_idx))
            .is_some_and(|t| t._id == source._id)
        {
            return None;
        }
        let rects: Vec<(PaneHandle, Rectangle)> = dest
            .pane_grid
            .panes
            .iter()
            .map(|(handle, pane)| (*handle, pane.bounds.get()))
            .collect();
        let proposal = crate::pane_drop::drop_target_at(&rects, self.mouse_position)?;
        Some((dest_idx, proposal))
    }

    /// Perform the proposed drop: move every pane of the dragged tab into
    /// the displayed tab's grid and retire the source chip.
    ///
    /// Called from the global mouse-release handler; a no-op when the
    /// cursor proposes nothing, so a drag that ends over the strip (or
    /// in a pane's neutral middle) still falls through to the reorder
    /// path untouched. Consuming the drag on success is what tells that
    /// path to stand down.
    pub(crate) fn merge_dragged_tab_if_proposed(&mut self) {
        let Some((dest_idx, proposal)) = self.tab_drop_proposal() else {
            return;
        };
        let Some(drag) = self.tab_drag.take().filter(|d| d.active) else {
            return;
        };
        let Some(src_idx) = self.tabs.iter().position(|t| t._id == drag.from_id) else {
            return;
        };
        // Index-addressed state shifts under the removal below, so pin
        // the destination by id and re-find it afterwards.
        let dest_id = self.tabs[dest_idx]._id;

        // Buffered output belongs to the sessions, which survive: flush
        // before the tab that owns their log bookkeeping goes away.
        self.flush_session_logs_final();
        // The source tab's AI chat dies with its chip. Abort the stream
        // first: a detached tool-followup pipeline would otherwise keep
        // polling a terminal that has moved into another tab, and keep
        // calling the model to do it.
        self.abort_chat_task_for(drag.from_id);

        let source = self.tabs.remove(src_idx);
        // Every pane travels, in the order it was laid out, so the group
        // the user assembled reads left-to-right the way it looked.
        let panes = detach_panes_in_layout_order(source.pane_grid);
        let was_pinned = source.pinned;

        // Same index bookkeeping a close does, minus everything that
        // would tear the sessions down (`close_tab_sessions`,
        // `monitor_reset_host`): the panes are moving, not dying.
        if let Some(ref mut progress) = self.connecting {
            match progress.tab_idx.cmp(&src_idx) {
                std::cmp::Ordering::Equal => self.connecting = None,
                std::cmp::Ordering::Greater => progress.tab_idx -= 1,
                std::cmp::Ordering::Less => {}
            }
        }
        self.adjust_last_terminal_tab_after_remove(src_idx);
        if self.hybrid_sftp_owner == Some(drag.from_id) {
            self.hybrid_sftp_owner = None;
            self.sftp = crate::state::SftpState::default();
        }

        let Some(dest_idx) = self.tabs.iter().position(|t| t._id == dest_id) else {
            return;
        };
        let tab = &mut self.tabs[dest_idx];
        let landed = insert_panes(&mut tab.pane_grid, proposal.target, panes);
        if let Some(first) = landed.first() {
            tab.focused = *first;
        }
        self.active_tab = Some(dest_idx);
        self.remember_terminal_tab_focus(dest_idx);
        if was_pinned {
            self.persist_pinned_tabs();
        }
        // Quick-connect credentials are kept alive by the panes that
        // reference them; the panes moved, so nothing should be pruned,
        // but run it for the same reason a close does: it is the one
        // place that decides.
        self.prune_quick_connects();
    }
}

/// Take every pane out of a grid, ordered the way it was laid out
/// (top-to-bottom, then left-to-right).
///
/// `State::panes` is keyed by creation order, which stops matching the
/// arrangement as soon as anything is closed or swapped, so the order
/// comes from the layout itself. `State::close` can't be used to drain
/// the last pane (it has no sibling to promote), and the grid is being
/// discarded anyway, so the map is consumed directly.
fn detach_panes_in_layout_order(
    grid: iced::widget::pane_grid::State<crate::state::Pane>,
) -> Vec<crate::state::Pane> {
    // The size is arbitrary: only the relative positions are read.
    let regions = grid.layout().pane_regions(0.0, 0.0, Size::new(4096.0, 4096.0));
    let mut order: Vec<(PaneHandle, Rectangle)> = regions.into_iter().collect();
    order.sort_by(|(_, a), (_, b)| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    let mut panes = grid.panes;
    let mut out: Vec<crate::state::Pane> = order
        .iter()
        .filter_map(|(handle, _)| panes.remove(handle))
        .collect();
    // Anything the layout didn't mention (it never happens, but the map
    // is the authority on what exists) still travels rather than being
    // silently dropped with its live session.
    out.extend(std::mem::take(&mut panes).into_values());
    out
}

/// Insert `panes` into `grid` at `target`, returning their handles in
/// the order they landed.
///
/// The first pane goes where the preview said; the rest chain off it
/// along the same axis, each one splitting the previously inserted pane
/// on its trailing side. Chaining forwards (rather than repeating the
/// target) is what keeps a multi-pane source in its original
/// left-to-right order even when it lands on a leading edge.
fn insert_panes(
    grid: &mut iced::widget::pane_grid::State<crate::state::Pane>,
    target: Target,
    panes: Vec<crate::state::Pane>,
) -> Vec<PaneHandle> {
    let mut iter = panes.into_iter();
    let Some(first) = iter.next() else {
        return Vec::new();
    };
    let edge = match target {
        Target::Edge(edge) => edge,
        Target::Pane(_, Region::Edge(edge)) => edge,
        // `Region::Center` is never proposed (see `pane_drop`), and
        // answering a swap for a pane that isn't in this grid is not a
        // thing, so treat it as "beside the target".
        Target::Pane(_, Region::Center) => Edge::Right,
    };
    let axis = match edge {
        Edge::Left | Edge::Right => Axis::Vertical,
        Edge::Top | Edge::Bottom => Axis::Horizontal,
    };
    // Anchor for the first insert. A grid edge has no public splitter of
    // its own (`split_node(.., None, ..)` is private), so the pane is
    // parked next to any existing one and then moved to the edge:
    // `move_to_edge` closes it and re-splits the ROOT, which is exactly
    // the operation, and nothing is drawn in between.
    let anchor = match target {
        Target::Pane(pane, _) => pane,
        Target::Edge(_) => *grid.panes.keys().next().expect("a grid always has a pane"),
    };
    let Some((mut previous, _)) = grid.split(axis, anchor, first) else {
        return Vec::new();
    };
    match target {
        Target::Edge(edge) => grid.move_to_edge(previous, edge),
        // A leading edge means the arriving pane takes the anchor's
        // place and pushes it along: `split` always appends, so swap the
        // two afterwards. This is what the widget's own private
        // `split_and_swap` does for the same regions.
        Target::Pane(pane, Region::Edge(Edge::Left | Edge::Top)) => {
            grid.swap(pane, previous);
        }
        _ => {}
    }
    let mut landed = vec![previous];
    for pane in iter {
        let Some((next, _)) = grid.split(axis, previous, pane) else {
            break;
        };
        landed.push(next);
        previous = next;
    }
    landed
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::pane_grid::State;

    fn grid(labels: &[&str]) -> State<crate::state::Pane> {
        let mut it = labels.iter();
        let first = it.next().expect("at least one pane");
        let (mut state, mut prev) = State::new(pane(first));
        for label in it {
            let (next, _) = state.split(Axis::Vertical, prev, pane(label)).expect("split");
            prev = next;
        }
        state
    }

    fn pane(label: &str) -> crate::state::Pane {
        crate::state::Pane::new(
            label.to_string(),
            std::sync::Arc::new(std::sync::Mutex::new(
                oryxis_terminal::TerminalState::new_no_pty(80, 24).expect("terminal"),
            )),
        )
    }

    /// Read the grid left-to-right the way it is laid out, so the tests
    /// assert the ARRANGEMENT rather than insertion order.
    fn labels(state: &State<crate::state::Pane>) -> Vec<String> {
        let regions = state.layout().pane_regions(0.0, 0.0, Size::new(4096.0, 4096.0));
        let mut order: Vec<_> = regions.into_iter().collect();
        order.sort_by(|(_, a), (_, b)| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        order
            .iter()
            .filter_map(|(h, _)| state.panes.get(h).map(|p| p.label.clone()))
            .collect()
    }

    /// The reporter's first case, end to end: one pane on screen, drop on
    /// its right, the arriving session sits beside it.
    #[test]
    fn a_right_edge_drop_lands_beside_the_target() {
        let mut dest = grid(&["dest"]);
        let target = *dest.panes.keys().next().expect("a pane");
        insert_panes(
            &mut dest,
            Target::Pane(target, Region::Edge(Edge::Right)),
            vec![pane("moved")],
        );
        assert_eq!(labels(&dest), ["dest", "moved"]);
    }

    /// And its mirror: a LEADING edge has to swap after the split, or
    /// the pane lands on the wrong side of the one it targeted.
    #[test]
    fn a_left_edge_drop_lands_before_the_target() {
        let mut dest = grid(&["dest"]);
        let target = *dest.panes.keys().next().expect("a pane");
        insert_panes(
            &mut dest,
            Target::Pane(target, Region::Edge(Edge::Left)),
            vec![pane("moved")],
        );
        assert_eq!(labels(&dest), ["moved", "dest"]);
    }

    /// The reporter's second case: with the grid already split, a drop on
    /// the footer spans the full width under BOTH panes, which is a root
    /// split rather than a split of either one.
    #[test]
    fn a_grid_edge_drop_spans_the_whole_grid() {
        let mut dest = grid(&["left", "right"]);
        insert_panes(&mut dest, Target::Edge(Edge::Bottom), vec![pane("moved")]);
        let regions = dest.layout().pane_regions(0.0, 0.0, Size::new(1000.0, 500.0));
        let moved = dest
            .panes
            .iter()
            .find(|(_, p)| p.label == "moved")
            .map(|(h, _)| *h)
            .expect("the moved pane");
        let rect = regions[&moved];
        assert_eq!(rect.width, 1000.0, "a root split spans the full width");
        assert_eq!(rect.y, 250.0, "and takes the bottom half");
    }

    /// A grouped source keeps its reading order, including on a leading
    /// edge, because each pane chains off the previous one instead of
    /// re-targeting the anchor.
    #[test]
    fn a_grouped_source_keeps_its_order() {
        let mut dest = grid(&["dest"]);
        let target = *dest.panes.keys().next().expect("a pane");
        insert_panes(
            &mut dest,
            Target::Pane(target, Region::Edge(Edge::Left)),
            vec![pane("a"), pane("b"), pane("c")],
        );
        assert_eq!(labels(&dest), ["a", "b", "c", "dest"]);
    }

    /// Detaching reads the LAYOUT, not the pane map: a grid whose panes
    /// were created out of visual order still hands them over the way
    /// they looked.
    #[test]
    fn detaching_follows_the_layout_not_the_creation_order() {
        let mut source = grid(&["first"]);
        let first = *source.panes.keys().next().expect("a pane");
        let (second, _) = source.split(Axis::Vertical, first, pane("second")).expect("split");
        // Swap them on screen: "second" now sits on the left, but it is
        // still the later key in the map.
        source.swap(first, second);
        let detached = detach_panes_in_layout_order(source);
        let order: Vec<&str> = detached.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(order, ["second", "first"]);
    }
}
