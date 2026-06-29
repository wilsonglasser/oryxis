//! UI helper widgets: toolbar. Split out of widgets/mod.rs.

use super::*;
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

/// Shared 24×24 toolbar icon button (search-collapse + overflow). Styled
/// like `sort_toolbar_button`; when `active` it carries an accent tint so
/// the open floating field / menu reads as toggled. A tooltip names the
/// action since the glyph alone isn't self-evident.
fn toolbar_icon_button(
    glyph: iced::widget::Text<'static, iced::Theme, iced::Renderer>,
    msg: Message,
    active: bool,
    tip: &'static str,
) -> Element<'static, Message> {
    let inner = button(
        container(glyph.size(15).color(OryxisColors::t().button_text))
            .center_y(Length::Fixed(24.0))
            .center_x(Length::Fixed(24.0)),
    )
    .on_press(msg)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
            _ if active => Color { a: 0.18, ..OryxisColors::t().accent },
            _ => OryxisColors::t().button_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    iced::widget::tooltip(
        inner,
        container(text(tip).size(11).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// Search icon shown in the toolbar when the window is too narrow for an
/// inline search field. Clicking it pops the floating search input
/// (`OverlayContent::ToolbarSearch`).
pub(crate) fn toolbar_search_icon(active: bool) -> Element<'static, Message> {
    toolbar_icon_button(
        iced_fonts::lucide::search(),
        Message::ToggleToolbarSearch,
        active,
        crate::i18n::t("search"),
    )
}

/// Overflow `…` icon that folds the toolbar's secondary actions into a
/// menu (`OverlayContent::ToolbarOverflow`) when even the icon-collapsed
/// search can't free enough room for them inline.
pub(crate) fn toolbar_overflow_icon(active: bool) -> Element<'static, Message> {
    toolbar_icon_button(
        iced_fonts::lucide::ellipsis(),
        Message::ToggleToolbarOverflow,
        active,
        crate::i18n::t("toolbar_more"),
    )
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
