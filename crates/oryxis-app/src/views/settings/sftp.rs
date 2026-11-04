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
                text_input("2", &self.vaults.cur().prefs.sftp_concurrency)
                    .id(iced::widget::Id::new("set-sftp-concurrency"))
                    .on_input(|v| Message::Settings(SettingsMessage::SettingSftpConcurrencyChanged(v)))
                    .padding(10)
                    .width(240)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
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
                        .boxed(),
                ),
            ]
        };

        // Enable/disable lives on the Plugins screen now; this
        // section only renders while SFTP is enabled, showing its
        // tuning knobs (parallelism, timeouts).
        let mut content_col: iced::widget::Column<iced::Element<'_, Message>> = column![]
            .width(Length::Fill)
            .align_x(dir_align_x());

        if self.vaults.cur().sftp_enabled {
            // Parallelism + the four timeout knobs are one transfer-
            // tuning theme, so they share a single card (each block
            // keeps its 13 px sub-title, 16 px between blocks).
            let tuning_section = panel_section(
                build_concurrency_block()
                    .push(Space::new().height(16).boxed())
                    .push(timeout_input(
                        t("connect_timeout"),
                        t("connect_timeout_desc"),
                        &self.vaults.cur().prefs.sftp_connect_timeout,
                        "set-sftp-connect-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpConnectTimeoutChanged(v)),
                    ).boxed())
                    .push(Space::new().height(16).boxed())
                    .push(timeout_input(
                        t("auth_timeout"),
                        t("auth_timeout_desc"),
                        &self.vaults.cur().prefs.sftp_auth_timeout,
                        "set-sftp-auth-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpAuthTimeoutChanged(v)),
                    ).boxed())
                    .push(Space::new().height(16).boxed())
                    .push(timeout_input(
                        t("channel_open_timeout"),
                        t("channel_open_timeout_desc"),
                        &self.vaults.cur().prefs.sftp_session_timeout,
                        "set-sftp-session-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpSessionTimeoutChanged(v)),
                    ).boxed())
                    .push(Space::new().height(16).boxed())
                    .push(timeout_input(
                        t("operation_timeout"),
                        t("operation_timeout_desc"),
                        &self.vaults.cur().prefs.sftp_op_timeout,
                        "set-sftp-op-timeout",
                        |v| Message::Settings(SettingsMessage::SettingSftpOpTimeoutChanged(v)),
                    ).boxed()),
            );
            // External editor (issue #84): the single application the
            // remote "Open with default text editor" action spawns, plus
            // the persisted auto-upload grant ("Autosave" in the save
            // dialog) so that choice is never a one-way trap.
            let editor_section = panel_section(column![
                crate::widgets::setting_label(t("setting_default_editor"), 13.0, OryxisColors::t().text_primary),
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
                        text_input(t("setting_default_editor_placeholder"), &self.device_prefs.sftp_default_editor)
                            .id(iced::widget::Id::new("set-sftp-editor"))
                            .on_input(|v| Message::Settings(SettingsMessage::SettingSftpDefaultEditorChanged(v)))
                            .padding(10)
                            .width(Length::Fill)
                            .style(crate::widgets::rounded_input_style)
                            .align_x(dir_align_x())
                            .boxed(),
                    ),
                    Space::new().width(8).boxed(),
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
                    self.vaults.cur().prefs.sftp_edit_autosave,
                    Message::Settings(SettingsMessage::ToggleSftpEditAutosave),
                ),
                Space::new().height(4),
                text(t("setting_edit_autosave_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            ]);
            // Download destination (issue #114): the panes' own directory
            // is the default, this makes every SFTP download stop to ask.
            // The row menu's "Download to..." asks regardless, so this
            // setting is only about the plain action. `sz` has its own
            // switch on Settings > Terminal, next to the download folder:
            // an `sz` in a terminal has no destination pane to point at,
            // so the two are asked separately.
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
                    self.vaults.cur().prefs.sftp_ask_download_dir,
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
                    self.vaults.cur().prefs.sftp_upload_temp_name,
                    Message::Settings(SettingsMessage::ToggleSftpUploadTempName),
                ),
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
                    t(self.vaults.cur().prefs.sftp_console_layout.label_key()).to_string(),
                    |s: &String| s.clone(),
                    220.0,
                    |v| Message::Settings(SettingsMessage::SftpConsoleLayoutChanged(v)),
                ),
            ]);
            // What a double click (and Enter) does on a row of the
            // dual-pane browser: FileZilla's two choices, one picker per
            // kind of row. The `..` row is outside both.
            let double_click_section = panel_section(column![
                text(t("setting_sftp_double_click"))
                    .size(13)
                    .color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(t("setting_sftp_double_click_desc"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(8),
                self.nav_pick_row(
                    t("setting_sftp_dir_double_click"),
                    crate::state::SftpDirDoubleClick::ALL
                        .into_iter()
                        .map(|a| t(a.label_key()).to_string())
                        .collect::<Vec<String>>(),
                    t(self.vaults.cur().prefs.sftp_dir_double_click.label_key()).to_string(),
                    |s: &String| s.clone(),
                    260.0,
                    |v| Message::Settings(SettingsMessage::SftpDirDoubleClickChanged(v)),
                ),
                Space::new().height(8),
                self.nav_pick_row(
                    t("setting_sftp_file_double_click"),
                    crate::state::SftpFileDoubleClick::ALL
                        .into_iter()
                        .map(|a| t(a.label_key()).to_string())
                        .collect::<Vec<String>>(),
                    t(self.vaults.cur().prefs.sftp_file_double_click.label_key()).to_string(),
                    |s: &String| s.clone(),
                    260.0,
                    |v| Message::Settings(SettingsMessage::SftpFileDoubleClickChanged(v)),
                ),
            ]);
            content_col = content_col
                .push(double_click_section)
                .push(Space::new().height(12).boxed())
                .push(console_section)
                .push(Space::new().height(12).boxed())
                .push(download_section)
                .push(Space::new().height(12).boxed())
                .push(editor_section)
                .push(Space::new().height(12).boxed())
                .push(tuning_section);
        }
        content_col = content_col.push(Space::new().height(24).boxed());

        scrollable(
            container(content_col)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        // Stable id so the keyboard router can keep the selected row
        // in view.
        .id(iced::widget::Id::new("settings-sftp-scroll"))
        .on_scroll(|s| Message::Settings(SettingsMessage::SectionScrolled(s.viewport.relative_offset().y)))
        .height(Length::Fill)
        .boxed()
    }
}
