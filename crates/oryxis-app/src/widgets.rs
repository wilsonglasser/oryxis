//! Free-standing UI helper widgets used across views.
//!
//! Each helper is a `pub(crate) fn` returning an `Element<'_, Message>`. None of
//! them borrow from the top-level `Oryxis` struct, keeping them here lets view
//! modules compose these building blocks without polluting the state machine file.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, pick_list, text, text_editor, text_input, Row, Space, Stack};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::app::Message;
use crate::theme::OryxisColors;

/// Corner radius used for text inputs and pick lists across the UI.
/// Bumped from the iced default (~2 px) so form controls feel modern and
/// match the rounded look of the cards and buttons.
pub const INPUT_RADIUS: f32 = 10.0;

/// Resolve a `Color` from a `#RRGGBB` hex string. Returns `None` for any
/// other input so callers can fall through to the global accent.
pub(crate) fn parse_hex_color(s: &str) -> Option<Color> {
    let trimmed = s.trim_start_matches('#');
    if trimmed.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&trimmed[0..2], 16).ok()?;
    let g = u8::from_str_radix(&trimmed[2..4], 16).ok()?;
    let b = u8::from_str_radix(&trimmed[4..6], 16).ok()?;
    Some(Color::from_rgb8(r, g, b))
}

/// Effective host icon style: resolve the per-host override, fall back to
/// the global default, then default to "circular" if both are missing
/// or contain an unknown value.
pub(crate) fn resolve_host_icon_style(per_host: Option<&str>, global: &str) -> HostIconStyle {
    let candidate = per_host.unwrap_or(global);
    match candidate {
        "square" => HostIconStyle::Square,
        "rounded" => HostIconStyle::Rounded,
        "outline" => HostIconStyle::Outline,
        "initials" => HostIconStyle::Initials,
        _ => HostIconStyle::Circular,
    }
}

/// Host icon shape, resolved by `resolve_host_icon_style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostIconStyle {
    Circular,
    /// Sharp-cornered square (radius 0). The earlier "square" value
    /// was actually rounded, which is now `Rounded` below.
    Square,
    /// Soft-cornered square (~25 % radius). This was the original
    /// `Square` rendering before user feedback split the two.
    Rounded,
    Outline,
    Initials,
}

/// Render a host badge in the chosen style. The badge is a fixed
/// `size x size` square; the inner geometry adapts to `style`:
///
/// - `Circular`: filled disc with the glyph centered (radius = size/2)
/// - `Square`: filled rounded square, same shape as the OS badge in
///   tab strips (compatibility look)
/// - `Outline`: transparent fill with a 1.5 px colored border + glyph
///   in the border color
/// - `Initials`: filled disc with the first one or two characters of
///   `label` instead of the OS glyph, using a contrasting foreground
///
/// `color` is the badge background / outline color (typically the
/// resolved per-host accent). `label` is the source for the initials
/// when style == Initials; for the other styles a caller supplies an
/// `Element` glyph via `glyph` (e.g. an OS lucide icon). Pass `None`
/// for `glyph` to render a blank circle when no OS could be detected.
pub(crate) fn host_icon<'a>(
    style: HostIconStyle,
    color: Color,
    label: &str,
    glyph: Option<Element<'a, Message>>,
    size: f32,
) -> Element<'a, Message> {
    let half = size / 2.0;
    match style {
        HostIconStyle::Circular | HostIconStyle::Square | HostIconStyle::Rounded => {
            let radius = match style {
                HostIconStyle::Circular => half,
                HostIconStyle::Square => 0.0,
                HostIconStyle::Rounded => size * 0.25,
                _ => 0.0,
            };
            let inner: Element<'a, Message> = glyph
                .unwrap_or_else(|| Space::new().width(0).height(0).into());
            container(inner)
                .center_x(Length::Fixed(size))
                .center_y(Length::Fixed(size))
                .style(move |_| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: Radius::from(radius), ..Default::default() },
                    ..Default::default()
                })
                .into()
        }
        HostIconStyle::Outline => {
            let inner: Element<'a, Message> = glyph
                .unwrap_or_else(|| Space::new().width(0).height(0).into());
            container(inner)
                .center_x(Length::Fixed(size))
                .center_y(Length::Fixed(size))
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border {
                        radius: Radius::from(half),
                        color,
                        width: 1.5,
                    },
                    ..Default::default()
                })
                .into()
        }
        HostIconStyle::Initials => {
            // Take up to two leading alphanumeric chars, uppercased.
            // "Saúde e Vida" -> "SE", "api-prod-1" -> "AP", "x" -> "X".
            let initials: String = label
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .take(2)
                .filter_map(|w| w.chars().next())
                .map(|c| c.to_ascii_uppercase())
                .collect();
            let display = if initials.is_empty() {
                "?".to_string()
            } else {
                initials
            };
            // Pick a foreground that reads against the filled color.
            // Cheap luminance heuristic: dark backgrounds get white,
            // light backgrounds get the app's text_primary.
            let lum = 0.299 * color.r + 0.587 * color.g + 0.114 * color.b;
            let fg = if lum < 0.55 {
                Color::WHITE
            } else {
                OryxisColors::t().text_primary
            };
            container(text(display).size((size * 0.45).max(8.0)).color(fg))
                .center_x(Length::Fixed(size))
                .center_y(Length::Fixed(size))
                .style(move |_| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: Radius::from(half), ..Default::default() },
                    ..Default::default()
                })
                .into()
        }
    }
}

