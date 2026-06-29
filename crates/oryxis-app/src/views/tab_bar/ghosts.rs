//! Tab bar: ghosts. Split out of views/tab_bar/mod.rs.

use super::*;
/// Floating chip shown over the strip while a tab is being drag-reordered:
/// a non-interactive copy of the dragged tab that tracks the cursor while
/// the real slot sits empty and the other tabs slide around it. Mirrors the
/// tab's badge (and label, unless it's a compact pinned chip) so the user
/// keeps sight of what they're moving.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drag_ghost<'a>(
    label: String,
    detected_os: Option<String>,
    compact: bool,
    width: f32,
    accent: Color,
    custom_icon: Option<String>,
    custom_color: Option<Color>,
) -> Element<'a, Message> {
    let (glyph, badge_color) = if let Some(name) = custom_icon.as_deref() {
        (crate::os_icon::custom_icon_glyph(name), custom_color.unwrap_or(accent))
    } else {
        crate::os_icon::resolve_icon(detected_os.as_deref(), accent)
    };
    let glyph_el: Element<'_, Message> = glyph.view(12.0, Color::WHITE);
    let badge = crate::widgets::host_icon(
        crate::widgets::HostIconStyle::Rounded,
        badge_color,
        &label,
        Some(glyph_el),
        TAB_ICON_SLOT,
    );
    let content: Element<'a, Message> = if compact {
        container(badge)
            .center_x(Length::Fixed(CHIP_W))
            .center_y(Length::Fixed(TAB_HEIGHT))
            .into()
    } else {
        crate::widgets::dir_row(vec![
            container(badge)
                .center_x(Length::Fixed(TAB_ICON_SLOT))
                .center_y(Length::Fixed(TAB_ICON_SLOT))
                .into(),
            Space::new().width(5).into(),
            text(truncate_label(&label, width))
                .size(12)
                .line_height(1.0)
                .wrapping(iced::widget::text::Wrapping::None)
                .font(SYSTEM_UI_SEMIBOLD)
                .color(accent)
                .into(),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    };
    container(content)
        .center_y(Length::Fixed(TAB_HEIGHT))
        .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 4.0 })
        .width(Length::Fixed(width))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.96, ..OryxisColors::t().bg_hover })),
            border: Border { radius: Radius::from(6.0), color: accent, width: 1.5 },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
            ..Default::default()
        })
        .into()
}

/// Floating drag ghost for an SFTP tab: the folder badge (tinted with the host
/// color) plus the label, mirroring `drag_ghost` but keeping the SFTP identity.
pub(crate) fn sftp_drag_ghost<'a>(label: String, compact: bool, width: f32, accent: Color) -> Element<'a, Message> {
    let badge = container(iced_fonts::lucide::folder_tree().size(12).color(Color::WHITE))
        .center_x(Length::Fixed(TAB_ICON_SLOT))
        .center_y(Length::Fixed(TAB_ICON_SLOT))
        .style(move |_| container::Style {
            background: Some(Background::Color(accent)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        });
    let content: Element<'a, Message> = if compact {
        container(badge)
            .center_x(Length::Fixed(CHIP_W))
            .center_y(Length::Fixed(TAB_HEIGHT))
            .into()
    } else {
        crate::widgets::dir_row(vec![
            badge.into(),
            Space::new().width(5).into(),
            text(truncate_label(&label, width))
                .size(12)
                .line_height(1.0)
                .wrapping(iced::widget::text::Wrapping::None)
                .font(SYSTEM_UI_SEMIBOLD)
                .color(accent)
                .into(),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    };
    container(content)
        .center_y(Length::Fixed(TAB_HEIGHT))
        .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 4.0 })
        .width(Length::Fixed(width))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.96, ..OryxisColors::t().bg_hover })),
            border: Border { radius: Radius::from(6.0), color: accent, width: 1.5 },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
            ..Default::default()
        })
        .into()
}
