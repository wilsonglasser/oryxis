//! Host editor / connection editor side panel.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, scrollable, text, text_input, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use oryxis_core::models::connection::AuthMethod;
use oryxis_core::models::identity::Identity;

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::state::ProxyKind;
use crate::theme::OryxisColors;
use crate::app::PANEL_WIDTH;
use crate::widgets::{
    dir_row, panel_divider, panel_field, panel_option_pick, panel_option_pick_jump,
    panel_option_row, panel_section,
};

impl Oryxis {
    pub(crate) fn view_host_panel(&self) -> Element<'_, Message> {
        let is_editing = self.editor_form.editing_id.is_some();
        let title = if is_editing { crate::i18n::t("edit_host") } else { crate::i18n::t("new_host") };
        let has_address = !self.editor_form.hostname.is_empty();

        // ── Header ──
        let panel_header = container(
            dir_row(vec![
                text(title).size(16).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                button(iced_fonts::lucide::chevron_right().size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::EditorCancel)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        border: Border::default(),
                        ..Default::default()
                    }).into(),
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 16.0, bottom: 12.0, left: 16.0 });

        // ── Section: Address ──
        // Icon + color reflect the detected OS (once the silent probe has
        // run) or a user-picked override.
        let editing_conn = self.editor_form.editing_id.and_then(|id| {
            self.connections.iter().find(|c| c.id == id)
        });
        let (addr_glyph, addr_color) = crate::os_icon::resolve_for(
            editing_conn.and_then(|c| c.detected_os.as_deref()),
            editing_conn.and_then(|c| c.custom_icon.as_deref()),
            editing_conn.and_then(|c| c.custom_color.as_deref()),
            OryxisColors::t().accent,
        );
        // Icon is a button when we're editing an existing host — clicking it
        // opens the icon/color picker so the user can override the OS mark.
        // For new (unsaved) hosts the id doesn't exist yet, so it's just a
        // static badge until the first save.
        let icon_element: Element<'_, Message> = if let Some(id) = self.editor_form.editing_id {
            button(
                container(addr_glyph.size(14).color(Color::WHITE))
                    .width(Length::Fixed(32.0))
                    .height(Length::Fixed(32.0))
                    .center_x(Length::Fixed(32.0))
                    .center_y(Length::Fixed(32.0)),
            )
            .on_press(Message::ShowIconPicker(id))
            .padding(0)
            .style(move |_, status| {
                let ring = match status {
                    BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.25),
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(addr_color)),
                    border: Border { radius: Radius::from(8.0), color: ring, width: 1.5 },
                    ..Default::default()
                }
            })
            .into()
        } else {
            container(addr_glyph.size(14).color(Color::WHITE))
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(32.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(addr_color)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                })
                .into()
        };

        let address_section = panel_section(column![
            dir_row(vec![
                icon_element,
                Space::new().width(10).into(),
                text_input(t("ip_or_hostname"), &self.editor_form.hostname)
                    .on_input(Message::EditorHostnameChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).into(),
            ]).align_y(iced::Alignment::Center),
        ]);

        // ── Section: General ──
        let general_section = panel_section(column![
            panel_field(t("label"), text_input(t("my_server_placeholder"), &self.editor_form.label)
                .on_input(Message::EditorLabelChanged).padding(10).style(crate::widgets::rounded_input_style).into()),
            Space::new().height(8),
            panel_field(t("parent_group"), text_input(t("group_placeholder"), &self.editor_form.group_name)
                .on_input(Message::EditorGroupChanged).padding(10).style(crate::widgets::rounded_input_style).into()),
        ]);

        // ── Section: SSH & Credentials ──
        let port_text = t("ssh_on_port").to_string();
        let mut ssh_items = column![
            // SSH on [port] port
            dir_row(vec![
                text(port_text).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(8).into(),
                text_input("22", &self.editor_form.port)
                    .on_input(Message::EditorPortChanged)
                    .padding(6)
                    .width(60)
                    .style(crate::widgets::rounded_input_style).into(),
            ]).align_y(iced::Alignment::Center),
        ]
        .push(Space::new().height(12))
        .push(text(t("credentials")).size(12).color(OryxisColors::t().text_muted))
        .push(Space::new().height(8))
        .push(
            dir_row(vec![
                iced_fonts::lucide::user().size(13).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                text_input(t("username"), &self.editor_form.username)
                    .on_input(Message::EditorUsernameChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).into(),
            ]).align_y(iced::Alignment::Center)
        );

        // Identity suggestion dropdown (only when username field is focused)
        if self.editor_form.username_focused && self.editor_form.selected_identity.is_none() && !self.identities.is_empty() {
            let search = self.editor_form.username.to_lowercase();
            let matching: Vec<&Identity> = if search.is_empty() {
                self.identities.iter().collect()
            } else {
                self.identities.iter()
                    .filter(|i| i.label.to_lowercase().contains(&search)
                        || i.username.as_deref().unwrap_or("").to_lowercase().contains(&search))
                    .collect()
            };
            if !matching.is_empty() {
                for identity in matching.iter().take(3) {
                    let label = identity.label.clone();
                    let subtitle = format!(
                        "{}{}",
                        identity.username.as_deref().unwrap_or(""),
                        if identity.key_id.is_some() {
                            let key_name = identity.key_id.and_then(|kid| {
                                self.keys.iter().find(|k| k.id == kid).map(|k| k.label.as_str())
                            }).unwrap_or("key");
                            format!(", {}", key_name)
                        } else { String::new() },
                    );
                    let ident_label = identity.label.clone();
                    ssh_items = ssh_items.push(
                        button(
                            container(
                                dir_row(vec![
                                    iced_fonts::lucide::user().size(12).color(OryxisColors::t().accent).into(),
                                    Space::new().width(8).into(),
                                    column![
                                        text(label.clone()).size(12).color(OryxisColors::t().text_primary),
                                        text(subtitle.clone()).size(10).color(OryxisColors::t().text_muted),
                                    ].into(),
                                ]).align_y(iced::Alignment::Center),
                            )
                            .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
                            .width(Length::Fill)
                            .style(|_| container::Style {
                                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                                border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
                                ..Default::default()
                            }),
                        )
                        .on_press(Message::EditorIdentityChanged(ident_label))
                        .width(Length::Fill)
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => Color::TRANSPARENT,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                ..Default::default()
                            }
                        }),
                    );
                    ssh_items = ssh_items.push(Space::new().height(2));
                }
            }
        }

        // If identity selected, show banner instead of password/key fields
        if let Some(ref ident_label) = self.editor_form.selected_identity {
            ssh_items = ssh_items.push(Space::new().height(8));
            ssh_items = ssh_items.push(
                container(
                    dir_row(vec![
                        iced_fonts::lucide::user().size(14).color(OryxisColors::t().accent).into(),
                        Space::new().width(8).into(),
                        column![
                            text(format!("{}: {}", t("identity"), ident_label)).size(12).color(OryxisColors::t().text_primary),
                            text(t("managed_by_identity")).size(10).color(OryxisColors::t().text_muted),
                        ].into(),
                        Space::new().width(Length::Fill).into(),
                        button(text("\u{00D7}").size(11).color(OryxisColors::t().text_muted))
                            .on_press(Message::EditorIdentityChanged("(none)".into()))
                            .padding(4)
                            .style(|_, _| button::Style::default()).into(),
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(10)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color { a: 0.15, ..OryxisColors::t().accent })),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().accent, width: 1.0 },
                    ..Default::default()
                }),
            );
        } else {
            // Show password + key fields normally
            ssh_items = ssh_items.push(Space::new().height(8));
            ssh_items = ssh_items.push(
                dir_row(vec![
                    iced_fonts::lucide::keyboard().size(13).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    text_input(
                        if self.editor_form.has_existing_password && !self.editor_form.password_touched {
                            "••••••••"
                        } else {
                            t("password")
                        },
                        &self.editor_form.password,
                    )
                        .on_input(Message::EditorPasswordChanged)
                        .secure(!self.editor_form.password_visible)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .into(),
                    Space::new().width(6).into(),
                    button(
                        if self.editor_form.password_visible {
                            iced_fonts::lucide::eye_off().size(14).color(OryxisColors::t().text_muted)
                        } else {
                            iced_fonts::lucide::eye().size(14).color(OryxisColors::t().text_muted)
                        }
                    )
                        .on_press(Message::EditorTogglePasswordVisibility)
                        .style(|_t, _s| button::Style::default())
                        .padding(8)
                        .into(),
                ]).align_y(iced::Alignment::Center)
            );
            ssh_items = ssh_items.push(Space::new().height(8));
            // "+ Key" is clickable — opens the existing key import panel.
            let add_key_btn = button(
                text(t("add_key_btn")).size(12).color(OryxisColors::t().accent),
            )
            .on_press(Message::ShowKeyPanel)
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Color { a: 0.1, ..OryxisColors::t().accent },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            ssh_items = ssh_items.push(
                dir_row(vec![
                    add_key_btn.into(),
                    Space::new().width(16).into(),
                    pick_list(
                        Some(self.editor_form.selected_key.clone().unwrap_or_else(|| "(none)".into())),
                        {
                            let mut opts = vec!["(none)".to_string()];
                            opts.extend(self.keys.iter().map(|k| k.label.clone()));
                            opts
                        },
                        |s: &String| s.clone(),
                    )
                    .on_select(Message::EditorKeyChanged)
                    .padding(10).style(crate::widgets::rounded_pick_list_style).into(),
                ]).align_y(iced::Alignment::Center)
            );
        }

        let ssh_section = panel_section(ssh_items);

        // ── Section: Advanced Options ──
        let jump_host_value = self.editor_form.jump_host.clone().unwrap_or_else(|| t("disabled").to_string());
        let auth_value = match self.editor_form.auth_method {
            AuthMethod::Auto => t("auth_auto"),
            AuthMethod::Password => t("auth_password"),
            AuthMethod::Key => t("auth_key"),
            AuthMethod::Agent => t("auth_agent"),
            AuthMethod::Interactive => t("auth_interactive"),
        };

        let advanced_section = panel_section(column![
            panel_option_row(
                iced_fonts::lucide::link(),
                t("host_chaining"),
                jump_host_value,
            ),
            panel_divider(),
            panel_option_pick(
                iced_fonts::lucide::shield(),
                t("auth_method"),
                vec![
                    t("auth_auto").to_string(),
                    t("auth_password").to_string(),
                    t("auth_key").to_string(),
                    t("auth_agent").to_string(),
                    t("auth_interactive").to_string(),
                ],
                auth_value.to_string(),
                Message::EditorAuthMethodChanged,
            ),
            panel_divider(),
            panel_option_pick_jump(
                iced_fonts::lucide::network(),
                t("jump_host"),
                {
                    let mut opts = vec!["(none)".to_string()];
                    for c in &self.connections {
                        if Some(c.id) != self.editor_form.editing_id {
                            opts.push(c.label.clone());
                        }
                    }
                    opts
                },
                self.editor_form.jump_host.clone().unwrap_or_else(|| "(none)".into()),
                Message::EditorJumpHostChanged,
            ),
            panel_divider(),
            dir_row(vec![
                iced_fonts::lucide::plug().size(14).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                text(t("expose_to_mcp")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                {
                    let on = self.editor_form.mcp_enabled;
                    let bg = if on { OryxisColors::t().success } else { OryxisColors::t().bg_hover };
                    let fg = crate::theme::contrast_text_for(bg);
                    button(text(if on { "ON" } else { "OFF" }).size(12).color(fg))
                        .on_press(Message::EditorToggleMcpEnabled)
                        .style(move |_theme, _status| button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            text_color: fg,
                            ..Default::default()
                        })
                        .into()
                },
            ]).align_y(iced::Alignment::Center),
            panel_divider(),
            dir_row(vec![
                iced_fonts::lucide::key_round().size(14).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                text(t("forward_ssh_agent")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                {
                    let on = self.editor_form.agent_forwarding;
                    let bg = if on { OryxisColors::t().success } else { OryxisColors::t().bg_hover };
                    let fg = crate::theme::contrast_text_for(bg);
                    button(text(if on { "ON" } else { "OFF" }).size(12).color(fg))
                        .on_press(Message::EditorToggleAgentForwarding)
                        .style(move |_theme, _status| button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            text_color: fg,
                            ..Default::default()
                        })
                        .into()
                },
            ]).align_y(iced::Alignment::Center),
        ]);

        // ── Section: Proxy ──
        let proxy_section = self.build_proxy_section();

        // ── Section: Port Forwarding ──
        let mut pf_items = column![
            dir_row(vec![
                iced_fonts::lucide::arrow_right_left().size(14).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                text(t("port_forwarding")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                button(text("+").size(14).color(OryxisColors::t().text_primary))
                    .on_press(Message::EditorAddPortForward)
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().bg_hover)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        text_color: OryxisColors::t().text_primary,
                        ..Default::default()
                    })
                    .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
                    .into(),
            ]).align_y(iced::Alignment::Center),
        ];

        for (i, pf) in self.editor_form.port_forwards.iter().enumerate() {
            let idx = i;
            pf_items = pf_items.push(Space::new().height(8));
            pf_items = pf_items.push(
                dir_row(vec![
                    text_input("8080", &pf.local_port)
                        .on_input(move |v| Message::EditorPortFwdLocalPortChanged(idx, v))
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style)
                        .into(),
                    text(" -> ").size(12).color(OryxisColors::t().text_muted).into(),
                    text_input("localhost", &pf.remote_host)
                        .on_input(move |v| Message::EditorPortFwdRemoteHostChanged(idx, v))
                        .padding(6)
                        .width(Length::Fill)
                        .style(crate::widgets::rounded_input_style)
                        .into(),
                    text(":").size(12).color(OryxisColors::t().text_muted).into(),
                    text_input("3306", &pf.remote_port)
                        .on_input(move |v| Message::EditorPortFwdRemotePortChanged(idx, v))
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style)
                        .into(),
                    button(text("\u{00D7}").size(11).color(OryxisColors::t().error))
                        .on_press(Message::EditorRemovePortForward(idx))
                        .style(|_, _| button::Style {
                            background: None,
                            border: Border::default(),
                            text_color: OryxisColors::t().error,
                            ..Default::default()
                        })
                        .padding(Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 4.0 })
                        .into(),
                ]).align_y(iced::Alignment::Center).spacing(4),
            );
        }

        let port_forward_section = panel_section(pf_items);

        // ── Error ──
        let panel_error: Element<'_, Message> = if let Some(err) = &self.host_panel_error {
            container(Element::from(text(err.clone()).size(11).color(OryxisColors::t().error)))
                .padding(Padding { top: 4.0, right: 16.0, bottom: 4.0, left: 16.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        // ── Bottom actions ──
        let save_btn_bg = if has_address { OryxisColors::t().accent } else { OryxisColors::t().bg_surface };
        let save_btn = button(
            container(text(crate::i18n::t("save")).size(14).color(OryxisColors::t().text_primary))
                .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
                .width(Length::Fill)
                .center_x(Length::Fill),
        )
        .on_press(Message::EditorSave)
        .width(Length::Fill)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(save_btn_bg)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let bottom = column![save_btn];
        // ── Layout ──
        let form_scroll = scrollable(
            column![
                address_section,
                Space::new().height(8),
                general_section,
                Space::new().height(8),
                ssh_section,
                Space::new().height(8),
                advanced_section,
                Space::new().height(8),
                proxy_section,
                Space::new().height(8),
                port_forward_section,
                Space::new().height(8),
                panel_error,
            ]
            .padding(Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
        )
        .height(Length::Fill);

        let panel_content = column![
            panel_header,
            form_scroll,
            container(bottom)
                .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 }),
        ]
        .height(Length::Fill);

        container(panel_content)
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
                ..Default::default()
            })
            .into()
    }

    /// Build the Proxy section. The picker mixes the static proxy types
    /// (None / SOCKS5 / SOCKS4 / HTTP / Command) with the user's saved
    /// `ProxyIdentity` entries — selecting an identity hides the inline
    /// fields and shows a readonly summary instead.
    fn build_proxy_section(&self) -> Element<'_, Message> {
        let kind = self.editor_form.proxy_kind;

        // Compose the picker option list. Identity entries come from
        // `self.proxy_identities` so the user can pick any saved
        // config without leaving the host editor.
        let mut options: Vec<ProxyKind> = ProxyKind::STATIC.to_vec();
        for pi in &self.proxy_identities {
            options.push(ProxyKind::Identity(pi.id));
        }

        // Capture the identities by reference so the closure can render
        // the user-chosen label for `Identity(_)` entries instead of
        // the generic Display fallback. The closure runs once per
        // option per render, which is fine.
        let identities = self.proxy_identities.clone();
        let identities_for_picker = identities.clone();
        let picker = panel_field(
            crate::i18n::t("proxy_type"),
            pick_list(Some(kind), options, move |k: &ProxyKind| match k {
                ProxyKind::Identity(id) => identities_for_picker
                    .iter()
                    .find(|pi| pi.id == *id)
                    .map(|pi| format!("📌 {}", pi.label))
                    .unwrap_or_else(|| crate::i18n::t("proxy_type_identity_deleted").into()),
                other => other.to_string(),
            })
            .on_select(Message::EditorProxyKindChanged)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style)
            .into(),
        );

        let mut col = column![picker];

        // Saved-identity selection: show a small readonly summary so
        // the user can see what they picked without flipping screens.
        // The actual identity edits live under Settings → Proxies.
        if let ProxyKind::Identity(id) = kind {
            let summary = identities
                .iter()
                .find(|pi| pi.id == id)
                .map(|pi| {
                    let kind_label = match &pi.proxy_type {
                        oryxis_core::models::connection::ProxyType::Socks5 => "SOCKS5",
                        oryxis_core::models::connection::ProxyType::Socks4 => "SOCKS4",
                        oryxis_core::models::connection::ProxyType::Http => "HTTP",
                        oryxis_core::models::connection::ProxyType::Command(_) => "CMD",
                    };
                    let user_part = pi
                        .username
                        .as_deref()
                        .map(|u| format!(" ({u})"))
                        .unwrap_or_default();
                    format!("{kind_label} — {}:{}{}", pi.host, pi.port, user_part)
                })
                .unwrap_or_else(|| crate::i18n::t("proxy_type_identity_deleted").into());
            col = col.push(Space::new().height(8)).push(
                text(summary).size(12).color(OryxisColors::t().text_muted),
            );
            return panel_section(col);
        }

        if kind == ProxyKind::None {
            return panel_section(col);
        }

        if kind == ProxyKind::Command {
            col = col
                .push(Space::new().height(8))
                .push(panel_field(
                    crate::i18n::t("proxy_command"),
                    text_input(
                        crate::i18n::t("proxy_command_placeholder"),
                        &self.editor_form.proxy_command,
                    )
                    .on_input(Message::EditorProxyCommandChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .into(),
                ));
            return panel_section(col);
        }

        if kind.needs_endpoint() {
            col = col
                .push(Space::new().height(8))
                .push(panel_field(
                    crate::i18n::t("proxy_host"),
                    text_input(
                        crate::i18n::t("proxy_host_placeholder"),
                        &self.editor_form.proxy_host,
                    )
                    .on_input(Message::EditorProxyHostChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .into(),
                ))
                .push(Space::new().height(8))
                .push(panel_field(
                    crate::i18n::t("proxy_port"),
                    text_input("1080", &self.editor_form.proxy_port)
                        .on_input(Message::EditorProxyPortChanged)
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style)
                        .into(),
                ))
                .push(Space::new().height(8))
                .push(panel_field(
                    crate::i18n::t("proxy_username"),
                    text_input(
                        crate::i18n::t("proxy_username_placeholder"),
                        &self.editor_form.proxy_username,
                    )
                    .on_input(Message::EditorProxyUsernameChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .into(),
                ));
        }

        if kind.supports_password() {
            // Mirror the main connection-password UX: show a hint when
            // the encrypted column already holds a value, and let the
            // user clear or replace it via the touched flag.
            let placeholder: &str = if self.editor_form.has_existing_proxy_password
                && !self.editor_form.proxy_password_touched
            {
                crate::i18n::t("proxy_password_existing")
            } else {
                crate::i18n::t("proxy_password_placeholder")
            };
            col = col
                .push(Space::new().height(8))
                .push(panel_field(
                    crate::i18n::t("proxy_password"),
                    text_input(placeholder, &self.editor_form.proxy_password)
                        .on_input(Message::EditorProxyPasswordChanged)
                        .secure(true)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .into(),
                ));
        }

        panel_section(col)
    }
}