/// Build a `Row` from elements written in left-to-right *reading order*,
/// reversing them when the active layout direction is RTL. Use anywhere the
/// physical placement of children should mirror with the layout setting
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

/// Horizontal alignment for content that should hug the *leading* edge
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
/// Honours the active layout direction via `dir_row`, under RTL each
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

/// Shared menu style for `combo_box` dropdowns. Matches the app's
/// surface + border palette so the native overlay reads like the rest
/// of the popovers. Apply via `.menu_style(combo_menu_style)`.
pub fn combo_menu_style(_theme: &Theme) -> iced::widget::overlay::menu::Style {
    let c = OryxisColors::t();
    iced::widget::overlay::menu::Style {
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(8.0),
            color: c.border,
            width: 1.0,
        },
        text_color: c.text_primary,
        selected_text_color: c.text_primary,
        selected_background: Background::Color(c.bg_hover),
        shadow: iced::Shadow::default(),
    }
}

/// Shared style closure for `text_editor` (multi-line). Mirrors
/// `rounded_input_style` so single-line and multi-line fields look identical:
/// same surface, border, radius, and accent-on-focus.
pub fn rounded_editor_style(_theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let c = OryxisColors::t();
    let (border_color, border_width) = match status {
        text_editor::Status::Focused { .. } => (c.accent, 1.5),
        _ => (c.border, 1.0),
    };
    text_editor::Style {
        background: Background::Color(c.bg_surface),
        border: Border {
            radius: Radius::from(INPUT_RADIUS),
            width: border_width,
            color: border_color,
        },
        placeholder: c.text_muted,
        value: c.text_primary,
        selection: c.accent,
    }
}

