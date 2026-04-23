//! Left navigation sidebar. Collapsible — `Oryxis::sidebar_collapsed` switches
//! between the full pill-shaped nav and a narrow icon rail.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, image, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED};
use crate::state::View;
use crate::theme::OryxisColors;
use crate::widgets::sidebar_nav_btn;

impl Oryxis {
    pub(crate) fn view_sidebar(&self) -> Element<'_, Message> {
        if self.sidebar_collapsed {
            self.view_sidebar_collapsed()
        } else {
            self.view_sidebar_expanded()
        }
    }

    fn view_sidebar_expanded(&self) -> Element<'_, Message> {
        // Header: centered logo. Collapse toggle lives in the tab bar now
        // so the logo stays visible in both sidebar states.
        let header = container(image(self.logo_small_handle.clone()).width(48).height(48))
            .padding(Padding { top: 12.0, right: 0.0, bottom: 10.0, left: 0.0 })
            .width(Length::Fill)
            .center_x(Length::Fill);

        let active_is_nav = self.active_tab.is_none();
        let nav_buttons: Vec<Element<'_, Message>> = vec![
            sidebar_nav_btn(iced_fonts::lucide::server(), crate::i18n::t("hosts"), View::Dashboard, active_is_nav && self.active_view == View::Dashboard),
            sidebar_nav_btn(iced_fonts::lucide::key_round(), crate::i18n::t("keychain"), View::Keys, active_is_nav && self.active_view == View::Keys),
            sidebar_nav_btn(iced_fonts::lucide::code(), crate::i18n::t("snippets"), View::Snippets, active_is_nav && self.active_view == View::Snippets),
            sidebar_nav_btn(iced_fonts::lucide::shield_check(), crate::i18n::t("known_hosts"), View::KnownHosts, active_is_nav && self.active_view == View::KnownHosts),
            sidebar_nav_btn(iced_fonts::lucide::history(), crate::i18n::t("history"), View::History, active_is_nav && self.active_view == View::History),
            sidebar_nav_btn(iced_fonts::lucide::settings(), crate::i18n::t("settings"), View::Settings, active_is_nav && self.active_view == View::Settings),
        ];

        let local_btn = button(
            container(
                row![
                    text("+").size(13).color(OryxisColors::t().text_muted),
                    Space::new().width(10),
                    text(crate::i18n::t("local_shell")).size(12).color(OryxisColors::t().text_muted),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .on_press(Message::OpenLocalShell)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            border: Border { radius: Radius::from(10.0), ..Default::default() },
            ..Default::default()
        });

        let sidebar_content = column![
            header,
            column(nav_buttons),
            Space::new().height(Length::Fill),
            container(local_btn)
                .padding(Padding { top: 0.0, right: 8.0, bottom: 12.0, left: 8.0 }),
        ]
        .width(Length::Fill);

        container(sidebar_content)
            .width(SIDEBAR_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                ..Default::default()
            })
            .into()
    }

    fn view_sidebar_collapsed(&self) -> Element<'_, Message> {
        let header = container(image(self.logo_small_handle.clone()).width(40).height(40))
            .padding(Padding { top: 12.0, right: 0.0, bottom: 10.0, left: 0.0 })
            .width(Length::Fill)
            .center_x(Length::Fill);

        let active_is_nav = self.active_tab.is_none();
        let icons: Vec<Element<'_, Message>> = vec![
            collapsed_nav_btn(iced_fonts::lucide::server(), View::Dashboard, active_is_nav && self.active_view == View::Dashboard),
            collapsed_nav_btn(iced_fonts::lucide::key_round(), View::Keys, active_is_nav && self.active_view == View::Keys),
            collapsed_nav_btn(iced_fonts::lucide::code(), View::Snippets, active_is_nav && self.active_view == View::Snippets),
            collapsed_nav_btn(iced_fonts::lucide::shield_check(), View::KnownHosts, active_is_nav && self.active_view == View::KnownHosts),
            collapsed_nav_btn(iced_fonts::lucide::history(), View::History, active_is_nav && self.active_view == View::History),
            collapsed_nav_btn(iced_fonts::lucide::settings(), View::Settings, active_is_nav && self.active_view == View::Settings),
        ];

        let local_btn = button(
            container(text("+").size(16).color(OryxisColors::t().text_muted))
                .center(Length::Fixed(40.0)),
        )
        .on_press(Message::OpenLocalShell)
        .width(Length::Fill)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }
        });

        let content = column![
            header,
            column(icons).spacing(2),
            Space::new().height(Length::Fill),
            container(local_btn)
                .padding(Padding { top: 0.0, right: 8.0, bottom: 12.0, left: 8.0 }),
        ]
        .width(Length::Fill);

        container(content)
            .width(SIDEBAR_WIDTH_COLLAPSED)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                ..Default::default()
            })
            .into()
    }
}

/// Chevron button that toggles the sidebar between expanded and collapsed.
/// `expanded=true` renders a `«` chevron; `false` renders `»`.
pub(crate) fn sidebar_toggle_btn<'a>(expanded: bool) -> Element<'a, Message> {
    let glyph = if expanded { "\u{00AB}" } else { "\u{00BB}" };
    button(
        container(text(glyph).size(14).color(OryxisColors::t().text_muted))
            .center(Length::Fixed(28.0)),
    )
    .on_press(Message::ToggleSidebar)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
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

/// Centered, icon-only nav button used in the collapsed rail.
fn collapsed_nav_btn<'a>(
    icon: iced::widget::Text<'a>,
    view: View,
    is_active: bool,
) -> Element<'a, Message> {
    let accent = OryxisColors::t().accent;
    let muted = OryxisColors::t().text_secondary;
    let fg = if is_active { accent } else { muted };
    let active_bg = Color { a: 0.15, ..accent };

    container(
        button(
            container(icon.size(16).color(fg))
                .center(Length::Fixed(40.0)),
        )
        .on_press(Message::ChangeView(view))
        .width(Length::Fill)
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                BtnStatus::Pressed => Color { a: 0.25, ..accent },
                _ if is_active => active_bg,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            }
        }),
    )
    .padding(Padding { top: 1.0, right: 8.0, bottom: 1.0, left: 8.0 })
    .into()
}
