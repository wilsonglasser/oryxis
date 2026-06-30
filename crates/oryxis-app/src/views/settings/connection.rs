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
        // The localized "(none)" sentinel shared by the entity-reference
        // pickers (identity / key / group / proxy): selecting it clears the
        // default; any other option is a saved entity's label.
        let none_label = crate::i18n::t("new_default_none").to_string();

        // Auth method picker, options + current selection mirror the host
        // editor's auth picker so the labels match.
        let auth_options: Vec<String> = [
            crate::i18n::t("auth_auto"), crate::i18n::t("auth_password"),
            crate::i18n::t("auth_key"), crate::i18n::t("auth_agent"),
            crate::i18n::t("auth_interactive"),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let auth_selected = crate::util::auth_method_label(&self.setting_default_auth_method);

        let mut identity_options = vec![none_label.clone()];
        identity_options.extend(self.identities.iter().map(|i| i.label.clone()));
        let identity_selected = self
            .setting_default_identity_id
            .and_then(|id| self.identities.iter().find(|i| i.id == id).map(|i| i.label.clone()))
            .unwrap_or_else(|| none_label.clone());

        let mut key_options = vec![none_label.clone()];
        key_options.extend(self.keys.iter().map(|k| k.label.clone()));
        let key_selected = self
            .setting_default_key_id
            .and_then(|id| self.keys.iter().find(|k| k.id == id).map(|k| k.label.clone()))
            .unwrap_or_else(|| none_label.clone());

        // Parent group: only the visible (non-phantom) groups, matching the
        // host editor's group combo.
        let visible_groups = self.visible_group_ids();
        let mut group_options = vec![none_label.clone()];
        group_options.extend(
            self.groups
                .iter()
                .filter(|g| visible_groups.contains(&g.id))
                .map(|g| g.label.clone()),
        );
        let group_selected = self
            .setting_default_group_id
            .and_then(|id| self.groups.iter().find(|g| g.id == id).map(|g| g.label.clone()))
            .unwrap_or_else(|| none_label.clone());

        let mut proxy_options = vec![none_label.clone()];
        proxy_options.extend(self.proxy_identities.iter().map(|p| p.label.clone()));
        let proxy_selected = self
            .setting_default_proxy_identity_id
            .and_then(|id| {
                self.proxy_identities.iter().find(|p| p.id == id).map(|p| p.label.clone())
            })
            .unwrap_or_else(|| none_label.clone());

        let encoding_options: Vec<String> = [
            "UTF-8", "Big5", "GBK", "gb18030", "Shift_JIS", "EUC-JP", "EUC-KR",
            "ISO-8859-1", "ISO-8859-15", "windows-1251", "windows-1252", "KOI8-R",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let encoding_selected =
            self.setting_default_encoding.clone().unwrap_or_else(|| "UTF-8".to_string());

        // A labeled pick_list row (label on the leading edge, picker
        // trailing). `label_key` is an i18n key resolved here.
        let pick_row = |label_key: &'static str,
                        options: Vec<String>,
                        selected: String,
                        on_select: fn(String) -> Message|
         -> Element<'_, Message> {
            dir_row(vec![
                text(crate::i18n::t(label_key))
                    .size(13)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                pick_list(Some(selected), options, |s: &String| s.clone())
                    .on_select(on_select)
                    .width(220)
                    .padding(10)
                    .style(crate::widgets::rounded_pick_list_style)
                    .into(),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        };

        // Environment-variables list editor, same add/remove/edit-row shape
        // as the host editor's env-vars block.
        let mut env_block = column![dir_row(vec![
            column![
                text(t("env_vars")).size(13).color(OryxisColors::t().text_secondary),
                Space::new().height(2),
                text(t("env_vars_desc")).size(11).color(OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .into(),
            Space::new().width(8).into(),
            button(text("+").size(14).color(OryxisColors::t().text_primary))
                .on_press(Message::DefaultAddEnvVar)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(OryxisColors::t().bg_hover)),
                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                    text_color: OryxisColors::t().text_primary,
                    ..Default::default()
                })
                .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
                .into(),
        ])
        .align_y(iced::Alignment::Center)];
        for (i, e) in self.setting_default_env_vars.iter().enumerate() {
            let idx = i;
            env_block = env_block.push(Space::new().height(8));
            env_block = env_block.push(
                dir_row(vec![
                    text_input("LC_EXAMPLE", &e.key)
                        .on_input(move |v| Message::DefaultEnvVarKeyChanged(idx, v))
                        .padding(6)
                        .width(Length::FillPortion(2))
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .into(),
                    text("=").size(12).color(OryxisColors::t().text_muted).into(),
                    text_input("value", &e.value)
                        .on_input(move |v| Message::DefaultEnvVarValueChanged(idx, v))
                        .padding(6)
                        .width(Length::FillPortion(3))
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .into(),
                    button(text("\u{00D7}").size(11).color(OryxisColors::t().error))
                        .on_press(Message::DefaultRemoveEnvVar(idx))
                        .style(|_, _| button::Style {
                            background: None,
                            border: Border::default(),
                            text_color: OryxisColors::t().error,
                            ..Default::default()
                        })
                        .padding(Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 4.0 })
                        .into(),
                ])
                .align_y(iced::Alignment::Center)
                .spacing(4),
            );
        }

        // The card is long once every default field is shown, so the header
        // doubles as a collapse toggle (chevron points down when open, into
        // the leading edge when collapsed). Hidden fields keep their state.
        let collapsed = self.setting_defaults_collapsed;
        let chevron = if collapsed {
            if crate::i18n::is_rtl_layout() {
                iced_fonts::lucide::chevron_left()
            } else {
                iced_fonts::lucide::chevron_right()
            }
        } else {
            iced_fonts::lucide::chevron_down()
        };
        let defaults_header = button(
            dir_row(vec![
                column![
                    text(crate::i18n::t("new_connection_defaults"))
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(t("new_connection_defaults_desc"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                ]
                .width(Length::Fill)
                .into(),
                Space::new().width(8).into(),
                chevron.size(16).color(OryxisColors::t().text_muted).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleDefaultsCollapsed)
        .padding(4)
        .width(Length::Fill)
        .style(|_, status| button::Style {
            background: match status {
                BtnStatus::Hovered | BtnStatus::Pressed => Some(Background::Color(
                    iced::Color { a: 0.12, ..OryxisColors::t().accent },
                )),
                _ => None,
            },
            text_color: OryxisColors::t().text_primary,
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        });

        let mut new_conn_defaults_col = column![defaults_header];
        if !collapsed {
            new_conn_defaults_col = new_conn_defaults_col
                .push(Space::new().height(10))
                .push(toggle_row(crate::i18n::t("forward_ssh_agent"), self.setting_default_agent_forwarding, Message::ToggleDefaultAgentForwarding))
                .push(Space::new().height(10))
                .push(dir_row(vec![
                    text(crate::i18n::t("port")).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    text_input("22", &self.setting_default_port)
                        .on_input(Message::DefaultPortChanged)
                        .padding(10).width(120)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
                ]).align_y(iced::Alignment::Center))
                .push(Space::new().height(10))
                .push(dir_row(vec![
                    text(crate::i18n::t("host_keepalive")).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    text_input(&self.setting_keepalive_interval, &self.setting_default_keepalive)
                        .on_input(Message::DefaultKeepaliveChanged)
                        .padding(10).width(120)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
                ]).align_y(iced::Alignment::Center))
                .push(Space::new().height(10))
                .push(dir_row(vec![
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
                ]).align_y(iced::Alignment::Center))
                .push(Space::new().height(10))
                .push(dir_row(vec![
                    text(t("username")).size(13).color(OryxisColors::t().text_secondary).into(),
                    Space::new().width(Length::Fill).into(),
                    text_input("", &self.setting_default_username)
                        .on_input(Message::DefaultUsernameChanged)
                        .padding(10).width(220)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()).into(),
                ]).align_y(iced::Alignment::Center))
                .push(Space::new().height(10))
                .push(pick_row("auth_method", auth_options, auth_selected, Message::DefaultAuthMethodChanged))
                .push(Space::new().height(10))
                .push(pick_row("identity", identity_options, identity_selected, Message::DefaultIdentityChanged))
                .push(Space::new().height(10))
                .push(pick_row("ssh_key", key_options, key_selected, Message::DefaultKeyChanged))
                .push(Space::new().height(10))
                .push(pick_row("parent_group", group_options, group_selected, Message::DefaultGroupChanged))
                .push(Space::new().height(10))
                .push(pick_row("default_proxy", proxy_options, proxy_selected, Message::DefaultProxyChanged))
                .push(Space::new().height(10))
                .push(toggle_row(t("expose_to_mcp"), self.setting_default_mcp_enabled, Message::ToggleDefaultMcpEnabled))
                .push(Space::new().height(10))
                .push(pick_row("host_encoding", encoding_options, encoding_selected, Message::DefaultEncodingChanged))
                .push(Space::new().height(14))
                .push(env_block);
        }
        let new_conn_defaults_section = panel_section(new_conn_defaults_col);

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
