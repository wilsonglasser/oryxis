//! Connection progress screen (shown while connecting to SSH) with host-key
//! verification dialog inline.

use iced::border::Radius;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::state::ConnectionStep;
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_connection_progress(&self) -> Element<'_, Message> {
        let progress = match &self.connecting {
            Some(p) => p,
            None => return Space::new().into(),
        };

        let failed = progress.failed;

        // Build the dot sequence dynamically. Two stages by default
        // (Connect + Authenticate). A third "Verify host" stage is
        // inserted between them whenever a host-key prompt is open.
        #[derive(Clone, Copy)]
        enum DotState { Done, Active, Todo }
        #[derive(Clone, Copy)]
        enum DotKind { Connect, Verify, Auth }
        // Mental model: dots are *milestones* we move toward. The pulsing dot
        // is always the TARGET of the current action (what we're trying to
        // reach), not the origin. So while step=Connecting, dot 2 (Auth) is
        // the target → it pulses; dot 1 (Connect origin) is solid. Dot 3
        // (Verify) is only inserted when the user must actually approve a
        // host key — in that case it's the active target.
        let states: Vec<(DotKind, DotState)> = if failed {
            vec![(DotKind::Connect, DotState::Done), (DotKind::Auth, DotState::Active)]
        } else if self.pending_host_key.is_some() {
            vec![
                (DotKind::Connect, DotState::Done),
                (DotKind::Verify, DotState::Active),
                (DotKind::Auth, DotState::Todo),
            ]
        } else {
            // Always two dots, always pulse the target (Auth). The in-between
            // "Handshake" step is hidden — it's transient and non-actionable.
            vec![(DotKind::Connect, DotState::Done), (DotKind::Auth, DotState::Active)]
        };
        let num_dots = states.len();

        // Header: host info — icon/color matches the detected OS (if the
        // backend probe already finished) so the Connecting screen reflects
        // the brand immediately on subsequent connects.
        let detected_os = self
            .connections
            .iter()
            .find(|c| c.label == progress.label)
            .and_then(|c| c.detected_os.clone());
        let (os_glyph, os_color) =
            crate::os_icon::resolve_icon(detected_os.as_deref(), OryxisColors::t().accent);
        let header = container(
            row![
                container(os_glyph.size(18).color(Color::WHITE))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0))
                    .center_x(Length::Fixed(40.0))
                    .center_y(Length::Fixed(40.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(os_color)),
                        border: Border { radius: Radius::from(10.0), ..Default::default() },
                        ..Default::default()
                    }),
                Space::new().width(14),
                column![
                    text(&progress.label).size(16).color(OryxisColors::t().text_primary),
                    Space::new().height(2),
                    text(&progress.hostname).size(12).color(OryxisColors::t().text_muted),
                ],
            ].align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 24.0, right: 0.0, bottom: 16.0, left: 0.0 });

        // Dot/line rendering. Active dots pulse via connect_anim_tick (100 ms
        // subscription only while connecting).
        let tick = self.connect_anim_tick;
        // Triangular wave 0 → 1 → 0 over ~800 ms.
        let phase = ((tick % 8) as f32) / 8.0;
        let pulse = if phase < 0.5 { phase * 2.0 } else { (1.0 - phase) * 2.0 };

        // Icon per stage — plug for Connect, shield for Verify, terminal for
        // Auth. Termius-style: bigger bullets (28px) with a glyph inside so
        // the sequence reads semantically instead of as abstract dots.
        let stage_icon = |kind: DotKind, color: Color| -> iced::widget::Text<'_> {
            match kind {
                DotKind::Connect => iced_fonts::lucide::plug().size(13).color(color),
                DotKind::Verify => iced_fonts::lucide::shield_check().size(13).color(color),
                DotKind::Auth => iced_fonts::lucide::terminal().size(13).color(color),
            }
        };

        let dot_from_state = |kind: DotKind, state: DotState| -> Element<'_, Message> {
            let (ring_color, is_active) = if failed {
                (OryxisColors::t().error, false)
            } else {
                match state {
                    DotState::Done => (OryxisColors::t().success, false),
                    DotState::Active => (OryxisColors::t().accent, true),
                    DotState::Todo => (OryxisColors::t().text_muted, false),
                }
            };
            // Icon color: white on filled dots (done/active pressed), muted
            // foreground on todo (outline only, glyph sits on bg_primary).
            let icon_color = match state {
                DotState::Done | DotState::Active => Color::WHITE,
                DotState::Todo => OryxisColors::t().text_muted,
            };
            let icon = stage_icon(kind, icon_color);
            container(icon)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0))
                .center_x(Length::Fixed(28.0))
                .center_y(Length::Fixed(28.0))
                .style(move |_| {
                    if is_active {
                        // Pulsing ring: fills a bit on each beat, border width
                        // breathes. Inner alpha stays solid enough to read the
                        // glyph clearly.
                        let border_width = 1.5 + pulse * 1.5;
                        container::Style {
                            background: Some(Background::Color(Color { a: 0.65 + pulse * 0.2, ..ring_color })),
                            border: Border {
                                radius: Radius::from(14.0),
                                color: ring_color,
                                width: border_width,
                            },
                            ..Default::default()
                        }
                    } else if matches!(state, DotState::Todo) {
                        // Todo: hollow outline only, keeps visual weight low.
                        container::Style {
                            background: Some(Background::Color(Color::TRANSPARENT)),
                            border: Border {
                                radius: Radius::from(14.0),
                                color: ring_color,
                                width: 1.2,
                            },
                            ..Default::default()
                        }
                    } else {
                        // Done: solid filled.
                        container::Style {
                            background: Some(Background::Color(ring_color)),
                            border: Border { radius: Radius::from(14.0), ..Default::default() },
                            ..Default::default()
                        }
                    }
                })
                .into()
        };
        let line_from_state = |left: DotState| -> Element<'_, Message> {
            let c = if failed {
                OryxisColors::t().error
            } else {
                match left {
                    DotState::Done => OryxisColors::t().success,
                    DotState::Active => OryxisColors::t().accent,
                    DotState::Todo => OryxisColors::t().text_muted,
                }
            };
            container(Space::new().height(2))
                .width(80)
                .style(move |_| container::Style {
                    background: Some(Background::Color(c)),
                    ..Default::default()
                })
                .into()
        };

        let mut bar_children: Vec<Element<'_, Message>> = Vec::with_capacity(num_dots * 2 - 1);
        for (i, (kind, st)) in states.iter().enumerate() {
            bar_children.push(dot_from_state(*kind, *st));
            if i + 1 < num_dots {
                bar_children.push(line_from_state(*st));
            }
        }
        let progress_bar = container(row(bar_children).align_y(iced::Alignment::Center))
            .padding(Padding { top: 0.0, right: 0.0, bottom: 16.0, left: 0.0 })
            .width(Length::Fill)
            .center_x(Length::Fill);

        // Host key verification or normal status
        let (status_widget, body_widget, bottom): (
            Element<'_, Message>,
            Element<'_, Message>,
            Element<'_, Message>,
        ) = if let Some(ref query) = self.pending_host_key {
            let is_changed = matches!(query.status, oryxis_ssh::HostKeyStatus::Changed { .. });

            let question_text = if is_changed {
                crate::i18n::t("hk_warning_title")
            } else {
                crate::i18n::t("hk_unknown_title")
            };
            let question_color = if is_changed { OryxisColors::t().error } else { OryxisColors::t().warning };

            let status: Element<'_, Message> = text(question_text).size(14).color(question_color).into();

            let mut body_col = column![];

            if is_changed {
                body_col = body_col
                    .push(Space::new().height(8))
                    .push(text(crate::i18n::t("hk_warning_desc")).size(13).color(OryxisColors::t().error))
                    .push(Space::new().height(12));
                if let oryxis_ssh::HostKeyStatus::Changed { ref old_fingerprint } = query.status {
                    body_col = body_col
                        .push(text(format!("{} {}", crate::i18n::t("hk_old_fingerprint"), old_fingerprint)).size(12).color(OryxisColors::t().text_muted))
                        .push(Space::new().height(8));
                }
            } else {
                body_col = body_col
                    .push(Space::new().height(8))
                    .push(text(format!(
                        "The authenticity of {} can not be established.",
                        query.hostname,
                    )).size(13).color(OryxisColors::t().text_secondary))
                    .push(Space::new().height(12));
            }

            body_col = body_col
                .push(text(format!("{} fingerprint is SHA256:", query.key_type)).size(13).color(OryxisColors::t().text_secondary))
                .push(Space::new().height(8))
                .push(text(&query.fingerprint).size(14).color(OryxisColors::t().text_primary))
                .push(Space::new().height(16))
                .push(text(crate::i18n::t("hk_add_question")).size(13).color(OryxisColors::t().text_secondary));

            let body: Element<'_, Message> = container(body_col)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 })
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                    border: Border { radius: Radius::from(10.0), ..Default::default() },
                    ..Default::default()
                })
                .into();

            let close_btn = button(
                container(text(crate::i18n::t("close")).size(13).color(OryxisColors::t().text_primary))
                    .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
            )
            .on_press(Message::SshHostKeyReject)
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

            let continue_btn = button(
                container(text(crate::i18n::t("hk_continue")).size(13).color(OryxisColors::t().text_primary))
                    .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
            )
            .on_press(Message::SshHostKeyContinue)
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            let accept_btn = {
                let fg = crate::theme::contrast_text_for(OryxisColors::t().success);
                button(
                    container(
                        text(crate::i18n::t("hk_add_and_continue"))
                            .size(13)
                            .font(iced::Font {
                                weight: iced::font::Weight::Semibold,
                                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                            })
                            .color(fg),
                    )
                    .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                )
                .on_press(Message::SshHostKeyAcceptAndSave)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().success)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                })
            };

            let btm: Element<'_, Message> = row![
                close_btn,
                Space::new().width(12),
                continue_btn,
                Space::new().width(Length::Fill),
                accept_btn,
            ].align_y(iced::Alignment::Center).into();

            (status, body, btm)
        } else {
            // Normal connection progress
            let status_text = if failed {
                "Connection failed with connection log:"
            } else {
                "Connecting..."
            };
            let status_color = if failed { OryxisColors::t().error } else { OryxisColors::t().text_secondary };
            let status: Element<'_, Message> = text(status_text).size(14).color(status_color).into();

            // Log entries
            let mut log_items: Vec<Element<'_, Message>> = Vec::new();
            for (step, msg) in &progress.logs {
                let icon_color = if msg.starts_with("Error") {
                    OryxisColors::t().error
                } else {
                    match step {
                        ConnectionStep::Connecting => OryxisColors::t().text_muted,
                        ConnectionStep::Handshake => OryxisColors::t().accent,
                        ConnectionStep::Authenticating => OryxisColors::t().warning,
                    }
                };

                let icon = if msg.starts_with("Error") {
                    iced_fonts::lucide::circle_alert()
                } else {
                    iced_fonts::lucide::settings()
                };

                log_items.push(
                    row![
                        icon.size(12).color(icon_color),
                        Space::new().width(10),
                        text(msg).size(13).color(OryxisColors::t().text_secondary),
                    ]
                    .align_y(iced::Alignment::Start)
                    .into(),
                );
                log_items.push(Space::new().height(6).into());
            }

            let log_list = scrollable(
                column(log_items).padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
            )
            .height(Length::Fill);

            let body: Element<'_, Message> = container(log_list)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                    border: Border { radius: Radius::from(10.0), ..Default::default() },
                    ..Default::default()
                })
                .into();

            // Bottom buttons
            let btm: Element<'_, Message> = if failed {
                row![
                    button(
                        container(text(crate::i18n::t("close")).size(13).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                    )
                    .on_press(Message::SshCloseProgress)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().width(8),
                    button(
                        container(text(crate::i18n::t("edit_host")).size(13).color(OryxisColors::t().text_primary))
                            .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                    )
                    .on_press(Message::SshEditFromProgress)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().width(Length::Fill),
                    {
                        let fg = crate::theme::contrast_text_for(OryxisColors::t().success);
                        button(
                            container(
                                text(crate::i18n::t("start_over"))
                                    .size(13)
                                    .font(iced::Font {
                                        weight: iced::font::Weight::Semibold,
                                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                    })
                                    .color(fg),
                            )
                            .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                        )
                        .on_press(Message::SshRetry)
                        .style(|_, _| button::Style {
                            background: Some(Background::Color(OryxisColors::t().success)),
                            border: Border { radius: Radius::from(8.0), ..Default::default() },
                            ..Default::default()
                        })
                    },
                ]
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                Space::new().height(0).into()
            };

            (status, body, btm)
        };

        container(
            column![
                header,
                progress_bar,
                status_widget,
                Space::new().height(12),
                body_widget,
                Space::new().height(12),
                bottom,
            ]
            .padding(32)
            .width(500)
            .height(Length::Fill),
        )
        .center_x(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            ..Default::default()
        })
        .into()
    }
}
