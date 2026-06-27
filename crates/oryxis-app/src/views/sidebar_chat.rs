//! AI chat sidebar rendering: the assistant message bubbles + the custom
//! markdown viewer that injects Copy / Play buttons into fenced code
//! blocks. The chat tab body itself (message list + input) stays inline in
//! the sidebar shell (`terminal.rs::view_terminal_sidebar`); this module
//! holds the heavy per-message rendering it calls into.

use iced::border::Radius;
use iced::widget::{container, text};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::state::{ChatMessage, ChatRole};
use crate::theme::OryxisColors;

impl Oryxis {
    /// Markdown settings shared by every assistant bubble in the chat
    /// sidebar. Built once per sidebar render by the caller; deriving
    /// the style from the theme per message per frame was measurable
    /// on long conversations.
    ///
    /// Compact heading scale, `with_text_size` ramps h1=2x and
    /// h2=1.75x base which reads as huge in a narrow sidebar. We
    /// override to a tighter ladder anchored at 13 px body.
    /// SauceCodePro Nerd Font is bundled in main.rs and carries the
    /// full Nerd Font PUA glyph set. Wire it into the markdown style
    /// so inline `code` and fenced code blocks render
    /// Powerline/Devicon/etc. glyphs the user pastes or the AI emits,
    /// matching what the terminal panel already shows. Body prose
    /// stays on the proportional default; cosmic-text's PUA fallback
    /// isn't reliable enough to count on for non-code text.
    pub(crate) fn chat_markdown_settings(&self) -> iced::widget::markdown::Settings {
        let mut md_style = iced::widget::markdown::Style::from(self.theme());
        let nerd = iced::Font::new("SauceCodePro Nerd Font");
        md_style.inline_code_font = nerd;
        md_style.code_block_font = nerd;
        iced::widget::markdown::Settings {
            text_size: 13.into(),
            h1_size: 17.into(),
            h2_size: 15.into(),
            h3_size: 14.into(),
            h4_size: 13.into(),
            h5_size: 13.into(),
            h6_size: 13.into(),
            code_size: 12.into(),
            spacing: 8.into(),
            style: md_style,
            selectable: true,
            group_selection: true,
        }
    }