/// Password text-input with a Lucide eye toggle overlaid inside the rounded
/// border. The input reserves trailing padding for the icon (right under LTR,
/// left under RTL); the button lives in a `Stack` above the input,
/// leading-edge-anchored on the trailing side. Hit-testing is constrained to
/// the button's bounding box, so clicks on the rest of the field still focus
/// the input. `inner_padding` controls vertical and leading-edge inset (12
/// for the vault hero field, 10 for inline form rows).
pub(crate) fn password_input_with_eye<'a, F>(
    placeholder: &'a str,
    value: &'a str,
    on_input: F,
    on_submit: Option<Message>,
    visible: bool,
    on_toggle: Message,
    inner_padding: f32,
) -> Element<'a, Message>
where
    F: Fn(String) -> Message + 'a,
{
    let rtl = crate::i18n::is_rtl_layout();
    // Reserve ~32 px on the trailing edge so the eye icon doesn't overlap
    // typed text. Leading edge keeps the requested inner padding.
    let trailing = 32.0;
    let (pad_left, pad_right) = if rtl {
        (trailing, inner_padding)
    } else {
        (inner_padding, trailing)
    };
    let mut field = text_input(placeholder, value)
        .on_input(on_input)
        .secure(!visible)
        .align_x(dir_align_x())
        .padding(Padding {
            top: inner_padding,
            right: pad_right,
            bottom: inner_padding,
            left: pad_left,
        })
        .width(Length::Fill)
        .style(rounded_input_style);
    if let Some(submit) = on_submit {
        field = field.on_submit(submit);
    }

    let icon = if visible {
        iced_fonts::lucide::eye_off()
    } else {
        iced_fonts::lucide::eye()
    }
    .size(14)
    .color(OryxisColors::t().text_muted);

    let toggle = button(icon)
        .on_press(on_toggle)
        .style(|_t, _s| button::Style::default())
        .padding(4);

    let (align, overlay_pad) = if rtl {
        (
            iced::alignment::Horizontal::Left,
            Padding { left: 2.0, ..Padding::ZERO },
        )
    } else {
        (
            iced::alignment::Horizontal::Right,
            Padding { right: 2.0, ..Padding::ZERO },
        )
    };

    let toggle_overlay = container(toggle)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(align)
        .align_y(iced::alignment::Vertical::Center)
        .padding(overlay_pad);

    Stack::new()
        .push(field)
        .push(toggle_overlay)
        .width(Length::Fill)
        .into()
}

/// Shared style closure for `pick_list`, matches `rounded_input_style` so
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

/// A section card with slightly lighter background. Children are aligned to
/// the leading edge so labels, descriptions, and inline widgets hug the
/// right side under RTL instead of pinning to physical left.
pub(crate) fn panel_section<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(content.width(Length::Fill).align_x(dir_align_x()))
        .padding(16)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_hover)),
            border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        })
        .into()
}

/// A labeled form field inside a section. Column aligned to the leading
/// edge so labels and inputs hug the right side under RTL.
pub(crate) fn panel_field<'a>(label: &'a str, input: Element<'a, Message>) -> Element<'a, Message> {
    iced::widget::column![
        text(label).size(12).color(OryxisColors::t().text_muted),
        Space::new().height(4),
        input,
    ]
    .width(Length::Fill)
    .align_x(dir_align_x())
    .into()
}

