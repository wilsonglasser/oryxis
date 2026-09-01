//! Pointer enter / leave for every card list.
//!
//! One arm per surface, all of them writing the same `HoverState`. They
//! are here rather than with their views because the floating action
//! icons are a convention (CLAUDE.md), so the shape is identical
//! everywhere and worth reading in one place.

use super::*;

/// How long the pointer has to rest on a chip before its close X takes
/// the badge's place.
///
/// Long enough that crossing the strip on the way to another tab never
/// materializes one under the cursor, short enough that aiming at the X
/// does not feel like waiting for permission. A deliberate pass over a
/// chip is well past this; a fast switch is well under it.
const CLOSE_DWELL: std::time::Duration = std::time::Duration::from_millis(300);

/// After a close clicked in the strip, chips arm the moment they take the
/// hover for this long. Closing several tabs in a row is one gesture, and
/// the strip is the thing moving during it: the cursor stays put while
/// tab after tab slides underneath, so charging a fresh dwell for each
/// would tax exactly the case the streak exists to serve.
const CLOSE_STREAK_GRACE: std::time::Duration = std::time::Duration::from_millis(1200);

impl Oryxis {
    /// A strip chip took the hover: start (or skip) its close-X dwell.
    ///
    /// Every chip enter goes through this, including the SFTP chips that
    /// show their X unconditionally, because the episode counter is what
    /// retires the dwell of the chip the cursor just left.
    pub(crate) fn arm_tab_close_dwell(&mut self) -> Task<Message> {
        let seq = self.hover.begin_tab_hover();
        if closing_streak(self.hover.tab_close_click_at) {
            self.hover.tab_close_armed = true;
            return Task::none();
        }
        Task::perform(
            async move { tokio::time::sleep(CLOSE_DWELL).await },
            move |()| Message::Tabs(TabsMessage::TabCloseDwell(seq)),
        )
    }

    pub(super) fn handle_tabs_hover(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            TabsMessage::CardHovered(idx) => {
                self.hover.card = Some(idx);
            }
            TabsMessage::CardUnhovered(idx) => {
                self.hover.leave_card(idx);
            }
            TabsMessage::FolderCardHovered(gid) => {
                self.hover.folder_card = Some(gid);
            }
            TabsMessage::FolderCardUnhovered(gid) => {
                self.hover.leave_folder_card(gid);
            }
            TabsMessage::KeyCardHovered(idx) => {
                self.hover.key_card = Some(idx);
            }
            TabsMessage::KeyCardUnhovered(idx) => {
                self.hover.leave_key_card(idx);
            }
            TabsMessage::IdentityCardHovered(idx) => {
                self.hover.identity_card = Some(idx);
            }
            TabsMessage::IdentityCardUnhovered(idx) => {
                self.hover.leave_identity_card(idx);
            }
            TabsMessage::SnippetCardHovered(idx) => {
                self.hover.snippet_card = Some(idx);
            }
            TabsMessage::SnippetCardUnhovered(idx) => {
                self.hover.leave_snippet_card(idx);
            }
            TabsMessage::PanelTabHovered(kind) => {
                self.hover.panel_tab = Some(kind);
                return self.arm_tab_close_dwell();
            }
            TabsMessage::PanelTabUnhovered(kind) => {
                // Crossing from one chip to the next publishes the
                // arriving chip's enter first, so the clear has to name
                // the chip it is leaving (the card-action convention).
                self.hover.leave_panel_tab(kind);
                if !self.hover.any_tab_chip() {
                    self.hover.tab_close_armed = false;
                }
            }
            TabsMessage::TabCloseDwell(seq) => {
                // Still the same hover episode, and a chip is still under
                // the cursor: the pointer rested long enough to mean it.
                if seq == self.hover.tab_hover_seq && self.hover.any_tab_chip() {
                    self.hover.tab_close_armed = true;
                }
            }
            TabsMessage::TabHovered(idx) => {
                self.hover.tab = Some(idx);
                // Terminal / SFTP hover are mutually exclusive (one cursor).
                self.hover.sftp_tab = None;
                // Live-slide: while a drag is active, entering another tab in
                // the same group slides the dragged tab into that slot right
                // away. Stable because after the move the dragged tab sits
                // under the cursor, so it won't re-trigger until the cursor
                // crosses into a genuinely different tab.
                if let Some(drag) = self.tab_drag.filter(|d| d.active)
                    && let Some(target) = self.tabs.get(idx).map(|t| t._id)
                    && drag.from_id != target
                {
                    // Reorders `tab_order` (display) only; storage vecs and the
                    // active pointers are untouched. Same-partition guard is in
                    // `slide_tab_in_order`.
                    self.slide_tab_in_order(drag.from_id, target);
                }
                return self.arm_tab_close_dwell();
            }
            TabsMessage::TabUnhovered(idx) => {
                self.hover.leave_tab(idx);
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}

/// Whether a close streak is still running, i.e. the last close X click
/// is recent enough that the chip now taking the hover should show its
/// own X on arrival instead of waiting out a dwell.
///
/// The pure half of that decision, so the window is testable without an
/// `Oryxis` and without a clock the test has to wait on.
fn closing_streak(last_click: Option<std::time::Instant>) -> bool {
    last_click.is_some_and(|t| t.elapsed() < CLOSE_STREAK_GRACE)
}

#[cfg(test)]
mod tests {
    use super::{closing_streak, CLOSE_STREAK_GRACE};
    use std::time::Instant;

    /// Closing several tabs in a row is one gesture: the strip slides the
    /// next chip under a cursor that never moved, so it arrives armed.
    /// Once the streak lapses the dwell is back in charge, which is what
    /// keeps a close from arming a chip the user reaches minutes later.
    #[test]
    fn a_close_arms_the_next_chip_only_while_the_streak_lasts() {
        assert!(!closing_streak(None), "no close yet, so no streak");
        assert!(closing_streak(Some(Instant::now())));

        let lapsed = Instant::now()
            .checked_sub(CLOSE_STREAK_GRACE * 2)
            .expect("the process has been up long enough to subtract from");
        assert!(!closing_streak(Some(lapsed)));
    }
}
