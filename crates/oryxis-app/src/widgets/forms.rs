//! UI helper widgets: forms. Split out of widgets/mod.rs.

use super::*;
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

/// `panel_field` plus a line saying where an unset field's value comes
/// from (D4 group inheritance).
///
/// `inherited` is `Some((value, group label))` only when the host
/// itself sets nothing AND an ancestor does; a host with its own value
/// renders exactly like `panel_field`, because the inherited value is
/// then not what will be used and saying otherwise would be a lie.
///
/// The hint is a LINE rather than placeholder text inside the input: a
/// greyed value in the box reads as something already typed, and the
/// user needs to see both that the field is empty and what will be used
/// because it is.
pub(crate) fn panel_field_inherited<'a>(
    label: &'a str,
    input: Element<'a, Message>,
    inherited: Option<(String, String)>,
) -> Element<'a, Message> {
    let Some((value, group)) = inherited else {
        // An empty label means the caller already has its own row
        // header (or needs none), so it gets the bare input back
        // instead of a blank line above it.
        return if label.is_empty() {
            input
        } else {
            panel_field(label, input)
        };
    };
    let mut col = iced::widget::column![];
    if !label.is_empty() {
        col = col
            .push(text(label).size(12).color(OryxisColors::t().text_muted))
            .push(Space::new().height(4));
    }
    col.push(input)
        .push(Space::new().height(3))
        .push(
            text(
                crate::i18n::t("inherited_from")
                    .replace("{value}", &value)
                    .replace("{group}", &group),
            )
            .size(10)
            .color(OryxisColors::t().accent),
        )
        .width(Length::Fill)
        .align_x(dir_align_x())
        .into()
}

/// `panel_field` for a credential input: standardizes the tri-state
/// password placeholder every editor tracks by hand. While an existing
/// secret is stored and the field untouched, the placeholder says the
/// value is kept unchanged; otherwise `empty_hint` shows (the field's
/// normal placeholder). Callers pass the `has_existing` / `touched`
/// bools they already track; no new state.
pub(crate) fn password_placeholder(has_existing: bool, touched: bool, empty_hint: &'static str) -> &'static str {
    if has_existing && !touched {
        crate::i18n::t("proxy_password_existing")
    } else {
        empty_hint
    }
}

/// The standard editor Cancel button (muted). Pair with
/// [`form_save_button`] inside [`form_footer`]; wrap in the surface's
/// keynav slot at the call site (the layer differs per surface).
pub(crate) fn form_cancel_button<'a>(msg: Message) -> Element<'a, Message> {
    styled_button(crate::i18n::t("cancel"), msg, OryxisColors::t().text_muted)
}

/// The standard editor primary button (accent). `on_save: None`
/// renders the disabled state (greyed, no on_press), so validation /
/// in-flight gating is uniform and double-submit is structurally
/// impossible.
pub(crate) fn form_save_button<'a>(label: &'a str, on_save: Option<Message>) -> Element<'a, Message> {
    styled_button_opt(label, on_save, OryxisColors::t().accent)
}

/// The standard editor footer: Cancel then Save, mirrored under RTL
/// (`dir_row`), with the proxy-identity form's spacing and padding.
/// Callers pass the buttons already wrapped in their layer's keynav
/// slot, recorded in this order (cancel, save) so build order stays
/// display order.
pub(crate) fn form_footer<'a>(
    cancel: Element<'a, Message>,
    save: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        dir_row(vec![cancel, Space::new().width(8).into(), save])
            .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 8.0, right: 16.0, bottom: 16.0, left: 16.0 })
    .into()
}

/// The standard inline-error slot: renders nothing (zero height) when
/// `None`, otherwise the error row every editor shares. Sits between
/// the last field (or the scrollable) and the footer, always.
pub(crate) fn form_error<'a>(error: Option<&'a str>) -> Element<'a, Message> {
    match error {
        Some(err) => container(
            text(err)
                .size(12)
                .color(OryxisColors::t().error)
                .width(Length::Fill)
                .align_x(crate::widgets::dir_align_x()),
        )
        .padding(Padding { top: 0.0, right: 16.0, bottom: 8.0, left: 16.0 })
        .into(),
        None => Space::new().into(),
    }
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

/// An option row shell: icon + label on the leading edge, a caller
/// supplied trailing control (usually a pick_list). Keyboard-navigable
/// callers wrap the control itself in `panel_nav_slot` before passing
/// it in, so the focus ring hugs the pick_list rather than the row.
pub(crate) fn panel_option_row<'a>(
    icon_widget: iced::widget::Text<'a>,
    label: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        dir_row(vec![
            icon_widget.size(13).color(OryxisColors::t().text_muted).into(),
            Space::new().width(10).into(),
            text(label).size(13).color(OryxisColors::t().text_secondary).into(),
            Space::new().width(Length::Fill).into(),
            control,
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

/// Chrome for the right-side editor panels (host editor, key import /
/// generate, identity, snippet, port-forward forms, ...):
/// `width` total (pass the live `Oryxis::panel_width`), the given panel
/// background, and a 4 px draggable handle on the LEADING edge (between
/// main content and panel) instead of a full frame. The handle replaces
/// the old 1 px separator and doubles as the resize grip: pressing it
/// arms `panel_resize_drag`, the global mouse-move handler in
/// `dispatch_tabs/window.rs` follows the cursor, and the global
/// left-release ends the drag and persists the width. Same affordance
/// (width, colour, cursor) as the terminal sidebar's region handle. The
/// full border used to double up with the status-bar hairline below and
/// the tab-bar hairline above (a visible 2 px "double border" at the
/// panel's bottom edge), and with the 1 px window frame on the trailing
/// edge; those chrome lines already bound the other three sides. Under
/// RTL the panel sits on the leading (left) side, and `dir_row` mirrors
/// the handle onto the panel's content-facing edge automatically.
pub(crate) fn side_panel_frame(
    content: Element<'_, Message>,
    background: iced::Color,
    width: f32,
) -> Element<'_, Message> {
    let handle = iced::widget::MouseArea::new(
        container(Space::new().width(Length::Fixed(4.0)).height(Length::Fill))
            .width(Length::Fixed(4.0))
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            }),
    )
    .on_press(Message::Tabs(crate::app::TabsMessage::SidePanelResizeStart))
    .interaction(iced::mouse::Interaction::ResizingHorizontally);
    let body = container(content)
        .width(width - 4.0)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            ..Default::default()
        });
    dir_row(vec![handle.into(), body.into()])
        .height(Length::Fill)
        .into()
}