/// The canonical on/off control: a small pill that fills with the
/// success color and the dot trailing when on, muted with the dot
/// leading when off. Every toggle in the app (settings rows, plugin
/// auto-update) renders this same switch so the affordance is
/// consistent. `msg` is dispatched on click; callers that track the
/// next state explicitly pass it pre-flipped.
pub(crate) fn toggle_switch<'a>(value: bool, msg: Message) -> Element<'a, Message> {
    let toggle_bg = if value { OryxisColors::t().success } else { OryxisColors::t().bg_selected };
    let toggle_text = if value { "  \u{25CF}" } else { "\u{25CF}  " };
    button(text(toggle_text).size(12).color(Color::WHITE))
        .on_press(msg)
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .style(move |_, _| button::Style {
            background: Some(Background::Color(toggle_bg)),
            border: Border { radius: Radius::from(10.0), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// Inline label + [`toggle_switch`], for compact placements (e.g.
/// plugin auto-update) where the control sits next to its label rather
/// than across a full-width row like [`toggle_row`].
pub(crate) fn toggle_switch_labeled<'a>(
    label: &'a str,
    value: bool,
    msg: Message,
) -> Element<'a, Message> {
    dir_row(vec![
        text(label).size(11).color(OryxisColors::t().text_secondary).into(),
        Space::new().width(8).into(),
        toggle_switch(value, msg),
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

/// A full-width settings row: label on the leading edge, [`toggle_switch`]
/// on the trailing edge.
pub(crate) fn toggle_row<'a>(label: &'a str, value: bool, msg: Message) -> Element<'a, Message> {
    dir_row(vec![
        text(label).size(13).color(OryxisColors::t().text_primary).into(),
        Space::new().width(Length::Fill).into(),
        toggle_switch(value, msg),
    ]).align_y(iced::Alignment::Center)
    .into()
}

/// Like [`toggle_row`] but with a muted description line under the
/// label. The toggle stays vertically centered against the whole
/// label+description block on the trailing edge.
pub(crate) fn toggle_row_desc<'a>(
    label: &'a str,
    desc: &'a str,
    value: bool,
    msg: Message,
) -> Element<'a, Message> {
    dir_row(vec![
        iced::widget::column![
            text(label).size(13).color(OryxisColors::t().text_primary),
            Space::new().height(2),
            text(desc).size(11).color(OryxisColors::t().text_muted),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x())
        .into(),
        Space::new().width(12).into(),
        toggle_switch(value, msg),
    ]).align_y(iced::Alignment::Center)
    .into()
}

/// Small semibold "h2" header used to segregate a settings section
/// into labelled groups (e.g. "General", "Dashboard", "Advanced") so
/// related cards read as a cluster and are easier to locate.
pub(crate) fn settings_group_header<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(12)
        .font(iced::Font {
            weight: iced::font::Weight::Semibold,
            ..iced::Font::DEFAULT
        })
        .color(OryxisColors::t().text_secondary)
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

/// Toolbar trigger button that opens the Sort dropdown. The glyph
/// reflects the active sort so the user can read the current mode
/// without opening the menu (A-z / Z-a / new-first / old-first).
/// Sizing matches the "+ Host" / "+ ADD" buttons (24 px tall) so all
/// toolbar actions share a visual baseline.
pub(crate) fn sort_toolbar_button(
    kind: crate::state::SortMenuKind,
    current: crate::state::ListSort,
) -> Element<'static, Message> {
    use crate::state::ListSort;
    let glyph: iced::widget::Text<'static, iced::Theme, iced::Renderer> = match current {
        ListSort::LabelAsc => iced_fonts::lucide::arrow_down_a_z(),
        ListSort::LabelDesc => iced_fonts::lucide::arrow_down_z_a(),
        ListSort::NewestFirst => iced_fonts::lucide::calendar_arrow_down(),
        ListSort::OldestFirst => iced_fonts::lucide::calendar_arrow_up(),
    };
    button(
        container(
            glyph
                .size(15)
                .color(OryxisColors::t().button_text),
        )
        .center_y(Length::Fixed(24.0))
        .center_x(Length::Fixed(24.0)),
    )
    .on_press(Message::ToggleSortMenu(kind))
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
            _ => OryxisColors::t().button_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Grid/List view toggle for the host dashboard toolbar. Shows the
