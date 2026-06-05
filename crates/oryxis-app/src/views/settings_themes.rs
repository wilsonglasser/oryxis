//! Custom terminal theme editor, rendered as a centered modal over the
//! app. Opened from the "+" card in the Settings -> Terminal theme grid
//! (create) or the edit affordance on a custom theme card. Built-in
//! presets aren't editable; custom themes live inline in that same grid.

use iced::border::Radius;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::state::ThemeColorSlot;
use crate::theme::OryxisColors;
use crate::widgets::{dir_row, parse_hex_color};

/// Standard ANSI slot names (technical identifiers, left untranslated like
/// the palette comments in `colors.rs`).
const ANSI_NAMES: [&str; 16] = [
    "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
    "Bright Black", "Bright Red", "Bright Green", "Bright Yellow",
    "Bright Blue", "Bright Magenta", "Bright Cyan", "Bright White",
];

impl Oryxis {
    /// The custom-theme editor modal. Caller renders it (over the base) only
    /// while `theme_editor` is `Some`.
    pub(crate) fn view_theme_editor_modal(&self) -> Element<'_, Message> {
        let Some(form) = self.theme_editor.as_ref() else {
            return Space::new().into();
        };
        let title = if form.editing_id.is_some() {
            t("theme_edit")
        } else {
            t("theme_new_custom")
        };

        let name_input = text_input(t("theme_name"), &form.name)
            .on_input(Message::ThemeEditorNameChanged)
            .padding(10)
            .size(13)
            .style(crate::widgets::rounded_input_style);

        // Background / foreground / cursor, then the 16 ANSI entries. Each
        // swatch opens the compact color-picker popover for its slot.
        let mut slots = column![
            color_row(t("theme_background"), ThemeColorSlot::Background, &form.background),
            color_row(t("theme_foreground"), ThemeColorSlot::Foreground, &form.foreground),
            color_row(t("theme_cursor"), ThemeColorSlot::Cursor, &form.cursor),
            Space::new().height(10),
            text(t("theme_ansi_colors"))
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(6),
        ]
        .spacing(6);
        for i in 0..16u8 {
            slots = slots.push(color_row(
                ANSI_NAMES[i as usize],
                ThemeColorSlot::Ansi(i),
                &form.ansi[i as usize],
            ));
        }

        let form_col = column![
            text(t("theme_name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            name_input,
            Space::new().height(16),
            slots,
        ]
        .width(Length::Fixed(330.0));

        let preview = column![
            text(t("theme_preview")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(8),
            theme_preview(form),
        ];

        let mut footer = column![].spacing(8);
        if let Some(err) = &form.error {
            footer = footer.push(text(err).size(12).color(OryxisColors::t().error));
        }
        footer = footer.push(
            dir_row(vec![
                Space::new().width(Length::Fill).into(),
                button(text(t("cancel")).size(13).color(OryxisColors::t().text_secondary))
                    .on_press(Message::ThemeEditorClose)
                    .padding(Padding { top: 9.0, right: 16.0, bottom: 9.0, left: 16.0 })
                    .style(|_, status| outline_btn_style(status))
                    .into(),
                Space::new().width(8).into(),
                button(text(t("save")).size(13).color(Color::WHITE))
                    .on_press(Message::ThemeEditorSave)
                    .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        );

        // Pad the scrollable content on the right so the scrollbar doesn't
        // sit on top of the hex inputs.
        let scroll_form = scrollable(
            container(form_col)
                .padding(Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 0.0 }),
        )
        .height(Length::Fixed(380.0));

        let body = column![
            text(title).size(18).color(OryxisColors::t().text_primary),
            Space::new().height(16),
            dir_row(vec![
                scroll_form.into(),
                Space::new().width(24).into(),
                preview.into(),
            ]),
            Space::new().height(16),
            footer,
        ];

        let card = container(body)
            .padding(24)
            .width(Length::Fixed(720.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });

        // Trap clicks inside the card so they don't dismiss; backdrop click
        // closes the editor.
        let card_trap: Element<'_, Message> =
            MouseArea::new(card).on_press(Message::NoOp).into();

        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill).style(|_| {
                container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                    ..Default::default()
                }
            }),
        )
        .on_press(Message::ThemeEditorClose)
        .into();

        let mut stack = iced::widget::Stack::new()
            .push(backdrop)
            .push(
                container(card_trap)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill);
        // Compact color-picker popover, anchored at the clicked swatch.
        if let Some((slot, anchor)) = self.theme_color_popover {
            stack = stack.push(self.color_popover(form, slot, anchor));
        }
        // `opaque` makes the whole modal capture every mouse event so
        // clicks / scroll / hover don't fall through to the view behind.
        iced::widget::opaque(stack)
    }

