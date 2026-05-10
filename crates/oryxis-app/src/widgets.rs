//! Free-standing UI helper widgets used across views.
//!
//! Each helper is a `pub(crate) fn` returning an `Element<'_, Message>`. None of
//! them borrow from the top-level `Oryxis` struct — keeping them here lets view
//! modules compose these building blocks without polluting the state machine file.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, pick_list, text, text_input, Row, Space};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::app::Message;
use crate::state::View;
use crate::theme::OryxisColors;

/// Corner radius used for text inputs and pick lists across the UI.
/// Bumped from the iced default (~2 px) so form controls feel modern and
/// match the rounded look of the cards and buttons.
pub const INPUT_RADIUS: f32 = 10.0;

/// Build a `Row` from elements written in left-to-right *reading order*,
/// reversing them when the active layout direction is RTL. Use anywhere the
/// physical placement of children should mirror with the layout setting —
/// e.g. sidebar vs. content, leading/trailing icon pairs.
///
/// The `iced::widget::row!` macro takes positional children and can't be
/// reversed after construction, so callers that need direction-awareness
/// should switch to this helper instead.
pub fn dir_row<'a, M: 'a>(items: Vec<Element<'a, M>>) -> Row<'a, M> {
    if crate::i18n::is_rtl_layout() {
        Row::with_children(items.into_iter().rev().collect::<Vec<_>>())
    } else {
        Row::with_children(items)
    }
}

/// Horizontal alignment for content that should hug the *leading* edge —
/// `Left` under LTR, `Right` under RTL. Use on `Column::align_x`,
/// `Container::align_x`, or `text(...).align_x(...)` inside `Length::Fill`
/// regions where children would otherwise glue to the physical left edge.
pub fn dir_align_x() -> iced::alignment::Horizontal {
    if crate::i18n::is_rtl_layout() {
        iced::alignment::Horizontal::Right
    } else {
        iced::alignment::Horizontal::Left
    }
}

/// Pick a column count for a card grid given the available content width.
/// Floor-divides slack by `min_card_width + h_gap`, clamped to `>= 1`.
/// Callers compute `available_width` from `window_size` minus the visible
/// chrome (left sidebar, optional right panel, padding).
pub fn card_grid_columns(available_width: f32, min_card_width: f32, h_gap: f32) -> usize {
    if available_width <= 0.0 || min_card_width <= 0.0 {
        return 1;
    }
    let n = ((available_width + h_gap) / (min_card_width + h_gap)).floor() as usize;
    n.max(1)
}

/// Distribute pre-built cards into rows of `cols` cards each. Cards must be
/// built with `Length::Fill` width so the row evenly divides the slack;
/// partial last rows are padded with invisible fillers so the trailing
/// card keeps the same per-card width as the full rows above.
///
/// Honours the active layout direction via `dir_row` — under RTL each
/// row's children are reversed, but the row order (top-to-bottom) stays
/// the same.
pub fn distribute_card_grid<'a, M: 'a>(
    cards: Vec<Element<'a, M>>,
    cols: usize,
    h_gap: f32,
    v_gap: f32,
) -> Element<'a, M> {
    use iced::widget::column;

    if cards.is_empty() {
        return Space::new().height(0).into();
    }
    let cols = cols.max(1);
    let mut grid_rows: Vec<Element<'a, M>> = Vec::new();
    let mut row_buf: Vec<Element<'a, M>> = Vec::with_capacity(cols);
    let total = cards.len();

    for (i, card) in cards.into_iter().enumerate() {
        row_buf.push(card);
        if row_buf.len() == cols {
            grid_rows.push(dir_row(std::mem::take(&mut row_buf)).spacing(h_gap).into());
            if i + 1 < total {
                grid_rows.push(Space::new().height(v_gap).into());
            }
        }
    }
    if !row_buf.is_empty() {
        while row_buf.len() < cols {
            row_buf.push(Space::new().width(Length::Fill).into());
        }
        grid_rows.push(dir_row(row_buf).spacing(h_gap).into());
    }
    column(grid_rows).width(Length::Fill).into()
}


