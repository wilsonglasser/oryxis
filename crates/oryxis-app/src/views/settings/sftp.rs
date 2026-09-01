//! Settings -> SFTP section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_sftp<'a>(&'a self) -> Element<'a, Message> {
        // Keyboard rows are recorded in visual order; each input row
        // focuses its field on Enter (ids are static, the fork's
        // widget::Id only takes &'static str). Recording happens at
        // construction, so everything below is built only when it
        // actually renders (`sftp_enabled`).
        self.keynav_settings_reset();
        let build_concurrency_block = || column![
            text(t("transfer_parallelism"))
                .size(13)
                .color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("setting_sftp_concurrency_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            self.settings_nav_slot_labeled(
                t("transfer_parallelism"),
                crate::keynav::RowAction::input(iced::widget::Id::new("set-sftp-concurrency")),
                10.0,
                text_input("2", &self.prefs.sftp_concurrency)
                    .id(iced::widget::Id::new("set-sftp-concurrency"))
                    .on_input(|v| Message::Settings(SettingsMessage::SettingSftpConcurrencyChanged(v)))
                    .padding(10)
                    .width(240)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .into(),
            ),
        ];

        // `value` carries the element lifetime: post-refactor
        // `text_input` borrows its fragments instead of copying.
        let timeout_input = |label: &str,
                             hint: &str,
                             value: &'a str,
                             id: &'static str,
                             on_input: fn(String) -> Message| {
            column![
                text(label.to_string())
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(hint.to_string())
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(8),
                self.settings_nav_slot_labeled(
                    label,
                    crate::keynav::RowAction::input(iced::widget::Id::new(id)),
                    10.0,
                    text_input("0", value)
                        .id(iced::widget::Id::new(id))
                        .on_input(on_input)
                        .padding(10)
                        .width(240)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .into(),
                ),
            ]
        };

        // Enable/disable lives on the Plugins screen now; this
        // section only renders while SFTP is enabled, showing its
        // tuning knobs (parallelism, timeouts).
        let mut content_col: iced::widget::Column<'_, Message> = column![]
            .width(Length::Fill)
            .align_x(dir_align_x());

        if self.sftp_enabled {
            // Parallelism + the four timeout knobs are one transfer-
            // tuning theme, so they share a single card (each block
            // keeps its 13 px sub-title, 16 px between blocks).
            let tuning_section = panel_section(
                build_concurrency_block()
                    .push(Space::new().height(16))
                    .push(timeout_input(
                        t("connect_timeout"),
                        t("connect_timeout_desc"),
                        &self.prefs.sftp_connect_timeout,
                        "set-sftp-connect-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpConnectTimeoutChanged(v)),
                    ))
                    .push(Space::new().height(16))
                    .push(timeout_input(
                        t("auth_timeout"),
                        t("auth_timeout_desc"),
                        &self.prefs.sftp_auth_timeout,
                        "set-sftp-auth-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpAuthTimeoutChanged(v)),
                    ))
                    .push(Space::new().height(16))
                    .push(timeout_input(
                        t("channel_open_timeout"),
                        t("channel_open_timeout_desc"),
                        &self.prefs.sftp_session_timeout,
                        "set-sftp-session-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpSessionTimeoutChanged(v)),
                    ))
                    .push(Space::new().height(16))
                    .push(timeout_input(
                        t("operation_timeout"),
                        t("operation_timeout_desc"),
                        &self.prefs.sftp_op_timeout,
                        "set-sftp-op-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpOpTimeoutChanged(v)),
                    )),
            );
            // External editor (issue #84): the single application the
            // remote "Open with default text editor" action spawns, plus
            // the persisted auto-upload grant ("Autosave" in the save
            // dialog) so that choice is never a one-way trap.
            let editor_section = panel_section(column![
                text(t("setting_default_editor"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(t("setting_default_editor_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(8),
                dir_row(vec![
                    self.settings_nav_slot_labeled(
                        t("setting_default_editor"),
                        crate::keynav::RowAction::input(iced::widget::Id::new("set-sftp-editor")),
                        10.0,
                        text_input(t("setting_default_editor_placeholder"), &self.prefs.sftp_default_editor)
                            .id(iced::widget::Id::new("set-sftp-editor"))
                            .on_input(|v| Message::Settings(SettingsMessage::SettingSftpDefaultEditorChanged(v)))
                            .padding(10)
                            .width(Length::Fill)
                            .style(crate::widgets::rounded_input_style)
                            .align_x(dir_align_x())
                            .into(),
                    ),
                    Space::new().width(8).into(),
                    self.settings_nav_slot(
                        crate::keynav::RowAction::activate(Message::Settings(
                            SettingsMessage::SettingSftpDefaultEditorBrowse,
                        )),
                        8.0,
                        crate::widgets::styled_button(
                            t("browse"),
                            Message::Settings(SettingsMessage::SettingSftpDefaultEditorBrowse),
                            OryxisColors::t().bg_selected,
                        ),
                    ),
                ])
                .align_y(iced::Alignment::Center),
                Space::new().height(16),
                self.nav_toggle_row(
                    t("setting_edit_autosave_toggle"),
                    self.prefs.sftp_edit_autosave,
                    Message::Settings(SettingsMessage::ToggleSftpEditAutosave),
                ),
                Space::new().height(4),
                text(t("setting_edit_autosave_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            ]);
            // Download destination (issue #114): the panes' own directory
            // is the default, this makes every download stop to ask. The
            // row menu's "Download to..." asks regardless, so the setting
            // is only about the plain action.
            let download_section = panel_section(column![
                text(t("setting_sftp_ask_download_dir"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(t("setting_sftp_ask_download_dir_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(8),
                self.nav_toggle_row(
                    t("setting_sftp_ask_download_dir_toggle"),
                    self.prefs.sftp_ask_download_dir,
                    Message::Settings(SettingsMessage::ToggleSftpAskDownloadDir),
                ),
                Space::new().height(14),
                text(t("setting_sftp_upload_temp_name"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(t("setting_sftp_upload_temp_name_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(8),
                self.nav_toggle_row(
                    t("setting_sftp_upload_temp_name_toggle"),
                    self.prefs.sftp_upload_temp_name,
                    Message::Settings(SettingsMessage::ToggleSftpUploadTempName),
                ),
                Space::new().height(14),
                // Moved here from Settings > Terminal (issue #143): a
                // download setting is looked for next to the rest of
                // the download behavior, and it serves every download
                // path (SFTP, ZMODEM, the sidebar browser) alike.
                self.default_download_dir_row(),
            ]);
            // Where an SFTP console opened on a live session lands. Its
            // own card: it is about the terminal side of SFTP, not
            // about transfers.
            let console_section = panel_section(column![
                text(t("setting_sftp_console_layout"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(t("setting_sftp_console_layout_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(8),
                self.nav_pick_row(
                    t("setting_sftp_console_layout"),
                    crate::state::SftpConsoleLayout::ALL
                        .into_iter()
                        .map(|l| t(l.label_key()).to_string())
                        .collect::<Vec<String>>(),
                    t(self.prefs.sftp_console_layout.label_key()).to_string(),
                    |s: &String| s.clone(),
                    220.0,
                    |v| Message::Settings(SettingsMessage::SftpConsoleLayoutChanged(v)),
                ),
            ]);
            content_col = content_col
                .push(console_section)
                .push(Space::new().height(12))
                .push(download_section)
                .push(Space::new().height(12))
                .push(editor_section)
                .push(Space::new().height(12))
                .push(tuning_section);
        }
        content_col = content_col.push(Space::new().height(24));

        scrollable(
            container(content_col)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-sftp-scroll"))
        .on_scroll(|v| Message::Settings(SettingsMessage::SectionScrolled(v.relative_offset().y)))
        .height(Length::Fill)
        .into()
    }

    pub(super) fn default_download_dir_row(&self) -> Element<'_, Message> {
        let configured = self.prefs.zmodem_download_dir.trim();
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
                text(crate::i18n::t("default_download_dir"))
                    .size(13)
                    .color(crate::theme::OryxisColors::t().text_primary),
                Space::new().height(2),
                text(shown)
                    .size(11)
                    .color(crate::theme::OryxisColors::t().text_muted),
            ]
            .width(Length::Fill)
            .into(),
            Space::new().width(10).into(),
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
            row = row.push(Space::new().width(8)).push(reset);
        }
        container(row.align_y(iced::Alignment::Center))
            .padding(Padding { top: 8.0, ..Padding::ZERO })
            .width(Length::Fill)
            .into()
    }
}
