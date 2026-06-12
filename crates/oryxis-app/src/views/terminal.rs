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
use crate::i18n::t;
use crate::state::{ChatMessage, ChatRole, TerminalTab};
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn view_terminal(&self) -> Element<'_, Message> {
        let chat_visible = self.active_tab
            .and_then(|idx| self.tabs.get(idx))
            .map(|tab| tab.chat_visible)
            .unwrap_or(false);

        let terminal_area: Element<'_, Message> = if let Some(tab_idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(tab_idx) {
                // Render the tab's panes through a `pane_grid`. With one
                // pane this is visually identical to the old single canvas;
                // splits add cells. Each cell gets a focus border (only
                // visible once there's more than one pane) and the grid
                // wires click-to-focus + drag-to-resize.
                let focused = tab.focused;
                let multipane = tab.pane_grid.panes.len() > 1;
                let grid = iced::widget::pane_grid(&tab.pane_grid, move |pane, pane_data, _max| {
                    let is_focused = pane == focused;
                    // The focus border only shows when there's more than one
                    // pane; the mouse-report gate uses real focus regardless.
                    let show_border = multipane && is_focused;
                    let border_color = if show_border {
                        OryxisColors::t().accent
                    } else {
                        OryxisColors::t().border
                    };
                    iced::widget::pane_grid::Content::new(
                        container(self.render_pane_canvas(pane_data, is_focused))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .style(move |_| container::Style {
                                border: Border {
                                    color: border_color,
                                    width: if multipane { 1.0 } else { 0.0 },
                                    radius: Radius::from(0.0),
                                },
                                ..Default::default()
                            }),
                    )
                })
                .on_click(Message::FocusPane)
                .on_resize(8, Message::ResizePane)
                .spacing(if multipane { 4 } else { 0 })
                .width(Length::Fill)
                .height(Length::Fill);

                // The AI/sidebar toggle now lives in the tab bar (panel
                // button right of `+`), so the terminal canvas no longer
                // carries its own floating sparkle overlay.
                let term_with_toggle: Element<'_, Message> = grid.into();

                // The session-group editor renders here, as a sibling of the
                // grid inside the terminal area, the same way the chat sidebar
                // does. Wrapping the whole terminal container from outside
                // (view_content) instead left the canvas eating clicks meant
                // for the panel, so keep it inside.
                if chat_visible || self.show_session_group_panel {
                    let mut children = vec![term_with_toggle];
                    if chat_visible {
                        children.push(self.view_terminal_sidebar(tab));
                    }
                    if self.show_session_group_panel {
                        children.push(self.view_session_group_panel());
                    }
                    dir_row(children)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                } else {
                    term_with_toggle
                }
            } else {
                container(text(t("no_active_session")).size(14).color(OryxisColors::t().text_muted))
                    .center(Length::Fill).into()
            }
        } else {
            container(text(t("no_active_session")).size(14).color(OryxisColors::t().text_muted))
                .center(Length::Fill).into()
        };

        container(terminal_area)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().terminal_bg)),
                ..Default::default()
            })
            .into()
    }

    /// Build the terminal canvas for one pane, applying the global font /
    /// rendering settings. Shared by every `pane_grid` cell. `is_focused`
    /// gates mouse-tracking reports so a focus-click on an inactive pane
    /// doesn't inject a stray report.
    fn render_pane_canvas<'a>(
        &'a self,
        pane: &'a crate::state::Pane,
        is_focused: bool,
    ) -> Element<'a, Message> {
        let term_view = TerminalView::new(Arc::clone(&pane.terminal))
            .focused(is_focused)
            .with_font_size(self.terminal_font_size)
            .with_font_name(&self.terminal_font_name)
            .with_copy_on_select(self.setting_copy_on_select)
            .with_right_click_copy(self.setting_right_click_copy)
            .with_bold_is_bright(self.setting_bold_is_bright)
            .with_keyword_highlight(self.setting_keyword_highlight)
            .with_smart_contrast(self.setting_smart_contrast)
            .with_word_delimiters(&self.setting_word_delimiters)
            .on_font_size_increase(Message::TerminalFontSizeIncrease)
            .on_font_size_decrease(Message::TerminalFontSizeDecrease)
            .on_paste_request(Message::TerminalPasteFromClipboard)
            .on_terminal_input(Message::TerminalInput)
            .with_link_hint(
                (!self.hint_link_click_used)
                    .then(|| crate::i18n::t("terminal_link_hint").to_string()),
            )
            .on_link_opened(Message::TerminalLinkOpened);
        // Wrap the canvas so the focused pane asks the OS to enable its IME.
        // The terminal is a canvas (not a text_input), so without this winit
        // keeps the IME disabled and CJK input can't be switched on.
        let term_canvas = canvas(term_view)
            .width(Length::Fill)
            .height(Length::Fill);
        crate::widgets::ime_host(
            term_canvas,
            is_focused,
            Arc::clone(&pane.terminal),
            self.terminal_font_size,
        )
    }

    pub(crate) fn view_terminal_sidebar<'a>(&'a self, tab: &'a TerminalTab) -> Element<'a, Message> {
        use crate::state::TerminalSidebarTab as STab;
        // Chat is only reachable when AI is enabled; otherwise the active
        // tab effectively falls back to Snippets.
        let active = if self.terminal_sidebar_tab == STab::Chat && !self.ai_enabled {
            STab::Snippets
        } else {
            self.terminal_sidebar_tab
        };

        // ── Tab strip ──
        // Icon tabs on the leading edge; contextual Reset (Chat only) and
        // the Close X on the trailing edge, same affordance as the chrome.
        let mut strip: Vec<Element<'_, Message>> = Vec::new();
        if self.ai_enabled {
            strip.push(sidebar_tab_btn(
                iced_fonts::lucide::sparkles(),
                active == STab::Chat,
                Message::SelectTerminalSidebarTab(STab::Chat),
            ));
        }
        strip.push(sidebar_tab_btn(
            iced_fonts::lucide::code(),
            active == STab::Snippets,
            Message::SelectTerminalSidebarTab(STab::Snippets),
        ));
        strip.push(Space::new().width(Length::Fill).into());
        if active == STab::Chat {
            strip.push(chat_header_btn(iced_fonts::lucide::rotate_ccw(), Message::ChatResetConversation));
            strip.push(Space::new().width(4).into());
        }
        strip.push(chat_header_btn(iced_fonts::lucide::x(), Message::ToggleChatSidebar));

        let header = container(
            dir_row(strip)
                .width(Length::Fill)
                .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
        .width(Length::Fill);

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
                        text(t("ask_ai_session")).size(12).color(OryxisColors::t().text_muted),
                    ]
                    .align_x(iced::Alignment::Center),
                )
                .center_x(Length::Fill)
                .padding(Padding { top: 40.0, right: 0.0, bottom: 0.0, left: 0.0 }),
            );
        } else {
            // Markdown settings are identical for every assistant
            // bubble, so build them once per sidebar render instead of
            // re-deriving the style from the theme per message.
            let md_settings = self.chat_markdown_settings();
            for msg in &tab.chat_history {
                // Skip empty assistant placeholders, they exist as
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
                let bubble = self.view_chat_message(msg, md_settings);
                messages_col = messages_col.push(bubble);
            }
        }

        // Hide the "Thinking..." indicator once the model has started
        // streaming visible text, the streaming bubble itself is the
        // signal of activity, and showing both reads as a stutter.
        let actively_streaming = tab
            .chat_history
            .last()
            .map(|m| m.role == crate::state::ChatRole::Assistant && !m.content.is_empty())
            .unwrap_or(false);
        if self.chat_loading && !actively_streaming {
            messages_col = messages_col.push(
                container(
                    text(t("thinking")).size(12).color(OryxisColors::t().text_muted),
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

        // Multi-line input, grows with content up to ~6 lines (~150 px),
        // then scrolls internally. Enter sends the message; Shift+Enter
        // inserts a newline. No send button, every chat-style UI uses
        // Enter today, so the arrow was just visual noise.
        let chat_editor = iced::widget::text_editor(&self.chat_input)
            .placeholder(t("ask_ai"))
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

        // 4 px draggable handle on the left edge, clicking starts a
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

        // Persistent reminder that the assistant runs commands on the
        // live server (some auto-execute), sitting just above the input.
        let chat_disclaimer = container(
            text(t("ai_chat_disclaimer"))
                .size(10)
                .color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 6.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .align_x(crate::widgets::dir_align_x());

        // ── Assemble sidebar ──
        // Chat body (messages + input) is the content for the Chat tab;
        // the other tabs swap their own content in below the strip.
        let chat_body: Element<'_, Message> =
            column![messages_scroll, input_separator, chat_disclaimer, input_row]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        let content: Element<'_, Message> = match active {
            STab::Chat => chat_body,
            STab::Snippets => self.snippets_tab_content(),
        };
        let panel_column = column![header, header_separator, content]
            .width(Length::Fill)
            .height(Length::Fill);

        // Optional toast, floats above the input area without taking
        // a row in the column layout. Cleared after ~1.8 s by a
        // `ToastClear` round-trip (see dispatch.rs::CopyToClipboard).
        let panel_inner: Element<'_, Message> = if let Some(text_) = self.toast.as_ref() {
            let chip = container(
                text(text_.clone())
                    .size(11)
                    .color(OryxisColors::t().text_primary),
            )
            .padding(Padding {
                top: 5.0,
                right: 12.0,
                bottom: 5.0,
                left: 12.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.95,
                    ..OryxisColors::t().bg_selected
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
            // Anchor the chip horizontally centered, vertically near
            // the bottom of the sidebar (just above the input row).
            // Using a column with fillers so we don't depend on iced
            // alignment quirks.
            let toast_overlay = container(
                column![
                    Space::new().height(Length::Fill),
                    container(chip)
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                    Space::new().height(Length::Fixed(70.0)),
                ]
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill);
            iced::widget::Stack::new()
                .push(panel_column)
                .push(toast_overlay)
                .into()
        } else {
            panel_column.into()
        };

        let panel = container(panel_inner)
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
    fn chat_markdown_settings(&self) -> iced::widget::markdown::Settings {
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

    /// Snippets tab of the terminal sidebar: New + sort header, a search
    /// box, and a compact list. Each row injects via Paste (no newline)
    /// or Run (with newline); the pencil opens the workspace editor.
    fn snippets_tab_content(&self) -> Element<'_, Message> {
        // The editor lives inline in the sidebar (the workspace is never
        // shown while a terminal tab is active, so navigating there is a
        // no-op). `show_snippet_panel` is the shared "editing a snippet"
        // flag, set by New / Edit and cleared on Save / close.
        if self.show_snippet_panel {
            return self.sidebar_snippet_editor();
        }

        let c = OryxisColors::t();

        let new_btn = button(
            container(
                dir_row(vec![
                    iced_fonts::lucide::plus().size(12).color(c.button_text).into(),
                    Space::new().width(6).into(),
                    text(t("snippet_btn"))
                        .size(11)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        })
                        .color(c.button_text)
                        .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .center_y(Length::Fixed(22.0))
            .padding(Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 12.0 }),
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
        });

        // Header: New + sort + search icons; when search is expanded, a
        // focused input (with a close X) takes over the whole row.
        let header_row: iced::widget::Row<'_, Message> = if self.sidebar_search_open {
            dir_row(vec![
                iced::widget::text_input(t("search"), &self.sidebar_snippet_search)
                    .id(iced::widget::Id::new("sidebar-snippet-search"))
                    .on_input(Message::SidebarSnippetSearchChanged)
                    .padding(8)
                    .size(13)
                    .style(crate::widgets::rounded_input_style)
                    .into(),
                Space::new().width(6).into(),
                chat_header_btn(iced_fonts::lucide::x(), Message::ToggleSidebarSearch),
            ])
        } else {
            dir_row(vec![
                new_btn.into(),
                Space::new().width(Length::Fill).into(),
                chat_header_btn(sort_glyph(self.snippets_sort), Message::ToggleSidebarSort),
                Space::new().width(2).into(),
                chat_header_btn(iced_fonts::lucide::search(), Message::ToggleSidebarSearch),
            ])
        };
        let header = container(header_row.width(Length::Fill).align_y(iced::Alignment::Center))
            .padding(Padding { top: 10.0, right: 12.0, bottom: 8.0, left: 12.0 });

        // Sort then filter, carrying original indices so Run/Paste/Edit
        // address the right snippet (the list reorders, `self.snippets`
        // does not).
        let needle = self.sidebar_snippet_search.to_lowercase();
        let mut order: Vec<usize> = (0..self.snippets.len()).collect();
        self.snippets_sort.sort_items(
            &mut order,
            |&i| self.snippets[i].label.clone(),
            |&i| self.snippets[i].created_at,
        );
        let mut list = column![]
            .spacing(6)
            .padding(Padding { top: 0.0, right: 12.0, bottom: 12.0, left: 12.0 });
        let mut any = false;
        for idx in order {
            let snip = &self.snippets[idx];
            if !needle.is_empty()
                && !snip.label.to_lowercase().contains(&needle)
                && !snip.command.to_lowercase().contains(&needle)
            {
                continue;
            }
            any = true;
            list = list.push(snippet_row(
                idx,
                &snip.label,
                &snip.command,
                self.hovered_snippet_card == Some(idx),
            ));
        }
        if !any {
            list = list.push(sidebar_placeholder(t("no_matches")));
        }

        // Built-in "global snippet": type the host's stored password +
        // Enter (e.g. to answer a sudo prompt). Shown only for a live SSH
        // session; the click no-ops with a toast if no password is stored.
        let ssh_active = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.active().ssh_session.is_some())
            .unwrap_or(false);
        let sudo_row: Element<'_, Message> = if ssh_active {
            container(
                button(
                    container(
                        dir_row(vec![
                            iced_fonts::lucide::shield_check().size(13).color(c.accent).into(),
                            Space::new().width(8).into(),
                            text(t("apply_sudo_password")).size(12).color(c.text_primary).into(),
                        ])
                        .align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                    .width(Length::Fill),
                )
                .on_press(Message::ApplySudoPassword)
                .width(Length::Fill)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => OryxisColors::t().bg_surface,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(8.0),
                            color: Color { a: 0.5, ..OryxisColors::t().accent },
                            width: 1.0,
                        },
                        ..Default::default()
                    }
                }),
            )
            .padding(Padding { top: 0.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .into()
        } else {
            Space::new().height(0).into()
        };

        let base = column![header, sudo_row, scrollable(list).height(Length::Fill)]
            .width(Length::Fill)
            .height(Length::Fill);

        if self.sidebar_sort_open {
            use crate::state::{ListSort, SortMenuKind};
            let menu = container(column![
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::LabelAsc,
                    iced_fonts::lucide::arrow_down_a_z::<iced::Theme, iced::Renderer>(),
                    "sort_label_asc",
                    self.snippets_sort == ListSort::LabelAsc,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::LabelDesc,
                    iced_fonts::lucide::arrow_down_z_a::<iced::Theme, iced::Renderer>(),
                    "sort_label_desc",
                    self.snippets_sort == ListSort::LabelDesc,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::NewestFirst,
                    iced_fonts::lucide::calendar_arrow_down::<iced::Theme, iced::Renderer>(),
                    "sort_newest_first",
                    self.snippets_sort == ListSort::NewestFirst,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::OldestFirst,
                    iced_fonts::lucide::calendar_arrow_up::<iced::Theme, iced::Renderer>(),
                    "sort_oldest_first",
                    self.snippets_sort == ListSort::OldestFirst,
                ),
            ])
            .width(Length::Fixed(190.0))
            .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
            // Anchor under the header, hugging the trailing edge.
            let positioned = container(column![
                Space::new().height(Length::Fixed(46.0)),
                container(menu)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 0.0 }),
            ])
            .width(Length::Fill)
            .height(Length::Fill);
            // Transparent backdrop dismisses the popover on any click.
            let backdrop: Element<'_, Message> = MouseArea::new(
                container(Space::new()).width(Length::Fill).height(Length::Fill),
            )
            .on_press(Message::ToggleSidebarSort)
            .into();
            iced::widget::Stack::new()
                .push(base)
                .push(backdrop)
                .push(positioned)
                .into()
        } else {
            base.into()
        }
    }

    /// Compact New / Edit snippet form rendered inline in the Snippets
    /// tab (reuses the same `snippet_*` state + messages as the workspace
    /// editor). A back arrow cancels; Save persists and returns to the
    /// list; Delete shows only when editing an existing snippet.
    fn sidebar_snippet_editor(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let title = if self.snippet_editing_id.is_some() {
            t("edit_snippet")
        } else {
            t("new_snippet")
        };

        let header = dir_row(vec![
            chat_header_btn(iced_fonts::lucide::arrow_left(), Message::HideSnippetPanel),
            Space::new().width(6).into(),
            text(title).size(14).color(c.text_primary).into(),
        ])
        .align_y(iced::Alignment::Center);

        let label_input: Element<'_, Message> =
            iced::widget::text_input("restart-nginx", &self.snippet_label)
                .on_input(Message::SnippetLabelChanged)
                .padding(8)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .into();
        // Multi-line, auto-grows with content; container caps the height
        // (~8 lines) and then it scrolls internally.
        let command_input: Element<'_, Message> = container(
            iced::widget::text_editor(&self.snippet_command)
                .placeholder("sudo systemctl restart nginx")
                .on_action(Message::SnippetCommandAction)
                .padding(8)
                .size(13)
                .height(Length::Shrink)
                .style(crate::widgets::rounded_editor_style),
        )
        .max_height(180.0)
        .into();

        let error: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            text(err.clone()).size(11).color(c.error).into()
        } else {
            Space::new().height(0).into()
        };

        let save = button(
            container(text(t("save")).size(13).color(c.button_text))
                .center_x(Length::Fill)
                .padding(Padding { top: 9.0, right: 0.0, bottom: 9.0, left: 0.0 }),
        )
        .on_press(Message::SaveSnippet)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let mut form = column![
            header,
            Space::new().height(12),
            text(t("name")).size(12).color(c.text_secondary),
            Space::new().height(4),
            label_input,
            Space::new().height(12),
            text(t("command_label")).size(12).color(c.text_secondary),
            Space::new().height(4),
            command_input,
            Space::new().height(10),
            error,
            Space::new().height(12),
            save,
        ]
        .spacing(0)
        .padding(12);

        if let Some(edit_id) = self.snippet_editing_id
            && let Some(idx) = self.snippets.iter().position(|s| s.id == edit_id)
        {
            let delete = button(
                container(text(t("delete")).size(13).color(OryxisColors::t().error))
                    .center_x(Length::Fill)
                    .padding(Padding { top: 9.0, right: 0.0, bottom: 9.0, left: 0.0 }),
            )
            .on_press(Message::DeleteSnippet(idx))
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().error,
                    width: 1.0,
                },
                ..Default::default()
            });
            form = form.push(Space::new().height(8)).push(delete);
        }

        form.width(Length::Fill).height(Length::Fill).into()
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
/// (`Run` / `Always` / `Deny`). Same `MouseArea` treatment as
/// `chat_header_btn` because the iced `button` widget chain inside
/// the chat scrollable was eating clicks on the bubble-level buttons.
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

