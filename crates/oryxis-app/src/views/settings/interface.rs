//! Settings -> Interface section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_interface(&self) -> Element<'_, Message> {
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
        let renderer_active_line: Element<'_, Message> =
            if let Some((backend, adapter)) = &self.renderer_active {
                column![
                    Space::new().height(4),
                    text(format!(
                        "{}: {} ({})",
                        crate::i18n::t("renderer_active"),
                        backend,
                        adapter
                    ))
                    .size(11)
                    .color(OryxisColors::t().text_secondary),
                ]
                .into()
            } else {
                Space::new().height(0).into()
            };
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
            // What the compositor actually selected. Resolves the
            // ambiguity of "Automatic" (which GPU backend won?)
            // and confirms an opengl/software override or a
            // runtime fallback actually took effect.
            renderer_active_line,
        ]);

        // Terminal teaching hints (the mouse-capture toast, the
        // "Ctrl + Click to open" link tip) are governed by one
        // tri-state mode. `Once` (default) shows each a single time
        // per pane; `Always` repeats; `Never` silences them.
        let hints_section = panel_section(column![
            dir_row(vec![
                text(crate::i18n::t("terminal_hints"))
                    .size(13)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                pick_list(
                    Some(crate::i18n::t(self.setting_hint_mode.label_key()).to_string()),
                    crate::util::HintMode::ALL
                        .iter()
                        .map(|m| crate::i18n::t(m.label_key()).to_string())
                        .collect::<Vec<_>>(),
                    |s: &String| s.clone(),
                )
                .on_select(Message::HintModeChanged)
                .width(200)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
            ])
            .align_y(iced::Alignment::Center),
            Space::new().height(4),
            text(crate::i18n::t("terminal_hints_desc"))
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
}
