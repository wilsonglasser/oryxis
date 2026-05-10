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

use iced::{Element, Length};

use crate::app::{Message, Oryxis};
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn view_dashboard(&self) -> Element<'_, Message> {
        let main_content = self.dashboard_main_content();

        // Panel priority order: discovery > dynamic-group editor >
        // host editor. Only one shows at a time, they all live in the
        // same right-hand slot.
        if self.cloud_discover_visible {
            let panel = self.view_cloud_discover_panel();
            dir_row(vec![main_content, panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.cloud_dynamic_form_visible {
            let panel = self.view_dynamic_group_form_panel();
            dir_row(vec![main_content, panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.show_host_panel {
            let panel = self.view_host_panel();
            dir_row(vec![main_content, panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content
        }
    }
}
