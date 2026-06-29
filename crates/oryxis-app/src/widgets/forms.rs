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
