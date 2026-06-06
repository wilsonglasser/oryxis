//! Port forwards (standalone tunnels) list and editor panel. Each row
//! carries an on/off toggle that opens / tears down a dedicated PTY-less
//! SSH session; the runtime state lives in `Oryxis::active_forwards`.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, checkbox, column, container, pick_list, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::port_forward_rule::{ForwardKind, PortForwardRule};

use crate::app::{Message, Oryxis, CARD_WIDTH, PANEL_WIDTH};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::{card_grid_columns, dir_align_x, dir_row, distribute_card_grid};

/// Human-readable one-line summary of a rule, kind-aware.
fn forward_summary(rule: &PortForwardRule) -> String {
    match rule.kind {
        ForwardKind::Local => format!(
            "{}:{} \u{2192} {}:{}",
            rule.listen_host, rule.listen_port, rule.target_host, rule.target_port
        ),
        ForwardKind::Remote => format!(
            "{}:{} \u{2190} {}:{}",
            rule.listen_host, rule.listen_port, rule.target_host, rule.target_port
        ),
        ForwardKind::Dynamic => {
            format!("SOCKS5 {}:{}", rule.listen_host, rule.listen_port)
        }
    }
}

impl Oryxis {
    pub(crate) fn view_port_forwards(&self) -> Element<'_, Message> {
        let toolbar = container(
            dir_row(vec![
                text(t("port_forwards")).size(20).color(OryxisColors::t().text_primary).into(),
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
                                text(t("port_forward_btn")).size(11).font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                                }).color(fg).into(),
                            ]).align_y(iced::Alignment::Center),
                        )
                        .center_y(Length::Fixed(24.0))
                        .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
                    )
                    .on_press(Message::ShowPortForwardPanel)
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

        let status: Element<'_, Message> = if let Some(err) = &self.pf_error {
            container(Element::from(text(err.clone()).size(12).color(OryxisColors::t().error)))
                .padding(Padding { top: 0.0, right: 24.0, bottom: 8.0, left: 24.0 }).into()
        } else {
            Space::new().height(0).into()
        };

        if self.port_forward_rules.is_empty() {
            let empty_state = container(
                column![
                    container(
                        iced_fonts::lucide::route().size(32).color(OryxisColors::t().text_muted),
                    )
                    .padding(16)
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(12.0), ..Default::default() },
                        ..Default::default()
                    }),
                    Space::new().height(20),
                    text(t("create_port_forward_title")).size(20).color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    text(t("create_port_forward_desc")).size(13).color(OryxisColors::t().text_muted),
                    Space::new().height(24),
                    crate::widgets::cta_button(
                        t("new_port_forward").to_string(),
                        Message::ShowPortForwardPanel,
                    ),
                ]
                .align_x(iced::Alignment::Center),
            )
            .center(Length::Fill);

            let main_content = column![toolbar, status, empty_state]
                .width(Length::Fill)
                .height(Length::Fill);
            if self.show_port_forward_panel {
                let panel = self.view_port_forward_panel();
                return dir_row(vec![main_content.into(), panel])
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            }
            return main_content.into();
        }

        let needle = self.port_forward_search.to_lowercase();
        let mut cards: Vec<Element<'_, Message>> = Vec::new();
        for (idx, rule) in self.port_forward_rules.iter().enumerate() {
            let host_label = self
                .connections
                .iter()
                .find(|c| c.id == rule.host_id)
                .map(|c| c.label.clone())
                .unwrap_or_else(|| t("pf_unknown_host").to_string());
            if !needle.is_empty()
                && !rule.label.to_lowercase().contains(&needle)
                && !forward_summary(rule).to_lowercase().contains(&needle)
                && !host_label.to_lowercase().contains(&needle)
            {
                continue;
            }

            let active = self.active_forwards.contains_key(&rule.id);
            let starting = self.port_forward_starting.contains(&rule.id);

            // Status dot: accent-green while up, muted while down.
            let dot_color = if active {
                OryxisColors::t().success
            } else {
                OryxisColors::t().text_muted
            };
            let glyph_el: Element<'_, Message> = iced_fonts::lucide::route()
                .size(14)
                .line_height(1.0)
                .color(Color::WHITE)
                .into();
            let icon_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.setting_default_host_icon,
            );
            let icon_box = crate::widgets::host_icon(
                icon_style,
                dot_color,
                &rule.label,
                Some(glyph_el),
                32.0,
            );

            // Trailing on/off toggle. Nested inside the card button (the
            // fork lets the inner press win), so clicking the toggle does
            // not also open the editor.
            let (toggle_label, toggle_msg, toggle_bg, toggle_fg) = if starting {
                (t("pf_starting"), None, OryxisColors::t().bg_surface, OryxisColors::t().text_muted)
            } else if active {
                (
                    t("pf_on"),
                    Some(Message::StopPortForward(rule.id)),
                    OryxisColors::t().success,
                    Color::WHITE,
                )
            } else {
                (
                    t("pf_off"),
                    Some(Message::StartPortForward(rule.id)),
                    OryxisColors::t().bg_surface,
                    OryxisColors::t().text_secondary,
                )
            };
            let mut toggle = button(
                container(text(toggle_label).size(11).color(toggle_fg))
                    .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
            )
            .style(move |_, st| {
                let bg = match st {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => toggle_bg,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            if let Some(msg) = toggle_msg {
                toggle = toggle.on_press(msg);
            }

            // Trash kebab, hover-revealed (floating-action convention).
            const TRASH_SLOT_W: f32 = 28.0;
            let show_trash = self.hovered_port_forward_card == Some(idx);
            let trash: Element<'_, Message> = if show_trash {
                button(text("\u{1F5D1}").size(13).color(OryxisColors::t().text_muted))
                    .on_press(Message::DeletePortForwardRule(idx))
                    .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
                    .style(|_, st| {
                        let bg = match st {
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
                Space::new().width(Length::Fixed(TRASH_SLOT_W)).height(Length::Fixed(20.0)).into()
            };

            let kind_badge = format!("{}  \u{00B7}  {}", rule.kind, host_label);

            let card_btn = button(
                container(
                    dir_row(vec![
                        icon_box,
                        Space::new().width(8).into(),
                        column![
                            text(&rule.label)
                                .size(13)
                                .color(OryxisColors::t().text_primary)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(forward_summary(rule))
                                .size(10)
                                .color(OryxisColors::t().text_muted)
                                .font(iced::Font::MONOSPACE)
                                .wrapping(iced::widget::text::Wrapping::None),
                            Space::new().height(2),
                            text(kind_badge)
                                .size(9)
                                .color(OryxisColors::t().text_secondary)
                                .wrapping(iced::widget::text::Wrapping::None),
                        ].width(Length::Fill).into(),
                        toggle.into(),
                        Space::new().width(4).into(),
                        trash,
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 8.0, right: 2.0, bottom: 8.0, left: 2.0 }),
            )
            .on_press(Message::EditPortForwardRule(idx))
            .width(Length::Fill)
            .style(move |_, st| {
                let (bg, bc, bw) = match st {
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

            let wrapped: Element<'_, Message> = MouseArea::new(card_btn)
                .on_enter(Message::PortForwardCardHovered(idx))
                .on_exit(Message::PortForwardCardUnhovered)
                .into();
            cards.push(container(wrapped).width(Length::Fill).clip(true).into());
        }

        let nav_width = if self.sidebar_collapsed {
            crate::app::SIDEBAR_WIDTH_COLLAPSED
        } else {
            crate::app::SIDEBAR_WIDTH
        };
        let panel_width = if self.show_port_forward_panel { PANEL_WIDTH } else { 0.0 };
        let available = (self.window_size.width - nav_width - panel_width - 48.0).max(0.0);
        let cols = card_grid_columns(available, CARD_WIDTH, 12.0);
        let grid_widget = distribute_card_grid(cards, cols, 12.0, 12.0);
        let grid = scrollable(
            column![grid_widget].padding(Padding { top: 0.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        ).height(Length::Fill);

        // Inline search in Classic mode (Workspace puts it on the sub-nav).
        let workspace_mode = self.setting_layout_mode == "workspace";
        let search_bar: Element<'_, Message> = if workspace_mode {
            Space::new().height(0).into()
        } else {
            container(
                text_input(t("search_port_forwards"), &self.port_forward_search)
                    .id(iced::widget::Id::new("search-port-forwards"))
                    .on_input(Message::PortForwardSearchChanged)
                    .padding(10)
                    .size(13)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
            )
            .padding(Padding { top: 0.0, right: 24.0, bottom: 12.0, left: 24.0 })
            .width(Length::Fill)
            .into()
        };

        let main_content = column![toolbar, search_bar, status, grid]
            .width(Length::Fill)
            .height(Length::Fill);
        if self.show_port_forward_panel {
            let panel = self.view_port_forward_panel();
            dir_row(vec![main_content.into(), panel]).width(Length::Fill).height(Length::Fill).into()
        } else {
            main_content.into()
        }
    }

    pub(crate) fn view_port_forward_panel(&self) -> Element<'_, Message> {
        let is_editing = self.pf_editing_id.is_some();
        let title = if is_editing { t("edit_port_forward") } else { t("new_port_forward") };

        let panel_header = container(
            dir_row(vec![
                text(title).size(18).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HidePortForwardPanel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_surface)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }).into(),
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 20.0, bottom: 16.0, left: 20.0 });

        // Kind picker. All three directions are implemented.
        let kind_options = ForwardKind::ALL.to_vec();
        let kind_picker = pick_list(Some(self.pf_kind), kind_options, |k: &ForwardKind| k.to_string())
            .on_select(Message::PfKindChanged)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);

        // Host picker. Options are connection labels; the on_select closure
        // resolves the label back to the connection id.
        let host_options: Vec<String> = self.connections.iter().map(|c| c.label.clone()).collect();
        let selected_host_label = self
            .pf_host_id
            .and_then(|id| self.connections.iter().find(|c| c.id == id))
            .map(|c| c.label.clone());
        let host_lookup: std::collections::HashMap<String, uuid::Uuid> = self
            .connections
            .iter()
            .map(|c| (c.label.clone(), c.id))
            .collect();
        let host_picker = pick_list(selected_host_label, host_options, |s: &String| s.clone())
            .on_select(move |label: String| {
                Message::PfHostChanged(host_lookup.get(&label).copied().unwrap_or_default())
            })
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);

        let label_field = |label: &str, value: &str, placeholder: &str, on_input: fn(String) -> Message| {
            column![
                text(label.to_string()).size(12).color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text_input(placeholder, value)
                    .on_input(on_input)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x()),
            ]
        };

        let mut form = column![
            label_field(t("name"), &self.pf_label, "my-db-tunnel", Message::PfLabelChanged),
            Space::new().height(14),
            text(t("pf_kind")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            kind_picker,
            Space::new().height(14),
            text(t("pf_host")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            host_picker,
            Space::new().height(14),
            label_field(t("pf_listen_host"), &self.pf_listen_host, "127.0.0.1", Message::PfListenHostChanged),
            Space::new().height(14),
            label_field(t("pf_listen_port"), &self.pf_listen_port, "8080", Message::PfListenPortChanged),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Target fields hidden for Dynamic (the SOCKS client picks the dest).
        if self.pf_kind.has_target() {
            form = form
                .push(Space::new().height(14))
                .push(label_field(t("pf_target_host"), &self.pf_target_host, "10.0.0.5", Message::PfTargetHostChanged))
                .push(Space::new().height(14))
                .push(label_field(t("pf_target_port"), &self.pf_target_port, "5432", Message::PfTargetPortChanged));
        }

        // Remote bind on 0.0.0.0 needs `GatewayPorts yes` on the server.
        if self.pf_kind == ForwardKind::Remote && self.pf_listen_host.trim() == "0.0.0.0" {
            form = form
                .push(Space::new().height(10))
                .push(text(t("gateway_ports_hint")).size(11).color(OryxisColors::t().warning));
        }

        form = form
            .push(Space::new().height(14))
            .push(
                checkbox(self.pf_auto_start)
                    .label(t("pf_auto_start"))
                    .on_toggle(Message::PfAutoStartToggled)
                    .size(16)
                    .text_size(12),
            );

        let panel_error: Element<'_, Message> = if let Some(err) = &self.pf_error {
            Element::from(text(err.clone()).size(11).color(OryxisColors::t().error))
        } else {
            Space::new().height(0).into()
        };

        let save_btn = button(
            container(text(t("save")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                .width(Length::Fill).center_x(Length::Fill),
        )
        .on_press(Message::SavePortForwardRule)
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let mut bottom = column![save_btn];
        if let Some(edit_id) = self.pf_editing_id
            && let Some(idx) = self.port_forward_rules.iter().position(|r| r.id == edit_id)
        {
            let del_btn = button(
                container(text(t("delete")).size(13).color(OryxisColors::t().error))
                    .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
                    .width(Length::Fill).center_x(Length::Fill),
            )
            .on_press(Message::DeletePortForwardRule(idx))
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
            scrollable(
                container(
                    column![
                        form,
                        Space::new().height(12),
                        panel_error,
                        Space::new().height(20),
                        bottom,
                    ].width(Length::Fill).align_x(dir_align_x()),
                )
                .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 20.0 }),
            ).height(Length::Fill),
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

    /// Standalone host-key verification modal, used when a backgrounded
    /// action (a manually toggled port forward) hits an unknown / changed
    /// key and there is no connect-progress screen to host the prompt
    /// inline. Reuses the same `SshHostKey*` messages as the terminal flow.
    pub(crate) fn view_host_key_modal(&self) -> Element<'_, Message> {
        let Some(query) = self.pending_host_key.as_ref() else {
            return Space::new().into();
        };
        let is_changed = matches!(query.status, oryxis_ssh::HostKeyStatus::Changed { .. });
        let title = if is_changed { t("hk_warning_title") } else { t("hk_unknown_title") };
        let title_color = if is_changed { OryxisColors::t().error } else { OryxisColors::t().warning };

        let mut body = column![
            text(title).size(16).color(title_color),
            Space::new().height(10),
        ];
        if is_changed {
            body = body
                .push(text(t("hk_warning_desc")).size(13).color(OryxisColors::t().error))
                .push(Space::new().height(8));
            if let oryxis_ssh::HostKeyStatus::Changed { old_fingerprint } = &query.status {
                body = body
                    .push(
                        text(format!("{} {}", t("hk_old_fingerprint"), old_fingerprint))
                            .size(12)
                            .color(OryxisColors::t().text_muted),
                    )
                    .push(Space::new().height(8));
            }
        }
        body = body
            .push(text(format!("{}:{}", query.hostname, query.port)).size(13).color(OryxisColors::t().text_secondary))
            .push(Space::new().height(8))
            .push(text(format!("{} SHA256:", query.key_type)).size(12).color(OryxisColors::t().text_secondary))
            .push(Space::new().height(4))
            .push(text(&query.fingerprint).size(13).color(OryxisColors::t().text_primary).font(iced::Font::MONOSPACE))
            .push(Space::new().height(14))
            .push(text(t("hk_add_question")).size(13).color(OryxisColors::t().text_secondary))
            .push(Space::new().height(18));

        let close_btn = button(
            container(text(t("close")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 }),
        )
        .on_press(Message::SshHostKeyReject)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let continue_btn = button(
            container(text(t("hk_continue")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 }),
        )
        .on_press(Message::SshHostKeyContinue)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        let accept_fg = crate::theme::contrast_text_for(OryxisColors::t().success);
        let accept_btn = button(
            container(text(t("hk_add_and_continue")).size(13).color(accept_fg))
                .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 }),
        )
        .on_press(Message::SshHostKeyAcceptAndSave)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().success)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let buttons = dir_row(vec![
            close_btn.into(),
            Space::new().width(8).into(),
            continue_btn.into(),
            Space::new().width(Length::Fill).into(),
            accept_btn.into(),
        ])
        .align_y(iced::Alignment::Center);

        let card = container(column![body, buttons].width(Length::Fill))
            .width(Length::Fixed(480.0))
            .padding(24)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(12.0) },
                ..Default::default()
            });

        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into()
    }

    /// Standalone keyboard-interactive (2FA / OTP) modal, used when a
    /// split-pane connect (which has no connect-progress screen) hits an
    /// Interactive auth challenge. Reuses the same `SshKbi*` messages as
    /// the inline connect-progress prompt. `name` and the prompt labels
    /// are server strings, rendered verbatim, never translated.
    pub(crate) fn view_kbi_modal(&self) -> Element<'_, Message> {
        let Some(kbi) = self.pending_kbi_prompt.as_ref() else {
            return Space::new().into();
        };
        let title = if kbi.name.trim().is_empty() {
            t("kbi_title").to_string()
        } else {
            kbi.name.clone()
        };

        let mut body = column![
            text(title).size(16).color(OryxisColors::t().accent),
            Space::new().height(10),
        ];
        if !kbi.instructions.trim().is_empty() {
            body = body
                .push(text(kbi.instructions.clone()).size(13).color(OryxisColors::t().text_secondary))
                .push(Space::new().height(10));
        }
        for (i, prompt) in kbi.prompts.iter().enumerate() {
            let value = self.kbi_inputs.get(i).map(|s| s.as_str()).unwrap_or("");
            let mut input = text_input(&prompt.prompt, value)
                .on_input(move |v| Message::SshKbiInput(i, v))
                .on_submit(Message::SshKbiSubmit)
                .padding(10)
                .size(14);
            if i == 0 {
                input = input.id(iced::widget::Id::new(crate::state::KBI_FIRST_INPUT_ID));
            }
            if !prompt.echo {
                input = input.secure(true);
            }
            body = body
                .push(text(prompt.prompt.clone()).size(12).color(OryxisColors::t().text_muted))
                .push(Space::new().height(4))
                .push(input)
                .push(Space::new().height(12));
        }
        body = body.push(Space::new().height(6));

        let cancel_btn = button(
            container(text(t("cancel")).size(13).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 }),
        )
        .on_press(Message::SshKbiCancel)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let submit_fg = crate::theme::contrast_text_for(OryxisColors::t().accent);
        let submit_btn = button(
            container(text(t("kbi_submit")).size(13).color(submit_fg))
                .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 }),
        )
        .on_press(Message::SshKbiSubmit)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let buttons = dir_row(vec![
            cancel_btn.into(),
            Space::new().width(Length::Fill).into(),
            submit_btn.into(),
        ])
        .align_y(iced::Alignment::Center);

        let card = container(column![body, buttons].width(Length::Fill))
            .width(Length::Fixed(480.0))
            .padding(24)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(12.0) },
                ..Default::default()
            });

        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into()
    }
}
