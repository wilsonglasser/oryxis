//! Settings screen — terminal, AI, theme, shortcuts, security, sync, about.

use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::widget::button::Status as BtnStatus;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::mcp::mcp_info_panel;
use crate::state::SettingsSection;
use crate::theme::OryxisColors;
use crate::widgets::{
    key_badge, panel_field, panel_section, settings_row, shortcut_row, styled_button, toggle_row,
};

impl Oryxis {
    pub(crate) fn view_settings(&self) -> Element<'_, Message> {
        // ── Settings sidebar ──
        let settings_sidebar = {
            let items: Vec<(&str, SettingsSection)> = vec![
                (crate::i18n::t("terminal_settings"), SettingsSection::Terminal),
                ("SFTP", SettingsSection::Sftp),
                (crate::i18n::t("ai_assistant"), SettingsSection::AI),
                (crate::i18n::t("theme"), SettingsSection::Theme),
                (crate::i18n::t("shortcuts"), SettingsSection::Shortcuts),
                (crate::i18n::t("security"), SettingsSection::Security),
                (crate::i18n::t("sync"), SettingsSection::Sync),
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
            // right edge only — iced's Border applies to all four sides at
            // once, so we compose the single-edge separator instead.
            let right_hairline = container(Space::new().width(1))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                });
            let panel = container(col)
                .width(200)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                    ..Default::default()
                });
            iced::widget::row![panel, right_hairline].height(Length::Fill)
        };

        // ── Settings content ──
        let settings_content: Element<'_, Message> = match self.settings_section {
            SettingsSection::Terminal => {
                let toggles_section = panel_section(column![
                    toggle_row(crate::i18n::t("copy_on_select"), self.setting_copy_on_select, Message::ToggleCopyOnSelect),
                    Space::new().height(10),
                    toggle_row(crate::i18n::t("bold_bright"), self.setting_bold_is_bright, Message::ToggleBoldIsBright),
                    Space::new().height(10),
                    toggle_row(crate::i18n::t("keyword_highlight"), self.setting_keyword_highlight, Message::ToggleKeywordHighlight),
                    Space::new().height(10),
                    toggle_row(crate::i18n::t("smart_contrast"), self.setting_smart_contrast, Message::ToggleSmartContrast),
                ]);

                let font_size_section = panel_section(column![
                    row![
                        text(crate::i18n::t("terminal_font_size")).size(13).color(OryxisColors::t().text_primary),
                        Space::new().width(Length::Fill),
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
                        }),
                        Space::new().width(8),
                        text(format!("{:.0}", self.terminal_font_size)).size(13).color(OryxisColors::t().text_primary),
                        Space::new().width(8),
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
                        }),
                    ].align_y(iced::Alignment::Center),
                ]);

                let keepalive_section = panel_section(column![
                    text(crate::i18n::t("keepalive_interval")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("How often (in seconds) to send SSH keepalive packets. Set to 0 to disable.")
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("0", &self.setting_keepalive_interval)
                        .on_input(Message::SettingKeepaliveChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style),
                ]);

                let scrollback_section = panel_section(column![
                    text(crate::i18n::t("scrollback")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("Limit number of terminal rows. Set to 0 for maximum.")
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("10000", &self.setting_scrollback_rows)
                        .on_input(Message::SettingScrollbackChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style),
                ]);

                // Font picker — full list, regardless of whether a given font is
                // bundled. The renderer falls back when a name can't be resolved.
                let fonts: Vec<String> = crate::app::TERMINAL_FONTS
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let font_picker_section = panel_section(column![
                    text(crate::i18n::t("terminal_font")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("Source Code Pro is bundled; others rely on the system — if a font isn't installed, the OS falls back to its default monospace.")
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
                    "Current version: {}", env!("CARGO_PKG_VERSION"),
                ))
                .size(11)
                .color(OryxisColors::t().text_muted);
                let check_now_btn = styled_button(
                    "Check for updates now",
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
                let auto_update_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("auto_check_updates"),
                        auto_update_enabled,
                        Message::SettingToggleAutoCheckUpdates,
                    ),
                    Space::new().height(4),
                    text("Query GitHub on startup for newer releases. You'll see a modal with options to skip, defer, or install.")
                        .size(11).color(OryxisColors::t().text_muted),
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
                    text("After the first successful SSH connect, silently run a probe (`cat /etc/os-release; uname -s`) to detect the remote OS and swap in a distro-specific icon on host cards.")
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
                    text("Silently retry disconnected SSH sessions every 30 seconds, up to the limit below.")
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text(crate::i18n::t("max_reconnect_attempts")).size(12).color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text_input("5", &self.setting_max_reconnect_attempts)
                        .on_input(Message::SettingMaxReconnectChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style),
                ]);

                scrollable(
                    container(
                        column![
                            text(crate::i18n::t("terminal_settings")).size(18).color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            toggles_section,
                            Space::new().height(12),
                            font_size_section,
                            Space::new().height(12),
                            font_picker_section,
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
                            Space::new().height(24),
                        ]
                        .width(Length::Fill),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Sftp => {
                let concurrency_section = panel_section(column![
                    text("Transfer parallelism")
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("Concurrent SFTP transfers per session (1–8). Each slot opens its own subsystem channel — higher = faster bulk transfers, more bandwidth pressure.")
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text_input("2", &self.setting_sftp_concurrency)
                        .on_input(Message::SettingSftpConcurrencyChanged)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style),
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
                            .style(crate::widgets::rounded_input_style),
                    ])
                };

                let connect_section = timeout_input(
                    "Connect timeout (s)",
                    "How long to wait for the TCP connect + SSH transport handshake before failing fast. Default 15.",
                    &self.setting_sftp_connect_timeout,
                    Message::SettingSftpConnectTimeoutChanged,
                );
                let auth_section = timeout_input(
                    "Auth timeout (s)",
                    "Cap on the authentication phase. Long enough to cover password + 2FA prompts. Default 30.",
                    &self.setting_sftp_auth_timeout,
                    Message::SettingSftpAuthTimeoutChanged,
                );
                let session_section = timeout_input(
                    "Channel open timeout (s)",
                    "How long a single SFTP / PTY / sibling channel-open is allowed to take before erroring. Default 10.",
                    &self.setting_sftp_session_timeout,
                    Message::SettingSftpSessionTimeoutChanged,
                );
                let op_section = timeout_input(
                    "Operation timeout (s)",
                    "Per-request cap (list, read, write, rename). Bounds the \"Loading…\" state when the remote stops responding. Applies live to the active session. Default 30.",
                    &self.setting_sftp_op_timeout,
                    Message::SettingSftpOpTimeoutChanged,
                );

                scrollable(
                    container(
                        column![
                            text("SFTP")
                                .size(18)
                                .color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            concurrency_section,
                            Space::new().height(12),
                            connect_section,
                            Space::new().height(12),
                            auth_section,
                            Space::new().height(12),
                            session_section,
                            Space::new().height(12),
                            op_section,
                            Space::new().height(24),
                        ]
                        .width(Length::Fill),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::AI => {
                let enable_section = panel_section(column![
                    toggle_row(crate::i18n::t("enable_ai"), self.ai_enabled, Message::ToggleAiEnabled),
                ]);

                let mut content_col = column![
                    text(crate::i18n::t("ai_assistant")).size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    enable_section,
                ]
                .spacing(12)
                .width(Length::Fill);

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

                    let model_input: Element<'_, Message> = text_input("Model name...", &self.ai_model)
                        .on_input(Message::AiModelChanged)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style)
                        .into();

                    // When a key is already stored, the input is cleared
                    // for security but the placeholder communicates that
                    // a key exists — typing replaces it on save.
                    let key_placeholder = if self.ai_api_key_set {
                        "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022} saved — type to replace"
                    } else {
                        "sk-..."
                    };
                    let key_input: Element<'_, Message> = text_input(key_placeholder, &self.ai_api_key)
                        .on_input(Message::AiApiKeyChanged)
                        .on_submit(Message::SaveAiApiKey)
                        .secure(true)
                        .padding(10)
                        .width(280)
                        .style(crate::widgets::rounded_input_style)
                        .into();
                    let save_btn = styled_button("Save", Message::SaveAiApiKey, OryxisColors::t().accent);
                    let key_status: Element<'_, Message> = if self.ai_api_key_set {
                        row![
                            iced_fonts::lucide::circle_check().size(13).color(OryxisColors::t().success),
                            Space::new().width(6),
                            text("API key saved").size(12).color(OryxisColors::t().success),
                        ]
                        .align_y(iced::Alignment::Center)
                        .into()
                    } else {
                        row![
                            iced_fonts::lucide::circle_alert().size(13).color(OryxisColors::t().text_muted),
                            Space::new().width(6),
                            text("No API key set").size(12).color(OryxisColors::t().text_muted),
                        ]
                        .align_y(iced::Alignment::Center)
                        .into()
                    };

                    let mut provider_col = column![
                        panel_field("Provider", provider_pick),
                        Space::new().height(12),
                        panel_field("Model", model_input),
                    ];

                    if current_info.kind == crate::ai::ProviderKind::Custom {
                        let url_input: Element<'_, Message> = text_input("https://api.example.com/v1/chat/completions", &self.ai_api_url)
                            .on_input(Message::AiApiUrlChanged)
                            .padding(10)
                            .width(300)
                            .style(crate::widgets::rounded_input_style)
                            .into();
                        provider_col = provider_col
                            .push(Space::new().height(12))
                            .push(panel_field("API URL", url_input));
                    }

                    provider_col = provider_col
                        .push(Space::new().height(12))
                        .push(panel_field(
                            "API Key",
                            row![key_input, Space::new().width(8), save_btn]
                                .align_y(iced::Alignment::Center)
                                .into(),
                        ))
                        .push(Space::new().height(4))
                        .push(key_status);

                    content_col = content_col.push(panel_section(provider_col));

                    // System prompt — multi-line editor that grows with the
                    // content. `Length::Shrink` lets the editor auto-resize
                    // to fit its text, capped by the panel's scroll area.
                    let prompt_editor: Element<'_, Message> = iced::widget::text_editor(&self.ai_system_prompt)
                        .placeholder("Custom instructions for the AI assistant...")
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
                        panel_field("Additional System Instructions", prompt_editor),
                        Space::new().height(4),
                        text("Optional. Added to the default system prompt that includes terminal context and bash tool instructions.")
                            .size(11).color(OryxisColors::t().text_muted),
                    ]);
                    content_col = content_col.push(prompt_section);
                }

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Theme => {
                use crate::theme::AppTheme;
                let active_name = AppTheme::active().name();

                let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
                let themes: Vec<&AppTheme> = AppTheme::ALL.iter().collect();

                for chunk in themes.chunks(2) {
                    let mut r = row![].spacing(12);
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
                        r = r.push(card);
                    }
                    // Fill remaining space if odd number
                    if chunk.len() == 1 {
                        r = r.push(Space::new().width(Length::FillPortion(1)));
                    }
                    grid_rows.push(r.into());
                }

                // Language picker
                let lang_options: Vec<String> = crate::i18n::Language::ALL
                    .iter()
                    .map(|l| l.name().to_string())
                    .collect();
                let active_lang_name = crate::i18n::Language::active().name().to_string();

                let language_section = panel_section(column![
                    row![
                        text(crate::i18n::t("language")).size(13).color(OryxisColors::t().text_primary),
                        Space::new().width(Length::Fill),
                        pick_list(
                            Some(active_lang_name),
                            lang_options,
                            |s: &String| s.clone(),
                        )
                        .on_select(Message::LanguageChanged)
                        .width(200)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style),
                    ].align_y(iced::Alignment::Center),
                ]);

                let mut content_col = column![
                    text(crate::i18n::t("theme")).size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                    language_section,
                    Space::new().height(12),
                ]
                .spacing(12)
                .width(Length::Fill);

                for row_el in grid_rows {
                    content_col = content_col.push(row_el);
                }

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Shortcuts => {
                let shortcuts: Vec<(Vec<&str>, &str)> = vec![
                    (vec!["Ctrl", "Shift", "C"], crate::i18n::t("copy_terminal")),
                    (vec!["Ctrl", "Shift", "V"], crate::i18n::t("paste_terminal")),
                    (vec!["Ctrl", "Shift", "W"], crate::i18n::t("close_tab")),
                    (vec!["Ctrl", "1...9"], crate::i18n::t("switch_tab")),
                    (vec!["Ctrl", "L"], crate::i18n::t("open_local")),
                    (vec!["Ctrl", "N"], crate::i18n::t("new_host_shortcut")),
                ];

                let mut rows_col = column![
                    text(crate::i18n::t("keyboard_shortcuts")).size(18).color(OryxisColors::t().text_primary),
                    Space::new().height(16),
                ].spacing(8).width(Length::Fill);

                for (keys, action) in shortcuts {
                    let badges: Vec<Element<'_, Message>> = keys.iter().map(|k| key_badge(k)).collect();
                    rows_col = rows_col.push(shortcut_row(badges, action));
                }

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
                    let input = text_input("New master password...", &self.vault_new_password)
                        .on_input(Message::VaultNewPasswordChanged)
                        .on_submit(Message::SetVaultPassword)
                        .secure(true)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style);
                    let btn = styled_button(crate::i18n::t("set_password"), Message::SetVaultPassword, OryxisColors::t().accent);
                    let error: Element<'_, Message> = if let Some(err) = &self.vault_password_error {
                        text(err.clone()).size(12).color(OryxisColors::t().error).into()
                    } else {
                        Space::new().height(0).into()
                    };
                    column![
                        Space::new().height(8),
                        text("Set a master password to protect your vault. You will need to enter it each time you open Oryxis.")
                            .size(11).color(OryxisColors::t().text_muted),
                        Space::new().height(8),
                        input,
                        Space::new().height(8),
                        btn,
                        error,
                    ].into()
                } else {
                    let note: Element<'_, Message> = text("Your vault is protected with a master password. Toggle off to remove it.")
                        .size(11).color(OryxisColors::t().text_muted).into();
                    let error: Element<'_, Message> = if let Some(err) = &self.vault_password_error {
                        text(err.clone()).size(12).color(OryxisColors::t().error).into()
                    } else {
                        Space::new().height(0).into()
                    };
                    column![Space::new().height(4), note, error].into()
                };

                let lock_btn = button(
                    container(
                        row![
                            iced_fonts::lucide::lock().size(14).color(OryxisColors::t().warning),
                            Space::new().width(10),
                            text(crate::i18n::t("lock_vault")).size(13).color(OryxisColors::t().warning),
                        ].align_y(iced::Alignment::Center),
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
                });

                // MCP Server section
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
                    text(crate::i18n::t("mcp_server")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    mcp_toggle,
                    Space::new().height(4),
                    row![
                        text(crate::i18n::t("mcp_server_desc")).size(11).color(OryxisColors::t().text_muted),
                        Space::new().width(Length::Fill),
                        mcp_guide_btn,
                    ].align_y(iced::Alignment::Center),
                ];
                if self.show_mcp_info {
                    mcp_col = mcp_col
                        .push(Space::new().height(12))
                        .push(mcp_info_panel(self.mcp_config_copied, &self.mcp_install_status));
                }
                let mcp_section = panel_section(mcp_col);

                // Export/Import section
                let export_btn = styled_button(crate::i18n::t("export_vault"), Message::ExportVault, OryxisColors::t().accent);
                let import_btn = styled_button(crate::i18n::t("import_vault"), Message::ImportVault, OryxisColors::t().text_muted);

                let mut export_import_section: iced::widget::Column<'_, Message> = column![
                    text(crate::i18n::t("export_import")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    row![export_btn, Space::new().width(8), import_btn],
                ];

                // Show export dialog inline
                if self.show_export_dialog {
                    let pw_input = text_input(crate::i18n::t("export_password"), &self.export_password)
                        .on_input(Message::ExportPasswordChanged)
                        .secure(true)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style);
                    let keys_toggle = row![
                        text(crate::i18n::t("include_private_keys")).size(13).color(OryxisColors::t().text_secondary),
                        Space::new().width(Length::Fill),
                        button(
                            text(if self.export_include_keys { "ON" } else { "OFF" }).size(12)
                        ).on_press(Message::ExportToggleKeys).style(move |_theme, _status| {
                            button::Style {
                                background: Some(Background::Color(if self.export_include_keys { OryxisColors::t().success } else { OryxisColors::t().bg_hover })),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: OryxisColors::t().text_primary,
                                ..Default::default()
                            }
                        }),
                    ].align_y(iced::Alignment::Center);
                    let confirm_btn = styled_button(crate::i18n::t("export_confirm"), Message::ExportConfirm, OryxisColors::t().success);
                    let cancel_btn = styled_button(crate::i18n::t("cancel"), Message::ExportImportDismiss, OryxisColors::t().text_muted);
                    export_import_section = export_import_section
                        .push(Space::new().height(12))
                        .push(pw_input)
                        .push(Space::new().height(8))
                        .push(keys_toggle)
                        .push(Space::new().height(8))
                        .push(row![confirm_btn, Space::new().width(8), cancel_btn]);
                }

                // Show import dialog inline
                if self.show_import_dialog {
                    let pw_input = text_input(crate::i18n::t("import_password"), &self.import_password)
                        .on_input(Message::ImportPasswordChanged)
                        .on_submit(Message::ImportConfirm)
                        .secure(true)
                        .padding(10)
                        .width(300)
                        .style(crate::widgets::rounded_input_style);
                    let confirm_btn = styled_button(crate::i18n::t("import_confirm"), Message::ImportConfirm, OryxisColors::t().success);
                    let cancel_btn = styled_button(crate::i18n::t("cancel"), Message::ExportImportDismiss, OryxisColors::t().text_muted);
                    export_import_section = export_import_section
                        .push(Space::new().height(12))
                        .push(text(crate::i18n::t("import_password_hint")).size(12).color(OryxisColors::t().text_muted))
                        .push(Space::new().height(4))
                        .push(pw_input)
                        .push(Space::new().height(8))
                        .push(row![confirm_btn, Space::new().width(8), cancel_btn]);
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

                // SSH config import — separate card, sits below the
                // vault export/import. One-shot batch importer; no
                // preview yet.
                let ssh_config_btn = styled_button(
                    "Import ~/.ssh/config",
                    Message::ImportSshConfig,
                    OryxisColors::t().accent,
                );
                let mut ssh_config_section: iced::widget::Column<'_, Message> = column![
                    text("SSH config import")
                        .size(14)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    text(
                        "Pick an OpenSSH config file (defaults to ~/.ssh/config). Each `Host` block becomes a connection record; wildcards and existing labels are skipped.",
                    )
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
                            mcp_section,
                            Space::new().height(12),
                            panel_section(export_import_section),
                            Space::new().height(12),
                            panel_section(ssh_config_section),
                            Space::new().height(24),
                        ]
                        .width(Length::Fill),
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
                .style(crate::widgets::rounded_input_style);

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

                let mode_label = if self.sync_mode == "auto" { "Auto" } else { "Manual" };
                let mode_pick = pick_list(
                    Some(mode_label.to_string()),
                    vec!["Auto".to_string(), "Manual".to_string()],
                    |s: &String| s.clone(),
                )
                .on_select(|v| Message::SyncModeChanged(v.to_lowercase()))
                .text_size(13)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style);

                let mut options_section: iced::widget::Column<'_, Message> = column![
                    text(crate::i18n::t("sync_options")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    sync_toggle,
                    Space::new().height(8),
                    row![
                        text(crate::i18n::t("sync_mode")).size(13).color(OryxisColors::t().text_secondary),
                        Space::new().width(Length::Fill),
                        mode_pick,
                    ].align_y(iced::Alignment::Center),
                ];

                if self.sync_enabled && self.sync_mode == "manual" {
                    let sync_btn = styled_button(crate::i18n::t("sync_now"), Message::SyncNow, OryxisColors::t().accent);
                    options_section = options_section
                        .push(Space::new().height(8))
                        .push(sync_btn);
                }

                if let Some(status) = &self.sync_status {
                    options_section = options_section
                        .push(Space::new().height(8))
                        .push(text(status.as_str()).size(12).color(OryxisColors::t().text_muted));
                }

                // Pairing
                let pair_btn = styled_button(crate::i18n::t("sync_pair_device"), Message::SyncStartPairing, OryxisColors::t().accent);
                let mut pairing_section: iced::widget::Column<'_, Message> = column![
                    text(crate::i18n::t("sync_pairing")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    pair_btn,
                ];

                if let Some(code) = &self.sync_pairing_code {
                    pairing_section = pairing_section
                        .push(Space::new().height(8))
                        .push(text(format!("{}: {}", crate::i18n::t("sync_pairing_code"), code)).size(18).color(OryxisColors::t().success));
                }

                // Paired devices list
                if !self.sync_peers.is_empty() {
                    pairing_section = pairing_section.push(Space::new().height(12));
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
                            row![
                                text(&peer.device_name).size(13).color(OryxisColors::t().text_primary),
                                Space::new().width(Length::Fill),
                                text(last_sync).size(11).color(OryxisColors::t().text_muted),
                                Space::new().width(8),
                                unpair,
                            ].align_y(iced::Alignment::Center),
                        ).push(Space::new().height(4));
                    }
                }

                // Advanced
                let signaling_input = text_input("https://...", &self.sync_signaling_url)
                    .on_input(Message::SyncSignalingUrlChanged)
                    .padding(8)
                    .width(300)
                    .style(crate::widgets::rounded_input_style);
                let relay_input = text_input(crate::i18n::t("sync_relay_optional"), &self.sync_relay_url)
                    .on_input(Message::SyncRelayUrlChanged)
                    .padding(8)
                    .width(300)
                    .style(crate::widgets::rounded_input_style);
                let port_input = text_input("0", &self.sync_listen_port)
                    .on_input(Message::SyncListenPortChanged)
                    .padding(8)
                    .width(100)
                    .style(crate::widgets::rounded_input_style);

                let advanced_section = panel_section(column![
                    text(crate::i18n::t("sync_advanced")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    text(crate::i18n::t("sync_signaling_url")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    signaling_input,
                    Space::new().height(8),
                    text(crate::i18n::t("sync_relay_url")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    relay_input,
                    Space::new().height(8),
                    text(crate::i18n::t("sync_listen_port")).size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(4),
                    port_input,
                ]);

                scrollable(
                    container(
                        column![
                            text(crate::i18n::t("sync")).size(18).color(OryxisColors::t().text_primary),
                            Space::new().height(16),
                            device_section,
                            Space::new().height(12),
                            panel_section(options_section),
                            Space::new().height(12),
                            panel_section(pairing_section),
                            Space::new().height(12),
                            advanced_section,
                            Space::new().height(24),
                        ]
                        .width(Length::Fill),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::About => {
                let about_section = panel_section(column![
                    text(concat!("Oryxis v", env!("CARGO_PKG_VERSION"))).size(16).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text("A modern SSH client built with Rust").size(13).color(OryxisColors::t().text_secondary),
                    Space::new().height(16),
                    settings_row("Built with", "Iced, russh, alacritty_terminal".into()),
                    Space::new().height(6),
                    settings_row("License", "AGPL-3.0".into()),
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
                    settings_row("Groups", self.groups.len().to_string()),
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
                        .width(Length::Fill),
                    )
                    .padding(Padding { top: 20.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }
        };

        container(
            row![
                settings_sidebar,
                container(settings_content)
                    .width(Length::Fill)
                    .height(Length::Fill),
            ],
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