/// Shared style closure for `text_input`. Apply via `.style(rounded_input_style)`
/// to get the app's accent-focused look with the consistent 10 px radius.
pub fn rounded_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let c = OryxisColors::t();
    let (border_color, border_width) = match status {
        text_input::Status::Focused { .. } => (c.accent, 1.5),
        text_input::Status::Disabled => (c.border, 1.0),
        _ => (c.border, 1.0),
    };
    text_input::Style {
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(INPUT_RADIUS),
            width: border_width,
            color: border_color,
        },
        icon: c.text_muted,
        placeholder: c.text_muted,
        value: c.text_primary,
        selection: c.accent,
    }
}

/// Shared style closure for `pick_list` — matches `rounded_input_style` so
/// selects and inputs sit side-by-side with the same geometry.
pub fn rounded_pick_list_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let c = OryxisColors::t();
    let border_color = match status {
        pick_list::Status::Opened { .. } => c.accent,
        pick_list::Status::Hovered => c.accent_hover,
        _ => c.border,
    };
    pick_list::Style {
        text_color: c.text_primary,
        placeholder_color: c.text_muted,
        handle_color: c.text_muted,
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(INPUT_RADIUS),
            width: 1.0,
            color: border_color,
        },
    }
}

pub(crate) fn sidebar_nav_btn<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    view: View,
    is_active: bool,
) -> Element<'a, Message> {
    let bg = if is_active {
        Color { a: 0.15, ..OryxisColors::t().accent }
    } else {
        Color::TRANSPARENT
    };
    let fg = if is_active {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_secondary
    };

    container(
        button(
            container(
                iced::widget::row![
                    icon_widget.size(14).color(fg),
                    Space::new().width(10),
                    text(label).size(13).color(fg),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 8.0, left: 16.0 }),
        )
        .on_press(Message::ChangeView(view))
        .width(Length::Fill)
        .style(move |_, status| {
            let hover_bg = match status {
                BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                BtnStatus::Pressed => Color { a: 0.25, ..OryxisColors::t().accent },
                _ => bg,
            };
            button::Style {
                background: Some(Background::Color(hover_bg)),
                border: Border { radius: Radius::from(10.0), ..Default::default() },
                ..Default::default()
            }
        }),
    )
    .padding(Padding { top: 1.0, right: 8.0, bottom: 1.0, left: 8.0 })
    .into()
}

/// A section card with slightly lighter background.
pub(crate) fn panel_section<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        })
        .into()
}

/// A labeled form field inside a section.
pub(crate) fn panel_field<'a>(label: &'a str, input: Element<'a, Message>) -> Element<'a, Message> {
    iced::widget::column![
        text(label).size(12).color(OryxisColors::t().text_muted),
        Space::new().height(4),
        input,
    ]
    .into()
}

/// A divider line inside a section.
pub(crate) fn toggle_row<'a>(label: &'a str, value: bool, msg: Message) -> Element<'a, Message> {
    let toggle_bg = if value { OryxisColors::t().success } else { OryxisColors::t().bg_selected };
    let toggle_text = if value { "  \u{25CF}" } else { "\u{25CF}  " };
    dir_row(vec![
        text(label).size(13).color(OryxisColors::t().text_primary).into(),
        Space::new().width(Length::Fill).into(),
        button(text(toggle_text).size(12).color(Color::WHITE))
            .on_press(msg)
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(move |_, _| button::Style {
                background: Some(Background::Color(toggle_bg)),
                border: Border { radius: Radius::from(10.0), ..Default::default() },
                ..Default::default()
            }).into(),
    ]).align_y(iced::Alignment::Center)
    .into()
}

pub(crate) fn panel_divider<'a>() -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().border)),
            ..Default::default()
        })
        .into()
}

