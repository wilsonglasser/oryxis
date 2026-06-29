//! Settings -> Connection section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_connection(&self) -> Element<'_, Message> {
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

        // Defaults pre-filled into a NEW host form (so the user doesn't
        // re-toggle agent forwarding / re-type a port every time).
        let term_default_options: Vec<String> = [
            "xterm-256color", "xterm", "screen-256color", "tmux-256color",
            "screen", "linux", "vt220", "vt100", "ansi",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let new_conn_defaults_section = panel_section(column![
            text(crate::i18n::t("new_connection_defaults")).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("new_connection_defaults_desc")).size(11).color(OryxisColors::t().text_muted),
            Space::new().height(10),
            toggle_row(crate::i18n::t("forward_ssh_agent"), self.setting_default_agent_forwarding, Message::ToggleDefaultAgentForwarding),
            Space::new().height(10),
            dir_row(vec![
                text(crate::i18n::t("port")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                text_input("22", &self.setting_default_port)
                    .on_input(Message::DefaultPortChanged)
                    .padding(10).width(120)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
            ]).align_y(iced::Alignment::Center),
            Space::new().height(10),
            dir_row(vec![
                text(crate::i18n::t("host_keepalive")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                text_input(&self.setting_keepalive_interval, &self.setting_default_keepalive)
                    .on_input(Message::DefaultKeepaliveChanged)
                    .padding(10).width(120)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
            ]).align_y(iced::Alignment::Center),
            Space::new().height(10),
            dir_row(vec![
                text(crate::i18n::t("host_terminal_type")).size(13).color(OryxisColors::t().text_secondary).into(),
                Space::new().width(Length::Fill).into(),
                pick_list(
                    Some(self.setting_default_terminal_type.clone()),
                    term_default_options,
                    |s: &String| s.clone(),
                )
                .on_select(Message::DefaultTerminalTypeChanged)
                .width(200).padding(10)
                .style(crate::widgets::rounded_pick_list_style).into(),
            ]).align_y(iced::Alignment::Center),
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
                    new_conn_defaults_section,
                    Space::new().height(12),
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
}