/// glyph for the CURRENT mode, styled like the sort button.
pub(crate) fn host_view_toggle_button(list_view: bool) -> Element<'static, Message> {
    let glyph: iced::widget::Text<'static, iced::Theme, iced::Renderer> = if list_view {
        iced_fonts::lucide::list()
    } else {
        iced_fonts::lucide::layout_grid()
    };
    button(
        container(
            glyph
                .size(15)
                .color(OryxisColors::t().button_text),
        )
        .center_y(Length::Fixed(24.0))
        .center_x(Length::Fixed(24.0)),
    )
    .on_press(Message::ToggleHostListView)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
            _ => OryxisColors::t().button_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Floating `⋮` kebab action button shown on hover over cards (and the
/// SFTP pane toolbar). Fixed 22×22 with the glyph centered, so the hover
/// highlight is a square with a soft radius instead of the wider-than-tall
/// rectangle a horizontally padded glyph produces. 22 matches the reserved
/// slot widths (`SNIP_DOTS_SLOT_W`, `DG_DOTS_SLOT_W`) so the kebab never
/// shifts layout when it replaces an idle placeholder. `show_hover` gates
/// the highlight: pass `false` while the glyph is transparent (card not
/// hovered) so the square doesn't flash as the pointer crosses the slot.
pub(crate) fn card_kebab_button<'a>(
    glyph_color: Color,
    show_hover: bool,
    on_press: Message,
) -> button::Button<'a, Message> {
    button(
        container(text("\u{22EE}").size(14).color(glyph_color))
            .center_x(Length::Fixed(22.0))
            .center_y(Length::Fixed(22.0)),
    )
    .on_press(on_press)
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered if show_hover => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
}

