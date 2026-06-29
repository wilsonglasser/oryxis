//! Settings -> Sync section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_sync(&self) -> Element<'_, Message> {
        // Device info
        let device_name_input = text_input(
            crate::i18n::t("sync_device_name_hint"),
            &self.sync.device_name,
        )
        .on_input(Message::SyncDeviceNameChanged)
        .padding(10)
        .width(300)
        .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        let device_section = panel_section(column![
            text(crate::i18n::t("sync_device")).size(14).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            text(crate::i18n::t("sync_device_name")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(4),
            device_name_input,
        ]);

        // Enable/disable lives on the Plugins screen now.
        let mode_label = if self.sync.mode == "auto" { t("sync_mode_auto") } else { t("sync_mode_manual") };
        let auto_label = t("sync_mode_auto").to_string();
        let manual_label = t("sync_mode_manual").to_string();
        let mode_pick = pick_list(
            Some(mode_label.to_string()),
            vec![auto_label.clone(), manual_label.clone()],
            |s: &String| s.clone(),
        )
        .on_select(move |v| {
            // Compare against localized labels first; fall back
            // to English so labels persisted in another locale
            // still resolve to a known mode.
            let mode = if v == auto_label || v == "Auto" {
                "auto"
            } else {
                "manual"
            };
            Message::SyncModeChanged(mode.to_string())
        })
        .text_size(13)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        let passwords_toggle = toggle_row(
            crate::i18n::t("sync_passwords"),
            self.sync.passwords,
            Message::SyncTogglePasswords,
        );

        // Live engine state indicator, sits right under the
        // enable toggle so the user sees whether the QUIC /
        // mDNS background tasks are actually up. The SFTP
        // transport runs no background engine, so reporting
        // "Engine stopped" there would read as broken; show a
        // transport-appropriate label instead.
        let engine_state = if self.sync.transport == "sftp" {
            let (label, color) = if self.sync.enabled {
                (
                    crate::i18n::t("sftp_sync_active_label"),
                    OryxisColors::t().success,
                )
            } else {
                (
                    crate::i18n::t("sync_engine_stopped_label"),
                    OryxisColors::t().text_muted,
                )
            };
            text(label).size(11).color(color)
        } else if self.sync.engine_running {
            text(crate::i18n::t("sync_engine_running_label"))
                .size(11)
                .color(OryxisColors::t().success)
        } else {
            text(crate::i18n::t("sync_engine_stopped_label"))
                .size(11)
                .color(OryxisColors::t().text_muted)
        };

        // Master enable panel sits at the top, same shape as
        // the Enable SFTP / Enable AI panels: a single toggle
        // (with the engine state hint right under it). When
        // the master toggle is off, every other Sync panel
        // is hidden below so the surface collapses to just
        // the on/off knob.
        let enable_section: iced::widget::Column<'_, Message> = column![
            engine_state,
        ];

        let mut options_section: iced::widget::Column<'_, Message> = column![
            text(crate::i18n::t("sync_options")).size(14).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            dir_row(vec![
                text(crate::i18n::t("sync_mode")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                mode_pick.into(),
            ]).align_y(iced::Alignment::Center),
            Space::new().height(8),
            passwords_toggle,
            Space::new().height(4),
            text(crate::i18n::t("sync_passwords_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ];

        if self.sync.enabled && self.sync.mode == "manual" {
            if self.sync.transport == "sftp" {
                // SFTP round: relabel + disable the button while a
                // round is in flight so the click has immediate
                // feedback. There's no engine/Cancel path (the
                // transfer can't be safely aborted mid-write).
                let (label, msg) = if self.sync.sftp.in_progress {
                    (crate::i18n::t("sftp_sync_running"), None)
                } else {
                    (crate::i18n::t("sync_now"), Some(Message::SyncNow))
                };
                options_section = options_section.push(Space::new().height(8)).push(
                    styled_button_opt(label, msg, OryxisColors::t().accent),
                );
            } else {
                // P2P: swap Sync Now <-> Cancel while a sync is in
                // flight. Cancel races a oneshot against the sync
                // future in dispatch; the click drops the QUIC
                // connection immediately.
                let action_btn = if self.sync.in_progress {
                    styled_button(
                        crate::i18n::t("sync_pairing_cancel"),
                        Message::SyncCancelInProgress,
                        OryxisColors::t().button_bg,
                    )
                } else {
                    styled_button(
                        crate::i18n::t("sync_now"),
                        Message::SyncNow,
                        OryxisColors::t().accent,
                    )
                };
                options_section = options_section
                    .push(Space::new().height(8))
                    .push(action_btn);
            }
        }

        // Status line directly under the action button. SFTP shows
        // its own round outcome (success muted / error red); P2P
        // keeps the engine status string.
        if self.sync.transport == "sftp" {
            if let Some(status) = &self.sync.sftp.status {
                let (txt, color) = match status {
                    Ok(s) => (s.clone(), OryxisColors::t().text_muted),
                    Err(e) => (e.clone(), OryxisColors::t().error),
                };
                options_section = options_section
                    .push(Space::new().height(8))
                    .push(text(txt).size(12).color(color));
            }
        } else if let Some(status) = &self.sync.status {
            options_section = options_section
                .push(Space::new().height(8))
                .push(text(status.as_str()).size(12).color(OryxisColors::t().text_muted));
        }

        // Pairing. The sub-view depends on `sync_pairing.state`:
        // Idle shows the two entry buttons; Hosting shows the
        // generated code; Joining shows the code + address form.
        let mut pairing_section: iced::widget::Column<'_, Message> = column![
            text(crate::i18n::t("sync_pairing")).size(14).color(OryxisColors::t().text_muted),
            Space::new().height(8),
        ];

        match self.sync.pairing.state {
            crate::state::SyncPairingState::Idle => {
                pairing_section = pairing_section.push(dir_row(vec![
                    styled_button(
                        crate::i18n::t("sync_host_pairing"),
                        Message::SyncStartPairing,
                        OryxisColors::t().accent,
                    ),
                    Space::new().width(8).into(),
                    styled_button(
                        crate::i18n::t("sync_join_pairing"),
                        Message::SyncJoinPairingRequested,
                        OryxisColors::t().button_bg,
                    ),
                ]));
                // Live mDNS-discovered devices on the LAN.
                // One-click "Pair" switches to the join form
                // with the address pre-filled, so the user
                // only has to enter the 6-digit code.
                if !self.sync.discovered.is_empty() {
                    pairing_section = pairing_section
                        .push(Space::new().height(14))
                        .push(text(crate::i18n::t("sync_discovered_devices"))
                            .size(12)
                            .color(OryxisColors::t().text_secondary))
                        .push(Space::new().height(6));
                    for peer in &self.sync.discovered {
                        let label = if peer.device_name.is_empty() {
                            crate::i18n::t("sync_discovered_unnamed").to_string()
                        } else {
                            peer.device_name.clone()
                        };
                        let pair_btn = styled_button(
                            crate::i18n::t("sync_pair_with_this"),
                            Message::SyncPairWithDiscovered(peer.device_id),
                            OryxisColors::t().button_bg,
                        );
                        pairing_section = pairing_section
                            .push(dir_row(vec![
                                text(label)
                                    .size(13)
                                    .color(OryxisColors::t().text_primary)
                                    .into(),
                                Space::new().width(8).into(),
                                text(peer.addr.to_string())
                                    .size(11)
                                    .color(OryxisColors::t().text_muted)
                                    .into(),
                                Space::new().width(Length::Fill).into(),
                                pair_btn,
                            ])
                            .align_y(iced::Alignment::Center))
                            .push(Space::new().height(4));
                    }
                }
            }
            crate::state::SyncPairingState::Hosting => {
                pairing_section = pairing_section
                    .push(text(crate::i18n::t("sync_pairing_show_code"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary))
                    .push(Space::new().height(6));
                if let Some(code) = &self.sync.pairing.code {
                    pairing_section = pairing_section
                        .push(text(code.as_str())
                            .size(30)
                            .color(OryxisColors::t().success));
                }
                // Cross-network pairing block: the link + a
                // Copy button + the QR. The link works only
                // when both ends have a signaling URL set
                // (Settings > Sync > Advanced).
                if let Some(link) = &self.sync.pairing.link {
                    pairing_section = pairing_section
                        .push(Space::new().height(12))
                        .push(text(crate::i18n::t("sync_pairing_link_label"))
                            .size(12)
                            .color(OryxisColors::t().text_secondary))
                        .push(Space::new().height(4))
                        .push(text(link.as_str())
                            .size(11)
                            .color(OryxisColors::t().text_muted))
                        .push(Space::new().height(6))
                        .push(styled_button(
                            crate::i18n::t("sync_pairing_copy_link"),
                            Message::CopyToClipboard(link.clone()),
                            OryxisColors::t().button_bg,
                        ));
                }
                pairing_section = pairing_section
                    .push(Space::new().height(12))
                    .push(styled_button(
                        crate::i18n::t("sync_pairing_cancel"),
                        Message::SyncCancelHostingPairing,
                        OryxisColors::t().button_bg,
                    ));
            }
            crate::state::SyncPairingState::Joining => {
                let code_input = text_input(
                    crate::i18n::t("sync_pairing_code_placeholder"),
                    &self.sync.pairing.join_code_input,
                )
                .on_input(Message::SyncJoinCodeChanged)
                .padding(8)
                .width(280)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x());
                let target_input = text_input(
                    crate::i18n::t("sync_pairing_target_placeholder"),
                    &self.sync.pairing.join_target_input,
                )
                .on_input(Message::SyncJoinTargetChanged)
                .padding(8)
                .width(320)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x());
                let link_input = text_input(
                    crate::i18n::t("sync_pairing_link_placeholder"),
                    &self.sync.pairing.join_link_input,
                )
                .on_input(Message::SyncJoinLinkChanged)
                .padding(8)
                .width(360)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x());
                pairing_section = pairing_section
                    .push(code_input)
                    .push(Space::new().height(8))
                    .push(target_input)
                    .push(Space::new().height(10))
                    .push(dir_row(vec![
                        styled_button(
                            crate::i18n::t("sync_pairing_connect"),
                            Message::SyncJoinPairingConnect,
                            OryxisColors::t().accent,
                        ),
                        Space::new().width(8).into(),
                        styled_button(
                            crate::i18n::t("sync_pairing_cancel"),
                            Message::SyncJoinPairingCancel,
                            OryxisColors::t().button_bg,
                        ),
                    ]))
                    .push(Space::new().height(14))
                    .push(text(crate::i18n::t("sync_pairing_or_separator"))
                        .size(11)
                        .color(OryxisColors::t().text_muted))
                    .push(Space::new().height(6))
                    .push(link_input)
                    .push(Space::new().height(8))
                    .push(styled_button(
                        crate::i18n::t("sync_pairing_connect_with_link"),
                        Message::SyncJoinPairingByLink,
                        OryxisColors::t().accent,
                    ));
            }
        }

        // Inline status banner inside the pairing card. The
        // same `sync_status` field also shows under "Sync Now"
        // in the Options card, but when the user is actively
        // pairing they're looking here, so we mirror it
        // adjacent to the form they're filling in.
        if !matches!(self.sync.pairing.state, crate::state::SyncPairingState::Idle)
            && let Some(status) = &self.sync.status
        {
            pairing_section = pairing_section
                .push(Space::new().height(8))
                .push(text(status.as_str())
                    .size(11)
                    .color(OryxisColors::t().text_muted));
        }

        // Paired devices list. Empty until the first successful
        // pairing on either side; pre-Phase B builds never
        // populated this because the engine wasn't wired.
        if !self.sync.peers.is_empty() {
            pairing_section = pairing_section
                .push(Space::new().height(14))
                .push(text(crate::i18n::t("sync_paired_devices"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary))
                .push(Space::new().height(6));
            for peer in &self.sync.peers {
                let last_sync = peer.last_synced_at
                    // Stored UTC; show in the user's local timezone.
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| crate::i18n::t("sync_never").into());
                let unpair = button(
                    text(crate::i18n::t("sync_unpair")).size(11).color(OryxisColors::t().error)
                ).on_press(Message::SyncUnpairDevice(peer.peer_id)).style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    ..Default::default()
                });
                pairing_section = pairing_section.push(
                    dir_row(vec![
                        text(&peer.device_name).size(13).color(OryxisColors::t().text_primary).into(),
                        Space::new().width(Length::Fill).into(),
                        text(last_sync).size(11).color(OryxisColors::t().text_muted).into(),
                        Space::new().width(8).into(),
                        unpair.into(),
                    ]).align_y(iced::Alignment::Center),
                ).push(Space::new().height(4));
            }
        }

        // Advanced
        let signaling_input = text_input("https://...", &self.sync.signaling_url)
            .on_input(Message::SyncSignalingUrlChanged)
            .padding(8)
            .width(300)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
        let signaling_token_input = container(
            crate::widgets::password_input_with_eye(
                crate::i18n::t("sync_signaling_token_placeholder"),
                &self.sync.signaling_token,
                Message::SyncSignalingTokenChanged,
                None,
                self.revealed_secrets
                    .contains(&crate::state::SecretField::SyncSignalingToken),
                Message::ToggleSecretVisibility(
                    crate::state::SecretField::SyncSignalingToken,
                ),
                8.0,
            ),
        )
        .width(300);
        let relay_input = text_input(crate::i18n::t("sync_relay_optional"), &self.sync.relay_url)
            .on_input(Message::SyncRelayUrlChanged)
            .padding(8)
            .width(300)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
        let port_input = text_input("0", &self.sync.listen_port)
            .on_input(Message::SyncListenPortChanged)
            .padding(8)
            .width(100)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        let advanced_section = panel_section(column![
            text(crate::i18n::t("sync_advanced")).size(14).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            text(crate::i18n::t("sync_signaling_url")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(4),
            signaling_input,
            Space::new().height(8),
            text(crate::i18n::t("sync_signaling_token")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(4),
            signaling_token_input,
            Space::new().height(8),
            text(crate::i18n::t("sync_relay_url")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(4),
            relay_input,
            Space::new().height(8),
            text(crate::i18n::t("sync_listen_port")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(4),
            port_input,
        ]);

        // Plain-language primer: what sync is, that it's optional and
        // LAN-only by default (no Oryxis server), and what the user
        // must set up to sync across networks. Answers the recurring
        // "is sync required / where does my data go?" question.
        let how_section = panel_section(column![
            text(crate::i18n::t("sync_how_title")).size(14).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            text(crate::i18n::t("sync_how_body"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
        ]);

        // Transport picker (P2P vs SFTP), the "one or the other"
        // choice. Always visible while sync is enabled; selecting
        // it persists the setting and (un)mounts the P2P engine.
        let is_sftp = self.sync.transport == "sftp";
        let p2p_label = crate::i18n::t("sync_transport_p2p").to_string();
        let sftp_label = crate::i18n::t("sync_transport_sftp").to_string();
        let transport_selected = if is_sftp {
            sftp_label.clone()
        } else {
            p2p_label.clone()
        };
        let sftp_label_for_select = sftp_label.clone();
        let transport_pick = pick_list(
            Some(transport_selected),
            vec![p2p_label.clone(), sftp_label.clone()],
            |s: &String| s.clone(),
        )
        .on_select(move |v| {
            let tr = if v == sftp_label_for_select || v == "SFTP" {
                "sftp"
            } else {
                "p2p"
            };
            Message::SyncTransportChanged(tr.to_string())
        })
        .text_size(13)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);
        let transport_section = panel_section(column![
            text(crate::i18n::t("sync_transport"))
                .size(14)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            dir_row(vec![
                text(crate::i18n::t("sync_transport_field"))
                    .size(13)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                transport_pick.into(),
            ])
            .align_y(iced::Alignment::Center),
        ]);

        // SFTP-transport config: host, remote path, passphrase,
        // status, and the group/known-host notes.
        // Host field opens the same rich "Select a host" modal as
        // the SFTP file browser (OS badge + label + address +
        // search), not a flat dropdown. The trigger shows the
        // current selection or a placeholder.
        let selected_conn = self
            .sync.sftp.host_id
            .and_then(|id| self.connections.iter().find(|c| c.id == id));
        let host_trigger_inner: Element<'_, Message> = if let Some(c) = selected_conn {
            dir_row(vec![
                host_badge(c, &self.setting_default_host_icon, 22.0),
                Space::new().width(10).into(),
                text(c.label.clone())
                    .size(13)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                text("\u{25BE}").size(12).color(OryxisColors::t().text_muted).into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            dir_row(vec![
                text(crate::i18n::t("select_a_host"))
                    .size(13)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                Space::new().width(Length::Fill).into(),
                text("\u{25BE}").size(12).color(OryxisColors::t().text_muted).into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        };
        let host_pick = button(host_trigger_inner)
            .on_press(Message::SyncSftpOpenPicker)
            .padding(10)
            .width(300)
            .style(|_, status| {
                let c = OryxisColors::t();
                let border = match status {
                    BtnStatus::Hovered => c.accent_hover,
                    _ => c.border,
                };
                button::Style {
                    background: Some(Background::Color(c.bg_surface)),
                    text_color: c.text_primary,
                    border: Border {
                        radius: Radius::from(8.0),
                        width: 1.0,
                        color: border,
                    },
                    ..Default::default()
                }
            });
        let path_input = text_input(
            "/home/user/oryxis-sync/",
            &self.sync.sftp.remote_path,
        )
        .on_input(Message::SyncSftpPathChanged)
        .padding(10)
        .width(300)
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x());
        let passphrase_input = text_input(
            crate::i18n::t("sftp_sync_passphrase_placeholder"),
            &self.sync.sftp.passphrase,
        )
        .on_input(Message::SyncSftpPassphraseChanged)
        .secure(true)
        .padding(10)
        .width(300)
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x());
        let mut sftp_section_col = column![
            text(crate::i18n::t("sftp_sync_title"))
                .size(14)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            panel_field(crate::i18n::t("sftp_sync_host"), host_pick.into()),
            Space::new().height(8),
            panel_field(crate::i18n::t("sftp_sync_path"), path_input.into()),
            Space::new().height(8),
            panel_field(
                crate::i18n::t("sftp_sync_passphrase"),
                passphrase_input.into(),
            ),
        ];
        // (The round status lives under the Sync Now button in the
        // options panel above, not here, so feedback sits next to
        // the control that triggers it.)
        sftp_section_col = sftp_section_col
            .push(Space::new().height(12))
            .push(
                text(crate::i18n::t("sftp_sync_note_group"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
            .push(Space::new().height(4))
            .push(
                text(crate::i18n::t("sftp_sync_note_bridge"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
            .push(Space::new().height(4))
            .push(
                text(crate::i18n::t("sftp_sync_note_hostkey"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            );
        let sftp_section = panel_section(sftp_section_col);

        let mut content_col: iced::widget::Column<'_, Message> = column![
            panel_section(enable_section),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        content_col = content_col
            .push(Space::new().height(12))
            .push(how_section);

        if self.sync.enabled {
            content_col = content_col
                .push(Space::new().height(12))
                .push(transport_section)
                .push(Space::new().height(12))
                .push(panel_section(options_section));
            if is_sftp {
                content_col = content_col
                    .push(Space::new().height(12))
                    .push(sftp_section);
            } else {
                content_col = content_col
                    .push(Space::new().height(12))
                    .push(device_section)
                    .push(Space::new().height(12))
                    .push(panel_section(pairing_section))
                    .push(Space::new().height(12))
                    .push(advanced_section);
            }
        }
        content_col = content_col.push(Space::new().height(24));

        scrollable(
            container(content_col)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill)
        .into()
    }
}
