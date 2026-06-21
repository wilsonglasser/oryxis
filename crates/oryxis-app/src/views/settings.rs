//! Settings screen, terminal, AI, theme, shortcuts, security, sync, about.

use iced::border::Radius;
use iced::widget::{button, checkbox, column, container, pick_list, scrollable, text, text_input, Space};
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
    /// Live preview of the tab strip under the current appearance
    /// settings. Mirrors `active_tab_bg` and the top-bar wash in
    /// `tab_bar.rs` so what the user sees here matches the real strip
    /// as they toggle: fill style (gradient/solid), the accent underline,
    /// the top-bar wash, and the connection status dot. Sample tab labels
    /// are literal demo content (same convention as the font preview).
    fn tab_appearance_preview(&self) -> Element<'_, Message> {
        let accent = OryxisColors::t().accent;
        let solid = self.setting_tab_fill_style == "solid";
        // Reuse the real strip's fill helper so the preview can never
        // drift from what `tab_bar.rs` actually paints.
        let active_bg = crate::views::tab_bar::active_tab_bg(accent, solid);
        // Connection status dot: the same green "connected" cue. Only
        // present (with its trailing gap) when the dot setting is on.
        let mut active_row: Vec<Element<'_, Message>> = Vec::new();
        if self.setting_show_tab_status_dot {
            active_row.push(
                container(Space::new().width(6).height(6))
                    .style(|_| container::Style {
                        background: Some(Background::Color(OryxisColors::t().success)),
                        border: Border { radius: Radius::from(3.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
            );
            active_row.push(Space::new().width(6).into());
        }
        active_row.push(text("production-web").size(12).color(accent).into());
        let active_tab = container(
            dir_row(active_row).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 })
        .style(move |_| container::Style {
            background: Some(active_bg),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        });
        let idle_tab = container(
            text("staging-db").size(12).color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 });
        // Bottom hairline: 2 px accent when the underline tint is on,
        // else the neutral 1 px chrome border (mirrors `view_main`).
        let (line_h, line_color) = if self.setting_tab_accent_line {
            (2.0_f32, accent)
        } else {
            (1.0_f32, OryxisColors::t().border)
        };
        let hairline = container(Space::new().width(Length::Fill).height(line_h))
            .style(move |_| container::Style {
                background: Some(Background::Color(line_color)),
                ..Default::default()
            });
        // Top-bar wash, identical direction + mix to the real strip.
        let bar_base = OryxisColors::t().bg_sidebar;
        let bar_bg: Background = if self.setting_tab_accent_wash {
            let washed = crate::theme::mix(bar_base, accent, 0.16);
            Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                    .add_stop(0.0, washed)
                    .add_stop(0.9, bar_base),
            ))
        } else {
            Background::Color(bar_base)
        };
        let strip = container(
            dir_row(vec![
                active_tab.into(),
                Space::new().width(4).into(),
                idle_tab.into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
        .style(move |_| container::Style {
            background: Some(bar_bg),
            ..Default::default()
        });
        column![strip, hairline].width(Length::Fill).into()
    }

    /// Live preview of a dashboard host card under the current dashboard
    /// settings: the default host icon shape, the optional address line,
    /// and the accent glass wash. Reuses `host_icon` and
    /// `card_accent_wash` so it tracks the real card exactly. Sample host
    /// name / address are literal demo content (like the font preview).
    fn card_appearance_preview(&self) -> Element<'_, Message> {
        let accent = OryxisColors::t().accent;
        let style = crate::widgets::resolve_host_icon_style(None, &self.setting_default_host_icon);
        let icon = crate::widgets::host_icon(
            style,
            accent,
            "production-web",
            Some(iced_fonts::lucide::server().size(16).color(Color::WHITE).into()),
            32.0,
        );
        let mut text_col = column![
            text("production-web").size(13).color(OryxisColors::t().text_primary),
        ];
        if self.setting_show_host_address {
            text_col = text_col
                .push(Space::new().height(2))
                .push(
                    text("deploy@10.0.0.4")
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                );
        }
        let card = container(
            dir_row(vec![
                icon,
                Space::new().width(10).into(),
                text_col.width(Length::Fill).align_x(dir_align_x()).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(10.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
        let card_el: Element<'_, Message> = card.into();
        if self.setting_card_accent_glass {
            crate::widgets::card_accent_wash(card_el, accent)
        } else {
            card_el
        }
    }

    pub(crate) fn view_settings(&self) -> Element<'_, Message> {
        // ── Settings sidebar ──
        let settings_sidebar = {
            // Order: most-touched at the top (visual + everyday
            // configuration), then per-feature toggles, then network
            // resources, then plugin / system / about. The previous
            // order was historical (followed the implementation
            // sequence) and didn't reflect how users actually move
            // through the panel.
            // Core sections, then the "feature plugin" sections (AI /
            // MCP / SFTP / Sync / Cloud Sync) which only appear once the
            // feature is enabled on the Plugins screen, then About. The
            // enable/disable toggles live on the Plugins screen, not here.
            let mut items: Vec<(&str, SettingsSection)> = vec![
                (crate::i18n::t("interface"), SettingsSection::Interface),
                (crate::i18n::t("terminal_settings"), SettingsSection::Terminal),
                (crate::i18n::t("connection"), SettingsSection::Connection),
                (crate::i18n::t("shortcuts"), SettingsSection::Shortcuts),
                (crate::i18n::t("security_privacy"), SettingsSection::Security),
                (crate::i18n::t("plugins"), SettingsSection::Plugins),
            ];
            if self.ai_enabled {
                items.push((crate::i18n::t("ai_assistant"), SettingsSection::AI));
            }
            // MCP gets its settings section once the MCP plugin is
            // present (installed / dev build), mirroring how Cloud Sync
            // appears once a cloud provider plugin is installed. The
            // server on/off toggle lives inside that section.
            if self.cloud_provider_installed("mcp") {
                items.push((crate::i18n::t("mcp_server"), SettingsSection::Mcp));
            }
            if self.sftp_enabled {
                items.push(("SFTP", SettingsSection::Sftp));
            }
            if self.sync_enabled {
                items.push((crate::i18n::t("sync"), SettingsSection::Sync));
            }
            // Cloud Sync knobs only matter once a cloud provider plugin
            // is installed (the cloud accounts themselves live on the
            // top-level Cloud surface).
            if self.any_cloud_provider_installed() {
                items.push((crate::i18n::t("settings_cloud_section"), SettingsSection::Cloud));
            }
            items.push((crate::i18n::t("about"), SettingsSection::About));
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
                // Selection / clipboard behaviour.
                let toggles_section = panel_section(toggles_col);

                // Text rendering toggles (their own card so they sit under
                // the Appearance group, not mixed with clipboard behaviour).
                let text_render_section = panel_section(column![
                    toggle_row(crate::i18n::t("bold_bright"), self.setting_bold_is_bright, Message::ToggleBoldIsBright),
                    Space::new().height(10),
                    toggle_row(crate::i18n::t("keyword_highlight"), self.setting_keyword_highlight, Message::ToggleKeywordHighlight),
                    Space::new().height(10),
                    toggle_row(crate::i18n::t("smart_contrast"), self.setting_smart_contrast, Message::ToggleSmartContrast),
                ]);

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

                let word_delimiters_section = panel_section(column![
                    text(crate::i18n::t("word_delimiters")).size(13).color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("setting_word_delimiters_desc"))
                        .size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    dir_row(vec![
                        text_input(oryxis_terminal::DEFAULT_WORD_DELIMITERS, &self.setting_word_delimiters)
                            .on_input(Message::SettingWordDelimitersChanged)
                            .padding(10)
                            .width(240)
                            .style(crate::widgets::rounded_input_style)
                            .align_x(dir_align_x())
                            .into(),
                        Space::new().width(8).into(),
                        styled_button(
                            crate::i18n::t("word_delimiters_reset"),
                            Message::SettingResetWordDelimiters,
                            OryxisColors::t().bg_selected,
                        ),
                    ]).align_y(iced::Alignment::Center),
                ]);

                // Terminal theme picker. First card is the "follow
                // app theme" sentinel (terminal_theme_override = None);
                // the rest are explicit palette previews so the user
                // can compare colours without applying each one. Per-host
                // overrides configured via the icon picker still win
                // over this global pick.
                let mut theme_cards: Vec<Element<'_, Message>> = Vec::new();
                // The sentinel renders as a real palette card previewing
                // the app-theme-derived palette (every app theme has a
                // same-named terminal palette), instead of the old
                // input-looking box that read as a text field.
                let app_theme_name = crate::theme::AppTheme::active().name();
                let follow_palette = self
                    .terminal_palette_for_name(app_theme_name)
                    .unwrap_or_default();
                let follow_label =
                    format!("{} ({})", t("terminal_theme_follow_app"), app_theme_name);
                theme_cards.push(crate::widgets::terminal_theme_card(
                    follow_palette,
                    &follow_label,
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

                // Font picker. The list comes from a fontdb scan of
                // monospace families installed on the system (cached
                // for the process lifetime; rescanning per frame read
                // every font file from disk), with a hardcoded
                // fallback when the scan returns nothing.
                let fonts: &'static [String] = crate::app::enumerate_terminal_fonts();
                // Live sample rendered in the picked font on the active
                // terminal palette: the user can confirm the font exists
                // on their machine and preview the theme at a glance. The
                // font name comes straight from the (`'static`) enumerated
                // list, so `Family::Name` needs no leak.
                let preview_font = fonts
                    .iter()
                    .find(|f| f.as_str() == self.terminal_font_name)
                    .map(|f| iced::Font {
                        family: iced::font::Family::Name(f.as_str()),
                        ..iced::Font::MONOSPACE
                    })
                    .unwrap_or(iced::Font::MONOSPACE);
                let active_term_theme = self
                    .terminal_theme_override
                    .clone()
                    .unwrap_or_else(|| crate::theme::AppTheme::active().name().to_string());
                let pal = self
                    .terminal_palette_for_name(&active_term_theme)
                    .unwrap_or_default();
                let (fg, bg) = (pal.foreground, pal.background);
                let (c_green, c_blue, c_cyan, c_yellow) =
                    (pal.ansi[2], pal.ansi[4], pal.ansi[6], pal.ansi[3]);
                let fs = self.terminal_font_size;
                let font_preview = container(
                    column![
                        text("The quick brown fox 1234567890 {}[]()<>")
                            .font(preview_font).size(fs).color(fg),
                        Space::new().height(4),
                        dir_row(vec![
                            text("user").font(preview_font).size(fs).color(c_green).into(),
                            text("@").font(preview_font).size(fs).color(fg).into(),
                            text("host").font(preview_font).size(fs).color(c_blue).into(),
                            text(":").font(preview_font).size(fs).color(fg).into(),
                            text("~/dev").font(preview_font).size(fs).color(c_cyan).into(),
                            text("$ ").font(preview_font).size(fs).color(fg).into(),
                            text("git status").font(preview_font).size(fs).color(c_yellow).into(),
                        ]),
                        Space::new().height(4),
                        // Nerd Font glyphs (branch, powerline, home, folder,
                        // github, git, code, terminal). Render as tofu boxes
                        // if the picked font lacks Nerd Font icon coverage,
                        // which is exactly the at-a-glance check we want.
                        text("\u{e0a0} \u{e0b0} \u{f015} \u{f07b} \u{f09b} \u{e702} \u{f121} \u{f120}")
                            .font(preview_font).size(fs).color(c_green),
                    ],
                )
                .padding(12)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(8.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                });
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
                    Space::new().height(12),
                    font_preview,
                ]);

                // Grouped under "h2" headers, same pattern as Interface:
                // Behavior (selection, delimiters, scrollback) then
                // Appearance (rendering, font, theme). Connection + logging
                // knobs live in their own sections.
                use crate::widgets::settings_group_header as gh;
                scrollable(
                    container(
                        column![
                            gh(crate::i18n::t("terminal_group_behavior")),
                            Space::new().height(8),
                            toggles_section,
                            Space::new().height(12),
                            word_delimiters_section,
                            Space::new().height(12),
                            scrollback_section,
                            Space::new().height(18),
                            gh(crate::i18n::t("terminal_group_appearance")),
                            Space::new().height(8),
                            text_render_section,
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
                    .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Connection => {
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

                scrollable(
                    container(
                        column![
                            keepalive_section,
                            Space::new().height(12),
                            auto_reconnect_section,
                            Space::new().height(12),
                            os_detection_section,
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

                // Enable/disable lives on the Plugins screen now; this
                // section only renders while SFTP is enabled, showing its
                // tuning knobs (parallelism, timeouts).
                let mut content_col: iced::widget::Column<'_, Message> = column![]
                    .width(Length::Fill)
                    .align_x(dir_align_x());

                if self.sftp_enabled {
                    content_col = content_col
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
                        .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::AI => {
                // Enable/disable lives on the Plugins screen now; this
                // section only renders while AI is enabled.
                let mut content_col = column![
                    // The assistant runs commands on connected servers
                    // (some auto-execute); keep the warning in view.
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
                        t("ai_key_saved_placeholder")
                    } else {
                        "sk-..."
                    };
                    let key_input: Element<'_, Message> = container(
                        crate::widgets::password_input_with_eye(
                            key_placeholder,
                            &self.ai_api_key,
                            Message::AiApiKeyChanged,
                            Some(Message::SaveAiApiKey),
                            self.revealed_secrets
                                .contains(&crate::state::SecretField::AiApiKey),
                            Message::ToggleSecretVisibility(
                                crate::state::SecretField::AiApiKey,
                            ),
                            10.0,
                        ),
                    )
                    .width(280)
                    .into();
                    let save_btn = styled_button(crate::i18n::t("save"), Message::SaveAiApiKey, OryxisColors::t().accent);
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
                            .push(panel_field(crate::i18n::t("api_url"), url_input));
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
                        .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Interface => {
                use crate::theme::AppTheme;
                let active_name = self.active_app_theme_name.as_str();

                // Built-in themes, then custom UI themes, then the "+" card.
                let mut cards: Vec<Element<'_, Message>> = Vec::new();
                for theme in AppTheme::ALL.iter() {
                    let name = theme.name();
                    cards.push(crate::views::settings_ui_themes::app_theme_card(
                        name,
                        theme.colors_ref(),
                        name == active_name,
                        Message::AppThemeChanged(name.to_string()),
                    ));
                }
                // Resolve custom colors up front (the card only reads Copy
                // values, so this temporary outlives the borrow).
                let custom_colors: Vec<crate::theme::ThemeColors> = self
                    .custom_ui_themes
                    .iter()
                    .map(|t| crate::theme::theme_colors_from_hex(&t.colors))
                    .collect();
                for (idx, theme) in self.custom_ui_themes.iter().enumerate() {
                    cards.push(self.ui_theme_custom_card(
                        idx,
                        &theme.name,
                        &custom_colors[idx],
                        theme.name == active_name,
                    ));
                }
                cards.push(crate::views::settings_ui_themes::ui_theme_add_card());

                // Chunk the cards into rows of two (Elements aren't Clone, so
                // drain pairs instead of `chunks`).
                let mut grid_rows: Vec<Element<'_, Message>> = Vec::new();
                let mut iter = cards.into_iter();
                while let Some(a) = iter.next() {
                    let mut cells = vec![a];
                    if let Some(b) = iter.next() {
                        cells.push(b);
                    } else {
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

                // The dashboard appearance toggles read as one cluster, so
                // they share a single card (matching the tabs group) instead
                // of one box per toggle. Each toggle keeps its muted
                // description line; 12 px separates the rows.
                let dashboard_section = panel_section(column![
                    toggle_row(
                        crate::i18n::t("flatten_hosts_label"),
                        self.flatten_hosts,
                        Message::FlattenHostsToggle,
                    ),
                    Space::new().height(4),
                    text(crate::i18n::t("flatten_hosts_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(12),
                    toggle_row(
                        crate::i18n::t("show_host_address_label"),
                        self.setting_show_host_address,
                        Message::ToggleShowHostAddress,
                    ),
                    Space::new().height(4),
                    text(crate::i18n::t("show_host_address_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(12),
                    toggle_row(
                        crate::i18n::t("card_accent_glass_label"),
                        self.setting_card_accent_glass,
                        Message::ToggleCardAccentGlass,
                    ),
                    Space::new().height(4),
                    text(crate::i18n::t("card_accent_glass_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]);

                // Tab fill style: gradient (default) vs a flat accent tint.
                // Token-as-value pattern like the other tab pickers.
                let fill_options = vec!["gradient".to_string(), "solid".to_string()];
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
                    Space::new().height(8),
                    toggle_row(
                        crate::i18n::t("tab_accent_wash"),
                        self.setting_tab_accent_wash,
                        Message::SettingToggleTabAccentWash,
                    ),
                    Space::new().height(8),
                    dir_row(vec![
                        text(crate::i18n::t("tab_fill_style"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(self.setting_tab_fill_style.clone()),
                            fill_options,
                            |s: &String| {
                                crate::i18n::t(if s == "solid" {
                                    "tab_fill_solid"
                                } else {
                                    "tab_fill_gradient"
                                })
                                .to_string()
                            },
                        )
                        .on_select(Message::SettingTabFillStyleChanged)
                        .width(180)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                    Space::new().height(12),
                    self.tab_appearance_preview(),
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
                    dir_row(vec![
                        text(crate::i18n::t("pinned_tab_style"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(self.setting_pinned_tab_style.clone()),
                            vec!["compact".to_string(), "full".to_string()],
                            |s: &String| {
                                crate::i18n::t(if s == "full" {
                                    "pinned_tab_style_full"
                                } else {
                                    "pinned_tab_style_compact"
                                })
                                .to_string()
                            },
                        )
                        .on_select(Message::SettingPinnedTabStyleChanged)
                        .width(180)
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
                    "horizontal".to_string(),
                    "vertical".to_string(),
                ];
                let layout_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("nav_orientation"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        pick_list(
                            Some(self.setting_nav_orientation.clone()),
                            layout_options,
                            |s: &String| {
                                crate::i18n::t(if s == "vertical" {
                                    "nav_orientation_vertical"
                                } else {
                                    "nav_orientation_horizontal"
                                })
                                .to_string()
                            },
                        )
                        .on_select(Message::SettingNavOrientationChanged)
                        .width(200)
                        .padding(10)
                        .style(crate::widgets::rounded_pick_list_style)
                        .into(),
                    ]).align_y(iced::Alignment::Center),
                    Space::new().height(4),
                    text(crate::i18n::t("nav_orientation_desc"))
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
                    Space::new().height(12),
                    self.card_appearance_preview(),
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

                // One-time tips (e.g. the terminal's "Ctrl + Click to
                // open the link") retire themselves after first use;
                // this brings them all back in one action.
                let hints_section = panel_section(column![
                    dir_row(vec![
                        text(crate::i18n::t("reset_hints"))
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .into(),
                        Space::new().width(Length::Fill).into(),
                        styled_button(
                            crate::i18n::t("reset_hints"),
                            Message::ResetHints,
                            OryxisColors::t().text_muted,
                        ),
                    ])
                    .align_y(iced::Alignment::Center),
                    Space::new().height(4),
                    text(crate::i18n::t("reset_hints_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]);

                // Explicit `Space::new()` between elements (no
                // `.spacing()`) so the gap before the first panel
                // matches the SFTP section's 16 px exactly; the
                // previous `.spacing(12)` was stacking on top of the
                // explicit gaps to roughly double them.
                // Grouped under "h2" headers so related cards read as a
                // cluster (the section had grown into a flat list that was
                // hard to scan). Group gaps are 18 px, intra-group 12 px,
                // header-to-first-card 8 px.
                use crate::widgets::settings_group_header as gh;
                let mut content_col = column![
                    gh(crate::i18n::t("interface_group_general")),
                    Space::new().height(8),
                    language_section,
                    Space::new().height(12),
                    layout_dir_section,
                    Space::new().height(12),
                    layout_section,
                    Space::new().height(18),
                    gh(crate::i18n::t("interface_group_dashboard")),
                    Space::new().height(8),
                    dashboard_section,
                    Space::new().height(12),
                    icon_section,
                    Space::new().height(18),
                    gh(crate::i18n::t("interface_group_tabs")),
                    Space::new().height(8),
                    tabs_section,
                    Space::new().height(12),
                    status_bar_section,
                    Space::new().height(18),
                    gh(crate::i18n::t("interface_group_theme")),
                    Space::new().height(8),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x());

                // App-theme swatch grid sits under the Theme header.
                for row_el in grid_rows {
                    content_col = content_col
                        .push(row_el)
                        .push(Space::new().height(8));
                }

                // Advanced: renderer backend + reset hints, plus the
                // system tray toggles on Windows (a no-op elsewhere, so
                // hidden on macOS/Linux).
                content_col = content_col
                    .push(Space::new().height(10))
                    .push(gh(crate::i18n::t("interface_group_advanced")))
                    .push(Space::new().height(8))
                    .push(rendering_section)
                    .push(Space::new().height(12))
                    .push(hints_section);
                if cfg!(target_os = "windows") {
                    content_col = content_col
                        .push(Space::new().height(12))
                        .push(tray_section);
                } else {
                    let _ = tray_section; // keep helper construction warning-free.
                }
                content_col = content_col.push(Space::new().height(24));

                scrollable(
                    container(content_col)
                        .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::Shortcuts => {
                use crate::hotkeys::{default_bindings, HotkeyAction};
                let defaults = default_bindings();

                // Header: title + hint + global reset button.
                let header = column![
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
                        .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
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
                    let input = container(crate::widgets::password_input_with_eye(
                        t("new_master_password_placeholder"),
                        &self.vault_new_password,
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
                    let cancel_btn = styled_button(crate::i18n::t("cancel"), Message::ExportImportDismiss, OryxisColors::t().text_muted);
                    export_import_section = export_import_section
                        .push(Space::new().height(12))
                        .push(pw_input)
                        .push(Space::new().height(10))
                        .push(categories)
                        .push(Space::new().height(8))
                        .push(keys_toggle)
                        .push(Space::new().height(8))
                        .push(dir_row(vec![confirm_btn, Space::new().width(8).into(), cancel_btn]));
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

                // Enable/disable lives on the Plugins screen now.
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
                let signaling_input = text_input("https://...", &self.sync_signaling_url)
                    .on_input(Message::SyncSignalingUrlChanged)
                    .padding(8)
                    .width(300)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x());
                let signaling_token_input = container(
                    crate::widgets::password_input_with_eye(
                        crate::i18n::t("sync_signaling_token_placeholder"),
                        &self.sync_signaling_token,
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
                        .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
                )
                .height(Length::Fill)
                .into()
            }

            SettingsSection::About => {
                // Channel-aware build string: nightly builds append the
                // channel + short commit so a nightly user sees exactly what
                // they're running, not just the base version number.
                let version_str = match crate::update::build_channel() {
                    crate::update::UpdateChannel::Nightly => format!(
                        "Oryxis v{} nightly ({})",
                        env!("CARGO_PKG_VERSION"),
                        env!("ORYXIS_GIT_SHA").chars().take(7).collect::<String>(),
                    ),
                    crate::update::UpdateChannel::Stable => {
                        format!("Oryxis v{}", env!("CARGO_PKG_VERSION"))
                    }
                };
                // Logo beside the name + tagline, like the lock screen.
                let about_header = dir_row(vec![
                    iced::widget::svg(self.logo_handle.clone())
                        .width(Length::Fixed(48.0))
                        .height(Length::Fixed(48.0))
                        .into(),
                    Space::new().width(14).into(),
                    column![
                        text(version_str).size(16).color(OryxisColors::t().text_primary),
                        Space::new().height(4),
                        text(t("app_tagline")).size(13).color(OryxisColors::t().text_secondary),
                    ]
                    .align_x(dir_align_x())
                    .into(),
                ])
                .align_y(iced::Alignment::Center);
                let about_section = panel_section(column![
                    about_header,
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

                // Each stat row navigates to its section (issue #38):
                // the count doubles as a shortcut into the data it
                // describes. Logs combines connection events + session
                // recordings, matching what the Logs view lists.
                let vault_section = panel_section(column![
                    text(crate::i18n::t("vault_stats")).size(14).color(OryxisColors::t().text_muted),
                    Space::new().height(8),
                    crate::widgets::settings_row_nav(
                        crate::i18n::t("hosts"),
                        self.connections.len().to_string(),
                        Message::ChangeView(crate::state::View::Dashboard),
                    ),
                    Space::new().height(6),
                    crate::widgets::settings_row_nav(
                        crate::i18n::t("keychain"),
                        self.keys.len().to_string(),
                        Message::ChangeView(crate::state::View::Keys),
                    ),
                    Space::new().height(6),
                    crate::widgets::settings_row_nav(
                        crate::i18n::t("snippets"),
                        self.snippets.len().to_string(),
                        Message::ChangeView(crate::state::View::Snippets),
                    ),
                    Space::new().height(6),
                    crate::widgets::settings_row_nav(
                        t("groups"),
                        self.groups.len().to_string(),
                        Message::ChangeView(crate::state::View::Dashboard),
                    ),
                    Space::new().height(6),
                    crate::widgets::settings_row_nav(
                        t("logs"),
                        (self.logs_total + self.session_logs_total).to_string(),
                        Message::ChangeView(crate::state::View::History),
                    ),
                ]);

                let auto_update_enabled = self.setting_auto_check_updates;
                let check_now_btn = styled_button(
                    t("check_for_updates_now"),
                    Message::CheckForUpdateManual,
                    OryxisColors::t().accent,
                );
                let status_line: Element<'_, Message> = match &self.update_check_status {
                    Some(status) => {
                        use crate::update::UpdateStatus;
                        let (msg, color) = match status {
                            UpdateStatus::Checking => (
                                t("update_check_checking").to_string(),
                                OryxisColors::t().text_muted,
                            ),
                            UpdateStatus::UpToDate => (
                                format!(
                                    "{} ({})",
                                    t("update_check_up_to_date"),
                                    env!("CARGO_PKG_VERSION"),
                                ),
                                OryxisColors::t().success,
                            ),
                            UpdateStatus::Failed(cause) => (
                                format!("{}: {}", t("update_check_failed"), cause),
                                OryxisColors::t().error,
                            ),
                        };
                        // Failures get an inline Retry next to the cause so
                        // the user doesn't have to hunt for the check button.
                        let mut line_items: Vec<Element<'_, Message>> =
                            vec![text(msg).size(11).color(color).into()];
                        if matches!(status, UpdateStatus::Failed(_)) {
                            line_items.push(Space::new().width(10).into());
                            line_items.push(styled_button(
                                t("retry"),
                                Message::CheckForUpdateManual,
                                OryxisColors::t().text_muted,
                            ));
                        }
                        let line = crate::widgets::dir_row(line_items)
                            .align_y(iced::Alignment::Center);
                        container(line)
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
                    check_now_btn,
                    status_line,
                ]);

                scrollable(
                    container(
                        column![
                            about_section,
                            Space::new().height(12),
                            auto_update_section,
                            Space::new().height(12),
                            vault_section,
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
            SettingsSection::Cloud => self.view_cloud_sync_settings(),
            SettingsSection::Plugins => self.view_plugins_panel(),
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
    pub(crate) fn view_settings_proxies(&self) -> Element<'_, Message> {
        // The standalone title binding became unused once the
        // toolbar block below inlines its own label; the previous
        // assignment leaked into the Text type-inference too. Drop
        // it explicitly so rustc doesn't try to pin down a generic
        // Theme parameter for an unread binding.
        // ── List rows ──
        let needle = self.proxy_search.trim().to_lowercase();
        let mut list = column![].spacing(8);
        for pi in self.proxy_identities.iter().filter(|pi| {
            needle.is_empty()
                || pi.label.to_lowercase().contains(&needle)
                || pi.host.to_lowercase().contains(&needle)
        }) {
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
            list = list.push(self.card_wash(row_el.into(), OryxisColors::t().accent));
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

        // Empty + no form open → polished centered empty state (matches
        // Hosts / Keychain / Snippets), no toolbar (search hidden + the
        // "+ New" lives in the CTA).
        if self.proxy_identities.is_empty() && !self.proxy_identity_form_visible {
            let empty = crate::widgets::empty_state(
                iced_fonts::lucide::router()
                    .size(32)
                    .color(OryxisColors::t().text_muted)
                    .into(),
                crate::i18n::t("proxy_identities_empty_title").to_string(),
                crate::i18n::t("proxy_identities_empty").to_string(),
                Some((
                    crate::i18n::t("new_proxy_identity").to_string(),
                    Message::ShowProxyIdentityForm(None),
                )),
            );
            return column![empty]
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // Toolbar: search on the leading edge (Fill), action button
        // trailing. The button hides while the form panel is open (the
        // panel carries its own Save/Cancel).
        let toolbar = container(
            dir_row(vec![
                self.vault_search_field(),
                Space::new().width(10).into(),
                if self.proxy_identity_form_visible {
                    Space::new().width(0).height(Length::Fixed(32.0)).into()
                } else {
                    add_btn
                },
            ]).align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 24.0, bottom: 16.0, left: 24.0 })
        .width(Length::Fill);

        let scroll = scrollable(
            column![list, Space::new().height(24)]
                .width(Length::Fill)
                .padding(Padding { top: 0.0, right: 24.0, bottom: 0.0, left: 24.0 })
                .align_x(dir_align_x()),
        )
        .height(Length::Fill);

        // The editor is a right-hand side panel hoisted to `view_main`
        // (active_side_panel) so it rises over the sub-nav band.
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
        // The MCP plugin is managed (installed / updated) from the
        // Plugins screen; the server's own on/off lives here.
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
            toggle_row(
                crate::i18n::t("mcp_server"),
                self.mcp_server_enabled,
                Message::ToggleMcpServer,
            ),
            Space::new().height(12),
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
                    panel_section(mcp_col),
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

    /// The inline create / edit form for a proxy identity. Used inside
    /// `view_settings_proxies` when `proxy_identity_form_visible` is on.
    pub(crate) fn view_proxy_identity_form(&self) -> Element<'_, Message> {
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
        let col = column![
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

        // ── Header (title + close), matching the host / session-group
        // side panels so every editor reads the same. ──
        let title = if self.editing_proxy_identity_id.is_some() {
            crate::i18n::t("edit_proxy_identity")
        } else {
            crate::i18n::t("new_proxy_identity")
        };
        let panel_header = container(
            dir_row(vec![
                text(title)
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                button(text("\u{00D7}").size(14).color(OryxisColors::t().text_muted))
                    .on_press(Message::HideProxyIdentityForm)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        border: Border::default(),
                        ..Default::default()
                    })
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 16.0, right: 16.0, bottom: 12.0, left: 16.0 });

        let form_scroll = scrollable(
            container(col).padding(Padding {
                top: 0.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            }),
        )
        .height(Length::Fill);

        // Inline error sits OUTSIDE the scrollable, just above the footer,
        // so it stays visible regardless of scroll position.
        let error_el: Element<'_, Message> = if let Some(err) = &self.proxy_identity_form_error {
            container(text(err.as_str()).size(12).color(OryxisColors::t().error))
                .padding(Padding { top: 0.0, right: 16.0, bottom: 8.0, left: 16.0 })
                .into()
        } else {
            Space::new().height(0).into()
        };

        let footer = container(
            dir_row(vec![cancel_btn, Space::new().width(8).into(), save_btn])
                .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 });

        let panel_content = column![panel_header, form_scroll, error_el, footer].height(Length::Fill);

        container(panel_content)
            .width(crate::app::PANEL_WIDTH)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    color: OryxisColors::t().border,
                    width: 1.0,
                    radius: Radius::from(0.0),
                },
                ..Default::default()
            })
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

/// i18n key for an export/import category's checkbox label.
fn category_label_key(c: oryxis_vault::ExportCategory) -> &'static str {
    use oryxis_vault::ExportCategory as C;
    match c {
        C::Connections => "cat_connections",
        C::Groups => "cat_groups",
        C::Keys => "cat_keys",
        C::Identities => "cat_identities",
        C::ProxyIdentities => "cat_proxies",
        C::CloudProfiles => "cat_cloud_profiles",
        C::Snippets => "cat_snippets",
        C::KnownHosts => "cat_known_hosts",
        C::PortForwardRules => "cat_port_forwards",
        C::SessionGroups => "cat_session_layouts",
        C::Settings => "cat_settings",
    }
}
