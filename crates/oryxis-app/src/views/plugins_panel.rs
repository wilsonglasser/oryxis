//! Plugins panel, manage the locally installed plugins.
//!
//! Plugins (MCP server, GIF export) run as local subprocesses. This
//! screen is the management surface: per-provider status and removal.
//! The app performs no network fetches — a plugin is whatever the
//! local cache or a dev build provides.

use iced::border::Radius;
use iced::widget::{column, container, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{SettingsMessage, PluginMessage, AgentMessage, AiMessage, Message, Oryxis};
use crate::state::{PluginUiEntry, PluginUiStatus};
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, panel_section, toggle_row_desc};

impl Oryxis {
    pub(crate) fn view_plugins_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order.
        self.keynav_settings_reset();
        // Built-in "feature plugins": SFTP / AI / MCP are enabled
        // here (their Settings sections only appear once enabled), the
        // same surface as the downloadable provider plugins below.
        let mut rows: Vec<Element<'_, Message>> = vec![
            text(crate::i18n::t("features"))
                .size(13)
                .color(OryxisColors::t().text_primary)
                .into(),
            Space::new().height(8).into(),
            panel_section(column![
                self.settings_nav_slot_labeled(
                    crate::i18n::t("ai_assistant"),
                    crate::keynav::RowAction::activate(Message::Ai(AiMessage::ToggleAiEnabled)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("ai_assistant"),
                        crate::i18n::t("feature_ai_desc"),
                        self.ai.enabled,
                        Message::Ai(AiMessage::ToggleAiEnabled),
                    ),
                ),
                Space::new().height(12),
                // MCP is not listed here: it's a real plugin binary (the
                // "Oryxis MCP Server" card below), so it's activated and
                // managed there, and its server on/off lives in the MCP
                // settings section that appears once the plugin is present.
                self.settings_nav_slot_labeled(
                    crate::i18n::t("sftp"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleSftpEnabled)),
                    8.0,
                    toggle_row_desc(
                        "SFTP",
                        crate::i18n::t("feature_sftp_desc"),
                        self.sftp_enabled,
                        Message::Settings(SettingsMessage::SettingToggleSftpEnabled),
                    ),
                ),
                Space::new().height(12),
                self.settings_nav_slot_labeled(
                    crate::i18n::t("remote_desktop"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleRemoteDesktop)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("remote_desktop"),
                        crate::i18n::t("feature_remote_desktop_desc"),
                        self.remote_desktop_enabled,
                        Message::Settings(SettingsMessage::SettingToggleRemoteDesktop),
                    ),
                ),
                Space::new().height(12),
                // Host monitoring (issue #83): niche + recurring, so the
                // whole subsystem hides until enabled here (the sidebar
                // Monitor tab, status-bar segment, per-host opt-in and
                // interval all appear only once this is on).
                self.settings_nav_slot_labeled(
                    crate::i18n::t("feature_monitoring"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleHostMonitoring)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("feature_monitoring"),
                        crate::i18n::t("feature_monitoring_desc"),
                        self.prefs.host_monitoring,
                        Message::Settings(SettingsMessage::SettingToggleHostMonitoring),
                    ),
                ),
                Space::new().height(12),
                // tmux manager (issue #116): same rule as monitoring.
                // Managing tmux from a panel is niche, so the sidebar
                // tab only exists once this is on.
                self.settings_nav_slot_labeled(
                    crate::i18n::t("feature_tmux"),
                    crate::keynav::RowAction::activate(Message::Settings(SettingsMessage::SettingToggleTmuxManager)),
                    8.0,
                    toggle_row_desc(
                        crate::i18n::t("feature_tmux"),
                        crate::i18n::t("feature_tmux_desc"),
                        self.prefs.tmux_manager,
                        Message::Settings(SettingsMessage::SettingToggleTmuxManager),
                    ),
                ),
                Space::new().height(12),
                // Features holds only the enable toggle; the confirm +
                // socket rows live in the Settings sidebar's SSH Agent
                // section, which appears while the agent is enabled.
                self.agent_server_toggle(),
            ]),
            Space::new().height(18).into(),
            // Plugins list header.
            text(crate::i18n::t("plugins_subtitle"))
                .size(12)
                .color(OryxisColors::t().text_muted)
                .into(),
            Space::new().height(14).into(),
        ];

        if self.plugins.is_empty() {
            rows.push(
                container(
                    text(crate::i18n::t("plugins_empty"))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                )
                .padding(16)
                .into(),
            );
        }

        for entry in &self.plugins {
            rows.push(plugin_card(self, entry));
            rows.push(Space::new().height(8).into());
        }

        scrollable(
            column(rows).padding(Padding {
                top: 24.0,
                right: 24.0,
                bottom: 24.0,
                left: 24.0,
            }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-plugins-scroll"))
        .on_scroll(|v| Message::Settings(SettingsMessage::SectionScrolled(v.relative_offset().y)))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }

    /// The ssh-agent ENABLE toggle, shown in the Features section. Any
    /// runtime error is surfaced inline under it. Off unix (pre-Phase-3
    /// Windows) with no listener, the whole row is hidden. The confirm +
    /// socket rows live in the sidebar's SSH Agent section
    /// (`view_settings_agent`), visible only while the agent is on.
    fn agent_server_toggle(&self) -> Element<'_, Message> {
        // No socket path means no listener on this platform: hide it.
        if crate::agent_server::listener_socket_display().is_none() {
            return Space::new().into();
        }

        let toggle = self.settings_nav_slot_labeled(
            crate::i18n::t("agent_server"),
            crate::keynav::RowAction::activate(Message::Agent(AgentMessage::AgentServerToggled(!self.agent.enabled))),
            8.0,
            toggle_row_desc(
                crate::i18n::t("agent_server"),
                crate::i18n::t("agent_server_desc"),
                self.agent.enabled,
                Message::Agent(AgentMessage::AgentServerToggled(!self.agent.enabled)),
            ),
        );

        if let Some(err) = &self.agent.error {
            return column![
                toggle,
                Space::new().height(6),
                text(err.clone()).size(11).color(OryxisColors::t().error),
            ]
            .into();
        }
        toggle
    }

}