    /// Compact color picker popover (SV square + hue bar + hex + presets),
    /// positioned at `anchor`, dismissed by clicking outside it.
    fn color_popover<'a>(
        &'a self,
        form: &'a crate::state::ThemeEditorForm,
        slot: ThemeColorSlot,
        anchor: iced::Point,
    ) -> Element<'a, Message> {
        let hex = match slot {
            ThemeColorSlot::Background => &form.background,
            ThemeColorSlot::Foreground => &form.foreground,
            ThemeColorSlot::Cursor => &form.cursor,
            ThemeColorSlot::Ansi(i) => &form.ansi[i as usize],
        };
        let color = parse_hex_color(hex).unwrap_or(Color::BLACK);

        let card = container(
            column![
                crate::color_picker::color_picker(color, move |hex| {
                    Message::ThemeEditorColorChanged(slot, hex)
                }),
                Space::new().height(10),
                text_input("#RRGGBB", hex)
                    .on_input(move |v| Message::ThemeEditorColorChanged(slot, v))
                    .padding(7)
                    .size(12)
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(10),
                preset_grid(slot),
            ]
            .spacing(0),
        )
        .padding(12)
        .width(Length::Fixed(236.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_primary)),
            border: Border {
                radius: Radius::from(10.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            ..Default::default()
        });
        let card_trap: Element<'_, Message> =
            MouseArea::new(card).on_press(Message::NoOp).into();

        // Clamp the anchor so the popover stays on screen.
        const PW: f32 = 236.0;
        const PH: f32 = 330.0;
        let x = anchor.x.min((self.window_size.width - PW).max(0.0)).max(0.0);
        let y = anchor.y.min((self.window_size.height - PH).max(0.0)).max(0.0);
        let positioned = column![row![Space::new().width(x), card_trap]]
            .push(Space::new().height(0));
        let positioned = column![Space::new().height(y), positioned];

        let pop_backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill),
        )
        .on_press(Message::ThemeEditorClosePicker)
        .into();

        iced::widget::Stack::new()
            .push(pop_backdrop)
            .push(positioned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Import-a-scheme modal: paste an iTerm / Windows Terminal / base16
    /// scheme; on import the parsed colors open in the editor for review.
    pub(crate) fn view_theme_import_modal(&self) -> Element<'_, Message> {
        let name_input = text_input(t("theme_name"), &self.theme_import_name)
            .on_input(Message::ThemeImportNameChanged)
            .padding(10)
            .size(13)
            .style(crate::widgets::rounded_input_style);

        let paste = container(
            iced::widget::text_editor(&self.theme_import_content)
                .on_action(Message::ThemeImportContentAction)
                .padding(10)
                .height(Length::Fixed(220.0))
                .font(iced::Font::MONOSPACE)
                .size(11),
        );

        let mut col = column![
            text(t("theme_import_title")).size(18).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(t("theme_import_hint")).size(12).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            text(t("theme_name")).size(12).color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            name_input,
            Space::new().height(12),
            paste,
        ]
        .spacing(0);
        if let Some(err) = &self.theme_import_error {
            col = col.push(Space::new().height(8));
            col = col.push(text(err).size(12).color(OryxisColors::t().error));
        }
        col = col.push(Space::new().height(16));
        col = col.push(
            dir_row(vec![
                Space::new().width(Length::Fill).into(),
                button(text(t("cancel")).size(13).color(OryxisColors::t().text_secondary))
                    .on_press(Message::ThemeImportClose)
                    .padding(Padding { top: 9.0, right: 16.0, bottom: 9.0, left: 16.0 })
                    .style(|_, status| outline_btn_style(status))
                    .into(),
                Space::new().width(8).into(),
                button(text(t("theme_import")).size(13).color(Color::WHITE))
                    .on_press(Message::ThemeImportApply)
                    .padding(Padding { top: 9.0, right: 18.0, bottom: 9.0, left: 18.0 })
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(OryxisColors::t().accent)),
                        border: Border { radius: Radius::from(8.0), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        );

        let card = container(col)
            .padding(24)
            .width(Length::Fixed(560.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
        let card_trap: Element<'_, Message> =
            MouseArea::new(card).on_press(Message::NoOp).into();
        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new()).width(Length::Fill).height(Length::Fill).style(|_| {
                container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                    ..Default::default()
                }
            }),
        )
        .on_press(Message::ThemeImportClose)
        .into();
        let stack = iced::widget::Stack::new()
            .push(backdrop)
            .push(
                container(card_trap)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill);
        iced::widget::opaque(stack)
    }
}

