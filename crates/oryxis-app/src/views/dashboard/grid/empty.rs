//! Dashboard grid: empty state. Split out of views/dashboard/grid/mod.rs.

use super::*;
use iced::widget::column;
impl Oryxis {
    /// Centered empty state shown when no hosts/groups/session groups exist.
    pub(crate) fn dashboard_empty_state(&self) -> Element<'_, Message> {
        let toolbar = self.dashboard_toolbar();
        let search_bar: Element<'_, Message> = Space::new().height(0).into();
        let status: Element<'_, Message> = Space::new().height(0).into();
        // Termius-style empty state, centered "Create host" with input
        let has_input = !self.quick_host_input.is_empty();
        let btn_bg = if has_input { OryxisColors::t().success } else { OryxisColors::t().bg_surface };

        let empty_state = container(
            column![
                // Icon
                container(
                    iced_fonts::lucide::server().size(32).color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(12.0), ..Default::default() },
                    ..Default::default()
                }),
                Space::new().height(20),
                text(crate::i18n::t("create_host_title")).size(20).color(OryxisColors::t().text_primary),
                Space::new().height(8),
                text(crate::i18n::t("create_host_desc"))
                    .size(13).color(OryxisColors::t().text_muted),
                Space::new().height(24),
                // Hostname input
                text_input(t("type_ip_or_hostname"), &self.quick_host_input)
                    .on_input(Message::QuickHostInput)
                    .on_submit(Message::QuickHostContinue)
                    .padding(14)
                    .width(380)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(12),
                // Continue button
                button(
                    container(text(crate::i18n::t("continue_btn")).size(14).color(OryxisColors::t().text_primary))
                        .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                        .width(380)
                        .center_x(380),
                )
                .on_press(Message::QuickHostContinue)
                .width(380)
                .style(move |_, _| button::Style {
                    background: Some(Background::Color(btn_bg)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            ]
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill);

        let main_content = column![toolbar, search_bar, status, empty_state]
            .width(Length::Fill)
            .height(Length::Fill);

        main_content.into()
    }
}
