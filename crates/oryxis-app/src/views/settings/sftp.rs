//! Settings -> SFTP section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_sftp(&self) -> Element<'_, Message> {
        let concurrency_section = panel_section(column![
            text(t("transfer_parallelism"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("setting_sftp_concurrency_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            text_input("2", &self.setting_sftp_concurrency)
                .on_input(Message::SettingSftpConcurrencyChanged)
                .padding(10)
                .width(240)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
        ]);

        let timeout_input = |label: &str, hint: &str, value: &str, on_input: fn(String) -> Message| {
            panel_section(column![
                text(label.to_string())
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(hint.to_string())
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(8),
                text_input("0", value)
                    .on_input(on_input)
                    .padding(10)
                    .width(240)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            ])
        };

        let connect_section = timeout_input(
            t("connect_timeout"),
            t("connect_timeout_desc"),
            &self.setting_sftp_connect_timeout,
            Message::SettingSftpConnectTimeoutChanged,
        );
        let auth_section = timeout_input(
            t("auth_timeout"),
            t("auth_timeout_desc"),
            &self.setting_sftp_auth_timeout,
            Message::SettingSftpAuthTimeoutChanged,
        );
        let session_section = timeout_input(
            t("channel_open_timeout"),
            t("channel_open_timeout_desc"),
            &self.setting_sftp_session_timeout,
            Message::SettingSftpSessionTimeoutChanged,
        );
        let op_section = timeout_input(
            t("operation_timeout"),
            t("operation_timeout_desc"),
            &self.setting_sftp_op_timeout,
            Message::SettingSftpOpTimeoutChanged,
        );

        // Enable/disable lives on the Plugins screen now; this
        // section only renders while SFTP is enabled, showing its
        // tuning knobs (parallelism, timeouts).
        let mut content_col: iced::widget::Column<'_, Message> = column![]
            .width(Length::Fill)
            .align_x(dir_align_x());

        if self.sftp_enabled {
            content_col = content_col
                .push(concurrency_section)
                .push(Space::new().height(12))
                .push(connect_section)
                .push(Space::new().height(12))
                .push(auth_section)
                .push(Space::new().height(12))
                .push(session_section)
                .push(Space::new().height(12))
                .push(op_section);
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
