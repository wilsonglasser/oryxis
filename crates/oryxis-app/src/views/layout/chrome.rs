//! Root layout: chrome. Split out of views/layout/mod.rs.

use super::*;
use iced::widget::{column, row};
/// Owned-label variant of `styled_button` for the error dialog link. The
/// label and URL come from `ErrorDialog` clones with no static lifetime,
/// so `styled_button(&str, ...)` would dangle on return.
/// Primary recovery-action button for the error dialog. Owned label
/// (the text comes from dialog state, not a 'static i18n ref); pressing
/// fires `ErrorDialogRunAction`, which dismisses the dialog and
/// dispatches the action's carried message.
pub(crate) fn dialog_action_button<'a>(label: String, danger: bool) -> Element<'a, Message> {
    let color = if danger {
        OryxisColors::t().error
    } else {
        OryxisColors::t().accent
    };
    let fg = OryxisColors::t().button_text;
    button(
        container(
            text(label).size(12).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(fg),
        )
        .padding(Padding { top: 5.0, right: 18.0, bottom: 5.0, left: 18.0 }),
    )
    .on_press(Message::ErrorDialogRunAction)
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color { a: 0.85, ..color },
            _ => color,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

pub(crate) fn open_link_button<'a>(label: String, url: String) -> Element<'a, Message> {
    let color = OryxisColors::t().accent;
    let fg = OryxisColors::t().button_text;
    button(
        container(
            text(label).size(12).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(fg),
        )
        .padding(Padding { top: 5.0, right: 18.0, bottom: 5.0, left: 18.0 }),
    )
    .on_press(Message::OpenUrl(url))
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color {
                a: 1.0,
                r: (color.r + 0.05).min(1.0),
                g: (color.g + 0.05).min(1.0),
                b: (color.b + 0.05).min(1.0),
            },
            _ => color,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// One choice row in the folder-delete modal: a full-width card with a
/// leading icon badge (tinted by `accent` to color-code safe vs
/// destructive), a semibold title and a muted one-line consequence. Both
/// hover and press shift the fill and tint the border, so the card never
/// reads as inert.
pub(crate) fn folder_choice_card<'a>(
    glyph: iced::widget::Text<'a>,
    title: &'a str,
    desc: &'a str,
    msg: Message,
    accent: Color,
) -> Element<'a, Message> {
    let c = OryxisColors::t();
    let badge = container(glyph.size(17).color(accent))
        .width(Length::Fixed(34.0))
        .height(Length::Fixed(34.0))
        .center_x(Length::Fixed(34.0))
        .center_y(Length::Fixed(34.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.12, ..accent })),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
    let texts = column![
        text(title.to_owned())
            .size(13)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(c.text_primary),
        Space::new().height(2),
        text(desc.to_owned())
            .size(11)
            .color(c.text_muted)
            .width(Length::Fill),
    ]
    .width(Length::Fill)
    .align_x(dir_align_x());
    let body = dir_row(vec![badge.into(), Space::new().width(12).into(), texts.into()])
        .align_y(iced::Alignment::Center);
    button(
        container(body).padding(Padding { top: 11.0, right: 14.0, bottom: 11.0, left: 14.0 }),
    )
    .width(Length::Fill)
    .on_press(msg)
    .style(move |_, status| {
        let (bg, border_color) = match status {
            iced::widget::button::Status::Pressed => (Color { a: 0.16, ..accent }, accent),
            iced::widget::button::Status::Hovered => {
                (c.bg_hover, Color { a: 0.55, ..accent })
            }
            _ => (Color::TRANSPARENT, c.border),
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(10.0), color: border_color, width: 1.0 },
            ..Default::default()
        }
    })
    .into()
}

/// Low-emphasis, full-width text button used for the modal "Cancel" so it
/// does not compete with the colored action cards above it. Transparent at
/// rest, picks up `bg_hover` / `bg_selected` on hover / press.
pub(crate) fn ghost_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    let c = OryxisColors::t();
    button(
        container(
            text(label.to_owned())
                .size(12)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(c.text_muted),
        )
        .center_x(Length::Fill)
        .padding(Padding { top: 7.0, right: 14.0, bottom: 7.0, left: 14.0 }),
    )
    .width(Length::Fill)
    .on_press(msg)
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Pressed => c.bg_selected,
            iced::widget::button::Status::Hovered => c.bg_hover,
            _ => Color::TRANSPARENT,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Invisible hit-zone used on the window edges and corners. Captures a press
/// and hands off to the OS as a native resize drag. Double-click on N/S
/// expands to full monitor height, same convention Windows uses (no
/// horizontal equivalent, so E/W stays drag-only).
pub(crate) fn resize_handle<'a>(direction: Direction, width: Length, height: Length) -> Element<'a, Message> {
    let mut area = MouseArea::new(container(Space::new()).width(width).height(height))
        .on_press(Message::WindowResizeDrag(direction))
        .interaction(match direction {
            Direction::North | Direction::South => iced::mouse::Interaction::ResizingVertically,
            Direction::East | Direction::West => iced::mouse::Interaction::ResizingHorizontally,
            Direction::NorthEast | Direction::SouthWest => iced::mouse::Interaction::ResizingDiagonallyUp,
            Direction::NorthWest | Direction::SouthEast => iced::mouse::Interaction::ResizingDiagonallyDown,
        });
    if matches!(direction, Direction::North | Direction::South) {
        area = area.on_double_click(Message::WindowExpandVertical);
    }
    area.into()
}

/// Layers the resize border on top of the given content, or returns the
/// content untouched when the window is maximized (no borders to grab).
pub(crate) fn wrap_with_resize<'a>(
    content: Element<'a, Message>,
    overlay: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    match overlay {
        Some(overlay) => Stack::new()
            .push(content)
            .push(overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => content,
    }
}

/// Transparent border frame made of 8 resize hit-zones (4 edges + 4 corners).
/// The centre is a `Space` with fill so pointer events fall through to the
/// base layer underneath.
pub(crate) fn resize_border<'a>() -> Element<'a, Message> {
    let t = RESIZE_EDGE;
    column![
        row![
            resize_handle(Direction::NorthWest, Length::Fixed(t), Length::Fixed(t)),
            resize_handle(Direction::North, Length::Fill, Length::Fixed(t)),
            resize_handle(Direction::NorthEast, Length::Fixed(t), Length::Fixed(t)),
        ]
        .height(Length::Fixed(t)),
        row![
            resize_handle(Direction::West, Length::Fixed(t), Length::Fill),
            Space::new().width(Length::Fill).height(Length::Fill),
            resize_handle(Direction::East, Length::Fixed(t), Length::Fill),
        ]
        .height(Length::Fill),
        row![
            resize_handle(Direction::SouthWest, Length::Fixed(t), Length::Fixed(t)),
            resize_handle(Direction::South, Length::Fill, Length::Fixed(t)),
            resize_handle(Direction::SouthEast, Length::Fixed(t), Length::Fixed(t)),
        ]
        .height(Length::Fixed(t)),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