/// Header glyph button used in the chat sidebar (reset, close). Built with
/// `MouseArea` directly instead of `button` to bypass any iced widget-tree
/// quirk that was eating clicks on the previous `button(...)` version. The
/// click area is the icon + 28×24 padding box.
/// One icon tab in the sidebar's tab strip. Active tab gets an accent
/// glyph on a faint accent wash; inactive is muted and transparent.
fn sidebar_tab_btn<'a>(
    icon: iced::widget::Text<'a>,
    active: bool,
    msg: Message,
) -> Element<'a, Message> {
    use iced::widget::MouseArea;
    let color = if active { OryxisColors::t().accent } else { OryxisColors::t().text_muted };
    MouseArea::new(
        container(icon.size(15).color(color))
            .center_x(Length::Fixed(34.0))
            .center_y(Length::Fixed(28.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(if active {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else {
                    Color::TRANSPARENT
                })),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }),
    )
    .on_press(msg)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// Glyph for the collapsed sort button, reflecting the current sort so
/// the icon doubles as a state indicator (matches the workspace toolbar).
fn sort_glyph<'a>(sort: crate::state::ListSort) -> iced::widget::Text<'a> {
    use crate::state::ListSort;
    match sort {
        ListSort::LabelAsc => iced_fonts::lucide::arrow_down_a_z(),
        ListSort::LabelDesc => iced_fonts::lucide::arrow_down_z_a(),
        ListSort::NewestFirst => iced_fonts::lucide::calendar_arrow_down(),
        ListSort::OldestFirst => iced_fonts::lucide::calendar_arrow_up(),
    }
}

