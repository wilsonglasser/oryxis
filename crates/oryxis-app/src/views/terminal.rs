//! Terminal view + AI chat sidebar.

use std::sync::Arc;

use iced::border::Radius;
use iced::widget::{
    button, canvas, column, container, row, scrollable, text, text_input, MouseArea, Space,
};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_terminal::widget::TerminalView;

use crate::app::{Message, Oryxis};
use crate::state::{ChatMessage, ChatRole, TerminalTab};
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_terminal(&self) -> Element<'_, Message> {
        let chat_visible = self.active_tab
            .and_then(|idx| self.tabs.get(idx))
            .map(|tab| tab.chat_visible)
            .unwrap_or(false);

        let terminal_area: Element<'_, Message> = if let Some(tab_idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(tab_idx) {
                let term_view = TerminalView::new(Arc::clone(&tab.terminal))
                    .with_font_size(self.terminal_font_size)
                    .with_font_name(&self.terminal_font_name);
                let term_canvas: Element<'_, Message> = canvas(term_view)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();

                // Chat toggle button (top-right overlay)
                let toggle_btn = button(
                    container(
                        iced_fonts::lucide::message_circle().size(14).color(
                            if chat_visible { OryxisColors::t().accent } else { OryxisColors::t().text_muted }
                        ),
                    )
                    .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 }),
                )
                .on_press(Message::ToggleChatSidebar)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_surface,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                });

                let toggle_row = container(toggle_btn)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 0.0, left: 0.0 });

                let term_with_toggle: Element<'_, Message> = column![toggle_row, term_canvas]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();

                if chat_visible {
                    let sidebar = self.view_chat_sidebar(tab);
                    row![term_with_toggle, sidebar]
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                } else {
                    term_with_toggle
                }
            } else {
                container(text("No active session").size(14).color(OryxisColors::t().text_muted))
                    .center(Length::Fill).into()
            }
        } else {
            container(text("No active session").size(14).color(OryxisColors::t().text_muted))
                .center(Length::Fill).into()
        };

        container(terminal_area)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::TERMINAL_BG)),
                ..Default::default()
            })
            .into()
    }

    pub(crate) fn view_chat_sidebar(&self, tab: &TerminalTab) -> Element<'_, Message> {
        // ── Header ──
        let close_btn: Element<'_, Message> = MouseArea::new(
            container(
                text("X").size(14).color(OryxisColors::t().text_muted),
            )
            .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_hover)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }),
        )
        .on_press(Message::ToggleChatSidebar)
        .into();

        let header = container(
            row![
                iced_fonts::lucide::message_circle().size(14).color(OryxisColors::t().accent),
                Space::new().width(8),
                text("AI Chat").size(14).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                close_btn,
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 12.0, right: 12.0, bottom: 12.0, left: 12.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            border: Border {
                width: 0.0,
                color: OryxisColors::t().border,
                radius: Radius::from(0.0),
            },
            ..Default::default()
        });

        let header_separator = container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });

        // ── Messages list ──
        let mut messages_col = column![].spacing(8).padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 });

        if tab.chat_history.is_empty() {
            messages_col = messages_col.push(
                container(
                    column![
                        iced_fonts::lucide::message_circle().size(24).color(OryxisColors::t().text_muted),
                        Space::new().height(8),
                        text("Ask AI about this session").size(12).color(OryxisColors::t().text_muted),
                    ]
                    .align_x(iced::Alignment::Center),
                )
                .center_x(Length::Fill)
                .padding(Padding { top: 40.0, right: 0.0, bottom: 0.0, left: 0.0 }),
            );
        } else {
            for msg in &tab.chat_history {
                let bubble = self.view_chat_message(msg);
                messages_col = messages_col.push(bubble);
            }
        }

        if self.chat_loading {
            messages_col = messages_col.push(
                container(
                    text("Thinking...").size(12).color(OryxisColors::t().text_muted),
                )
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }),
            );
        }

        let messages_scroll = scrollable(messages_col)
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Input area ──
        let input_separator = container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });

        let send_btn = button(
            container(
                iced_fonts::lucide::arrow_right().size(14).color(OryxisColors::t().text_primary),
            )
            .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 }),
        )
        .on_press(Message::SendChat)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().accent,
                _ => OryxisColors::t().bg_surface,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        });

        let input_row = container(
            row![
                text_input("Ask AI...", &self.chat_input)
                    .on_input(Message::ChatInputChanged)
                    .on_submit(Message::SendChat)
                    .padding(10)
                    .width(Length::Fill),
                Space::new().width(4),
                send_btn,
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 12.0, bottom: 12.0, left: 12.0 })
        .width(Length::Fill);

        // ── Assemble sidebar ──
        container(
            column![header, header_separator, messages_scroll, input_separator, input_row]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(350)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                width: 1.0,
                color: OryxisColors::t().border,
                radius: Radius::from(0.0),
            },
            ..Default::default()
        })
        .into()
    }

    pub(crate) fn view_chat_message(&self, msg: &ChatMessage) -> Element<'_, Message> {
        match msg.role {
            ChatRole::User => {
                let bubble = container(
                    text(msg.content.clone()).size(13).color(Color::WHITE),
                )
                .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .max_width(280)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(12.0), ..Default::default() },
                    ..Default::default()
                });

                container(bubble)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .into()
            }
            ChatRole::Assistant => {
                let bubble = container(
                    text(msg.content.clone()).size(13).color(OryxisColors::t().text_primary),
                )
                .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .max_width(280)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(12.0), ..Default::default() },
                    ..Default::default()
                });

                container(bubble)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Left)
                    .into()
            }
            ChatRole::System => {
                let bubble = container(
                    text(msg.content.clone()).size(11).color(OryxisColors::t().text_muted),
                )
                .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
                .max_width(300)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color { r: 0.12, g: 0.12, b: 0.14, a: 1.0 })),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                });

                container(bubble)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Left)
                    .into()
            }
        }
    }




}
