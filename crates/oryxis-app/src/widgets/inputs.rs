//! UI helper widgets: inputs. Split out of widgets/mod.rs.

use super::*;
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
