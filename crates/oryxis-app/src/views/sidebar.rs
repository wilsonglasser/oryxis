//! Vault navigation: the vertical icon rail (the `"vertical"`
//! `nav_orientation`) rendered on the leading edge of the vault content,
//! plus the local-shell picker modal.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, tooltip, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, NAV_RAIL_WIDTH_EXPANDED, SIDEBAR_WIDTH_COLLAPSED};
use crate::state::View;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row};

impl Oryxis {
    /// Vertical nav rail of the vault sub-sections (Termius-style), shown
    /// on the leading edge of the content when `nav_orientation` is
    /// `"vertical"`. Personal badge on top, scrollable section list in the
    /// middle, and a pinned footer (Settings gear + expand/collapse
    /// toggle) that stays visible regardless of scroll. Collapsed = icon
    /// rail with tooltips; expanded (`setting_nav_rail_expanded`) = wide
    /// rail with labels.
    pub(crate) fn view_vault_nav_rail(&self) -> Element<'_, Message> {
        let active = self.active_tab.is_none();
        let expanded = self.setting_nav_rail_expanded;
        let act = |view: View| active && self.active_view == view;

        let mut items: Vec<Element<'_, Message>> = vec![
            rail_item(iced_fonts::lucide::server(), crate::i18n::t("hosts"), View::Dashboard, act(View::Dashboard), expanded),
            rail_item(iced_fonts::lucide::key_round(), crate::i18n::t("keychain"), View::Keys, act(View::Keys), expanded),
            rail_item(iced_fonts::lucide::code(), crate::i18n::t("snippets"), View::Snippets, act(View::Snippets), expanded),
            rail_item(iced_fonts::lucide::route(), crate::i18n::t("port_forwards"), View::PortForwarding, act(View::PortForwarding), expanded),
        ];
        if self.logs_surface_visible() {
            items.push(rail_item(iced_fonts::lucide::history(), crate::i18n::t("logs"), View::History, act(View::History), expanded));
        }
        items.push(rail_item(iced_fonts::lucide::cloud(), crate::i18n::t("cloud_accounts"), View::Cloud, act(View::Cloud), expanded));
        items.push(rail_item(iced_fonts::lucide::router(), crate::i18n::t("proxies"), View::Proxies, act(View::Proxies), expanded));
        items.push(rail_item(iced_fonts::lucide::shield_check(), crate::i18n::t("known_hosts"), View::KnownHosts, act(View::KnownHosts), expanded));

        let nav = scrollable(
            column(items)
                .spacing(4)
                .padding(Padding { top: 8.0, right: 0.0, bottom: 0.0, left: 0.0 }),
        )
        .height(Length::Fill)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6.0).scroller_width(4.0),
            ))
            .style(rail_scroll_style);

        // Footer: Settings + expand/collapse toggle, pinned below the
        // scrollable so they're always reachable.
        let settings_item = rail_item(
            iced_fonts::lucide::settings(),
            crate::i18n::t("settings"),
            View::Settings,
            active && self.active_view == View::Settings,
            expanded,
        );
        let footer = container(column![settings_item, rail_toggle_item(expanded)].spacing(4))
            .padding(Padding { top: 8.0, right: 0.0, bottom: 12.0, left: 0.0 });

        // Static "Personal" vault switcher placeholder (non-interactive
        // until multi-vault lands): glyph only when collapsed, glyph +
        // name + chevron when expanded.
        let badge_inner: Element<'_, Message> = if expanded {
            dir_row(vec![
                iced_fonts::lucide::lock().size(14).color(OryxisColors::t().accent).into(),
                Space::new().width(8).into(),
                text("Personal").size(13).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                iced_fonts::lucide::chevron_down().size(12).color(OryxisColors::t().text_muted).into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            iced_fonts::lucide::lock().size(16).color(OryxisColors::t().accent).into()
        };
        let badge_pad = if expanded {
            Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 }
        } else {
            Padding::ZERO
        };
        // Collapsed: center the lock glyph in the badge (it's shrink-fit,
        // so without this it pins to the leading edge and misaligns with
        // the centered nav icons below). Expanded: the row owns its layout.
        let badge_align = if expanded {
            iced::alignment::Horizontal::Left
        } else {
            iced::alignment::Horizontal::Center
        };
        let vault_badge = container(
            container(badge_inner)
                .center_y(Length::Fixed(40.0))
                .width(Length::Fill)
                .align_x(badge_align)
                .padding(badge_pad)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(8.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }),
        )
        .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 });

        // Vault badge only when there's more than one vault to switch.
        let mut col_items: Vec<Element<'_, Message>> = Vec::new();
        if self.show_vault_switcher() {
            col_items.push(vault_badge.into());
        }
        col_items.push(nav.into());
        col_items.push(footer.into());
        let content = iced::widget::Column::with_children(col_items).width(Length::Fill);

        let width = if expanded {
            NAV_RAIL_WIDTH_EXPANDED
        } else {
            SIDEBAR_WIDTH_COLLAPSED
        };
        container(content)
            .width(width)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                ..Default::default()
            })
            .into()
    }
}

