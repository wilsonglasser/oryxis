//! Settings -> Interface section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    /// The one-line "your Ctrl+digit slots are off by one" notice under
    /// the tab-number picker, with the button that aligns them.
    ///
    /// Shown only when it is actually true: numbering on AND the legacy
    /// mapping still in place (Home owning the first slot, which only
    /// vaults from before the change have). Everyone else gets a
    /// zero-height `Space`, which the fork keeps in the child list, so
    /// the surrounding column's slot count never changes.
    fn tab_number_slot_offset_notice(&self) -> Element<'_, Message> {
        use crate::views::tab_bar::TabNumberStyle;
        if self.tab_number_style() == TabNumberStyle::Off
            || !self.prefs.tab_slot_includes_home
        {
            return Space::new().into();
        }
        crate::widgets::dir_row(vec![
            text(crate::i18n::t("tab_number_slot_offset"))
                .size(11)
                .color(OryxisColors::t().warning)
                .width(Length::Fill)
                .into(),
            self.settings_nav_slot_labeled(
                crate::i18n::t("tab_number_slot_align"),
                crate::keynav::RowAction::activate(Message::Settings(
                    SettingsMessage::SettingToggleTabSlotIncludesHome,
                )),
                6.0,
                styled_button(
                    crate::i18n::t("tab_number_slot_align"),
                    Message::Settings(SettingsMessage::SettingToggleTabSlotIncludesHome),
                    OryxisColors::t().bg_selected,
                ),
            ),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }

    pub(crate) fn view_settings_interface(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order: the sections are
        // deliberately CONSTRUCTED in the same order they render (the
        // recording happens at construction), so keep any new section
        // in its on-screen position.
        self.keynav_settings_reset();

        // ── General ──
        // Language picker. Token-as-value ("auto" or a language code)
        // like the close-button picker; the display closure resolves
        // codes to the native language name. "Auto (OS)" leads the
        // list and is the first-run default.
        let mut lang_options: Vec<String> = vec!["auto".to_string()];
        lang_options.extend(
            crate::i18n::Language::ALL
                .iter()
                .map(|l| l.code().to_string()),
        );
        let active_lang_token = if self.prefs.language_choice == "auto" {
            "auto".to_string()
        } else {
            // Normalize through the resolver so a stale persisted code
            // still highlights the language it falls back to.
            crate::i18n::Language::active().code().to_string()
        };
        let language_row = self.nav_pick_row(
            crate::i18n::t("language"),
            lang_options,
            active_lang_token,
            |s: &String| {
                if s == "auto" {
                    crate::i18n::t("language_auto_os").to_string()
                } else {
                    crate::i18n::Language::from_code(s).name().to_string()
                }
            },
            200.0,
            |v| Message::Settings(SettingsMessage::LanguageChanged(v)),
        );

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
        let layout_dir_row = self.nav_pick_row(
            crate::i18n::t("layout_direction"),
            dir_options,
            active_dir_name,
            |s: &String| s.clone(),
            240.0,
            |v| Message::Settings(SettingsMessage::LayoutDirectionChanged(v)),
        );

        // Layout mode picker: same token-as-value pattern as
        // the close-button picker. The display closure
        // translates the token to the localized label.
        let layout_options = vec![
            "horizontal".to_string(),
            "vertical".to_string(),
        ];
        // One card for the whole General group (language, layout
        // direction, navigation): the rows share a theme, so they
        // share a container instead of one box per row.
        let general_section = panel_section(column![
            language_row,
            Space::new().height(16),
            layout_dir_row,
            Space::new().height(16),
            self.nav_pick_row(
                crate::i18n::t("nav_orientation"),
                layout_options,
                self.prefs.nav_orientation.clone(),
                |s: &String| {
                    crate::i18n::t(if s == "vertical" {
                        "nav_orientation_vertical"
                    } else {
                        "nav_orientation_horizontal"
                    })
                    .to_string()
                },
                200.0,
                |v| Message::Settings(SettingsMessage::SettingNavOrientationChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("nav_orientation_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ]);

        // ── Dashboard ──
        // The whole Dashboard group shares one card: appearance
        // toggles plus the default-icon picker and its live card
        // preview. Each toggle keeps its muted description line;
        // 12 px separates the rows, 16 px the icon sub-block.
        let icon_options = vec![
            "circular".to_string(),
            "square".to_string(),
            "rounded".to_string(),
            "outline".to_string(),
            "initials".to_string(),
        ];
        let dashboard_section = panel_section(column![
            self.nav_toggle_row(
                crate::i18n::t("flatten_hosts_label"),
                self.flatten_hosts,
                Message::Settings(SettingsMessage::FlattenHostsToggle),
            ),
            Space::new().height(4),
            text(crate::i18n::t("flatten_hosts_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("show_host_address_label"),
                self.prefs.show_host_address,
                Message::Settings(SettingsMessage::ToggleShowHostAddress),
            ),
            Space::new().height(4),
            text(crate::i18n::t("show_host_address_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("card_accent_glass_label"),
                self.prefs.card_accent_glass,
                Message::Settings(SettingsMessage::ToggleCardAccentGlass),
            ),
            Space::new().height(4),
            text(crate::i18n::t("card_accent_glass_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(16),
            // Default host icon picker: tokens drive the value,
            // localized labels come from `to_string`.
            self.nav_pick_row(
                crate::i18n::t("default_host_icon"),
                icon_options,
                self.prefs.default_host_icon.clone(),
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
                200.0,
                |v| Message::Settings(SettingsMessage::SettingDefaultHostIconChanged(v)),
            ),
            Space::new().height(12),
            self.card_appearance_preview(),
        ]);

        // ── Tabs ──
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
        // Tab fill style: gradient (default) vs a flat accent tint.
        // Token-as-value pattern like the other tab pickers.
        let fill_options = vec!["gradient".to_string(), "solid".to_string()];
        // Tabs card: per-tab chrome only (close button, pinned
        // style, status dot, accent text/colour and fill). The
        // strip-level knobs live in the Top bar card below; the
        // shared live preview sits outside both (#79 review).
        let tabs_section = panel_section(column![
            self.nav_pick_row(
                crate::i18n::t("close_button_position"),
                close_options,
                self.prefs.tab_close_button_side.clone(),
                |s: &String| {
                    crate::i18n::t(if s == "right" {
                        "close_position_right"
                    } else {
                        "close_position_left"
                    })
                    .to_string()
                },
                160.0,
                |v| Message::Settings(SettingsMessage::SettingTabCloseButtonSideChanged(v)),
            ),
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("pinned_tab_style"),
                vec!["compact".to_string(), "full".to_string()],
                self.prefs.pinned_tab_style.clone(),
                |s: &String| {
                    crate::i18n::t(if s == "full" {
                        "pinned_tab_style_full"
                    } else {
                        "pinned_tab_style_compact"
                    })
                    .to_string()
                },
                180.0,
                |v| Message::Settings(SettingsMessage::SettingPinnedTabStyleChanged(v)),
            ),
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("tab_number_style"),
                vec!["off".to_string(), "prefix".to_string(), "icon".to_string()],
                self.prefs.tab_number_style.clone(),
                |s: &String| {
                    crate::i18n::t(match s.as_str() {
                        "prefix" => "tab_number_style_prefix",
                        "icon" => "tab_number_style_icon",
                        _ => "tab_number_style_off",
                    })
                    .to_string()
                },
                200.0,
                |v| Message::Settings(SettingsMessage::SettingTabNumberStyleChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("tab_number_style_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            // The numbers are strip positions and so is Ctrl+digit, but
            // on a vault from before the slot change Home holds slot 1
            // and the two read one apart. Only someone who turns the
            // numbers ON can be misled by that, so the offer to align
            // them lives right here instead of nagging everyone else.
            self.tab_number_slot_offset_notice(),
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("duplicate_tab_position"),
                vec!["next".to_string(), "end".to_string(), "start".to_string()],
                self.prefs.duplicate_tab_position.clone(),
                |s: &String| {
                    crate::i18n::t(match s.as_str() {
                        "end" => "duplicate_tab_position_end",
                        "start" => "duplicate_tab_position_start",
                        _ => "duplicate_tab_position_next",
                    })
                    .to_string()
                },
                200.0,
                |v| Message::Settings(SettingsMessage::SettingDuplicateTabPositionChanged(v)),
            ),
            Space::new().height(8),
            self.nav_toggle_row(
                crate::i18n::t("show_tab_host_address_label"),
                self.prefs.show_tab_host_address,
                Message::Settings(SettingsMessage::ToggleShowTabHostAddress),
            ),
            Space::new().height(4),
            text(crate::i18n::t("show_tab_host_address_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            self.nav_toggle_row(
                crate::i18n::t("show_tab_status_dot"),
                self.prefs.show_tab_status_dot,
                Message::Settings(SettingsMessage::SettingToggleShowTabStatusDot),
            ),
            Space::new().height(8),
            self.nav_toggle_row(
                crate::i18n::t("tab_accent_text"),
                self.prefs.tab_accent_text,
                Message::Settings(SettingsMessage::SettingToggleTabAccentText),
            ),
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("tab_accent_color"),
                vec!["host".to_string(), "app".to_string()],
                self.prefs.tab_accent_color.clone(),
                |s: &String| {
                    crate::i18n::t(if s == "app" {
                        "tab_accent_color_app"
                    } else {
                        "tab_accent_color_host"
                    })
                    .to_string()
                },
                180.0,
                |v| Message::Settings(SettingsMessage::SettingTabAccentColorChanged(v)),
            ),
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("tab_fill_style"),
                fill_options,
                self.prefs.tab_fill_style.clone(),
                |s: &String| {
                    crate::i18n::t(if s == "solid" {
                        "tab_fill_solid"
                    } else {
                        "tab_fill_gradient"
                    })
                    .to_string()
                },
                180.0,
                |v| Message::Settings(SettingsMessage::SettingTabFillStyleChanged(v)),
            ),
        ]);

        // Top bar card: strip-level knobs (bar position, status bar,
        // the bottom hairline and the whole-bar wash). Built as a
        // mutable column because the side-dock options only render when
        // the position is left / right.
        let mut top_bar_col = column![
            self.nav_pick_row(
                crate::i18n::t("tab_bar_position"),
                vec![
                    "top".to_string(),
                    "bottom".to_string(),
                    "left".to_string(),
                    "right".to_string(),
                ],
                self.prefs.tab_bar_position.clone(),
                |s: &String| {
                    crate::i18n::t(match s.as_str() {
                        "bottom" => "tab_bar_position_bottom",
                        "left" => "tab_bar_position_left",
                        "right" => "tab_bar_position_right",
                        _ => "tab_bar_position_top",
                    })
                    .to_string()
                },
                180.0,
                |v| Message::Settings(SettingsMessage::SettingTabBarPositionChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("tab_bar_position_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("inactive_tab_style"),
                vec![
                    "none".to_string(),
                    "border".to_string(),
                    "underline".to_string(),
                ],
                self.prefs.inactive_tab_style.clone(),
                |s: &String| {
                    crate::i18n::t(match s.as_str() {
                        "border" => "inactive_tab_style_border",
                        "underline" => "inactive_tab_style_underline",
                        _ => "inactive_tab_style_none",
                    })
                    .to_string()
                },
                180.0,
                |v| Message::Settings(SettingsMessage::SettingInactiveTabStyleChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("inactive_tab_style_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("tab_width_mode"),
                vec!["adaptive".to_string(), "uniform".to_string()],
                self.prefs.tab_width_mode.clone(),
                |s: &String| {
                    crate::i18n::t(match s.as_str() {
                        "uniform" => "tab_width_mode_uniform",
                        _ => "tab_width_mode_adaptive",
                    })
                    .to_string()
                },
                180.0,
                |v| Message::Settings(SettingsMessage::SettingTabWidthModeChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("tab_width_mode_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            // Ceiling for the uniform mode only: an optional knob whose UI
            // is hidden while it cannot apply.
            self.tab_uniform_size_row(),
        ];
        if matches!(self.prefs.tab_bar_position.as_str(), "left" | "right") {
            top_bar_col = top_bar_col
                .push(Space::new().height(8))
                .push(self.nav_toggle_row(
                    crate::i18n::t("pinned_tabs_top_bar"),
                    self.prefs.pinned_tabs_top_bar,
                    Message::Settings(SettingsMessage::SettingTogglePinnedTabsTopBar),
                ))
                .push(Space::new().height(4))
                .push(
                    text(crate::i18n::t("pinned_tabs_top_bar_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                )
                .push(Space::new().height(8))
                .push(self.nav_toggle_row(
                    crate::i18n::t("side_hide_top_bar"),
                    self.prefs.side_hide_top_bar,
                    Message::Settings(SettingsMessage::SettingToggleSideHideTopBar),
                ))
                .push(Space::new().height(4))
                .push(
                    text(crate::i18n::t("side_hide_top_bar_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                )
                .push(Space::new().height(8))
                .push(self.nav_toggle_row(
                    crate::i18n::t("side_full_height"),
                    self.prefs.side_full_height,
                    Message::Settings(SettingsMessage::SettingToggleSideFullHeight),
                ))
                .push(Space::new().height(4))
                .push(
                    text(crate::i18n::t("side_full_height_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                );
        }
        let top_bar_section = panel_section(top_bar_col.push(column![
            Space::new().height(8),
            self.nav_toggle_row(
                crate::i18n::t("tab_accent_line"),
                self.prefs.tab_accent_line,
                Message::Settings(SettingsMessage::SettingToggleTabAccentLine),
            ),
            Space::new().height(8),
            self.nav_toggle_row(
                crate::i18n::t("tab_accent_wash"),
                self.prefs.tab_accent_wash,
                Message::Settings(SettingsMessage::SettingToggleTabAccentWash),
            ),
        ]));

        // Status bar card: its own group with a live preview below (the
        // toggles used to hide inside the Top bar card, where "Show
        // status bar" read as a top-bar knob).
        let mut status_bar_col = column![self.nav_toggle_row(
            crate::i18n::t("show_status_bar"),
            self.prefs.show_status_bar,
            Message::Settings(SettingsMessage::SettingToggleShowStatusBar),
        )];
        // Per-element visibility, shown only while the bar itself is on
        // (moot otherwise): connection text, version, and the optional
        // latency / terminal-size / cwd segments.
        if self.prefs.show_status_bar {
            for (label, on, msg) in [
                (
                    "status_show_connection",
                    self.prefs.status_show_connection,
                    SettingsMessage::SettingToggleStatusConnection,
                ),
                (
                    "status_show_version",
                    self.prefs.status_show_version,
                    SettingsMessage::SettingToggleStatusVersion,
                ),
                (
                    "status_show_latency",
                    self.prefs.status_show_latency,
                    SettingsMessage::SettingToggleStatusLatency,
                ),
                (
                    "status_show_dimensions",
                    self.prefs.status_show_dimensions,
                    SettingsMessage::SettingToggleStatusDimensions,
                ),
                (
                    "status_show_cwd",
                    self.prefs.status_show_cwd,
                    SettingsMessage::SettingToggleStatusCwd,
                ),
                (
                    "status_bar_align_left",
                    self.prefs.status_bar_align_left,
                    SettingsMessage::SettingToggleStatusAlignLeft,
                ),
            ] {
                status_bar_col = status_bar_col
                    .push(Space::new().height(8))
                    .push(self.nav_toggle_row(
                        crate::i18n::t(label),
                        on,
                        Message::Settings(msg),
                    ));
            }
        }
        let status_bar_section = panel_section(status_bar_col);

        // ── Advanced ──
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
                Space::new().into()
            };
        let rendering_section = panel_section(column![
            self.nav_pick_row(
                crate::i18n::t("renderer_backend"),
                renderer_options,
                self.prefs.renderer_backend.clone(),
                |s: &String| {
                    let key = match s.as_str() {
                        "opengl" => "renderer_opengl",
                        "software" => "renderer_software",
                        _ => "renderer_auto",
                    };
                    crate::i18n::t(key).to_string()
                },
                200.0,
                |v| Message::Settings(SettingsMessage::SettingRendererBackendChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("renderer_backend_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            // What the compositor actually selected. Resolves the
            // ambiguity of "Automatic" (which GPU backend won?)
            // and confirms an opengl/software override or a
            // runtime fallback actually took effect.
            renderer_active_line,
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("performance_mode"),
                self.prefs.performance_mode,
                Message::Settings(SettingsMessage::SettingTogglePerformanceMode),
            ),
            Space::new().height(4),
            text(crate::i18n::t("performance_mode_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(16),
            // Terminal teaching hints (the mouse-capture toast, the
            // "hold Ctrl and click" link toast) are governed by one
            // tri-state mode. `Once` (default) shows each a single
            // time per pane; `Always` repeats; `Never` silences them.
            self.nav_pick_row(
                crate::i18n::t("terminal_hints"),
                crate::util::HintMode::ALL
                    .iter()
                    .map(|m| crate::i18n::t(m.label_key()).to_string())
                    .collect::<Vec<_>>(),
                crate::i18n::t(self.prefs.hint_mode.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::HintModeChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("terminal_hints_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(16),
            // What a pane does when its session ends (issue #208).
            // A lone REMOTE pane is unaffected: it still relabels itself
            // and rides the auto-reconnect sweep, neither of which a
            // split tab can use without taking its live siblings down.
            self.nav_pick_row(
                crate::i18n::t("pane_end_action"),
                crate::util::PaneEndAction::ALL
                    .iter()
                    .map(|a| crate::i18n::t(a.label_key()).to_string())
                    .collect::<Vec<_>>(),
                crate::i18n::t(self.prefs.pane_end_action.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::PaneEndActionChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("pane_end_action_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ]);

        // Tray toggles only mean something on Windows (the
        // tray module is a no-op on macOS/Linux). Hide the
        // whole section on those platforms so we don't dangle
        // settings the user can't actually exercise. The rows are
        // recorded only there too (cfg'd construction below).
        let tray_section: Option<Element<'_, Message>> = if cfg!(target_os = "windows") {
            Some(panel_section(column![
                text(crate::i18n::t("system_tray"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(8),
                self.nav_toggle_row(
                    crate::i18n::t("close_to_tray"),
                    self.prefs.close_to_tray,
                    Message::Settings(SettingsMessage::SettingToggleCloseToTray),
                ),
                Space::new().height(4),
                text(crate::i18n::t("close_to_tray_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(10),
                self.nav_toggle_row(
                    crate::i18n::t("minimize_to_tray"),
                    self.prefs.minimize_to_tray,
                    Message::Settings(SettingsMessage::SettingToggleMinimizeToTray),
                ),
                Space::new().height(4),
                text(crate::i18n::t("minimize_to_tray_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            ]))
        } else {
            None
        };

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
            general_section,
            Space::new().height(18),
            gh(crate::i18n::t("interface_group_dashboard")),
            Space::new().height(8),
            dashboard_section,
            Space::new().height(18),
            gh(crate::i18n::t("interface_group_tabs")),
            Space::new().height(8),
            tabs_section,
            Space::new().height(18),
            gh(crate::i18n::t("interface_group_top_bar")),
            Space::new().height(8),
            top_bar_section,
            Space::new().height(12),
            // Shared live preview (tab chips + hairline), outside
            // both cards since it renders the two groups at once.
            self.tab_appearance_preview(),
            Space::new().height(18),
            gh(crate::i18n::t("interface_group_status_bar")),
            Space::new().height(8),
            status_bar_section,
            Space::new().height(12),
            self.status_bar_preview(),
            Space::new().height(18),
            gh(crate::i18n::t("interface_group_theme")),
            Space::new().height(8),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // ONE row under the Theme header: the theme in force, as a
        // real card so the preview survives, opening the gallery
        // (mirrors the terminal side).
        content_col = content_col.push(self.active_app_theme_row());

        // Advanced: renderer backend + performance mode + teaching
        // hints in one card, plus the system tray toggles on Windows
        // (a no-op elsewhere, so hidden on macOS/Linux).
        content_col = content_col
            .push(Space::new().height(10))
            .push(gh(crate::i18n::t("interface_group_advanced")))
            .push(Space::new().height(8))
            .push(rendering_section);
        if let Some(tray) = tray_section {
            content_col = content_col
                .push(Space::new().height(12))
                .push(tray);
        }
        content_col = content_col.push(Space::new().height(24));

        scrollable(
            container(content_col)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-interface-scroll"))
        .on_scroll(|v| Message::Settings(SettingsMessage::SectionScrolled(v.relative_offset().y)))
        .height(Length::Fill)
        .into()
    }

    /// The app-theme gallery: every built-in and custom chrome theme as a
    /// card, plus the create / import entries. Behind a modal for the same
    /// reason the terminal one is (`terminal_theme_gallery`): the grid was
    /// the tallest thing in Settings > Interface and pushed Advanced, the
    /// tray toggles and everything else below a wall of swatches.
    ///
    /// The cards record themselves as keyboard rows in RENDER order, so
    /// the modal is walkable the moment it opens.
    pub(crate) fn ui_theme_gallery(&self) -> Element<'_, Message> {
        self.keynav_settings_reset();
        // ── Theme ──
        // Built-in themes, then custom UI themes, then the "+" card.
        // Each card is a keyboard row (Enter applies / opens it);
        // recorded here so the Theme group follows the Tabs group in
        // the keyboard order, exactly as rendered.
        let active_name = self.active_app_theme_name.as_str();
        let mut cards: Vec<Element<'_, Message>> = Vec::new();
        for (bidx, theme) in crate::theme::AppTheme::ALL.iter().enumerate() {
            let name = theme.name();
            cards.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::AppThemeChanged(name.to_string()))),
                10.0,
                // Hover reveals a clone icon (duplicate the preset into an
                // editable custom UI theme); Enter still applies it.
                self.ui_builtin_theme_card(
                    bidx,
                    name,
                    theme.colors_ref(),
                    name == active_name,
                ),
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
            cards.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::AppThemeChanged(
                    theme.name.clone(),
                ))),
                10.0,
                self.ui_theme_custom_card(
                    idx,
                    &theme.name,
                    &custom_colors[idx],
                    theme.name == active_name,
                ),
            ));
        }
        cards.push(self.settings_nav_slot_labeled(
            t("theme_new_custom"),
            crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::UiThemeEditorNew)),
            10.0,
            crate::views::settings_ui_themes::ui_theme_add_card(),
        ));
        cards.push(self.settings_nav_slot_labeled(
            t("theme_import"),
            crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::UiThemeImportOpen)),
            10.0,
            crate::views::settings_ui_themes::ui_theme_import_card(),
        ));
        cards.push(self.settings_nav_slot_labeled(
            crate::i18n::t("theme_community"),
            crate::keynav::RowAction::activate(Message::OpenUrl(
                "https://oryxis.app/themes".to_string(),
            )),
            10.0,
            crate::views::settings_ui_themes::ui_theme_community_card(),
        ));

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
        let footer: Element<'_, Message> = dir_row(vec![
            Space::new().width(Length::Fill).into(),
            crate::widgets::form_cancel_button(Message::Settings(
                SettingsMessage::CloseUiThemeGallery,
            )),
        ])
        .align_y(iced::Alignment::Center)
        .into();
        let mut grid = iced::widget::Column::new().width(Length::Fill);
        for row_el in grid_rows {
            grid = grid.push(row_el).push(Space::new().height(8));
        }
        let card = container(
            iced::widget::column![
                text(crate::i18n::t("interface_group_theme"))
                    .size(18)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(6),
                text(crate::i18n::t("app_theme_desc"))
                    .size(12)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(16),
                // Right padding is the scrollbar's own gutter: it is drawn
                // INSIDE the viewport, so a full-width grid gets a bar
                // painted over its right-hand cards.
                scrollable(
                    container(grid)
                        .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 0.0 }),
                )
                .height(Length::Fixed(460.0)),
                Space::new().height(12),
                footer,
            ],
        )
        .padding(24)
        .width(Length::Fixed(720.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
        card.into()
    }

    /// The single Settings row that stands in for the grid: the app theme
    /// in force, rendered as its own card so the preview survives the move
    /// into the gallery, and clicking it opens that gallery.
    fn active_app_theme_row(&self) -> Element<'_, Message> {
        // Borrowed from state, not cloned: `app_theme_card` ties the
        // returned element's lifetime to the label.
        let name = self.active_app_theme_name.as_str();
        let colors = self
            .custom_ui_themes
            .iter()
            .find(|t| t.name == name)
            .map(|t| crate::theme::theme_colors_from_hex(&t.colors))
            .unwrap_or_else(|| {
                crate::theme::AppTheme::ALL
                    .iter()
                    .find(|t| t.name() == name)
                    .map(|t| *t.colors_ref())
                    .unwrap_or(*crate::theme::AppTheme::OryxisDark.colors_ref())
            });
        self.settings_nav_slot_labeled(
            crate::i18n::t("interface_group_theme"),
            crate::keynav::RowAction::activate(Message::Settings(
                SettingsMessage::OpenUiThemeGallery,
            )),
            10.0,
            crate::views::settings_ui_themes::app_theme_card(
                name,
                &colors,
                true,
                Message::Settings(SettingsMessage::OpenUiThemeGallery),
            ),
        )
    }


    /// Width ceiling for the uniform tab mode. Hidden entirely under the
    /// adaptive mode, following the rule that an inapplicable setting
    /// shows no UI at all rather than a dead control.
    fn tab_uniform_size_row(&self) -> Element<'_, Message> {
        if self.prefs.tab_width_mode != "uniform" {
            return Space::new().into();
        }
        column![
            Space::new().height(8),
            self.nav_pick_row(
                crate::i18n::t("tab_uniform_size"),
                vec!["small".to_string(), "medium".to_string(), "large".to_string()],
                self.prefs.tab_uniform_size.clone(),
                |s: &String| {
                    crate::i18n::t(match s.as_str() {
                        "small" => "tab_uniform_size_small",
                        "large" => "tab_uniform_size_large",
                        _ => "tab_uniform_size_medium",
                    })
                    .to_string()
                },
                180.0,
                |v| Message::Settings(SettingsMessage::SettingTabUniformSizeChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("tab_uniform_size_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ]
        .into()
    }

}
