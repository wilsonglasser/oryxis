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
use crate::state::TerminalTab;
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

        let base = container(terminal_area)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().terminal_bg)),
                ..Default::default()
            });
        // Toast (copy feedback, OSC 9 notifications, …) floats over the whole
        // terminal area so it shows whether or not the chat sidebar is open.
        match self.toast_overlay() {
            Some(overlay) => iced::widget::Stack::new().push(base).push(overlay).into(),
            None => base.into(),
        }
    }

    /// Bottom-center toast chip over the terminal, or `None` when no toast is
    /// pending. Shared by the main view; the chat sidebar no longer renders its
    /// own copy (that only showed while the sidebar was open).
    fn toast_overlay(&self) -> Option<Element<'_, Message>> {
        let text_ = self.toast.as_ref()?;
        let chip = container(
            text(text_.clone()).size(11).color(OryxisColors::t().text_primary),
        )
        .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
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
        // Clicking the chip dismisses it immediately. Only the chip is
        // interactive; the surrounding Fill stays transparent to clicks so it
        // never steals input from the terminal underneath.
        let chip = MouseArea::new(chip)
            .on_press(Message::ToastClear)
            .interaction(iced::mouse::Interaction::Pointer);
        Some(
            container(
                column![
                    Space::new().height(Length::Fill),
                    container(chip)
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                    Space::new().height(Length::Fixed(48.0)),
                ]
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
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
            .with_bell_flash(pane.bell_flash)
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
            self.terminal_font_name.clone(),
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
                t("tab_tip_chat"),
            ));
        }
        strip.push(sidebar_tab_btn(
            iced_fonts::lucide::code(),
            active == STab::Snippets,
            Message::SelectTerminalSidebarTab(STab::Snippets),
            t("snippets"),
        ));
        strip.push(sidebar_tab_btn(
            iced_fonts::lucide::cog(),
            active == STab::HostConfig,
            Message::SelectTerminalSidebarTab(STab::HostConfig),
            t("tab_tip_host_config"),
        ));
        strip.push(Space::new().width(Length::Fill).into());
        if active == STab::Chat {
            strip.push(icon_tooltip(
                chat_header_btn(iced_fonts::lucide::rotate_ccw(), Message::ChatResetConversation),
                t("chat_reset_tip"),
            ));
            strip.push(Space::new().width(4).into());
        }
        strip.push(icon_tooltip(
            chat_header_btn(iced_fonts::lucide::x(), Message::ToggleChatSidebar),
            t("close"),
        ));

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

        // While a chat task is in flight (streaming a reply or auto-running
        // a tool chain) offer an explicit Stop. It aborts the live task so a
        // runaway tool loop can be halted by hand, without closing the panel.
        let stop_control: Element<'_, Message> = if self.chat_task.is_some() {
            container(
                button(
                    dir_row(vec![
                        iced_fonts::lucide::circle_stop()
                            .size(12)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        text(t("chat_stop"))
                            .size(11)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                    ])
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
                .on_press(Message::ChatStop)
                .style(|_, status| {
                    let c = OryxisColors::t();
                    let bg = match status {
                        BtnStatus::Hovered => c.button_bg_hover,
                        _ => c.button_bg,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: c.text_primary,
                        border: Border {
                            radius: Radius::from(8.0),
                            width: 1.0,
                            color: c.border,
                        },
                        ..Default::default()
                    }
                }),
            )
            .center_x(Length::Fill)
            .padding(Padding { top: 6.0, right: 12.0, bottom: 0.0, left: 12.0 })
            .into()
        } else {
            Space::new().into()
        };

        // ── Assemble sidebar ──
        // Chat body (messages + input) is the content for the Chat tab;
        // the other tabs swap their own content in below the strip.
        let chat_body: Element<'_, Message> =
            column![messages_scroll, input_separator, stop_control, chat_disclaimer, input_row]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        let content: Element<'_, Message> = match active {
            STab::Chat => chat_body,
            STab::Snippets => self.snippets_tab_content(),
            STab::HostConfig => self.host_config_tab_content(tab),
        };
        let panel_column = column![header, header_separator, content]
            .width(Length::Fill)
            .height(Length::Fill);

        // The toast now floats over the whole terminal view (see
        // `view_terminal` / `toast_overlay`), not just this sidebar, so it
        // shows even when the chat panel is closed.
        let panel = container(panel_column)
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
}

/// One icon tab in the sidebar's tab strip. Active tab gets an accent
/// glyph on a faint accent wash; inactive is muted and transparent.
fn sidebar_tab_btn<'a>(
    icon: iced::widget::Text<'a>,
    active: bool,
    msg: Message,
    tip: &'a str,
) -> Element<'a, Message> {
    let color = if active { OryxisColors::t().accent } else { OryxisColors::t().text_muted };
    let btn = button(
        container(icon.size(15).color(color))
            .center_x(Length::Fixed(34.0))
            .center_y(Length::Fixed(28.0)),
    )
    .padding(0)
    .on_press(msg)
    .style(move |_, status| {
        // Selected tab keeps its accent tint; an unselected tab fills with
        // bg_hover on hover/press for clear pointer feedback.
        let bg = if active {
            Color { a: 0.15, ..OryxisColors::t().accent }
        } else {
            match status {
                BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    icon_tooltip(btn.into(), tip)
}

/// Wrap an icon control in a small bottom-anchored tooltip, the shared
/// affordance for the sidebar tab strip and close affordances.
fn icon_tooltip<'a>(inner: Element<'a, Message>, tip: &'a str) -> Element<'a, Message> {
    iced::widget::tooltip(
        inner,
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
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}


pub(crate) fn chat_header_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(icon.size(13).color(OryxisColors::t().text_muted))
            .center_x(Length::Fixed(28.0))
            .center_y(Length::Fixed(24.0)),
    )
    .padding(0)
    .on_press(msg)
    .style(|_, status| {
        // Fill with bg_hover on hover/press so close/reset/action icons
        // give the same pointer feedback as the rest of the chrome.
        let bg = match status {
            BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}
