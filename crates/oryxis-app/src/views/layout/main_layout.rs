//! Root layout: main_layout. Split out of views/layout/mod.rs.

use super::*;
use iced::widget::{column, row};
impl Oryxis {
    pub(crate) fn view_main(&self) -> Element<'_, Message> {
        // Single top-bar layout: the tab bar (Home icon + session tabs +
        // burger) spans the full width; there is no classic full-height
        // sidebar. The vault sub-sections render either as a horizontal
        // pill strip below the bar or as a vertical icon rail on the left
        // of the content, per `setting_nav_orientation`.
        // Browser-style fullscreen suppresses every piece of chrome (tab
        // bar, status bar) so the content fills the monitor edge-to-edge.
        // The X-close affordance and the on-enter hint banner are drawn as
        // Stack overlays below.
        let immersive = self.window_fullscreen;
        let tab_bar: Element<'_, Message> = if immersive {
            Space::new().height(0).into()
        } else {
            self.view_tab_bar()
        };
        let content = self.view_content();
        // Status bar is opt-out (Interface → Show status bar) and
        // also suppressed in immersive fullscreen.
        let status_bar: Element<'_, Message> = if self.setting_show_status_bar && !immersive {
            self.view_status_bar()
        } else {
            Space::new().height(0).into()
        };

        // Tab-bar bottom hairline. When a connection tab is active and
        // it has a per-host accent color, paint the hairline 2 px and
        // tint it that color (JetBrains-style "respiração" of the
        // active project). Falls back to the global accent for tabs
        // without a per-host color, and the neutral border for non-
        // connection screens so settings / dashboard don't look like
        // they belong to whichever host happened to be open last.
        let accent_tint: Option<Color> = if self.setting_tab_accent_line {
            Some(self.top_accent_tint())
        } else {
            None
        };
        let (hair_height, hair_color) = match accent_tint {
            Some(c) => (2.0_f32, c),
            None => (1.0_f32, OryxisColors::t().border),
        };
        let h_separator: Element<'_, Message> = if immersive {
            Space::new().height(0).into()
        } else {
            container(Space::new().height(hair_height))
                .width(Length::Fill)
                .style(move |_| {
                    // When the accent line is on, the border washes
                    // left→right (bright accent on the leading edge fading
                    // out), matching the card accent wash and ready to
                    // double as an (infinite) progress bar later. Off →
                    // the neutral 1px border.
                    let bg = match accent_tint {
                        Some(c) => Background::Gradient(iced::Gradient::Linear(
                            iced::gradient::Linear::new(iced::Radians(
                                std::f32::consts::FRAC_PI_2,
                            ))
                            .add_stop(0.0, c)
                            .add_stop(0.85, Color { a: 0.0, ..c }),
                        )),
                        None => Background::Color(hair_color),
                    };
                    container::Style {
                        background: Some(bg),
                        ..Default::default()
                    }
                })
                .into()
        };
        // Vault contextual nav: shown only when the Home area is active.
        // On Sftp / Settings / a connection tab it's hidden.
        let in_vault_area = self.in_vault_area();
        let vertical_rail = self.setting_nav_orientation == "vertical";
        // Horizontal pill strip pinned above the content.
        let sub_nav: Element<'_, Message> = if in_vault_area && !vertical_rail {
            self.view_vault_sub_nav()
        } else {
            Space::new().height(0).into()
        };
        // Vertical icon rail on the leading edge of the content.
        let nav_rail: Option<Element<'_, Message>> = if in_vault_area && vertical_rail {
            Some(self.view_vault_nav_rail())
        } else {
            None
        };

        // Compose the content with its nav (rail on the leading edge OR
        // sub-nav strip above) and the side panel (editor) on the trailing
        // edge. The side panel rises full-height, covering the sub-nav band
        // on its own side; the vertical rail stays on the leading edge.
        let inner: Element<'_, Message> = match nav_rail {
            Some(rail) => {
                // With the rail on the side (no sub-nav strip on top), the
                // view toolbars' 16px top padding reads as a tighter top
                // gutter than the 24px left gutter. Add 8px so the content's
                // top spacing matches its left and the corner looks square.
                let content = container(content)
                    .padding(Padding { top: 8.0, right: 0.0, bottom: 0.0, left: 0.0 })
                    .width(Length::Fill)
                    .height(Length::Fill);
                dir_row(vec![rail, content.into()]).height(Length::Fill).into()
            }
            None => column![sub_nav, content].height(Length::Fill).into(),
        };
        let body: Element<'_, Message> = match self.active_side_panel() {
            Some(panel) => dir_row(vec![inner, panel]).height(Length::Fill).into(),
            None => inner,
        };
        let right_side: Element<'_, Message> =
            column![tab_bar, h_separator, body].height(Length::Fill).into();
        let layout = column![right_side, status_bar];

        let base: Element<'_, Message> = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            })
            .into();

        // Edge/corner resize handles, only when the window isn't
        // maximized or in immersive fullscreen (no borders to grab in
        // either case). Placed as the topmost stack layer so they win
        // over tab-bar buttons near the frame, while the Space in the
        // middle is pass-through.
        let resize_overlay: Option<Element<'_, Message>> =
            if self.window_maximized || immersive { None } else { Some(resize_border()) };

        // SFTP close-guard: the close button lives in the always-visible tab
        // strip, so this modal must render globally (not just on the SFTP
        // surface) or a close click from a terminal would set the pending
        // state with no modal to resolve it.
        if self.pending_sftp_close.is_some() {
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(iced::widget::opaque(crate::views::sftp::close_guard_modal()))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Burger menu overlay (top-left dropdown). Renders first so any
        // other modal stacked below still wins, but in practice the
        // burger menu and the bigger modals (share dialog, picker, etc.)
        // never coexist on the user's screen at the same time.
        if self.show_burger_menu {
            let menu = self.view_burger_menu();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Vault sub-nav overflow ("…") dropdown, same overlay shape as
        // the burger menu.
        if self.show_subnav_overflow {
            let menu = self.view_subnav_overflow_menu();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Share dialog overlay
        if self.show_share_dialog {
            let share_include_keys = self.share.include_keys;
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
                    list = list.push(
                        iced::widget::checkbox(self.share.groups.contains(&id))
                            .label(g.label.as_str())
                            .on_toggle(move |_| Message::ShareToggleGroup(id))
                            .size(16)
                            .text_size(13),
                    );
                }
                list = list.push(
                    iced::widget::checkbox(self.share.include_ungrouped)
                        .label(crate::i18n::t("export_ungrouped"))
                        .on_toggle(|_| Message::ShareToggleUngrouped)
                        .size(16)
                        .text_size(13),
                );
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
                Space::new().height(0).into()
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
                    container(crate::widgets::password_input_with_eye(
                        crate::i18n::t("export_password"),
                        &self.share.password,
                        Message::SharePasswordChanged,
                        None,
                        self.revealed_secrets
                            .contains(&crate::state::SecretField::SharePassword),
                        Message::ToggleSecretVisibility(
                            crate::state::SecretField::SharePassword,
                        ),
                        10.0,
                    ))
                    .width(280),
                    Space::new().height(8),
                    group_picker,
                    row![
                        text(crate::i18n::t("include_private_keys")).size(13).color(OryxisColors::t().text_secondary),
                        Space::new().width(Length::Fill),
                        button(
                            text(if share_include_keys { "ON" } else { "OFF" }).size(12)
                        ).on_press(Message::ShareToggleKeys).style(move |_theme, _status| {
                            button::Style {
                                background: Some(Background::Color(if share_include_keys { OryxisColors::t().success } else { OryxisColors::t().bg_hover })),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: OryxisColors::t().text_primary,
                                ..Default::default()
                            }
                        }),
                    ].align_y(iced::Alignment::Center).width(280),
                    Space::new().height(12),
                    row![
                        styled_button(crate::i18n::t("share"), Message::ShareConfirm, OryxisColors::t().accent),
                        Space::new().width(8),
                        styled_button(crate::i18n::t("cancel"), Message::ShareDismiss, OryxisColors::t().text_muted),
                    ],
                    if let Some(status) = &self.share.status {
                        let (msg, color) = match status {
                            Ok(m) => (m.as_str(), OryxisColors::t().success),
                            Err(m) => (m.as_str(), OryxisColors::t().error),
                        };
                        Element::from(column![Space::new().height(8), text(msg).size(12).color(color)])
                    } else {
                        Element::from(Space::new().height(0))
                    },
                ]
                .padding(24),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog_content.into(),
                    Some(Message::ShareDismiss),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // SSH config import preview. Lists every parsed host with a
        // checkbox so the user picks which to add; hosts whose label
        // already exists are flagged and start unticked.
        if self.show_ssh_import_dialog {
            let ssh_total = self.ssh_import_hosts.len();
            let ssh_selected =
                self.ssh_import_selected.iter().filter(|s| **s).count();
            let mut list = column![].spacing(4);
            for (i, host) in self.ssh_import_hosts.iter().enumerate() {
                let checked =
                    self.ssh_import_selected.get(i).copied().unwrap_or(false);
                let exists =
                    self.ssh_import_existing.get(i).copied().unwrap_or(false);
                // "user@hostname:port", falling back to the alias when
                // HostName is omitted (OpenSSH treats the alias as host).
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
                let mut label = format!("{}  ({detail})", host.alias);
                if exists {
                    label.push_str("  · ");
                    label.push_str(crate::i18n::t("ssh_import_exists"));
                }
                list = list.push(
                    iced::widget::checkbox(checked)
                        .label(label)
                        .on_toggle(move |_| Message::SshImportToggle(i))
                        .size(16)
                        .text_size(13),
                );
            }
            let dialog_content = container(
                column![
                    text(crate::i18n::t("import_ssh_config_btn"))
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
                    Space::new().height(8),
                    row![
                        styled_button(crate::i18n::t("select_all"), Message::SshImportSelectAll(true), OryxisColors::t().accent),
                        Space::new().width(8),
                        styled_button(crate::i18n::t("deselect_all"), Message::SshImportSelectAll(false), OryxisColors::t().text_muted),
                    ],
                    Space::new().height(8),
                    container(
                        iced::widget::scrollable(list)
                            .height(Length::Fixed(280.0))
                    )
                    .width(440),
                    Space::new().height(12),
                    row![
                        styled_button(crate::i18n::t("import_from_file"), Message::SshImportConfirm, OryxisColors::t().success),
                        Space::new().width(8),
                        styled_button(crate::i18n::t("cancel"), Message::SshImportDismiss, OryxisColors::t().text_muted),
                    ],
                ]
                .padding(24),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog_content.into(),
                    Some(Message::SshImportDismiss),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Generic blocking error dialog. Currently surfaces the
        // "AWS session-manager-plugin missing" case but reusable for
        // any "user must read this and act" failure. Title + body +
        // optional "open URL" button (the URL opens in the system
        // browser via Message::OpenUrl).
        if let Some(dialog) = self.error_dialog.clone() {
            let mut buttons = iced::widget::row![styled_button(
                crate::i18n::t("close"),
                Message::ErrorDialogDismiss,
                OryxisColors::t().text_muted,
            )]
            .spacing(8);
            if let Some(link) = dialog.link.clone() {
                buttons = buttons.push(open_link_button(link.label, link.url));
            }
            if let Some(action) = dialog.action.clone() {
                // Recovery action, accent-styled like the link button;
                // dispatching goes through ErrorDialogRunAction so the
                // dialog also dismisses itself.
                buttons = buttons.push(dialog_action_button(action.label, action.danger));
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
            .max_width(520)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog_content.into(),
                    Some(Message::ErrorDialogDismiss),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Cloud import confirmation modal. Always opens on Import (no
        // ECS-only short-circuit) so the user can set the target
        // group from the same surface that already gates the
        // transport choice. Transport row hides itself when the
        // batch is ECS-only since dynamic groups always run ECS Exec.
        if self.cloud_import_confirm_visible {
            use oryxis_core::models::cloud::TransportKind;
            let n_ec2 = self.cloud_discover_selected_ec2.len();
            let n_ecs = self.cloud_discover_selected_ecs.len();
            let summary = if n_ec2 > 0 && n_ecs > 0 {
                format!("{} EC2 + {} ECS", n_ec2, n_ecs)
            } else if n_ec2 > 0 {
                format!("{} EC2", n_ec2)
            } else {
                format!("{} ECS", n_ecs)
            };

            // Import-into field + chevron. The suggestion dropdown
            // is no longer inline; it's a floating popover rendered
            // via the global OverlayState (`CloudDiscoverGroupPicker`)
            // injected into the modal's own Stack below so it can
            // visually rise above the dialog instead of pushing
            // siblings. Input + chevron heights are explicitly fixed
            // to 36 so they stay aligned in the row.
            const COMBO_HEIGHT: f32 = 36.0;
            let group_input = iced::widget::text_input(
                crate::i18n::t("cloud_discover_import_into_placeholder"),
                &self.cloud_discover_default_group_name,
            )
            .on_input(Message::CloudDiscoverDefaultGroupNameChanged)
            .padding(8)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());
            let chevron_btn = iced::widget::button(
                container(
                    iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                )
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(COMBO_HEIGHT)),
            )
            .on_press(Message::ToggleCloudDiscoverGroupPicker)
            .padding(0)
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => OryxisColors::t().bg_hover,
                    _ => OryxisColors::t().bg_surface,
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });

            // Transport picker is always rendered. For ECS-only
            // imports the value is ignored on save (dynamic groups
            // always run ECS Exec), but keeping the row in place
            // preserves the row geometry + the explanatory hint
            // beneath it and avoids the modal looking sparse when
            // the user happens to pick zero EC2 hosts.
            let transport_section: Element<'_, Message> = {
                let transport_options = vec![
                    TransportKind::Ssh,
                    TransportKind::InstanceConnect,
                    TransportKind::Ssm,
                ];
                let transport_pick = iced::widget::pick_list(
                    Some(self.cloud_discover_default_transport),
                    transport_options,
                    |t| match t {
                        TransportKind::Ssh => "SSH".to_string(),
                        TransportKind::InstanceConnect => "EC2 Instance Connect".to_string(),
                        TransportKind::Ssm => "SSM Session".to_string(),
                        TransportKind::EcsExec => "ECS Exec".to_string(),
                        TransportKind::KubectlExec => "kubectl exec".to_string(),
                    },
                )
                .on_select(Message::CloudDiscoverDefaultTransportChanged)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style);
                column![
                    text(crate::i18n::t("cloud_dynamic_form_transport"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    container(transport_pick).width(Length::Fixed(320.0)),
                    Space::new().height(8),
                    text(crate::i18n::t("cloud_import_transport_hint"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(16),
                ]
                .into()
            };
            // Silence the now-unused n_ec2 binding; kept by name so
            // the summary text above can read it without re-querying.
            let _ = n_ec2;

            let dialog_content = container(
                column![
                    text(crate::i18n::t("cloud_import_confirm_title"))
                        .size(16)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(summary).size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(16),
                    // "Import into" comes BEFORE Transport: the
                    // dropdown is anchored to the chevron and opens
                    // downward, so having the field higher in the
                    // dialog gives the menu maximum vertical room
                    // to extend without escaping the screen edge.
                    text(crate::i18n::t("cloud_discover_import_into"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    // Wrap the combo row in `bounds_reporter` so the
                    // toggle handler can read its on-screen rect and
                    // anchor the picker overlay right below it. The
                    // cell lives on Oryxis state; the wrapper here
                    // just writes to it on every draw pass. Wrapping
                    // the whole row (input + chevron) means the menu
                    // can mirror the full combo width by default,
                    // covering the empty area between the input and
                    // the chevron edge.
                    crate::widgets::bounds_reporter(
                        dir_row(vec![
                            container(group_input)
                                .width(Length::Fill)
                                .height(Length::Fixed(COMBO_HEIGHT))
                                .into(),
                            Space::new().width(6).into(),
                            container(chevron_btn)
                                .height(Length::Fixed(COMBO_HEIGHT))
                                .into(),
                        ])
                        .width(Length::Fixed(308.0))
                        .align_y(iced::Alignment::Center),
                        self.cloud_discover_default_group_combo_bounds.clone(),
                    ),
                    Space::new().height(16),
                    transport_section,
                    crate::widgets::dir_row(vec![
                        styled_button(
                            crate::i18n::t("import_btn_label"),
                            Message::CloudDiscoverImportConfirmed,
                            OryxisColors::t().accent,
                        ),
                        Space::new().width(8).into(),
                        styled_button(
                            crate::i18n::t("cancel"),
                            Message::CloudDiscoverImportCancelled,
                            OryxisColors::t().text_muted,
                        ),
                    ]),
                ]
                .padding(24),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });

            let centered = container(
                MouseArea::new(dialog_content).on_press(Message::NoOp),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

            // Intentionally NOT routed through `widgets::modal_overlay`:
            // this modal injects a positioned group-picker popover into its
            // own Stack (below) and uses a context-dependent scrim message,
            // neither of which the simple helper hosts. It stays mouse-safe
            // via `opaque` and keyboard-safe via `any_modal_blocks_input`.
            //
            // Scrim behaviour: while the group picker is open,
            // off-dialog clicks dismiss only the picker so the user
            // doesn't accidentally cancel the whole import. Wrapped
            // in `iced::widget::opaque` so hover events stop here
            // instead of bleeding through to the dashboard cards
            // beneath the modal (otherwise iced's Stack lets mouse
            // hover propagate to lower layers, lighting up rows
            // under the cursor while the modal is open).
            let on_scrim_click = if self.cloud_discover_default_group_picker_open {
                Message::ToggleCloudDiscoverGroupPicker
            } else {
                Message::CloudDiscoverImportCancelled
            };
            let scrim: Element<'_, Message> = iced::widget::opaque(
                MouseArea::new(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Background::Color(Color::from_rgba(
                                0.0, 0.0, 0.0, 0.5,
                            ))),
                            ..Default::default()
                        }),
                )
                .on_press(on_scrim_click),
            );

            // Group-picker context menu: same pattern as the
            // existing kebab menus. Built via the global
            // `OverlayState` + `render_overlay_menu` pipeline so the
            // menu styling, backdrop, and dismiss-on-click-outside
            // all behave like every other context menu in the app.
            // Injected here (inside the modal's Stack) because the
            // modal short-circuits the global overlay path further
            // down in `view_main`.
            let mut modal_stack =
                Stack::new().push(base).push(scrim).push(centered);
            if let Some(ref ovl) = self.overlay
                && matches!(ovl.content, OverlayContent::CloudDiscoverGroupPicker)
            {
                let menu = self.render_overlay_menu(ovl);
                // Width matches the combo's measured width from the
                // bounds_reporter (falls back to 308 on the very
                // first open when the cell is still zeroed). Height
                // clamp keeps tall menus on-screen.
                let combo = self.cloud_discover_default_group_combo_bounds.get();
                let menu_width = if combo.width > 0.0 { combo.width } else { 308.0 };
                let menu_height = 280.0_f32;
                let x = ovl
                    .x
                    .min((self.window_size.width - menu_width).max(0.0))
                    .max(0.0);
                let y = ovl
                    .y
                    .min((self.window_size.height - menu_height).max(0.0))
                    .max(0.0);
                let backdrop: Element<'_, Message> = MouseArea::new(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .on_press(Message::ToggleCloudDiscoverGroupPicker)
                .into();
                let positioned: Element<'_, Message> = column![
                    Space::new().height(y),
                    row![
                        Space::new().width(x),
                        container(menu).width(Length::Fixed(menu_width)),
                    ],
                ]
                .into();
                modal_stack = modal_stack.push(backdrop).push(positioned);
            }

            return wrap_with_resize(
                modal_stack
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Folder rename modal, shown after the user picks "Rename" from
        // the folder context menu.
        if let Some((_gid, ref input)) = self.folder_rename {
            let dialog = container(
                column![
                    text(crate::i18n::t("rename_folder"))
                        .size(16)
                        .color(OryxisColors::t().text_primary)
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                    Space::new().height(12),
                    text_input(crate::i18n::t("folder_name"), input.as_str())
                        .on_input(Message::FolderRenameInput)
                        .on_submit(Message::ConfirmRenameFolder)
                        .padding(10)
                        .width(Length::Fill)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                    Space::new().height(12),
                    dir_row(vec![
                        styled_button(crate::i18n::t("save"), Message::ConfirmRenameFolder, OryxisColors::t().accent),
                        Space::new().width(8).into(),
                        styled_button(crate::i18n::t("cancel"), Message::CancelFolderModal, OryxisColors::t().text_muted),
                    ]),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x())
                .padding(24),
            )
            .width(Length::Fixed(360.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog.into(),
                    Some(Message::CancelFolderModal),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Folder delete confirmation, three-way choice instead of a yes/no
        // since destroying hosts vs only the folder are very different
        // intentions and deserve explicit affordances.
        if let Some(gid) = self.folder_delete {
            let folder_name = self
                .groups
                .iter()
                .find(|g| g.id == gid)
                .map(|g| g.label.clone())
                .unwrap_or_default();
            let host_count = self
                .connections
                .iter()
                .filter(|c| c.group_id == Some(gid))
                .count();
            let c = OryxisColors::t();

            // Tinted circular warning badge anchoring the dialog.
            let badge = container(
                iced_fonts::lucide::triangle_alert().size(22).color(c.error),
            )
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(48.0))
            .center_x(Length::Fixed(48.0))
            .center_y(Length::Fixed(48.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(Color { a: 0.12, ..c.error })),
                border: Border { radius: Radius::from(24.0), ..Default::default() },
                ..Default::default()
            });

            // Subtitle: the folder name, plus the host count when it carries
            // any (an empty folder has nothing to qualify).
            let subtitle = if host_count == 0 {
                format!("\"{}\"", folder_name)
            } else {
                format!("\"{}\"  ·  {}", folder_name, crate::i18n::host_count(host_count))
            };
            let header = column![
                badge,
                Space::new().height(14),
                text(crate::i18n::t("delete_folder_question"))
                    .size(17)
                    .font(iced::Font {
                        weight: iced::font::Weight::Semibold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(c.text_primary),
                Space::new().height(6),
                text(subtitle)
                    .size(12)
                    .color(c.text_muted)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            ]
            .width(Length::Fill)
            .align_x(iced::Alignment::Center);

            // Empty folders have no hosts to move or destroy, so the
            // three-way choice collapses to a single, honest "remove the
            // folder" action.
            let actions = if host_count == 0 {
                column![folder_choice_card(
                    iced_fonts::lucide::trash(),
                    crate::i18n::t("delete_folder_empty"),
                    crate::i18n::t("delete_folder_empty_desc"),
                    Message::DeleteFolderWithHosts,
                    c.error,
                )]
            } else {
                column![
                    folder_choice_card(
                        iced_fonts::lucide::folder_open(),
                        crate::i18n::t("delete_folder_keep_hosts"),
                        crate::i18n::t("delete_folder_keep_hosts_desc"),
                        Message::DeleteFolderKeepHosts,
                        c.accent,
                    ),
                    Space::new().height(10),
                    folder_choice_card(
                        iced_fonts::lucide::trash(),
                        crate::i18n::t("delete_folder_with_hosts"),
                        crate::i18n::t("delete_folder_with_hosts_desc"),
                        Message::DeleteFolderWithHosts,
                        c.error,
                    ),
                ]
            }
            .width(Length::Fill);

            let dialog = container(
                column![
                    header,
                    Space::new().height(20),
                    actions,
                    Space::new().height(14),
                    ghost_button(crate::i18n::t("cancel"), Message::CancelFolderModal),
                ]
                .width(Length::Fill)
                .padding(24),
            )
            .width(Length::Fixed(400.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(c.bg_surface)),
                border: Border { radius: Radius::from(14.0), color: c.border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog.into(),
                    Some(Message::CancelFolderModal),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // "Clear all" confirmation for the Logs view: states exactly
        // what gets wiped (recordings + connection events) before the
        // irreversible ClearLogs runs.
        if self.clear_history_confirm {
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
                        styled_button(
                            crate::i18n::t("cancel"),
                            Message::CancelClearHistory,
                            OryxisColors::t().text_muted,
                        ),
                        Space::new().width(8).into(),
                        styled_button(
                            crate::i18n::t("clear_all"),
                            Message::ClearLogs,
                            OryxisColors::t().error,
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

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog.into(),
                    Some(Message::CancelClearHistory),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // New-tab picker (opens via the "+" button in the tab bar).
        if self.show_new_tab_picker {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_new_tab_picker(),
                    Some(Message::HideNewTabPicker),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Tab-jump modal, Termius-style "Jump to" list. Opens via the
        // ⋯ button in the tab bar or the global Ctrl+J shortcut.
        if self.show_tab_jump {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_tab_jump_modal(),
                    Some(Message::HideTabJump),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Icon/color picker (from the host editor). Intentionally NOT routed
        // through `widgets::modal_overlay`: it injects a color-popover layer
        // into its own Stack, which the simple helper can't host. Stays
        // mouse-safe via `opaque` and keyboard-safe via `any_modal_blocks_input`.
        if self.show_icon_picker {
            let picker = self.view_icon_picker();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(picker)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Chain editor (from the host editor's "Host Chaining" row). Scrim
        // dismiss is context-dependent: pop the add-a-hop sub-view first,
        // else close the editor (mirrors Esc).
        if self.show_chain_editor {
            let on_scrim = if self.chain_editor_adding {
                Message::ChainEditorCancelAdd
            } else {
                Message::CloseChainEditor
            };
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_chain_editor(),
                    Some(on_scrim),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Per-host terminal theme picker (from the host editor).
        if self.show_theme_picker {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_terminal_theme_picker(),
                    Some(Message::EditorCloseThemePicker),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Custom terminal theme editor (from the "+" card / edit affordance
        // in Settings -> Terminal). Exempt from `modal_overlay` (nested color
        // popover in its own Stack); mouse-safe via `opaque`, keyboard-safe
        // via `any_modal_blocks_input`.
        if self.theme_editor.is_some() {
            let editor = self.view_theme_editor_modal();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(editor)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Import-a-scheme modal (Settings -> Terminal "Import" card).
        if self.show_theme_import {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_theme_import_modal(),
                    Some(Message::ThemeImportClose),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Custom UI (chrome) theme editor (Settings -> Interface). Exempt
        // from `modal_overlay` (nested color popover in its own Stack);
        // mouse-safe via `opaque`, keyboard-safe via `any_modal_blocks_input`.
        if self.ui_theme_editor.is_some() {
            let editor = self.view_ui_theme_editor_modal();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(editor)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Note: the update modal is rendered at the top-level `view()`
        // dispatcher (see `Oryxis::view`) so it overlays the lock screen
        // too. Don't re-render it here.

        if let Some(ref overlay) = self.overlay {
            let menu = self.render_overlay_menu(overlay);

            // The `+` split popover is hover-driven: it opens on hover and
            // dismisses on mouse-out (`SplitMenuLeave`), so a click-dismiss
            // backdrop is redundant for it. Worse, a full-screen backdrop sits
            // on top of the `+` button and swallows the click, so the first
            // click on `+` only closes the popover and a second is needed to
            // open a new tab. Skip the backdrop here so the click reaches the
            // button. Every other overlay through this path is click-triggered
            // and keeps its click-outside dismissal.
            let is_hover_popover = matches!(overlay.content, OverlayContent::SplitMenu);

            // Position the menu, clamping to window bounds to prevent clipping.
            // Under RTL, anchor by the menu's right edge so it grows toward
            // the leading (left) side, mirroring native OS dropdown behavior.
            // Width must match the value used in `render_overlay_menu` so
            // clamping lines up with the rendered box.
            let menu_width = self.overlay_menu_width(overlay);
            let menu_height = 80.0_f32; // approximate menu height
            let raw_x = if crate::i18n::is_rtl_layout() {
                overlay.x - menu_width
            } else {
                overlay.x
            };
            let x = raw_x.min(self.window_size.width - menu_width).max(0.0);
            let y = overlay.y.min(self.window_size.height - menu_height).max(0.0);
            let positioned_menu: Element<'_, Message> = column![
                Space::new().height(y),
                row![
                    Space::new().width(x),
                    menu,
                ],
            ]
            .into();

            let mut stack = Stack::new().push(base);
            if !is_hover_popover {
                // Transparent backdrop that dismisses the menu on click.
                let backdrop: Element<'_, Message> = MouseArea::new(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .on_press(Message::HideOverlayMenu)
                .into();
                stack = stack.push(backdrop);
            }
            return wrap_with_resize(
                stack
                    .push(positioned_menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // SFTP row right-click menu, rendered at the layout root so the
        // window-coord click position lines up with the menu origin
        // without having to compensate for the title + tab bar height.
        if let Some(ref row_menu) = self.sftp.row_menu {
            // "Cross-pane action available" = the pane opposite the
            // right-clicked row is connected (remote with a client) or is
            // a local destination. The row menu uses this to decide
            // whether to offer Upload / Download / Relay.
            let other_side = if row_menu.side == crate::state::SftpPaneSide::Left {
                crate::state::SftpPaneSide::Right
            } else {
                crate::state::SftpPaneSide::Left
            };
            let other = self.sftp.pane(other_side);
            let cross_pane_ready = if other.is_remote {
                other.client.is_some()
            } else {
                true
            };
            let other_is_remote = other.is_remote;
            let src_pane = self.sftp.pane(row_menu.side);
            let source_is_remote = src_pane.is_remote;
            let other_label = other.host_label.clone();
            // Current directory of the source pane + its local path, fed to
            // the directory-level actions (Refresh / New / Open in FM).
            let pane_dir = if source_is_remote {
                src_pane.remote_path.clone()
            } else {
                src_pane.local_path.to_string_lossy().into_owned()
            };
            let local_dir = src_pane.local_path.clone();
            let show_hidden = src_pane.show_hidden;
            // Count of selected rows in the same pane as the right-
            // clicked row, drives the bulk vs single menu mode.
            let selection_count_same_pane = self
                .sftp
                .selected_rows
                .iter()
                .filter(|(s, _)| *s == row_menu.side)
                .count();
            let menu = crate::views::sftp::row_context_menu_box(
                row_menu,
                cross_pane_ready,
                source_is_remote,
                other_is_remote,
                other_label,
                selection_count_same_pane,
                crate::views::sftp::DirActionCtx {
                    pane_dir: &pane_dir,
                    local_dir: &local_dir,
                    show_hidden,
                },
            );
            let backdrop: Element<'_, Message> = MouseArea::new(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::SftpRowMenuClose)
            .into();
            // Nudge the menu a few px down/right so it doesn't sit
            // directly under the cursor, feels like the OS-native menu
            // anchoring.
            let menu_width = crate::views::sftp::ROW_CONTEXT_MENU_WIDTH;
            let rtl = crate::i18n::is_rtl_layout();
            // Under RTL, nudge toward the leading side so the menu grows
            // left-from-cursor instead of right-from-cursor.
            let nudged_x = if rtl {
                row_menu.x - 2.0 - menu_width
            } else {
                row_menu.x + 2.0
            };
            let nudged_y = row_menu.y + 2.0;
            let menu_height = crate::views::sftp::row_context_menu_height(
                row_menu,
                cross_pane_ready,
                source_is_remote,
                other_is_remote,
                selection_count_same_pane,
            );
            let x = nudged_x
                .min(self.window_size.width - menu_width)
                .max(0.0);
            let y = nudged_y
                .min(self.window_size.height - menu_height)
                .max(0.0);
            let positioned_menu: Element<'_, Message> = column![
                Space::new().height(y),
                row![Space::new().width(x), menu],
            ]
            .into();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(backdrop)
                    .push(positioned_menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Floating drag ghost, rendered last so it sits above
        // everything else. Tracks the cursor while a cross-pane SFTP
        // drag is in flight; non-interactive so it doesn't swallow the
        // release event that ends the drag.
        if let Some(drag) = &self.sftp.drag
            && drag.active
        {
            let ghost = crate::views::sftp::drag_ghost(&drag.label);
            // Offset slightly off the cursor, matches OS drag previews
            // and keeps the label out from under the pointer. Direction
            // mirrors under RTL so the ghost trails the cursor on the
            // leading side instead of running off-screen at the edge.
            let ghost_width = 200.0_f32;
            let x_offset = if crate::i18n::is_rtl_layout() {
                -ghost_width - 12.0
            } else {
                12.0
            };
            let x = (self.mouse_position.x + x_offset)
                .min(self.window_size.width - ghost_width)
                .max(0.0);
            let y = (self.mouse_position.y + 12.0)
                .min(self.window_size.height - 40.0)
                .max(0.0);
            let positioned: Element<'_, Message> = column![
                Space::new().height(y),
                row![Space::new().width(x), ghost],
            ]
            .into();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(positioned)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // No modal open. Wrap `base` in a single-child Stack so it sits
        // at exactly the same tree position as in the modal branches
        // above (every one of which passes `Stack::new().push(base)
        // .push(modal)` as the content). iced keys scrollable offsets by
        // tree position, not by Id, so if `base`'s depth shifted when a
        // modal opened (bare `base` here vs. nested under a Stack there)
        // every scrollable inside it (host list, editor form, ...) would
        // reset to the top. Keeping the depth constant preserves them.
        wrap_with_resize(
            Stack::new()
                .push(base)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            resize_overlay,
        )
    }

    /// Browser-style immersive-mode overlays: on-enter hint banner and
    /// hover-only round X close button. Stacked on top of whatever the
    /// caller passed so they never get hidden by content underneath.
    /// The X only renders when the mouse sits in the top 60 px so the
    /// affordance is discoverable but unobtrusive once the user gets
    /// used to F11.
    pub(crate) fn layer_fullscreen_overlays<'a>(
        &'a self,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        const TOP_HOVER_ZONE: f32 = 60.0;
        const HINT_BANNER_HEIGHT: f32 = 32.0;
        let in_top_zone = self.mouse_position.y < TOP_HOVER_ZONE;

        let mut layers = Stack::new()
            .push(content)
            .width(Length::Fill)
            .height(Length::Fill);

        if self.fullscreen_hint_visible {
            let hint = container(
                text(crate::i18n::t("fullscreen_exit_hint"))
                    .size(12)
                    .color(OryxisColors::t().text_primary),
            )
            .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.92,
                    ..OryxisColors::t().bg_selected
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
            let centered = column![
                Space::new().height(12.0),
                container(hint).center_x(Length::Fill),
                Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill);
            layers = layers.push(centered);
        }

        if in_top_zone {
            // Round 28×28 button with the lucide `x` glyph centered.
            // Clicking toggles fullscreen off (same Message as F11).
            // Anchored top-center with a small top inset; when the
            // hint banner is also visible the button sits below it
            // so the two affordances don't overlap.
            let close_btn = button(
                container(
                    iced_fonts::lucide::x::<iced::Theme, iced::Renderer>()
                        .size(14)
                        .color(OryxisColors::t().button_text),
                )
                .center(Length::Fixed(28.0)),
            )
            .on_press(Message::WindowFullscreenToggle)
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => OryxisColors::t().error,
                    _ => Color {
                        a: 0.85,
                        ..OryxisColors::t().bg_selected
                    },
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(14.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });
            let top_offset = if self.fullscreen_hint_visible {
                12.0 + HINT_BANNER_HEIGHT + 8.0
            } else {
                12.0
            };
            let positioned = column![
                Space::new().height(top_offset),
                container(close_btn).center_x(Length::Fill),
                Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill);
            layers = layers.push(positioned);
        }

        layers.into()
    }
}
