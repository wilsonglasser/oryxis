//! Host editor / connection editor side panel.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, scrollable, text, text_editor, text_input, Space};
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
    dir_align_x, dir_row, panel_divider, panel_field, panel_option_pick,
    panel_section, password_input_with_eye,
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
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
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
            editing_conn.and_then(|c| c.username.as_deref()),
            OryxisColors::t().accent,
        );
        // Icon is a button when we're editing an existing host, clicking it
        // opens the icon/color picker so the user can override the OS mark.
        // For new (unsaved) hosts the id doesn't exist yet, so it's just a
        // static badge until the first save.
        let icon_element: Element<'_, Message> = if let Some(id) = self.editor_form.editing_id {
            button(
                container(addr_glyph.view(18.0, Color::WHITE))
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
            container(addr_glyph.view(18.0, Color::WHITE))
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
                    .id(iced::widget::Id::new("editor-hostname"))
                    .on_input(Message::EditorHostnameChanged)
                    .on_submit(Message::EditorSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
            ]).align_y(iced::Alignment::Center),
        ]);

        // ── Section: General ──
        // Parent Group field now mirrors the Discover modal's
        // combo: text input + chevron that opens a floating
        // group-picker popover with its own search. Typing a brand
        // new name still creates the group on Save (existing
        // EditorGroupChanged → SaveConnection path is unchanged).
        const PARENT_COMBO_HEIGHT: f32 = 36.0;
        let parent_input = text_input(
            t("group_placeholder"),
            &self.editor_form.group_name,
        )
        .on_input(Message::EditorGroupChanged)
        .on_submit(Message::EditorSave)
        .padding(10)
        .width(Length::Fill)
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x());
        let parent_chevron = button(
            container(
                iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .center_x(Length::Fixed(32.0))
            .center_y(Length::Fixed(PARENT_COMBO_HEIGHT)),
        )
        .on_press(Message::ToggleGroupPicker(
            crate::state::GroupPickerTarget::EditorParent,
        ))
        .padding(0)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => OryxisColors::t().bg_surface,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }
        });
        let parent_combo: Element<'_, Message> = crate::widgets::bounds_reporter(
            dir_row(vec![
                container(parent_input)
                    .width(Length::Fill)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .into(),
                Space::new().width(6).into(),
                container(parent_chevron)
                    .height(Length::Fixed(PARENT_COMBO_HEIGHT))
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
            self.editor_parent_combo_bounds.clone(),
        );
        let general_section = panel_section(column![
            panel_field(t("label"), text_input(t("my_server_placeholder"), &self.editor_form.label)
                .on_input(Message::EditorLabelChanged).on_submit(Message::EditorSave).padding(10).style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into()),
            Space::new().height(8),
            panel_field(t("parent_group"), parent_combo),
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
                    .on_submit(Message::EditorSave)
                    .padding(6)
                    .width(60)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
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
                    .on_submit(Message::EditorSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
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
            let pw_placeholder: &'static str = if self.editor_form.has_existing_password
                && !self.editor_form.password_touched
            {
                "••••••••"
            } else {
                t("password")
            };
            ssh_items = ssh_items.push(
                dir_row(vec![
                    iced_fonts::lucide::keyboard().size(13).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    password_input_with_eye(
                        pw_placeholder,
                        &self.editor_form.password,
                        Message::EditorPasswordChanged,
                        Some(Message::EditorSave),
                        self.editor_form.password_visible,
                        Message::EditorTogglePasswordVisibility,
                        10.0,
                    ),
                ]).align_y(iced::Alignment::Center)
            );
            ssh_items = ssh_items.push(Space::new().height(8));
            // "+ Key" is clickable, opens the existing key import panel.
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

        // Cloud-managed transport picker, only rendered when the
        // connection being edited carries a `cloud_ref` (i.e. it was
        // imported from a cloud provider). Lets the user flip between
        // SSH (default) and AWS Instance Connect / SSM transports.
        if let Some(current) = self.editor_form.cloud_transport {
            use oryxis_core::models::cloud::TransportKind;
            let options = vec![
                TransportKind::Ssh,
                TransportKind::InstanceConnect,
                TransportKind::Ssm,
            ];
            ssh_items = ssh_items
                .push(Space::new().height(12))
                .push(text(t("cloud_dynamic_form_transport")).size(12).color(OryxisColors::t().text_muted))
                .push(Space::new().height(8))
                .push(
                    pick_list(Some(current), options, |t| match t {
                        TransportKind::Ssh => "SSH".to_string(),
                        TransportKind::InstanceConnect => "EC2 Instance Connect".to_string(),
                        TransportKind::Ssm => "SSM Session".to_string(),
                        TransportKind::EcsExec => "ECS Exec".to_string(),
                        TransportKind::KubectlExec => "kubectl exec".to_string(),
                    })
                    .on_select(Message::EditorCloudTransportChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style),
                );
        }

        // Initial command, sent to the shell right after the
        // session opens. Useful for hosts that drop into `/bin/sh`
        // when the user wanted `bash`, or to `cd` somewhere by
        // default. Lives at the end of the SSH section because it's
        // a per-shell concern, not auth.
        ssh_items = ssh_items
            .push(Space::new().height(12))
            .push(
                text(t("initial_command_label"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .push(Space::new().height(8))
            .push(
                // Multi-line, auto-grows with content; container caps the
                // height (~8 lines) and then it scrolls internally. Supports
                // multi-command scripts (one command per line).
                container(
                    text_editor(&self.editor_initial_command)
                        .placeholder(t("initial_command_ph"))
                        .on_action(Message::EditorInitialCommandChanged)
                        .padding(10)
                        .height(Length::Shrink)
                        .style(crate::widgets::rounded_editor_style),
                )
                .max_height(200.0),
            );

        let ssh_section = panel_section(ssh_items);

        // ── Section: Advanced Options ──
        // Chain summary for the "Host Chaining" row: the hop labels
        // joined in order (bastion > db-proxy > ...), or "disabled"
        // when empty. Hops pointing at a since-deleted host resolve to
        // a placeholder rather than vanishing, so the count stays
        // honest until the user opens the editor and prunes them.
        let chain_summary = if self.editor_form.jump_chain.is_empty() {
            t("disabled").to_string()
        } else {
            self.editor_form
                .jump_chain
                .iter()
                .map(|id| {
                    self.connections
                        .iter()
                        .find(|c| c.id == *id)
                        .map(|c| c.label.clone())
                        .unwrap_or_else(|| t("unknown").to_string())
                })
                .collect::<Vec<_>>()
                .join(" › ")
        };
        let auth_value = match self.editor_form.auth_method {
            AuthMethod::Auto => t("auth_auto"),
            AuthMethod::Password => t("auth_password"),
            AuthMethod::Key => t("auth_key"),
            AuthMethod::Agent => t("auth_agent"),
            AuthMethod::Interactive => t("auth_interactive"),
        };

        let advanced_section = panel_section(column![
            // Single "Host Chaining" entry point. Clicking opens the
            // chain editor (Termius-style multi-hop). Replaces the old
            // read-only row + separate single-host "Jump Host" picker,
            // which both edited the same `jump_chain` and were
            // redundant.
            container(
                button(
                    dir_row(vec![
                        iced_fonts::lucide::link().size(14).color(OryxisColors::t().text_muted).into(),
                        Space::new().width(10).into(),
                        text(t("host_chaining")).size(13).color(OryxisColors::t().text_secondary).into(),
                        Space::new().width(Length::Fill).into(),
                        text(chain_summary)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(8).into(),
                        iced_fonts::lucide::chevron_right().size(12).color(OryxisColors::t().text_muted).into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::OpenChainEditor)
                .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 0.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(6.0), ..Default::default() },
                        ..Default::default()
                    }
                })
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
            container(
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
                ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }),
            panel_divider(),
            container(
                dir_row(vec![
                    iced_fonts::lucide::file_text().size(14).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    text(t("session_logging")).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    {
                        // Tri-state: Default (inherit global) / On / Off.
                        let (label_key, bg) = match self.editor_form.session_logging {
                            None => ("session_log_default", OryxisColors::t().bg_hover),
                            Some(true) => ("session_log_on", OryxisColors::t().success),
                            Some(false) => ("session_log_off", OryxisColors::t().error),
                        };
                        let fg = crate::theme::contrast_text_for(bg);
                        button(text(t(label_key)).size(12).color(fg))
                            .on_press(Message::EditorCycleSessionLogging)
                            .style(move |_theme, _status| button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: fg,
                                ..Default::default()
                            })
                            .into()
                    },
                ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }),
            panel_divider(),
            container(
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
                ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }),
            panel_divider(),
            // Per-host keepalive override. Empty placeholder reflects the
            // global default so the user sees what "inherit" means without
            // opening Settings. "0" in the field disables keepalive on this
            // host even when the global default is non-zero.
            container(
                dir_row(vec![
                    iced_fonts::lucide::activity().size(14).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(10).into(),
                    column![
                        text(t("host_keepalive")).size(13).color(OryxisColors::t().text_secondary),
                        Space::new().height(2),
                        text(t("host_keepalive_desc")).size(11).color(OryxisColors::t().text_muted),
                    ].width(Length::Fill).into(),
                    Space::new().width(12).into(),
                    text_input(
                        &self.setting_keepalive_interval,
                        &self.editor_form.keepalive_interval,
                    )
                        .on_input(Message::EditorKeepaliveChanged)
                        .on_submit(Message::EditorSave)
                        .padding(6)
                        .width(100)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                ]).align_y(iced::Alignment::Center)
            )
            .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 }),
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
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                    text(" -> ").size(12).color(OryxisColors::t().text_muted).into(),
                    text_input("localhost", &pf.remote_host)
                        .on_input(move |v| Message::EditorPortFwdRemoteHostChanged(idx, v))
                        .padding(6)
                        .width(Length::Fill)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                    text(":").size(12).color(OryxisColors::t().text_muted).into(),
                    text_input("3306", &pf.remote_port)
                        .on_input(move |v| Message::EditorPortFwdRemotePortChanged(idx, v))
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
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

        // ── Section: Environment Variables ──
        let mut env_items = column![
            dir_row(vec![
                iced_fonts::lucide::variable().size(14).color(OryxisColors::t().text_muted).into(),
                Space::new().width(10).into(),
                column![
                    text(t("env_vars")).size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(2),
                    text(t("env_vars_desc")).size(11).color(OryxisColors::t().text_muted),
                ].width(Length::Fill).into(),
                Space::new().width(8).into(),
                button(text("+").size(14).color(OryxisColors::t().text_primary))
                    .on_press(Message::EditorAddEnvVar)
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

        for (i, e) in self.editor_form.env_vars.iter().enumerate() {
            let idx = i;
            env_items = env_items.push(Space::new().height(8));
            env_items = env_items.push(
                dir_row(vec![
                    text_input("LC_EXAMPLE", &e.key)
                        .on_input(move |v| Message::EditorEnvVarKeyChanged(idx, v))
                        .padding(6)
                        .width(Length::FillPortion(2))
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                    text("=").size(12).color(OryxisColors::t().text_muted).into(),
                    text_input("value", &e.value)
                        .on_input(move |v| Message::EditorEnvVarValueChanged(idx, v))
                        .padding(6)
                        .width(Length::FillPortion(3))
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                    button(text("\u{00D7}").size(11).color(OryxisColors::t().error))
                        .on_press(Message::EditorRemoveEnvVar(idx))
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

        let env_var_section = panel_section(env_items);

        // ── Section: Terminal appearance ──
        // A single "click to open picker" tile that mirrors the
        // current pick (palette swatches if a specific theme is set,
        // a plain "inherit" row otherwise). The full picker lives in
        // its own modal so this section stays compact.
        // Themed preview tile: shows the chosen per-host palette, or the
        // inherited global theme when there's no override, so the row is
        // always a real preview instead of a bare "use global" dropdown.
        // Click opens the full picker modal.
        // Resolve the override (built-in OR custom) to a palette for the
        // preview swatch; fall back to the inherited global when there's no
        // override (or the named custom theme was deleted).
        let override_name = self
            .editor_form
            .terminal_theme
            .as_deref()
            .filter(|name| self.terminal_palette_for_name(name).is_some());
        let (preview_palette, theme_label) = match override_name {
            Some(name) => (
                self.terminal_palette_for_name(name).unwrap(),
                name.to_string(),
            ),
            None => (
                self.resolve_global_terminal_palette(),
                format!(
                    "{} ({})",
                    crate::i18n::t("terminal_theme_inherit_global"),
                    self.resolve_global_terminal_theme_name()
                ),
            ),
        };
        let theme_trigger: Element<'_, Message> = terminal_theme_trigger(preview_palette, theme_label);

        // Per-host icon shape override. The "Use default" entry maps to
        // an empty string which clears the override (resolved to the
        // global default_host_icon at render time).
        // Tokens drive the picker value (same pattern as Settings
        // -> Interface). Empty string is the "use default" token; the
        // dispatcher treats it as a None override on the form field.
        let icon_options = vec![
            String::new(),
            "circular".to_string(),
            "square".to_string(),
            "rounded".to_string(),
            "outline".to_string(),
            "initials".to_string(),
        ];
        let icon_selected = self.editor_form.icon_style.clone().unwrap_or_default();
        let icon_picker = pick_list(
            Some(icon_selected),
            icon_options,
            |s: &String| {
                let key = match s.as_str() {
                    "circular" => "icon_circular",
                    "square" => "icon_square",
                    "rounded" => "icon_rounded",
                    "outline" => "icon_outline",
                    "initials" => "icon_initials",
                    _ => "icon_use_default",
                };
                crate::i18n::t(key).to_string()
            },
        )
        .on_select(Message::EditorIconStyleChanged)
        .width(220)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        // Per-host terminal encoding. "UTF-8" is the default (stored as
        // None); the rest are encoding_rs labels the SSH engine transcodes.
        let encoding_options: Vec<String> = [
            "UTF-8", "Big5", "GBK", "gb18030", "Shift_JIS", "EUC-JP",
            "EUC-KR", "ISO-8859-1", "ISO-8859-15", "windows-1251",
            "windows-1252", "KOI8-R",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let encoding_selected = self
            .editor_form
            .encoding
            .clone()
            .unwrap_or_else(|| "UTF-8".to_string());
        let encoding_picker = pick_list(Some(encoding_selected), encoding_options, |s: &String| s.clone())
            .on_select(Message::EditorEncodingChanged)
            .width(220)
            .padding(10)
            .style(crate::widgets::rounded_pick_list_style);

        let appearance_section = panel_section(column![
            text(crate::i18n::t("terminal_theme"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(crate::i18n::t("host_terminal_theme_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            theme_trigger,
            Space::new().height(12),
            text(crate::i18n::t("host_icon_style"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(crate::i18n::t("host_icon_style_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            icon_picker,
            Space::new().height(12),
            text(crate::i18n::t("host_encoding"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(crate::i18n::t("host_encoding_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            encoding_picker,
        ]);

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

        // The error must live OUTSIDE the scrollable so it sits above
        // the Save button at the bottom of the panel, otherwise long
        // forms hide it below the fold and the user clicks Save again
        // wondering why nothing happens.
        let bottom = column![panel_error, save_btn].spacing(8);
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
                env_var_section,
                Space::new().height(8),
                appearance_section,
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
    /// `ProxyIdentity` entries, selecting an identity hides the inline
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
                    format!("{kind_label}, {}:{}{}", pi.host, pi.port, user_part)
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
                    .on_submit(Message::EditorSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
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
                    .on_submit(Message::EditorSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
                ))
                .push(Space::new().height(8))
                .push(panel_field(
                    crate::i18n::t("proxy_port"),
                    text_input("1080", &self.editor_form.proxy_port)
                        .on_input(Message::EditorProxyPortChanged)
                        .on_submit(Message::EditorSave)
                        .padding(6)
                        .width(70)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
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
                    .on_submit(Message::EditorSave)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
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
                        .on_submit(Message::EditorSave)
                        .secure(true)
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into(),
                ));
        }

        panel_section(col)
    }
}

/// Full-width "click to open the theme picker" tile, painted in a
/// terminal palette: `label` in the theme foreground, ANSI swatches on
/// the trailing edge, the theme background as the fill. Used for both a
/// chosen per-host theme and the "use global" state (where it previews
/// the inherited global theme).
fn terminal_theme_trigger<'a>(
    palette: oryxis_terminal::TerminalPalette,
    label: String,
) -> Element<'a, Message> {
    let bg = palette.background;
    let fg = palette.foreground;
    let swatches: Vec<Element<'a, Message>> = [1usize, 2, 3, 4, 5, 6]
        .iter()
        .map(|&i| {
            let color = palette.ansi[i];
            container(
                Space::new()
                    .width(Length::Fixed(10.0))
                    .height(Length::Fixed(10.0)),
            )
            .style(move |_| container::Style {
                background: Some(Background::Color(color)),
                border: Border { radius: Radius::from(5.0), ..Default::default() },
                ..Default::default()
            })
            .into()
        })
        .collect();
    button(
        container(
            dir_row(vec![
                text(label).size(13).color(fg).into(),
                Space::new().width(Length::Fill).into(),
                iced::widget::Row::with_children(swatches).spacing(4).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .width(Length::Fill),
    )
    .on_press(Message::EditorOpenThemePicker)
    .padding(0)
    .width(Length::Fill)
    .style(move |_, _| button::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: Radius::from(8.0), ..Default::default() },
        ..Default::default()
    })
    .into()
}
