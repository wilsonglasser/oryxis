//! Snippets (saved commands) list and editor panel.

use iced::border::Radius;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_snippets(&self) -> Element<'_, Message> {
        let toolbar = container(
            row![
                text("Snippets").size(20).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(
                    container(
                        row![
                            text("+").size(13).font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::with_name("Inter")
                            }).color(OryxisColors::t().text_primary),
                            Space::new().width(4),
                            text("SNIPPET").size(11).font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::with_name("Inter")
                            }).color(OryxisColors::t().text_primary),
                        ].align_y(iced::Alignment::Center),
                    )
                    .center_y(Length::Fixed(24.0))
                    .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
                )
                .on_press(Message::ShowSnippetPanel)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().accent_hover,
                        _ => OryxisColors::t().accent,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let status: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        let section_title = container(
            text("Commands").size(14).color(OryxisColors::t().text_muted),
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
                    button(
                        container(text(crate::i18n::t("new_snippet")).size(14).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                            .width(380)
                            .center_x(380),
                    )
                    .on_press(Message::ShowSnippetPanel)
                    .width(380)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);

            if self.show_snippet_panel {
                let panel = self.view_snippet_panel();
                return row![main_content, panel]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
            return main_content.into();
        }

        for (idx, snip) in self.snippets.iter().enumerate() {
            let icon_box = container(iced_fonts::lucide::code().size(14).color(Color::WHITE))
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

            let edit_btn = button(text("...").size(12).color(OryxisColors::t().text_muted))
                .on_press(Message::EditSnippet(idx))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border::default(),
                    ..Default::default()
                });

            let cmd_preview = if snip.command.len() > 30 {
                format!("{}...", &snip.command[..30])
            } else {
                snip.command.clone()
            };

            let card = button(
                container(
                    row![
                        icon_box,
                        Space::new().width(12),
                        column![
                            text(&snip.label).size(13).color(OryxisColors::t().text_primary),
                            Space::new().height(2),
                            text(cmd_preview).size(10).color(OryxisColors::t().text_muted).font(iced::Font::MONOSPACE),
                        ].width(Length::Fill),
                        edit_btn,
                    ].align_y(iced::Alignment::Center),
                )
                .padding(16),
            )
            .on_press(Message::RunSnippet(idx))
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

            cards.push(card.into());
        }

        let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for card in cards {
            current_row.push(card);
            if current_row.len() == 3 {
                grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
                grid_rows.push(Space::new().height(12).into());
            }
        }
        if !current_row.is_empty() {
            while current_row.len() < 3 {
                current_row.push(Space::new().width(CARD_WIDTH).into());
            }
            grid_rows.push(row(std::mem::take(&mut current_row)).spacing(12).into());
        }

        let grid = scrollable(
            column(grid_rows).padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        let main_content = column![toolbar, status, section_title, grid]
            .width(Length::Fill).height(Length::Fill);

        if self.show_snippet_panel {
            let panel = self.view_snippet_panel();
            row![main_content, panel].width(Length::Fill).height(Length::Fill).into()
        } else {
            main_content.into()
        }
    }

    pub(crate) fn view_snippet_panel(&self) -> Element<'_, Message> {
        let is_editing = self.snippet_editing_id.is_some();
        let title = if is_editing { "Edit Snippet" } else { "New Snippet" };

        let panel_header = container(
            row![
                text(title).size(18).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                button(text("X").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideSnippetPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }),
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        let form = column![
            text("Name").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("restart-nginx", &self.snippet_label)
                .on_input(Message::SnippetLabelChanged)
                .padding(10),
            Space::new().height(14),
            text("Command").size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text_input("sudo systemctl restart nginx", &self.snippet_command)
                .on_input(Message::SnippetCommandChanged)
                .padding(10),
        ];

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
                ].height(Length::Fill),
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
