//! Settings -> About section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_about(&self) -> Element<'_, Message> {
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
}