/// One row of the toolbar Sort dropdown (Hosts / Keychain / Snippets).
/// Mirrors `context_menu_item` but adds a trailing checkmark when the
/// row matches the current sort. Icon is taken pre-built so the
/// caller can pass any `iced_fonts::lucide::*` glyph (their lifetime
/// is `'static`, which keeps the helper monomorphizable without a
/// closure that would force shorter borrows).
pub(crate) fn sort_menu_row(
    kind: crate::state::SortMenuKind,
    sort: crate::state::ListSort,
    icon: iced::widget::Text<'static, iced::Theme, iced::Renderer>,
    label_key: &'static str,
    is_active: bool,
) -> Element<'static, Message> {
    let check: Element<'static, Message> = if is_active {
        iced_fonts::lucide::check()
            .size(13)
            .color(OryxisColors::t().accent)
            .into()
    } else {
        Space::new().width(13).into()
    };
    button(
        container(
            dir_row(vec![
                icon.size(14)
                    .color(OryxisColors::t().text_secondary)
                    .into(),
                Space::new().width(10).into(),
                text(crate::i18n::t(label_key))
                    .size(12)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(Length::Fill).into(),
                check,
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(dir_align_x()),
    )
    .on_press(Message::SetListSort(kind, sort))
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

pub(crate) fn context_menu_item<'a>(
    icon: impl Into<crate::os_icon::BrandIcon>,
    label: &'a str,
    msg: Message,
    color: Color,
) -> Element<'a, Message> {
    button(
        container(
            dir_row(vec![
                icon.into().view(14.0, color),
                Space::new().width(8).into(),
                text(label).size(12).color(OryxisColors::t().text_primary).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(dir_align_x()),
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

pub(crate) fn settings_row<'a>(label: &'static str, value: String) -> Element<'a, Message> {
    // Transparent row inside the surrounding `panel_section` (which
    // already supplies the bg + border + radius). The earlier
    // `bg_surface` fill made these rows render lighter than the
    // panel around them and out of step with the rest of Settings,
    // where panel children sit directly on the panel background.
    container(
        dir_row(vec![
            text(label).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            text(value).size(13).color(OryxisColors::t().text_primary).into(),
        ]),
    )
    .padding(Padding { top: 6.0, right: 4.0, bottom: 6.0, left: 4.0 })
    .width(Length::Fill)
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
        dir_row(vec![
            text(label.to_owned())
                .size(13)
                .color(OryxisColors::t().text_secondary)
                .into(),
            Space::new().width(Length::Fill).into(),
            text(display).size(13).color(OryxisColors::t().accent).into(),
        ]),
    )
    .padding(Padding { top: 6.0, right: 4.0, bottom: 6.0, left: 4.0 })
    .width(Length::Fill);
    iced::widget::MouseArea::new(body)
        .on_press(Message::OpenUrl(url))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// Same shape as `settings_row`, but the whole row is clickable and
/// dispatches an arbitrary message (pointer cursor as the affordance).
/// Used by the About > Vault Statistics rows to jump to each section.
pub(crate) fn settings_row_nav<'a>(
    label: &'a str,
    value: String,
    msg: Message,
) -> Element<'a, Message> {
    let body = container(
        dir_row(vec![
            text(label.to_owned())
                .size(13)
                .color(OryxisColors::t().text_secondary)
                .into(),
            Space::new().width(Length::Fill).into(),
            text(value).size(13).color(OryxisColors::t().text_primary).into(),
        ]),
    )
    .padding(Padding { top: 6.0, right: 4.0, bottom: 6.0, left: 4.0 })
    .width(Length::Fill);
    iced::widget::MouseArea::new(body)
        .on_press(msg)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// Wide call-to-action button, Semibold label, theme-defined
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

/// Primary styled button, bold Inter, compact vertical padding, wide
/// horizontal padding. Used for Connect / Save / Cancel / destructive actions.
/// On hover the background lightens; keeps consistent language with split
/// buttons elsewhere (+ HOST, + ADD).
pub(crate) fn styled_button(label: &str, msg: Message, color: Color) -> Element<'_, Message> {
    styled_button_opt(label, Some(msg), color)
}

/// Like [`styled_button`] but the action is optional. `None` renders a
/// muted, non-interactive button (no `on_press`), so actions that only
/// apply when some state exists (e.g. "Reset hints" with no dismissed
/// hints) can communicate "nothing to do" instead of silently no-oping.
pub(crate) fn styled_button_opt(
    label: &str,
    msg: Option<Message>,
    color: Color,
) -> Element<'_, Message> {
    let enabled = msg.is_some();
    // Accent-colored CTAs share the per-theme `button_text` pairing so
    // every primary button (here, `+ HOST`, `+ ADD`, `New Snippet`,
    // etc.) renders in the same text color across the app. Non-accent
    // call sites (Cancel on bg_hover, Destroy on error, …) still
    // auto-pick via the luminance heuristic.
    let fg = if !enabled {
        OryxisColors::t().text_muted
    } else if color == OryxisColors::t().accent {
        OryxisColors::t().button_text
    } else {
        crate::theme::contrast_text_for(color)
    };
    let disabled_bg = OryxisColors::t().bg_selected;
    let mut b = button(
        container(
            text(label.to_owned()).size(12).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(fg),
        )
        .padding(Padding { top: 5.0, right: 18.0, bottom: 5.0, left: 18.0 }),
    )
    .style(move |_, status| {
        let bg = if !enabled {
            disabled_bg
        } else {
            match status {
                iced::widget::button::Status::Hovered => Color {
                    a: 1.0,
                    r: (color.r + 0.05).min(1.0),
                    g: (color.g + 0.05).min(1.0),
                    b: (color.b + 0.05).min(1.0),
                },
                _ => color,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    b.into()
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
    // Pin the chip cluster to the row's leading edge inside its 200 px slot:
    // LTR aligns left (keys first, gap before the label), RTL aligns right
    // (label first, gap, then keys). dir_row handles the outer reversal,
    // align_x keeps the chips snug against the slot's trailing edge under
    // RTL so the gap sits between keys and label instead of bunching them.
    let keys_box = container(Row::with_children(keys).spacing(4))
        .width(200)
        .align_x(dir_align_x());
    dir_row(vec![
        keys_box.into(),
        text(action).size(13).color(OryxisColors::t().text_secondary).into(),
    ]).align_y(iced::Alignment::Center).into()
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


/// Shared cell type for `bounds_reporter`. Single-threaded
/// (`Rc<Cell<_>>`) is fine for iced's event loop in 0.13; bump to
/// `Arc<AtomicRefCell<_>>` if iced ever multithreads the layout pass.
pub(crate) type BoundsCell = std::rc::Rc<std::cell::Cell<iced::Rectangle>>;

/// Build a fresh, zeroed `BoundsCell` ready to be cloned into a
/// `bounds_reporter` and held in app state for later reads.
pub(crate) fn new_bounds_cell() -> BoundsCell {
    std::rc::Rc::new(std::cell::Cell::new(iced::Rectangle::new(
        iced::Point::ORIGIN,
        iced::Size::ZERO,
    )))
}

/// Wraps `content` and writes the laid-out screen-space bounds to
/// `cell` on every `draw` pass. Lets other code (typically context-
/// menu anchor logic) read the widget's on-screen rect synchronously
/// instead of going through the async `Operation` round-trip. Cell
/// value reflects the LAST rendered frame, which is what every
/// popover/anchor flow wants anyway. Everything except `draw`
/// delegates straight to the inner widget, so behaviour is otherwise
/// identical to the unwrapped child.
pub(crate) fn bounds_reporter<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    cell: BoundsCell,
) -> Element<'a, Message> {
    use iced::advanced::widget::{tree, Operation, Tree, Widget};
    use iced::advanced::{layout, mouse, overlay, renderer, Layout, Shell};
    use iced::{Event, Length as L, Rectangle, Size, Vector};

    struct BoundsReporter<'a, Message> {
        content: Element<'a, Message>,
        cell: BoundsCell,
    }

    impl<Message> Widget<Message, Theme, iced::Renderer> for BoundsReporter<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }
        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }
        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }
        fn diff(&self, tree: &mut Tree) {
            self.content.as_widget().diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.as_widget().size()
        }
        fn size_hint(&self) -> Size<L> {
            self.content.as_widget().size_hint()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content
                .as_widget_mut()
                .layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            // Draw runs after final positioning, so `layout.bounds()`
            // here is the screen-space rect (offset by parent
            // translations). Cache it so anchor lookups in `update`
            // hit the correct on-screen coordinates.
            self.cell.set(layout.bounds());
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, shell, viewport,
            );
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content.as_widget_mut().overlay(
                tree,
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }

    Element::new(BoundsReporter {
        content: content.into(),
        cell,
    })
}