    pub(crate) fn view_chat_message<'a>(
        &'a self,
        msg: &'a ChatMessage,
        md_settings: iced::widget::markdown::Settings,
    ) -> Element<'a, Message> {
        match msg.role {
            ChatRole::User => {
                // The accent fill pairs with the per-theme `button_text`
                // (same rule the rest of the CTA buttons follow). User
                // messages stay capped at 280 px and right-aligned to
                // keep the standard chat shape.
                let bubble = container(
                    text(msg.content.as_str())
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
                // returned Element, which is why we cache, instead of
                // parsing per render.
                // Heading scale / fonts come from `md_settings`, built
                // once per sidebar render in `chat_markdown_settings`.
                // Custom viewer overrides `code_block` to add Copy +
                // Play buttons inside each fenced block; everything
                // else (paragraphs, headings, lists) renders with the
                // default markdown behaviour.
                let md: Element<'_, Message> = iced::widget::markdown::view_with(
                    msg.parsed_md.iter(),
                    md_settings,
                    &ChatMdViewer,
                );

                // Bubble fills the sidebar width, earlier we clamped
                // at 300 px which left a wide empty strip when the user
                // dragged the sidebar wider. The chat is the only thing
                // in this column, so wider = more useful. Per-code-block
                // Copy / Play buttons are injected by `ChatMdViewer`
                // inside each fenced code block; the bubble itself
                // doesn't need a separate copy affordance.
                let bubble = container(md)
                    .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
                    .width(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        text_color: Some(OryxisColors::t().text_primary),
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
                    text(msg.content.as_str()).size(11).color(OryxisColors::t().text_muted),
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
            ChatRole::PendingTool => {
                // AI proposed a `risky` command, show it inline with
                // RUN / ALWAYS RUN / DENY buttons. Warning-tinted
                // surface so the user notices it's an action prompt,
                // not a regular message.
                // The three action messages each need an owned copy of
                // the command; the displayed text below borrows it.
                let cmd_for_run = msg.content.clone();
                let cmd_for_always = msg.content.clone();
                let cmd_for_deny = msg.content.clone();
                let warning_subtle = Color {
                    a: 0.12,
                    ..OryxisColors::t().warning
                };
                let warning_border = Color {
                    a: 0.45,
                    ..OryxisColors::t().warning
                };
                let bubble = container(
                    iced::widget::column![
                        iced::widget::row![
                            iced_fonts::lucide::triangle_alert()
                                .size(13)
                                .color(OryxisColors::t().warning),
                            iced::widget::Space::new().width(6),
                            text(t("ai_wants_to_run"))
                                .size(12)
                                .font(iced::Font {
                                    weight: iced::font::Weight::Semibold,
                                    ..iced::Font::new(
                                        crate::theme::SYSTEM_UI_FAMILY
                                    )
                                })
                                .color(OryxisColors::t().text_primary),
                        ]
                        .align_y(iced::Alignment::Center),
                        iced::widget::Space::new().height(6),
                        container(
                            text(msg.content.as_str())
                                .size(12)
                                .font(iced::Font::new("SauceCodePro Nerd Font"))
                                .color(OryxisColors::t().text_primary),
                        )
                        .padding(Padding {
                            top: 6.0,
                            right: 8.0,
                            bottom: 6.0,
                            left: 8.0,
                        })
                        .width(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Background::Color(
                                OryxisColors::t().bg_surface,
                            )),
                            border: Border {
                                radius: Radius::from(6.0),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        iced::widget::Space::new().height(8),
                        iced::widget::row![
                            pending_tool_btn(
                                t("ai_tool_run"),
                                Message::ChatToolApprove(cmd_for_run),
                                OryxisColors::t().accent,
                                OryxisColors::t().button_text,
                            ),
                            pending_tool_btn(
                                t("ai_tool_always"),
                                Message::ChatToolApproveAlways(cmd_for_always),
                                OryxisColors::t().success,
                                OryxisColors::t().button_text,
                            ),
                            pending_tool_btn(
                                t("ai_tool_deny"),
                                Message::ChatToolDeny(cmd_for_deny),
                                OryxisColors::t().bg_hover,
                                OryxisColors::t().text_primary,
                            ),
                        ]
                        .spacing(6)
                        .align_y(iced::Alignment::Center),
                    ],
                )
                .padding(Padding {
                    top: 10.0,
                    right: 12.0,
                    bottom: 10.0,
                    left: 12.0,
                })
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(warning_subtle)),
                    border: Border {
                        radius: Radius::from(12.0),
                        color: warning_border,
                        width: 1.0,
                    },
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
                            text(t("ai_provider_failed"))
                                .size(12)
                                .color(OryxisColors::t().error),
                        ]
                        .align_y(iced::Alignment::Center),
                        iced::widget::Space::new().height(4),
                        text(msg.content.as_str())
                            .size(11)
                            .color(OryxisColors::t().text_muted),
                        iced::widget::Space::new().height(8),
                        crate::widgets::styled_button(
                            t("retry"),
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

/// Custom markdown viewer that injects Copy / Play buttons inside each
/// fenced code block. Everything else (paragraphs, headings, lists)
/// renders with the iced default. Text widgets in iced 0.14 aren't
/// selectable, so the Copy button is the user's escape hatch for
/// pulling commands out of the assistant's response.
struct ChatMdViewer;

impl<'a>
    iced::widget::markdown::Viewer<
        'a,
        Message,
        iced::Theme,
        iced::Renderer,
    > for ChatMdViewer
{
    fn on_link_click(_url: iced::widget::markdown::Uri) -> Message {
        Message::NoOp
    }

    fn code_block(
        &self,
        settings: iced::widget::markdown::Settings,
        _language: Option<&'a str>,
        code: &'a str,
        lines: &'a [iced::widget::markdown::Text],
    ) -> Element<'a, Message> {
        // Reuse the stock code-block rendering for the actual text /
        // syntax highlighting / horizontal scroll, then stack a tiny
        // toolbar of Copy + Play in the top-right corner. Built with
        // `MouseArea` instead of `button(...)` because the `button`
        // widget chain inside our chat scrollable swallows clicks
        // (same iced quirk `chat_header_btn` works around, see its
        // comment below).
        let body: Element<'a, Message> = iced::widget::markdown::code_block(
            settings,
            lines,
            Self::on_link_click,
        );
        let copy = iced::widget::MouseArea::new(
            container(
                iced_fonts::lucide::copy()
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding {
                top: 3.0,
                right: 5.0,
                bottom: 3.0,
                left: 5.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.10,
                    ..OryxisColors::t().text_secondary
                })),
                border: Border {
                    radius: Radius::from(4.0),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .on_press(Message::CopyToClipboard(code.to_string()))
        .interaction(iced::mouse::Interaction::Pointer);
        let play = iced::widget::MouseArea::new(
            container(
                iced_fonts::lucide::play()
                    .size(12)
                    .color(OryxisColors::t().success),
            )
            .padding(Padding {
                top: 3.0,
                right: 5.0,
                bottom: 3.0,
                left: 5.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.12,
                    ..OryxisColors::t().success
                })),
                border: Border {
                    radius: Radius::from(4.0),
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        // Manually clicking Play is the user's explicit go-ahead, so
        // we route it through `ChatToolApprove` (run once) and skip
        // the risk gate, re-prompting after a deliberate Play would
        // be redundant.
        .on_press(Message::ChatToolApprove(code.to_string()))
        .interaction(iced::mouse::Interaction::Pointer);
        let toolbar = container(
            iced::widget::row![copy, iced::widget::Space::new().width(4), play]
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .padding(Padding {
            top: 4.0,
            right: 4.0,
            bottom: 0.0,
            left: 0.0,
        });
        iced::widget::Stack::new()
            .push(body)
            .push(toolbar)
            .into()
    }

}

/// Filled chip-button used by the PendingTool confirmation prompt
/// (`Run` / `Always` / `Deny`). Kept on `MouseArea` (fires on press)
/// historically because clicks here appeared to be "eaten"; the real
/// cause was the terminal canvas capturing every left release
/// (`oryxis-terminal/src/widget.rs`), now gated to only capture releases
/// over itself, so plain `button` works in the sidebar again.
fn pending_tool_btn<'a>(
    label: &'a str,
    msg: Message,
    bg: Color,
    fg: Color,
) -> Element<'a, Message> {
    use iced::widget::MouseArea;
    MouseArea::new(
        container(
            text(label.to_owned())
                .size(12)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(fg),
        )
        .padding(Padding {
            top: 5.0,
            right: 12.0,
            bottom: 5.0,
            left: 12.0,
        })
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .on_press(msg)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}
