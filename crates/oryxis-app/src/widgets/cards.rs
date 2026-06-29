//! UI helper widgets: cards. Split out of widgets/mod.rs.

use super::*;
/// Soft left-to-right accent wash on a card: the card's own colour
/// (host brand / group / key / snippet colour) at low alpha on the left
/// edge fading to transparent across the card. The colour is first toned
/// toward the surface (darkened on dark themes, lightened on light ones)
/// so a vivid brand colour blends instead of glaring. No border of its
/// own (the card keeps its own); just the gradient. Overlaid via a
/// `Stack`, rounded to match so it doesn't square off the corners.
/// Shared across the dashboard and every internal card list, gated by
/// the `setting_card_accent_glass` toggle at the call sites.
pub(crate) fn card_accent_wash<'a>(card: Element<'a, Message>, color: Color) -> Element<'a, Message> {
    let bg = OryxisColors::t().bg_surface;
    let tinted = crate::theme::tone_toward_surface(color, bg, 0.4);
    let wash = container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(move |_| container::Style {
            background: Some(Background::Gradient(iced::Gradient::Linear(
                // Angle points toward stop 1 (right), so stop 0 is the
                // left edge: colour → transparent, left to right.
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                    .add_stop(0.0, Color { a: 0.20, ..tinted })
                    .add_stop(0.6, Color { a: 0.0, ..tinted }),
            ))),
            border: Border {
                radius: Radius::from(10.0),
                ..Default::default()
            },
            ..Default::default()
        });
    Stack::new().push(card).push(wash).into()
}

/// Centered empty-state block: a rounded `bg_surface` icon tile, a
/// 20px title, a 13px muted description, and an optional CTA button
/// (`cta_button`). Centered in the available space. The shared template
/// behind every "nothing here yet" screen (hosts, keychain, snippets,
/// port forwards, cloud, proxies, known hosts, history). Pass the icon
/// pre-sized/coloured (e.g. `lucide::route().size(32).color(...).into()`).
pub(crate) fn empty_state<'a>(
    icon: Element<'a, Message>,
    title: String,
    desc: String,
    cta: Option<(String, Message)>,
) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = vec![
        // Fixed square box with the glyph centered. Padding-only sizing
        // tracked the glyph's own width/height (rarely equal), so the box
        // came out slightly oblong; a fixed 64x64 keeps it square on every
        // empty state regardless of which icon it holds.
        container(icon)
            .center_x(Length::Fixed(64.0))
            .center_y(Length::Fixed(64.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(12.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
        Space::new().height(20).into(),
        text(title)
            .size(20)
            .color(OryxisColors::t().text_primary)
            .into(),
        Space::new().height(8).into(),
        text(desc)
            .size(13)
            .color(OryxisColors::t().text_muted)
            .align_x(iced::alignment::Horizontal::Center)
            .into(),
    ];
    if let Some((label, msg)) = cta {
        items.push(Space::new().height(24).into());
        items.push(cta_button(label, msg));
    }
    container(
        iced::widget::Column::with_children(items).align_x(iced::Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}

/// Visual swatch card for a terminal palette. Renders the theme's
/// background as the card fill, the theme name in the foreground
/// color, and a strip of the six main ANSI colors so the user can
/// compare palettes without having to apply each one.
/// Shared theme-card chassis used by both the terminal and UI theme
/// pickers, so they're pixel-identical: a card painted with `bg`, the name
/// in `fg`, and `dots` (representative palette colors) on the trailing edge.
/// Selected gets a 2px accent border; hover lightens the fill.
pub(crate) fn theme_preview_card<'a>(
    name: &str,
    bg: Color,
    fg: Color,
    dots: Vec<Color>,
    selected: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let dot_els: Vec<Element<'a, Message>> = dots
        .into_iter()
        .map(|color| {
            container(Space::new().width(Length::Fixed(12.0)).height(Length::Fixed(12.0)))
                .style(move |_| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                })
                .into()
        })
        .collect();

    let body = dir_row(vec![
        text(name.to_owned()).size(13).color(fg).into(),
        Space::new().width(Length::Fill).into(),
        Row::with_children(dot_els).spacing(4).into(),
    ])
    .align_y(iced::Alignment::Center);

    let border_color = if selected { OryxisColors::t().accent } else { Color::TRANSPARENT };
    let border_width = if selected { 2.0 } else { 0.0 };

    button(
        container(body)
            .padding(Padding { top: 12.0, right: 14.0, bottom: 12.0, left: 14.0 })
            .height(Length::Fixed(THEME_CARD_HEIGHT))
            .align_y(iced::alignment::Vertical::Center)
            .width(Length::Fill),
    )
    .on_press(on_press)
    .padding(0)
    .width(Length::Fill)
    .style(move |_, status| {
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
            border: Border { radius: Radius::from(8.0), color: border_color, width: border_width },
            ..Default::default()
        }
    })
    .into()
}

/// Fixed card height shared by theme cards and the "+ / Import" cards so
/// every cell in the grid lines up.
pub(crate) const THEME_CARD_HEIGHT: f32 = 44.0;

/// Outline "action" card (+ New theme, Import) the same height as a theme
/// card, so the grid stays uniform.
pub(crate) fn theme_outline_card<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    fg: Color,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        container(
            dir_row(vec![
                icon.size(14).color(fg).into(),
                Space::new().width(8).into(),
                text(label).size(13).color(fg).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fixed(THEME_CARD_HEIGHT)),
    )
    .on_press(on_press)
    .padding(0)
    .width(Length::Fill)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
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

pub(crate) fn terminal_theme_card<'a>(
    palette: oryxis_terminal::TerminalPalette,
    name: &str,
    selected: bool,
    on_press: Message,
) -> Element<'a, Message> {
    // ANSI red → cyan (skip black/white, they barely read against the bg).
    let dots: Vec<Color> = [1usize, 2, 3, 4, 5, 6].iter().map(|&i| palette.ansi[i]).collect();
    theme_preview_card(name, palette.background, palette.foreground, dots, selected, on_press)
}
