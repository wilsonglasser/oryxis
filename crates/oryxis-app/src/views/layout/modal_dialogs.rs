//! Modal dialog content builders (share, SSH-config import, blocking
//! error, clear-history, kill-port). Split out of
//! views/layout/main_layout.rs; each returns just the dialog `Element`,
//! wrapped by `layer_modals`.

use super::*;
use crate::messages::MonitorMessage;
use iced::widget::{column, row};

impl Oryxis {
    /// Content for the share / group-export dialog (password, optional
    /// per-folder checklist, include-keys toggle, Share + Cancel).
    pub(crate) fn build_share_dialog(&self) -> Element<'_, Message> {
        let share_include_keys = self.share.include_keys;
        // Keyboard rows in visual order: the password field (Enter
        // focuses it), its reveal eye, the per-folder checkboxes, the
        // keys toggle, then Share as the default (Enter shares) and
        // Cancel. The field row is recorded before the widget is
        // built (its eye slot records during construction).
        self.modal_nav_reset();
        let pw_idx = self.modal_nav_record(crate::keynav::RowAction::input(
            iced::widget::Id::new("share-password"),
        ));
        let pw_input = self.modal_nav_ring_at(
            pw_idx,
            10.0,
            false,
            container(crate::widgets::password_input_with_eye_nav(
                crate::i18n::t("export_password"),
                &self.share.password,
                |v| Message::Share(ShareMessage::SharePasswordChanged(v.into())),
                None,
                self.revealed_secrets
                    .contains(&crate::state::SecretField::SharePassword),
                Message::Settings(SettingsMessage::ToggleSecretVisibility(
                    crate::state::SecretField::SharePassword,
                )),
                10.0,
                Some(iced::widget::Id::new("share-password")),
                |eye| self.modal_nav_slot(
                    crate::keynav::RowAction::activate(
                        Message::Settings(SettingsMessage::ToggleSecretVisibility(
                            crate::state::SecretField::SharePassword,
                        )),
                    ),
                    6.0,
                    false,
                    eye,
                ),
            ))
            .width(280)
            .into(),
        );
        // Group-mode export: a per-folder include/exclude checklist
        // sits between the password and the keys toggle. A single-host
        // share skips it (no folder choice to make).
        let group_picker: Element<'_, Message> = if self.share.group_mode {
            let mut list = column![text(crate::i18n::t("export_groups"))
                .size(12)
                .color(OryxisColors::t().text_muted)]
            .spacing(6);
            for g in &self.groups {
                let id = g.id;
                list = list.push(self.modal_nav_slot(
                    crate::keynav::RowAction::activate(Message::Share(ShareMessage::ShareToggleGroup(id))),
                    4.0,
                    false,
                    iced::widget::checkbox(self.share.groups.contains(&id))
                        .label(g.label.as_str())
                        .on_toggle(move |_| Message::Share(ShareMessage::ShareToggleGroup(id)))
                        .size(16)
                        .text_size(13)
                        .into(),
                ));
            }
            list = list.push(self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Share(ShareMessage::ShareToggleUngrouped)),
                4.0,
                false,
                iced::widget::checkbox(self.share.include_ungrouped)
                    .label(crate::i18n::t("export_ungrouped"))
                    .on_toggle(|_| Message::Share(ShareMessage::ShareToggleUngrouped))
                    .size(16)
                    .text_size(13)
                    .into(),
            ));
            column![
                iced::widget::container(
                    iced::widget::scrollable(list)
                        .height(Length::Fixed(160.0))
                )
                .width(280),
                Space::new().height(8),
            ]
            .into()
        } else {
            Space::new().into()
        };
        let dialog_title = if self.share.group_mode {
            crate::i18n::t("export_hosts")
        } else {
            crate::i18n::t("share")
        };
        let dialog_content = container(
            column![
                text(dialog_title).size(16).color(OryxisColors::t().text_primary),
                Space::new().height(12),
                pw_input,
                Space::new().height(8),
                group_picker,
                row![
                    text(crate::i18n::t("include_private_keys")).size(13).color(OryxisColors::t().text_secondary),
                    Space::new().width(Length::Fill),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Share(ShareMessage::ShareToggleKeys)),
                        4.0,
                        false,
                        button(
                            text(if share_include_keys { "ON" } else { "OFF" }).size(12)
                        ).on_press(Message::Share(ShareMessage::ShareToggleKeys)).style(move |_theme, _status| {
                            button::Style {
                                background: Some(Background::Color(if share_include_keys { OryxisColors::t().success } else { OryxisColors::t().bg_hover })),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: OryxisColors::t().text_primary,
                                ..Default::default()
                            }
                        }).into(),
                    ),
                ].align_y(iced::Alignment::Center).width(280),
                Space::new().height(12),
                row![
                    self.modal_nav_slot_default(
                        crate::keynav::RowAction::activate(Message::Share(ShareMessage::ShareConfirm)),
                        6.0,
                        true,
                        styled_button(crate::i18n::t("share"), Message::Share(ShareMessage::ShareConfirm), OryxisColors::t().accent),
                    ),
                    Space::new().width(8),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Share(ShareMessage::ShareDismiss)),
                        6.0,
                        false,
                        styled_button(crate::i18n::t("cancel"), Message::Share(ShareMessage::ShareDismiss), OryxisColors::t().text_muted),
                    ),
                ],
                if let Some(status) = &self.share.status {
                    let (msg, color) = match status {
                        Ok(m) => (m.as_str(), OryxisColors::t().success),
                        Err(m) => (m.as_str(), OryxisColors::t().error),
                    };
                    Element::from(column![Space::new().height(8), text(msg).size(12).color(color)])
                } else {
                    Element::from(Space::new())
                },
            ]
            .padding(24),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        dialog_content.into()
    }

    /// The ssh-agent per-signature confirm card: a tool is asking the
    /// agent to sign with a vault key. Shows the key label, its SHA-256
    /// fingerprint, the requesting process (when known), a "remember
    /// this key this session" checkbox, and Deny / Allow. Deny is the
    /// default (Enter confirms the ringed default = Allow only when the
    /// user Tabs to it; the safe default is the initial ring on Deny,
    /// handled by the keynav router).
    pub(crate) fn build_agent_confirm_dialog(
        &self,
        card: &crate::state::AgentConfirmCard,
    ) -> Element<'_, Message> {
        self.modal_nav_reset();
        let always = self.agent_confirm_always();

        let mut info = column![
            text(crate::i18n::t("agent_confirm_title"))
                .size(16)
                .color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(crate::i18n::t("agent_confirm_body"))
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(12),
            crate::widgets::panel_field(
                crate::i18n::t("agent_confirm_key"),
                text(card.key_comment.clone())
                    .size(13)
                    .color(OryxisColors::t().text_primary)
                    .into(),
            ),
            Space::new().height(8),
            crate::widgets::panel_field(
                crate::i18n::t("key_fingerprint"),
                text(card.key_fingerprint.clone())
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
            ),
        ];
        if let Some(peer) = &card.peer {
            info = info.push(Space::new().height(8));
            info = info.push(crate::widgets::panel_field(
                crate::i18n::t("agent_confirm_process"),
                text(peer.clone())
                    .size(12)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
            ));
        }

        let always_row = self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Agent(AgentMessage::AgentConfirmToggleAlways)),
            4.0,
            false,
            iced::widget::checkbox(always)
                .label(crate::i18n::t("agent_confirm_always_session"))
                .on_toggle(|_| Message::Agent(AgentMessage::AgentConfirmToggleAlways))
                .size(16)
                .text_size(13)
                .into(),
        );

        let buttons = crate::widgets::dir_row(vec![
            self.modal_nav_slot_default(
                crate::keynav::RowAction::activate(Message::Agent(AgentMessage::AgentConfirmDecision {
                    allow: false,
                    always: false,
                })),
                6.0,
                true,
                styled_button(
                    crate::i18n::t("agent_confirm_deny"),
                    Message::Agent(AgentMessage::AgentConfirmDecision { allow: false, always: false }),
                    OryxisColors::t().error,
                ),
            ),
            Space::new().width(8).into(),
            self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Agent(AgentMessage::AgentConfirmDecision {
                    allow: true,
                    always,
                })),
                6.0,
                false,
                styled_button(
                    crate::i18n::t("agent_confirm_allow"),
                    Message::Agent(AgentMessage::AgentConfirmDecision { allow: true, always }),
                    OryxisColors::t().accent,
                ),
            ),
        ]);

        let dialog = container(
            column![
                info,
                Space::new().height(12),
                always_row,
                Space::new().height(16),
                buttons,
            ]
            .padding(24)
            .width(400),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }

    /// "A highlight rule wants to run a snippet on this session" (C6).
    ///
    /// A security prompt, and it is worded like one: what asked for it
    /// is REMOTE OUTPUT, so the dialog shows the line that matched and
    /// the snippet's actual text, not just their names. The default
    /// (and Esc, and a click outside) is Don't send.
    pub(crate) fn build_trigger_confirm_dialog<'a>(
        &'a self,
        card: &'a crate::dispatch_terminal::TriggerConfirmCard,
    ) -> Element<'a, Message> {
        self.modal_nav_reset();
        let rule = if card.rule_name.trim().is_empty() {
            crate::i18n::t("highlight_rules").to_string()
        } else {
            card.rule_name.clone()
        };

        let info = column![
            text(crate::i18n::t("trigger_confirm_title"))
                .size(16)
                .color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(crate::i18n::t("trigger_confirm_body"))
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(12),
            crate::widgets::panel_field(
                crate::i18n::t("trigger_confirm_rule"),
                text(rule).size(13).color(OryxisColors::t().text_primary).into(),
            ),
            Space::new().height(8),
            crate::widgets::panel_field(
                crate::i18n::t("trigger_confirm_line"),
                text(card.line.clone())
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
            ),
            Space::new().height(8),
            crate::widgets::panel_field(
                &card.snippet_label,
                text(card.snippet_body.clone())
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .color(OryxisColors::t().text_primary)
                    .into(),
            ),
        ];

        let buttons = crate::widgets::dir_row(vec![
            self.modal_nav_slot_default(
                crate::keynav::RowAction::activate(Message::Terminal(
                    TerminalMessage::TriggerConfirmDecision(false),
                )),
                6.0,
                true,
                styled_button(
                    crate::i18n::t("trigger_confirm_deny"),
                    Message::Terminal(TerminalMessage::TriggerConfirmDecision(false)),
                    OryxisColors::t().bg_hover,
                ),
            ),
            Space::new().width(8).into(),
            self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Terminal(
                    TerminalMessage::TriggerConfirmDecision(true),
                )),
                6.0,
                false,
                styled_button(
                    crate::i18n::t("trigger_confirm_allow"),
                    Message::Terminal(TerminalMessage::TriggerConfirmDecision(true)),
                    OryxisColors::t().accent,
                ),
            ),
        ]);

        container(
            column![info, Space::new().height(16), buttons]
                .padding(24)
                .width(440),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
    }

    /// "Open this link?" for a Ctrl+click in a remote pane.
    ///
    /// A security prompt of the same family as the trigger one above:
    /// the text was printed by a REMOTE host, and an OSC 8 link's label
    /// can say anything at all about where it points, so what the dialog
    /// shows is the TARGET (elided in the middle, both ends intact) and
    /// the host that offered it. Cancel is the default; Esc and a click
    /// outside open nothing.
    ///
    /// When the link carries a loopback callback, the dialog also says
    /// which port is about to be forwarded and to where. That line is
    /// the consent for the tunnel: it opens a listener on this machine,
    /// which is not something to do silently on a click.
    pub(crate) fn build_link_confirm_dialog<'a>(
        &'a self,
        card: &'a crate::dispatch_terminal::LinkConfirmCard,
    ) -> Element<'a, Message> {
        self.modal_nav_reset();
        let mut info = column![
            text(crate::i18n::t("terminal_link_confirm_title"))
                .size(16)
                .color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(
                crate::i18n::t("terminal_link_confirm_body")
                    .replace("{host}", &card.host_label)
            )
            .size(13)
            .color(OryxisColors::t().text_secondary),
            Space::new().height(12),
            crate::widgets::panel_field(
                crate::i18n::t("terminal_link_confirm_target"),
                text(card.display.clone())
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .color(OryxisColors::t().text_primary)
                    .into(),
            ),
        ];
        if let Some(cb) = &card.callback {
            let (label, body) = if card.tunnelable {
                (
                    crate::i18n::t("terminal_link_confirm_tunnel"),
                    crate::i18n::t("terminal_link_confirm_tunnel_body")
                        .replace("{port}", &cb.port.to_string())
                        .replace("{host}", &card.host_label),
                )
            } else {
                // Nothing to open a channel on (telnet, serial, mosh).
                // Saying so beats a login that fails at its last step
                // for reasons nothing on screen explains.
                (
                    crate::i18n::t("terminal_link_confirm_tunnel"),
                    crate::i18n::t("terminal_link_confirm_no_tunnel")
                        .replace("{port}", &cb.port.to_string()),
                )
            };
            info = info.push(Space::new().height(8)).push(
                crate::widgets::panel_field(
                    label,
                    text(body)
                        .size(11)
                        .color(OryxisColors::t().text_secondary)
                        .into(),
                ),
            );
        }

        let buttons = crate::widgets::dir_row(vec![
            self.modal_nav_slot_default(
                crate::keynav::RowAction::activate(Message::Terminal(
                    TerminalMessage::TerminalLinkDecision(false),
                )),
                6.0,
                true,
                styled_button(
                    crate::i18n::t("cancel"),
                    Message::Terminal(TerminalMessage::TerminalLinkDecision(false)),
                    OryxisColors::t().bg_hover,
                ),
            ),
            Space::new().width(8).into(),
            // Copying is the escape hatch for a link the user wants to
            // look at before following it (VS Code offers the same).
            self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Terminal(
                    TerminalMessage::TerminalLinkCopy,
                )),
                6.0,
                false,
                styled_button(
                    crate::i18n::t("terminal_link_confirm_copy"),
                    Message::Terminal(TerminalMessage::TerminalLinkCopy),
                    OryxisColors::t().bg_hover,
                ),
            ),
            Space::new().width(8).into(),
            self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Terminal(
                    TerminalMessage::TerminalLinkDecision(true),
                )),
                6.0,
                false,
                styled_button(
                    crate::i18n::t("terminal_link_confirm_open"),
                    Message::Terminal(TerminalMessage::TerminalLinkDecision(true)),
                    OryxisColors::t().accent,
                ),
            ),
        ]);

        container(
            column![info, Space::new().height(16), buttons]
                .padding(24)
                .width(460),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
    }

    /// The one-entry Import hub (owner call: ONE standardized import):
    /// names every supported source, and the "Choose file" picker's
    /// result is format-detected from its content, so the user never
    /// has to know which button matches their old client.
    pub(crate) fn build_import_hub_dialog(&self) -> Element<'_, Message> {
        self.modal_nav_reset();
        // Product names, deliberately untranslated; the sentence
        // around them localizes.
        const SOURCES: [&str; 8] = [
            "Oryxis export (.oryxis)",
            "OpenSSH config (~/.ssh/config)",
            "PuTTY / KiTTY sessions (.reg export)",
            "WinSCP sites (WinSCP.ini / .reg export)",
            "mRemoteNG (confCons.xml)",
            "MobaXterm (MobaXterm.ini)",
            "Xshell / SecureCRT / FinalShell (session folder)",
            "Termius export / CSV (any host table)",
        ];
        let mut sources = column![].spacing(4);
        for s in SOURCES {
            sources = sources.push(
                dir_row(vec![
                    text("•").size(12).color(OryxisColors::t().text_muted).into(),
                    Space::new().width(8).into(),
                    text(s).size(13).color(OryxisColors::t().text_primary).into(),
                ])
                .align_y(iced::Alignment::Center),
            );
        }
        let error_line: Element<'_, Message> = match &self.import_hub_error {
            Some(e) => column![
                Space::new().height(10),
                text(e.clone()).size(12).color(OryxisColors::t().error),
            ]
            .into(),
            None => Space::new().into(),
        };
        // A protected mRemoteNG file is parked here while its password
        // is asked for; Enter in the field retries directly.
        let password_row: Element<'_, Message> = if self.import_hub_pending.is_some() {
            let field = iced::widget::text_input(
                crate::i18n::t("password"),
                &self.import_hub_password,
            )
            .id(iced::widget::Id::new("import-hub-password"))
            .secure(true)
            .on_input(|v| {
                Message::Share(ShareMessage::ImportHubPasswordChanged(v.into()))
            })
            .on_submit(Message::Share(ShareMessage::ImportHubUnlock))
            .padding(10)
            .style(crate::widgets::rounded_input_style);
            self.modal_nav_record(crate::keynav::RowAction::input(
                iced::widget::Id::new("import-hub-password"),
            ));
            column![
                Space::new().height(12),
                text(crate::i18n::t("import_hub_protected"))
                    .size(12)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(6),
                row![
                    field,
                    Space::new().width(8),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Share(
                            ShareMessage::ImportHubUnlock,
                        )),
                        6.0,
                        false,
                        styled_button(
                            crate::i18n::t("import_hub_unlock"),
                            Message::Share(ShareMessage::ImportHubUnlock),
                            OryxisColors::t().accent,
                        ),
                    ),
                ]
                .align_y(iced::Alignment::Center),
            ]
            .into()
        } else {
            Space::new().into()
        };
        container(
            column![
                text(crate::i18n::t("import_hub_title"))
                    .size(16)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(6),
                text(crate::i18n::t("import_hub_desc"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(12),
                sources,
                password_row,
                error_line,
                Space::new().height(16),
                row![
                    self.modal_nav_slot_default(
                        crate::keynav::RowAction::activate(Message::Share(
                            ShareMessage::ImportHubPick,
                        )),
                        6.0,
                        true,
                        styled_button(
                            crate::i18n::t("import_hub_choose"),
                            Message::Share(ShareMessage::ImportHubPick),
                            OryxisColors::t().accent,
                        ),
                    ),
                    Space::new().width(8),
                    // The per-session formats keep one file per host,
                    // so a folder is the unit those users think in.
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Share(
                            ShareMessage::ImportHubPickFolder,
                        )),
                        6.0,
                        false,
                        styled_button(
                            crate::i18n::t("import_hub_choose_folder"),
                            Message::Share(ShareMessage::ImportHubPickFolder),
                            OryxisColors::t().bg_hover,
                        ),
                    ),
                    Space::new().width(8),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Share(
                            ShareMessage::ImportHubDismiss,
                        )),
                        6.0,
                        false,
                        styled_button(
                            crate::i18n::t("cancel"),
                            Message::Share(ShareMessage::ImportHubDismiss),
                            OryxisColors::t().text_muted,
                        ),
                    ),
                ],
            ]
            .padding(24)
            .width(460),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
    }

    /// Content for the import preview (per-host checklist with
    /// select/deselect all, then Import + Cancel). Serves both the
    /// ssh_config flow and the third-party importers: the row data
    /// resolves from whichever batch is pending.
    pub(crate) fn build_ssh_import_dialog(&self) -> Element<'_, Message> {
        // (label, detail) per row, source-agnostic. Direct batches
        // are already Connections; ssh_config rows derive from the
        // parsed block.
        let direct = self.ssh_import_direct.as_ref();
        let rows: Vec<(String, String)> = match direct {
            Some(direct) => direct
                .hosts
                .iter()
                .map(|h| {
                    let c = &h.conn;
                    let mut detail = c.hostname.clone();
                    if let Some(u) = &c.username {
                        detail = format!("{u}@{detail}");
                    }
                    if c.port > 0 {
                        detail = format!("{detail}:{}", c.port);
                    }
                    // A ProxyCommand runs an arbitrary shell command at
                    // connect time. Some source formats (PuTTY / WinSCP)
                    // carry one, and a user tricked into importing a
                    // hostile file must be able to SEE it before the
                    // first connect spawns it, not discover it running.
                    if matches!(
                        c.proxy.as_ref().map(|p| &p.proxy_type),
                        Some(oryxis_core::models::connection::ProxyType::Command(_))
                    ) {
                        detail.push_str("  · ");
                        detail.push_str(crate::i18n::t("import_has_proxy_command"));
                    }
                    (c.label.clone(), detail)
                })
                .collect(),
            None => self
                .ssh_import_hosts
                .iter()
                .map(|host| {
                    // "user@hostname:port", falling back to the alias
                    // when HostName is omitted (OpenSSH treats the
                    // alias as host).
                    let mut detail = host
                        .hostname
                        .clone()
                        .unwrap_or_else(|| host.alias.clone());
                    if let Some(u) = &host.user {
                        detail = format!("{u}@{detail}");
                    }
                    if let Some(p) = host.port {
                        detail = format!("{detail}:{p}");
                    }
                    (host.alias.clone(), detail)
                })
                .collect(),
        };
        let title_key = direct.map_or("import_ssh_config_btn", |d| d.source_key);
        let ssh_total = rows.len();
        let ssh_selected =
            self.ssh_import_selected.iter().filter(|s| **s).count();
        // Keyboard rows in visual order: select/deselect all, the
        // per-host checkboxes, then Import (default) + Cancel.
        // Pre-build the top buttons so the recording order matches
        // the screen (the checkbox list is constructed next).
        self.modal_nav_reset();
        let select_all_btn = self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Share(ShareMessage::SshImportSelectAll(true))),
            6.0,
            false,
            styled_button(
                crate::i18n::t("select_all"),
                Message::Share(ShareMessage::SshImportSelectAll(true)),
                OryxisColors::t().accent,
            ),
        );
        let deselect_all_btn = self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Share(ShareMessage::SshImportSelectAll(false))),
            6.0,
            false,
            styled_button(
                crate::i18n::t("deselect_all"),
                Message::Share(ShareMessage::SshImportSelectAll(false)),
                OryxisColors::t().text_muted,
            ),
        );
        let mut list = column![].spacing(4);
        for (i, (name, detail)) in rows.iter().enumerate() {
            let checked =
                self.ssh_import_selected.get(i).copied().unwrap_or(false);
            let exists =
                self.ssh_import_existing.get(i).copied().unwrap_or(false);
            let mut label = format!("{name}  ({detail})");
            if exists {
                label.push_str("  · ");
                label.push_str(crate::i18n::t("ssh_import_exists"));
            }
            list = list.push(self.modal_nav_slot(
                crate::keynav::RowAction::activate(Message::Share(ShareMessage::SshImportToggle(i))),
                4.0,
                false,
                iced::widget::checkbox(checked)
                    .label(label)
                    .on_toggle(move |_| Message::Share(ShareMessage::SshImportToggle(i)))
                    .size(16)
                    .text_size(13)
                    .into(),
            ));
        }
        // Sessions the parser could not map: named, muted, honest.
        let skipped_line: Element<'_, Message> = match direct.filter(|d| !d.skipped.is_empty())
        {
            Some(d) => column![
                Space::new().height(6),
                text(format!(
                    "{} {}",
                    crate::i18n::t("import_skipped"),
                    d.skipped.join(", ")
                ))
                .size(11)
                .color(OryxisColors::t().warning),
            ]
            .into(),
            None => Space::new().into(),
        };
        let dialog_content = container(
            column![
                text(crate::i18n::t(title_key))
                    .size(16)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(format!(
                    "{} ({}/{})",
                    crate::i18n::t("ssh_import_select"),
                    ssh_selected,
                    ssh_total,
                ))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
                skipped_line,
                Space::new().height(8),
                row![select_all_btn, Space::new().width(8), deselect_all_btn],
                Space::new().height(8),
                container(
                    iced::widget::scrollable(list)
                        .height(Length::Fixed(280.0))
                )
                .width(440),
                Space::new().height(12),
                row![
                    self.modal_nav_slot_default(
                        crate::keynav::RowAction::activate(Message::Share(ShareMessage::SshImportConfirm)),
                        6.0,
                        true,
                        styled_button(crate::i18n::t("import_from_file"), Message::Share(ShareMessage::SshImportConfirm), OryxisColors::t().success),
                    ),
                    Space::new().width(8),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Share(ShareMessage::SshImportDismiss)),
                        6.0,
                        false,
                        styled_button(crate::i18n::t("cancel"), Message::Share(ShareMessage::SshImportDismiss), OryxisColors::t().text_muted),
                    ),
                ],
            ]
            .padding(24),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        dialog_content.into()
    }

    /// Content for the generic blocking error dialog (title, selectable
    /// body, Close plus an optional link / recovery action). The dialog
    /// value is cloned by the caller, matching the original branch.
    pub(crate) fn build_error_dialog(&self, dialog: crate::state::ErrorDialog) -> Element<'_, Message> {
        // Keyboard: Close and the optional link/action are recorded
        // rows; the action button (when present) is the default so
        // a bare Enter runs it, matching the desktop convention for
        // a dialog that IS the confirmation step.
        self.modal_nav_reset();
        use crate::keynav::RowAction;
        // A dialog carrying a DANGEROUS action is a confirmation, not a
        // report, so its negative button says Cancel. "Close" beside
        // "Close group" (or "Close" beside "Remove") reads as a second
        // way to do the thing, which is the opposite of what it does.
        let dismiss_key = match &dialog.action {
            Some(a) if a.danger => "cancel",
            _ => "close",
        };
        let mut buttons = iced::widget::row![self.modal_nav_slot(
            RowAction::activate(Message::ErrorDialogDismiss),
            6.0,
            false,
            styled_button(
                crate::i18n::t(dismiss_key),
                Message::ErrorDialogDismiss,
                OryxisColors::t().text_muted,
            ),
        )]
        .spacing(8);
        if let Some(link) = dialog.link.clone() {
            let url = link.url.clone();
            buttons = buttons.push(self.modal_nav_slot(
                RowAction::activate(Message::OpenUrl(url)),
                6.0,
                false,
                open_link_button(link.label, link.url),
            ));
        }
        if let Some(action) = dialog.action.clone() {
            // Recovery action, accent-styled like the link button;
            // dispatching goes through ErrorDialogRunAction so the
            // dialog also dismisses itself.
            buttons = buttons.push(self.modal_nav_slot_default(
                RowAction::activate(Message::ErrorDialogRunAction),
                6.0,
                true,
                dialog_action_button(action.label, action.danger),
            ));
        }

        // Body uses Rich text with `.selectable(true)` so the user
        // can highlight and copy the failure message (key when the
        // dialog explains how to install a missing dependency or
        // includes a path / command to run).
        let body_span: iced::widget::text::Span<'_, ()> =
            iced::widget::text::Span::new(dialog.body.clone())
                .color(OryxisColors::t().text_secondary);
        let dialog_body = iced::widget::text::Rich::<'_, (), Message>::with_spans(
            [body_span],
        )
        .size(13)
        .selectable(true);

        let dialog_content = container(
            column![
                text(dialog.title.clone())
                    .size(16)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(12),
                dialog_body,
                Space::new().height(20),
                buttons,
            ]
            .padding(24),
        )
        .width(Length::Shrink.max(520.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
        dialog_content.into()
    }

    /// Content for the Logs "Clear all" confirmation (entry count, then
    /// Cancel + Clear all).
    pub(crate) fn build_clear_history_dialog(&self) -> Element<'_, Message> {
        let total = self.logs_total + self.session_logs_total;
        let dialog = container(
            column![
                text(crate::i18n::t("clear_history_title"))
                    .size(16)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(6),
                text(crate::i18n::t("clear_history_confirm"))
                    .size(13)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                text(format!("{} {}", total, crate::i18n::t("entries")))
                    .size(13)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(16),
                crate::widgets::dir_row(vec![
                    // Keyboard: Clear all is the default (Enter
                    // confirms); arrows/Tab reach Cancel; Esc
                    // cancels via close_topmost_modal.
                    {
                        self.modal_nav_reset();
                        self.modal_nav_slot(
                            crate::keynav::RowAction::activate(
                                Message::History(HistoryMessage::CancelClearHistory),
                            ),
                            6.0,
                            false,
                            styled_button(
                                crate::i18n::t("cancel"),
                                Message::History(HistoryMessage::CancelClearHistory),
                                OryxisColors::t().text_muted,
                            ),
                        )
                    },
                    Space::new().width(8).into(),
                    self.modal_nav_slot_default(
                        crate::keynav::RowAction::activate(Message::History(HistoryMessage::ClearLogs)),
                        6.0,
                        true,
                        styled_button(
                            crate::i18n::t("clear_all"),
                            Message::History(HistoryMessage::ClearLogs),
                            OryxisColors::t().error,
                        ),
                    ),
                ])
                .align_y(iced::Alignment::Center),
            ]
            .padding(24)
            .width(360),
        )
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }

    /// "Lock the vault?" confirmation. Manual Lock Vault tears down
    /// every live SSH session and tab (plus RDP/VNC tunnels), so the
    /// button asks first, with the actual impact: the session labels
    /// that are open right now, capped with an overflow line.
    ///
    /// Layout follows `build_monitor_kill_dialog` (its destructive-remote
    /// sibling): a warning badge, a centered header block, and a centered
    /// action row, so the buttons never float at the physical left of a
    /// wide dialog. Keyboard: Cancel is the default-ringed row, so a stray
    /// Enter can never sever connections; arrows / Tab reach Lock; Esc
    /// and the backdrop cancel via `close_topmost_modal` / the overlay
    /// dismiss.
    pub(crate) fn build_lock_confirm_dialog(&self) -> Element<'_, Message> {
        use crate::keynav::RowAction;
        let c = OryxisColors::t();
        self.modal_nav_reset();

        // What the lock severs, in the user's own labels: live terminal
        // panes (SSH / Telnet / serial), standalone SFTP tabs, and the
        // RDP/VNC forwards (each Arc drop cancels its -L). Rows are
        // built eagerly (the icon glyph is not `Clone`), then capped
        // below; the pre-cap count also picks between the list and the
        // "nothing open" note.
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        for tab in &self.tabs {
            for pane in tab.pane_grid.panes.values() {
                if pane.session.as_ref().is_some_and(|s| s.is_alive()) {
                    rows.push(lock_impact_row(iced_fonts::lucide::terminal(), pane.label.clone()));
                }
            }
        }
        for (idx, tab) in self.sftp_tabs.iter().enumerate() {
            // A tab counts only while it has a mounted session: a
            // disconnected SFTP tab has nothing to sever. The ACTIVE
            // tab's live state rides the swap-on-focus buffer
            // (`self.sftp`); its parked slot is a taken default, so
            // resolve each tab against where its state actually lives.
            let st: &crate::state::SftpState = if self.active_sftp == Some(idx) {
                &self.sftp
            } else {
                &tab.state
            };
            if st.left.session.is_some() || st.right.session.is_some() {
                rows.push(lock_impact_row(iced_fonts::lucide::folder(), tab.label.clone()));
            }
        }
        let tunnels = self.remote_desktop_forwards.len();
        if tunnels > 0 {
            rows.push(lock_impact_row(
                iced_fonts::lucide::monitor(),
                format!(
                    "{} ×{}",
                    crate::i18n::t("lock_vault_confirm_tunnels"),
                    tunnels
                ),
            ));
        }
        let total = rows.len();

        // Tinted warning badge, the same anchor the destructive confirms
        // (folder delete, kill-port) use.
        let badge = container(iced_fonts::lucide::triangle_alert().size(22).color(c.error))
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(48.0))
            .center_x(Length::Fixed(48.0))
            .center_y(Length::Fixed(48.0))
            .style(|_| container::Style {
                background: Some(Background::Color(Color { a: 0.12, ..c.error })),
                border: Border { radius: Radius::from(24.0), ..Default::default() },
                ..Default::default()
            });

        // Impact block: the actual session labels when anything is open
        // (capped at four rows with an overflow line), else a quiet
        // "nothing open" note. Rows are informational, not keyboard rows.
        let impact: Element<'_, Message> = if total == 0 {
            text(crate::i18n::t("lock_vault_confirm_count").replacen("{count}", "0", 1))
                .size(12)
                .color(c.text_muted)
                .into()
        } else {
            const MAX_ROWS: usize = 4;
            let extra = total.saturating_sub(MAX_ROWS);
            rows.truncate(MAX_ROWS);
            if extra > 0 {
                rows.push(
                    text(
                        crate::i18n::t("lock_vault_confirm_more")
                            .replacen("{count}", &extra.to_string(), 1),
                    )
                    .size(11)
                    .color(c.text_muted)
                    .into(),
                );
            }
            container(column(rows).spacing(6))
                .width(Length::Fill)
                .padding(10)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color { a: 0.05, ..c.text_primary })),
                    border: Border { radius: Radius::from(10.0), color: c.border, width: 1.0 },
                    ..Default::default()
                })
                .into()
        };

        // Keyboard rows, in visual order: the remember opt-in, then
        // Cancel (first button AND default, the
        // `build_monitor_kill_dialog` precedent), Sleep, Lock. All three
        // buttons are the same widget (`styled_button` family: identical
        // padding / radius / font), so the trio renders at exactly one
        // height; widths follow the label, like every dialog in the app.
        let remember = self.modal_nav_slot(
            RowAction::activate(Message::Vault(VaultMessage::LockVaultConfirmRememberToggled)),
            4.0,
            false,
            iced::widget::checkbox(self.vault_ui.lock_confirm_remember)
                .label(crate::i18n::t("lock_vault_confirm_remember"))
                .on_toggle(|_| Message::Vault(VaultMessage::LockVaultConfirmRememberToggled))
                .size(16)
                .text_size(12)
                .into(),
        );
        let cancel = self.modal_nav_slot_default(
            RowAction::activate(Message::Vault(VaultMessage::CancelLockVaultConfirm)),
            6.0,
            false,
            styled_button(
                crate::i18n::t("cancel"),
                Message::Vault(VaultMessage::CancelLockVaultConfirm),
                c.text_muted,
            ),
        );
        // Sleep: the soft lock (issue #169 ask). Same operation the idle
        // auto-lock runs, so sessions and tabs survive the lock screen.
        let sleep = self.modal_nav_slot(
            RowAction::activate(Message::Vault(VaultMessage::LockVaultConfirmProceed {
                sleep: true,
            })),
            6.0,
            false,
            styled_button(
                crate::i18n::t("lock_vault_sleep"),
                Message::Vault(VaultMessage::LockVaultConfirmProceed { sleep: true }),
                c.accent,
            ),
        );
        // The filled error style marks the destructive action, matching
        // the clear-history "Clear all" button.
        let lock = self.modal_nav_slot(
            RowAction::activate(Message::Vault(VaultMessage::LockVaultConfirmProceed {
                sleep: false,
            })),
            6.0,
            true,
            styled_button(
                crate::i18n::t("lock_vault"),
                Message::Vault(VaultMessage::LockVaultConfirmProceed { sleep: false }),
                c.error,
            ),
        );

        let dialog = container(
            column![
                badge,
                Space::new().height(14),
                text(crate::i18n::t("lock_vault_confirm_title"))
                    .size(17)
                    .font(iced::Font {
                        weight: iced::font::Weight::Semibold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(c.text_primary),
                Space::new().height(8),
                text(crate::i18n::t("lock_vault_confirm_body"))
                    .size(13)
                    .color(c.text_secondary)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
                Space::new().height(6),
                // Its own line, not appended to the body: existing body
                // translations stay valid across all 23 languages.
                text(crate::i18n::t("lock_vault_confirm_sleep_hint"))
                    .size(12)
                    .color(c.text_muted)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
                Space::new().height(if total == 0 { 4 } else { 12 }),
                impact,
                Space::new().height(14),
                remember,
                Space::new().height(14),
                crate::widgets::dir_row(vec![
                    cancel,
                    Space::new().width(8).into(),
                    sleep,
                    Space::new().width(8).into(),
                    lock,
                ]),
            ]
            .width(Length::Fill)
            .align_x(iced::Alignment::Center)
            .padding(24),
        )
        .width(Length::Fixed(420.0))
        .style(|_| container::Style {
            background: Some(Background::Color(c.bg_surface)),
            border: Border { radius: Radius::from(14.0), color: c.border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }

    /// Content for the "kill the process on this port" confirmation
    /// (issue #96): the exact target, the signal, the irreversibility,
    /// and whatever the host said when a run came back unhappy.
    ///
    /// Keyboard: unlike `build_clear_history_dialog`, whose default row
    /// IS the destructive action, Cancel is the default here. The
    /// action is remote, irreversible and can take down a live service,
    /// so it follows the `AgentConfirm` precedent instead: a stray
    /// Enter must never stop somebody's database.
    pub(crate) fn build_monitor_kill_dialog(
        &self,
        pending: &crate::monitor::kill::PendingKill,
    ) -> Element<'_, Message> {
        use crate::monitor::kill::KillPhase;
        let c = OryxisColors::t();
        let running = pending.phase == KillPhase::Running;

        // Tinted warning badge, same anchor the folder-delete confirm
        // uses for a destructive choice.
        let badge = container(iced_fonts::lucide::triangle_alert().size(22).color(c.error))
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(48.0))
            .center_x(Length::Fixed(48.0))
            .center_y(Length::Fixed(48.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(Color { a: 0.12, ..c.error })),
                border: Border { radius: Radius::from(24.0), ..Default::default() },
                ..Default::default()
            });

        // Target line, monospace so port / PID line up: what is being
        // signalled, in the host's own vocabulary.
        let target = {
            let mut parts = vec![format!("{}/{}", pending.port, pending.proto)];
            if let Some(name) = &pending.process {
                parts.push(name.clone());
            }
            parts.push(match pending.pid {
                Some(pid) => format!("PID {pid}"),
                None => crate::i18n::t("monitor_kill_pid_unknown").to_string(),
            });
            parts.join("  ·  ")
        };

        let mut body = column![
            badge,
            Space::new().height(14),
            text(crate::i18n::t("monitor_kill_title"))
                .size(17)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(c.text_primary),
            Space::new().height(8),
            text(target)
                .size(13)
                .font(iced::Font::MONOSPACE)
                .color(c.text_primary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
            Space::new().height(4),
            text(crate::i18n::t(pending.signal.label_key()))
                .size(12)
                .color(c.text_muted)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
            Space::new().height(12),
            text(
                crate::i18n::t("monitor_kill_warning")
                    .replacen("{host}", &pending.host, 1),
            )
            .size(12)
            .color(c.warning)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
        ]
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);

        // No PID means an unescalated run has nothing to signal; say
        // so before the user commits, not after it fails.
        if pending.pid.is_none() {
            body = body.push(Space::new().height(8)).push(
                text(crate::i18n::t("monitor_kill_unknown_pid"))
                    .size(11)
                    .color(c.text_muted)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            );
        }

        if let KillPhase::Failed(outcome) = &pending.phase {
            body = body.push(Space::new().height(12)).push(
                container(
                    text(outcome.message())
                        .size(12)
                        .color(c.error)
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                )
                .width(Length::Fill)
                .padding(10)
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.10, ..c.error })),
                    border: Border {
                        radius: Radius::from(8.0),
                        color: Color { a: 0.4, ..c.error },
                        width: 1.0,
                    },
                    ..Default::default()
                }),
            );
        }

        // Keyboard rows, in visual order. Cancel first AND default.
        self.modal_nav_reset();
        let cancel = self.modal_nav_slot_default(
            crate::keynav::RowAction::activate(Message::Monitor(MonitorMessage::CancelKillPort)),
            6.0,
            false,
            styled_button(
                crate::i18n::t("cancel"),
                Message::Monitor(MonitorMessage::CancelKillPort),
                c.text_muted,
            ),
        );
        let mut actions = vec![cancel, Space::new().width(8).into()];
        if running {
            // Nothing to press while a run is on the wire: a second
            // confirm would queue a second signal behind the first.
            actions.push(
                container(text(crate::i18n::t("monitor_kill_running")).size(12).color(c.text_muted))
                    .padding(Padding { top: 8.0, right: 4.0, bottom: 8.0, left: 4.0 })
                    .into(),
            );
        } else {
            // A failure escalation can fix replaces the confirm button
            // with the retry: re-running it unescalated would just fail
            // the same way.
            let (label, msg) = match &pending.phase {
                KillPhase::Failed(outcome) if outcome.can_retry_with_sudo() => (
                    crate::i18n::t("monitor_kill_retry_sudo"),
                    Message::Monitor(MonitorMessage::RetryKillWithSudo),
                ),
                _ => (
                    crate::i18n::t("monitor_kill_process"),
                    Message::Monitor(MonitorMessage::ConfirmKillPort),
                ),
            };
            // A failure escalation CANNOT fix leaves Cancel alone: an
            // enabled "Kill" that is guaranteed to fail again is worse
            // than no button.
            let dead_end = matches!(
                &pending.phase,
                KillPhase::Failed(outcome) if !outcome.can_retry_with_sudo()
            );
            if !dead_end {
                actions.push(self.modal_nav_slot(
                    crate::keynav::RowAction::activate(msg.clone()),
                    6.0,
                    true,
                    styled_button(label, msg, c.error),
                ));
            }
        }

        let dialog = container(
            column![body, Space::new().height(20), dir_row(actions)]
                .width(Length::Fill)
                .align_x(iced::Alignment::Center)
                .padding(24),
        )
        .width(Length::Fixed(420.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(c.bg_surface)),
            border: Border { radius: Radius::from(14.0), color: c.border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }
}

/// One row of the lock-confirm impact list: a small glyph + the label of
/// a session the lock will sever. Labels are clipped on one line so a
/// long hostname can't wrap the tight row.
fn lock_impact_row(icon: iced::widget::Text<'static>, label: String) -> Element<'static, Message> {
    dir_row(vec![
        icon.size(12).color(OryxisColors::t().text_muted).into(),
        Space::new().width(8).into(),
        text(label)
            .size(12)
            .color(OryxisColors::t().text_secondary)
            .width(Length::Fill)
            .wrapping(iced::widget::text::Wrapping::None)
            .into(),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}
