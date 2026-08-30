//! Settings -> Advanced section view: the debug-logging file toggle and
//! the environment report for GitHub issues.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_advanced(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order.
        self.keynav_settings_reset();
        // ── Debug logging ──
        let log_path = crate::logging::log_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        // Under `--debug-log` the sink is pinned on for the whole run, so
        // the row reports the flag instead of the usual description and
        // the toggle answers with the same sentence (see the handler).
        let forced = crate::logging::is_forced();
        let debug_desc = if forced { "debug_logging_forced" } else { "debug_logging_desc" };
        let debug_col = column![
            self.nav_toggle_row(
                t("debug_logging"),
                self.prefs.debug_logging || forced,
                Message::Settings(SettingsMessage::SettingToggleDebugLogging),
            ),
            Space::new().height(4),
            text(t(debug_desc)).size(11).color(OryxisColors::t().text_muted),
            Space::new().height(12),
            settings_row(t("debug_log_file"), log_path),
            Space::new().height(8),
            dir_row(vec![
                self.settings_nav_slot(
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::RevealDebugLog)),
                    6.0,
                    styled_button(
                        crate::i18n::open_in_file_manager_label(),
                        Message::Settings(SettingsMessage::RevealDebugLog),
                        OryxisColors::t().bg_selected,
                    ),
                ),
                Space::new().width(10).into(),
                self.settings_nav_slot(
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::ClearDebugLog)),
                    6.0,
                    styled_button(
                        t("debug_log_clear"),
                        Message::Settings(SettingsMessage::ClearDebugLog),
                        OryxisColors::t().bg_selected,
                    ),
                ),
            ]),
            // ── Performance HUD ──
            Space::new().height(16),
            self.nav_toggle_row(
                t("perf_overlay"),
                self.prefs.perf_overlay,
                Message::Settings(SettingsMessage::SettingTogglePerfOverlay),
            ),
            Space::new().height(4),
            text(t("perf_overlay_desc")).size(11).color(OryxisColors::t().text_muted),
        ];

        // ── Environment information ──
        // The report is rendered verbatim so the user sees exactly what
        // the Copy button puts on the clipboard, nothing hidden.
        let env_report = crate::logging::environment_report(self.renderer_active.as_ref());
        let report_block = container(
            text(env_report.clone())
                .size(11)
                .font(iced::Font::MONOSPACE)
                .color(OryxisColors::t().text_secondary),
        )
        .padding(12)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        });
        // Key-derivation parameters (E1). Read-only: shows the vault's
        // tuned Argon2id profile, or that it uses the crate defaults.
        let kdf_line = match self.vault.as_ref().and_then(|v| v.kdf_params()) {
            Some(p) => t("kdf_params_label")
                .replacen("{mib}", &(p.m_kib / 1024).to_string(), 1)
                .replacen("{t}", &p.t.to_string(), 1),
            None => t("kdf_params_default").to_string(),
        };
        // Debug logging, the performance HUD and the environment
        // report are one diagnostics theme, so they share a card.
        let diagnostics_section = panel_section(debug_col.push(Space::new().height(16)).push(column![
            text(t("env_info")).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("env_info_desc")).size(11).color(OryxisColors::t().text_muted),
            Space::new().height(10),
            text(kdf_line).size(11).color(OryxisColors::t().text_secondary),
            Space::new().height(10),
            report_block,
            Space::new().height(10),
            self.settings_nav_slot_labeled(
                t("copy_env_info"),
                crate::keynav::RowAction::activate(Message::CopyToClipboard(
                    env_report.clone(),
                )),
                6.0,
                styled_button(
                    t("copy_env_info"),
                    Message::CopyToClipboard(env_report),
                    OryxisColors::t().accent,
                ),
            ),
        ]));

        scrollable(
            container(
                column![
                    diagnostics_section,
                    Space::new().height(24),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-advanced-scroll"))
        .on_scroll(|v| Message::Settings(SettingsMessage::SectionScrolled(v.relative_offset().y)))
        .height(Length::Fill)
        .into()
    }
}
