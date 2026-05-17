//! Snippets (saved commands) list and editor panel.

use iced::border::Radius;
use iced::widget::{button, column, container, scrollable, text, text_input, MouseArea, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_align_x, dir_row, distribute_card_grid};

impl Oryxis {
    pub(crate) fn view_snippets(&self) -> Element<'_, Message> {
        let toolbar = container(
            dir_row(vec![
                text(t("snippets")).size(20).color(OryxisColors::t().text_primary).into(),
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
                                text(t("snippet_btn")).size(11).font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                }).color(fg).into(),
                            ]).align_y(iced::Alignment::Center),
                        )
                        .center_y(Length::Fixed(24.0))
                        .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
                    )
                    .on_press(Message::ShowSnippetPanel)
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

        let status: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        // Section title aligns with the card's leading border (the
        // scrollable's left padding is already 24 px; the title sits
        // outside the scrollable so we apply the same here).
        let section_title = container(
            text(t("commands")).size(14).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 4.0, right: 24.0, bottom: 8.0, left: 24.0 });

        let mut cards: Vec<Element<'_, Message>> = Vec::new();

        if self.snippets.is_empty() {
            let empty_state = container(
                column![
                    container(
                        iced_fonts::lucide::code().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(crate::i18n::t("create_snippet_title")).size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(crate::i18n::t("create_snippet_desc"))
                        .size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    crate::widgets::cta_button(
                        crate::i18n::t("new_snippet").to_string(),
                        Message::ShowSnippetPanel,
                    ),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_snippet_panel {
                let panel = self.view_snippet_panel();
                return dir_row(vec![main_content.into(), panel])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
            return main_content.into();
        }

        let snippet_needle = self.snippet_search.to_lowercase();
        for (idx, snip) in self.snippets.iter().enumerate() {
            if !snippet_needle.is_empty()
                && !snip.label.to_lowercase().contains(&snippet_needle)
                && !snip.command.to_lowercase().contains(&snippet_needle)
            {
                continue;
            }
            // Use host_icon so the snippet badge follows the global
            // `default_host_icon` shape (Circular by default in v0.7)
            // and the rest of the cards on this screen look the same.
            let snip_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.setting_default_host_icon,
            );
            // `line_height(1.0)` collapses the default text padding so
            // the glyph sits at the optical centre of the badge; the
            // default ~1.2 multiplier pushed it visually upward and
            // the badge looked misaligned next to the label column.
            let glyph_el: Element<'_, Message> = iced_fonts::lucide::code()
                .size(14)
                .line_height(1.0)
                .color(Color::WHITE)
                .into();
            let icon_box = crate::widgets::host_icon(
                snip_style,
                OryxisColors::t().accent,
                &snip.label,
                Some(glyph_el),
                32.0,
            );

            // Vertical ellipsis (⋮) only when the row is hovered, so it
            // matches the host-card / keychain affordance. A fixed
            // placeholder reserves the slot in the unhovered state so
            // the label column width stays constant.
            const SNIP_DOTS_SLOT_W: f32 = 22.0;
            let show_dots = self.hovered_snippet_card == Some(idx);
            let edit_btn: Element<'_, Message> = if show_dots {
                button(text("\u{22EE}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::EditSnippet(idx))
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
                    .width(Length::Fixed(SNIP_DOTS_SLOT_W))
                    .height(Length::Fixed(20.0))
                    .into()
            };

            let cmd_preview = if snip.command.len() > 30 {
                format!("{}...", &snip.command[..30])
            } else {
                snip.command.clone()
            };

            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(8).into(),
                        column![
                            text(&snip.label)
                                .size(13)
                                .color(OryxisColors::t().text_primary)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(cmd_preview)
                                .size(10)
                                .color(OryxisColors::t().text_muted)
                                .font(iced::Font::MONOSPACE)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ].width(Length::Fill).into(),
                        edit_btn,
                    ]).align_y(iced::Alignment::Center),
                )
                // Match the host card padding (top/bottom 8, left 2,
                // right reserved for the trailing slot) so the row
                // height + indent line up with the rest of the grid.
                .padding(Padding { top: 8.0, right: 2.0, bottom: 8.0, left: 2.0 }),
            )
            .on_press(Message::RunSnippet(idx))
            .width(Length::Fill)
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

            // Wrap the button in a MouseArea so we can track hover
            // for the kebab show/hide affordance, same trick the host
            // cards use.
            let wrapped: Element<'_, Message> = MouseArea::new(card_btn)
                .on_enter(Message::SnippetCardHovered(idx))
                .on_exit(Message::SnippetCardUnhovered)
                .into();
            cards.push(container(wrapped).width(Length::Fill).clip(true).into());
        }

        let nav_width = if self.sidebar_collapsed {
            crate::app::SIDEBAR_WIDTH_COLLAPSED
        } else {
            crate::app::SIDEBAR_WIDTH
        };
        let panel_width = if self.show_snippet_panel { PANEL_WIDTH } else { 0.0 };
        let available = (self.window_size.width - nav_width - panel_width - 48.0).max(0.0);
        let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
        let snippets_grid = distribute_card_grid(cards, cols, 12.0, 12.0);

        let grid = scrollable(
            column![snippets_grid].padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        // Inline search bar in Classic mode (Workspace puts it on
        // the contextual sub-nav). Collapses to zero height in
        // Workspace so we don't render the input twice.
        let workspace_mode = self.setting_layout_mode == "workspace";
        let search_bar: Element<'_, Message> = if workspace_mode {
            Space::new().height(0).into()
        } else {
            container(
                text_input(t("search_snippets"), &self.snippet_search)
                    .on_input(Message::SnippetSearchChanged)
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
            .width(Length::Fill)
            .into()
        };

        let main_content = column![toolbar, search_bar, status, section_title, grid]
            .width(Length::Fill).height(Length::Fill);

        if self.show_snippet_panel {
            let panel = self.view_snippet_panel();
            dir_row(vec![main_content.into(), panel]).width(Length::Fill).height(Length::Fill).into()
        } else {
            main_content.into()
        }
    }

    pub(crate) fn view_snippet_panel(&self) -> Element<'_, Message> {
        let is_editing = self.snippet_editing_id.is_some();
        let title = if is_editing { t("edit_snippet") } else { t("new_snippet") };

        let panel_header = container(
            dir_row(vec![
                text(title).size(18).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideSnippetPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }).into(),
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        let form = column![
            text(t("name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("restart-nginx", &self.snippet_label)
                .on_input(Message::SnippetLabelChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            Space::new().height(14),
            text(t("command_label")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("sudo systemctl restart nginx", &self.snippet_command)
                .on_input(Message::SnippetCommandChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        let panel_error: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            Element::from(text(err.clone()).size(11).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        let save_btn = button(
            container(text(crate::i18n::t("save")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill).center_x(Length::Fill),
        )
        .on_press(Message::SaveSnippet)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let mut bottom = column![save_btn];
        if let Some(edit_id) = self.snippet_editing_id
            && let Some(idx) = self.snippets.iter().position(|s| s.id == edit_id) {
                let del_btn = button(
                    container(text(crate::i18n::t("delete")).size(13).color(OryxisColors::t().error))
                        .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                        .width(Length::Fill).center_x(Length::Fill),
                )
                .on_press(Message::DeleteSnippet(idx))
                .width(Length::Fill)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().error, width: 1.0 },
                    ..Default::default()
                });
                bottom = bottom.push(Space::new().height(8));
                bottom = bottom.push(del_btn);
            }

        let panel_content = column![
            panel_header,
            container(
                column![
                    form,
                    Space::new().height(12),
                    panel_error,
                    Space::new().height(Length::Fill),
                    bottom,
                ].height(Length::Fill).width(Length::Fill).align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 })
            .height(Length::Fill),
        ].height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }
}
