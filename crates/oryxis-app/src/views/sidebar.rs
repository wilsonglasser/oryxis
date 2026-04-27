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
            sidebar_nav_btn(iced_fonts::lucide::folder_tree(), crate::i18n::t("sftp"), View::Sftp, active_is_nav && self.active_view == View::Sftp),
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
        .style(|_, status| {
            // Same hover treatment as the sidebar nav rows above.
            // Was a flat transparent style with no feedback at all,
            // making the row feel inert.
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                BtnStatus::Pressed => OryxisColors::t().bg_selected,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(10.0), ..Default::default() },
                ..Default::default()
            }
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
            collapsed_nav_btn(iced_fonts::lucide::folder_tree(), View::Sftp, active_is_nav && self.active_view == View::Sftp),
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

/// Toggle the sidebar between expanded and collapsed. Uses the
/// `panel-left-{close,open}` lucide pair — a small rectangle with a
/// vertical bar, animated open/closed depending on state. Reads as
/// "sidebar panel" much faster than a generic `«` / `»` chevron, which
/// is what we used to ship.
pub(crate) fn sidebar_toggle_btn<'a>(expanded: bool) -> Element<'a, Message> {
    let icon = if expanded {
        iced_fonts::lucide::panel_left_close()
    } else {
        iced_fonts::lucide::panel_left_open()
    };
    button(
        container(icon.size(15).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(28.0))
            .height(Length::Fixed(40.0)),
    )
    .on_press(Message::ToggleSidebar)
    .padding(0)
    .style(|_, status| {
        // Match the chrome / new-tab buttons' subtle hover style — a
        // tinted overlay using the icon color, not a flat white wash.
        // Squared corners so it lines up flush with the rest of the
        // tab bar's right cluster.
        let hover_color = OryxisColors::t().text_secondary;
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

impl Oryxis {
    /// Local Shell picker modal — listed shells come from
    /// `dispatch_settings::detect_local_shells`. Shown only on
    /// Windows; non-Windows platforms `OpenLocalShell` directly.
    pub(crate) fn view_local_shell_picker(&self) -> Element<'_, Message> {
        let shells = self.local_shells.as_deref();
        let mut list = column![].spacing(2);

        // Probe still in flight — show a hint instead of an empty
        // dropdown so the user knows the picker is loading rather
        // than broken.
        if shells.is_none() {
            list = list.push(
                container(
                    text(crate::i18n::t("detecting_shells"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(Padding {
                    top: 8.0,
                    right: 16.0,
                    bottom: 8.0,
                    left: 12.0,
                }),
            );
        }

        for spec in shells.unwrap_or(&[]) {
            list = list.push(
                button(
                    row![
                        iced_fonts::lucide::terminal()
                            .size(14)
                            .color(OryxisColors::t().accent),
                        Space::new().width(10),
                        text(spec.label.clone())
                            .size(13)
                            .color(OryxisColors::t().text_primary),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::OpenLocalShellWith {
                    program: spec.program.clone(),
                    args: spec.args.clone(),
                    label: spec.label.clone(),
                })
                .padding(Padding {
                    top: 8.0,
                    right: 16.0,
                    bottom: 8.0,
                    left: 12.0,
                })
                .width(Length::Fill)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(6.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            );
        }
        let header = container(
            text(crate::i18n::t("local_shell"))
                .size(15)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(OryxisColors::t().text_primary),
        )
        .padding(Padding {
            top: 16.0,
            right: 16.0,
            bottom: 8.0,
            left: 16.0,
        });
        let body = container(list)
            .padding(Padding {
                top: 4.0,
                right: 8.0,
                bottom: 12.0,
                left: 8.0,
            })
            .width(Length::Fill);
        // Wrap the dialog in a MouseArea NoOp so clicks on its body
        // don't fall through to the scrim and dismiss it.
        let dialog = iced::widget::MouseArea::new(
            container(column![header, body])
                .width(Length::Fixed(360.0))
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(12.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    shadow: iced::Shadow {
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                        offset: iced::Vector::new(0.0, 8.0),
                        blur_radius: 24.0,
                    },
                    ..Default::default()
                }),
        )
        .on_press(Message::NoOp);
        container(dialog)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}
