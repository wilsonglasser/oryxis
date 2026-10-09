//! Settings -> MCP Server section view: the server enable toggle, the
//! setup guide button, and the expandable info panel with the config
//! snippet, token management and the vault-password embed flow.

use super::*;
use iced::widget::column;

use crate::mcp::{client_path_hint, mcp_config_snippet_display, token_mask};
use crate::mcp_clients::McpClient;

impl Oryxis {
    /// Standalone MCP Server settings section. Was nested inside the
    /// Security panel in 0.6 when MCP shipped with the installer; in
    /// 0.7 it lives in its own Settings sidebar entry because the
    /// plugin distribution + setup-guide affordances deserve room
    /// without competing with the Security toggles.
    pub(super) fn view_settings_mcp(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order: the guide button
        // is defined here but only recorded (slot-wrapped) at its use
        // site below, after the server toggle.
        self.keynav_settings_reset();
        // The MCP plugin is managed (installed / updated) from the
        // Plugins screen; the server's own on/off lives here.
        let mcp_guide_btn = button(
            container(text(crate::i18n::t("mcp_setup_guide")).size(12).color(OryxisColors::t().accent))
                .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
        )
        .on_press(if self.mcp.show_info { Message::Mcp(McpMessage::HideMcpInfo) } else { Message::Mcp(McpMessage::ShowMcpInfo) })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.1, ..OryxisColors::t().accent },
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), color: OryxisColors::t().accent, width: 1.0 },
                ..Default::default()
            }
        });
        let mut mcp_col = column![
            self.nav_toggle_row(
                crate::i18n::t("mcp_server"),
                self.mcp.server_enabled,
                Message::Mcp(McpMessage::ToggleMcpServer),
            ),
            Space::new().height(12),
            dir_row(vec![
                text(crate::i18n::t("mcp_server_desc")).size(11).color(OryxisColors::t().text_muted).boxed(),
                Space::new().width(Length::Fill).boxed(),
                self.settings_nav_slot_labeled(
                    crate::i18n::t("mcp_setup_guide"),
                    crate::keynav::RowAction::activate(if self.mcp.show_info {
                        Message::Mcp(McpMessage::HideMcpInfo)
                    } else {
                        Message::Mcp(McpMessage::ShowMcpInfo)
                    }),
                    6.0,
                    mcp_guide_btn.boxed(),
                ),
            ]).align_y(iced::Alignment::Center),
        ];
        if self.mcp.show_info {
            mcp_col = mcp_col
                .push(Space::new().height(12).boxed())
                .push(mcp_info_panel(self));
        }

        scrollable(
            container(
                column![
                    panel_section(mcp_col),
                    Space::new().height(24),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-mcp-scroll"))
        .on_scroll(|s| Message::Settings(SettingsMessage::SectionScrolled(s.viewport.relative_offset().y)))
        .height(Length::Fill)
        .boxed()
    }
}

/// A selectable chip (the Native / WSL target and the AI client rows):
/// filled with the accent while selected, hover feedback otherwise.
fn chip_btn<'a>(label: Element<'a, Message>, selected: bool, msg: Message) -> Element<'a, Message> {
    button(container(label).padding(Padding { top: 4.0, right: 14.0, bottom: 4.0, left: 14.0 }))
        .on_press(msg)
        .style(move |_, status| {
            let bg = if selected {
                OryxisColors::t().accent
            } else if matches!(status, BtnStatus::Hovered) {
                OryxisColors::t().bg_hover
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            }
        })
        .boxed()
}

/// Text colour on a chip: inverted on the accent fill.
fn chip_text_color(selected: bool) -> Color {
    if selected {
        OryxisColors::t().bg_primary
    } else {
        OryxisColors::t().text_secondary
    }
}