/// Thin scrollbar for the rail: hidden (transparent scroller) at rest,
/// revealed only while the rail is hovered or being dragged. Built on the
/// theme default so the track/border stay consistent.
fn rail_scroll_style(theme: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let visible = !matches!(status, scrollable::Status::Active { .. });
    let scroller_bg = if visible {
        Background::Color(Color { a: 0.45, ..OryxisColors::t().text_muted })
    } else {
        Background::Color(Color::TRANSPARENT)
    };
    for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
        rail.background = None;
        rail.border = Border::default();
        rail.scroller.background = scroller_bg;
        rail.scroller.border = Border { radius: Radius::from(3.0), ..Default::default() };
    }
    style
}

/// One rail entry. Collapsed → a centered square icon button wrapped in a
/// tooltip showing the section label; expanded → an icon + label row.
fn rail_item<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    view: View,
    is_active: bool,
    expanded: bool,
) -> Element<'a, Message> {
    if expanded {
        expanded_nav_btn(icon, label, view, is_active)
    } else {
        tooltip(
            collapsed_nav_btn(icon, view, is_active),
            container(text(label).size(11).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }),
            tooltip::Position::Right,
        )
        .into()
    }
}

/// Footer expand/collapse toggle, matching the rail-item shape.
fn rail_toggle_item<'a>(expanded: bool) -> Element<'a, Message> {
    let rtl = crate::i18n::is_rtl_layout();
    let icon = match (rtl, expanded) {
        (false, true) => iced_fonts::lucide::panel_left_close(),
        (false, false) => iced_fonts::lucide::panel_left_open(),
        (true, true) => iced_fonts::lucide::panel_right_close(),
        (true, false) => iced_fonts::lucide::panel_right_open(),
    };
    let muted = OryxisColors::t().text_secondary;
    if expanded {
        container(
            button(
                container(
                    dir_row(vec![
                        icon.size(16).color(muted).into(),
                        Space::new().width(10).into(),
                        text(crate::i18n::t("collapse")).size(13).color(muted).into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .width(Length::Fill)
                .center_y(Length::Fixed(40.0))
                .align_x(dir_align_x())
                .padding(Padding { top: 0.0, right: 16.0, bottom: 0.0, left: 16.0 }),
            )
            .on_press(Message::ToggleNavRailExpanded)
            .padding(0)
            .width(Length::Fill)
            .style(rail_btn_style(false)),
        )
        .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 })
        .into()
    } else {
        tooltip(
            container(
                button(container(icon.size(16).color(muted)).center(Length::Fixed(40.0)))
                    .on_press(Message::ToggleNavRailExpanded)
                    .padding(0)
                    .width(Length::Fixed(40.0))
                    .style(rail_btn_style(false)),
            )
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }),
            container(text(crate::i18n::t("expand")).size(11).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }),
            tooltip::Position::Right,
        )
        .into()
    }
}

/// Shared button style for rail entries (active accent wash, hover tint).
fn rail_btn_style(
    is_active: bool,
) -> impl Fn(&iced::Theme, BtnStatus) -> button::Style {
    let accent = OryxisColors::t().accent;
    let active_bg = Color { a: 0.15, ..accent };
    move |_, status| {
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
    }
}

/// Centered, icon-only nav button (square highlight) for the collapsed rail.
fn collapsed_nav_btn<'a>(
    icon: iced::widget::Text<'a>,
    view: View,
    is_active: bool,
) -> Element<'a, Message> {
    let fg = if is_active {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_secondary
    };
    container(
        button(
            container(icon.size(16).color(fg)).center(Length::Fixed(40.0)),
        )
        .on_press(Message::ChangeView(view))
        .padding(0)
        .width(Length::Fixed(40.0))
        .style(rail_btn_style(is_active)),
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 })
    .into()
}