/// An option row: [icon] [label] ... [value]
pub(crate) fn panel_option_row<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    value: String,
) -> Element<'a, Message> {
    container(
        dir_row(vec![
            icon_widget.size(13).color(OryxisColors::t().text_muted).into(),
            Space::new().width(10).into(),
            text(label).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            text(value).size(12).color(OryxisColors::t().text_muted).into(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 8.0, right: 0.0, bottom: 8.0, left: 0.0 })
    .into()
}

pub(crate) fn context_menu_item<'a>(
    icon: impl Into<crate::os_icon::BrandIcon>,
    label: &'a str,
    msg: Message,
    color: Color,
) -> Element<'a, Message> {
    button(
        dir_row(vec![
            icon.into().view(14.0, color),
            Space::new().width(8).into(),
            text(label).size(12).color(OryxisColors::t().text_primary).into(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .width(Length::Fill)
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// An option row with a pick_list for selection.
pub(crate) fn panel_option_pick<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    options: Vec<String>,
    selected: String,
    on_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    container(
        dir_row(vec![
            icon_widget.size(13).color(OryxisColors::t().text_muted).into(),
            Space::new().width(10).into(),
            text(label).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            pick_list(Some(selected), options, |s: &String| s.clone()).on_select(on_change).width(120).padding(10).style(rounded_pick_list_style).into(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
    .into()
}

/// An option row with pick_list for jump host.
pub(crate) fn panel_option_pick_jump<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    options: Vec<String>,
    selected: String,
    on_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    container(
        dir_row(vec![
            icon_widget.size(13).color(OryxisColors::t().text_muted).into(),
            Space::new().width(10).into(),
            text(label).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            pick_list(Some(selected), options, |s: &String| s.clone()).on_select(on_change).width(140).padding(10).style(rounded_pick_list_style).into(),
        ])
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
    .into()
}

pub(crate) fn settings_row<'a>(label: &'static str, value: String) -> Element<'a, Message> {
    container(
        iced::widget::row![
            text(label).size(13).color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            text(value).size(13).color(OryxisColors::t().text_primary),
        ],
    )
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .width(300)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    })
    .into()
}

/// Same shape as `settings_row`, but the value text renders in the
/// accent color and a click anywhere on the row dispatches
/// `Message::OpenUrl(url)` so the OS default browser opens it. Used in
/// the About panel for the GitHub line.
pub(crate) fn settings_row_link<'a>(
    label: &'a str,
    display: String,
    url: String,
) -> Element<'a, Message> {
    let body = container(
        iced::widget::row![
            text(label.to_owned())
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            text(display).size(13).color(OryxisColors::t().accent),
        ],
    )
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .width(300)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border { radius: Radius::from(6.0), ..Default::default() },
        ..Default::default()
    });
    iced::widget::MouseArea::new(body)
        .on_press(Message::OpenUrl(url))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// Wide call-to-action button — Semibold label, theme-defined