/// Wraps `content` (a terminal pane canvas) and, while `enabled` is true,
/// asks the runtime to turn the OS IME on for this surface. The terminal is
/// an `iced` canvas, not a `text_input`, so nothing in its widget tree ever
/// requests an input method, and winit defaults `set_ime_allowed(false)`,
/// which locks the IME to direct (English) mode and blocks CJK composition.
/// This decorator closes that gap: every other behaviour delegates straight
/// to the inner widget, so it is transparent apart from the IME request.
///
/// The request is only honoured by the shell during a `RedrawRequested`
/// frame, so we issue it there. Only the focused pane (`enabled`) drives the
/// IME, so split panes don't fight over the cursor area. The committed text
/// itself arrives as `Event::InputMethod(Commit(..))` and is routed to the
/// PTY in `subscription.rs` / `dispatch_terminal.rs`, not here.
///
/// The candidate box is anchored near the bottom-left (the usual prompt row)
/// rather than the live caret. Caret-following is a future polish; this keeps
/// the popup on-screen and functional.
pub(crate) fn ime_host<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    enabled: bool,
    terminal: std::sync::Arc<std::sync::Mutex<oryxis_terminal::TerminalState>>,
    font_size: f32,
) -> Element<'a, Message> {
    use iced::advanced::widget::{tree, Operation, Tree, Widget};
    use iced::advanced::{input_method, layout, mouse, overlay, renderer, Layout, Shell};
    use iced::{Event, Length as L, Point, Rectangle, Size, Vector};

    struct ImeHost<'a, Message> {
        content: Element<'a, Message>,
        enabled: bool,
        terminal: std::sync::Arc<std::sync::Mutex<oryxis_terminal::TerminalState>>,
        font_size: f32,
    }

    impl<Message> Widget<Message, Theme, iced::Renderer> for ImeHost<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }
        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }
        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }
        fn diff(&self, tree: &mut Tree) {
            self.content.as_widget().diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.as_widget().size()
        }
        fn size_hint(&self) -> Size<L> {
            self.content.as_widget().size_hint()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content.as_widget_mut().layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content
                .as_widget_mut()
                .update(tree, event, layout, cursor, renderer, shell, viewport);

            // The shell only honours an input-method request issued during a
            // redraw frame; only the focused pane requests it.
            if self.enabled
                && matches!(
                    event,
                    Event::Window(iced::window::Event::RedrawRequested(_))
                )
            {
                let b = layout.bounds();
                // Anchor the candidate window at the terminal caret. try_lock
                // so a frame that races the render thread just falls back to
                // the bottom-left instead of blocking the UI.
                let cursor_area = match self.terminal.try_lock() {
                    Ok(state) => oryxis_terminal::ime_caret_rect(
                        b,
                        self.font_size,
                        state.cursor_cell(),
                    ),
                    Err(_) => {
                        let h = 18.0_f32.min(b.height);
                        Rectangle::new(
                            Point::new(b.x + 8.0, b.y + (b.height - h).max(0.0)),
                            Size::new(2.0, h),
                        )
                    }
                };
                let ime: input_method::InputMethod = input_method::InputMethod::Enabled {
                    cursor: cursor_area,
                    purpose: input_method::Purpose::Normal,
                    preedit: None,
                };
                shell.request_input_method(&ime);
            }
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content
                .as_widget_mut()
                .overlay(tree, layout, renderer, viewport, translation)
        }
    }

    Element::new(ImeHost {
        content: content.into(),
        enabled,
        terminal,
        font_size,
    })
}

