//! Icon + color picker modal, opened from the icon box in the host editor.
//!
//! Lets the user override the auto-detected OS icon and color for a given
//! connection. "Reset to auto" clears the override and lets OS detection take
//! over again on the next successful connect.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{
    button, column, container, row, scrollable, text, text_input, MouseArea, Space, Stack,
};
use iced::{Background, Border, Color, Element, Length, Point};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::os_icon;
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row, styled_button};

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
                .view(28.0, Color::WHITE),
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

        // ── Icon section: curated presets up front, with a search box
        // that filters the full Lucide library. The whole font is
        // bundled already, so searching every glyph adds no weight. ──
        let selected_icon = self.icon_picker_icon.clone();
        let query = self.icon_picker_icon_search.trim().to_string();

        // Ids to render: search hits when the box has a query, otherwise
        // the curated preset list.
        let icon_ids: Vec<&'static str> = if query.is_empty() {
            os_icon::CUSTOM_ICONS.iter().map(|(id, _)| *id).collect()
        } else {
            os_icon::lucide_search(&query, 120)
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };

        let mut icon_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut current_row: Vec<Element<'_, Message>> = Vec::new();
        for id in icon_ids {
            let is_selected = selected_icon.as_deref() == Some(id);
            current_row.push(icon_cell(id, is_selected));
            if current_row.len() == 7 {
                icon_rows.push(dir_row(std::mem::take(&mut current_row)).spacing(6).into());
            }
        }
        if !current_row.is_empty() {
            icon_rows.push(dir_row(current_row).spacing(6).into());
        }

        let icon_search = text_input(t("icon_search"), &self.icon_picker_icon_search)
            .on_input(Message::IconPickerIconSearchChanged)
            .padding(8)
            .size(12)
            .width(Length::Fill)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());

        let icon_grid: Element<'_, Message> = if icon_rows.is_empty() {
            text(t("no_matches")).size(12).color(OryxisColors::t().text_muted).into()
        } else {
            column(icon_rows).spacing(6).into()
        };

        let icons_block = column![
            text(t("icon")).size(12).font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(OryxisColors::t().text_secondary),
            Space::new().height(8),
            icon_search,
            Space::new().height(10),
            icon_grid,
        ];

        // ── Background color: a swatch + hex field. Clicking the swatch
        // opens the shared HSV picker (same widget the custom-theme
        // editor uses) as a popover, so the picker isn't always taking
        // up vertical space in the modal. ──
        let current_color = self
            .icon_picker_color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(OryxisColors::t().accent);

        let hex_input = text_input("#RRGGBB", &self.icon_picker_hex_input)
            .on_input(Message::IconPickerHexInputChanged)
            .padding(8)
            .size(12)
            .width(120)
            .style(crate::widgets::rounded_input_style).align_x(dir_align_x());

        let swatch = button(Space::new().width(28).height(28))
            .on_press(Message::IconPickerOpenColorPopover)
            .padding(0)
            .style(move |_, status| {
                let border_color = match status {
                    BtnStatus::Hovered => OryxisColors::t().text_primary,
                    _ => OryxisColors::t().border,
                };
                button::Style {
                    background: Some(Background::Color(current_color)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: border_color,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });

        let colors_block = column![
            text(t("background_color")).size(12).font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(OryxisColors::t().text_secondary),
            Space::new().height(8),
            dir_row(vec![
                swatch.into(),
                Space::new().width(10).into(),
                hex_input.into(),
            ]).align_y(iced::Alignment::Center),
        ];


        // ── Footer actions ──
        let actions = dir_row(vec![
            styled_button(t("reset_to_auto"), Message::IconPickerResetAuto, OryxisColors::t().bg_selected),
            Space::new().width(Length::Fill).into(),
            styled_button(t("cancel"), Message::HideIconPicker, OryxisColors::t().bg_hover),
            Space::new().width(8).into(),
            styled_button(t("save"), Message::IconPickerSave, OryxisColors::t().accent),
        ])
        .align_y(iced::Alignment::Center);

        // Header (preview + title) and footer (actions) stay fixed; the
        // icon grid + color palette scroll in between so the modal never
        // exceeds the viewport height.
        let header = dir_row(vec![
            preview.into(),
            Space::new().width(14).into(),
            column![
                text(t("custom_icon")).size(16).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                }).color(OryxisColors::t().text_primary),
                Space::new().height(4),
                text(t("custom_icon_desc"))
                    .size(11).color(OryxisColors::t().text_muted),
            ].into(),
        ])
        .align_y(iced::Alignment::Center);

        // Colors first, icons below: the background colour is the
        // dominant visual cue the user picks, the glyph layers on top.
        // Putting it on top mirrors the natural top-down editing order
        // and avoids the icon grid pushing the colours below the fold
        // on shorter viewports.
        let scroll_area = scrollable(
            column![
                colors_block,
                Space::new().height(20),
                icons_block,
                Space::new().height(8),
            ]
            // Extra trailing pad so the scrollbar overlay never bites
            // into the right-most swatch / icon cell. 22 px clears the
            // scrollbar's full width plus a small visual buffer.
            .padding(iced::Padding { top: 0.0, right: 22.0, bottom: 0.0, left: 0.0 }),
        )
        .height(Length::Fill);

        let dialog = container(
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

        // Two MouseAreas: outer scrim dismisses on click-outside, inner
        // wrapper around the dialog absorbs clicks so they don't bubble
        // out and accidentally trip the scrim's HideIconPicker.
        let dialog_capture: Element<'_, Message> = MouseArea::new(dialog)
            .on_press(Message::NoOp)
            .into();

        let centered = container(dialog_capture)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

        // `opaque` makes the scrim swallow every mouse event (hover +
        // scroll, not just clicks) so nothing bleeds through to the host
        // list / editor stacked beneath the modal. Without it iced's
        // Stack lets hover/scroll propagate to the lower layer.
        let modal: Element<'_, Message> = MouseArea::new(
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
        .on_press(Message::HideIconPicker)
        .into();

        // The HSV color picker floats on top of the modal as a popover
        // anchored at the cursor, so the swatch acts like a context menu.
        let mut stack = Stack::new()
            .push(modal)
            .width(Length::Fill)
            .height(Length::Fill);
        if let Some(anchor) = self.icon_color_popover {
            stack = stack.push(self.icon_color_popover_view(anchor));
        }
        iced::widget::opaque(stack)
    }

    /// The floating HSV picker shown when the background-color swatch is
    /// clicked. Positioned at `anchor` (clamped to the window) with a
    /// full-screen backdrop that dismisses it on click-outside.
    fn icon_color_popover_view(&self, anchor: Point) -> Element<'_, Message> {
        let current = self
            .icon_picker_color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(OryxisColors::t().accent);

        let card = container(
            crate::color_picker::color_picker(current, Message::IconPickerSelectColor),
        )
        .padding(12)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(10.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
        let card_trap: Element<'_, Message> =
            MouseArea::new(card).on_press(Message::NoOp).into();

        // Picker box footprint, used to clamp it inside the window.
        const PW: f32 = 238.0;
        const PH: f32 = 228.0;
        let x = anchor.x.min((self.window_size.width - PW).max(0.0)).max(0.0);
        let y = anchor.y.min((self.window_size.height - PH).max(0.0)).max(0.0);
        let positioned = column![
            Space::new().height(y),
            row![Space::new().width(x), card_trap],
        ];

        let pop_backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill),
        )
        .on_press(Message::IconPickerCloseColorPopover)
        .into();

        Stack::new()
            .push(pop_backdrop)
            .push(positioned)
            .width(Length::Fill)
            .height(Length::Fill)
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
                .view(20.0, OryxisColors::t().text_primary),
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

fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