/// `button_bg` / `button_text` pair, fixed 380-wide / 8 px radius.
/// Used for empty-state primary actions on Keys / Snippets and
/// anywhere else we want the same prominent affordance.
pub(crate) fn cta_button<'a>(label: String, msg: Message) -> Element<'a, Message> {
    let fg = OryxisColors::t().button_text;
    button(
        container(
            text(label)
                .size(14)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(fg),
        )
        .padding(Padding { top: 12.0, right: 0.0, bottom: 12.0, left: 0.0 })
        .width(380)
        .center_x(380),
    )
    .on_press(msg)
    .width(380)
    .style(|_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => OryxisColors::t().button_bg_hover,
            _ => OryxisColors::t().button_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Primary styled button — bold Inter, compact vertical padding, wide
/// horizontal padding. Used for Connect / Save / Cancel / destructive actions.
/// On hover the background lightens; keeps consistent language with split
/// buttons elsewhere (+ HOST, + ADD).
pub(crate) fn styled_button(label: &str, msg: Message, color: Color) -> Element<'_, Message> {
    // Accent-colored CTAs share the per-theme `button_text` pairing so
    // every primary button (here, `+ HOST`, `+ ADD`, `New Snippet`,
    // etc.) renders in the same text color across the app. Non-accent
    // call sites (Cancel on bg_hover, Destroy on error, …) still
    // auto-pick via the luminance heuristic.
    let fg = if color == OryxisColors::t().accent {
        OryxisColors::t().button_text
    } else {
        crate::theme::contrast_text_for(color)
    };
    button(
        container(
            text(label.to_owned()).size(12).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(fg),
        )
        .padding(Padding { top: 5.0, right: 18.0, bottom: 5.0, left: 18.0 }),
    )
    .on_press(msg)
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color {
                a: 1.0,
                r: (color.r + 0.05).min(1.0),
                g: (color.g + 0.05).min(1.0),
                b: (color.b + 0.05).min(1.0),
            },
            _ => color,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

pub(crate) fn key_badge<'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(11).color(OryxisColors::t().text_primary))
        .padding(Padding { top: 3.0, right: 6.0, bottom: 3.0, left: 6.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        })
        .into()
}

pub(crate) fn shortcut_row<'a>(keys: Vec<Element<'a, Message>>, action: &'a str) -> Element<'a, Message> {
    iced::widget::row![
        Row::with_children(keys).spacing(4).width(200),
        text(action).size(13).color(OryxisColors::t().text_secondary),
    ].align_y(iced::Alignment::Center).into()
}

/// Visual swatch card for a terminal palette. Renders the theme's
/// background as the card fill, the theme name in the foreground
/// color, and a strip of the six main ANSI colors so the user can
/// compare palettes without having to apply each one.
pub(crate) fn terminal_theme_card<'a>(
    theme: oryxis_terminal::TerminalTheme,
    selected: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let palette = theme.palette();
    let bg = palette.background;
    let fg = palette.foreground;
    let name = theme.name();

    // Render ANSI red → cyan (skip black/white because they barely
    // read against the background).
    let dot_indices: [usize; 6] = [1, 2, 3, 4, 5, 6];
    let dots: Vec<Element<'_, Message>> = dot_indices
        .iter()
        .map(|&i| {
            let color = palette.ansi[i];
            container(
                Space::new()
                    .width(Length::Fixed(12.0))
                    .height(Length::Fixed(12.0)),
            )
            .style(move |_| container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: Radius::from(6.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        })
        .collect();

    let body = dir_row(vec![
        text(name).size(13).color(fg).into(),
        Space::new().width(Length::Fill).into(),
        Row::with_children(dots).spacing(4).into(),
    ])
    .align_y(iced::Alignment::Center);

    let border_color = if selected {
        OryxisColors::t().accent
    } else {
        Color::TRANSPARENT
    };
    let border_width = if selected { 2.0 } else { 0.0 };

    button(
        container(body)
            .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
            .width(Length::Fill),
    )
    .on_press(on_press)
    .padding(0)
    .width(Length::Fill)
    .style(move |_, status| {
        // Slight lighten on hover, otherwise the theme bg is the card
        // fill. The hover blend uses an inverted overlay so dark
        // themes get a subtle highlight without breaking the preview.
        let card_bg = match status {
            BtnStatus::Hovered => Color {
                a: bg.a,
                r: (bg.r + 0.05).min(1.0),
                g: (bg.g + 0.05).min(1.0),
                b: (bg.b + 0.05).min(1.0),
            },
            _ => bg,
        };
        button::Style {
            background: Some(Background::Color(card_bg)),
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

/// Companion to `terminal_theme_card` for the "no override" sentinel
/// row that sits at the top of every theme picker. Uses the app's
/// surface color rather than a palette so it doesn't pretend to be a
/// theme of its own.
pub(crate) fn terminal_theme_inherit_card<'a>(
    label: &'a str,
    selected: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let border_color = if selected {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().border
    };
    let border_width = if selected { 2.0 } else { 1.0 };

    button(
        container(
            text(label.to_owned())
                .size(13)
                .color(OryxisColors::t().text_primary),
        )
        .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
        .width(Length::Fill),
    )
    .on_press(on_press)
    .padding(0)
    .width(Length::Fill)
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