/// Icon + label nav row for the expanded rail.
fn expanded_nav_btn<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    view: View,
    is_active: bool,
) -> Element<'a, Message> {
    let fg = if is_active {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_secondary
    };
    container(
        button(
            container(
                dir_row(vec![
                    icon.size(16).color(fg).into(),
                    Space::new().width(10).into(),
                    text(label).size(13).color(fg).into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            // Fixed 40px height (same as collapsed_nav_btn's
            // `.center(Length::Fixed(40.0))`) so the pill height is
            // identical in both modes: toggling collapse/expand must not
            // shift row heights. With a 16px icon centered in 40px the
            // vertical inset lands at a clean 12px.
            .center_y(Length::Fixed(40.0))
            .align_x(dir_align_x())
            .padding(Padding { top: 0.0, right: 16.0, bottom: 0.0, left: 16.0 }),
        )
        .on_press(Message::ChangeView(view))
        // Zero the button's default padding so the inner container's
        // inset is exact (matches collapsed_nav_btn, which also zeroes it).
        .padding(0)
        .width(Length::Fill)
        .style(rail_btn_style(is_active)),
    )
    .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 })
    .into()
}

impl Oryxis {
    /// Local Shell picker modal. Rows come from the curated, persisted
    /// local-terminal list (`Oryxis::local_terminals`); a "+ terminal"
    /// footer jumps to the management card in Settings → Terminal.
    pub(crate) fn view_local_shell_picker(&self) -> Element<'_, Message> {
        let entries = self.local_terminals.as_deref();
        let mut list = column![].spacing(2);

        // Probe still in flight, show a hint instead of an empty
        // dropdown so the user knows the picker is loading rather
        // than broken.
        if entries.is_none() {
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

        for entry in entries.unwrap_or(&[]) {
            // Same icon + color resolution as the Settings card so the
            // picker row matches: explicit override, OS hint, then a
            // generic terminal glyph.
            let (glyph, col) = crate::os_icon::local_terminal_icon(
                entry.icon.as_deref(),
                &entry.label,
                entry.color.as_deref(),
                OryxisColors::t().accent,
            );
            list = list.push(
                button(
                    container(
                        dir_row(vec![
                            container(glyph.view(13.0, Color::WHITE))
                                .width(Length::Fixed(22.0))
                                .height(Length::Fixed(22.0))
                                .center_x(Length::Fixed(22.0))
                                .center_y(Length::Fixed(22.0))
                                .style(move |_| container::Style {
                                    background: Some(Background::Color(col)),
                                    border: Border {
                                        radius: Radius::from(6.0),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })
                                .into(),
                            Space::new().width(10).into(),
                            text(entry.label.clone())
                                .size(13)
                                .color(OryxisColors::t().text_primary)
                                .into(),
                        ])
                        .align_y(iced::Alignment::Center),
                    )
                    .width(Length::Fill)
                    .align_x(dir_align_x()),
                )
                .on_press(Message::OpenLocalShellWith {
                    program: entry.program.clone(),
                    args: entry.args.clone(),
                    label: entry.label.clone(),
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
        // Footer shortcut into the management card (always present, even
        // when the list is empty), separated by a top hairline.
        let footer = container(
            button(
                container(
                    dir_row(vec![
                        iced_fonts::lucide::plus()
                            .size(14)
                            .color(OryxisColors::t().accent)
                            .into(),
                        Space::new().width(8).into(),
                        text(crate::i18n::t("add_terminal"))
                            .size(13)
                            .color(OryxisColors::t().accent)
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .on_press(Message::OpenLocalTerminalsSettings)
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
                    BtnStatus::Pressed => OryxisColors::t().bg_selected,
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
        )
        .padding(Padding {
            top: 6.0,
            right: 8.0,
            bottom: 8.0,
            left: 8.0,
        })
        .width(Length::Fill);
        // 1px separator above the footer so it reads as a distinct zone.
        let divider = container(Space::new().width(Length::Fill).height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });
        // Wrap the dialog in a MouseArea NoOp so clicks on its body
        // don't fall through to the scrim and dismiss it.
        let dialog = iced::widget::MouseArea::new(
            container(column![header, body, divider, footer])
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
        // Bare card; `widgets::modal_overlay` (the caller) centers + scrims.
        dialog.into()
    }
}
