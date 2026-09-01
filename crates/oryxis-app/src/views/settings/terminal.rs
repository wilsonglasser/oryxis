//! Settings -> Terminal section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    /// Long-command threshold pick for smart tabs, shown only while the
    /// smart-tabs toggle is on (an off feature hides all of its UI).
    /// "Default sidebar tab" picker (issue #85). The tab options are
    /// gated by the GLOBAL feature toggles, so the picker never offers a
    /// tab that is switched off app-wide (Chat needs AI, Files needs
    /// SFTP, Monitor needs the host-monitoring feature); a "Last opened"
    /// sentinel leads the list and is the default. A pinned tab whose
    /// gate is later turned off is kept in the list so the picker still
    /// reflects the current choice. Per-session availability (no live
    /// SSH) is handled at open time by `sidebar_region_tab`, not here.
    fn sidebar_default_tab_row(&self) -> Element<'_, Message> {
        use crate::state::TerminalSidebarTab as STab;
        let mut tabs: Vec<STab> = STab::ALL
            .into_iter()
            .filter(|t| match t {
                STab::Chat => self.ai.enabled,
                STab::Files => self.sftp_enabled,
                STab::Monitor => self.prefs.host_monitoring,
                _ => true,
            })
            .collect();
        if let Some(sel) = self.prefs.sidebar_default_tab
            && !tabs.contains(&sel)
        {
            tabs.push(sel);
        }
        let last = crate::i18n::t("sidebar_default_last").to_string();
        let mut options = vec![last.clone()];
        options.extend(tabs.iter().map(|t| crate::i18n::t(t.label_key()).to_string()));
        let selected = match self.prefs.sidebar_default_tab {
            None => last,
            Some(t) => crate::i18n::t(t.label_key()).to_string(),
        };
        column![
            self.nav_pick_row(
                crate::i18n::t("sidebar_default_tab"),
                options,
                selected,
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::SidebarDefaultTabChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("sidebar_default_tab_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ]
        .into()
    }

    /// Per-tab placement pickers (issue #102): every sidebar tab
    /// chooses the LEFT or RIGHT region, or HIDDEN for tabs the user
    /// never wants, replacing the pre-#102 whole-sidebar left/right
    /// toggle. Same global gating as the default-tab picker above (an
    /// app-wide-off feature hides all of its UI); per-session gates
    /// (no live SSH) don't apply to a persisted location.
    fn sidebar_tab_side_rows(&self) -> Element<'_, Message> {
        use crate::state::{SidebarPlacement, TerminalSidebarTab as STab};
        let tabs: Vec<STab> = STab::ALL
            .into_iter()
            .filter(|t| match t {
                STab::Chat => self.ai.enabled,
                STab::Files => self.sftp_enabled,
                STab::Monitor => self.prefs.host_monitoring,
                STab::Tmux => self.prefs.tmux_manager,
                _ => true,
            })
            .collect();
        let placement_label =
            |p: SidebarPlacement| crate::i18n::t(p.label_key()).to_string();
        let options: Vec<String> =
            SidebarPlacement::ALL.iter().map(|p| placement_label(*p)).collect();
        let mut col = column![
            text(crate::i18n::t("sidebar_tab_locations"))
                .size(12)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(crate::i18n::t("sidebar_tab_locations_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(6),
        ];
        for tab in tabs {
            let selected = placement_label(self.prefs.sidebar_tab_placement(tab));
            col = col.push(self.nav_pick_row(
                crate::i18n::t(tab.label_key()),
                options.clone(),
                selected,
                |s: &String| s.clone(),
                140.0,
                move |v| {
                    // The picker hands back the translated label; map
                    // it to the placement's stable code before it
                    // travels.
                    let placement = SidebarPlacement::ALL
                        .into_iter()
                        .find(|p| crate::i18n::t(p.label_key()) == v)
                        .unwrap_or_else(|| tab.default_placement());
                    Message::Settings(SettingsMessage::SidebarTabSideChanged(
                        tab,
                        placement.code().to_string(),
                    ))
                },
            ));
        }
        col.into()
    }

    fn smart_tabs_threshold_row(&self) -> Element<'_, Message> {
        if !self.prefs.smart_tabs {
            return Space::new().into();
        }
        column![
            Space::new().height(10),
            self.nav_pick_row(
                crate::i18n::t("smart_tabs_threshold"),
                crate::smart_tabs::threshold_options()
                    .into_iter()
                    .map(|(_, l)| l)
                    .collect::<Vec<_>>(),
                crate::smart_tabs::threshold_label(self.prefs.smart_long_secs),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::SmartTabsThresholdChanged(v)),
            ),
        ]
        .into()
    }

    /// Sub-row for the shell-integration key: what it is for, the Copy
    /// button that puts the snippet (key already substituted) on the
    /// clipboard, and Rotate.
    ///
    /// Shown under the capture toggle and only while it is on: the key
    /// does nothing while nothing is being captured, and a control that
    /// cannot matter yet is noise. Nested like the command-log folder row.
    fn shell_integration_row(&self) -> Element<'_, Message> {
        if !self.prefs.command_history {
            return Space::new().into();
        }
        let indent = if crate::i18n::is_rtl_layout() {
            Padding { right: 22.0, ..Padding::ZERO }
        } else {
            Padding { left: 22.0, ..Padding::ZERO }
        };
        let copy = self.settings_nav_slot_labeled(
            crate::i18n::t("shell_integration_copy"),
            crate::keynav::RowAction::activate(Message::Settings(
                SettingsMessage::CopyShellIntegrationSnippet,
            )),
            8.0,
            crate::widgets::styled_button_opt(
                crate::i18n::t("shell_integration_copy"),
                Some(Message::Settings(SettingsMessage::CopyShellIntegrationSnippet)),
                crate::theme::OryxisColors::t().accent,
            ),
        );
        let rotate = self.settings_nav_slot_labeled(
            crate::i18n::t("shell_integration_rotate"),
            crate::keynav::RowAction::activate(Message::Settings(
                SettingsMessage::RegenerateShellIntegrationNonce,
            )),
            8.0,
            crate::widgets::styled_button_opt(
                crate::i18n::t("shell_integration_rotate"),
                Some(Message::Settings(SettingsMessage::RegenerateShellIntegrationNonce)),
                crate::theme::OryxisColors::t().text_secondary,
            ),
        );
        container(
            column![
                text(crate::i18n::t("shell_integration_hint"))
                    .size(12)
                    .color(crate::theme::OryxisColors::t().text_muted)
                    .width(Length::Fill),
                Space::new().height(6),
                crate::widgets::dir_row(vec![
                    Space::new().width(Length::Fill).into(),
                    copy,
                    Space::new().width(8).into(),
                    rotate,
                ])
                .align_y(iced::Alignment::Center),
            ]
            .width(Length::Fill),
        )
        .padding(Padding { top: 8.0, ..indent })
        .width(Length::Fill)
        .into()
    }

    /// Row for the terminal background picture: its label, the chosen
    /// file name (or "None"), a Browse button and, once one is set, a
    /// Remove. Both buttons record keyboard rows in visual order, so
    /// Tab reaches Remove right after Browse.
    fn terminal_bg_image_row(&self) -> Element<'_, Message> {
        let path = self.prefs.terminal_bg_image.trim();
        // The file NAME, not the whole path: a wallpaper lives six
        // directories deep and the row would wrap. The full path is
        // recoverable from the picker, which opens where it left off.
        let current = if path.is_empty() {
            crate::i18n::t("none").to_string()
        } else {
            std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string())
        };
        let browse = self.settings_nav_slot(
            crate::keynav::RowAction::activate(Message::Settings(
                SettingsMessage::TerminalBgImageBrowse,
            )),
            8.0,
            crate::widgets::styled_button_opt(
                crate::i18n::t("browse"),
                Some(Message::Settings(SettingsMessage::TerminalBgImageBrowse)),
                crate::theme::OryxisColors::t().accent,
            ),
        );
        let mut row: Vec<Element<'_, Message>> = vec![
            text(crate::i18n::t("terminal_bg_image"))
                .size(13)
                .color(crate::theme::OryxisColors::t().text_primary)
                .into(),
            Space::new().width(Length::Fill).into(),
            text(current)
                .size(12)
                .color(crate::theme::OryxisColors::t().text_muted)
                .into(),
            Space::new().width(10).into(),
            browse,
        ];
        if !path.is_empty() {
            row.push(Space::new().width(8).into());
            row.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(
                    SettingsMessage::TerminalBgImageCleared,
                )),
                8.0,
                crate::widgets::styled_button_opt(
                    crate::i18n::t("remove"),
                    Some(Message::Settings(SettingsMessage::TerminalBgImageCleared)),
                    crate::theme::OryxisColors::t().error,
                ),
            ));
        }
        crate::widgets::dir_row(row)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .into()
    }

    /// Sub-row for the command-log folder, shown only while the
    /// live-append toggle is on: the effective folder (default
    /// `~/.oryxis/command-history/`) with a Change button, indented
    /// like the other nested sub-options.
    fn command_history_dir_row(&self) -> Element<'_, Message> {
        if !self.prefs.command_history_file {
            return Space::new().into();
        }
        let indent = if crate::i18n::is_rtl_layout() {
            Padding { right: 22.0, ..Padding::ZERO }
        } else {
            Padding { left: 22.0, ..Padding::ZERO }
        };
        let dir = self.command_history_dir().display().to_string();
        let change = self.settings_nav_slot(
            crate::keynav::RowAction::activate(Message::CommandHistory(CommandHistoryMessage::PickCommandHistoryDir)),
            8.0,
            crate::widgets::styled_button_opt(
                crate::i18n::t("browse"),
                Some(Message::CommandHistory(CommandHistoryMessage::PickCommandHistoryDir)),
                crate::theme::OryxisColors::t().accent,
            ),
        );
        container(
            crate::widgets::dir_row(vec![
                text(dir)
                    .size(12)
                    .color(crate::theme::OryxisColors::t().text_muted)
                    .width(Length::Fill)
                    .into(),
                Space::new().width(10).into(),
                change,
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 8.0, ..indent })
        .width(Length::Fill)
        .into()
    }

    /// Row for the ZMODEM download folder: the resolved path (default or
    /// configured) plus a Browse button, and a Reset when a custom folder
    /// is set. Always shown (transfers work regardless of other toggles).
    /// The terminal-theme gallery: every built-in and custom palette as a
    /// card, plus the create / import entries. Lives behind a modal
    /// rather than inline in Settings, where 31 built-ins plus the user's
    /// own pushed every group below it off the page.
    ///
    /// The cards record themselves as keyboard rows in RENDER order, same
    /// as any settings row, so the modal is walkable the moment it opens.
    pub(crate) fn terminal_theme_gallery(&self) -> Element<'_, Message> {
        // Filter, matched against every card's visible label with no
        // special cases (the follow sentinel and the action cards
        // participate too): one rule the user can predict.
        let gallery_filter = self.theme_ui.gallery_filter.trim().to_lowercase();
        let shows = |label: &str| {
            gallery_filter.is_empty() || label.to_lowercase().contains(&gallery_filter)
        };
        // The filter is the first keyboard row of the modal, so it must
        // RECORD before the cards do (construction order is record
        // order); the widget itself is drawn later, ring applied by
        // index.
        let filter_idx = self.settings_nav_record(crate::keynav::RowAction::input(
            iced::widget::Id::new("terminal-theme-gallery-filter"),
        ));
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
        if shows(&follow_label) {
            theme_cards.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::TerminalThemeChanged(String::new()))),
                10.0,
                crate::widgets::terminal_theme_card(
                    follow_palette,
                    &follow_label,
                    self.terminal_theme_override.is_none(),
                    Message::Settings(SettingsMessage::TerminalThemeChanged(String::new())),
                ),
            ));
        }
        for (bidx, theme) in oryxis_terminal::TerminalTheme::ALL.iter().enumerate() {
            if !shows(theme.name()) {
                continue;
            }
            let is_selected = self
                .terminal_theme_override
                .as_deref()
                == Some(theme.name());
            theme_cards.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::TerminalThemeChanged(
                    theme.name().to_string(),
                ))),
                10.0,
                // Hover reveals a clone icon (duplicate the preset into an
                // editable custom theme); Enter still applies the theme.
                self.terminal_builtin_theme_card(bidx, theme, is_selected),
            ));
        }
        // User-defined themes after the built-ins, each with the
        // hover edit / delete affordances. Enter applies the theme
        // (the card's own click action); edit / delete stay
        // hover-only.
        for (idx, ct) in self.custom_terminal_themes.iter().enumerate() {
            if !shows(&ct.name) {
                continue;
            }
            let is_selected =
                self.terminal_theme_override.as_deref() == Some(ct.name.as_str());
            let palette = self
                .terminal_palette_for_name(&ct.name)
                .unwrap_or_default();
            theme_cards.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::TerminalThemeChanged(
                    ct.name.clone(),
                ))),
                10.0,
                self.terminal_custom_theme_card(
                    idx,
                    &ct.name,
                    palette,
                    is_selected,
                ),
            ));
        }
        // "+ New custom theme" + "Import" cards last.
        if shows(t("theme_new_custom")) {
            theme_cards.push(self.settings_nav_slot_labeled(
                t("theme_new_custom"),
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ThemeEditorNew)),
                10.0,
                crate::views::settings_themes::terminal_theme_add_card(),
            ));
        }
        if shows(t("theme_import")) {
            theme_cards.push(self.settings_nav_slot_labeled(
                t("theme_import"),
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ThemeImportOpen)),
                10.0,
                crate::views::settings_themes::terminal_theme_import_card(),
            ));
        }
        if shows(t("theme_community")) {
            theme_cards.push(self.settings_nav_slot_labeled(
                t("theme_community"),
                crate::keynav::RowAction::activate(Message::OpenUrl(
                    "https://oryxis.app/themes".to_string(),
                )),
                10.0,
                crate::views::settings_themes::terminal_theme_community_card(),
            ));
        }
        // 2-column responsive grid for theme cards. Cards still
        // use the existing swatch-+-name layout (the "bolinhas"
        // style); only the row arrangement changes from a single
        // tall column to a side-by-side pair so the picker
        // doesn't dominate the settings panel vertically.
        // Built here (the cards need this view's locals) and handed to
        // the gallery modal.
        let theme_grid = crate::widgets::distribute_card_grid(
            theme_cards,
            2,
            8.0,
            8.0,
        );
        // Bare card, same shape as the theme-import modal:
        // `widgets::modal_overlay` (the caller) owns the scrim, the
        // centering and the click-trap. Scrollable because the list grows
        // with every custom theme, which is exactly why it stopped being
        // inline.
        let footer: Element<'_, Message> = crate::widgets::dir_row(vec![
            Space::new().width(Length::Fill).into(),
            crate::widgets::form_cancel_button(Message::Settings(
                SettingsMessage::CloseTerminalThemeGallery,
            )),
        ])
        .align_y(iced::Alignment::Center)
        .into();
        let filter_input = self.settings_nav_ring_at(
            filter_idx,
            10.0,
            iced::widget::text_input(t("filter_placeholder"), &self.theme_ui.gallery_filter)
                .id(iced::widget::Id::new("terminal-theme-gallery-filter"))
                .on_input(|v| {
                    Message::Settings(SettingsMessage::ThemeGalleryFilterChanged(v))
                })
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .into(),
        );
        let card = container(
            column![
                text(t("terminal_theme")).size(18).color(OryxisColors::t().text_primary),
                Space::new().height(6),
                text(t("terminal_theme_desc")).size(12).color(OryxisColors::t().text_muted),
                Space::new().height(12),
                filter_input,
                Space::new().height(12),
                // The scrollbar is drawn INSIDE the viewport, so a grid
                // that fills the full width gets a bar painted over its
                // right-hand cards. The padding is the bar's own gutter.
                scrollable(
                    container(theme_grid)
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

    pub(crate) fn view_settings_terminal(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order: the sections below
        // are deliberately CONSTRUCTED in the same order they render
        // (recording happens at construction), so keep any new section
        // in its on-screen position.
        self.keynav_settings_reset();
        let mut toggles_col: iced::widget::Column<'_, Message> = column![
            self.nav_toggle_row(crate::i18n::t("copy_on_select"), self.prefs.copy_on_select, Message::Settings(SettingsMessage::ToggleCopyOnSelect)),
        ];
        // Right-click scheme (PuTTY's Context menu / Paste / Extend). The
        // single authority for the gesture.
        let rc_is_paste =
            self.prefs.terminal_right_click == crate::util::RightClickMode::Paste;
        toggles_col = toggles_col.push(Space::new().height(10)).push(self.nav_pick_row(
            crate::i18n::t("terminal_right_click"),
            crate::util::RightClickMode::ALL
                .iter()
                .map(|m| crate::i18n::t(m.label_key()).to_string())
                .collect::<Vec<_>>(),
            crate::i18n::t(self.prefs.terminal_right_click.label_key()).to_string(),
            |s: &String| s.clone(),
            200.0,
            |v| Message::Settings(SettingsMessage::TerminalRightClickChanged(v)),
        ));
        // "Copy on right-click" is a sub-option of copy-on-select, and
        // only meaningful when the right-click scheme is Paste (Menu and
        // Extend repurpose the gesture entirely). Hidden otherwise.
        if self.prefs.copy_on_select && rc_is_paste {
            let indent = if crate::i18n::is_rtl_layout() {
                Padding { right: 22.0, ..Padding::ZERO }
            } else {
                Padding { left: 22.0, ..Padding::ZERO }
            };
            toggles_col = toggles_col
                .push(Space::new().height(8))
                .push(
                    container(self.nav_toggle_row(
                        crate::i18n::t("copy_requires_right_click"),
                        self.prefs.right_click_copy,
                        Message::Settings(SettingsMessage::ToggleRightClickCopy),
                    ))
                    .padding(indent),
                );
        }
        // X11-style middle-click paste (xterm / PuTTY tradition). Its own
        // gesture, so it sits outside the copy-on-select bundle; the
        // paste still routes through the careful-paste / paste-guard
        // checks like every other paste path.
        //
        // State comes from the binding table, not a setting: the gesture
        // IS a chord on `TerminalPasteSelection`, editable in Settings >
        // Shortcuts like any other, and this toggle adds / removes it.
        toggles_col = toggles_col
            .push(Space::new().height(10))
            .push(self.nav_toggle_row(
                crate::i18n::t("middle_click_paste"),
                self.middle_click_pastes(),
                Message::Settings(SettingsMessage::ToggleMiddleClickPaste),
            ));
        // Careful paste: the multi-line paste guard (line-count preview
        // before anything reaches the session). Default on; the toggle is
        // the power-user opt-out.
        toggles_col = toggles_col
            .push(Space::new().height(10))
            .push(self.nav_toggle_row(
                crate::i18n::t("careful_paste_label"),
                self.prefs.careful_paste,
                Message::Settings(SettingsMessage::ToggleCarefulPaste),
            ))
            .push(Space::new().height(4))
            .push(
                text(crate::i18n::t("careful_paste_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            );
        // Content heuristics (bidi/invisible, control bytes, curl|sh,
        // homographs): its own switch so the multi-line check and the
        // suspicious-content check opt in/out independently.
        toggles_col = toggles_col
            .push(Space::new().height(10))
            .push(self.nav_toggle_row(
                crate::i18n::t("paste_guard_label"),
                self.prefs.paste_guard,
                Message::Settings(SettingsMessage::TogglePasteGuard),
            ))
            .push(Space::new().height(4))
            .push(
                text(crate::i18n::t("paste_guard_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            );
        // The whole Behavior group shares one card: selection /
        // clipboard toggles, then the word-delimiter and scrollback
        // sub-blocks (each keeps its 13 px sub-title). Constructed
        // in visual order so the keyboard rows record in order.
        let toggles_col = toggles_col.push(Space::new().height(16));
        let word_delimiters_block = column![
            text(crate::i18n::t("word_delimiters")).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("setting_word_delimiters_desc"))
                .size(11).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            dir_row(vec![
                self.settings_nav_slot_labeled(
                    t("word_delimiters"),
                    crate::keynav::RowAction::input(iced::widget::Id::new("set-terminal-word-delimiters")),
                    10.0,
                    text_input(oryxis_terminal::DEFAULT_WORD_DELIMITERS, &self.prefs.word_delimiters)
                        .id(iced::widget::Id::new("set-terminal-word-delimiters"))
                        .on_input(|v| Message::Settings(SettingsMessage::SettingWordDelimitersChanged(v)))
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .into(),
                ),
                Space::new().width(8).into(),
                self.settings_nav_slot(
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingResetWordDelimiters)),
                    6.0,
                    styled_button(
                        crate::i18n::t("word_delimiters_reset"),
                        Message::Settings(SettingsMessage::SettingResetWordDelimiters),
                        OryxisColors::t().bg_selected,
                    ),
                ),
            ]).align_y(iced::Alignment::Center),
        ];

        let scrollback_block = column![
            text(crate::i18n::t("scrollback")).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("setting_scrollback_desc"))
                .size(11).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            self.settings_nav_slot_labeled(
                t("scrollback"),
                crate::keynav::RowAction::input(iced::widget::Id::new("set-terminal-scrollback")),
                10.0,
                text_input("10000", &self.prefs.scrollback_rows)
                    .id(iced::widget::Id::new("set-terminal-scrollback"))
                    .on_input(|v| Message::Settings(SettingsMessage::SettingScrollbackChanged(v)))
                    .padding(10)
                    .width(240)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .into(),
            ),
            // PuTTY's two "jump back to the live edge" behaviors, so a user
            // stranded deep in history returns without reaching for the
            // wheel / scrollbar.
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("scrollback_reset_keypress"),
                self.prefs.scrollback_reset_keypress,
                Message::Settings(SettingsMessage::ToggleScrollbackResetKeypress),
            ),
            Space::new().height(10),
            self.nav_toggle_row(
                crate::i18n::t("scrollback_reset_output"),
                self.prefs.scrollback_reset_output,
                Message::Settings(SettingsMessage::ToggleScrollbackResetOutput),
            ),
            // Issue #117. On by default: a suggestion that never sends
            // itself costs nothing to ignore, and the alternative is
            // typing a password the app is already holding.
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("terminal_password_autofill"),
                self.prefs.terminal_password_autofill,
                Message::Settings(SettingsMessage::ToggleTerminalPasswordAutofill),
            ),
            Space::new().height(4),
            text(crate::i18n::t("terminal_password_autofill_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            // Ctrl+click on a link. Both default ON: the confirmation is
            // what stops a remote host's OSC 8 label from standing in for
            // its target, and the tunnel is what makes a CLI login's
            // loopback callback reach the machine that opened it.
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("terminal_link_confirm"),
                self.prefs.terminal_link_confirm,
                Message::Settings(SettingsMessage::ToggleTerminalLinkConfirm),
            ),
            Space::new().height(4),
            text(crate::i18n::t("terminal_link_confirm_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("terminal_link_tunnel"),
                self.prefs.terminal_link_tunnel,
                Message::Settings(SettingsMessage::ToggleTerminalLinkTunnel),
            ),
            Space::new().height(4),
            text(crate::i18n::t("terminal_link_tunnel_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ];
        // Where downloads land is behaviour, not appearance: it sat under
        // the Appearance header only because ZMODEM shipped alongside the
        // rendering toggles. The label lost its "ZMODEM" prefix too, since
        // the folder is the app's download destination and nothing about
        // it is protocol-specific. The SETTING key stays
        // `zmodem_download_dir`: renaming it would silently drop the
        // folder anyone had already configured.
        let behavior_section = panel_section(
            toggles_col
                .push(word_delimiters_block)
                .push(Space::new().height(16))
                .push(scrollback_block),
        );

        // Settings > Terminal used to have one "Appearance" card holding
        // everything that was not clipboard behaviour: the bell, OSC 52,
        // OSC 9, smart tabs, the sidebar dock, command-history capture.
        // None of that is appearance. Split into four cards, each named
        // after what the settings inside it actually govern.

        // Appearance: what the grid LOOKS like. Font blocks and the theme
        // picker join this card below.
        let mut appearance_col = column![
            self.nav_toggle_row(crate::i18n::t("bold_bright"), self.prefs.bold_is_bright, Message::Settings(SettingsMessage::ToggleBoldIsBright)),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("keyword_highlight"), self.prefs.keyword_highlight, Message::Settings(SettingsMessage::ToggleKeywordHighlight)),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("smart_contrast"), self.prefs.smart_contrast, Message::Settings(SettingsMessage::ToggleSmartContrast)),
            Space::new().height(10),
            self.nav_pick_row(
                crate::i18n::t("terminal_opacity"),
                crate::theme::OPACITY_STEPS
                    .iter()
                    .map(|p| format!("{p}%"))
                    .collect::<Vec<_>>(),
                format!("{}%", self.prefs.terminal_opacity),
                |s: &String| s.clone(),
                120.0,
                |v| Message::Settings(SettingsMessage::TerminalOpacityChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("terminal_opacity_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ];
        // Background picture. The fit and dim rows only exist while a
        // picture is set: they are meaningless without one, and the
        // optional-features rule says a control that governs nothing
        // should not be on screen.
        let has_bg_image = !self.prefs.terminal_bg_image.trim().is_empty();
        appearance_col = appearance_col
            .push(Space::new().height(10))
            .push(self.terminal_bg_image_row())
            .push(Space::new().height(4))
            .push(
                text(crate::i18n::t("terminal_bg_image_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            );
        if has_bg_image {
            appearance_col = appearance_col
                .push(Space::new().height(10))
                .push(self.nav_pick_row(
                    crate::i18n::t("terminal_bg_fit"),
                    oryxis_terminal::BgFit::ALL
                        .iter()
                        .map(|f| {
                            crate::i18n::t(crate::terminal_appearance::bg_fit_label_key(*f))
                                .to_string()
                        })
                        .collect::<Vec<_>>(),
                    crate::i18n::t(crate::terminal_appearance::bg_fit_label_key(
                        oryxis_terminal::BgFit::from_str_or_default(&self.prefs.terminal_bg_fit),
                    ))
                    .to_string(),
                    |s: &String| s.clone(),
                    160.0,
                    |v| Message::Settings(SettingsMessage::TerminalBgFitChanged(v)),
                ))
                .push(Space::new().height(10))
                .push(self.nav_pick_row(
                    crate::i18n::t("terminal_bg_dim"),
                    crate::terminal_appearance::DIM_STEPS
                        .iter()
                        .map(|p| format!("{p}%"))
                        .collect::<Vec<_>>(),
                    format!("{}%", self.prefs.terminal_bg_dim),
                    |s: &String| s.clone(),
                    120.0,
                    |v| Message::Settings(SettingsMessage::TerminalBgDimChanged(v)),
                ))
                .push(Space::new().height(4))
                .push(
                    text(crate::i18n::t("terminal_bg_dim_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                );
        }

        // Notifications: everything whose job is to GET YOUR ATTENTION.
        // Smart tabs and its threshold live here rather than with the tab
        // settings because the threshold is "tell me when a command has
        // run this long", which is the same promise as the bell.
        let notifications_col = column![
            self.nav_pick_row(
                crate::i18n::t("terminal_bell"),
                crate::util::BellMode::ALL
                    .iter()
                    .map(|m| crate::i18n::t(m.label_key()).to_string())
                    .collect::<Vec<_>>(),
                crate::i18n::t(self.prefs.bell_mode.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::BellModeChanged(v)),
            ),
            Space::new().height(10),
            self.nav_pick_row(
                crate::i18n::t("terminal_notification"),
                crate::util::NotificationMode::ALL
                    .iter()
                    .map(|m| crate::i18n::t(m.label_key()).to_string())
                    .collect::<Vec<_>>(),
                crate::i18n::t(self.prefs.notification_mode.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::NotificationModeChanged(v)),
            ),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("smart_tabs"), self.prefs.smart_tabs, Message::Settings(SettingsMessage::SettingToggleSmartTabs)),
            self.smart_tabs_threshold_row(),
        ];

        // Integration: what the REMOTE END is allowed to drive, and what
        // we record off the session. Every row here is a channel between
        // the shell and the app rather than a preference about drawing.
        let integration_col = column![
            self.nav_pick_row(
                crate::i18n::t("terminal_clipboard"),
                crate::util::ClipboardAccess::ALL
                    .iter()
                    .map(|m| crate::i18n::t(m.label_key()).to_string())
                    .collect::<Vec<_>>(),
                crate::i18n::t(self.prefs.clipboard_access.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::ClipboardAccessChanged(v)),
            ),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("terminal_auto_title"), crate::state::auto_title_enabled(), Message::Settings(SettingsMessage::ToggleTerminalAutoTitle)),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("command_history_capture"), self.prefs.command_history, Message::Settings(SettingsMessage::ToggleCommandHistory)),
            self.shell_integration_row(),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("cmd_history_file"), self.prefs.command_history_file, Message::CommandHistory(CommandHistoryMessage::ToggleCommandHistoryFile)),
            self.command_history_dir_row(),
        ];

        // Sidebar: which region each tab docks to (issue #102) and how
        // the panel opens.
        let sidebar_col = column![
            self.nav_toggle_row(
                crate::i18n::t("sidebar_auto_open"),
                self.prefs.sidebar_auto_open,
                Message::Settings(SettingsMessage::SettingToggleSidebarAutoOpen),
            ),
            Space::new().height(4),
            text(crate::i18n::t("sidebar_auto_open_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(10),
            self.sidebar_default_tab_row(),
            Space::new().height(10),
            self.sidebar_tab_side_rows(),
        ];

        // The +/- stepper maps naturally onto the picker action:
        // Left decreases, Right increases the font size.
        let font_size_block = column![
            self.settings_nav_slot_labeled(
                t("terminal_font_size"),
                crate::keynav::RowAction::picker(
                    Some(Message::Settings(SettingsMessage::TerminalFontSizeDecrease)),
                    Some(Message::Settings(SettingsMessage::TerminalFontSizeIncrease)),
                ),
                8.0,
                dir_row(vec![
                text(crate::i18n::t("terminal_font_size")).size(13).color(OryxisColors::t().text_primary).into(),
                Space::new().width(Length::Fill).into(),
                button(
                    container(text("\u{2212}").size(14).color(OryxisColors::t().text_primary))
                        .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 }),
                )
                .on_press(Message::Settings(SettingsMessage::TerminalFontSizeDecrease))
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
                .on_press(Message::Settings(SettingsMessage::TerminalFontSizeIncrease))
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
                ]).align_y(iced::Alignment::Center).into(),
            ),
        ];

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
        // The sample carries the picked weight too, so "does this font
        // have a Medium" is answered by looking at it.
        let preview_weight = self.terminal_font_weight.font_weight();
        let preview_font = fonts
            .iter()
            .find(|f| f.as_str() == self.terminal_font_name)
            .map(|f| iced::Font {
                family: iced::font::Family::Name(f.as_str()),
                weight: preview_weight,
                ..iced::Font::MONOSPACE
            })
            .unwrap_or(iced::Font {
                weight: preview_weight,
                ..iced::Font::MONOSPACE
            });
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
        // Left/Right cycle the installed fonts without opening the
        // dropdown; `fonts` is a `'static` slice so cycle_pair borrows
        // it directly.
        let (font_prev, font_next) = crate::keynav::slots::cycle_pair(
            fonts,
            &self.terminal_font_name,
            |v| Message::Settings(SettingsMessage::TerminalFontChanged(v)),
        );
        let mut font_picker_block = column![
            text(crate::i18n::t("terminal_font")).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("setting_font_desc"))
                .size(11).color(OryxisColors::t().text_muted),
            Space::new().height(8),
            self.settings_nav_slot_labeled(
                t("terminal_font"),
                crate::keynav::RowAction::picker(font_prev, font_next),
                8.0,
                pick_list(
                    Some(self.terminal_font_name.clone()),
                    fonts,
                    |s: &String| s.clone(),
                )
                .on_select(|v| Message::Settings(SettingsMessage::TerminalFontChanged(v)))
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(260).padding(10).style(crate::widgets::rounded_pick_list_style)
                .into(),
            ),
        ];
        // Font pack affordance (issue #109): the pick_list can't
        // annotate rows, so the downloadable-but-not-yet-loaded pack
        // entries are named here instead. The line disappears once
        // every pack font has been requested (they are ordinary picker
        // entries from then on).
        let pack_missing: Vec<&str> = crate::fonts::PACK_FONTS
            .iter()
            .filter(|p| {
                !p.faces
                    .iter()
                    .any(|f| self.loaded_pack_fonts.contains(f.key()))
            })
            .map(|p| p.family)
            .collect();
        if !pack_missing.is_empty() {
            font_picker_block = font_picker_block.push(Space::new().height(6)).push(
                text(format!(
                    "{} {}",
                    t("font_pack_available"),
                    pack_missing.join(", ")
                ))
                .size(11)
                .color(OryxisColors::t().text_muted),
            );
        }
        // Font weight (issue #155). Sits under the family because it
        // is a property OF the family: the picker always offers the
        // four, and the line below says when the picked family has no
        // face for the one selected (cosmic-text has no synthetic
        // emboldening, so that pick would change nothing on screen).
        let weights = crate::fonts::TerminalFontWeight::ALL;
        let (weight_prev, weight_next) = crate::keynav::slots::cycle_pair(
            &weights,
            &self.terminal_font_weight,
            |w| Message::Settings(SettingsMessage::TerminalFontWeightChanged(w)),
        );
        let font_picker_block = font_picker_block
            .push(Space::new().height(14))
            .push(
                text(t("terminal_font_weight"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
            )
            .push(Space::new().height(8))
            .push(self.settings_nav_slot_labeled(
                t("terminal_font_weight"),
                crate::keynav::RowAction::picker(weight_prev, weight_next),
                8.0,
                pick_list(
                    Some(self.terminal_font_weight),
                    weights.to_vec(),
                    |w: &crate::fonts::TerminalFontWeight| w.to_string(),
                )
                .on_select(|w| {
                    Message::Settings(SettingsMessage::TerminalFontWeightChanged(w))
                })
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(260).padding(10).style(crate::widgets::rounded_pick_list_style)
                .into(),
            ));
        // The weight's honesty line, right under the picker it is about.
        let font_picker_block = if crate::app::terminal_font_serves_weight(
            &self.terminal_font_name,
            self.terminal_font_weight.css(),
        ) {
            font_picker_block
        } else {
            font_picker_block.push(Space::new().height(6)).push(
                text(t("font_weight_unavailable"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
        };

        // Stroke widening. Sits under the weight because the two are
        // the same question asked twice: the weight picks a face the
        // font shipped, this thickens whatever face resolved. It is
        // what compensates for us rasterizing raw coverage while the
        // platform text stacks widen strokes before compositing.
        let thicknesses = crate::fonts::TextThickness::ALL;
        let (thick_prev, thick_next) = crate::keynav::slots::cycle_pair(
            &thicknesses,
            &self.terminal_text_thickness,
            |t| Message::Settings(SettingsMessage::TerminalTextThicknessChanged(t)),
        );
        let font_picker_block = font_picker_block
            .push(Space::new().height(14))
            .push(
                text(t("terminal_text_thickness"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
            )
            .push(Space::new().height(4))
            .push(
                text(t("setting_text_thickness_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            )
            .push(Space::new().height(8))
            .push(self.settings_nav_slot_labeled(
                t("terminal_text_thickness"),
                crate::keynav::RowAction::picker(thick_prev, thick_next),
                8.0,
                pick_list(
                    Some(self.terminal_text_thickness),
                    thicknesses.to_vec(),
                    |v: &crate::fonts::TextThickness| v.to_string(),
                )
                .on_select(|v| {
                    Message::Settings(SettingsMessage::TerminalTextThicknessChanged(v))
                })
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(260).padding(10).style(crate::widgets::rounded_pick_list_style)
                .into(),
            ));
        let font_picker_block = font_picker_block
            .push(Space::new().height(12))
            .push(font_preview);
        // One Appearance card: rendering toggles, then the font
        // size stepper and the font picker + live sample. The
        // terminal-theme gallery keeps its own card below (its own
        // sub-theme, and a grid that large reads better boxed
        // separately).
        let appearance_section = panel_section(
            appearance_col
                .push(Space::new().height(16))
                .push(font_size_block)
                .push(Space::new().height(16))
                .push(font_picker_block),
        );

        // Terminal theme picker. First card is the "follow
        // app theme" sentinel (terminal_theme_override = None);
        // the rest are explicit palette previews so the user
        // can compare colours without applying each one. Per-host
        // overrides configured via the icon picker still win
        // over this global pick. Each card is a keyboard row (Enter
        // applies / opens it); built after the font picker so the
        // recording matches the render order.
        let app_theme_name = crate::theme::AppTheme::active().name();
        let follow_palette = self
            .terminal_palette_for_name(app_theme_name)
            .unwrap_or_default();
        let follow_label =
            format!("{} ({})", t("terminal_theme_follow_app"), app_theme_name);
        // The grid lives in a modal now, not inline (owner ask): with 17
        // built-ins plus every custom theme it was the tallest thing in
        // Settings by a wide margin, and it pushed every group below it
        // out of reach. The row shows the palette in force as a real
        // card, so the preview survives the move; clicking it opens the
        // gallery, the same shape the host editor's picker already has.
        let current_theme_name = self
            .terminal_theme_override
            .clone()
            .unwrap_or_else(|| follow_label.clone());
        let current_palette = self
            .terminal_theme_override
            .as_deref()
            .and_then(|n| self.terminal_palette_for_name(n))
            .unwrap_or(follow_palette);
        let theme_picker_section = panel_section(column![
            text(t("terminal_theme")).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("terminal_theme_desc"))
                .size(11).color(OryxisColors::t().text_muted),
            Space::new().height(10),
            self.settings_nav_slot_labeled(
                t("terminal_theme"),
                crate::keynav::RowAction::activate(Message::Settings(
                    SettingsMessage::OpenTerminalThemeGallery,
                )),
                10.0,
                crate::widgets::terminal_theme_card(
                    current_palette,
                    &current_theme_name,
                    true,
                    Message::Settings(SettingsMessage::OpenTerminalThemeGallery),
                ),
            ),
        ]);

        // Grouped under "h2" headers, same pattern as Interface:
        // Behavior (selection, delimiters, scrollback) then
        // Appearance (rendering, font, theme). Connection + logging
        // knobs live in their own sections.
        // Split panes get their own block: they are the only settings
        // that do nothing at all until a tab is split, so mixing them
        // into Appearance made them read as global terminal knobs.
        let split_panes_section = panel_section(column![
            self.nav_pick_row(
                crate::i18n::t("pane_gap"),
                vec!["0".into(), "4".into(), "8".into(), "12".into()],
                self.prefs.pane_gap.clone(),
                |v| {
                    if v == "0" {
                        crate::i18n::t("pane_gap_none").to_string()
                    } else {
                        format!("{v} px")
                    }
                },
                140.0,
                |v| Message::Settings(SettingsMessage::PaneGapChanged(v)),
            ),
            Space::new().height(10),
            self.nav_toggle_row(
                crate::i18n::t("pane_border_inactive"),
                self.prefs.pane_border_inactive,
                Message::Settings(SettingsMessage::TogglePaneBorderInactive),
            ),
        ]);

        use crate::widgets::settings_group_header as gh;
        scrollable(
            container(
                column![
                    gh(crate::i18n::t("terminal_group_behavior")),
                    Space::new().height(8),
                    behavior_section,
                    Space::new().height(18),
                    gh(crate::i18n::t("terminal_group_appearance")),
                    Space::new().height(8),
                    appearance_section,
                    Space::new().height(12),
                    // Right after the appearance card: rules are what
                    // the grid looks like too, just the half the user
                    // writes themselves.
                    self.highlight_rules_section(),
                    Space::new().height(12),
                    theme_picker_section,
                    Space::new().height(18),
                    gh(crate::i18n::t("terminal_group_split_panes")),
                    Space::new().height(8),
                    split_panes_section,
                    Space::new().height(18),
                    gh(crate::i18n::t("terminal_group_notifications")),
                    Space::new().height(8),
                    panel_section(notifications_col),
                    Space::new().height(18),
                    gh(crate::i18n::t("terminal_group_integration")),
                    Space::new().height(8),
                    panel_section(integration_col),
                    Space::new().height(18),
                    gh(crate::i18n::t("terminal_group_sidebar")),
                    Space::new().height(8),
                    panel_section(sidebar_col),
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
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-terminal-scroll"))
        .on_scroll(|v| Message::Settings(SettingsMessage::SectionScrolled(v.relative_offset().y)))
        .height(Length::Fill)
        .into()
    }
}