/// One provider row, single-line: brand icon + name + version +
/// status badge on the leading edge, the status's primary action and
/// the kebab trigger on the trailing edge. Secondary actions (check
/// for updates, the auto-update override, uninstall) live in the
/// kebab menu (`build_menu_plugin_actions`), also reachable by
/// right-clicking the row. Only an error / dev-build hint adds a
/// second line. Takes the app so every control registers as a
/// keyboard row at construction, in visual order.
fn plugin_card<'a>(app: &Oryxis, entry: &'a PluginUiEntry) -> Element<'a, Message> {
    let id = entry.provider_id.clone();

    let (badge_label, badge_color) = match &entry.status {
        PluginUiStatus::DevBuild => (
            crate::i18n::t("plugin_status_dev_build"),
            OryxisColors::t().accent,
        ),
        PluginUiStatus::Installed(_) => (
            crate::i18n::t("plugin_status_installed"),
            OryxisColors::t().success,
        ),
        PluginUiStatus::NotInstalled => (
            crate::i18n::t("plugin_status_not_installed"),
            OryxisColors::t().text_muted,
        ),
    };

    // Inline version tail next to the name: current version only.
    let version = match &entry.status {
        PluginUiStatus::Installed(v) => Some(format!("v{v}")),
        _ => None,
    };

    // Second line only where one is genuinely needed: the dev-build
    // explainer.
    let detail: Option<(String, Color)> = match &entry.status {
        PluginUiStatus::DevBuild => Some((
            crate::i18n::t("plugin_dev_build_hint").to_string(),
            OryxisColors::t().text_muted,
        )),
        _ => None,
    };

    let badge = container(
        text(badge_label)
            .size(10)
            .color(badge_color),
    )
    .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.14, ..badge_color })),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    });

    // Provider brand logo (AWS smile, Kubernetes wheel, ...) instead of
    // a generic package box. MCP has no brand SVG, so it gets a server
    // glyph; unknown providers fall back to the cloud glyph.
    let (brand_icon, brand_icon_color) = if entry.provider_id == "mcp" {
        (
            crate::os_icon::BrandIcon::Glyph(iced_fonts::lucide::server()),
            OryxisColors::t().accent,
        )
    } else {
        crate::os_icon::provider_icon(&entry.provider_id, OryxisColors::t().accent)
    };

    let mut row_items: Vec<Element<'_, Message>> = vec![
        brand_icon.view(16.0, brand_icon_color),
        Space::new().width(10).into(),
        text(&entry.display_name)
            .size(14)
            .color(OryxisColors::t().text_primary)
            .into(),
    ];
    if let Some(v) = version {
        row_items.push(Space::new().width(8).into());
        row_items.push(
            text(v).size(11).color(OryxisColors::t().text_secondary).into(),
        );
    }
    row_items.push(Space::new().width(10).into());
    row_items.push(badge.into());
    row_items.push(Space::new().width(Length::Fill).into());

    // Kebab trigger, on every row that has secondary actions. Always
    // visible (Settings controls aren't hover-revealed cards) and a
    // recorded keyboard row like the pills.
    let has_menu = matches!(entry.status, PluginUiStatus::Installed(_))
        || (matches!(entry.status, PluginUiStatus::DevBuild) && entry.cached_install);
    if has_menu {
        row_items.push(Space::new().width(8).into());
        row_items.push(app.settings_nav_slot(
            crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::ShowPluginMenu(id.clone()))),
            6.0,
            crate::widgets::card_kebab_button(
                OryxisColors::t().text_muted,
                true,
                Message::Plugin(PluginMessage::ShowPluginMenu(id.clone())),
            )
            .into(),
        ));
    }

    // The detail line must hug the leading edge under RTL, hence the
    // dir-aware alignment (the width:Fill gives it slack to align in).
    let mut card = column![dir_row(row_items).align_y(iced::Alignment::Center)]
        .spacing(6)
        .width(Length::Fill)
        .align_x(dir_align_x());
    if let Some((line, color)) = detail {
        card = card.push(text(line).size(11).color(color));
    }

    let styled = container(card)
        .padding(Padding { top: 10.0, right: 16.0, bottom: 10.0, left: 16.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            // Match the `panel_section` bg used elsewhere in Settings
            // so a plugin card looks like every other settings panel
            // instead of the lighter `bg_surface` it used before.
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

    if has_menu {
        // Right-click anywhere on the row opens the same kebab menu
        // (app-wide card convention).
        iced::widget::MouseArea::new(styled)
            .on_right_press(Message::Plugin(PluginMessage::ShowPluginMenu(id)))
            .into()
    } else {
        styled.into()
    }
}
