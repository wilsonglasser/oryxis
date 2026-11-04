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
                STab::Chat => self.vaults.cur().ai.enabled,
                STab::Files => self.vaults.cur().sftp_enabled,
                STab::Monitor => self.vaults.cur().prefs.host_monitoring,
                _ => true,
            })
            .collect();
        if let Some(sel) = self.vaults.cur().prefs.sidebar_default_tab
            && !tabs.contains(&sel)
        {
            tabs.push(sel);
        }
        let last = crate::i18n::t("sidebar_default_last").to_string();
        let mut options = vec![last.clone()];
        options.extend(tabs.iter().map(|t| crate::i18n::t(t.label_key()).to_string()));
        let selected = match self.vaults.cur().prefs.sidebar_default_tab {
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
        ].boxed()
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
                STab::Chat => self.vaults.cur().ai.enabled,
                STab::Files => self.vaults.cur().sftp_enabled,
                STab::Monitor => self.vaults.cur().prefs.host_monitoring,
                STab::Tmux => self.vaults.cur().prefs.tmux_manager,
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
            let selected = placement_label(self.vaults.cur().prefs.sidebar_tab_placement(tab));
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
        col.boxed()
    }

    fn smart_tabs_threshold_row(&self) -> Element<'_, Message> {
        if !self.vaults.cur().prefs.smart_tabs {
            return Space::new().boxed();
        }
        column![
            Space::new().height(10),
            self.nav_pick_row(
                crate::i18n::t("smart_tabs_threshold"),
                crate::smart_tabs::threshold_options()
                    .into_iter()
                    .map(|(_, l)| l)
                    .collect::<Vec<_>>(),
                crate::smart_tabs::threshold_label(self.vaults.cur().prefs.smart_long_secs),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::SmartTabsThresholdChanged(v)),
            ),
        ].boxed()
    }

    /// Sub-row for the history suggestions while typing (issue #266),
    /// nested under the capture toggle like the key row below and shown
    /// only while it is on: without the capture there is no history to
    /// offer and the mirror the suggestions read is not fed, so a hidden
    /// row is an OFF row (`history_suggest_enabled` reads both).
    fn history_suggest_row(&self) -> Element<'_, Message> {
        if !self.vaults.cur().prefs.command_history {
            return Space::new().boxed();
        }
        let indent = if crate::i18n::is_rtl_layout() {
            Padding { right: 22.0, ..Padding::ZERO }
        } else {
            Padding { left: 22.0, ..Padding::ZERO }
        };
        column![
            Space::new().height(10),
            container(self.nav_toggle_row(
                crate::i18n::t("terminal_history_suggest"),
                self.vaults.cur().prefs.terminal_history_suggest,
                Message::Settings(SettingsMessage::ToggleTerminalHistorySuggest),
            ))
            .width(Length::Fill)
            .padding(indent),
            Space::new().height(4),
            container(
                text(crate::i18n::t("terminal_history_suggest_desc"))
                    .size(11)
                    .color(crate::theme::OryxisColors::t().text_muted),
            )
            .width(Length::Fill)
            .padding(indent),
        ]
        .boxed()
    }

    /// Sub-row for the shell-integration key: what it is for, the Copy
    /// button that puts the snippet (key already substituted) on the
    /// clipboard, and Rotate.
    ///
    /// Shown under the capture toggle and only while it is on: the key
    /// does nothing while nothing is being captured, and a control that
    /// cannot matter yet is noise. Nested like the command-log folder row.
    fn shell_integration_row(&self) -> Element<'_, Message> {
        if !self.vaults.cur().prefs.command_history {
            return Space::new().boxed();
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
                    Space::new().width(Length::Fill).boxed(),
                    copy,
                    Space::new().width(8).boxed(),
                    rotate,
                ])
                .align_y(iced::Alignment::Center),
            ]
            .width(Length::Fill),
        )
        .padding(Padding { top: 8.0, ..indent })
        .width(Length::Fill)
        .boxed()
    }

    /// Row for the terminal background picture: its label, the chosen
    /// file name (or "None"), a Browse button and, once one is set, a
    /// Remove. Both buttons record keyboard rows in visual order, so
    /// Tab reaches Remove right after Browse.
    fn terminal_bg_image_row(&self) -> Element<'_, Message> {
        let path = self.vaults.cur().prefs.terminal_bg_image.trim();
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
                .boxed(),
            Space::new().width(Length::Fill).boxed(),
            text(current)
                .size(12)
                .color(crate::theme::OryxisColors::t().text_muted)
                .boxed(),
            Space::new().width(10).boxed(),
            browse,
        ];
        if !path.is_empty() {
            row.push(Space::new().width(8).boxed());
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
            .boxed()
    }

    /// Sub-row for the command-log folder, shown only while the
    /// live-append toggle is on: the effective folder (default
    /// `~/.oryxis/command-history/`) with a Browse button, and a Reset
    /// when a custom folder is set, indented like the other nested
    /// sub-options.
    fn command_history_dir_row(&self) -> Element<'_, Message> {
        if !self.vaults.cur().prefs.command_history_file {
            return Space::new().boxed();
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
        // The folder is the device's (`command_history_file_dir`), the
        // toggle above it the vault's.
        let mut row = crate::widgets::dir_row(vec![
            crate::widgets::device_badge(),
            Space::new().width(6).boxed(),
            text(dir)
                .size(12)
                .color(crate::theme::OryxisColors::t().text_muted)
                .width(Length::Fill)
                .boxed(),
            Space::new().width(10).boxed(),
            change,
        ]);
        // Reset-to-default only when a custom folder is set.
        if self.device_prefs.command_history_file_dir.is_some() {
            let reset = self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::CommandHistory(CommandHistoryMessage::ClearCommandHistoryDir)),
                8.0,
                crate::widgets::styled_button_opt(
                    crate::i18n::t("reset"),
                    Some(Message::CommandHistory(CommandHistoryMessage::ClearCommandHistoryDir)),
                    crate::theme::OryxisColors::t().text_muted,
                ),
            );
            row = row.push(Space::new().width(8).boxed()).push(reset);
        }
        container(row.align_y(iced::Alignment::Center))
            .padding(Padding { top: 8.0, ..indent })
            .width(Length::Fill)
            .boxed()
    }

    /// The folder a snippet's save steps write into: the resolved path
    /// with a Browse button, and a Reset when a custom folder is set.
    fn snippet_output_dir_row(&self) -> Element<'_, Message> {
        let browse = self.settings_nav_slot_labeled(
            crate::i18n::t("snippet_output_dir"),
            crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::PickSnippetOutputDir)),
            8.0,
            crate::widgets::styled_button_opt(
                crate::i18n::t("browse"),
                Some(Message::Settings(SettingsMessage::PickSnippetOutputDir)),
                crate::theme::OryxisColors::t().accent,
            ),
        );
        let mut row = crate::widgets::dir_row(vec![
            column![
                crate::widgets::setting_label(crate::i18n::t("snippet_output_dir"), 13.0, crate::theme::OryxisColors::t().text_primary),
                Space::new().height(2),
                text(self.snippet_output_dir().display().to_string())
                    .size(11)
                    .color(crate::theme::OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .boxed(),
            Space::new().width(10).boxed(),
            browse,
        ]);
        if self.device_prefs.snippet_output_dir.is_some() {
            let reset = self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ClearSnippetOutputDir)),
                8.0,
                crate::widgets::styled_button_opt(
                    crate::i18n::t("reset"),
                    Some(Message::Settings(SettingsMessage::ClearSnippetOutputDir)),
                    crate::theme::OryxisColors::t().text_muted,
                ),
            );
            row = row.push(Space::new().width(8).boxed()).push(reset);
        }
        container(row.align_y(iced::Alignment::Center))
            .padding(Padding { top: 10.0, ..Padding::ZERO })
            .width(Length::Fill)
            .boxed()
    }

    /// The terminal-theme gallery: every built-in and custom palette as a
    /// card, plus the create / import entries. Lives behind a modal
    /// rather than inline in Settings, where 31 built-ins plus the user's
    /// own pushed every group below it off the page.
    ///
    /// The cards record themselves as keyboard rows in RENDER order, same
    /// as any settings row, so the modal is walkable the moment it opens.
    pub(crate) fn terminal_theme_gallery(&self) -> Element<'_, Message> {
        // Filter: the typed line matches every card's visible label,
        // its localized tags and their English keywords, and the chip
        // gates on tone (`theme_tags::theme_matches`, the one rule every
        // theme list applies). The follow sentinel takes part with the
        // palette it previews; the action cards only by label.
        let gallery_filter = self.vaults.cur().theme_ui.gallery_filter.trim().to_lowercase();
        let gallery_tone = self.vaults.cur().theme_ui.gallery_tone;
        let shows = |label: &str, traits: Option<&[oryxis_terminal::ThemeTrait]>| {
            crate::theme_tags::theme_matches(&gallery_filter, gallery_tone, label, traits)
        };
        // The filter is the first keyboard row of the modal, so it must
        // RECORD before the cards do (construction order is record
        // order); the widget itself is drawn later, ring applied by
        // index. The chips record right after it, in the order they
        // are drawn between the line and the grid.
        let filter_idx = self.settings_nav_record(crate::keynav::RowAction::input(
            iced::widget::Id::new("terminal-theme-gallery-filter"),
        ));
        let tone_chips: Vec<Element<'_, Message>> = crate::theme_tags::TONE_CHOICES
            .iter()
            .map(|(tone, key)| {
                let msg = Message::Settings(SettingsMessage::ThemeGalleryToneChanged(*tone));
                self.settings_nav_slot(
                    crate::keynav::RowAction::activate(msg.clone()),
                    14.0,
                    crate::theme_tags::tone_chip(t(key), gallery_tone == *tone, msg),
                )
            })
            .collect();
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
        let follow_traits = follow_palette.traits();
        if shows(&follow_label, Some(&follow_traits)) {
            theme_cards.push(self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::TerminalThemeChanged(String::new()))),
                10.0,
                crate::widgets::terminal_theme_card(
                    follow_palette,
                    &follow_label,
                    self.vaults.cur().terminal_theme_override.is_none(),
                    Message::Settings(SettingsMessage::TerminalThemeChanged(String::new())),
                ),
            ));
        }
        for (bidx, theme) in oryxis_terminal::TerminalTheme::ALL.iter().enumerate() {
            let traits = theme.traits();
            if !shows(theme.name(), Some(&traits)) {
                continue;
            }
            let is_selected = self
                .vaults.cur().terminal_theme_override
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
        for (idx, ct) in self.vaults.cur().custom_terminal_themes.iter().enumerate() {
            let palette = self
                .terminal_palette_for_name(&ct.name)
                .unwrap_or_default();
            let traits = palette.traits();
            if !shows(&ct.name, Some(&traits)) {
                continue;
            }
            let is_selected =
                self.vaults.cur().terminal_theme_override.as_deref() == Some(ct.name.as_str());
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
        if shows(t("theme_new_custom"), None) {
            theme_cards.push(self.settings_nav_slot_labeled(
                t("theme_new_custom"),
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ThemeEditorNew)),
                10.0,
                crate::views::settings_themes::terminal_theme_add_card(),
            ));
        }
        if shows(t("theme_import"), None) {
            theme_cards.push(self.settings_nav_slot_labeled(
                t("theme_import"),
                crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ThemeImportOpen)),
                10.0,
                crate::views::settings_themes::terminal_theme_import_card(),
            ));
        }
        if shows(t("theme_community"), None) {
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
            Space::new().width(Length::Fill).boxed(),
            crate::widgets::form_cancel_button(Message::Settings(
                SettingsMessage::CloseTerminalThemeGallery,
            )),
        ])
        .align_y(iced::Alignment::Center)
        .boxed();
        let filter_input = self.settings_nav_ring_at(
            filter_idx,
            10.0,
            iced::widget::text_input(t("filter_placeholder"), &self.vaults.cur().theme_ui.gallery_filter)
                .id(iced::widget::Id::new("terminal-theme-gallery-filter"))
                .on_input(|v| {
                    Message::Settings(SettingsMessage::ThemeGalleryFilterChanged(v))
                })
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .boxed(),
        );
        let card = container(
            column![
                text(t("terminal_theme")).size(18).color(OryxisColors::t().text_primary),
                Space::new().height(6),
                text(t("terminal_theme_desc")).size(12).color(OryxisColors::t().text_muted),
                Space::new().height(12),
                filter_input,
                Space::new().height(8),
                crate::widgets::dir_row(tone_chips).spacing(6),
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
        card.boxed()
    }

    /// Row for the download folder: the resolved path (default or
    /// configured) plus a Browse button, and a Reset when a custom folder
    /// is set. Always shown (transfers work regardless of other toggles).
    fn default_download_dir_row(&self) -> Element<'_, Message> {
        let configured = self.device_prefs.zmodem_download_dir.trim();
        let shown = if configured.is_empty() {
            dirs::download_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.oryxis/downloads".to_string())
        } else {
            configured.to_string()
        };
        let browse = self.settings_nav_slot_labeled(
            t("default_download_dir"),
            crate::keynav::RowAction::activate(Message::Zmodem(ZmodemMessage::PickZmodemDownloadDir)),
            8.0,
            crate::widgets::styled_button_opt(
                crate::i18n::t("browse"),
                Some(Message::Zmodem(ZmodemMessage::PickZmodemDownloadDir)),
                crate::theme::OryxisColors::t().accent,
            ),
        );
        let mut row = crate::widgets::dir_row(vec![
            column![
                crate::widgets::setting_label(crate::i18n::t("default_download_dir"), 13.0, crate::theme::OryxisColors::t().text_primary),
                Space::new().height(2),
                text(shown)
                    .size(11)
                    .color(crate::theme::OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .boxed(),
            Space::new().width(10).boxed(),
            browse,
        ]);
        // Reset-to-default only when a custom folder is set.
        if !configured.is_empty() {
            let reset = self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Zmodem(ZmodemMessage::ClearZmodemDownloadDir)),
                8.0,
                crate::widgets::styled_button_opt(
                    crate::i18n::t("reset"),
                    Some(Message::Zmodem(ZmodemMessage::ClearZmodemDownloadDir)),
                    crate::theme::OryxisColors::t().text_muted,
                ),
            );
            row = row.push(Space::new().width(8).boxed()).push(reset);
        }
        container(row.align_y(iced::Alignment::Center))
            .padding(Padding { top: 8.0, ..Padding::ZERO })
            .width(Length::Fill)
            .boxed()
    }

    pub(crate) fn view_settings_terminal(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order: the sections below
        // are deliberately CONSTRUCTED in the same order they render
        // (recording happens at construction), so keep any new section
        // in its on-screen position.
        self.keynav_settings_reset();
        let mut toggles_col: iced::widget::Column<iced::Element<'_, Message>> = column![
            self.nav_toggle_row(crate::i18n::t("copy_on_select"), self.vaults.cur().prefs.copy_on_select, Message::Settings(SettingsMessage::ToggleCopyOnSelect)),
        ];
        // Right-click scheme (PuTTY's Context menu / Paste / Extend). The
        // single authority for the gesture.
        let rc_is_paste =
            self.vaults.cur().prefs.terminal_right_click == crate::util::RightClickMode::Paste;
        toggles_col = toggles_col.push(Space::new().height(10).boxed()).push(self.nav_pick_row(
            crate::i18n::t("terminal_right_click"),
            crate::util::RightClickMode::ALL
                .iter()
                .map(|m| crate::i18n::t(m.label_key()).to_string())
                .collect::<Vec<_>>(),
            crate::i18n::t(self.vaults.cur().prefs.terminal_right_click.label_key()).to_string(),
            |s: &String| s.clone(),
            200.0,
            |v| Message::Settings(SettingsMessage::TerminalRightClickChanged(v)),
        ));
        // "Copy on right-click" is a sub-option of copy-on-select, and
        // only meaningful when the right-click scheme is Paste (Menu and
        // Extend repurpose the gesture entirely). Hidden otherwise.
        if self.vaults.cur().prefs.copy_on_select && rc_is_paste {
            let indent = if crate::i18n::is_rtl_layout() {
                Padding { right: 22.0, ..Padding::ZERO }
            } else {
                Padding { left: 22.0, ..Padding::ZERO }
            };
            toggles_col = toggles_col
                .push(Space::new().height(8).boxed())
                .push(
                    container(self.nav_toggle_row(
                        crate::i18n::t("copy_requires_right_click"),
                        self.vaults.cur().prefs.right_click_copy,
                        Message::Settings(SettingsMessage::ToggleRightClickCopy),
                    ))
                    .padding(indent).boxed(),
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
            .push(Space::new().height(10).boxed())
            .push(self.nav_toggle_row(
                crate::i18n::t("middle_click_paste"),
                self.middle_click_pastes(),
                Message::Settings(SettingsMessage::ToggleMiddleClickPaste),
            ));
        // Careful paste: the multi-line paste guard (line-count preview
        // before anything reaches the session). Default on; the toggle is
        // the power-user opt-out.
        toggles_col = toggles_col
            .push(Space::new().height(10).boxed())
            .push(self.nav_toggle_row(
                crate::i18n::t("careful_paste_label"),
                self.vaults.cur().prefs.careful_paste,
                Message::Settings(SettingsMessage::ToggleCarefulPaste),
            ))
            .push(Space::new().height(4).boxed())
            .push(
                text(crate::i18n::t("careful_paste_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted).boxed(),
            );
        // Content heuristics (bidi/invisible, control bytes, curl|sh,
        // homographs): its own switch so the multi-line check and the
        // suspicious-content check opt in/out independently.
        toggles_col = toggles_col
            .push(Space::new().height(10).boxed())
            .push(self.nav_toggle_row(
                crate::i18n::t("paste_guard_label"),
                self.vaults.cur().prefs.paste_guard,
                Message::Settings(SettingsMessage::TogglePasteGuard),
            ))
            .push(Space::new().height(4).boxed())
            .push(
                text(crate::i18n::t("paste_guard_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted).boxed(),
            );
        // The whole Behavior group shares one card: selection /
        // clipboard toggles, then the word-delimiter and scrollback
        // sub-blocks (each keeps its 13 px sub-title). Constructed
        // in visual order so the keyboard rows record in order.
        let toggles_col = toggles_col.push(Space::new().height(16).boxed());
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
                    text_input(oryxis_terminal::DEFAULT_WORD_DELIMITERS, &self.vaults.cur().prefs.word_delimiters)
                        .id(iced::widget::Id::new("set-terminal-word-delimiters"))
                        .on_input(|v| Message::Settings(SettingsMessage::SettingWordDelimitersChanged(v)))
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().width(8).boxed(),
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
                text_input("10000", &self.vaults.cur().prefs.scrollback_rows)
                    .id(iced::widget::Id::new("set-terminal-scrollback"))
                    .on_input(|v| Message::Settings(SettingsMessage::SettingScrollbackChanged(v)))
                    .padding(10)
                    .width(240)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                    .boxed(),
            ),
            // PuTTY's two "jump back to the live edge" behaviors, so a user
            // stranded deep in history returns without reaching for the
            // wheel / scrollbar.
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("scrollback_reset_keypress"),
                self.vaults.cur().prefs.scrollback_reset_keypress,
                Message::Settings(SettingsMessage::ToggleScrollbackResetKeypress),
            ),
            Space::new().height(10),
            self.nav_toggle_row(
                crate::i18n::t("scrollback_reset_output"),
                self.vaults.cur().prefs.scrollback_reset_output,
                Message::Settings(SettingsMessage::ToggleScrollbackResetOutput),
            ),
            // Issue #117. On by default: a suggestion that never sends
            // itself costs nothing to ignore, and the alternative is
            // typing a password the app is already holding.
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("terminal_password_autofill"),
                self.vaults.cur().prefs.terminal_password_autofill,
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
                self.vaults.cur().prefs.terminal_link_confirm,
                Message::Settings(SettingsMessage::ToggleTerminalLinkConfirm),
            ),
            Space::new().height(4),
            text(crate::i18n::t("terminal_link_confirm_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(12),
            self.nav_toggle_row(
                crate::i18n::t("terminal_link_tunnel"),
                self.vaults.cur().prefs.terminal_link_tunnel,
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
        //
        // ZMODEM's own knobs live here rather than under Settings > SFTP:
        // `sz` runs in a terminal, so what it does about a destination is
        // a terminal question, and the two protocols ask separately.
        let zmodem_block = column![
            text(t("setting_zmodem_ask_download_dir"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("setting_zmodem_ask_download_dir_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            self.nav_toggle_row(
                t("setting_zmodem_ask_download_dir_toggle"),
                self.vaults.cur().prefs.zmodem_ask_download_dir,
                Message::Settings(SettingsMessage::ToggleZmodemAskDownloadDir),
            ),
            Space::new().height(14),
            self.default_download_dir_row(),
        ];
        let behavior_section = panel_section(
            toggles_col
                .push(word_delimiters_block.boxed())
                .push(Space::new().height(16).boxed())
                .push(scrollback_block.boxed())
                .push(Space::new().height(16).boxed())
                .push(zmodem_block.boxed()),
        );

        // Settings > Terminal used to have one "Appearance" card holding
        // everything that was not clipboard behaviour: the bell, OSC 52,
        // OSC 9, smart tabs, the sidebar dock, command-history capture.
        // None of that is appearance. Split into four cards, each named
        // after what the settings inside it actually govern.

        // Appearance: what the grid LOOKS like. Font blocks and the theme
        // picker join this card below.
        let mut appearance_col = column![
            self.nav_toggle_row(crate::i18n::t("bold_bright"), self.vaults.cur().prefs.bold_is_bright, Message::Settings(SettingsMessage::ToggleBoldIsBright)),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("keyword_highlight"), self.vaults.cur().prefs.keyword_highlight, Message::Settings(SettingsMessage::ToggleKeywordHighlight)),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("smart_contrast"), self.vaults.cur().prefs.smart_contrast, Message::Settings(SettingsMessage::ToggleSmartContrast)),
            Space::new().height(10),
            // The caret a pane starts with (issue #266). A program that
            // sets its own (vim's modes, a prompt's DECSCUSR) keeps it,
            // which the line under the pair says.
            self.nav_pick_row(
                crate::i18n::t("terminal_cursor_shape"),
                crate::util::CursorShapeMode::ALL
                    .iter()
                    .map(|m| crate::i18n::t(m.label_key()).to_string())
                    .collect::<Vec<_>>(),
                crate::i18n::t(self.vaults.cur().prefs.terminal_cursor_shape.label_key()).to_string(),
                |s: &String| s.clone(),
                160.0,
                |v| Message::Settings(SettingsMessage::TerminalCursorShapeChanged(v)),
            ),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("terminal_cursor_blink"), self.vaults.cur().prefs.terminal_cursor_blink, Message::Settings(SettingsMessage::ToggleTerminalCursorBlink)),
            Space::new().height(4),
            text(crate::i18n::t("terminal_cursor_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(10),
            self.nav_pick_row(
                crate::i18n::t("terminal_opacity"),
                crate::theme::OPACITY_STEPS
                    .iter()
                    .map(|p| format!("{p}%"))
                    .collect::<Vec<_>>(),
                format!("{}%", self.vaults.cur().prefs.terminal_opacity),
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
        let has_bg_image = !self.vaults.cur().prefs.terminal_bg_image.trim().is_empty();
        appearance_col = appearance_col
            .push(Space::new().height(10).boxed())
            .push(self.terminal_bg_image_row())
            .push(Space::new().height(4).boxed())
            .push(
                text(crate::i18n::t("terminal_bg_image_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted).boxed(),
            );
        if has_bg_image {
            appearance_col = appearance_col
                .push(Space::new().height(10).boxed())
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
                        oryxis_terminal::BgFit::from_str_or_default(&self.vaults.cur().prefs.terminal_bg_fit),
                    ))
                    .to_string(),
                    |s: &String| s.clone(),
                    160.0,
                    |v| Message::Settings(SettingsMessage::TerminalBgFitChanged(v)),
                ))
                .push(Space::new().height(10).boxed())
                .push(self.nav_pick_row(
                    crate::i18n::t("terminal_bg_dim"),
                    crate::terminal_appearance::DIM_STEPS
                        .iter()
                        .map(|p| format!("{p}%"))
                        .collect::<Vec<_>>(),
                    format!("{}%", self.vaults.cur().prefs.terminal_bg_dim),
                    |s: &String| s.clone(),
                    120.0,
                    |v| Message::Settings(SettingsMessage::TerminalBgDimChanged(v)),
                ))
                .push(Space::new().height(4).boxed())
                .push(
                    text(crate::i18n::t("terminal_bg_dim_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted).boxed(),
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
                crate::i18n::t(self.vaults.cur().prefs.bell_mode.label_key()).to_string(),
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
                crate::i18n::t(self.vaults.cur().prefs.notification_mode.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::NotificationModeChanged(v)),
            ),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("smart_tabs"), self.vaults.cur().prefs.smart_tabs, Message::Settings(SettingsMessage::SettingToggleSmartTabs)),
            self.smart_tabs_threshold_row(),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("confirm_close_session_tab"), self.vaults.cur().prefs.confirm_close_session_tab, Message::Settings(SettingsMessage::ToggleConfirmCloseSessionTab)),
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
                crate::i18n::t(self.vaults.cur().prefs.clipboard_access.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::ClipboardAccessChanged(v)),
            ),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("terminal_auto_title"), crate::state::auto_title_enabled(), Message::Settings(SettingsMessage::ToggleTerminalAutoTitle)),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("command_history_capture"), self.vaults.cur().prefs.command_history, Message::Settings(SettingsMessage::ToggleCommandHistory)),
            self.shell_integration_row(),
            self.history_suggest_row(),
            Space::new().height(10),
            self.nav_toggle_row(crate::i18n::t("cmd_history_file"), self.vaults.cur().prefs.command_history_file, Message::CommandHistory(CommandHistoryMessage::ToggleCommandHistoryFile)),
            self.command_history_dir_row(),
            self.snippet_output_dir_row(),
        ];

        // Sidebar: which region each tab docks to (issue #102) and how
        // the panel opens.
        let sidebar_col = column![
            self.nav_toggle_row(
                crate::i18n::t("sidebar_auto_open"),
                self.vaults.cur().prefs.sidebar_auto_open,
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
                text(crate::i18n::t("terminal_font_size")).size(13).color(OryxisColors::t().text_primary).boxed(),
                Space::new().width(Length::Fill).boxed(),
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
                }).boxed(),
                Space::new().width(8).boxed(),
                text(format!("{:.0}", self.vaults.cur().terminal_font_size)).size(13).color(OryxisColors::t().text_primary).boxed(),
                Space::new().width(8).boxed(),
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
                }).boxed(),
                ]).align_y(iced::Alignment::Center).boxed(),
            ),
        ];
        // The stepper edits the PREFERENCE; the zoom chords add a
        // session-only delta on top (`terminal_font_zoom`). When one is
        // in effect the terminal is not drawing the number above, so
        // say so here rather than let the row look wrong.
        let font_size_block = if self.terminal_font_zoom != 0.0 {
            font_size_block.push(Space::new().height(4).boxed()).push(
                text(
                    crate::i18n::t("terminal_font_zoomed")
                        .replace("{n}", &format!("{:.0}", self.terminal_font_px())),
                )
                .size(11)
                .color(OryxisColors::t().text_muted).boxed(),
            )
        } else {
            font_size_block
        };
        // Ctrl+wheel zoom (and the touchpad pinch Windows turns into
        // it, #225). State comes from the binding table: the gesture IS
        // the pair of Ctrl+wheel chords on the font zoom actions,
        // editable in Settings > Shortcuts, and this toggle adds /
        // removes them, the same shape as the middle-click paste toggle.
        let font_size_block = font_size_block
            .push(Space::new().height(10).boxed())
            .push(self.nav_toggle_row(
                crate::i18n::t("wheel_zoom"),
                self.wheel_zoom_bound(),
                Message::Settings(SettingsMessage::ToggleWheelZoom),
            ))
            .push(Space::new().height(4).boxed())
            .push(
                text(crate::i18n::t("wheel_zoom_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted).boxed(),
            );

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
        let preview_weight = self.vaults.cur().terminal_font_weight.font_weight();
        let preview_font = fonts
            .iter()
            .find(|f| f.as_str() == self.vaults.cur().terminal_font_name)
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
            .vaults.cur().terminal_theme_override
            .clone()
            .unwrap_or_else(|| crate::theme::AppTheme::active().name().to_string());
        let pal = self
            .terminal_palette_for_name(&active_term_theme)
            .unwrap_or_default();
        let (fg, bg) = (pal.foreground, pal.background);
        let (c_green, c_blue, c_cyan, c_yellow) =
            (pal.ansi[2], pal.ansi[4], pal.ansi[6], pal.ansi[3]);
        let fs = self.vaults.cur().terminal_font_size;
        let font_preview = container(
            column![
                text("The quick brown fox 1234567890 {}[]()<>")
                    .font(preview_font).size(fs).color(fg),
                Space::new().height(4),
                dir_row(vec![
                    text("user").font(preview_font).size(fs).color(c_green).boxed(),
                    text("@").font(preview_font).size(fs).color(fg).boxed(),
                    text("host").font(preview_font).size(fs).color(c_blue).boxed(),
                    text(":").font(preview_font).size(fs).color(fg).boxed(),
                    text("~/dev").font(preview_font).size(fs).color(c_cyan).boxed(),
                    text("$ ").font(preview_font).size(fs).color(fg).boxed(),
                    text("git status").font(preview_font).size(fs).color(c_yellow).boxed(),
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
            &self.vaults.cur().terminal_font_name,
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
                    Some(self.vaults.cur().terminal_font_name.clone()),
                    fonts,
                    |s: &String| s.clone(),
                )
                .on_select(|v| Message::Settings(SettingsMessage::TerminalFontChanged(v)))
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(260).padding(10).style(crate::widgets::rounded_pick_list_style)
                .boxed(),
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
            font_picker_block = font_picker_block.push(Space::new().height(6).boxed()).push(
                text(format!(
                    "{} {}",
                    t("font_pack_available"),
                    pack_missing.join(", ")
                ))
                .size(11)
                .color(OryxisColors::t().text_muted).boxed(),
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
            &self.vaults.cur().terminal_font_weight,
            |w| Message::Settings(SettingsMessage::TerminalFontWeightChanged(w)),
        );
        let font_picker_block = font_picker_block
            .push(Space::new().height(14).boxed())
            .push(
                text(t("terminal_font_weight"))
                    .size(13)
                    .color(OryxisColors::t().text_primary).boxed(),
            )
            .push(Space::new().height(8).boxed())
            .push(self.settings_nav_slot_labeled(
                t("terminal_font_weight"),
                crate::keynav::RowAction::picker(weight_prev, weight_next),
                8.0,
                pick_list(
                    Some(self.vaults.cur().terminal_font_weight),
                    weights.to_vec(),
                    |w: &crate::fonts::TerminalFontWeight| w.to_string(),
                )
                .on_select(|w| {
                    Message::Settings(SettingsMessage::TerminalFontWeightChanged(w))
                })
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(260).padding(10).style(crate::widgets::rounded_pick_list_style)
                .boxed(),
            ));
        // The weight's honesty line, right under the picker it is about.
        let font_picker_block = if crate::app::terminal_font_serves_weight(
            &self.vaults.cur().terminal_font_name,
            self.vaults.cur().terminal_font_weight.css(),
        ) {
            font_picker_block
        } else {
            font_picker_block.push(Space::new().height(6).boxed()).push(
                text(t("font_weight_unavailable"))
                    .size(11)
                    .color(OryxisColors::t().text_muted).boxed(),
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
            &self.vaults.cur().terminal_text_thickness,
            |t| Message::Settings(SettingsMessage::TerminalTextThicknessChanged(t)),
        );
        let font_picker_block = font_picker_block
            .push(Space::new().height(14).boxed())
            .push(
                text(t("terminal_text_thickness"))
                    .size(13)
                    .color(OryxisColors::t().text_primary).boxed(),
            )
            .push(Space::new().height(4).boxed())
            .push(
                text(t("setting_text_thickness_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted).boxed(),
            )
            .push(Space::new().height(8).boxed())
            .push(self.settings_nav_slot_labeled(
                t("terminal_text_thickness"),
                crate::keynav::RowAction::picker(thick_prev, thick_next),
                8.0,
                pick_list(
                    Some(self.vaults.cur().terminal_text_thickness),
                    thicknesses.to_vec(),
                    |v: &crate::fonts::TextThickness| v.to_string(),
                )
                .on_select(|v| {
                    Message::Settings(SettingsMessage::TerminalTextThicknessChanged(v))
                })
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(260).padding(10).style(crate::widgets::rounded_pick_list_style)
                .boxed(),
            ));
        let font_picker_block = font_picker_block
            .push(Space::new().height(12).boxed())
            .push(font_preview.boxed());
        // One Appearance card: rendering toggles, then the font
        // size stepper and the font picker + live sample. The
        // terminal-theme gallery keeps its own card below (its own
        // sub-theme, and a grid that large reads better boxed
        // separately).
        let appearance_section = panel_section(
            appearance_col
                .push(Space::new().height(16).boxed())
                .push(font_size_block.boxed())
                .push(Space::new().height(16).boxed())
                .push(font_picker_block.boxed()),
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
            .vaults.cur().terminal_theme_override
            .clone()
            .unwrap_or_else(|| follow_label.clone());
        let current_palette = self
            .vaults.cur().terminal_theme_override
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
                self.vaults.cur().prefs.pane_gap.clone(),
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
                self.vaults.cur().prefs.pane_border_inactive,
                Message::Settings(SettingsMessage::TogglePaneBorderInactive),
            ),
            Space::new().height(10),
            self.nav_toggle_row(
                crate::i18n::t("pane_headers"),
                self.vaults.cur().prefs.pane_headers,
                Message::Settings(SettingsMessage::TogglePaneHeaders),
            ),
            Space::new().height(4),
            text(crate::i18n::t("pane_headers_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(16),
            // What a pane does when its session ends (issue #208): a
            // split-pane question, so it sits with the split-pane knobs.
            // A lone REMOTE pane is unaffected: it still relabels itself
            // and rides the auto-reconnect sweep, neither of which a
            // split tab can use without taking its live siblings down.
            self.nav_pick_row(
                crate::i18n::t("pane_end_action"),
                crate::util::PaneEndAction::ALL
                    .iter()
                    .map(|a| crate::i18n::t(a.label_key()).to_string())
                    .collect::<Vec<_>>(),
                crate::i18n::t(self.vaults.cur().prefs.pane_end_action.label_key()).to_string(),
                |s: &String| s.clone(),
                200.0,
                |v| Message::Settings(SettingsMessage::PaneEndActionChanged(v)),
            ),
            Space::new().height(4),
            text(crate::i18n::t("pane_end_action_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
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
        .on_scroll(|s| Message::Settings(SettingsMessage::SectionScrolled(s.viewport.relative_offset().y)))
        .height(Length::Fill)
        .boxed()
    }
}
