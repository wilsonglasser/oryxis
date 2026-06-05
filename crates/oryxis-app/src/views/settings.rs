//! Settings screen, terminal, AI, theme, shortcuts, security, sync, about.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::mcp::mcp_info_panel;
use crate::state::SettingsSection;
use crate::theme::OryxisColors;
use crate::widgets::{
    dir_align_x, dir_row, key_badge, panel_field, panel_section, settings_row, shortcut_row,
    styled_button, toggle_row,
};

impl Oryxis {
    pub(crate) fn view_settings(&self) -> Element<'_, Message> {
        // ── Settings sidebar ──
        let settings_sidebar = {
            // Order: most-touched at the top (visual + everyday
            // configuration), then per-feature toggles, then network
            // resources, then plugin / system / about. The previous
            // order was historical (followed the implementation
            // sequence) and didn't reflect how users actually move
            // through the panel.
            let items: Vec<(&str, SettingsSection)> = vec![
                (crate::i18n::t("interface"), SettingsSection::Interface),
                (crate::i18n::t("terminal_settings"), SettingsSection::Terminal),
                (crate::i18n::t("ai_assistant"), SettingsSection::AI),
                ("SFTP", SettingsSection::Sftp),
                (crate::i18n::t("sync"), SettingsSection::Sync),
                (crate::i18n::t("cloud_accounts"), SettingsSection::Cloud),
                (crate::i18n::t("proxies"), SettingsSection::Proxies),
                (crate::i18n::t("known_hosts"), SettingsSection::KnownHosts),
                (crate::i18n::t("mcp_server"), SettingsSection::Mcp),
                (crate::i18n::t("plugins"), SettingsSection::Plugins),
                (crate::i18n::t("shortcuts"), SettingsSection::Shortcuts),
                (crate::i18n::t("security"), SettingsSection::Security),
                (crate::i18n::t("about"), SettingsSection::About),
            ];
            let mut col = column![]
                .padding(Padding { top: 12.0, right: 8.0, bottom: 8.0, left: 8.0 });

            for (label, section) in items {
                let is_active = self.settings_section == section;
                let bg = if is_active {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else {
                    Color::TRANSPARENT
                };
                let fg = if is_active {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().text_secondary
                };
                let btn: Element<'_, Message> = button(
                    container(text(label).size(13).color(fg))
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
                )
                .on_press(Message::ChangeSettingsSection(section))
                .width(Length::Fill)
                .style(move |_, status| {
                    let hover_bg = match status {
                        BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                        BtnStatus::Pressed => Color { a: 0.25, ..OryxisColors::t().accent },
                        _ => bg,
                    };
                    button::Style {
                        background: Some(Background::Color(hover_bg)),
                        border: Border { radius: Radius::from(10.0), ..Default::default() },
                        ..Default::default()
                    }
                })
                .into();
                col = col.push(btn);
            }

            // Wrap the panel in a row so we can stick a 1 px hairline on the
            // right edge only, iced's Border applies to all four sides at
            // once, so we compose the single-edge separator instead.
            let right_hairline = container(Space::new().width(1))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                });
            // Wrap the section list in a scrollable so a short window
            // doesn't clip the bottom entries (About / Plugins were
            // disappearing when the height dropped below ~520 px).
            let panel = container(scrollable(col).height(Length::Fill))
                .width(200)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                    ..Default::default()
                });
            // `dir_row` flips the panel + hairline pair under RTL so the
            // hairline always sits on the inner edge (between sidebar and
            // content), regardless of which side the sidebar lands on.
            crate::widgets::dir_row(vec![panel.into(), right_hairline.into()])
                .height(Length::Fill)
        };

        // ── Settings content ──
        let settings_content: Element<'_, Message> = match self.settings_section {
            SettingsSection::Terminal => {
                let mut toggles_col: iced::widget::Column<'_, Message> = column![
                    toggle_row(crate::i18n::t("copy_on_select"), self.setting_copy_on_select, Message::ToggleCopyOnSelect),
                ];
                // Sub-option, only meaningful while copy-on-select is on.
                // Indent it on the leading edge so it reads as nested.
                if self.setting_copy_on_select {
                    let indent = if crate::i18n::is_rtl_layout() {
                        Padding { right: 22.0, ..Padding::ZERO }
                    } else {
                        Padding { left: 22.0, ..Padding::ZERO }
                    };
                    toggles_col = toggles_col
                        .push(Space::new().height(8))
                        .push(
                            container(toggle_row(
                                crate::i18n::t("copy_requires_right_click"),
                                self.setting_right_click_copy,
                                Message::ToggleRightClickCopy,
                            ))
                            .padding(indent),
                        );
                }
                let toggles_section = panel_section(
                    toggles_col
                        .push(Space::new().height(10))
                        .push(toggle_row(crate::i18n::t("bold_bright"), self.setting_bold_is_bright, Message::ToggleBoldIsBright))
                        .push(Space::new().height(10))
                        .push(toggle_row(crate::i18n::t("keyword_highlight"), self.setting_keyword_highlight, Message::ToggleKeywordHighlight))
                        .push(Space::new().height(10))
                        .push(toggle_row(crate::i18n::t("smart_contrast"), self.setting_smart_contrast, Message::ToggleSmartContrast)),
                );

                let font_size_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("terminal_font_size")).size(13).color(OryxisColors::t().text_primary).into(),
                        Space::new().width(Length::Fill).into(),
                        button(
                            container(text("\u{2212}").size(14).color(OryxisColors::t().text_primary))
                                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
                        )
                        .on_press(Message::TerminalFontSizeDecrease)
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => OryxisColors::t().bg_selected,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                ..Default::default()
                            }
                        }).into(),
                        Space::new().width(8).into(),
                        text(format!("{:.0}", self.terminal_font_size)).size(13).color(OryxisColors::t().text_primary).into(),
                        Space::new().width(8).into(),
                        button(
                            container(text("+").size(14).color(OryxisColors::t().text_primary))
                                .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
                        )
                        .on_press(Message::TerminalFontSizeIncrease)
                        .style(|_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => OryxisColors::t().bg_selected,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                ..Default::default()
                            }
                        }).into(),
                    ]).align_y(iced::Alignment::Center),
                ]);

                let keepalive_section = panel_section(column![
                    text(crate::i18n::t("keepalive_interval")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("setting_keepalive_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("30", &self.setting_keepalive_interval)
                        .on_input(Message::SettingKeepaliveChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                ]);

                let scrollback_section = panel_section(column![
                    text(crate::i18n::t("scrollback")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("setting_scrollback_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("10000", &self.setting_scrollback_rows)
                        .on_input(Message::SettingScrollbackChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                ]);

                // Terminal theme picker. First card is the "follow
                // app theme" sentinel (terminal_theme_override = None);
                // the rest are explicit palette previews so the user
                // can compare colours without applying each one. Per-host
                // overrides configured via the icon picker still win
                // over this global pick.
                let mut theme_cards: Vec<Element<'_, Message>> = Vec::new();
                theme_cards.push(crate::widgets::terminal_theme_inherit_card(
                    t("terminal_theme_follow_app"),
                    self.terminal_theme_override.is_none(),
                    Message::TerminalThemeChanged(String::new()),
                ));
                for theme in oryxis_terminal::TerminalTheme::ALL.iter() {
                    let is_selected = self
                        .terminal_theme_override
                        .as_deref()
                        == Some(theme.name());
                    theme_cards.push(crate::widgets::terminal_theme_card(
                        theme.palette(),
                        theme.name(),
                        is_selected,
                        Message::TerminalThemeChanged(theme.name().to_string()),
                    ));
                }
                // User-defined themes after the built-ins, each with the
                // hover edit / delete affordances.
                for (idx, ct) in self.custom_terminal_themes.iter().enumerate() {
                    let is_selected =
                        self.terminal_theme_override.as_deref() == Some(ct.name.as_str());
                    let palette = self
                        .terminal_palette_for_name(&ct.name)
                        .unwrap_or_default();
                    theme_cards.push(self.terminal_custom_theme_card(
                        idx,
                        &ct.name,
                        palette,
                        is_selected,
                    ));
                }
                // "+ New custom theme" + "Import" cards last.
                theme_cards.push(crate::views::settings_themes::terminal_theme_add_card());
                theme_cards.push(crate::views::settings_themes::terminal_theme_import_card());
                // 2-column responsive grid for theme cards. Cards still
                // use the existing swatch-+-name layout (the "bolinhas"
                // style); only the row arrangement changes from a single
                // tall column to a side-by-side pair so the picker
                // doesn't dominate the settings panel vertically.
                let theme_grid = crate::widgets::distribute_card_grid(
                    theme_cards,
                    2,
                    8.0,
                    8.0,
                );
                let theme_picker_section = panel_section(column![
                    text(t("terminal_theme")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("terminal_theme_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(10),
                    theme_grid,
                ]);

                // Font picker. The list comes from a live fontdb scan
                // of monospace families installed on the system, with a
                // hardcoded fallback when the scan returns nothing.
                let fonts: Vec<String> = crate::app::enumerate_terminal_fonts();
                let font_picker_section = panel_section(column![
                    text(crate::i18n::t("terminal_font")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("setting_font_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    pick_list(
                        Some(self.terminal_font_name.clone()),
                        fonts,
                        |s: &String| s.clone(),
                    )
                    .on_select(Message::TerminalFontChanged)
                    .width(260).padding(10).style(crate::widgets::rounded_pick_list_style),
                ]);

                let auto_update_enabled = self.setting_auto_check_updates;
                let current_version_line = text(format!(
                    "{} {}", t("current_version"), env!("CARGO_PKG_VERSION"),
                ))
                .size(11)
                .color(OryxisColors::t().text_muted);
                let check_now_btn = styled_button(
                    t("check_for_updates_now"),
                    Message::CheckForUpdateManual,
                    OryxisColors::t().accent,
                );
                let status_line: Element<'_, Message> = match &self.update_check_status {
                    Some(msg) => {
                        let is_checking = msg == "Checking\u{2026}";
                        let color = if is_checking {
                            OryxisColors::t().text_muted
                        } else if msg.starts_with("You're") {
                            OryxisColors::t().success
                        } else {
                            OryxisColors::t().error
                        };
                        container(text(msg.clone()).size(11).color(color))
                            .padding(Padding { top: 8.0, right: 0.0, bottom: 0.0, left: 0.0 })
                            .into()
                    }
                    None => Space::new().height(0).into(),
                };
                let channel_picker = pick_list(
                    Some(self.setting_update_channel),
                    crate::update::UPDATE_CHANNELS.to_vec(),
                    |c: &crate::update::UpdateChannel| match c {
                        crate::update::UpdateChannel::Stable => t("update_channel_stable").to_string(),
                        crate::update::UpdateChannel::Nightly => t("update_channel_nightly").to_string(),
                    },
                )
                .on_select(Message::SettingUpdateChannelChanged)
                .width(260)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style);
                // Bleeding-edge warning, only while the nightly channel is
                // selected, so stable users don't see scary copy.
                let channel_note: Element<'_, Message> =
                    if self.setting_update_channel == crate::update::UpdateChannel::Nightly {
                        container(
                            text(t("update_channel_nightly_warning"))
                                .size(11)
                                .color(OryxisColors::t().text_muted),
                        )
                        .padding(Padding { top: 4.0, right: 0.0, bottom: 0.0, left: 0.0 })
                        .into()
                    } else {
                        Space::new().height(0).into()
                    };
                let auto_update_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("auto_check_updates"),
                        auto_update_enabled,
                        Message::SettingToggleAutoCheckUpdates,
                    ),
                    Space::new().height(4),
                    text(t("setting_update_check_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(12),
                    text(t("update_channel")).size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    channel_picker,
                    channel_note,
                    Space::new().height(10),
                    current_version_line,
                    Space::new().height(8),
                    check_now_btn,
                    status_line,
                ]);

                let os_detection_enabled = self.setting_os_detection;
                let os_detection_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("os_detection"),
                        os_detection_enabled,
                        Message::SettingToggleOsDetection,
                    ),
                    Space::new().height(4),
                    text(t("setting_os_detect_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                ]);

                let auto_reconnect_enabled = self.setting_auto_reconnect;
                let auto_reconnect_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("auto_reconnect"),
                        auto_reconnect_enabled,
                        Message::SettingToggleAutoReconnect,
                    ),
                    Space::new().height(4),
                    text(t("setting_reconnect_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text(crate::i18n::t("max_reconnect_attempts")).size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text_input("5", &self.setting_max_reconnect_attempts)
                        .on_input(Message::SettingMaxReconnectChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                ]);

                scrollable(
                    container(
                        column![
                            text(crate::i18n::t("terminal_settings")).size(18).color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            toggles_section,
                            Space::new().height(12),
                            keepalive_section,
                            Space::new().height(12),
                            scrollback_section,
                            Space::new().height(12),
                            auto_reconnect_section,
                            Space::new().height(12),
                            os_detection_section,
                            Space::new().height(12),
                            auto_update_section,
                            Space::new().height(12),
                            font_size_section,
                            Space::new().height(12),
                            font_picker_section,
                            Space::new().height(12),
                            theme_picker_section,
                            Space::new().height(24),
                        ]
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Sftp => {
                let concurrency_section = panel_section(column![
                    text(t("transfer_parallelism"))
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("setting_sftp_concurrency_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("2", &self.setting_sftp_concurrency)
                        .on_input(Message::SettingSftpConcurrencyChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                ]);

                let timeout_input = |label: &str, hint: &str, value: &str, on_input: fn(String) -> Message| {
                    panel_section(column![
                        text(label.to_string())
                            .size(13)
                            .color(OryxisColors::t().text_primary),
                        Space::new().height(4),
                        text(hint.to_string())
                            .size(11)
                            .color(OryxisColors::t().text_muted),
                        Space::new().height(8),
                        text_input("0", value)
                            .on_input(on_input)
                            .padding(10)
                            .width(240)
                            .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                    ])
                };

                let connect_section = timeout_input(
                    t("connect_timeout"),
                    t("connect_timeout_desc"),
                    &self.setting_sftp_connect_timeout,
                    Message::SettingSftpConnectTimeoutChanged,
                );
                let auth_section = timeout_input(
                    t("auth_timeout"),
                    t("auth_timeout_desc"),
                    &self.setting_sftp_auth_timeout,
                    Message::SettingSftpAuthTimeoutChanged,
                );
                let session_section = timeout_input(
                    t("channel_open_timeout"),
                    t("channel_open_timeout_desc"),
                    &self.setting_sftp_session_timeout,
                    Message::SettingSftpSessionTimeoutChanged,
                );
                let op_section = timeout_input(
                    t("operation_timeout"),
                    t("operation_timeout_desc"),
                    &self.setting_sftp_op_timeout,
                    Message::SettingSftpOpTimeoutChanged,
                );

                let enable_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("enable_sftp"),
                        self.sftp_enabled,
                        Message::SettingToggleSftpEnabled,
                    ),
                ]);

                // Tuning knobs (parallelism, timeouts) only render
                // when SFTP is enabled, matching the AI / Sync
                // sections' pattern: a single master toggle stays
                // visible at the top, the rest collapses when off.
                let mut content_col: iced::widget::Column<'_, Message> = column![
                    text(t("sftp"))
                        .size(18)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    enable_section,
                ]
                .width(Length::Fill)
                .align_x(dir_align_x());

                if self.sftp_enabled {
                    content_col = content_col
                        .push(Space::new().height(12))
                        .push(concurrency_section)
                        .push(Space::new().height(12))
                        .push(connect_section)
                        .push(Space::new().height(12))
                        .push(auth_section)
                        .push(Space::new().height(12))
                        .push(session_section)
                        .push(Space::new().height(12))
                        .push(op_section);
                }
                content_col = content_col.push(Space::new().height(24));

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::AI => {
                let enable_section = panel_section(column![
                    toggle_row(crate::i18n::t("enable_ai"), self.ai_enabled, Message::ToggleAiEnabled),
                ]);

                // Use explicit `Space::new()` between elements (no
                // `.spacing()`) so the gap before the first panel
                // matches the SFTP section's 16 px exactly; the
                // previous `.spacing(12)` was stacking on top of the
                // explicit 16 to give a ~40 px gap.
                let mut content_col = column![
                    text(crate::i18n::t("ai_assistant")).size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    enable_section,
                    Space::new().height(8),
                    // The assistant runs commands on connected servers
                    // (some auto-execute); warn before / while enabled.
                    text(crate::i18n::t("ai_enable_warning")).size(12).color(OryxisColors::t().text_muted),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x());

                if self.ai_enabled {
                    let current_info = crate::ai::provider_info(&self.ai_provider);
                    let provider_options: Vec<String> = crate::ai::PROVIDERS
                        .iter()
                        .map(|p| p.display.to_string())
                        .collect();

                    let provider_pick: Element<'_, Message> = pick_list(
                        Some(current_info.display.to_string()),
                        provider_options,
                        |s: &String| s.clone(),
                    )
                    .on_select(Message::AiProviderChanged)
                    .width(220)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .into();

                    let model_input: Element<'_, Message> = text_input(t("ai_model_placeholder"), &self.ai_model)
                        .on_input(Message::AiModelChanged)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into();

                    // When a key is already stored, the input is cleared
                    // for security but the placeholder communicates that
                    // a key exists, typing replaces it on save.
                    let key_placeholder = if self.ai_api_key_set {
                        "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022} saved, type to replace"
                    } else {
                        "sk-..."
                    };
                    let key_input: Element<'_, Message> = text_input(key_placeholder, &self.ai_api_key)
                        .on_input(Message::AiApiKeyChanged)
                        .on_submit(Message::SaveAiApiKey)
                        .secure(true)
                        .padding(10)
                        .width(280)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .into();
                    let save_btn = styled_button("Save", Message::SaveAiApiKey, OryxisColors::t().accent);
                    let key_status: Element<'_, Message> = if self.ai_api_key_set {
                        dir_row(vec![
                            iced_fonts::lucide::circle_check().size(13).color(OryxisColors::t().success).into(),
                            Space::new().width(6).into(),
                            text(t("api_key_saved")).size(12).color(OryxisColors::t().success).into(),
                        ])
                        .align_y(iced::Alignment::Center)
                        .into()
                    } else {
                        dir_row(vec![
                            iced_fonts::lucide::circle_alert().size(13).color(OryxisColors::t().text_muted).into(),
                            Space::new().width(6).into(),
                            text(t("no_api_key")).size(12).color(OryxisColors::t().text_muted).into(),
                        ])
                        .align_y(iced::Alignment::Center)
                        .into()
                    };

                    let mut provider_col = column![
                        panel_field(t("provider"), provider_pick),
                        Space::new().height(12),
                        panel_field(t("model"), model_input),
                    ];

                    if current_info.kind == crate::ai::ProviderKind::Custom {
                        let url_input: Element<'_, Message> = text_input("https://api.example.com/v1/chat/completions", &self.ai_api_url)
                            .on_input(Message::AiApiUrlChanged)
                            .padding(10)
                            .width(300)
                            .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                            .into();
                        provider_col = provider_col
                            .push(Space::new().height(12))
                            .push(panel_field("API URL", url_input));
                    }

                    provider_col = provider_col
                        .push(Space::new().height(12))
                        .push(panel_field(
                            "API Key",
                            dir_row(vec![key_input, Space::new().width(8).into(), save_btn])
                                .align_y(iced::Alignment::Center)
                                .into(),
                        ))
                        .push(Space::new().height(4))
                        .push(key_status);

                    content_col = content_col
                        .push(Space::new().height(12))
                        .push(panel_section(provider_col));

                    // System prompt, multi-line editor that grows with the
                    // content. `Length::Shrink` lets the editor auto-resize
                    // to fit its text, capped by the panel's scroll area.
                    let prompt_editor: Element<'_, Message> = iced::widget::text_editor(&self.ai_system_prompt)
                        .placeholder(t("ai_system_prompt_placeholder"))
                        .on_action(Message::AiSystemPromptAction)
                        .padding(10)
                        .height(Length::Shrink)
                        .style(|_theme, status| {
                            let c = OryxisColors::t();
                            let (border_color, border_width) = match status {
                                iced::widget::text_editor::Status::Focused { .. } => (c.accent, 1.5),
                                _ => (c.border, 1.0),
                            };
                            iced::widget::text_editor::Style {
                                background: iced::Background::Color(c.bg_surface),
                                border: iced::Border {
                                    radius: iced::border::Radius::from(crate::widgets::INPUT_RADIUS),
                                    width: border_width,
                                    color: border_color,
                                },
                                placeholder: c.text_muted,
                                value: c.text_primary,
                                selection: c.accent,
                            }
                        })
                        .into();
                    let prompt_section = panel_section(column![
                        panel_field(t("additional_system_prompt"), prompt_editor),
                        Space::new().height(4),
                        text(t("ai_system_prompt_desc"))
                            .size(11).color(OryxisColors::t().text_muted),
                    ]);
                    content_col = content_col
                        .push(Space::new().height(12))
                        .push(prompt_section);
                }

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Interface => {
                use crate::theme::AppTheme;
                let active_name = AppTheme::active().name();

                let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
                let themes: Vec<&AppTheme> = AppTheme::ALL.iter().collect();

                for chunk in themes.chunks(2) {
                    let mut cells: Vec<Element<'_, Message>> = Vec::new();
                    for theme in chunk {
                        let name = theme.name();
                        let is_active = name == active_name;
                        let colors = match theme {
                            AppTheme::OryxisDark => &crate::theme::ORYXIS_DARK,
                            AppTheme::OryxisLight => &crate::theme::ORYXIS_LIGHT,
                            AppTheme::Termius => &crate::theme::TERMIUS,
                            AppTheme::Darcula => &crate::theme::DARCULA,
                            AppTheme::IslandsDark => &crate::theme::ISLANDS_DARK,
                            AppTheme::Dracula => &crate::theme::DRACULA,
                            AppTheme::Monokai => &crate::theme::MONOKAI,
                            AppTheme::HackerGreen => &crate::theme::HACKER_GREEN,
                            AppTheme::Nord => &crate::theme::NORD,
                            AppTheme::NordLight => &crate::theme::NORD_LIGHT,
                            AppTheme::SolarizedDark => &crate::theme::SOLARIZED_DARK,
                            AppTheme::SolarizedLight => &crate::theme::SOLARIZED_LIGHT,
                            AppTheme::PaperLight => &crate::theme::PAPER_LIGHT,
                        };
                        let border_color = if is_active {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().border
                        };
                        let border_width = if is_active { 2.0 } else { 1.0 };

                        let preview_bg = colors.bg_primary;
                        let accent_bar = colors.accent;
                        let success_bar = colors.success;
                        let error_bar = colors.error;

                        let preview = container(
                            column![
                                Space::new().height(20),
                                row![
                                    container(Space::new().width(30).height(4))
                                        .style(move |_| container::Style {
                                            background: Some(Background::Color(accent_bar)),
                                            border: Border { radius: Radius::from(2.0), ..Default::default() },
                                            ..Default::default()
                                        }),
                                    Space::new().width(4),
                                    container(Space::new().width(20).height(4))
                                        .style(move |_| container::Style {
                                            background: Some(Background::Color(success_bar)),
                                            border: Border { radius: Radius::from(2.0), ..Default::default() },
                                            ..Default::default()
                                        }),
                                    Space::new().width(4),
                                    container(Space::new().width(15).height(4))
                                        .style(move |_| container::Style {
                                            background: Some(Background::Color(error_bar)),
                                            border: Border { radius: Radius::from(2.0), ..Default::default() },
                                            ..Default::default()
                                        }),
                                ].padding(Padding { top: 0.0, right: 8.0, bottom: 8.0, left: 8.0 }),
                            ],
                        )
                        .width(120)
                        .style(move |_| container::Style {
                            background: Some(Background::Color(preview_bg)),
                            border: Border { radius: Radius::from(6.0), ..Default::default() },
                            ..Default::default()
                        });

                        let card: Element<'_, Message> = button(
                            container(
                                column![
                                    preview,
                                    Space::new().height(8),
                                    text(name).size(12).color(OryxisColors::t().text_primary),
                                ]
                                .align_x(iced::Alignment::Center),
                            )
                            .padding(12),
                        )
                        .on_press(Message::AppThemeChanged(name.to_string()))
                        .width(Length::FillPortion(1))
                        .style(move |_, status| {
                            let bg = match status {
                                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                                _ => OryxisColors::t().bg_surface,
                            };
                            button::Style {
                                background: Some(Background::Color(bg)),
                                border: Border {
                                    radius: Radius::from(8.0),
                                    color: border_color,
                                    width: border_width,
                                },
                                ..Default::default()
                            }
                        })
                        .into();
                        cells.push(card);
                    }
                    // Fill remaining space if odd number
                    if chunk.len() == 1 {
                        cells.push(Space::new().width(Length::FillPortion(1)).into());
                    }
                    grid_rows.push(dir_row(cells).spacing(12).into());
                }

                // Language picker
                let lang_options: Vec<String> = crate::i18n::Language::ALL
                    .iter()
                    .map(|l| l.name().to_string())
                    .collect();
                let active_lang_name = crate::i18n::Language::active().name().to_string();

                let language_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("language")).size(13).color(OryxisColors::t().text_primary).into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(active_lang_name),
                            lang_options,
                            |s: &String| s.clone(),
                        )
                        .on_select(Message::LanguageChanged)
                        .width(200)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                ]);

                // Layout direction picker, Auto (follow language) by
                // default; explicit LTR/RTL overrides regardless of
                // language. Useful for users who want Persian text but a
                // familiar sidebar position.
                let dir_options: Vec<String> = crate::i18n::LayoutDirection::ALL
                    .iter()
                    .map(|d| crate::i18n::t(d.label_key()).to_string())
                    .collect();
                let active_dir_name = crate::i18n::t(
                    crate::i18n::LayoutDirection::active().label_key(),
                )
                .to_string();

                let layout_dir_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("layout_direction")).size(13).color(OryxisColors::t().text_primary).into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(active_dir_name),
                            dir_options,
                            |s: &String| s.clone(),
                        )
                        .on_select(Message::LayoutDirectionChanged)
                        .width(240)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                ]);

                let flatten_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("flatten_hosts_label"),
                        self.flatten_hosts,
                        Message::FlattenHostsToggle,
                    ),
                    Space::new().height(4),
                    text(crate::i18n::t("flatten_hosts_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]);

                let status_bar_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("show_status_bar"),
                        self.setting_show_status_bar,
                        Message::SettingToggleShowStatusBar,
                    ),
                    Space::new().height(8),
                    toggle_row(
                        crate::i18n::t("tab_accent_line"),
                        self.setting_tab_accent_line,
                        Message::SettingToggleTabAccentLine,
                    ),
                ]);

                // Tray toggles only mean something on Windows (the
                // tray module is a no-op on macOS/Linux). Hide the
                // whole section on those platforms so we don't dangle
                // settings the user can't actually exercise.
                let tray_section = panel_section(column![
                    text(crate::i18n::t("system_tray"))
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(8),
                    toggle_row(
                        crate::i18n::t("close_to_tray"),
                        self.setting_close_to_tray,
                        Message::SettingToggleCloseToTray,
                    ),
                    Space::new().height(4),
                    text(crate::i18n::t("close_to_tray_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(10),
                    toggle_row(
                        crate::i18n::t("minimize_to_tray"),
                        self.setting_minimize_to_tray,
                        Message::SettingToggleMinimizeToTray,
                    ),
                    Space::new().height(4),
                    text(crate::i18n::t("minimize_to_tray_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]);

                // Tab close button position picker. We use the token
                // strings ("left" / "right") as the picker's value type
                // and only translate to the localized display in the
                // `to_string` closure. The previous wiring used the
                // localized labels as values, so the on_select handler
                // always saw "Left"/"Right" (case + spelling locale-
                // dependent) and never matched the "right" arm.
                let close_options = vec![
                    "left".to_string(),
                    "right".to_string(),
                ];
                let tabs_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("close_button_position"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(self.setting_tab_close_button_side.clone()),
                            close_options,
                            |s: &String| {
                                crate::i18n::t(if s == "right" {
                                    "close_position_right"
                                } else {
                                    "close_position_left"
                                })
                                .to_string()
                            },
                        )
                        .on_select(Message::SettingTabCloseButtonSideChanged)
                        .width(160)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                    Space::new().height(8),
                    toggle_row(
                        crate::i18n::t("show_tab_status_dot"),
                        self.setting_show_tab_status_dot,
                        Message::SettingToggleShowTabStatusDot,
                    ),
                ]);

                // Layout mode picker: same token-as-value pattern as
                // the close-button picker. The display closure
                // translates the token to the localized label.
                let layout_options = vec![
                    "classic".to_string(),
                    "workspace".to_string(),
                ];
                let layout_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("layout_mode"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(self.setting_layout_mode.clone()),
                            layout_options,
                            |s: &String| {
                                crate::i18n::t(if s == "workspace" {
                                    "layout_mode_workspace"
                                } else {
                                    "layout_mode_classic"
                                })
                                .to_string()
                            },
                        )
                        .on_select(Message::SettingLayoutModeChanged)
                        .width(200)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                    Space::new().height(4),
                    text(crate::i18n::t("layout_mode_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]);

                // Default host icon picker: tokens drive the value,
                // localized labels come from `to_string`.
                let icon_options = vec![
                    "circular".to_string(),
                    "square".to_string(),
                    "rounded".to_string(),
                    "outline".to_string(),
                    "initials".to_string(),
                ];
                let icon_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("default_host_icon"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(self.setting_default_host_icon.clone()),
                            icon_options,
                            |s: &String| {
                                let key = match s.as_str() {
                                    "square" => "icon_square",
                                    "rounded" => "icon_rounded",
                                    "outline" => "icon_outline",
                                    "initials" => "icon_initials",
                                    _ => "icon_circular",
                                };
                                crate::i18n::t(key).to_string()
                            },
                        )
                        .on_select(Message::SettingDefaultHostIconChanged)
                        .width(200)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                ]);

                // Renderer backend picker + a hint that it only takes
                // effect after a restart (the backend is fixed at process
                // start). Escape hatch for GPU/driver stacks that corrupt
                // the wgpu surface: "auto" (best/Vulkan), "opengl" (still
                // GPU, dodges most Vulkan-on-Mesa bugs), "software" (CPU).
                // Token-as-value pattern: the picker stores the token and
                // the display closure translates it to the localized label.
                let renderer_options = vec![
                    "auto".to_string(),
                    "opengl".to_string(),
                    "software".to_string(),
                ];
                let rendering_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("renderer_backend"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(self.setting_renderer_backend.clone()),
                            renderer_options,
                            |s: &String| {
                                let key = match s.as_str() {
                                    "opengl" => "renderer_opengl",
                                    "software" => "renderer_software",
                                    _ => "renderer_auto",
                                };
                                crate::i18n::t(key).to_string()
                            },
                        )
                        .on_select(Message::SettingRendererBackendChanged)
                        .width(200)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                    Space::new().height(4),
                    text(crate::i18n::t("renderer_backend_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]);

                // Explicit `Space::new()` between elements (no
                // `.spacing()`) so the gap before the first panel
                // matches the SFTP section's 16 px exactly; the
                // previous `.spacing(12)` was stacking on top of the
                // explicit gaps to roughly double them.
                let mut content_col = column![
                    text(crate::i18n::t("interface")).size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    language_section,
                    Space::new().height(12),
                    layout_dir_section,
                    Space::new().height(12),
                    flatten_section,
                    Space::new().height(12),
                    status_bar_section,
                    Space::new().height(12),
                    tabs_section,
                    Space::new().height(12),
                    layout_section,
                    Space::new().height(12),
                    icon_section,
                    Space::new().height(12),
                    rendering_section,
                    Space::new().height(12),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x());

                // Tray section sits at the end so it doesn't push the
                // more-common toggles down on Linux/macOS (where it's
                // suppressed entirely).
                if cfg!(target_os = "windows") {
                    content_col = content_col.push(tray_section).push(Space::new().height(12));
                } else {
                    let _ = tray_section; // keep helper construction warning-free.
                }

                for row_el in grid_rows {
                    content_col = content_col
                        .push(Space::new().height(12))
                        .push(row_el);
                }

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Shortcuts => {
                use crate::hotkeys::{default_bindings, HotkeyAction};
                let defaults = default_bindings();

                // Header: title + hint + global reset button.
                let header = column![
                    text(crate::i18n::t("keyboard_shortcuts"))
                        .size(18)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(6),
                    text(crate::i18n::t("hotkey_edit_hint"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(10),
                    styled_button(
                        crate::i18n::t("hotkey_reset_all"),
                        Message::ResetAllHotkeys,
                        OryxisColors::t().bg_selected,
                    ),
                    Space::new().height(16),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x());

                let mut rows_col = column![header]
                    .spacing(8)
                    .width(Length::Fill)
                    .align_x(dir_align_x());

                for action in HotkeyAction::all() {
                    let row_el = self.hotkey_editor_row(*action, defaults.get(action).copied());
                    rows_col = rows_col.push(row_el);
                }

                // Read-only footer: terminal copy/paste and Ctrl+Wheel
                // zoom are handled in different layers (the terminal
                // widget owns copy selection; the wheel handler lives
                // in the scroll event). Surfaced here so the user
                // doesn't think they're missing.
                let static_rows = column![
                    Space::new().height(20),
                    text(crate::i18n::t("hotkey_terminal_handled"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    shortcut_row(
                        vec![key_badge("Ctrl"), key_badge("Shift"), key_badge("C")],
                        crate::i18n::t("copy_terminal"),
                    ),
                    shortcut_row(
                        vec![key_badge("Ctrl"), key_badge("Shift"), key_badge("V")],
                        crate::i18n::t("paste_terminal"),
                    ),
                    shortcut_row(
                        vec![key_badge("Ctrl"), key_badge("Wheel")],
                        crate::i18n::t("font_zoom_wheel"),
                    ),
                ]
                .spacing(8)
                .width(Length::Fill)
                .align_x(dir_align_x());
                rows_col = rows_col.push(static_rows);

                scrollable(
                    container(rows_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Security => {
                let password_toggle = toggle_row(
                    crate::i18n::t("vault_password"),
                    self.vault_has_user_password,
                    Message::ToggleVaultPassword,
                );

                let password_section: Element<'_, Message> = if !self.vault_has_user_password {
                    // Show password input to enable
                    let input = text_input(t("new_master_password_placeholder"), &self.vault_new_password)
                        .on_input(Message::VaultNewPasswordChanged)
                        .on_submit(Message::SetVaultPassword)
                        .secure(true)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
                    let btn = styled_button(crate::i18n::t("set_password"), Message::SetVaultPassword, OryxisColors::t().accent);
                    let error: Element<'_, Message> = if let Some(err) = &self.vault_password_error {
                        text(err.clone()).size(12).color(OryxisColors::t().error).into()
                    } else {
                        Space::new().height(0).into()
                    };
                    column![
                        Space::new().height(8),
                        text(t("vault_set_password_desc"))
                            .size(11).color(OryxisColors::t().text_muted),
                        Space::new().height(8),
                        input,
                        Space::new().height(8),
                        btn,
                        error,
                    ].into()
                } else {
                    let note: Element<'_, Message> = text(t("vault_protected_note"))
                        .size(11).color(OryxisColors::t().text_muted).into();
                    let error: Element<'_, Message> = if let Some(err) = &self.vault_password_error {
                        text(err.clone()).size(12).color(OryxisColors::t().error).into()
                    } else {
                        Space::new().height(0).into()
                    };
                    column![Space::new().height(4), note, error].into()
                };

                // Lock Vault only makes sense once a master password is
                // set; without one, locking has nothing to protect and
                // the unlock screen would have no way to re-enter (the
                // vault re-opens itself with an empty key). Show the
                // button when a password exists; otherwise replace with
                // a muted note telling the user how to enable locking.
                let lock_btn: Element<'_, Message> = if self.vault_has_user_password {
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

                let mut export_import_section: iced::widget::Column<'_, Message> = column![
                    text(crate::i18n::t("export_import")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    dir_row(vec![export_btn, Space::new().width(8).into(), import_btn]),
                ];

                // Show export dialog inline
                if self.show_export_dialog {
                    let pw_input = text_input(crate::i18n::t("export_password"), &self.export_password)
                        .on_input(Message::ExportPasswordChanged)
                        .secure(true)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
                    let keys_toggle = dir_row(vec![
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
                    ]).align_y(iced::Alignment::Center);
                    let confirm_btn = styled_button(crate::i18n::t("export_confirm"), Message::ExportConfirm, OryxisColors::t().success);
                    let cancel_btn = styled_button(crate::i18n::t("cancel"), Message::ExportImportDismiss, OryxisColors::t().text_muted);
                    export_import_section = export_import_section
                        .push(Space::new().height(12))
                        .push(pw_input)
                        .push(Space::new().height(8))
                        .push(keys_toggle)
                        .push(Space::new().height(8))
                        .push(dir_row(vec![confirm_btn, Space::new().width(8).into(), cancel_btn]));
                }

                // Show import dialog inline
                if self.show_import_dialog {
                    let pw_input = text_input(crate::i18n::t("import_password"), &self.import_password)
                        .on_input(Message::ImportPasswordChanged)
                        .on_submit(Message::ImportConfirm)
                        .secure(true)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
                    let confirm_btn = styled_button(crate::i18n::t("import_confirm"), Message::ImportConfirm, OryxisColors::t().success);
                    let cancel_btn = styled_button(crate::i18n::t("cancel"), Message::ExportImportDismiss, OryxisColors::t().text_muted);
                    export_import_section = export_import_section
                        .push(Space::new().height(12))
                        .push(text(crate::i18n::t("import_password_hint")).size(12).color(OryxisColors::t().text_muted))
                        .push(Space::new().height(4))
                        .push(pw_input)
                        .push(Space::new().height(8))
                        .push(dir_row(vec![confirm_btn, Space::new().width(8).into(), cancel_btn]));
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

                scrollable(
                    container(
                        column![
                            text(crate::i18n::t("security")).size(18).color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            panel_section(column![password_toggle]),
                            password_section,
                            Space::new().height(24),
                            lock_btn,
                            Space::new().height(24),
                            panel_section(export_import_section),
                            Space::new().height(12),
                            panel_section(ssh_config_section),
                            Space::new().height(24),
                        ]
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Sync => {
                // Device info
                let device_name_input = text_input(
                    crate::i18n::t("sync_device_name_hint"),
                    &self.sync_device_name,
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

                // Sync toggle
                let sync_toggle = toggle_row(
                    crate::i18n::t("sync_enable"),
                    self.sync_enabled,
                    Message::SyncToggleEnabled,
                );

                let mode_label = if self.sync_mode == "auto" { t("sync_mode_auto") } else { t("sync_mode_manual") };
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
                    self.sync_passwords,
                    Message::SyncTogglePasswords,
                );

                // Live engine state indicator, sits right under the
                // enable toggle so the user sees whether the QUIC /
                // mDNS background tasks are actually up.
                let engine_state = if self.sync_engine_running {
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
                    sync_toggle,
                    Space::new().height(4),
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

                if self.sync_enabled && self.sync_mode == "manual" {
                    // Swap Sync Now <-> Cancel while a sync is in
                    // flight. Cancel races a oneshot against the
                    // sync future in dispatch; the click drops the
                    // QUIC connection immediately.
                    let action_btn = if self.sync_in_progress {
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

                if let Some(status) = &self.sync_status {
                    options_section = options_section
                        .push(Space::new().height(8))
                        .push(text(status.as_str()).size(12).color(OryxisColors::t().text_muted));
                }

                // Pairing. The sub-view depends on `sync_pairing_state`:
                // Idle shows the two entry buttons; Hosting shows the
                // generated code; Joining shows the code + address form.
                let mut pairing_section: iced::widget::Column<'_, Message> = column![
                    text(crate::i18n::t("sync_pairing")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                ];

                match self.sync_pairing_state {
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
                        if !self.sync_discovered.is_empty() {
                            pairing_section = pairing_section
                                .push(Space::new().height(14))
                                .push(text(crate::i18n::t("sync_discovered_devices"))
                                    .size(12)
                                    .color(OryxisColors::t().text_secondary))
                                .push(Space::new().height(6));
                            for peer in &self.sync_discovered {
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
                        if let Some(code) = &self.sync_pairing_code {
                            pairing_section = pairing_section
                                .push(text(code.as_str())
                                    .size(30)
                                    .color(OryxisColors::t().success));
                        }
                        // Cross-network pairing block: the link + a
                        // Copy button + the QR. The link works only
                        // when both ends have a signaling URL set
                        // (Settings > Sync > Advanced).
                        if let Some(link) = &self.sync_pairing_link {
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
                            &self.sync_join_code_input,
                        )
                        .on_input(Message::SyncJoinCodeChanged)
                        .padding(8)
                        .width(280)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x());
                        let target_input = text_input(
                            crate::i18n::t("sync_pairing_target_placeholder"),
                            &self.sync_join_target_input,
                        )
                        .on_input(Message::SyncJoinTargetChanged)
                        .padding(8)
                        .width(320)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x());
                        let link_input = text_input(
                            crate::i18n::t("sync_pairing_link_placeholder"),
                            &self.sync_join_link_input,
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
                if !matches!(self.sync_pairing_state, crate::state::SyncPairingState::Idle)
                    && let Some(status) = &self.sync_status
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
                if !self.sync_peers.is_empty() {
                    pairing_section = pairing_section
                        .push(Space::new().height(14))
                        .push(text(crate::i18n::t("sync_paired_devices"))
                            .size(12)
                            .color(OryxisColors::t().text_secondary))
                        .push(Space::new().height(6));
                    for peer in &self.sync_peers {
                        let last_sync = peer.last_synced_at
                            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
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
                let signaling_input = text_input("https://...", &self.sync_signaling_url)
                    .on_input(Message::SyncSignalingUrlChanged)
                    .padding(8)
                    .width(300)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
                let signaling_token_input = text_input(
                    crate::i18n::t("sync_signaling_token_placeholder"),
                    &self.sync_signaling_token,
                )
                .on_input(Message::SyncSignalingTokenChanged)
                .secure(true)
                .padding(8)
                .width(300)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x());
                let relay_input = text_input(crate::i18n::t("sync_relay_optional"), &self.sync_relay_url)
                    .on_input(Message::SyncRelayUrlChanged)
                    .padding(8)
                    .width(300)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
                let port_input = text_input("0", &self.sync_listen_port)
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

                let mut content_col: iced::widget::Column<'_, Message> = column![
                    text(crate::i18n::t("sync")).size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    panel_section(enable_section),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x());

                if self.sync_enabled {
                    content_col = content_col
                        .push(Space::new().height(12))
                        .push(device_section)
                        .push(Space::new().height(12))
                        .push(panel_section(options_section))
                        .push(Space::new().height(12))
                        .push(panel_section(pairing_section))
                        .push(Space::new().height(12))
                        .push(advanced_section);
                }
                content_col = content_col.push(Space::new().height(24));

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::About => {
                let about_section = panel_section(column![
                    text(concat!("Oryxis v", env!("CARGO_PKG_VERSION"))).size(16).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("app_tagline")).size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(16),
                    settings_row(t("built_with"), "Iced, russh, alacritty_terminal".into()),
                    Space::new().height(6),
                    settings_row(t("license"), "AGPL-3.0".into()),
                    Space::new().height(6),
                    crate::widgets::settings_row_link(
                        crate::i18n::t("website"),
                        "oryxis.app".into(),
                        "https://oryxis.app/".into(),
                    ),
                    Space::new().height(6),
                    crate::widgets::settings_row_link(
                        crate::i18n::t("github"),
                        "github.com/wilsonglasser/oryxis".into(),
                        "https://github.com/wilsonglasser/oryxis".into(),
                    ),
                ]);

                let vault_section = panel_section(column![
                    text(crate::i18n::t("vault_stats")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    settings_row(crate::i18n::t("hosts"), self.connections.len().to_string()),
                    Space::new().height(6),
                    settings_row(crate::i18n::t("keychain"), self.keys.len().to_string()),
                    Space::new().height(6),
                    settings_row(crate::i18n::t("snippets"), self.snippets.len().to_string()),
                    Space::new().height(6),
                    settings_row(t("groups"), self.groups.len().to_string()),
                ]);

                scrollable(
                    container(
                        column![
                            text(crate::i18n::t("about")).size(18).color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            about_section,
                            Space::new().height(12),
                            vault_section,
                            Space::new().height(24),
                        ]
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }
            SettingsSection::Proxies => self.view_settings_proxies(),
            SettingsSection::Cloud => self.view_cloud_accounts(),
            SettingsSection::Plugins => self.view_plugins_panel(),
            SettingsSection::KnownHosts => self.view_known_hosts(),
            SettingsSection::Mcp => self.view_settings_mcp(),
        };

        container(crate::widgets::dir_row(vec![
            settings_sidebar.into(),
            container(settings_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ]))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Settings → Proxies. List of saved `ProxyIdentity` rows + an
    /// inline create / edit form. Form is hidden by default; clicking
    /// "+ New" or a row's edit icon opens it pre-populated.
    fn view_settings_proxies(&self) -> Element<'_, Message> {
        // The standalone title binding became unused once the
        // toolbar block below inlines its own label; the previous
        // assignment leaked into the Text type-inference too. Drop
        // it explicitly so rustc doesn't try to pin down a generic
        // Theme parameter for an unread binding.
        // ── List rows ──
        let mut list = column![].spacing(8);
        if self.proxy_identities.is_empty() && !self.proxy_identity_form_visible {
            list = list.push(
                text(crate::i18n::t("proxy_identities_empty"))
                    .size(13)
                    .color(OryxisColors::t().text_muted),
            );
        }
        for pi in &self.proxy_identities {
            let kind_label = match &pi.proxy_type {
                oryxis_core::models::connection::ProxyType::Socks5 => "SOCKS5",
                oryxis_core::models::connection::ProxyType::Socks4 => "SOCKS4",
                oryxis_core::models::connection::ProxyType::Http => "HTTP",
                oryxis_core::models::connection::ProxyType::Command(_) => "CMD",
            };
            let summary = format!("{}, {}:{}", kind_label, pi.host, pi.port);
            let id = pi.id;
            let edit_btn = button(text(crate::i18n::t("edit")).size(12))
                .on_press(Message::ShowProxyIdentityForm(Some(id)))
                .padding(Padding {
                    top: 4.0,
                    right: 10.0,
                    bottom: 4.0,
                    left: 10.0,
                })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(4.0),
                            color: OryxisColors::t().border,
                            width: 1.0,
                        },
                        text_color: OryxisColors::t().text_secondary,
                        ..Default::default()
                    }
                });
            let delete_btn = button(text(crate::i18n::t("delete")).size(12))
                .on_press(Message::DeleteProxyIdentity(id))
                .padding(Padding {
                    top: 4.0,
                    right: 10.0,
                    bottom: 4.0,
                    left: 10.0,
                })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => Color { a: 0.10, ..OryxisColors::t().error },
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(4.0),
                            color: OryxisColors::t().border,
                            width: 1.0,
                        },
                        text_color: OryxisColors::t().error,
                        ..Default::default()
                    }
                });
            // Card layout matching the Hosts / Keychain / Snippets
            // pattern: host_icon badge on the leading edge, label +
            // subtitle column in the middle, action buttons trailing.
            let proxy_style = crate::widgets::resolve_host_icon_style(
                None,
                &self.setting_default_host_icon,
            );
            let glyph_el: Element<'_, Message> = iced_fonts::lucide::globe()
                .size(16)
                .line_height(1.0)
                .color(Color::WHITE)
                .into();
            let badge = crate::widgets::host_icon(
                proxy_style,
                OryxisColors::t().accent,
                &pi.label,
                Some(glyph_el),
                32.0,
            );
            let row_el = container(
                dir_row(vec![
                    badge,
                    Space::new().width(8).into(),
                    column![
                        text(&pi.label)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .wrapping(iced::widget::text::Wrapping::None),
                        Space::new().height(2),
                        text(summary)
                            .size(10)
                            .color(OryxisColors::t().text_muted)
                            .wrapping(iced::widget::text::Wrapping::None),
                    ]
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .into(),
                    edit_btn.into(),
                    Space::new().width(8).into(),
                    delete_btn.into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding {
                top: 8.0,
                right: 12.0,
                bottom: 8.0,
                left: 8.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(10.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            })
            .width(Length::Fill);
            list = list.push(row_el);
        }

        // "+ Proxy" button, same Cloud-Accounts pattern: bold plus
        // glyph + bold label in the accent fill. Lives on the
        // trailing edge of the toolbar so the section header reads
        // exactly like Hosts / Keychain / Snippets / Cloud.
        let add_btn: Element<'_, Message> = {
            let fg = OryxisColors::t().button_text;
            button(
                container(
                    dir_row(vec![
                        text("+").size(13).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        }).color(fg).into(),
                        Space::new().width(4).into(),
                        text(crate::i18n::t("new_proxy_identity"))
                            .size(11)
                            .font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                            })
                            .color(fg)
                            .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .center_y(Length::Fixed(24.0))
                .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 14.0 }),
            )
            .on_press(Message::ShowProxyIdentityForm(None))
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
            })
            .into()
        };

        // ── Inline form (only when visible) ──
        let form: Element<'_, Message> = if self.proxy_identity_form_visible {
            self.view_proxy_identity_form()
        } else {
            Space::new().height(0).into()
        };

        // Toolbar layout matches Hosts / Keychain / Cloud Accounts:
        // bold title on the leading edge, action button trailing,
        // no extra descriptive paragraph in between (let the empty
        // state speak for itself).
        let toolbar = container(
            dir_row(vec![
                text(crate::i18n::t("proxies"))
                    .size(18)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                if self.proxy_identity_form_visible {
                    Space::new().width(0).height(Length::Fixed(32.0)).into()
                } else {
                    add_btn
                },
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 20.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        // Mirror the Hosts / Keychain layout: toolbar lives OUTSIDE
        // the scrollable so its leading padding doesn't stack with
        // the scrollable's, and the list + form inside use their
        // own leading padding tuned to land flush with the toolbar
        // title (Hosts achieves this by sharing the scrollable
        // `left: 24` with the toolbar's `left: 24` for matching
        // indents at the top of the panel).
        let scroll = scrollable(
            column![
                list,
                Space::new().height(16),
                form,
                Space::new().height(24),
            ]
            .width(Length::Fill)
            .padding(Padding { top: 0.0, right: 24.0, bottom: 0.0, left: 24.0 })
            .align_x(dir_align_x()),
        )
        .height(Length::Fill);

        column![toolbar, scroll]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Standalone MCP Server settings section. Was nested inside the
    /// Security panel in 0.6 when MCP shipped with the installer; in
    /// 0.7 it lives in its own Settings sidebar entry because the
    /// plugin distribution + setup-guide affordances deserve room
    /// without competing with the Security toggles.
    fn view_settings_mcp(&self) -> Element<'_, Message> {
        let mcp_toggle = toggle_row(
            crate::i18n::t("enable_mcp_server"),
            self.mcp_server_enabled,
            Message::ToggleMcpServer,
        );
        let mcp_guide_btn = button(
            container(text(crate::i18n::t("mcp_setup_guide")).size(12).color(OryxisColors::t().accent))
                .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 }),
        )
        .on_press(if self.show_mcp_info { Message::HideMcpInfo } else { Message::ShowMcpInfo })
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
            mcp_toggle,
            Space::new().height(4),
            dir_row(vec![
                text(crate::i18n::t("mcp_server_desc")).size(11).color(OryxisColors::t().text_muted).into(),
                Space::new().width(Length::Fill).into(),
                mcp_guide_btn.into(),
            ]).align_y(iced::Alignment::Center),
        ];
        if self.show_mcp_info {
            mcp_col = mcp_col
                .push(Space::new().height(12))
                .push(mcp_info_panel(
                    self.mcp_config_copied,
                    &self.mcp_install_status,
                    &self.mcp_server_token,
                    self.mcp_token_visible,
                    self.mcp_target_wsl,
                ));
        }

        scrollable(
            container(
                column![
                    text(crate::i18n::t("mcp_server")).size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    panel_section(mcp_col),
                    Space::new().height(24),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill)
        .into()
    }

    /// The inline create / edit form for a proxy identity. Used inside
    /// `view_settings_proxies` when `proxy_identity_form_visible` is on.
    fn view_proxy_identity_form(&self) -> Element<'_, Message> {
        use crate::state::ProxyKind;

        // The picker only offers the four wire types, None / Identity
        // are not valid for a saved identity itself.
        let wire_kinds: &[ProxyKind] = &[
            ProxyKind::Socks5,
            ProxyKind::Socks4,
            ProxyKind::Http,
            ProxyKind::Command,
        ];

        let kind_picker = pick_list(
            Some(self.proxy_identity_form_kind),
            wire_kinds,
            |k: &ProxyKind| k.to_string(),
        )
        .on_select(Message::ProxyIdentityFormKindChanged)
        .padding(10)
        .style(crate::widgets::rounded_pick_list_style);

        let pw_placeholder: &str = if self.proxy_identity_form_has_existing_password
            && !self.proxy_identity_form_password_touched
        {
            crate::i18n::t("proxy_password_existing")
        } else {
            crate::i18n::t("proxy_password_placeholder")
        };

        let pw_input = text_input(pw_placeholder, &self.proxy_identity_form_password)
            .on_input(Message::ProxyIdentityFormPasswordChanged)
            .secure(!self.proxy_identity_form_password_visible)
            .padding(10)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        let save_label = if self.editing_proxy_identity_id.is_some() {
            crate::i18n::t("save")
        } else {
            crate::i18n::t("add")
        };
        // Match the keychain / vault buttons: bold accent for the
        // primary action, muted color for cancel.
        let save_btn = styled_button(
            save_label,
            Message::SaveProxyIdentity,
            OryxisColors::t().accent,
        );
        let cancel_btn = styled_button(
            crate::i18n::t("cancel"),
            Message::HideProxyIdentityForm,
            OryxisColors::t().text_muted,
        );

        // Use the shared `panel_field` helper for label/input pairs
        // gives the same 4-px gap between label and control as every
        // other form in the app, instead of glueing them together.
        use crate::widgets::panel_field;
        let mut col = column![
            panel_field(
                crate::i18n::t("proxy_identity_label"),
                text_input("home-bastion", &self.proxy_identity_form_label)
                    .on_input(Message::ProxyIdentityFormLabelChanged)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(12),
            panel_field(crate::i18n::t("proxy_type"), kind_picker.into()),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_host"),
                text_input(
                    crate::i18n::t("proxy_host_placeholder"),
                    &self.proxy_identity_form_host,
                )
                .on_input(Message::ProxyIdentityFormHostChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                .into(),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_port"),
                text_input("1080", &self.proxy_identity_form_port)
                    .on_input(Message::ProxyIdentityFormPortChanged)
                    .padding(6)
                    .width(70)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
            ),
            Space::new().height(12),
            panel_field(
                crate::i18n::t("proxy_username"),
                text_input(
                    crate::i18n::t("proxy_username_placeholder"),
                    &self.proxy_identity_form_username,
                )
                .on_input(Message::ProxyIdentityFormUsernameChanged)
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                .into(),
            ),
            Space::new().height(12),
            panel_field(crate::i18n::t("proxy_password"), pw_input.into()),
        ];

        if let Some(err) = &self.proxy_identity_form_error {
            col = col.push(Space::new().height(8)).push(
                text(err.as_str())
                    .size(12)
                    .color(OryxisColors::t().error),
            );
        }

        col = col.push(Space::new().height(16)).push(
            dir_row(vec![cancel_btn, Space::new().width(8).into(), save_btn])
                .align_y(iced::Alignment::Center),
        );

        container(col)
            .padding(Padding {
                top: 16.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_hover)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    }

    /// Single row in the Shortcuts editor list. Renders:
    /// - capture state ("Press a key…") when this row is currently
    ///   being edited;
    /// - badges + clickable surface in normal state;
    /// - reset button only when the binding differs from the
    ///   factory default, so the user can spot overrides at a glance.
    fn hotkey_editor_row(
        &self,
        action: crate::hotkeys::HotkeyAction,
        default: Option<crate::hotkeys::HotkeyBinding>,
    ) -> Element<'_, Message> {
        let binding = self.hotkey_bindings.get(&action).copied();
        let is_editing = self.editing_hotkey == Some(action);
        let is_overridden = matches!(
            (binding, default),
            (Some(b), Some(d)) if b != d
        );

        // Badge cluster: either the captured-mode placeholder or the
        // serialized binding pills. For family actions the suffix
        // badge is rendered with a distinct muted style so the user
        // sees at a glance which slot is fixed.
        let pills: Element<'_, Message> = if is_editing {
            text(crate::i18n::t("hotkey_press_a_key"))
                .size(12)
                .color(OryxisColors::t().accent)
                .into()
        } else if let Some(b) = binding {
            let labels = b.badges();
            let n = labels.len();
            let primary_editable = action.primary_editable();
            let badges: Vec<Element<'_, Message>> = labels
                .into_iter()
                .enumerate()
                .map(|(i, lbl)| {
                    let is_suffix = i == n - 1;
                    if is_suffix && !primary_editable {
                        // Fixed-suffix badge: same solid pill as the
                        // modifiers so it stays legible, but with a
                        // dashed-feel via a tinted border + muted
                        // text. The earlier alpha-40 background
                        // washed out completely against the dark
                        // button surface; this keeps the visual
                        // distinction without losing contrast.
                        container(
                            text(lbl)
                                .size(11)
                                .color(OryxisColors::t().text_secondary),
                        )
                        .padding(Padding {
                            top: 3.0,
                            right: 6.0,
                            bottom: 3.0,
                            left: 6.0,
                        })
                        .style(|_| container::Style {
                            background: Some(Background::Color(OryxisColors::t().bg_selected)),
                            border: Border {
                                radius: Radius::from(4.0),
                                color: OryxisColors::t().border,
                                width: 1.0,
                            },
                            ..Default::default()
                        })
                        .into()
                    } else {
                        key_badge_owned(lbl)
                    }
                })
                .collect();
            iced::widget::Row::with_children(badges)
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            // Unbound (post-conflict). Render a dashed placeholder.
            text(crate::i18n::t("hotkey_unbound"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .into()
        };

        // Wrap badges in a clickable surface (button with neutral
        // style). The editing-state row gets an accent border so it
        // reads "pending input" against the other rows.
        let pills_btn = button(pills).on_press(Message::StartEditingHotkey(action)).style(
            move |_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                    _ => OryxisColors::t().button_bg,
                };
                let border_color = if is_editing {
                    OryxisColors::t().accent
                } else {
                    OryxisColors::t().border
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: border_color,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            },
        );
        let pills_box = container(pills_btn)
            .width(220)
            .align_x(dir_align_x());

        let label = text(crate::i18n::t(action.label_key()))
            .size(13)
            .color(OryxisColors::t().text_secondary);

        let reset_el: Element<'_, Message> = if is_overridden {
            button(
                text(crate::i18n::t("hotkey_reset"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
            .on_press(Message::ResetHotkey(action))
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => Some(Background::Color(OryxisColors::t().button_bg_hover)),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    border: Border {
                        radius: Radius::from(4.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
        } else {
            Space::new().width(0).into()
        };

        dir_row(vec![
            pills_box.into(),
            label.into(),
            Space::new().width(Length::Fill).into(),
            reset_el,
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }
}

/// Owned-label variant of `widgets::key_badge`. The editor builds
/// labels at runtime from `HotkeyBinding::badges()` so we can't reuse
/// the `&'a str` shape directly without leaking.
fn key_badge_owned(label: String) -> Element<'static, Message> {
    container(text(label).size(11).color(OryxisColors::t().text_primary))
        .padding(Padding {
            top: 3.0,
            right: 6.0,
            bottom: 3.0,
            left: 6.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