/// The single, canonical full-window modal overlay: `base` view, a scrim
/// that absorbs both click and hover, and a centered `card` that traps its
/// own clicks. Every blocking modal should route through this so the scrim
/// can never reintroduce mouse bleed-through to the live UI behind it.
///
/// - `on_scrim_click`: `Some(msg)` makes an outside click dismiss the modal;
///   `None` is for auth-style modals (host key, 2FA, update) that must not
///   dismiss on a stray backdrop click. Either way the scrim fully absorbs
///   the click, so nothing reaches `base`.
/// - `top_reserve`: a transparent band (px) at the top of the *scrim only*,
///   so the window title bar (drag / minimize / maximize / close) stays
///   hittable while the modal is open. The card still centers over the full
///   height. Pass `40.0` for app-level modals, `0.0` for in-view ones.
///
/// `interaction(Idle)` on the scrim is load-bearing: without it iced lets
/// hover events bleed through the `Stack` to widgets below. The card's own
/// `MouseArea` is what stops a click *on the card* from falling through to
/// the scrim and triggering a dismiss, this helper owns that step because it
/// is the one every hand-rolled modal forgot.
pub(crate) fn modal_overlay<'a>(
    base: Element<'a, Message>,
    card: Element<'a, Message>,
    on_scrim_click: Option<Message>,
    top_reserve: f32,
) -> Element<'a, Message> {
    use iced::widget::{column, MouseArea};

    let scrim_fill = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
            ..Default::default()
        });
    let scrim_body: Element<'a, Message> = if top_reserve > 0.0 {
        column![Space::new().height(Length::Fixed(top_reserve)), scrim_fill]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        scrim_fill.into()
    };

    let scrim: Element<'a, Message> = MouseArea::new(scrim_body)
        .interaction(iced::mouse::Interaction::Idle)
        .on_press(on_scrim_click.unwrap_or(Message::NoOp))
        .into();

    let card_trap: Element<'a, Message> =
        MouseArea::new(card).on_press(Message::NoOp).into();
    let centered = container(card_trap)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    Stack::new()
        .push(base)
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
