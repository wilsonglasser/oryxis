//! Icon + color picker modal — opened from the icon box in the host editor.
//!
//! Lets the user override the auto-detected OS icon and color for a given
//! connection. "Reset to auto" clears the override and lets OS detection take
//! over again on the next successful connect.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::os_icon;
use crate::theme::OryxisColors;
use crate::widgets::styled_button;

impl Oryxis {
    pub(crate) fn view_icon_picker(&self) -> Element<'_, Message> {
        // ── Preview: current selection in a bigger box at the top ──
        let preview_color = self
            .icon_picker_color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(OryxisColors::t().accent);
        let preview_icon_id = self
            .icon_picker_icon
            .as_deref()
            .unwrap_or("server");
        let preview = container(
            os_icon::custom_icon_glyph(preview_icon_id)
                .size(24)
                .color(Color::WHITE),
        )
        .width(Length::Fixed(56.0))
        .height(Length::Fixed(56.0))
        .center_x(Length::Fixed(56.0))
        .center_y(Length::Fixed(56.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(preview_color)),
            border: Border { radius: Radius::from(12.0), ..Default::default() },
            ..Default::default()
        });

        // ── Icon grid ──
        let selected_icon = self.icon_picker_icon.clone();
        let mut icon_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for (id, _label) in os_icon::CUSTOM_ICONS.iter() {
            let id_str = id.to_string();
            let is_selected = selected_icon.as_deref() == Some(*id);
            current_row.push(icon_cell(id, is_selected));
            if current_row.len() == 7 {
                icon_rows.push(row(std::mem::take(&mut current_row)).spacing(6).into());
            }
            let _ = id_str;
        }
        if !current_row.is_empty() {
            icon_rows.push(row(current_row).spacing(6).into());
        }
        let icons_block = column![
            text("Icon").size(12).font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::with_name("Inter")
            }).color(OryxisColors::t().text_secondary),
            Space::new().height(8),
            column(icon_rows).spacing(6),
        ];
        let _ = id_str_unused_suppress(&selected_icon); // keep borrowck simple

        // ── Color grid ──
        let mut color_row_children: Vec<Element<'_, Message>> = Vec::new();
        for hex in os_icon::PRESET_COLORS.iter() {
            let hex_str = hex.to_string();
            let is_selected = self.icon_picker_color.as_deref() == Some(*hex);
            color_row_children.push(color_swatch(hex, is_selected));
            let _ = hex_str;
        }
        let color_grid = {
            // Reshape into two rows of 8 for a tidy 2xN layout.
            let mut rows: Vec<Element<'_, Message>> = Vec::new();
            let mut chunk: Vec<Element<'_, Message>> = Vec::new();
            for c in color_row_children {
                chunk.push(c);
                if chunk.len() == 7 {
                    rows.push(row(std::mem::take(&mut chunk)).spacing(6).into());
                }
            }
            if !chunk.is_empty() {
                rows.push(row(chunk).spacing(6).into());
            }
            column(rows).spacing(6)
        };

        let hex_input = text_input("#RRGGBB", &self.icon_picker_hex_input)
            .on_input(Message::IconPickerHexInputChanged)
            .padding(8)
            .size(12)
            .width(120);

        let colors_block = column![
            text("Background color").size(12).font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::with_name("Inter")
            }).color(OryxisColors::t().text_secondary),
            Space::new().height(8),
            color_grid,
            Space::new().height(10),
            row![
                text("Custom:").size(11).color(OryxisColors::t().text_muted),
                Space::new().width(8),
                hex_input,
            ].align_y(iced::Alignment::Center),
        ];

        // ── Footer actions ──
        let actions = row![
            styled_button("Reset to auto", Message::IconPickerResetAuto, OryxisColors::t().bg_selected),
            Space::new().width(Length::Fill),
            styled_button("Cancel", Message::HideIconPicker, OryxisColors::t().bg_hover),
            Space::new().width(8),
            styled_button("Save", Message::IconPickerSave, OryxisColors::t().accent),
        ]
        .align_y(iced::Alignment::Center);

        // Header (preview + title) and footer (actions) stay fixed; the
        // icon grid + color palette scroll in between so the modal never
        // exceeds the viewport height.
        let header = row![
            preview,
            Space::new().width(14),
            column![
                text("Custom icon").size(16).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::with_name("Inter")
                }).color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text("Override the auto-detected icon and color. Use \u{201c}Reset to auto\u{201d} to let OS detection drive it again.")
                    .size(11).color(OryxisColors::t().text_muted),
            ],
        ]
        .align_y(iced::Alignment::Center);

        let scroll_area = scrollable(
            column![
                icons_block,
                Space::new().height(20),
                colors_block,
                Space::new().height(8),
            ]
            .padding(iced::Padding { top: 0.0, right: 10.0, bottom: 0.0, left: 0.0 }),
        )
        .height(Length::Fill);

        let body = container(
            column![
                header,
                Space::new().height(16),
                scroll_area,
                Space::new().height(12),
                actions,
            ],
        )
        .padding(20)
        .width(Length::Fixed(560.0))
        .height(Length::Fixed(600.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}

fn icon_cell<'a>(id: &'static str, is_selected: bool) -> Element<'a, Message> {
    let border_color = if is_selected {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().border
    };
    let border_width = if is_selected { 2.0 } else { 1.0 };
    button(
        container(
            os_icon::custom_icon_glyph(id)
                .size(16)
                .color(OryxisColors::t().text_primary),
        )
        .center_x(Length::Fixed(44.0))
        .center_y(Length::Fixed(44.0)),
    )
    .on_press(Message::IconPickerSelectIcon(id.to_string()))
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => OryxisColors::t().bg_surface,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(8.0),
                color: border_color,
                width: border_width,
            },
            ..Default::default()
        }
    })
    .into()
}

fn color_swatch<'a>(hex: &'static str, is_selected: bool) -> Element<'a, Message> {
    let color = parse_hex_color(hex).unwrap_or(OryxisColors::t().accent);
    let ring_color = if is_selected {
        OryxisColors::t().text_primary
    } else {
        Color::TRANSPARENT
    };
    let ring_width = if is_selected { 2.0 } else { 0.0 };

    button(
        container(Space::new().width(Length::Fixed(28.0)).height(Length::Fixed(28.0)))
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0)),
    )
    .on_press(Message::IconPickerSelectColor(hex.to_string()))
    .style(move |_, status| {
        let (bg, extra) = match status {
            BtnStatus::Hovered => (color, 0.15),
            _ => (color, 0.0),
        };
        button::Style {
            background: Some(Background::Color(Color { a: 1.0 - extra * 0.0, ..bg })),
            border: Border {
                radius: Radius::from(6.0),
                color: ring_color,
                width: ring_width,
            },
            ..Default::default()
        }
    })
    .into()
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

#[inline]
fn id_str_unused_suppress(_: &Option<String>) {}

/// Transparent backdrop that dismisses the picker on click.
pub(crate) fn icon_picker_backdrop<'a>() -> Element<'a, Message> {
    MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Message::HideIconPicker)
    .into()
}
