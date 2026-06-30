//! Settings -> Security & Privacy section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_security(&self) -> Element<'_, Message> {
        // The switch reflects either a committed password or an open
        // set-password form, so toggling it before a password exists
        // visibly moves the control (and reveals / hides the form).
        let password_toggle = toggle_row(
            crate::i18n::t("vault_password"),
            self.vault_ui.has_user_password || self.vault_ui.show_password_form,
            Message::ToggleVaultPassword,
        );

        let password_section: Element<'_, Message> = if !self.vault_ui.has_user_password {
            // No master password yet. Always lead with a highlighted
            // callout explaining why one matters; reveal the actual
            // input form only once the user flips the switch on.
            let importance = container(
                dir_row(vec![
                    iced_fonts::lucide::shield()
                        .size(20)
                        .color(OryxisColors::t().accent)
                        .into(),
                    Space::new().width(12).into(),
                    column![
                        text(t("vault_importance_title"))
                            .size(13)
                            .font(iced::Font {
                                weight: iced::font::Weight::Semibold,
                                ..iced::Font::DEFAULT
                            })
                            .color(OryxisColors::t().text_primary),
                        Space::new().height(4),
                        text(t("vault_importance_desc"))
                            .size(11)
                            .color(OryxisColors::t().text_secondary),
                    ]
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .into(),
                ])
                .align_y(iced::Alignment::Start),
            )
            .padding(14)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color { a: 0.10, ..OryxisColors::t().accent })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: Color { a: 0.4, ..OryxisColors::t().accent },
                    width: 1.0,
                },
                ..Default::default()
            });

            if !self.vault_ui.show_password_form {
                // Switch is off: callout only, no input fields.
                column![Space::new().height(8), importance].into()
            } else {
            // Show password input to enable
            let input = container(crate::widgets::password_input_with_eye(
                t("new_master_password_placeholder"),
                &self.vault_ui.new_password,
                Message::VaultNewPasswordChanged,
                Some(Message::SetVaultPassword),
                self.revealed_secrets
                    .contains(&crate::state::SecretField::VaultNewPassword),
                Message::ToggleSecretVisibility(
                    crate::state::SecretField::VaultNewPassword,
                ),
                10.0,
            ))
            .width(300);
            // Second hidden entry: both are masked, so a typo in
            // the first would otherwise only surface at the next
            // unlock, when the only recovery is to destroy the
            // vault. Require them to match before accepting.
            let confirm = container(crate::widgets::password_input_with_eye(
                t("confirm_master_password_placeholder"),
                &self.vault_ui.confirm_password,
                Message::VaultConfirmPasswordChanged,
                Some(Message::SetVaultPassword),
                self.revealed_secrets
                    .contains(&crate::state::SecretField::VaultConfirmPassword),
                Message::ToggleSecretVisibility(
                    crate::state::SecretField::VaultConfirmPassword,
                ),
                10.0,
            ))
            .width(300);
            let btn = styled_button(crate::i18n::t("set_password"), Message::SetVaultPassword, OryxisColors::t().accent);
            let error: Element<'_, Message> = if let Some(err) = &self.vault_ui.password_error {
                text(err.clone()).size(12).color(OryxisColors::t().error).into()
            } else {
                Space::new().height(0).into()
            };
            column![
                Space::new().height(8),
                importance,
                Space::new().height(12),
                text(t("vault_set_password_desc"))
                    .size(11).color(OryxisColors::t().text_muted),
                Space::new().height(8),
                input,
                Space::new().height(8),
                confirm,
                Space::new().height(8),
                btn,
                error,
            ].into()
            }
        } else {
            let note: Element<'_, Message> = text(t("vault_protected_note"))
                .size(11).color(OryxisColors::t().text_muted).into();
            let error: Element<'_, Message> = if let Some(err) = &self.vault_ui.password_error {
                text(err.clone()).size(12).color(OryxisColors::t().error).into()
            } else {
                Space::new().height(0).into()
            };
            // Explicit Remove button: toggling the header switch off
            // also removes the password, but that's not discoverable;
            // an outright button makes the destructive-but-reversible
            // action obvious. Reuses the same handler as the toggle.
            let remove_btn = styled_button(
                crate::i18n::t("remove_password"),
                Message::ToggleVaultPassword,
                OryxisColors::t().error,
            );
            column![Space::new().height(4), note, Space::new().height(8), remove_btn, error].into()
        };

        // Lock Vault only makes sense once a master password is
        // set; without one, locking has nothing to protect and
        // the unlock screen would have no way to re-enter (the
        // vault re-opens itself with an empty key). Show the
        // button when a password exists; otherwise replace with
        // a muted note telling the user how to enable locking.
        let lock_btn: Element<'_, Message> = if self.vault_ui.has_user_password {
            button(
                container(
                    dir_row(vec![
                        iced_fonts::lucide::lock().size(14).color(OryxisColors::t().warning).into(),
                        Space::new().width(10).into(),
                        text(crate::i18n::t("lock_vault")).size(13).color(OryxisColors::t().warning).into(),
                    ]).align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 }),
            )
            .on_press(Message::LockVault)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Color { a: 0.15, ..OryxisColors::t().warning },
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(8.0), color: OryxisColors::t().warning, width: 1.0 },
                    ..Default::default()
                }
            })
            .into()
        } else {
            text(crate::i18n::t("lock_vault_requires_password"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .into()
        };

        // MCP Server moved to its own Settings sidebar entry
        // in v0.7 (see `view_settings_mcp`). Keeping it here
        // was crowding the Security panel.

        // Export/Import section
        let export_btn = styled_button(crate::i18n::t("export_vault"), Message::ExportVault, OryxisColors::t().accent);
        let import_btn = styled_button(crate::i18n::t("import_vault"), Message::ImportVault, OryxisColors::t().text_muted);
        // Restore from a remote host. Export-to-SFTP is reached from
        // inside the export dialog (it needs the password first).
        let import_sftp_btn = styled_button(crate::i18n::t("import_from_sftp"), Message::ImportFromSftp, OryxisColors::t().text_muted);

        let mut export_import_section: iced::widget::Column<'_, Message> = column![
            text(crate::i18n::t("export_import")).size(14).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            dir_row(vec![export_btn, Space::new().width(8).into(), import_btn, Space::new().width(8).into(), import_sftp_btn]),
        ];

        // Show export dialog inline
        if self.show_export_dialog {
            let pw_input = container(crate::widgets::password_input_with_eye(
                crate::i18n::t("export_password"),
                &self.export_password,
                Message::ExportPasswordChanged,
                None,
                self.revealed_secrets
                    .contains(&crate::state::SecretField::ExportPassword),
                Message::ToggleSecretVisibility(
                    crate::state::SecretField::ExportPassword,
                ),
                10.0,
            ))
            .width(300);
            // One checkbox per category, all checked by default.
            let mut categories: iced::widget::Column<'_, Message> =
                column![text(crate::i18n::t("export_select_what"))
                    .size(12)
                    .color(OryxisColors::t().text_muted)]
                .spacing(6);
            for cat in oryxis_vault::ExportCategory::ALL {
                categories = categories.push(
                    checkbox(self.export_selection.get(cat))
                        .label(crate::i18n::t(category_label_key(cat)))
                        .on_toggle(move |_| Message::ExportToggleCategory(cat))
                        .size(16)
                        .text_size(13),
                );
            }
            // Private-key material is a sub-option of the Keys
            // category, only meaningful when Keys is being exported.
            let keys_toggle: Element<'_, Message> = if self.export_selection.keys {
                dir_row(vec![
                    text(crate::i18n::t("include_private_keys")).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    button(
                        text(if self.export_include_keys { "ON" } else { "OFF" }).size(12)
                    ).on_press(Message::ExportToggleKeys).style(move |_theme, _status| {
                        button::Style {
                            background: Some(Background::Color(if self.export_include_keys { OryxisColors::t().success } else { OryxisColors::t().bg_hover })),
                            border: Border { radius: Radius::from(4.0), ..Default::default() },
                            text_color: OryxisColors::t().text_primary,
                            ..Default::default()
                        }
                    }).into(),
                ]).align_y(iced::Alignment::Center).into()
            } else {
                Space::new().height(0).into()
            };
            let confirm_btn = styled_button(crate::i18n::t("export_confirm"), Message::ExportConfirm, OryxisColors::t().success);
            let sftp_btn = styled_button(crate::i18n::t("export_to_sftp"), Message::ExportToSftp, OryxisColors::t().accent);
            let cancel_btn = styled_button(crate::i18n::t("cancel"), Message::ExportImportDismiss, OryxisColors::t().text_muted);
            export_import_section = export_import_section
                .push(Space::new().height(12))
                .push(pw_input)
                .push(Space::new().height(10))
                .push(categories)
                .push(Space::new().height(8))
                .push(keys_toggle)
                .push(Space::new().height(8))
                .push(dir_row(vec![confirm_btn, Space::new().width(8).into(), sftp_btn, Space::new().width(8).into(), cancel_btn]));
        }

        // Show import dialog inline
        if self.show_import_dialog {
            let pw_input = container(crate::widgets::password_input_with_eye(
                crate::i18n::t("import_password"),
                &self.import_password,
                Message::ImportPasswordChanged,
                // Enter inspects in phase 1, imports in phase 2.
                Some(if self.import_summary.is_some() {
                    Message::ImportConfirm
                } else {
                    Message::ImportInspect
                }),
                self.revealed_secrets
                    .contains(&crate::state::SecretField::ImportPassword),
                Message::ToggleSecretVisibility(
                    crate::state::SecretField::ImportPassword,
                ),
                10.0,
            ))
            .width(300);
            let cancel_btn = styled_button(crate::i18n::t("cancel"), Message::ExportImportDismiss, OryxisColors::t().text_muted);
            export_import_section = export_import_section
                .push(Space::new().height(12))
                .push(text(crate::i18n::t("import_password_hint")).size(12).color(OryxisColors::t().text_muted))
                .push(Space::new().height(4))
                .push(pw_input);
            if let Some(summary) = &self.import_summary {
                // Phase 2: the file is decrypted, show what it
                // holds. Present categories are interactive
                // checkboxes (with counts); absent ones are
                // greyed so the user sees the full shape.
                let mut categories: iced::widget::Column<'_, Message> =
                    column![text(crate::i18n::t("import_select_what"))
                        .size(12)
                        .color(OryxisColors::t().text_muted)]
                    .spacing(6);
                for cat in oryxis_vault::ExportCategory::ALL {
                    let count = summary.count(cat);
                    let label = crate::i18n::t(category_label_key(cat));
                    if count > 0 {
                        categories = categories.push(
                            checkbox(self.import_selection.get(cat))
                                .label(format!("{label} ({count})"))
                                .on_toggle(move |_| Message::ImportToggleCategory(cat))
                                .size(16)
                                .text_size(13),
                        );
                    } else {
                        categories = categories.push(
                            text(format!("{label} ({})", crate::i18n::t("import_not_in_file")))
                                .size(13)
                                .color(OryxisColors::t().text_muted),
                        );
                    }
                }
                let confirm_btn = styled_button(crate::i18n::t("import_confirm"), Message::ImportConfirm, OryxisColors::t().success);
                export_import_section = export_import_section
                    .push(Space::new().height(10))
                    .push(categories)
                    .push(Space::new().height(8))
                    .push(dir_row(vec![confirm_btn, Space::new().width(8).into(), cancel_btn]));
            } else {
                // Phase 1: enter the password, then inspect.
                let inspect_btn = styled_button(crate::i18n::t("import_inspect"), Message::ImportInspect, OryxisColors::t().accent);
                export_import_section = export_import_section
                    .push(Space::new().height(8))
                    .push(dir_row(vec![inspect_btn, Space::new().width(8).into(), cancel_btn]));
            }
        }

        // SFTP backup-target picker (export to / import from a
        // saved host). Reuses the export/import password + selection
        // state above; here the user only picks the host and path.
        if self.sftp_backup.open {
            let is_import = self.sftp_backup.is_import;
            let host_options: Vec<String> =
                self.connections.iter().map(|c| c.label.clone()).collect();
            let selected_host = self
                .sftp_backup.host
                .and_then(|i| self.connections.get(i))
                .map(|c| c.label.clone());
            let host_lookup: std::collections::HashMap<String, usize> = self
                .connections
                .iter()
                .enumerate()
                .map(|(i, c)| (c.label.clone(), i))
                .collect();
            let host_picker = pick_list(selected_host, host_options, |s: &String| s.clone())
                .on_select(move |label: String| {
                    Message::SftpBackupHostSelected(
                        host_lookup.get(&label).copied().unwrap_or(0),
                    )
                })
                .width(300)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style);
            let path_field = text_input("vault.oryxis", &self.sftp_backup.path)
                .on_input(Message::SftpBackupPathChanged)
                .on_submit(Message::SftpBackupConfirm)
                .width(300)
                .padding(10)
                .style(crate::widgets::rounded_input_style);
            // Restore collects the decrypt password here (export
            // already has it in the dialog above), so both flows ask
            // for the password before the confirm button.
            let import_pw: Option<Element<'_, Message>> = if is_import {
                Some(
                    container(crate::widgets::password_input_with_eye(
                        crate::i18n::t("import_password"),
                        &self.import_password,
                        Message::ImportPasswordChanged,
                        Some(Message::SftpBackupConfirm),
                        self.revealed_secrets
                            .contains(&crate::state::SecretField::ImportPassword),
                        Message::ToggleSecretVisibility(
                            crate::state::SecretField::ImportPassword,
                        ),
                        10.0,
                    ))
                    .width(300)
                    .into(),
                )
            } else {
                None
            };
            let title_key = if is_import { "restore_from_sftp" } else { "backup_to_sftp" };
            let confirm_msg = if self.sftp_backup.busy {
                None
            } else {
                Some(Message::SftpBackupConfirm)
            };
            let confirm_label = if self.sftp_backup.busy {
                crate::i18n::t("sftp_backup_working")
            } else if is_import {
                crate::i18n::t("sftp_backup_restore_confirm")
            } else {
                crate::i18n::t("sftp_backup_confirm")
            };
            let confirm_btn =
                styled_button_opt(confirm_label, confirm_msg, OryxisColors::t().success);
            let cancel_btn = styled_button(
                crate::i18n::t("cancel"),
                Message::SftpBackupCancel,
                OryxisColors::t().text_muted,
            );
            let mut sftp_section: iced::widget::Column<'_, Message> = column![
                text(crate::i18n::t(title_key)).size(13).color(OryxisColors::t().text_primary),
                Space::new().height(2),
                text(crate::i18n::t("sftp_backup_hint")).size(12).color(OryxisColors::t().text_muted),
                Space::new().height(8),
                text(crate::i18n::t("sftp_backup_host")).size(12).color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                host_picker,
                Space::new().height(8),
                text(crate::i18n::t("sftp_backup_remote_path")).size(12).color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                path_field,
            ];
            if let Some(pw) = import_pw {
                sftp_section = sftp_section
                    .push(Space::new().height(10))
                    .push(pw);
            }
            sftp_section = sftp_section
                .push(Space::new().height(10))
                .push(dir_row(vec![confirm_btn, Space::new().width(8).into(), cancel_btn]));
            if let Some(status) = &self.sftp_backup.status {
                let (msg, color) = match status {
                    Ok(m) => (m.clone(), OryxisColors::t().success),
                    Err(e) => (e.clone(), OryxisColors::t().error),
                };
                sftp_section = sftp_section
                    .push(Space::new().height(8))
                    .push(text(msg).size(12).color(color));
            }
            export_import_section = export_import_section
                .push(Space::new().height(14))
                .push(sftp_section);
        }

        // Status messages
        if let Some(status) = &self.export_status {
            let (msg, color) = match status {
                Ok(m) => (m.as_str(), OryxisColors::t().success),
                Err(m) => (m.as_str(), OryxisColors::t().error),
            };
            export_import_section = export_import_section
                .push(Space::new().height(8))
                .push(text(msg).size(12).color(color));
        }
        if let Some(status) = &self.import_status {
            let (msg, color) = match status {
                Ok(m) => (m.as_str(), OryxisColors::t().success),
                Err(m) => (m.as_str(), OryxisColors::t().error),
            };
            export_import_section = export_import_section
                .push(Space::new().height(8))
                .push(text(msg).size(12).color(color));
        }

        // SSH config import, separate card, sits below the
        // vault export/import. One-shot batch importer; no
        // preview yet.
        let ssh_config_btn = styled_button(
            t("import_ssh_config_btn"),
            Message::ImportSshConfig,
            OryxisColors::t().accent,
        );
        let mut ssh_config_section: iced::widget::Column<'_, Message> = column![
            text(t("ssh_config_import"))
                .size(14)
                .color(OryxisColors::t().text_muted),
            Space::new().height(4),
            text(t("ssh_config_import_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            ssh_config_btn,
        ];
        if let Some(status) = &self.ssh_config_import_status {
            let (msg, color) = match status {
                Ok(m) => (m.as_str(), OryxisColors::t().success),
                Err(m) => (m.as_str(), OryxisColors::t().error),
            };
            ssh_config_section = ssh_config_section
                .push(Space::new().height(8))
                .push(text(msg).size(12).color(color));
        }

        // Privacy & logging: session recordings, connection
        // history and the retention window. Moved here from the
        // Terminal section, recordings are scrubbed for secrets
        // and sealed at rest, so they belong with the vault.
        let privacy_mode_section = panel_section(column![
            toggle_row(
                crate::i18n::t("privacy_mode_label"),
                self.setting_privacy_mode,
                Message::TogglePrivacyMode,
            ),
            Space::new().height(4),
            text(crate::i18n::t("privacy_mode_desc"))
                .size(11).color(OryxisColors::t().text_muted),
        ]);

        let session_logging_enabled = self.setting_session_logging;
        let session_logging_section = panel_section(column![
            toggle_row(
                crate::i18n::t("session_logging"),
                session_logging_enabled,
                Message::SettingToggleSessionLogging,
            ),
            Space::new().height(4),
            text(t("setting_session_logging_desc"))
                .size(11).color(OryxisColors::t().text_muted),
        ]);

        let connection_history_enabled = self.setting_connection_history;
        let connection_history_section = panel_section(column![
            toggle_row(
                crate::i18n::t("connection_history"),
                connection_history_enabled,
                Message::SettingToggleConnectionHistory,
            ),
            Space::new().height(4),
            text(t("setting_connection_history_desc"))
                .size(11).color(OryxisColors::t().text_muted),
        ]);

        // Retention: auto-delete connection events + finished
        // recordings past the picked age. Codes are stable
        // setting values; the mapper localizes per code.
        const RETENTION_CODES: [&str; 7] =
            ["off", "1d", "3d", "7d", "14d", "30d", "90d"];
        let retention_selected = RETENTION_CODES
            .iter()
            .copied()
            .find(|c| *c == self.setting_logs_retention)
            .unwrap_or("off");
        let logs_retention_section = panel_section(column![
            text(crate::i18n::t("log_retention_label"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("setting_log_retention_desc"))
                .size(11).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            pick_list(
                Some(retention_selected),
                &RETENTION_CODES[..],
                |code: &&str| {
                    crate::i18n::t(match *code {
                        "1d" => "log_retention_1d",
                        "3d" => "log_retention_3d",
                        "7d" => "log_retention_7d",
                        "14d" => "log_retention_14d",
                        "30d" => "log_retention_30d",
                        "90d" => "log_retention_90d",
                        _ => "log_retention_off",
                    })
                    .to_string()
                },
            )
            .on_select(Message::LogsRetentionChanged)
            .width(260).padding(10).style(crate::widgets::rounded_pick_list_style),
        ]);

        scrollable(
            container(
                column![
                    panel_section(column![password_toggle]),
                    password_section,
                    Space::new().height(24),
                    lock_btn,
                    Space::new().height(24),
                    privacy_mode_section,
                    Space::new().height(12),
                    session_logging_section,
                    Space::new().height(12),
                    connection_history_section,
                    Space::new().height(12),
                    logs_retention_section,
                    Space::new().height(24),
                    panel_section(export_import_section),
                    Space::new().height(12),
                    panel_section(ssh_config_section),
                    Space::new().height(24),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill)
        .into()
    }
}
