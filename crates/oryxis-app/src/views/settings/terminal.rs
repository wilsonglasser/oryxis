//! Settings -> Terminal section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_terminal(&self) -> Element<'_, Message> {
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
            Space::new().height(10),
            toggle_row(crate::i18n::t("terminal_auto_title"), crate::state::auto_title_enabled(), Message::ToggleTerminalAutoTitle),
            Space::new().height(10),
            dir_row(vec![
                text(crate::i18n::t("terminal_bell")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                pick_list(
                    Some(crate::i18n::t(self.setting_bell_mode.label_key()).to_string()),
                    crate::util::BellMode::ALL
                        .iter()
                        .map(|m| crate::i18n::t(m.label_key()).to_string())
                        .collect::<Vec<_>>(),
                    |s: &String| s.clone(),
                )
                .on_select(Message::BellModeChanged)
                .width(200)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
            ]).align_y(iced::Alignment::Center),
            Space::new().height(10),
            dir_row(vec![
                text(crate::i18n::t("terminal_clipboard")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                pick_list(
                    Some(crate::i18n::t(self.setting_clipboard_access.label_key()).to_string()),
                    crate::util::ClipboardAccess::ALL
                        .iter()
                        .map(|m| crate::i18n::t(m.label_key()).to_string())
                        .collect::<Vec<_>>(),
                    |s: &String| s.clone(),
                )
                .on_select(Message::ClipboardAccessChanged)
                .width(200)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
            ]).align_y(iced::Alignment::Center),
            Space::new().height(10),
            dir_row(vec![
                text(crate::i18n::t("terminal_notification")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                pick_list(
                    Some(crate::i18n::t(self.setting_notification_mode.label_key()).to_string()),
                    crate::util::NotificationMode::ALL
                        .iter()
                        .map(|m| crate::i18n::t(m.label_key()).to_string())
                        .collect::<Vec<_>>(),
                    |s: &String| s.clone(),
                )
                .on_select(Message::NotificationModeChanged)
                .width(200)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
            ]).align_y(iced::Alignment::Center),
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
                    Space::new().height(18),
                    gh(crate::i18n::t("local_terminals")),
                    Space::new().height(8),
                    self.local_terminals_card(),
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
