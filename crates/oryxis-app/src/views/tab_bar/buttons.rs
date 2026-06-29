//! Tab bar: buttons. Split out of views/tab_bar/mod.rs.

use super::*;
/// Plus button that trails the last tab (browser-style), opening the
/// new-tab picker (search + recent connections) as a centered modal
/// overlay. `inline` renders it tab-strip-sized with rounded hover
/// (sitting among the tabs); the docked variant (strip overflow) keeps
/// the squared full-height look so it reads as part of the chrome
/// strip next to it. `PLUS_BUTTON_WIDTH` still feeds the layout-math
/// `RIGHT_CLUSTER_WIDTH` budget in both placements.
///
/// Uses `lucide::plus` instead of a literal `+` text character, on
/// Windows, Segoe UI's `+` renders much chunkier than the codicon
/// `−` / `□` / `✕` glyphs right next to it, breaking visual rhythm.
pub(crate) fn new_tab_btn<'a>(inline: bool) -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    let height = if inline { BAR_HEIGHT - 8.0 } else { BAR_HEIGHT };
    let radius = if inline { 6.0 } else { 0.0 };
    let btn = button(
        container(iced_fonts::lucide::plus().size(15).color(hover_color))
            .center(Length::Fixed(height))
            .height(Length::Fixed(height)),
    )
    .on_press(Message::ShowNewTabPicker)
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(radius), ..Default::default() },
            ..Default::default()
        }
    });
    // Click = new tab (default). Hovering reveals the New-Tab / Split
    // popover (no-op unless a terminal tab is open, see `ShowSplitMenu`).
    MouseArea::new(btn)
        .on_enter(Message::ShowSplitMenu)
        .on_exit(Message::SplitMenuLeave)
        .into()
}

/// Tab-jump button, opens the Termius-style "Jump to" modal listing
/// all open tabs + Quick connect entries. Always visible regardless of
/// how many tabs are open, so the user has a discoverable escape hatch
/// from a packed tab strip.
pub(crate) fn tab_jump_btn<'a>() -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    button(
        container(
            text("\u{22EF}") // horizontal ellipsis ⋯
                .size(15)
                .color(hover_color),
        )
        .center(Length::Fixed(DOTS_BUTTON_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::ShowTabJump)
    .padding(0)
    .style(move |_, status| {
        // Match the window-chrome / new-tab buttons' subtle hover
        // and squared corners so the right cluster reads as one
        // continuous strip.
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

/// Terminal side-panel toggle (Chat / Snippets / History). Sits right of
/// the `+ new tab` button. Replaces the old host-search button, which
/// only duplicated `+`'s "open the new-tab picker" action.
pub(crate) fn sidebar_btn<'a>() -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    button(
        container(
            iced_fonts::lucide::panel_right().size(15).color(hover_color),
        )
        .center(Length::Fixed(SIDEBAR_BUTTON_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::ToggleChatSidebar)
    .padding(0)
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

/// Burger menu trigger at the leading edge of the tab bar. When the
/// menu is open the button paints with the accent hover state so the
/// click affordance reads as "active control" instead of a stray glyph.
pub(crate) fn burger_menu_btn<'a>(is_open: bool) -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    let resting_bg = if is_open {
        Color { a: 0.2, ..hover_color }
    } else {
        Color::TRANSPARENT
    };
    // Symmetric padding around the glyph (no fixed-width `center`,
    // which left an empty right gap that read as a margin), so the
    // burger is all padding and no margin.
    button(
        container(
            iced_fonts::lucide::menu().size(15).color(hover_color),
        )
        .center_y(Length::Fixed(BAR_HEIGHT))
        .padding(Padding { top: 0.0, right: 11.0, bottom: 0.0, left: 11.0 }),
    )
    .on_press(Message::ToggleBurgerMenu)
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => resting_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .into()
}

/// Minimize / maximize / close glyph button for the window chrome.
/// Fills the full bar height (no padding) so hover backgrounds reach the
/// very top and bottom edges, same behaviour as Windows / VS Code.
pub(crate) fn window_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    hover_color: Color,
) -> Element<'a, Message> {
    button(
        container(icon.size(15).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(CHROME_BUTTON_WIDTH))
            .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(msg)
    .padding(0)
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
