//! Per-host terminal theme picker, opened from the "Terminal Theme"
//! row in the host editor. Renders a column of palette swatch cards;
//! the first card is the "inherit global theme" sentinel. Selecting a
//! card commits to `editor_form.terminal_theme` and closes the modal.

use iced::border::Radius;
use iced::widget::{column, container, scrollable, text, Space};
use iced::{Background, Border, Element, Length};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::styled_button;

impl Oryxis {
    pub(crate) fn view_terminal_theme_picker(&self) -> Element<'_, Message> {
        // Header, title + short description matching the row in the
        // host editor that opened this modal.
        let header = column![
            text(t("terminal_theme")).size(16).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(OryxisColors::t().text_primary),
            Space::new().height(4),
            text(t("host_terminal_theme_desc"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        ];

        // Cards, first row is the inherit sentinel, the rest are
        // real palette previews. Click commits + closes via the
        // EditorTerminalThemeChanged handler. The inherit row renders
        // as a real theme card previewing the GLOBAL palette (it used
        // to be an input-looking box that read as a text field), so
        // "use global" shows exactly what inheriting means.
        let mut cards: Vec<Element<'_, Message>> = Vec::new();
        let global_name = self.resolve_global_terminal_theme_name();
        let global_palette = self
            .terminal_palette_for_name(&global_name)
            .unwrap_or_default();
        let inherit_label = format!(
            "{} ({})",
            t("terminal_theme_inherit_global"),
            global_name,
        );
        cards.push(crate::widgets::terminal_theme_card(
            global_palette,
            &inherit_label,
            self.editor_form.terminal_theme.is_none(),
            Message::EditorTerminalThemeChanged(String::new()),
        ));
        for theme in oryxis_terminal::TerminalTheme::ALL.iter() {
            let is_selected =
                self.editor_form.terminal_theme.as_deref() == Some(theme.name());
            cards.push(crate::widgets::terminal_theme_card(
                theme.palette(),
                theme.name(),
                is_selected,
                Message::EditorTerminalThemeChanged(theme.name().to_string()),
            ));
        }
        // User-defined themes, selectable per host like the built-ins.
        for ct in self.custom_terminal_themes.iter() {
            let is_selected =
                self.editor_form.terminal_theme.as_deref() == Some(ct.name.as_str());
            let palette = self.terminal_palette_for_name(&ct.name).unwrap_or_default();
            cards.push(crate::widgets::terminal_theme_card(
                palette,
                &ct.name,
                is_selected,
                Message::EditorTerminalThemeChanged(ct.name.clone()),
            ));
        }

        let scroll_area = scrollable(
            column(cards).spacing(8).padding(iced::Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 0.0,
            }),
        )
        .height(Length::Fill);

        let close_btn = styled_button(
            t("close"),
            Message::EditorCloseThemePicker,
            OryxisColors::t().bg_hover,
        );
        // Dismiss action hugs the trailing edge (dialog convention),
        // mirrored under RTL.
        let close_align = if crate::i18n::is_rtl_layout() {
            iced::alignment::Horizontal::Left
        } else {
            iced::alignment::Horizontal::Right
        };

        let dialog = container(
            column![
                header,
                Space::new().height(16),
                scroll_area,
                Space::new().height(12),
                container(close_btn)
                    .width(Length::Fill)
                    .align_x(close_align),
            ],
        )
        .padding(20)
        .width(Length::Fixed(480.0))
        .height(Length::Fixed(560.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        // Bare card; `widgets::modal_overlay` (the caller) owns centering,
        // the absorbing scrim, and the click-trap.
        dialog.into()
    }
}