/// Monospaced code block widget.
fn code_block<'a>(content: &str) -> Element<'a, Message> {
    container(
        // `selectable(true)` lets the user drag-highlight the snippet
        // and copy it with Ctrl+C, instead of being forced through the
        // Copy button.
        text(content.to_owned()).size(12).selectable(true).color(OryxisColors::t().text_primary),
    )
    .padding(12)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_primary)),
        border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
        ..Default::default()
    })
    .boxed()
}

/// The expandable MCP info panel shown inside the MCP Server settings.
/// Takes the app so every interactive control can record a settings
/// keynav slot, which also means construction below strictly follows
/// visual order.
fn mcp_info_panel(app: &crate::app::Oryxis) -> Element<'_, Message> {
    let copied = app.mcp.config_copied;
    let install_status = &app.mcp.install_status;
    let token: &str = &app.mcp.server_token;
    let token_visible = app.mcp.token_visible;
    let target_wsl = app.mcp.target_wsl;
    let client = app.mcp.client;
    let client_detected = app.mcp.detected.contains(&client);
    let vault_pw = app.mcp_vault_pw();

    // `target_wsl` switches the snippet (and the Copy / Install button
    // targets, handled in dispatch) between the native client and a
    // Claude Code / Cursor running inside WSL. The toggle that flips it
    // is Windows-only, so on other platforms this stays false.
    // The snippet obeys the SAME visibility the token row does: it
    // spells out the token and, when embedded, the vault password, so
    // revealing one and hiding the other would be a mask in name only.
    // Copy / Install rebuild the JSON from state and always carry the
    // real values.
    let json_text =
        mcp_config_snippet_display(client, token, vault_pw.as_deref(), target_wsl, token_visible);
    let path_hint = client_path_hint(client, target_wsl, &app.mcp.wsl_markers);

    // "Copy", not "Copy JSON": the snippet is TOML for Codex.
    let copy_label = if copied {
        crate::i18n::t("mcp_copied")
    } else {
        crate::i18n::t("mcp_token_copy")
    };
    let copy_color = if copied { OryxisColors::t().success } else { OryxisColors::t().accent };

    let copy_btn = button(
        container(text(copy_label).size(12).color(copy_color))
            .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
    )
    .on_press(Message::Mcp(McpMessage::CopyMcpConfig))
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.1, ..copy_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: copy_color, width: 1.0 },
            ..Default::default()
        }
    });

    // Install writes into the client's own folder, so it is only
    // offered once that folder was seen: an undetected client keeps
    // Copy, and the note under the chips says why.
    let (install_label, install_color) = match install_status {
        Some(Ok(_)) => (crate::i18n::t("mcp_installed").to_string(), OryxisColors::t().success),
        Some(Err(_)) => (crate::i18n::t("mcp_install_failed").to_string(), OryxisColors::t().error),
        None if client_detected => (
            crate::i18n::t("mcp_install_to").replace("{client}", client.name()),
            OryxisColors::t().success,
        ),
        None => (
            crate::i18n::t("mcp_install_to").replace("{client}", client.name()),
            OryxisColors::t().text_muted,
        ),
    };
    let install_btn = button(
        container(text(install_label).size(12).color(install_color))
            .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
    )
    .on_press_maybe(client_detected.then_some(Message::Mcp(McpMessage::InstallMcpConfig)))
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.1, ..install_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: install_color, width: 1.0 },
            ..Default::default()
        }
    });

    let close_btn = button(
        container(text(crate::i18n::t("mcp_info_close")).size(12).color(OryxisColors::t().text_muted))
            .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
    )
    .on_press(Message::Mcp(McpMessage::HideMcpInfo))
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        }
    });

    // Token row: shows the active MCP token (masked by default), with
    // show/hide, copy, and regenerate affordances. Show/Hide governs
    // every secret on the panel, this row and the snippet below, which
    // is where the token and the opt-in vault password are spelled out
    // again.
    let token_display: String = if token.is_empty() {
        crate::i18n::t("mcp_token_unset").to_string()
    } else if token_visible {
        token.to_string()
    } else {
        token_mask(token)
    };
    let token_color = if token.is_empty() {
        OryxisColors::t().warning
    } else {
        OryxisColors::t().text_primary
    };
    let toggle_label = if token_visible {
        crate::i18n::t("mcp_token_hide")
    } else {
        crate::i18n::t("mcp_token_show")
    };

    fn token_action_btn<'a>(
        label: &'a str,
        color: Color,
        msg: Message,
    ) -> Element<'a, Message> {
        button(
            container(text(label).size(11).color(color))
                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
        )
        .on_press(msg)
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.12, ..color },
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), color, width: 1.0 },
                ..Default::default()
            }
        })
        .boxed()
    }

    let mut info_col = column![
        text(crate::i18n::t("mcp_info_title")).size(14).color(OryxisColors::t().text_primary),
        Space::new().height(8),
        text(crate::i18n::t("mcp_info_desc")).size(12).color(OryxisColors::t().text_secondary),
    ];

    // Target toggle (Native / WSL): only relevant on Windows, where the
    // binary is an `.exe` a WSL-resident client reaches via `/mnt/c`.
    // On other platforms there is a single target, so the toggle is
    // omitted and `target_wsl` stays false.
    #[cfg(target_os = "windows")]
    {
        fn target_btn<'a>(label: &'a str, selected: bool, msg: Message) -> Element<'a, Message> {
            chip_btn(
                text(label).size(11).color(chip_text_color(selected)).boxed(),
                selected,
                msg,
            )
        }

        let target_row = crate::widgets::dir_row(vec![
            text(crate::i18n::t("mcp_target_label"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .boxed(),
            Space::new().width(8).boxed(),
            app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Mcp(McpMessage::SetMcpTarget(false))),
                6.0,
                target_btn(crate::i18n::t("mcp_target_native"), !target_wsl, Message::Mcp(McpMessage::SetMcpTarget(false))),
            ),
            Space::new().width(6).boxed(),
            app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Mcp(McpMessage::SetMcpTarget(true))),
                6.0,
                target_btn(crate::i18n::t("mcp_target_wsl"), target_wsl, Message::Mcp(McpMessage::SetMcpTarget(true))),
            ),
        ])
        .align_y(iced::Alignment::Center);

        info_col = info_col.push(Space::new().height(12).boxed()).push(target_row.boxed());
    }

    // Client row: one chip per AI client this target can hold, the
    // detected ones marked, the selected one filled. The first chip
    // carries the labeled slot so the settings search can reveal the
    // row. Below it, one line says what the detection found about the
    // selected client (or that it is still looking).
    let clients: Vec<McpClient> = if target_wsl {
        McpClient::available_in_wsl()
    } else {
        McpClient::available()
    };
    let mut client_items: Vec<Element<'_, Message>> = vec![
        text(crate::i18n::t("mcp_clients_label"))
            .size(11)
            .color(OryxisColors::t().text_muted)
            .boxed(),
        Space::new().width(8).boxed(),
    ];
    for (i, c) in clients.iter().copied().enumerate() {
        let selected = c == client;
        let detected = app.mcp.detected.contains(&c);
        let msg = Message::Mcp(McpMessage::SetMcpClient(c));
        let mut label_items: Vec<Element<'_, Message>> = Vec::new();
        if detected {
            // A dot a reader scans faster than a word; the word is the
            // tooltip.
            label_items.push(
                text("\u{25CF}")
                    .size(9)
                    .color(if selected { OryxisColors::t().bg_primary } else { OryxisColors::t().success })
                    .boxed(),
            );
            label_items.push(Space::new().width(5).boxed());
        }
        label_items.push(text(c.name()).size(11).color(chip_text_color(selected)).boxed());
        let chip = chip_btn(
            crate::widgets::dir_row(label_items).align_y(iced::Alignment::Center).boxed(),
            selected,
            msg.clone(),
        );
        let chip = if detected {
            crate::views::terminal::icon_tooltip(chip, crate::i18n::t("mcp_client_detected"))
        } else {
            chip
        };
        let action = crate::keynav::RowAction::activate(msg);
        let slot = if i == 0 {
            app.settings_nav_slot_labeled(crate::i18n::t("mcp_clients_label"), action, 6.0, chip)
        } else {
            app.settings_nav_slot(action, 6.0, chip)
        };
        if i > 0 {
            client_items.push(Space::new().width(6).boxed());
        }
        client_items.push(slot);
    }
    let client_row = crate::widgets::dir_row(client_items)
        .align_y(iced::Alignment::Center)
        .wrap();
    info_col = info_col.push(Space::new().height(12).boxed()).push(client_row.boxed());
    if app.mcp.detecting {
        info_col = info_col.push(Space::new().height(4).boxed()).push(
            text(crate::i18n::t("mcp_client_detecting"))
                .size(10)
                .color(OryxisColors::t().text_muted)
                .boxed(),
        );
    } else if !client_detected {
        info_col = info_col.push(Space::new().height(4).boxed()).push(
            text(crate::i18n::t("mcp_client_not_detected").replace("{client}", client.name()))
                .size(10)
                .color(OryxisColors::t().warning)
                .boxed(),
        );
    }

    // Token row, built after the target row so the keynav slots the
    // action buttons record land in visual order.
    let mut token_items: Vec<Element<'_, Message>> = vec![
        text(crate::i18n::t("mcp_token_label"))
            .size(11)
            .color(OryxisColors::t().text_muted)
            .boxed(),
        Space::new().width(8).boxed(),
        container(
            text(token_display)
                .size(11)
                .selectable(true)
                .font(iced::Font::MONOSPACE)
                .color(token_color),
        )
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        })
        .boxed(),
    ];
    // The reveal is offered whenever the panel holds a secret at all: a
    // vault whose auth token is unset can still carry an embedded master
    // password in the snippet, and a masked value with no way back would
    // be worse than showing it.
    if !token.is_empty() || vault_pw.is_some() {
        token_items.push(Space::new().width(8).boxed());
        token_items.push(app.settings_nav_slot(
            crate::keynav::RowAction::activate(Message::Mcp(McpMessage::ToggleMcpTokenVisibility)),
            6.0,
            token_action_btn(
                toggle_label,
                OryxisColors::t().text_secondary,
                Message::Mcp(McpMessage::ToggleMcpTokenVisibility),
            ),
        ));
    }
    if !token.is_empty() {
        token_items.push(Space::new().width(6).boxed());
        token_items.push(app.settings_nav_slot(
            crate::keynav::RowAction::activate(Message::Mcp(McpMessage::CopyMcpToken)),
            6.0,
            token_action_btn(
                crate::i18n::t("mcp_token_copy"),
                OryxisColors::t().accent,
                Message::Mcp(McpMessage::CopyMcpToken),
            ),
        ));
    }
    token_items.push(Space::new().width(6).boxed());
    token_items.push(app.settings_nav_slot_labeled(
        crate::i18n::t("mcp_token_regenerate"),
        crate::keynav::RowAction::activate(Message::Mcp(McpMessage::RegenerateMcpToken)),
        6.0,
        token_action_btn(
            crate::i18n::t("mcp_token_regenerate"),
            OryxisColors::t().warning,
            Message::Mcp(McpMessage::RegenerateMcpToken),
        ),
    ));
    let token_row = crate::widgets::dir_row(token_items)
        .align_y(iced::Alignment::Center);

    info_col = info_col
        .push(Space::new().height(12).boxed())
        .push(token_row.boxed())
        .push(Space::new().height(4).boxed())
        .push(
            text(crate::i18n::t("mcp_token_desc"))
                .size(10)
                .color(OryxisColors::t().text_muted).boxed(),
        )
        .push(Space::new().height(12).boxed())
        .push(code_block(&json_text))
        .push(Space::new().height(8).boxed())
        .push(
            text(format!("{} {}", crate::i18n::t("mcp_info_path_label"), path_hint))
                .size(11)
                .color(OryxisColors::t().text_muted).boxed(),
        );

    // Explain that the WSL snippet targets a client living inside the
    // distro, shown only while that target is selected.
    #[cfg(target_os = "windows")]
    if target_wsl {
        info_col = info_col
            .push(Space::new().height(8).boxed())
            .push(
                text(crate::i18n::t("mcp_info_note_wsl"))
                    .size(11)
                    .color(OryxisColors::t().warning).boxed(),
            );
    }

    // The install verdict arrives on its own, so it fills a slot that
    // is always there: pushed only once it landed, it shifted the vault
    // password block below and dropped the focus of its typed
    // confirmation.
    let mut install_slot = iced::widget::Column::<iced::Element<'_, _>>::new();
    if let Some(Err(e)) = install_status {
        install_slot = install_slot
            .push(Space::new().height(4).boxed())
            .push(text(e.clone()).size(11).color(OryxisColors::t().error).boxed());
    } else if let Some(Ok(path)) = install_status {
        install_slot = install_slot
            .push(Space::new().height(4).boxed())
            .push(text(format!("{} {path}", crate::i18n::t("mcp_installed_to"))).size(11).color(OryxisColors::t().success).boxed());
    }
    info_col = info_col.push(install_slot.boxed());

    // ── Vault password (ORYXIS_VAULT_PASSWORD) ──
    // A password-protected vault makes the MCP server exit at startup
    // unless the client passes the master password, which MCP clients
    // report as a failed connection (issue #72). Surface that here and
    // offer to embed the password after an explicit typed confirmation;
    // without a master password the static note still explains the
    // variable for users who add one later.
    info_col = info_col.push(Space::new().height(8).boxed());
    if !app.vault_ui.has_user_password {
        info_col = info_col.push(
            text(crate::i18n::t("mcp_info_vault_password_note"))
                .size(11)
                .color(OryxisColors::t().text_muted).boxed(),
        );
    } else if app.mcp.include_vault_password {
        info_col = info_col
            .push(
                crate::widgets::dir_row(vec![
                    text(crate::i18n::t("mcp_vault_pw_included"))
                        .size(11)
                        .color(OryxisColors::t().success)
                        .boxed(),
                    Space::new().width(8).boxed(),
                    app.settings_nav_slot(
                        crate::keynav::RowAction::activate(Message::Mcp(McpMessage::McpVaultPwRemove)),
                        6.0,
                        token_action_btn(
                            crate::i18n::t("remove"),
                            OryxisColors::t().warning,
                            Message::Mcp(McpMessage::McpVaultPwRemove),
                        ),
                    ),
                ])
                .align_y(iced::Alignment::Center).boxed(),
            )
            .push(Space::new().height(4).boxed())
            .push(
                text(crate::i18n::t("mcp_vault_pw_plaintext_warning"))
                    .size(10)
                    .color(OryxisColors::t().warning).boxed(),
            );
    } else if let Some(typed) = &app.mcp.vault_pw_prompt {
        // Typed confirmation: embedding only happens after the user
        // proves they know the master password, so an unattended
        // unlocked app can't be used to exfiltrate it into a file.
        let input_id = iced::widget::Id::new("mcp-vault-pw");
        let pw_input = app.settings_nav_slot(
            crate::keynav::RowAction::input(input_id.clone()),
            6.0,
            iced::widget::text_input(crate::i18n::t("mcp_vault_pw_placeholder"), typed)
                .id(input_id)
                .secure(true)
                .on_input(|v| Message::Mcp(McpMessage::McpVaultPwInput(v)))
                .on_submit(Message::Mcp(McpMessage::McpVaultPwConfirm))
                .padding(8)
                .size(12)
                .width(240)
                .style(crate::widgets::rounded_input_style)
                .boxed(),
        );
        info_col = info_col
            .push(
                text(crate::i18n::t("mcp_vault_pw_confirm_prompt"))
                    .size(11)
                    .color(OryxisColors::t().text_secondary).boxed(),
            )
            .push(Space::new().height(6).boxed())
            .push(
                crate::widgets::dir_row(vec![
                    pw_input,
                    Space::new().width(6).boxed(),
                    app.settings_nav_slot(
                        crate::keynav::RowAction::activate(Message::Mcp(McpMessage::McpVaultPwConfirm)),
                        6.0,
                        token_action_btn(
                            crate::i18n::t("mcp_vault_pw_confirm"),
                            OryxisColors::t().success,
                            Message::Mcp(McpMessage::McpVaultPwConfirm),
                        ),
                    ),
                    Space::new().width(6).boxed(),
                    app.settings_nav_slot(
                        crate::keynav::RowAction::activate(Message::Mcp(McpMessage::McpVaultPwPromptCancel)),
                        6.0,
                        token_action_btn(
                            crate::i18n::t("cancel"),
                            OryxisColors::t().text_secondary,
                            Message::Mcp(McpMessage::McpVaultPwPromptCancel),
                        ),
                    ),
                ])
                .align_y(iced::Alignment::Center).boxed(),
            );
        if app.mcp.vault_pw_error {
            info_col = info_col.push(Space::new().height(4).boxed()).push(
                text(crate::i18n::t("mcp_vault_pw_wrong"))
                    .size(11)
                    .color(OryxisColors::t().error).boxed(),
            );
        }
        info_col = info_col.push(Space::new().height(4).boxed()).push(
            text(crate::i18n::t("mcp_vault_pw_plaintext_warning"))
                .size(10)
                .color(OryxisColors::t().text_muted).boxed(),
        );
    } else {
        info_col = info_col
            .push(
                text(crate::i18n::t("mcp_vault_pw_note"))
                    .size(11)
                    .color(OryxisColors::t().warning).boxed(),
            )
            .push(Space::new().height(6).boxed())
            .push(app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Mcp(McpMessage::McpVaultPwPromptOpen)),
                6.0,
                token_action_btn(
                    crate::i18n::t("mcp_vault_pw_include"),
                    OryxisColors::t().accent,
                    Message::Mcp(McpMessage::McpVaultPwPromptOpen),
                ),
            ));
        // Outcome of the last Remove: confirm the scrub, or surface a
        // failure so the user is never told the credential is gone while
        // it lingers on disk.
        match &app.mcp.vault_pw_strip_status {
            Some(Ok(())) => {
                info_col = info_col.push(Space::new().height(6).boxed()).push(
                    text(crate::i18n::t("mcp_vault_pw_removed"))
                        .size(10)
                        .color(OryxisColors::t().text_muted).boxed(),
                );
            }
            Some(Err(e)) => {
                info_col = info_col.push(Space::new().height(6).boxed()).push(
                    text(format!(
                        "{} {e}",
                        crate::i18n::t("mcp_vault_pw_remove_failed")
                    ))
                    .size(10)
                    .color(OryxisColors::t().error).boxed(),
                );
            }
            None => {}
        }
    }

    info_col = info_col
        .push(Space::new().height(12).boxed())
        .push(crate::widgets::dir_row(vec![
            app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Mcp(McpMessage::InstallMcpConfig)),
                6.0,
                install_btn.boxed(),
            ),
            Space::new().width(8).boxed(),
            app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Mcp(McpMessage::CopyMcpConfig)),
                6.0,
                copy_btn.boxed(),
            ),
            Space::new().width(8).boxed(),
            app.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Mcp(McpMessage::HideMcpInfo)),
                6.0,
                close_btn.boxed(),
            ),
        ]).boxed());

    container(info_col)
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), color: OryxisColors::t().accent, width: 1.0 },
            ..Default::default()
        })
        .boxed()
}
