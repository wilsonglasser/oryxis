//! Settings -> Shortcuts section view. Split out of views/settings/mod.rs.

use super::*;
use iced::widget::column;

impl Oryxis {
    pub(crate) fn view_settings_shortcuts(&self) -> Element<'_, Message> {
        use crate::hotkeys::{default_bindings, HotkeyAction};
        let defaults = default_bindings();

        // Header: title + hint + global reset button.
        let header = column![
                                text(crate::i18n::t("hotkey_edit_hint"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(10),
            styled_button(
                crate::i18n::t("hotkey_reset_all"),
                Message::ResetAllHotkeys,
                OryxisColors::t().bg_selected,
            ),
            Space::new().height(16),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        let mut rows_col = column![header]
            .spacing(8)
            .width(Length::Fill)
            .align_x(dir_align_x());

        for action in HotkeyAction::all() {
            let row_el = self.hotkey_editor_row(*action, defaults.get(action).copied());
            rows_col = rows_col.push(row_el);
        }

        // Read-only footer: terminal copy/paste and Ctrl+Wheel
        // zoom are handled in different layers (the terminal
        // widget owns copy selection; the wheel handler lives
        // in the scroll event). Surfaced here so the user
        // doesn't think they're missing.
        let static_rows = column![
            Space::new().height(20),
            text(crate::i18n::t("hotkey_terminal_handled"))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(8),
            shortcut_row(
                vec![key_badge("Ctrl"), key_badge("Shift"), key_badge("C")],
                crate::i18n::t("copy_terminal"),
            ),
            shortcut_row(
                vec![key_badge("Ctrl"), key_badge("Shift"), key_badge("V")],
                crate::i18n::t("paste_terminal"),
            ),
            shortcut_row(
                vec![key_badge("Ctrl"), key_badge("Wheel")],
                crate::i18n::t("font_zoom_wheel"),
            ),
        ]
        .spacing(8)
        .width(Length::Fill)
        .align_x(dir_align_x());
        rows_col = rows_col.push(static_rows);

        scrollable(
            container(rows_col)
                .padding(Padding { top: 24.0, right: 24.0, bottom: 24.0, left: 24.0 }),
        )
        .height(Length::Fill)
        .into()
    }
}