/// Centered muted text for an empty / not-yet-built sidebar tab.
fn sidebar_placeholder<'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(12).color(OryxisColors::t().text_muted))
        .center_x(Length::Fill)
        .padding(Padding { top: 40.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .into()
}

/// An icon action with a tooltip, used for the floating snippet-row
/// actions so Paste (no newline) and Run (+ Enter) are self-explanatory.
fn action_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    tip: &'a str,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        chat_header_btn(icon, msg),
        container(text(tip).size(11).color(OryxisColors::t().text_primary))
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
        iced::widget::tooltip::Position::Top,
    )
    .into()
}

/// One row in the Snippets tab. Label + a single ellipsized line of the
/// command read inline; the Edit / Paste / Run actions float over the
/// trailing edge and only appear on hover (see the card-icon convention
/// in CLAUDE.md). `hovered` is `self.hovered_snippet_card == Some(idx)`.
fn snippet_row<'a>(
    idx: usize,
    label: &'a str,
    command: &'a str,
    hovered: bool,
) -> Element<'a, Message> {
    let c = OryxisColors::t();
    // First line only, ellipsized, so multi-line snippets stay one row.
    let first = command.lines().next().unwrap_or("");
    let multiline = command.lines().nth(1).is_some();
    let preview: String = {
        let head: String = first.chars().take(48).collect();
        if multiline || first.chars().count() > 48 {
            format!("{head}…")
        } else {
            head
        }
    };
    let info = column![
        text(label).size(13).color(c.text_primary),
        text(preview).size(11).color(c.text_muted),
    ]
    .spacing(2)
    .width(Length::Fill);

    let card = container(info)
        .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

    let row_el: Element<'a, Message> = if hovered {
        let actions = container(
            dir_row(vec![
                action_btn(iced_fonts::lucide::pencil(), Message::EditSnippet(idx), t("edit_snippet")),
                action_btn(iced_fonts::lucide::clipboard_copy(), Message::PasteSnippet(idx), t("snippet_paste")),
                action_btn(iced_fonts::lucide::play(), Message::RunSnippet(idx), t("snippet_run")),
            ])
            .spacing(2)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 3.0, right: 5.0, bottom: 3.0, left: 5.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let overlay = container(actions)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center)
            .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 0.0 });
        iced::widget::Stack::new().push(card).push(overlay).into()
    } else {
        card.into()
    };

    MouseArea::new(row_el)
        .on_enter(Message::SnippetCardHovered(idx))
        .on_exit(Message::SnippetCardUnhovered)
        .into()
}

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
