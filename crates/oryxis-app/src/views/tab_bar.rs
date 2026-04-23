//! Tab bar + window chrome.
//!
//! All tabs (nav + session) render as consistent pill-shaped chips. Session
//! tabs show a small host-icon badge by default that morphs into an X on
//! hover (Termius-style close affordance). A `+` at the end opens a new
//! local-shell tab. The rest of the bar doubles as the OS window drag
//! region, with minimize / maximize / close on the far right.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, row, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::state::View;
use crate::theme::OryxisColors;

const TAB_HEIGHT: f32 = 24.0;

impl Oryxis {
    pub(crate) fn view_tab_bar(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = Vec::new();

        // Sidebar collapse toggle sits at the very start of the tab bar so
        // the sidebar header can stay dedicated to the logo.
        items.push(super::sidebar::sidebar_toggle_btn(!self.sidebar_collapsed));

        let nav_label = match self.active_view {
            View::Dashboard => "Hosts",
            View::Keys => "Keychain",
            View::Snippets => "Snippets",
            View::KnownHosts => "Known Hosts",
            View::History => "History",
            View::Settings => "Settings",
            View::Terminal => "",
        };
        if !nav_label.is_empty() {
            items.push(nav_chip(nav_label, self.active_view, self.active_tab.is_none()));
        }

        for (idx, tab) in self.tabs.iter().enumerate() {
            let is_active = self.active_tab == Some(idx);
            let is_hovered = self.hovered_tab == Some(idx);
            // Resolve the OS-specific badge colour per tab by looking up the
            // connection that matches this tab's label — after OS detection
            // this picks up the new brand colour on the next re-render.
            let base_label = tab.label.trim_end_matches(" (disconnected)");
            let detected_os = self
                .connections
                .iter()
                .find(|c| c.label == base_label)
                .and_then(|c| c.detected_os.clone());
            items.push(session_tab(idx, &tab.label, is_active, is_hovered, detected_os.as_deref()));
        }

        // "+ new tab" (opens a local shell for now — can be replaced later with
        // a Termius-style recent-connections overlay).
        items.push(new_tab_btn());

        // Drag region — everything between the tabs and window chrome.
        let drag_region: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fill).height(Length::Fixed(TAB_HEIGHT)))
                .width(Length::Fill)
                .height(Length::Fixed(TAB_HEIGHT)),
        )
        .on_press(Message::WindowDrag)
        .into();
        items.push(drag_region);

        // Maximize glyph swaps for a "restore" glyph when the window is
        // already maximized — canonical desktop control affordance.
        // Plain square for maximize, two overlapping squares (`copy`) for
        // restore — same visual language as the native Windows title bar.
        let max_icon = if self.window_maximized {
            iced_fonts::lucide::copy()
        } else {
            iced_fonts::lucide::square()
        };
        let control_row: Element<'_, Message> = row![
            window_btn(iced_fonts::lucide::minus(), Message::WindowMinimize, OryxisColors::t().text_secondary),
            window_btn(max_icon, Message::WindowMaximizeToggle, OryxisColors::t().text_secondary),
            window_btn(iced_fonts::lucide::x(), Message::WindowClose, OryxisColors::t().error),
        ]
        .align_y(iced::Alignment::Center)
        .into();
        items.push(control_row);

        container(row(items).spacing(6).align_y(iced::Alignment::Center))
            .width(Length::Fill)
            .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 6.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                ..Default::default()
            })
            .into()
    }
}

/// Nav pill for the current grid view (Hosts / Keychain / …) — uses the same
/// chip shape as the session tabs so the bar reads as a single row of bullets.
fn nav_chip<'a>(label: &'a str, view: View, is_active: bool) -> Element<'a, Message> {
    let fg = if is_active {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_muted
    };
    let bg = if is_active {
        OryxisColors::t().bg_surface
    } else {
        Color::TRANSPARENT
    };

    button(
        container(text(label).size(11).color(fg))
            .center_y(Length::Fixed(TAB_HEIGHT))
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }),
    )
    .on_press(Message::ChangeView(view))
    .style(move |_, status| {
        let hover_bg = match status {
            BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            _ => bg,
        };
        button::Style {
            background: Some(Background::Color(hover_bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Session tab: icon badge (host icon by default, X on hover) + label.
fn session_tab<'a>(
    idx: usize,
    label: &'a str,
    is_active: bool,
    is_hovered: bool,
    detected_os: Option<&str>,
) -> Element<'a, Message> {
    let fg = if is_active {
        OryxisColors::t().text_primary
    } else {
        OryxisColors::t().text_muted
    };
    let bg = if is_active {
        OryxisColors::t().bg_surface
    } else {
        Color::TRANSPARENT
    };

    let is_disconnected = label.ends_with(" (disconnected)");
    let display_label = label.trim_end_matches(" (disconnected)").to_string();

    // Icon slot morphs icon ⇄ X based on hover. The X is wrapped in its own
    // MouseArea with on_press dispatching CloseTab, so clicking the X closes
    // the tab while clicking the label area elsewhere selects it.
    let icon_slot: Element<'_, Message> = if is_hovered {
        MouseArea::new(
            container(
                iced_fonts::lucide::x().size(11).color(OryxisColors::t().text_secondary),
            )
            .center_x(Length::Fixed(18.0))
            .center_y(Length::Fixed(18.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_hover)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }),
        )
        .on_press(Message::CloseTab(idx))
        .into()
    } else {
        let fallback = if is_disconnected {
            OryxisColors::t().text_muted
        } else {
            OryxisColors::t().accent
        };
        // Brand colour derived from detected OS (or accent fallback).
        let (glyph, mut badge_color) = crate::os_icon::resolve_icon(detected_os, fallback);
        if is_disconnected {
            badge_color = OryxisColors::t().text_muted;
        }
        container(glyph.size(10).color(Color::WHITE))
            .center_x(Length::Fixed(18.0))
            .center_y(Length::Fixed(18.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(badge_color)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            })
            .into()
    };

    let tab_btn = button(
        container(
            row![
                icon_slot,
                Space::new().width(6),
                text(display_label).size(11).color(fg),
            ]
            .align_y(iced::Alignment::Center),
        )
        .center_y(Length::Fixed(TAB_HEIGHT))
        .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 4.0 }),
    )
    .on_press(Message::SelectTab(idx))
    .style(move |_, status| {
        let hover_bg = match status {
            BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            _ => bg,
        };
        button::Style {
            background: Some(Background::Color(hover_bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });

    MouseArea::new(tab_btn)
        .on_enter(Message::TabHovered(idx))
        .on_exit(Message::TabUnhovered)
        .on_right_press(Message::ShowTabMenu(idx))
        .into()
}

/// Plus button at the end of the tab row — opens the new-tab picker
/// (search + recent connections) as a centered modal overlay.
fn new_tab_btn<'a>() -> Element<'a, Message> {
    button(
        container(text("+").size(15).color(OryxisColors::t().text_muted))
            .center_x(Length::Fixed(24.0))
            .center_y(Length::Fixed(TAB_HEIGHT)),
    )
    .on_press(Message::ShowNewTabPicker)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Minimize / maximize / close glyph button for the window chrome.
fn window_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    hover_color: Color,
) -> Element<'a, Message> {
    button(
        container(icon.size(12).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(40.0))
            .height(Length::Fixed(TAB_HEIGHT)),
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
