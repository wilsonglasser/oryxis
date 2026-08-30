//! Settings -> About section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_about(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order.
        self.keynav_settings_reset();
        // Channel-aware build string: nightly builds append the
        // channel + short commit so a nightly user sees exactly what
        // they're running, not just the base version number.
        let version_str = if env!("ORYXIS_CHANNEL") == "nightly" {
            format!(
                "Oryxis v{} nightly ({})",
                env!("CARGO_PKG_VERSION"),
                env!("ORYXIS_GIT_SHA").chars().take(7).collect::<String>(),
            )
        } else {
            format!("Oryxis v{}", env!("CARGO_PKG_VERSION"))
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
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::OpenUrl(
                    "https://oryxis.app/".to_string(),
                )),
                8.0,
                crate::widgets::settings_row_link(
                    crate::i18n::t("website"),
                    "oryxis.app".into(),
                    "https://oryxis.app/".into(),
                ),
            ),
            Space::new().height(6),
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::OpenUrl(
                    "https://github.com/wilsonglasser/oryxis".to_string(),
                )),
                8.0,
                crate::widgets::settings_row_link(
                    crate::i18n::t("github"),
                    "github.com/wilsonglasser/oryxis".into(),
                    "https://github.com/wilsonglasser/oryxis".into(),
                ),
            ),
        ]);

        // Each stat row navigates to its section (issue #38):
        // the count doubles as a shortcut into the data it
        // describes. Two rows are composites, each summing exactly
        // what its destination view lists: Logs combines connection
        // events + session recordings, and Keychain combines SSH keys
        // + identities (issue #148: counting keys alone reported 0 on
        // a vault whose keychain held only identities). Proxy
        // identities stay out; they are the Proxies view.
        let vault_section = panel_section(column![
            text(crate::i18n::t("vault_stats")).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(8),
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Navigation(NavigationMessage::ChangeView(
                    crate::state::View::Dashboard,
                ))),
                8.0,
                crate::widgets::settings_row_nav(
                    crate::i18n::t("hosts"),
                    self.connections.len().to_string(),
                    Message::Navigation(NavigationMessage::ChangeView(crate::state::View::Dashboard)),
                ),
            ),
            Space::new().height(6),
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Navigation(NavigationMessage::ChangeView(
                    crate::state::View::Keys,
                ))),
                8.0,
                crate::widgets::settings_row_nav(
                    crate::i18n::t("keychain"),
                    (self.keys.len() + self.identities.len()).to_string(),
                    Message::Navigation(NavigationMessage::ChangeView(crate::state::View::Keys)),
                ),
            ),
            Space::new().height(6),
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Navigation(NavigationMessage::ChangeView(
                    crate::state::View::Snippets,
                ))),
                8.0,
                crate::widgets::settings_row_nav(
                    crate::i18n::t("snippets"),
                    self.snippets.len().to_string(),
                    Message::Navigation(NavigationMessage::ChangeView(crate::state::View::Snippets)),
                ),
            ),
            Space::new().height(6),
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Navigation(NavigationMessage::ChangeView(
                    crate::state::View::Dashboard,
                ))),
                8.0,
                crate::widgets::settings_row_nav(
                    t("groups"),
                    self.groups.len().to_string(),
                    Message::Navigation(NavigationMessage::ChangeView(crate::state::View::Dashboard)),
                ),
            ),
            Space::new().height(6),
            self.settings_nav_slot(
                crate::keynav::RowAction::activate(Message::Navigation(NavigationMessage::ChangeView(
                    crate::state::View::History,
                ))),
                8.0,
                crate::widgets::settings_row_nav(
                    t("logs"),
                    (self.logs_total + self.session_logs_total).to_string(),
                    Message::Navigation(NavigationMessage::ChangeView(crate::state::View::History)),
                ),
            ),
        ]);

        scrollable(
            container(
                column![
                    about_section,
                    Space::new().height(12),
                    vault_section,
                    Space::new().height(24),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-about-scroll"))
        .on_scroll(|v| Message::Settings(SettingsMessage::SectionScrolled(v.relative_offset().y)))
        .height(Length::Fill)
        .into()
    }

}
