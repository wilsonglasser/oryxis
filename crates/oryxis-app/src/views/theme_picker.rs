//! Per-host terminal theme picker, opened from the "Terminal Theme"
//! row in the host editor. Renders a column of palette swatch cards;
//! the first card is the "inherit global theme" sentinel. Selecting a
//! card commits to `editor_form.terminal_theme` and closes the modal.

use iced::border::Radius;
use iced::widget::{column, container, scrollable, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length};

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
        // EditorTerminalThemeChanged handler.
        let mut cards: Vec<Element<'_, Message>> = Vec::new();
        cards.push(crate::widgets::terminal_theme_inherit_card(
            t("terminal_theme_inherit_global"),
            self.editor_form.terminal_theme.is_none(),
            Message::EditorTerminalThemeChanged(String::new()),
        ));
        for theme in oryxis_terminal::TerminalTheme::ALL.iter() {
            let is_selected =
                self.editor_form.terminal_theme.as_deref() == Some(theme.name());
            cards.push(crate::widgets::terminal_theme_card(
                *theme,
                is_selected,
                Message::EditorTerminalThemeChanged(theme.name().to_string()),
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

        let dialog = container(
            column![
                header,
                Space::new().height(16),
                scroll_area,
                Space::new().height(12),
                container(close_btn)
                    .width(Length::Fill)
                    .align_x(crate::widgets::dir_align_x()),
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

        // Inner MouseArea swallows clicks on the dialog so they don't
        // bubble out to the scrim's HideIconPicker, same pattern as
        // tab_jump and the icon picker.
        let dialog_capture: Element<'_, Message> = MouseArea::new(dialog)
            .on_press(Message::NoOp)
            .into();

        let centered = container(dialog_capture)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

        MouseArea::new(
            container(centered)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(
                        0.0, 0.0, 0.0, 0.5,
                    ))),
                    ..Default::default()
                }),
        )
        .on_press(Message::EditorCloseThemePicker)
        .into()
    }
}
