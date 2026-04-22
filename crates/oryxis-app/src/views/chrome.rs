//! Standalone window chrome — drag region + minimize / maximize / close.
//!
//! Used by screens that don't render the full tab bar (vault setup / unlock /
//! error). The main app layout embeds the controls inside `view_tab_bar`.

use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, row, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length};
use iced_fonts::lucide as bs;

use crate::app::Message;
use crate::theme::OryxisColors;

/// Top bar with drag region + window controls. Renders at a fixed 28 px height
/// in the sidebar background tone so it blends with the tab bar on the main
/// screen.
pub(crate) fn window_chrome_bar<'a>() -> Element<'a, Message> {
    let drag_region: Element<'_, Message> = MouseArea::new(
        container(Space::new().width(Length::Fill).height(Length::Fixed(28.0)))
            .width(Length::Fill)
            .height(Length::Fixed(28.0)),
    )
    .on_press(Message::WindowDrag)
    .into();

    // Lock screen doesn't know if the window is maximized (rare to lock with
    // vault while maximized), so we default to `maximize_2` here — good
    // enough since the action toggles either way.
    let controls: Element<'_, Message> = row![
        chrome_btn(bs::minus(), Message::WindowMinimize, OryxisColors::t().text_secondary),
        chrome_btn(bs::square(), Message::WindowMaximizeToggle, OryxisColors::t().text_secondary),
        chrome_btn(bs::x(), Message::WindowClose, OryxisColors::t().error),
    ]
    .align_y(iced::Alignment::Center)
    .into();

    container(
        row![drag_region, controls].align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
        ..Default::default()
    })
    .into()
}

fn chrome_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    hover_color: Color,
) -> Element<'a, Message> {
    button(
        container(icon.size(12).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(40.0))
            .height(Length::Fixed(28.0)),
    )
    .on_press(msg)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .into()
}
