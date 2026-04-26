//! Terminal view + AI chat sidebar.

use std::sync::Arc;

use iced::border::Radius;
use iced::widget::{
    button, canvas, column, container, row, scrollable, text, MouseArea, Space,
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
                let term_view = TerminalView::new(Arc::clone(&tab.active().terminal))
                    .with_font_size(self.terminal_font_size)
                    .with_font_name(&self.terminal_font_name)
                    .with_copy_on_select(self.setting_copy_on_select)
                    .with_bold_is_bright(self.setting_bold_is_bright)
                    .with_keyword_highlight(self.setting_keyword_highlight);
                let term_canvas: Element<'_, Message> = canvas(term_view)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();

                // Chat toggle — overlaid in the top-right corner of the
                // terminal canvas so it doesn't steal vertical space. The
                // sparkles always render in accent and the toggle hides
                // entirely when the sidebar is open (the sidebar header
                // shows the same sparkle, no point duplicating it).
                let term_with_toggle: Element<'_, Message> = if chat_visible {
                    term_canvas
                } else {
                    let toggle_btn = button(
                        container(
                            iced_fonts::lucide::sparkles()
                                .size(14)
                                .color(OryxisColors::t().accent),
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

                    let toggle_overlay: Element<'_, Message> = container(toggle_btn)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Right)
                        .align_y(iced::alignment::Vertical::Top)
                        .padding(Padding { top: 4.0, right: 8.0, bottom: 0.0, left: 0.0 })
                        .into();

                    iced::widget::Stack::new()
                        .push(term_canvas)
                        .push(toggle_overlay)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                };

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

    pub(crate) fn view_chat_sidebar<'a>(&'a self, tab: &'a TerminalTab) -> Element<'a, Message> {
        // ── Header ──
        // Reset (clears chat history) and Close X — both transparent at
        // rest, bg_hover on hover, same affordance pattern as the chrome.
        let reset_btn = chat_header_btn(iced_fonts::lucide::rotate_ccw(), Message::ChatResetConversation);
        let close_btn = chat_header_btn(iced_fonts::lucide::x(), Message::ToggleChatSidebar);

        let header = container(
            row![
                iced_fonts::lucide::sparkles().size(14).color(OryxisColors::t().accent),
                Space::new().width(8),
                text("AI Chat").size(14).color(OryxisColors::t().text_primary),
                Space::new().width(Length::Fill),
                reset_btn,
                Space::new().width(4),
                close_btn,
            ]
            // Row needs an explicit Fill width — without it, the inner
            // `Space::Fill` collapses and the reset/close buttons end up
            // packed against the title text instead of pushed to the
            // right edge of the sidebar (so clicks at the visual right
            // were landing on empty container area, not the buttons).
            .width(Length::Fill)
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
                        iced_fonts::lucide::sparkles().size(24).color(OryxisColors::t().text_muted),
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
                // Skip empty assistant placeholders — they exist as
                // staging slots for streaming chunks; an empty one is
                // either pre-first-token (covered by the "Thinking..."
                // bubble below) or a stream that ended before any text
                // arrived (e.g. straight to a tool call). Either way,
                // an empty padded box would just look like a glitch.
                if msg.role == crate::state::ChatRole::Assistant
                    && msg.content.is_empty()
                {
                    continue;
                }
                let bubble = self.view_chat_message(msg);
                messages_col = messages_col.push(bubble);
            }
        }

        // Hide the "Thinking..." indicator once the model has started
        // streaming visible text — the streaming bubble itself is the
        // signal of activity, and showing both reads as a stutter.
        let actively_streaming = tab
            .chat_history
            .last()
            .map(|m| m.role == crate::state::ChatRole::Assistant && !m.content.is_empty())
            .unwrap_or(false);
        if self.chat_loading && !actively_streaming {
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
            .id(iced::widget::Id::new("chat-scroll"))
            .on_scroll(|viewport| Message::ChatScrolled(viewport.relative_offset().y))
            .width(Length::Fill)
            .height(Length::Fill);

        // ── Input area ──
        let input_separator = container(Space::new().height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            });

        // Multi-line input — grows with content up to ~6 lines (~150 px),
        // then scrolls internally. Enter sends the message; Shift+Enter
        // inserts a newline. No send button — every chat-style UI uses
        // Enter today, so the arrow was just visual noise.
        let chat_editor = iced::widget::text_editor(&self.chat_input)
            .placeholder("Ask AI...")
            .on_action(Message::ChatInputAction)
            .padding(10)
            .height(Length::Shrink)
            .key_binding(|key_press| {
                use iced::keyboard::{key::Named, Key};
                use iced::widget::text_editor::{Binding, KeyPress};
                let KeyPress { key, modifiers, .. } = &key_press;
                if matches!(key, Key::Named(Named::Enter)) && !modifiers.shift() {
                    return Some(Binding::Custom(Message::SendChat));
                }
                Binding::from_key_press(key_press)
            })
            .style(|_theme, status| {
                let c = OryxisColors::t();
                let (border_color, border_width) = match status {
                    iced::widget::text_editor::Status::Focused { .. } => (c.accent, 1.5),
                    _ => (c.border, 1.0),
                };
                iced::widget::text_editor::Style {
                    background: Background::Color(c.bg_surface),
                    border: Border {
                        radius: Radius::from(crate::widgets::INPUT_RADIUS),
                        width: border_width,
                        color: border_color,
                    },
                    placeholder: c.text_muted,
                    value: c.text_primary,
                    selection: c.accent,
                }
            });

        let input_row = container(
            container(chat_editor).max_height(150.0),
        )
        .padding(Padding { top: 8.0, right: 12.0, bottom: 12.0, left: 12.0 })
        .width(Length::Fill);

        // 4 px draggable handle on the left edge — clicking starts a
        // resize, the global mouse-move handler in app.rs follows the
        // cursor, and the global mouse-up stops the drag.
        let resize_handle: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fixed(4.0)).height(Length::Fill))
                .width(Length::Fixed(4.0))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
        )
        .on_press(Message::ChatSidebarResizeStart)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into();

        // ── Assemble sidebar ──
        let panel = container(
            column![header, header_separator, messages_scroll, input_separator, input_row]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        });

        container(
            row![resize_handle, panel]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fixed(self.chat_sidebar_width))
        .height(Length::Fill)
        .into()
    }

    pub(crate) fn view_chat_message<'a>(&'a self, msg: &'a ChatMessage) -> Element<'a, Message> {
        match msg.role {
            ChatRole::User => {
                // The accent fill pairs with the per-theme `button_text`
                // (same rule the rest of the CTA buttons follow). User
                // messages stay capped at 280 px and right-aligned to
                // keep the standard chat shape.
                let bubble = container(
                    text(msg.content.clone())
                        .size(13)
                        .color(OryxisColors::t().button_text),
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
                // Markdown items are pre-parsed when the message is added
                // to history (state.rs::ChatMessage.parsed_md). The view
                // needs to borrow that slice, so it must outlive the
                // returned Element — which is why we cache, instead of
                // parsing per render.
                // Compact heading scale — `with_text_size` ramps h1=2x and
                // h2=1.75x base which reads as huge in a narrow sidebar.
                // We override to a tighter ladder anchored at 13 px body.
                let md_settings = iced::widget::markdown::Settings {
                    text_size: 13.into(),
                    h1_size: 17.into(),
                    h2_size: 15.into(),
                    h3_size: 14.into(),
                    h4_size: 13.into(),
                    h5_size: 13.into(),
                    h6_size: 13.into(),
                    code_size: 12.into(),
                    spacing: 8.into(),
                    style: iced::widget::markdown::Style::from(self.theme()),
                };
                let md: Element<'_, Message> = iced::widget::markdown::view(
                    &msg.parsed_md,
                    md_settings,
                )
                .map(|_uri: iced::widget::markdown::Uri| Message::NoOp);

                // Bubble fills the sidebar width — earlier we clamped
                // at 300 px which left a wide empty strip when the user
                // dragged the sidebar wider. The chat is the only thing
                // in this column, so wider = more useful.
                //
                // Right-padding bumped to 36 px so the floating Copy
                // button has somewhere to sit without overlapping the
                // first line of text.
                let bubble = container(md)
                    .padding(Padding { top: 8.0, right: 36.0, bottom: 8.0, left: 12.0 })
                    .width(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        text_color: Some(OryxisColors::t().text_primary),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    });

                // Hover-affordance Copy button — text widgets in iced
                // 0.14 don't support text selection, so an explicit
                // copy-the-whole-message button covers the gap. Sits
                // top-right of the bubble; subtle by default, brighter
                // on hover.
                let copy_msg = msg.content.clone();
                let copy_btn = iced::widget::button(
                    iced_fonts::lucide::copy()
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .on_press(Message::CopyToClipboard(copy_msg))
                .padding(Padding {
                    top: 4.0,
                    right: 6.0,
                    bottom: 4.0,
                    left: 6.0,
                })
                .style(|_, status| {
                    let bg = match status {
                        iced::widget::button::Status::Hovered => {
                            Color { a: 0.25, ..OryxisColors::t().text_secondary }
                        }
                        iced::widget::button::Status::Pressed => {
                            Color { a: 0.4, ..OryxisColors::t().text_secondary }
                        }
                        _ => Color::TRANSPARENT,
                    };
                    iced::widget::button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    }
                });
                let copy_overlay = container(copy_btn)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(Padding {
                        top: 6.0,
                        right: 6.0,
                        bottom: 0.0,
                        left: 0.0,
                    });
                let stacked = iced::widget::Stack::new()
                    .push(bubble)
                    .push(copy_overlay);

                container(stacked)
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
            ChatRole::Error => {
                // Distinct error treatment: red-tinted border, alert
                // icon, "Retry" button. Stops a transient API blip from
                // looking like an actual model reply that the user
                // would otherwise scroll past.
                let bubble = container(
                    iced::widget::column![
                        iced::widget::row![
                            iced_fonts::lucide::circle_alert()
                                .size(13)
                                .color(OryxisColors::t().error),
                            iced::widget::Space::new().width(6),
                            text("Failed to reach the AI provider")
                                .size(12)
                                .color(OryxisColors::t().error),
                        ]
                        .align_y(iced::Alignment::Center),
                        iced::widget::Space::new().height(4),
                        text(msg.content.clone())
                            .size(11)
                            .color(OryxisColors::t().text_muted),
                        iced::widget::Space::new().height(8),
                        crate::widgets::styled_button(
                            "Retry",
                            Message::ChatRetry,
                            OryxisColors::t().accent,
                        ),
                    ],
                )
                .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .max_width(300)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color {
                        a: 0.10,
                        ..OryxisColors::t().error
                    })),
                    border: Border {
                        radius: Radius::from(12.0),
                        color: Color {
                            a: 0.40,
                            ..OryxisColors::t().error
                        },
                        width: 1.0,
                    },
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

/// Header glyph button used in the chat sidebar (reset, close). Built with
/// `MouseArea` directly instead of `button` to bypass any iced widget-tree
/// quirk that was eating clicks on the previous `button(...)` version. The
/// click area is the icon + 28×24 padding box.
fn chat_header_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
) -> Element<'a, Message> {
    use iced::widget::MouseArea;
    MouseArea::new(
        container(icon.size(13).color(OryxisColors::t().text_muted))
            .center_x(Length::Fixed(28.0))
            .center_y(Length::Fixed(24.0))
            .style(|_| container::Style {
                border: Border {
                    radius: Radius::from(4.0),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .on_press(msg)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}
