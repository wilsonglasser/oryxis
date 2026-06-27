//! Connection progress screen (shown while connecting to SSH) with host-key
//! verification dialog inline.

use iced::alignment::Horizontal;
use iced::border::Radius;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
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

        // Header: host badge mirrors the configured icon for this host
        // (per-host custom icon/color + shape from the icon_style setting),
        // falling back to the detected OS brand. "Edit Host" lives here on
        // the trailing edge when failed, freeing the bottom action row.
        // Resolve by stored index first (set at connect time), guarded by a
        // label check so a reordered list can't grab the wrong host, then
        // fall back to a label search. The badge stayed teal before because
        // a missing match collapsed every field to None and the brand color
        // never resolved.
        let conn = self
            .connections
            .get(progress.connection_idx)
            .filter(|c| c.label == progress.label)
            .or_else(|| self.connections.iter().find(|c| c.label == progress.label));
        let badge_style = crate::widgets::resolve_host_icon_style(
            conn.and_then(|c| c.icon_style.as_deref()),
            &self.setting_default_host_icon,
        );
        // Two-step color, mirroring the dashboard host card: resolve the
        // brand color from detected OS / custom icon, then let an explicit
        // custom_color / legacy color hex override it.
        let (glyph, icon_color) = crate::os_icon::resolve_for(
            conn.and_then(|c| c.detected_os.as_deref()),
            conn.and_then(|c| c.custom_icon.as_deref()),
            conn.and_then(|c| c.custom_color.as_deref()),
            conn.and_then(|c| c.username.as_deref()),
            OryxisColors::t().accent,
        );
        let badge_color = conn
            .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
            .and_then(crate::widgets::parse_hex_color)
            .unwrap_or(icon_color);
        let glyph_el: Element<'_, Message> = glyph.view(20.0, Color::WHITE);
        let badge = crate::widgets::host_icon(badge_style, badge_color, &progress.label, Some(glyph_el), 40.0);

        let mut header_children: Vec<Element<'_, Message>> = vec![
            badge,
            Space::new().width(14).into(),
            column![
                text(&progress.label).size(16).color(OryxisColors::t().text_primary),
                Space::new().height(2),
                text(&progress.hostname).size(12).color(OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .align_x(crate::widgets::dir_align_x())
            .into(),
        ];
        if failed {
            header_children.push(
                button(
                    container(text(crate::i18n::t("edit_host")).size(13).color(OryxisColors::t().text_primary))
                        .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
                )
                .on_press(Message::SshEditFromProgress)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_surface)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                })
                .into(),
            );
        }

        let header = container(crate::widgets::dir_row(header_children).align_y(iced::Alignment::Center))
            .padding(Padding { top: 24.0, right: 0.0, bottom: 16.0, left: 0.0 });

        // Pulse for the in-flight timeline node while still connecting.
        // Triangular wave 0 -> 1 -> 0 over ~800 ms, driven by the 100 ms
        // connect_anim_tick subscription (only alive while connecting).
        let tick = self.connect_anim_tick;
        let phase = ((tick % 8) as f32) / 8.0;
        let pulse = if phase < 0.5 { phase * 2.0 } else { (1.0 - phase) * 2.0 };

        // Host key verification or normal status/log timeline.
        let (status_widget, body_widget, bottom): (
            Element<'_, Message>,
            Element<'_, Message>,
            Element<'_, Message>,
        ) = if let Some(ref legacy) = self.pending_legacy_algo {
            // Server speaks only legacy algorithms in some category. Offer
            // to enable them (weaker) or cancel.
            let host_label = self
                .connections
                .iter()
                .find(|c| c.id == legacy.conn_id)
                .map(|c| c.label.clone())
                .unwrap_or_default();
            let cat_key = match legacy.category {
                oryxis_ssh::NegCategory::Cipher => "algo_ciphers",
                oryxis_ssh::NegCategory::Kex => "algo_kex",
                oryxis_ssh::NegCategory::Mac => "algo_macs",
                oryxis_ssh::NegCategory::HostKey => "algo_host_keys",
            };
            let status: Element<'_, Message> = text(crate::i18n::t("legacy_algo_title"))
                .size(14)
                .color(OryxisColors::t().warning)
                .into();
            let desc = crate::i18n::t("legacy_algo_desc")
                .replace("{host}", &host_label)
                .replace("{category}", crate::i18n::t(cat_key));
            let mut body_col = column![
                text(desc).size(13).color(OryxisColors::t().text_secondary),
                Space::new().height(10),
                text(crate::i18n::t("legacy_algo_offers"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(4),
            ];
            for off in &legacy.server_offers {
                body_col = body_col.push(
                    text(format!("  {off}")).size(12).color(OryxisColors::t().text_primary),
                );
            }
            let body: Element<'_, Message> = body_col.into();
            let btm: Element<'_, Message> = row![
                crate::widgets::styled_button(
                    crate::i18n::t("cancel"),
                    Message::LegacyAlgoCancel,
                    OryxisColors::t().text_muted,
                ),
                Space::new().width(Length::Fill),
                crate::widgets::styled_button(
                    crate::i18n::t("legacy_algo_connect_once"),
                    Message::LegacyAlgoAccept { remember: false },
                    OryxisColors::t().bg_hover,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    crate::i18n::t("legacy_algo_always"),
                    Message::LegacyAlgoAccept { remember: true },
                    OryxisColors::t().accent,
                ),
            ]
            .align_y(iced::Alignment::Center)
            .into();
            (status, body, btm)
        } else if let Some(ref kbi) = self.pending_kbi_prompt {
            // Keyboard-interactive (2FA / OTP). `name` and the prompt labels
            // are server strings, rendered verbatim, never translated. Only
            // the chrome (title fallback, buttons) goes through i18n.
            let title = if kbi.name.trim().is_empty() {
                crate::i18n::t("kbi_title").to_string()
            } else {
                kbi.name.clone()
            };
            let status: Element<'_, Message> =
                text(title).size(14).color(OryxisColors::t().accent).into();

            let mut body_col = column![].push(Space::new().height(8));

            if !kbi.instructions.trim().is_empty() {
                body_col = body_col
                    .push(text(kbi.instructions.clone()).size(13).color(OryxisColors::t().text_secondary))
                    .push(Space::new().height(12));
            }

            for (i, prompt) in kbi.prompts.iter().enumerate() {
                let value = self.kbi_inputs.get(i).map(|s| s.as_str()).unwrap_or("");
                let mut input = text_input(&prompt.prompt, value)
                    .on_input(move |v| Message::SshKbiInput(i, v))
                    .on_submit(Message::SshKbiSubmit)
                    .padding(10)
                    .size(14);
                // First field gets the shared id so the prompt handler can
                // focus it (type-and-Enter without a click).
                if i == 0 {
                    input = input.id(iced::widget::Id::new(crate::state::KBI_FIRST_INPUT_ID));
                }
                // echo == false means a secret (password / OTP): mask it.
                if !prompt.echo {
                    input = input.secure(true);
                }
                body_col = body_col
                    .push(text(prompt.prompt.clone()).size(12).color(OryxisColors::t().text_muted))
                    .push(Space::new().height(4))
                    .push(input)
                    .push(Space::new().height(12));
            }

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

            let cancel_btn = button(
                container(text(crate::i18n::t("cancel")).size(13).color(OryxisColors::t().text_primary))
                    .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
            )
            .on_press(Message::SshKbiCancel)
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), ..Default::default() },
                ..Default::default()
            });

            let submit_btn = {
                let fg = crate::theme::contrast_text_for(OryxisColors::t().accent);
                button(
                    container(
                        text(crate::i18n::t("kbi_submit"))
                            .size(13)
                            .font(iced::Font {
                                weight: iced::font::Weight::Semibold,
                                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                            })
                            .color(fg),
                    )
                    .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
                )
                .on_press(Message::SshKbiSubmit)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().accent)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                })
            };

            let btm: Element<'_, Message> = row![
                cancel_btn,
                Space::new().width(Length::Fill),
                submit_btn,
            ].align_y(iced::Alignment::Center).into();

            (status, body, btm)
        } else if let Some(ref query) = self.pending_host_key {
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
            // Normal connection progress / failure: a vertical timeline of
            // log lines. This deliberately drops the horizontal "two big
            // bullets joined by a line" bar so the screen reads as our own
            // rather than a Termius clone.
            let status_text = if failed {
                crate::i18n::t("connection_failed_log")
            } else {
                crate::i18n::t("connecting_status")
            };
            let status_color = if failed { OryxisColors::t().error } else { OryxisColors::t().text_secondary };
            let status: Element<'_, Message> = text(status_text).size(14).color(status_color).into();

            (status, self.view_connection_log_timeline(progress, failed, pulse), self.view_connection_log_buttons(progress, failed))
        };

        container(
            column![
                header,
                status_widget,
                Space::new().height(12),
                body_widget,
                Space::new().height(16),
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

    /// The log box rendered as a vertical timeline: one node per log line,
    /// linked by a rail line, message text to the right. `selectable_group`
    /// gives continuous drag-selection across lines (Ctrl+C copies the joined
    /// text); the "Copy logs" button below grabs the whole log regardless.
    fn view_connection_log_timeline(
        &self,
        progress: &crate::state::ConnectionProgress,
        failed: bool,
        pulse: f32,
    ) -> Element<'_, Message> {
        let n = progress.logs.len();
        let mut rows: Vec<Element<'_, Message>> = Vec::with_capacity(n);

        for (i, (step, msg)) in progress.logs.iter().enumerate() {
            let is_error = msg.starts_with("Error");
            let is_last = i + 1 == n;
            // The in-flight node pulses only while we're still connecting.
            let is_active = !failed && is_last;

            // Node color matches the old per-line icon coloring.
            let node_color = if is_error {
                OryxisColors::t().error
            } else {
                match step {
                    ConnectionStep::Connecting => OryxisColors::t().text_muted,
                    ConnectionStep::Handshake => OryxisColors::t().accent,
                    ConnectionStep::Authenticating => OryxisColors::t().warning,
                }
            };

            // Marker: alert glyph for errors, pulsing ring for the active
            // node, solid dot otherwise. All sized to sit centered on the
            // rail so the connector line stays vertically aligned.
            let marker: Element<'_, Message> = if is_error {
                iced_fonts::lucide::circle_alert().size(14).color(node_color).into()
            } else if is_active {
                let bw = 1.5 + pulse * 1.5;
                let fill = Color { a: 0.55 + pulse * 0.25, ..node_color };
                container(Space::new())
                    .width(Length::Fixed(13.0))
                    .height(Length::Fixed(13.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(fill)),
                        border: Border { radius: Radius::from(7.0), color: node_color, width: bw },
                        ..Default::default()
                    })
                    .into()
            } else {
                container(Space::new())
                    .width(Length::Fixed(10.0))
                    .height(Length::Fixed(10.0))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(node_color)),
                        border: Border { radius: Radius::from(5.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into()
            };

            // Connector descends from this node toward the next one. Dimmed so
            // it reads as a guide line, not a solid bar. Omitted on the last
            // node so the line doesn't dangle below the final marker.
            let line_color = Color { a: 0.6, ..node_color };
            let connector: Element<'_, Message> = if is_last {
                Space::new().into()
            } else {
                container(Space::new().width(Length::Fixed(2.0)))
                    .width(Length::Fixed(2.0))
                    .height(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(line_color)),
                        ..Default::default()
                    })
                    .into()
            };

            // Rail: small top nudge to center the marker on the first text
            // line, the marker, then the fill connector. The column is
            // Fill-height so the connector stretches to the row's height
            // (driven by the message cell).
            let rail = column![Space::new().height(Length::Fixed(2.0)), marker, connector]
                .align_x(Horizontal::Center)
                .width(Length::Fixed(22.0))
                .height(Length::Fill);

            // Selectable message. Bottom padding (except last) gives the rows
            // breathing room and lets the connector span cleanly to the next
            // node.
            let span: iced::widget::text::Span<'_, ()> =
                iced::widget::text::Span::new(msg.clone()).color(OryxisColors::t().text_secondary);
            let message = iced::widget::rich_text::<(), Message, _, _>([span])
                .size(13)
                .selectable(true);
            let message_cell = container(message).width(Length::Fill).padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: if is_last { 0.0 } else { 14.0 },
                left: 0.0,
            });

            rows.push(
                row![rail, Space::new().width(8), message_cell]
                    .align_y(iced::Alignment::Start)
                    .into(),
            );
        }

        let timeline = column(rows).padding(Padding { top: 14.0, right: 16.0, bottom: 14.0, left: 12.0 });
        let log_list = scrollable(iced::widget::selectable_group::<(), Message, _, _>(timeline))
            .height(Length::Fill);

        container(log_list)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { radius: Radius::from(10.0), ..Default::default() },
                ..Default::default()
            })
            .into()
    }

    /// Bottom action row for the failed state: "Copy logs" on the leading
    /// edge, then Close / Start over on the trailing edge ("Edit Host" lives
    /// in the header). While connecting (not failed) there are no buttons.
    fn view_connection_log_buttons(
        &self,
        progress: &crate::state::ConnectionProgress,
        failed: bool,
    ) -> Element<'_, Message> {
        if !failed {
            return Space::new().height(0).into();
        }

        // Whole-log payload for the clipboard: host header + every line.
        let mut payload = format!("{}\n{}\n", progress.label, progress.hostname);
        for (_, m) in &progress.logs {
            payload.push_str(m);
            payload.push('\n');
        }

        let copy_btn = button(
            container(
                row![
                    iced_fonts::lucide::copy().size(13).color(OryxisColors::t().text_secondary),
                    Space::new().width(8),
                    text(crate::i18n::t("copy_logs")).size(13).color(OryxisColors::t().text_primary),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 10.0, right: 18.0, bottom: 10.0, left: 18.0 }),
        )
        .on_press(Message::CopyToClipboard(payload))
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let close_btn = button(
            container(text(crate::i18n::t("close")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 24.0, bottom: 10.0, left: 24.0 }),
        )
        .on_press(Message::SshCloseProgress)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let start_over_btn = {
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
        };

        crate::widgets::dir_row(vec![
            copy_btn.into(),
            Space::new().width(Length::Fill).into(),
            close_btn.into(),
            Space::new().width(8).into(),
            start_over_btn.into(),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }
}
