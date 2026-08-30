//! Dashboard grid: empty state. Split out of views/dashboard/grid/mod.rs.

use super::*;
use iced::widget::column;

/// Width of the whole centered block (input, Continue, the "or"
/// divider and every secondary action), so the column reads as one
/// object instead of a stack of differently sized parts.
const BLOCK_WIDTH: f32 = 380.0;

impl Oryxis {
    /// Centered empty state shown when no hosts/groups/session groups exist.
    ///
    /// No toolbar here: with an empty vault there is nothing to search,
    /// sort, filter or re-layout, and the add menu's entries render as
    /// real buttons below instead (same catalog, `add_host_actions`).
    /// Matches the other empty vault views (see `view_history`).
    pub(crate) fn dashboard_empty_state(&self) -> Element<'_, Message> {
        // Termius-style empty state, centered "Create host" with input
        let has_input = !self.quick_host_input.is_empty();
        let btn_bg = if has_input { OryxisColors::t().success } else { OryxisColors::t().bg_surface };
        // An explicit connect target (user@, a port, an IP literal)
        // makes Enter / the button quick-connect directly instead of
        // opening the editor (issue #97, see `QuickHostContinue`), so
        // the button must say so instead of lying with "Continue".
        let connects_directly = oryxis_core::ssh_target::SshTarget::parse(
            self.quick_host_input.trim(),
        )
        .is_some_and(|t| t.is_explicit())
            && self.quick_connect_target(self.quick_host_input.trim()).is_some();

        // The toolbar's recording is from a previous frame (a host was
        // just deleted); the toolbar isn't on screen, so drop it and
        // blank the anchor cells its dropdowns would point at. The
        // content zone was cleared by the caller and is re-recorded
        // below, in display order.
        self.keynav_toolbar_reset();
        self.keynav_toolbar_zero_trigger_bounds();

        let mut items: Vec<Element<'_, Message>> = vec![
            // Icon
            container(iced_fonts::lucide::server().size(32).color(OryxisColors::t().text_muted))
                .padding(16)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(12.0), ..Default::default() },
                    ..Default::default()
                })
                .into(),
            Space::new().height(20).into(),
            text(crate::i18n::t("create_host_title"))
                .size(20)
                .color(OryxisColors::t().text_primary)
                .into(),
            Space::new().height(8).into(),
            text(crate::i18n::t("create_host_desc")).size(13).color(OryxisColors::t().text_muted).into(),
            Space::new().height(24).into(),
            // Hostname input. Enter on its keyboard row focuses it (the
            // id), typing then submits with the same Enter.
            self.content_action_slot(
                crate::keynav::RowAction::input(iced::widget::Id::new(QUICK_HOST_INPUT_ID)),
                8.0,
                text_input(t("type_ip_or_hostname"), &self.quick_host_input)
                    .id(QUICK_HOST_INPUT_ID)
                    .on_input(|v| Message::Navigation(NavigationMessage::QuickHostInput(v)))
                    // No submit binding while a side panel is open: the
                    // fork's `text_input` fires on_submit on ANY Enter,
                    // focused or not, and the empty state stays mounted
                    // BEHIND the host editor, so an Enter meant for the
                    // editor also landed here and rebuilt its form (the
                    // dispatcher's modal guard cannot help, the message
                    // itself must not exist). The click path is
                    // unaffected: the Continue button below keeps its
                    // own on_press.
                    .on_submit_maybe(
                        (!self.side_panel_open())
                            .then(|| Message::Navigation(NavigationMessage::QuickHostContinue)),
                    )
                    .padding(14)
                    .width(BLOCK_WIDTH)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(12).into(),
            // Continue button
            self.content_action_slot(
                crate::keynav::RowAction::activate(Message::Navigation(NavigationMessage::QuickHostContinue)),
                8.0,
                button(
                    container(
                        text(crate::i18n::t(if connects_directly {
                            "connect"
                        } else {
                            "continue_btn"
                        }))
                        .size(14)
                        .color(OryxisColors::t().text_primary),
                    )
                    .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                    .width(BLOCK_WIDTH)
                    .center_x(BLOCK_WIDTH),
                )
                .on_press(Message::Navigation(NavigationMessage::QuickHostContinue))
                .width(BLOCK_WIDTH)
                .style(move |_, status| {
                    // Hover / press lift the fill a step in both states
                    // (idle surface and the input-filled success fill).
                    let bg = match status {
                        BtnStatus::Hovered | BtnStatus::Pressed if has_input => {
                            OryxisColors::t().accent_hover
                        }
                        BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                        _ => btn_bg,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .into(),
            ),
        ];

        // Secondary paths: the "+ Host ▾" menu's own entries, as
        // buttons. A dropdown on an otherwise blank screen hides the
        // only other ways in (import) behind a chevron the first-run
        // user has no reason to click.
        let actions = self.add_host_actions();
        if !actions.is_empty() {
            items.push(Space::new().height(24).into());
            items.push(crate::views::add_actions::or_divider(BLOCK_WIDTH));
            items.push(Space::new().height(16).into());
            for action in actions {
                items.push(self.content_action_slot(
                    crate::keynav::RowAction::activate(action.msg.clone()),
                    8.0,
                    crate::views::add_actions::secondary_action_button(action, BLOCK_WIDTH),
                ));
                items.push(Space::new().height(8).into());
            }
        }

        let empty_state = container(column(items).align_x(iced::Alignment::Center)).center(Length::Fill);

        column![empty_state].width(Length::Fill).height(Length::Fill).into()
    }
}

/// Text-input id of the quick-host field, shared by the widget and its
/// keyboard row (`RowAction::input` focuses by id).
const QUICK_HOST_INPUT_ID: &str = "empty-quick-host";


