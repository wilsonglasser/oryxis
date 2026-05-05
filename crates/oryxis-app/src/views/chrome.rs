//! Standalone window chrome — drag region + minimize / maximize / close.
//!
//! Used by screens that don't render the full tab bar (vault setup / unlock /
//! error). The main app layout embeds the controls inside `view_tab_bar`.

use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length};
use iced_fonts::codicon as cd;

use crate::app::Message;
use crate::theme::OryxisColors;

/// Chrome bar height — must match the main view's `BAR_HEIGHT` so the lock
/// screen's chrome doesn't visually pop when transitioning to the main app
/// after unlocking.
const CHROME_HEIGHT: f32 = 40.0;

/// Top bar with drag region + window controls. Renders at a fixed 28 px height
/// in the sidebar background tone so it blends with the tab bar on the main
/// screen. Uses VS Code's codicon glyphs to match the native Windows chrome
/// look the user expects (and stays identical cross-platform).
pub(crate) fn window_chrome_bar<'a>() -> Element<'a, Message> {
    let drag_region: Element<'_, Message> = MouseArea::new(
        container(Space::new().width(Length::Fill).height(Length::Fixed(CHROME_HEIGHT)))
            .width(Length::Fill)
            .height(Length::Fixed(CHROME_HEIGHT)),
    )
    .on_press(Message::WindowDrag)
    .into();

    // Lock screen doesn't know whether the window is currently maximized; the
    // toggle works either way, so we always show the maximize glyph here.
    // `dir_row` flips the trio under RTL so close ends up on the leading edge.
    let controls: Element<'_, Message> = crate::widgets::dir_row(vec![
        chrome_btn(cd::chrome_minimize(), Message::WindowMinimize, OryxisColors::t().text_secondary),
        chrome_btn(cd::chrome_maximize(), Message::WindowMaximizeToggle, OryxisColors::t().text_secondary),
        chrome_btn(cd::chrome_close(), Message::WindowClose, OryxisColors::t().error),
    ])
    .align_y(iced::Alignment::Center)
    .into();

    container(
        crate::widgets::dir_row(vec![drag_region, controls])
            .align_y(iced::Alignment::Center),
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
        container(icon.size(15).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(46.0))
            .height(Length::Fixed(CHROME_HEIGHT)),
    )
    // Same fix as the main tab bar — `button` defaults to 5 px padding on
    // top/bottom, which prevented the hover background from filling the
    // whole chrome strip.
    .padding(0)
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
