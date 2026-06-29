//! Dashboard grid: session cards. Split out of views/dashboard/grid/mod.rs.

use super::*;
impl Oryxis {
    /// One session-group card: primary click opens the saved arrangement;
    /// hovering reveals floating edit / delete icons (the per-card action
    /// convention). Distinct `boxes` glyph + the group's own color set it
    /// apart from host cards.
    pub(crate) fn session_group_card<'a>(
        &'a self,
        idx: usize,
        group: &'a oryxis_core::models::SessionGroup,
    ) -> (Element<'a, Message>, Color) {
        let rtl = crate::i18n::is_rtl_layout();
        let hovered = self.hovered_session_group_card == Some(idx);
        let bg_color = group
            .color
            .as_deref()
            .and_then(crate::os_icon::parse_hex_color)
            .unwrap_or_else(|| OryxisColors::t().accent);

        // Custom icon when the user picked one, else the default group glyph.
        let glyph = group
            .icon_style
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(crate::os_icon::custom_icon_glyph)
            .unwrap_or(BrandIcon::Glyph(iced_fonts::lucide::boxes()));
        let host_style = crate::widgets::resolve_host_icon_style(
            None,
            &self.setting_default_host_icon,
        );
        let icon_box = crate::widgets::host_icon(
            host_style,
            bg_color,
            &group.label,
            Some(glyph.view(18.0, Color::WHITE)),
            32.0,
        );

        let panes = count_leaves(&group.layout);
        let subtitle = format!("{} {}", panes, t("session_group_panes"));
        let label_el = text(group.label.clone())
            .size(13)
            .color(OryxisColors::t().text_primary)
            .wrapping(iced::widget::text::Wrapping::None);
        let subtitle_el = text(subtitle)
            .size(10)
            .color(OryxisColors::t().text_muted)
            .wrapping(iced::widget::text::Wrapping::None);

        let card_btn = button(
            container(
                dir_row(vec![
                    icon_box,
                    Space::new().width(8).into(),
                    iced::widget::Column::with_children(vec![
                        label_el.into(),
                        Space::new().height(2).into(),
                        subtitle_el.into(),
                    ])
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .clip(true)
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 }),
        )
        .on_press(Message::OpenSessionGroup(idx))
        .width(Length::Fill)
        .style(move |_, status| {
            let (bg, bc, bw) = match status {
                BtnStatus::Hovered => (OryxisColors::t().bg_hover, OryxisColors::t().accent, 1.5),
                BtnStatus::Pressed => (OryxisColors::t().bg_selected, OryxisColors::t().accent, 2.0),
                _ => (OryxisColors::t().bg_surface, OryxisColors::t().border, 1.0),
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(10.0), color: bc, width: bw },
                ..Default::default()
            }
        });

        // `⋮` kebab → context menu (Open / Edit / Duplicate / Delete), same
        // as the host card. Shown on hover or while this card's menu is open.
        let menu_open = matches!(
            self.overlay.as_ref().map(|o| &o.content),
            Some(crate::state::OverlayContent::SessionGroupActions(i)) if *i == idx
        );
        let show_dots = hovered || menu_open;
        let dots_glyph_color = if show_dots {
            OryxisColors::t().text_muted
        } else {
            Color::TRANSPARENT
        };
        let dots_btn = crate::widgets::card_kebab_button(
            dots_glyph_color,
            show_dots,
            Message::ShowSessionGroupMenu(idx),
        );
        let dots_align = if rtl {
            iced::alignment::Horizontal::Left
        } else {
            iced::alignment::Horizontal::Right
        };
        let dots_pad = if rtl {
            Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 4.0 }
        } else {
            Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 0.0 }
        };
        // Idle shows a muted chevron (this card opens into a restored
        // session, the same "opens a container" affordance the host-group
        // folders use); hover / menu-open swaps it for the ⋮ kebab.
        let trailing: Element<'a, Message> = if show_dots {
            dots_btn.into()
        } else {
            let chevron = if rtl {
                iced_fonts::lucide::chevron_left()
            } else {
                iced_fonts::lucide::chevron_right()
            };
            container(chevron.size(14).color(OryxisColors::t().text_muted))
                .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
                .into()
        };
        let dots_overlay = container(trailing)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(dots_align)
            .align_y(iced::alignment::Vertical::Center)
            .padding(dots_pad);

        let card_element: Element<'a, Message> = iced::widget::Stack::new()
            .push(card_btn)
            .push(dots_overlay)
            .into();

        let wrapped = MouseArea::new(card_element)
            .on_enter(Message::SessionGroupCardHovered(idx))
            .on_exit(Message::SessionGroupCardUnhovered)
            .on_right_press(Message::ShowSessionGroupMenu(idx));

        (
            Element::from(container(wrapped).width(Length::Fill).clip(true)),
            bg_color,
        )
    }
}
