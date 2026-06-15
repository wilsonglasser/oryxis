//! Dashboard view, the folders + hosts grid (the main screen of the
//! app). Split into submodules per chunk so each file stays focused:
//!
//! - `toolbar`: breadcrumb + back + per-folder action button.
//! - `grid`: the host / folder / dynamic-group cards grid (also
//!   handles the empty-state and dynamic-group early-returns).
//!
//! The orchestrator below glues `dashboard_main_content` together
//! with the right-side panel slot (host editor / discovery /
//! dynamic-group editor).

mod grid;
mod toolbar;

use iced::Element;

use crate::app::{Message, Oryxis};

impl Oryxis {
    pub(crate) fn view_dashboard(&self) -> Element<'_, Message> {
        // The side panel (discovery / dynamic-group / host / session-group
        // editor) is hoisted to `view_main::active_side_panel` so it can
        // rise over the sub-nav. Here we only build the main content; the
        // `available`-width math in `dashboard_main_content` still subtracts
        // the panel width when one is open.
        self.dashboard_main_content()
    }
}
