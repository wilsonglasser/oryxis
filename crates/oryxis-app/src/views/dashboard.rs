//! Dashboard — folders/hosts grid.

use iced::border::Radius;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::AuthMethod;

use crate::app::{Message, Oryxis, CARD_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn view_dashboard(&self) -> Element<'_, Message> {
        // ── Toolbar ──
        let toolbar_left: Element<'_, Message> = if let Some(gid) = self.active_group {
            let group_name = self.groups.iter()
                .find(|g| g.id == gid)
                .map(|g| g.label.as_str())
                .unwrap_or(t("group_fallback_label"));
            dir_row(vec![
                button(
                    dir_row(vec![
                        iced_fonts::lucide::arrow_left().size(14).color(OryxisColors::t().accent).into(),
                        Space::new().width(6).into(),
                        text(t("all_hosts")).size(14).color(OryxisColors::t().accent).into(),
                    ]).align_y(iced::Alignment::Center),
                )
                .on_press(Message::BackToRoot)
                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                }).into(),
                text("/").size(16).color(OryxisColors::t().text_muted).into(),
                Space::new().width(8).into(),
                iced_fonts::lucide::folder().size(16).color(OryxisColors::t().accent).into(),
                Space::new().width(6).into(),
                text(group_name).size(16).color(OryxisColors::t().text_primary).into(),
            ]).align_y(iced::Alignment::Center).into()
        } else {
            text(t("hosts")).size(20).color(OryxisColors::t().text_primary).into()
        };

        let toolbar = container(
            dir_row(vec![
                toolbar_left,
                Space::new().width(Length::Fill).into(),
                {
                    let fg = OryxisColors::t().button_text;
                    button(
                        container(
                            dir_row(vec![
                                text("+").size(13).font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                }).color(fg).into(),
                                Space::new().width(4).into(),
                                text(t("host_btn")).size(11).font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                }).color(fg).into(),
                            ]).align_y(iced::Alignment::Center),
                        )
                        .center_y(Length::Fixed(24.0))
                        .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
                    )
                    .on_press(Message::ShowNewConnection)
                    .style(|_, status| {
                        let bg = match status {
                            BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                            _ => OryxisColors::t().button_bg,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border { radius: Radius::from(6.0), ..Default::default() },
                            ..Default::default()
                        }
                    }).into()
                },
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // ── Search bar ──
        let search_bar = container(
            text_input(t("search_hosts"), &self.host_search)
                .on_input(Message::HostSearchChanged)
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style),
        )
        .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
        .width(Length::Fill);

        // ── Status ──
        let status: Element<'_, Message> = if let Some(err) = &self.host_panel_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        // ── Host cards grid ──
        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if self.connections.is_empty() {
            // Termius-style empty state — centered "Create host" with input
            let has_input = !self.quick_host_input.is_empty();
            let btn_bg = if has_input { OryxisColors::t().success } else { OryxisColors::t().bg_surface };

            let empty_state = container(
                column![
                    // Icon
                    container(
                        iced_fonts::lucide::server().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(crate::i18n::t("create_host_title")).size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(crate::i18n::t("create_host_desc"))
                        .size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    // Hostname input
                    text_input(t("type_ip_or_hostname"), &self.quick_host_input)
                        .on_input(Message::QuickHostInput)
                        .on_submit(Message::QuickHostContinue)
                        .padding(14)
                        .width(380)
                        .style(crate::widgets::rounded_input_style),
                    Space::new().height(12),
                    // Continue button
                    button(
                        container(text(crate::i18n::t("continue_btn")).size(14).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                            .width(380)
                            .center_x(380),
                    )
                    .on_press(Message::QuickHostContinue)
                    .width(380)
                    .style(move |_, _| button::Style {
                        background: Some(Background::Color(btn_bg)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, search_bar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_host_panel {
                let panel = self.view_host_panel();
                return dir_row(vec![main_content.into(), panel])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            } else {
                return main_content.into();
            }
        }

        if self.active_group.is_none() {
            // Root view: show folder cards for groups that have connections
            let mut shown_groups = std::collections::HashSet::new();
            for conn in &self.connections {
                if let Some(gid) = conn.group_id
                    && shown_groups.insert(gid)
                        && let Some(group) = self.groups.iter().find(|g| g.id == gid) {
                            let count = self.connections.iter().filter(|c| c.group_id == Some(gid)).count();
                            let label = group.label.clone();
                            // Plural form differs across languages (English
                            // pluralizes, Persian/Chinese/Japanese don't); use
                            // the bare i18n word so every locale stays correct.
                            let count_text = format!("{} {}", count, t("hosts").to_lowercase());

                            // Folder card — matches host card layout (square icon,
                            // same padding and hover feedback).
                            let icon_box = container(
                                iced_fonts::lucide::folder().size(14).color(Color::WHITE),
                            )
                            .width(Length::Fixed(32.0))
                            .height(Length::Fixed(32.0))
                            .center_x(Length::Fixed(32.0))
                            .center_y(Length::Fixed(32.0))
                            .style(|_| container::Style {
                                background: Some(Background::Color(OryxisColors::t().accent)),
                                border: Border { radius: Radius::from(8.0), ..Default::default() },
                                ..Default::default()
                            });

                            // ⋮ button — only rendered while the folder
                            // row is hovered, mirroring the host-card UX.
                            // A fixed-width placeholder reserves the slot
                            // so the label width budget never changes.
                            const FOLDER_DOTS_SLOT_W: f32 = 22.0;
                            let folder_show_dots = self.hovered_folder_card == Some(gid);
                            let actions_btn: Element<'_, Message> = if folder_show_dots {
                                button(
                                    text("\u{22EE}").size(14).color(OryxisColors::t().text_muted),
                                )
                                .on_press(Message::ShowFolderActions(gid))
                                .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
                                .style(|_, status| {
                                    let bg = match status {
                                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                        _ => Color::TRANSPARENT,
                                    };
                                    button::Style {
                                        background: Some(Background::Color(bg)),
                                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                                        ..Default::default()
                                    }
                                })
                                .into()
                            } else {
                                Space::new()
                                    .width(Length::Fixed(FOLDER_DOTS_SLOT_W))
                                    .height(Length::Fixed(1.0))
                                    .into()
                            };

                            let folder_card = button(
                                container(
                                    dir_row(vec![
                                        icon_box.into(),
                                        Space::new().width(8).into(),
                                        column![
                                            text(label).size(13).color(OryxisColors::t().text_primary),
                                            Space::new().height(2),
                                            text(count_text).size(10).color(OryxisColors::t().text_muted),
                                        ]
                                        .width(Length::Fill)
                                        .align_x(crate::widgets::dir_align_x())
                                        .into(),
                                        actions_btn,
                                    ]).align_y(iced::Alignment::Center),
                                )
                                .padding(Padding { top: 8.0, right: 6.0, bottom: 8.0, left: 8.0 }),
                            )
                            .on_press(Message::OpenGroup(gid))
                            .width(CARD_WIDTH)
                            .style(|_, status| {
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

                            // Wrap in MouseArea so hover events drive the
                            // dots-button visibility (same UX as host cards).
                            let wrapped = MouseArea::new(folder_card)
                                .on_enter(Message::FolderCardHovered(gid))
                                .on_exit(Message::FolderCardUnhovered);
                            cards.push(wrapped.into());
                        }
            }
        }

        // Show host cards — filtered by active group and search
        let search_lower = self.host_search.to_lowercase();
        for (idx, conn) in self.connections.iter().enumerate() {
            // Filter: at root show ungrouped only, inside folder show that group
            if let Some(gid) = self.active_group {
                if conn.group_id != Some(gid) { continue; }
            } else if conn.group_id.is_some() {
                continue; // hide grouped hosts at root (they're inside folder cards)
            }

            // Filter by search query
            if !search_lower.is_empty() {
                let label_match = conn.label.to_lowercase().contains(&search_lower);
                let host_match = conn.hostname.to_lowercase().contains(&search_lower);
                if !label_match && !host_match { continue; }
            }

            let is_connected = self.tabs.iter().any(|t| t.label == conn.label);
            let auth_label = match conn.auth_method {
                AuthMethod::Auto => t("auth_auto"),
                AuthMethod::Password => t("auth_password"),
                AuthMethod::Key => t("auth_key"),
                AuthMethod::Agent => t("auth_agent"),
                AuthMethod::Interactive => t("auth_interactive"),
            };
            let subtitle = format!("{}@{}:{} · {}", conn.username.as_deref().unwrap_or("root"), conn.hostname, conn.port, auth_label);

            // Resolve icon + brand color from detected OS (if any). Disconnected
            // hosts use the app accent; connected ones use the brand color or
            // success green as fallback.
            let default_fallback = if is_connected {
                OryxisColors::t().success
            } else {
                OryxisColors::t().accent
            };
            let (os_glyph, icon_color) = crate::os_icon::resolve_for(
                conn.detected_os.as_deref(),
                conn.custom_icon.as_deref(),
                conn.custom_color.as_deref(),
                default_fallback,
            );
            // Fixed 32x32 square box centered on the glyph — Termius-style.
            let icon_box = container(os_glyph.size(14).color(Color::WHITE))
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(32.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(icon_color)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            // Vertical ellipsis (⋮) — always occupies the same space so the
            // card's geometry is stable; the button itself is only interactive
            // (and visible) on hover or when its context menu is open. A
            // transparent placeholder keeps the subtitle width budget constant.
            let show_dots = self.hovered_card == Some(idx) || self.card_context_menu == Some(idx);
            const DOTS_SLOT_W: f32 = 22.0;
            let dots_btn: Element<'_, Message> = if show_dots {
                button(
                    text("\u{22EE}").size(14).color(OryxisColors::t().text_muted),
                )
                .on_press(Message::ShowCardMenu(idx))
                .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .into()
            } else {
                // Invisible placeholder of identical width — reserves the
                // horizontal slot so the subtitle wrap budget never changes.
                Space::new().width(Length::Fixed(DOTS_SLOT_W)).height(Length::Fixed(1.0)).into()
            };

            // Card body: icon + labels + (dots or placeholder). Subtitle is
            // clamped to a single line via `wrapping::None` so the card's
            // height is identical for every host, regardless of how long the
            // "user@host:port · Auth" string is.
            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box.into(),
                        Space::new().width(8).into(),
                        column![
                            text(&conn.label)
                                .size(13)
                                .color(OryxisColors::t().text_primary)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(subtitle)
                                .size(10)
                                .color(OryxisColors::t().text_muted)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .clip(true)
                        .into(),
                        dots_btn,
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 8.0, right: 2.0, bottom: 8.0, left: 8.0 }),
            )
            .on_press(Message::ConnectSsh(idx))
            .width(CARD_WIDTH)
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

            // Wrap in MouseArea for hover tracking and right-click
            let wrapped = MouseArea::new(card_btn)
                .on_enter(Message::CardHovered(idx))
                .on_exit(Message::CardUnhovered)
                .on_right_press(Message::ShowCardMenu(idx));

            cards.push(container(wrapped).width(CARD_WIDTH).into());
        }

        // Grid layout (3 cols). Use `dir_row` so cards flow right-to-left
        // when RTL layout is active.
        let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in cards {
            current_row.push(card);
            if current_row.len() == 3 {
                grid_rows.push(dir_row(std::mem::take(&mut current_row)).spacing(12).into());
                grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            grid_rows.push(dir_row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        // Each grid row holds up to 3 fixed-width cards; once the row
        // is narrower than the available column width, the column's
        // cross-axis alignment decides whether the row sticks to the
        // leading or trailing edge. Use `dir_align_x()` so cards begin
        // from the trailing edge of the LTR layout (= leading edge of
        // the RTL layout), keeping them aligned with the toolbar title
        // / actions on the same side.
        // The column needs `Length::Fill` for `align_x` to have any
        // slack to align inside — without it the column shrinks to
        // content and the rows still hug the leading edge.
        let grid = scrollable(
            column(grid_rows)
                .width(Length::Fill)
                .padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 })
                .align_x(crate::widgets::dir_align_x()),
        ).height(Length::Fill);

        // ── Main + side panel ──
        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);

        if self.show_host_panel {
            let panel = self.view_host_panel();
            dir_row(vec![main_content.into(), panel])
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            main_content.into()
        }
    }
}