impl Oryxis {
    /// A selectable custom terminal theme card with floating edit / delete
    /// icons revealed on hover (project card-action convention).
    pub(crate) fn terminal_custom_theme_card<'a>(
        &'a self,
        idx: usize,
        name: &'a str,
        palette: oryxis_terminal::TerminalPalette,
        is_selected: bool,
    ) -> Element<'a, Message> {
        let card = crate::widgets::terminal_theme_card(
            palette,
            name,
            is_selected,
            Message::TerminalThemeChanged(name.to_string()),
        );
        let mut stack = iced::widget::Stack::new().push(card);
        if self.hovered_theme_card == Some(idx) {
            let actions = container(
                dir_row(vec![
                    theme_icon_btn(iced_fonts::lucide::pencil(), Message::ThemeEditorEdit(idx)),
                    Space::new().width(4).into(),
                    theme_icon_btn(iced_fonts::lucide::trash(), Message::ThemeDelete(idx)),
                ])
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top)
            .padding(6);
            stack = stack.push(actions);
        }
        MouseArea::new(stack)
            .on_enter(Message::ThemeCardHovered(idx))
            .on_exit(Message::ThemeCardUnhovered)
            .into()
    }
}

/// "+ New custom theme" card that sits at the end of the terminal theme
/// grid and opens the editor.
pub(crate) fn terminal_theme_add_card<'a>() -> Element<'a, Message> {
    button(
        container(
            dir_row(vec![
                iced_fonts::lucide::plus()
                    .size(14)
                    .color(OryxisColors::t().accent)
                    .into(),
                Space::new().width(8).into(),
                text(t("theme_new_custom"))
                    .size(13)
                    .color(OryxisColors::t().accent)
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .width(Length::Fill),
    )
    .on_press(Message::ThemeEditorNew)
    .padding(0)
    .width(Length::Fill)
    .style(|_, status| {
        let bg = match status {
            button::Status::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .into()
}

/// "Import" card that opens the paste-a-scheme modal.
pub(crate) fn terminal_theme_import_card<'a>() -> Element<'a, Message> {
    button(
        container(
            dir_row(vec![
                iced_fonts::lucide::download()
                    .size(14)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
                Space::new().width(8).into(),
                text(t("theme_import"))
                    .size(13)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .width(Length::Fill),
    )
    .on_press(Message::ThemeImportOpen)
    .padding(0)
    .width(Length::Fill)
    .style(|_, status| {
        let bg = match status {
            button::Status::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .into()
}

/// Small floating icon button used for the per-card edit / delete actions.
fn theme_icon_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(icon.size(13).color(OryxisColors::t().text_primary))
            .center_x(Length::Fixed(24.0))
            .center_y(Length::Fixed(24.0)),
    )
    .on_press(msg)
    .padding(0)
    .style(|_, status| {
        let bg = match status {
            button::Status::Hovered => OryxisColors::t().bg_hover,
            _ => OryxisColors::t().bg_surface,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .into()
}

/// One editable color slot: label, clickable swatch (opens the color-picker
/// popover for this slot), hex input.
fn color_row<'a>(
    label: &'a str,
    slot: ThemeColorSlot,
    hex: &'a str,
) -> Element<'a, Message> {
    let swatch_color = parse_hex_color(hex).unwrap_or(Color::TRANSPARENT);
    let swatch = button(Space::new().width(22).height(22))
        .on_press(Message::ThemeEditorOpenPicker(slot))
        .padding(0)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(swatch_color)),
            border: Border {
                radius: Radius::from(5.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });
    dir_row(vec![
        text(label)
            .size(13)
            .color(OryxisColors::t().text_secondary)
            .width(Length::Fixed(100.0))
            .into(),
        swatch.into(),
        Space::new().width(10).into(),
        text_input("#RRGGBB", hex)
            .on_input(move |v| Message::ThemeEditorColorChanged(slot, v))
            .padding(7)
            .size(12)
            .width(Length::Fixed(110.0))
            .style(crate::widgets::rounded_input_style)
            .into(),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

/// Compact preset-color grid inside the picker popover.
fn preset_grid<'a>(slot: ThemeColorSlot) -> Element<'a, Message> {
    let mut rows = column![].spacing(5);
    let mut current = row![].spacing(5);
    let mut n = 0;
    for hex in crate::os_icon::PRESET_COLORS.iter() {
        let color = parse_hex_color(hex).unwrap_or(Color::TRANSPARENT);
        let sw = button(Space::new().width(18).height(18))
            .on_press(Message::ThemeEditorColorChanged(slot, (*hex).to_string()))
            .padding(0)
            .style(move |_, status| {
                let border = match status {
                    button::Status::Hovered => OryxisColors::t().text_primary,
                    _ => OryxisColors::t().border,
                };
                button::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: Radius::from(4.0), color: border, width: 1.0 },
                    ..Default::default()
                }
            });
        current = current.push(sw);
        n += 1;
        if n % 9 == 0 {
            rows = rows.push(current);
            current = row![].spacing(5);
        }
    }
    if n % 9 != 0 {
        rows = rows.push(current);
    }
    rows.into()
}

/// Live preview: a sample terminal box painted with the in-progress colors.
fn theme_preview<'a>(form: &'a crate::state::ThemeEditorForm) -> Element<'a, Message> {
    let bg = parse_hex_color(&form.background).unwrap_or(Color::BLACK);
    let fg = parse_hex_color(&form.foreground).unwrap_or(Color::WHITE);
    let green = parse_hex_color(&form.ansi[2]).unwrap_or(fg);
    let blue = parse_hex_color(&form.ansi[4]).unwrap_or(fg);
    let yellow = parse_hex_color(&form.ansi[3]).unwrap_or(fg);
    let red = parse_hex_color(&form.ansi[1]).unwrap_or(fg);

    let mono = iced::Font::MONOSPACE;
    let line = |s: &'a str, c: Color| text(s).size(12).font(mono).color(c);

    let swatch_strip: Vec<Element<'a, Message>> = (0..16usize)
        .map(|i| {
            let c = parse_hex_color(&form.ansi[i]).unwrap_or(Color::TRANSPARENT);
            container(Space::new().width(14).height(14))
                .style(move |_| container::Style {
                    background: Some(Background::Color(c)),
                    border: Border { radius: Radius::from(3.0), ..Default::default() },
                    ..Default::default()
                })
                .into()
        })
        .collect();

    container(
        column![
            dir_row(vec![
                line("user", green).into(),
                line("@host", blue).into(),
                line(":~$ ", fg).into(),
                line("git status", fg).into(),
            ]),
            line("On branch main", fg),
            dir_row(vec![
                line("  modified: ", fg).into(),
                line("src/main.rs", red).into(),
            ]),
            dir_row(vec![
                line("  new file: ", fg).into(),
                line("themes.rs", green).into(),
            ]),
            line("warning: 2 files changed", yellow),
            Space::new().height(12),
            row(swatch_strip).spacing(4),
        ]
        .spacing(4),
    )
    .padding(16)
    .width(Length::Fixed(300.0))
    .style(move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    })
    .into()
}

fn outline_btn_style(status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => OryxisColors::t().bg_hover,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    }
}
