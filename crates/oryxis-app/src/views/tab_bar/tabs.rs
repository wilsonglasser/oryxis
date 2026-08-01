//! Tab bar: tabs. Split out of views/tab_bar/mod.rs.

use super::*;
pub(crate) fn area_tab<'a>(
    label: &'a str,
    glyph: iced::widget::Text<'a>,
    view: View,
    is_active: bool,
    solid_fill: bool,
) -> Element<'a, Message> {
    let fg = if is_active {
        OryxisColors::t().accent
    } else {
        // text_secondary (not text_muted) so the inactive area icon stays
        // lively over the top-bar accent wash instead of reading as a dull
        // grey glyph.
        OryxisColors::t().text_secondary
    };
    // Same "lit from above" vertical gradient as the active session
    // tab, in the app accent, so the strip carries exactly one visual
    // language for "active" (issue #38: the old flat teal pill read as
    // a different kind of element next to gradient session tabs).
    let bg: Background = if is_active {
        active_tab_bg(OryxisColors::t().accent, solid_fill)
    } else {
        Background::Color(Color::TRANSPARENT)
    };
    let style = move |_: &iced::Theme, status: BtnStatus| {
        let hover_bg: Background = match status {
            BtnStatus::Hovered if !is_active => {
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
            }
            _ => bg,
        };
        button::Style {
            background: Some(hover_bg),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    };
    // Icon-only (e.g. the Home tab): a square button (side == the
    // labeled tabs' rendered height, TAB_HEIGHT + the button's default
    // 5px top/bottom padding) so the frame echoes the square glyph
    // instead of stretching wide. Zero padding so the square is exact.
    let btn: Element<'a, Message> = if label.is_empty() {
        const SQUARE: f32 = TAB_ROW_HEIGHT;
        button(
            container(glyph.size(16).color(fg))
                .center_x(Length::Fixed(SQUARE))
                .center_y(Length::Fixed(SQUARE)),
        )
        .padding(0)
        .on_press(Message::Navigation(NavigationMessage::ChangeView(view)))
        .style(style)
        .into()
    } else {
        button(
            container(
                crate::widgets::dir_row(vec![
                    container(glyph.size(14).color(fg))
                        .center_x(Length::Fixed(TAB_ICON_SLOT))
                        .center_y(Length::Fixed(TAB_ICON_SLOT))
                        .into(),
                    Space::new().width(6).into(),
                    text(label)
                        .size(12)
                        .line_height(1.0)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .font(SYSTEM_UI_SEMIBOLD)
                        .color(fg)
                        .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .center_y(Length::Fixed(TAB_HEIGHT))
            .padding(Padding { top: 0.0, right: 10.0, bottom: 0.0, left: 6.0 }),
        )
        .on_press(Message::Navigation(NavigationMessage::ChangeView(view)))
        .style(style)
        .into()
    };
    btn
}

/// A SFTP browser tab chip in the strip, styled to match the terminal session
/// tabs: a rounded folder badge (tinted with the mounted host's accent) + the
/// label, with the close X *inside* the tab fill as a trailing slot (shown on
/// active / hover). Active claims `width`; inactive shrinks. Right-click opens
/// the tab context menu; pinned tabs get an accent outline.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sftp_session_tab<'a>(
    idx: usize,
    // Plain `&str` (not `&'a str`): the caller passes a per-frame
    // redacted label under Privacy Mode (issue #78), so the element
    // must not borrow it.
    label: &str,
    is_active: bool,
    width: f32,
    // The mounted host's brand colour, NOT gated by `tab_accent_color`:
    // the folder badge always carries the host identity (matching the
    // terminal tabs' OS badge) even when the accent source is pinned to
    // the app. Falls back to the app accent for a no-host tab.
    badge_accent: Color,
    // The gated accent (None when `tab_accent_color = "app"`): drives the
    // text, border and gradient wash only.
    host_accent: Option<Color>,
    // `tab_accent_text` setting: when false the label / close X render
    // in the theme's neutral text colours instead of the host accent.
    accent_text: bool,
    pinned: bool,
    solid_fill: bool,
) -> Element<'a, Message> {
    // The contrast-validated (issue #79) gated accent is what may render
    // as text, border and gradient wash over the strip; the raw brand
    // colour fills the badge (white glyph on top stays legible).
    let effective_accent = crate::theme::readable_accent_on(
        host_accent.unwrap_or_else(|| OryxisColors::t().accent),
        OryxisColors::t().bg_sidebar,
    );
    let active_fg = if accent_text {
        effective_accent
    } else {
        OryxisColors::t().text_primary
    };
    let fg = if is_active {
        active_fg
    } else {
        OryxisColors::t().text_muted
    };
    let bg: Background = if is_active {
        active_tab_bg(effective_accent, solid_fill)
    } else {
        Background::Color(Color::TRANSPARENT)
    };
    // Badge: always the folder glyph (so an SFTP tab stays recognizable as
    // SFTP, not mistaken for a terminal), tinted with the mounted host's color
    // (custom or OS-brand) so it still "inherits" the host's hue.
    let badge = container(iced_fonts::lucide::folder_tree().size(12).color(Color::WHITE))
        .center_x(Length::Fixed(TAB_ICON_SLOT))
        .center_y(Length::Fixed(TAB_ICON_SLOT))
        .style(move |_| container::Style {
            background: Some(Background::Color(badge_accent)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        });
    // Always render the X inside the tab fill (no separate hover state).
    let show_close = true;
    let label_width = (width - TAB_ICON_SLOT - TAB_ICON_SLOT - 12.0).max(0.0);
    let label_text = text(truncate_label(label, label_width))
        .size(12)
        .line_height(1.0)
        .wrapping(iced::widget::text::Wrapping::None)
        .font(SYSTEM_UI_SEMIBOLD)
        .color(fg)
        .width(Length::Fill);
    // Close X as a MouseArea (so it nests inside the select button), inside the
    // tab fill. Reserves its slot even when hidden so the label doesn't jump.
    let trailing: Element<'_, Message> = if show_close {
        MouseArea::new(
            container(
                iced_fonts::lucide::x().size(11).color(if is_active {
                    active_fg
                } else {
                    OryxisColors::t().text_secondary
                }),
            )
            .center_x(Length::Fixed(TAB_ICON_SLOT))
            .center_y(Length::Fixed(TAB_ICON_SLOT))
            .style(move |_| container::Style {
                background: Some(Background::Color(if is_active {
                    Color::TRANSPARENT
                } else {
                    OryxisColors::t().bg_hover
                })),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }),
        )
        .on_press(Message::Sftp(SftpMessage::CloseSftpTab(idx)))
        .into()
    } else {
        Space::new().width(TAB_ICON_SLOT).height(TAB_ICON_SLOT).into()
    };
    let inner_row = crate::widgets::dir_row(vec![
        badge.into(),
        Space::new().width(5).into(),
        label_text.into(),
        Space::new().width(4).into(),
        trailing,
    ])
    .align_y(iced::Alignment::Center);
    let tab_btn = button(
        container(inner_row)
            .center_y(Length::Fixed(TAB_HEIGHT))
            .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 6.0 }),
    )
    .width(Length::Fixed(width))
    .on_press(Message::Sftp(SftpMessage::SelectSftpTab(idx)))
    .style(move |_, status| {
        let hover_bg: Background = match status {
            BtnStatus::Hovered if !is_active => {
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
            }
            _ => bg,
        };
        let border = if pinned {
            Border { radius: Radius::from(6.0), color: effective_accent, width: 1.5 }
        } else {
            Border { radius: Radius::from(6.0), ..Default::default() }
        };
        button::Style { background: Some(hover_bg), border, ..Default::default() }
    });
    MouseArea::new(tab_btn)
        .on_enter(Message::Sftp(SftpMessage::SftpTabHovered(idx)))
        .on_exit(Message::Sftp(SftpMessage::SftpTabUnhovered))
        .on_right_press(Message::Sftp(SftpMessage::ShowSftpTabMenu(idx)))
        .into()
}

/// The Settings tab (issue #120). Deliberately plainer than the session
/// tabs: it has no host, so no OS badge, no per-host accent, no privacy
/// redaction and no context menu. It carries the app accent and a gear,
/// which is exactly the vocabulary the toolbar's Settings button already
/// uses, so the strip entry reads as the same destination.
pub(crate) fn settings_tab<'a>(
    label: &'a str,
    is_active: bool,
    is_hovered: bool,
    width: f32,
    close_on_right: bool,
    solid_fill: bool,
) -> Element<'a, Message> {
    let accent = crate::theme::readable_accent_on(
        OryxisColors::t().accent,
        OryxisColors::t().bg_sidebar,
    );
    let fg = if is_active {
        OryxisColors::t().text_primary
    } else {
        OryxisColors::t().text_muted
    };
    let bg: Background = if is_active {
        active_tab_bg(accent, solid_fill)
    } else {
        Background::Color(Color::TRANSPARENT)
    };
    let badge = || {
        container(iced_fonts::lucide::settings().size(12).color(Color::WHITE))
            .center_x(Length::Fixed(TAB_ICON_SLOT))
            .center_y(Length::Fixed(TAB_ICON_SLOT))
            .style(move |_| container::Style {
                background: Some(Background::Color(accent)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            })
            .into()
    };
    // Same close affordance as every other tab: a button (so hover and
    // press tint toward the error colour), shown only when the tab is
    // active or hovered, and placed by the user's close-button-side
    // setting rather than pinned to the right.
    let close_btn = || -> Element<'a, Message> {
        button(
            container(iced_fonts::lucide::x().size(11).color(if is_active {
                OryxisColors::t().text_primary
            } else {
                OryxisColors::t().text_secondary
            }))
            .center_x(Length::Fixed(TAB_ICON_SLOT))
            .center_y(Length::Fixed(TAB_ICON_SLOT)),
        )
        .padding(0)
        .style(move |_, status| {
            let rest = if is_active {
                Color::TRANSPARENT
            } else {
                OryxisColors::t().bg_hover
            };
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.18, ..OryxisColors::t().error },
                BtnStatus::Pressed => Color { a: 0.34, ..OryxisColors::t().error },
                _ => rest,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .on_press(Message::Tabs(TabsMessage::CloseSettingsTab))
        .into()
    };
    let show_close = is_active || is_hovered;
    // `truncate_label` already reserves the badge + gaps; only the
    // trailing X slot is on top of that. Subtracting the badge again here
    // is what truncated "Settings" to "Sett…" on a min-width chip, so the
    // reserve has to match `settings_tab_width` exactly.
    let label_width = (width - TAB_ICON_SLOT - 4.0).max(0.0);
    let label_text = text(truncate_label(label, label_width))
        .size(12)
        .line_height(1.0)
        .wrapping(iced::widget::text::Wrapping::None)
        .font(SYSTEM_UI_SEMIBOLD)
        .color(fg)
        .width(Length::Fill);
    let mut items: Vec<Element<'a, Message>> = vec![
        // Leading slot follows the session tabs: the X REPLACES the badge
        // on hover unless close-on-right is set, in which case the badge
        // stays put and the X gets its own trailing slot.
        if close_on_right || !show_close { badge() } else { close_btn() },
        Space::new().width(5).into(),
        label_text.into(),
    ];
    if close_on_right {
        // Reserved even when hidden, so the label doesn't jump on hover.
        items.push(Space::new().width(4).into());
        items.push(if show_close {
            close_btn()
        } else {
            Space::new().width(TAB_ICON_SLOT).height(TAB_ICON_SLOT).into()
        });
    }
    let inner_row = crate::widgets::dir_row(items).align_y(iced::Alignment::Center);
    let tab_btn = button(
        container(inner_row)
            .center_y(Length::Fixed(TAB_HEIGHT))
            .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 2.0 }),
    )
    .width(Length::Fixed(width))
    .on_press(Message::Navigation(NavigationMessage::ChangeView(View::Settings)))
    .style(move |_, status| {
        let hover_bg: Background = match status {
            BtnStatus::Hovered if !is_active => {
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
            }
            _ => bg,
        };
        button::Style {
            background: Some(hover_bg),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    // The hover flag is also what arms a reorder drag (the press handler
    // reads it), so this MouseArea is what makes the tab draggable, not
    // just what reveals the X.
    MouseArea::new(tab_btn)
        .on_enter(Message::Tabs(TabsMessage::SettingsTabHovered))
        .on_exit(Message::Tabs(TabsMessage::SettingsTabUnhovered))
        .into()
}

/// Compact (Chrome-style) pinned SFTP tab: icon-only folder chip at a fixed
/// width. Select on click, right-click opens the context menu. Mirrors
/// `pinned_tab_chip` for the SFTP side.
pub(crate) fn sftp_pinned_chip<'a>(idx: usize, is_active: bool, badge_accent: Color, host_accent: Option<Color>, solid_fill: bool) -> Element<'a, Message> {
    // Folder glyph (SFTP identity) tinted with the host brand, ungated so
    // the identity survives `tab_accent_color = "app"`.
    let badge = container(iced_fonts::lucide::folder_tree().size(12).color(Color::WHITE))
        .center_x(Length::Fixed(TAB_ICON_SLOT))
        .center_y(Length::Fixed(TAB_ICON_SLOT))
        .style(move |_| container::Style {
            background: Some(Background::Color(badge_accent)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        });
    // Contrast-validated (issue #79) so a black brand colour still
    // produces a visible "lit from above" active wash on dark themes.
    let wash_accent = host_accent.unwrap_or_else(|| OryxisColors::t().accent);
    let bg: Background = if is_active {
        active_tab_bg(
            crate::theme::readable_accent_on(wash_accent, OryxisColors::t().bg_sidebar),
            solid_fill,
        )
    } else {
        Background::Color(Color::TRANSPARENT)
    };
    // Match `pinned_tab_chip` exactly: same CHIP_W box, default button padding
    // (so the height lines up with the Home icon), the active "lit from above"
    // gradient as the only selected cue, and NO accent outline (the icon-only
    // shape is itself the pin affordance).
    let tab_btn = button(
        container(badge)
            .center_x(Length::Fixed(CHIP_W))
            .center_y(Length::Fixed(TAB_HEIGHT)),
    )
    .width(Length::Fixed(CHIP_W))
    .on_press(Message::Sftp(SftpMessage::SelectSftpTab(idx)))
    .style(move |_, status| {
        let hover_bg: Background = match status {
            _ if is_active => bg,
            BtnStatus::Hovered => Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06)),
            _ => Background::Color(Color::TRANSPARENT),
        };
        button::Style {
            background: Some(hover_bg),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    MouseArea::new(tab_btn)
        .on_enter(Message::Sftp(SftpMessage::SftpTabHovered(idx)))
        .on_exit(Message::Sftp(SftpMessage::SftpTabUnhovered))
        .on_right_press(Message::Sftp(SftpMessage::ShowSftpTabMenu(idx)))
        .into()
}

/// The `Underline` inactive-tab-style overlay (issue #87): a 2px
/// neutral rule pinned to the chip's edge, sized to the button's
/// RENDERED box (`TAB_ROW_HEIGHT`, padding included) so the Stack
/// overlay aligns exactly and never reserves layout space. Sizing it to
/// `TAB_HEIGHT` instead squeezed the button back to its content box, so
/// an underlined chip lost its 10 px of vertical padding and the rule
/// landed right under the label instead of on the chip's edge, where
/// the `Border` style draws its own bottom edge.
///
/// Horizontal strips put the rule on the inner, content-facing edge
/// (bottom on a top strip, top on a bottom strip). The vertical docks
/// deliberately do NOT rotate it: a 2px vertical tick floating in the
/// gutter beside each chip reads as an artifact rather than as an
/// underline, which is what the reporter saw on the left dock. Stacked
/// chips are a list, so the rule stays horizontal and lands under each
/// one, where it reads as the list separator it visually is.
fn inactive_edge_line<'a>(width: f32, color: Color) -> Element<'a, Message> {
    use crate::views::tab_bar::{tab_bar_pos, TabBarPos};
    const T: f32 = 2.0;
    let rule = |w: Length, h: Length| -> Element<'a, Message> {
        container(Space::new())
            .width(w)
            .height(h)
            .style(move |_| container::Style {
                background: Some(Background::Color(color)),
                ..Default::default()
            })
            .into()
    };
    let frame = |el: Element<'a, Message>| -> Element<'a, Message> {
        container(el)
            .width(Length::Fixed(width))
            .height(Length::Fixed(TAB_ROW_HEIGHT))
            .into()
    };
    match tab_bar_pos() {
        TabBarPos::Top => frame(
            iced::widget::Column::new()
                .push(Space::new().height(Length::Fill))
                .push(rule(Length::Fill, Length::Fixed(T)))
                .into(),
        ),
        TabBarPos::Bottom => frame(
            iced::widget::Column::new()
                .push(rule(Length::Fill, Length::Fixed(T)))
                .push(Space::new().height(Length::Fill))
                .into(),
        ),
        // Both vertical docks: same bottom rule as the top strip, acting
        // as the separator between stacked chips.
        TabBarPos::Left | TabBarPos::Right => frame(
            iced::widget::Column::new()
                .push(Space::new().height(Length::Fill))
                .push(rule(Length::Fill, Length::Fixed(T)))
                .into(),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn session_tab<'a>(
    idx: usize,
    // Plain `&str` (not `&'a str`): the caller passes a per-frame
    // redacted label under Privacy Mode (issue #78), so the element
    // must not borrow it.
    label: &str,
    pane_count: usize,
    is_active: bool,
    is_hovered: bool,
    detected_os: Option<&str>,
    width: f32,
    close_on_right: bool,
    status_dot: Option<Color>,
    // Smart-tabs attention (finished / failed / activity on a background
    // tab), drawn as a dot on the badge's top-right corner so it can
    // coexist with the connection-state dot at the bottom-right.
    attention_dot: Option<Color>,
    host_accent: Option<Color>,
    // `tab_accent_text` setting: when false the label / close X render
    // in the theme's neutral text colours instead of the host accent.
    accent_text: bool,
    host_icon_style: crate::widgets::HostIconStyle,
    // Session-group tabs override the OS-derived badge with the icon + color
    // the user set on the group, so the strip matches the dashboard card.
    custom_icon: Option<&'a str>,
    custom_color: Option<Color>,
    // Full-style pinned tab: draws a distinct left-edge accent border.
    pinned: bool,
    solid_fill: bool,
    // OSC 9;4 progress from the focused pane; drawn as a growing border.
    progress: Option<oryxis_terminal::Progress>,
    // Hybrid tab (issue #61): `Some(state)` renders the clickable mode
    // glyph (>_ terminal / folder files); `None` hides it (no SSH).
    files_mode: Option<bool>,
    // Optional second-line address (`host:port`) shown below the
    // label when the "Show host address on tabs" setting is on.
    address: Option<String>,
) -> Element<'a, Message> {
    // Contrast validator (issue #79): the accent renders as the active
    // tab's TEXT (plus borders and the gradient wash) over the strip, so
    // a too-dark brand colour (AlmaLinux black, macOS grey) is repaired
    // toward readability. The OS badge below keeps the raw brand colour.
    let effective_accent = crate::theme::readable_accent_on(
        host_accent.unwrap_or_else(|| OryxisColors::t().accent),
        OryxisColors::t().bg_sidebar,
    );
    // Neutral-text mode keeps the accent in the wash / border / dots
    // but paints the label and X like any other primary text.
    let active_fg = if accent_text {
        effective_accent
    } else {
        OryxisColors::t().text_primary
    };
    let fg = if is_active {
        active_fg
    } else {
        OryxisColors::t().text_muted
    };
    // Active tab paints a vertical gradient JetBrains-style: a
    // saturated tint at the top (highlight, ~0.28 alpha) fading to
    // almost transparent at the bottom (~0.04 alpha). Pairs with the
    // border-bottom hairline in `view_main` so the active tab reads
    // as "lit from above" instead of a flat chip. Inactive tabs stay
    // transparent so hover gets the only visible cue. The active fill
    // honours the user's gradient/solid choice via `active_tab_bg`.
    let bg: Background = if is_active {
        active_tab_bg(effective_accent, solid_fill)
    } else {
        Background::Color(Color::TRANSPARENT)
    };

    let is_disconnected = label.ends_with(" (disconnected)");
    let display_label_full = format!("{}. {}", idx + 1, label.trim_end_matches(" (disconnected)"));
    // When the close X gets its own trailing slot, the label has less
    // horizontal room. Reserve the X's slot + a small gap so the
    // truncation kicks in earlier instead of the X clipping over the
    // last few characters.
    let mut label_width = if close_on_right {
        (width - TAB_ICON_SLOT - 4.0).max(0.0)
    } else {
        width
    };
    // The hybrid mode glyph takes a chip slot out of the label's room.
    if files_mode.is_some() {
        label_width = (label_width - 20.0).max(0.0);
    }
    // So does the split pane-count pill. `tab_content_width` already
    // reserves this when it sizes the chip, so leaving it out here let the
    // label claim room the pill was going to occupy and spill past the
    // chip's edge on any grouped tab (#108).
    if pane_count > 1 {
        label_width = (label_width - (COUNT_DISC + COUNT_GAP)).max(0.0);
    }
    let display_label = truncate_label(&display_label_full, label_width);

    let show_close = is_active || is_hovered;
    let os_badge: Element<'_, Message> = {
        let fallback = if is_disconnected {
            OryxisColors::t().text_muted
        } else {
            OryxisColors::t().accent
        };
        let (glyph, mut badge_color) = if let Some(name) = custom_icon {
            (
                crate::os_icon::custom_icon_glyph(name),
                custom_color.unwrap_or(fallback),
            )
        } else {
            crate::os_icon::resolve_icon(detected_os, fallback)
        };
        if is_disconnected {
            badge_color = OryxisColors::t().text_muted;
        }
        // host_icon respects the user's chosen shape (Circular /
        // Square / Outline / Initials). For Initials the OS glyph is
        // ignored and the leading letters of the label render; for
        // the other styles the glyph paints inside the shape.
        let glyph_el: Element<'_, Message> = glyph.view(12.0, Color::WHITE);
        let base = crate::widgets::host_icon(
            host_icon_style,
            badge_color,
            label,
            Some(glyph_el),
            TAB_ICON_SLOT,
        );
        // Wrap in a container so the existing status_dot Stack code
        // below still has a container to compose with; host_icon
        // already returns an Element so we re-wrap to keep the
        // dot-overlay branch unchanged.
        let base = container(base)
            .center_x(Length::Fixed(TAB_ICON_SLOT))
            .center_y(Length::Fixed(TAB_ICON_SLOT));
        // Corner dots stacked over the badge: connection state keeps the
        // bottom-right, smart-tabs attention takes the top-right, so both
        // can show at once. The split pane-count is a separate inline chip
        // after the icon (built below), so it stays legible instead of
        // crowding the glyph.
        if status_dot.is_some() || attention_dot.is_some() {
            let mut stack = iced::widget::Stack::new().push(base);
            if let Some(c) = status_dot {
                stack = stack.push(corner_dot(c, 7.0, 1.5, iced::alignment::Vertical::Bottom));
            }
            if let Some(c) = attention_dot {
                stack = stack.push(corner_dot(c, 7.0, 1.5, iced::alignment::Vertical::Top));
            }
            stack
                .width(Length::Fixed(TAB_ICON_SLOT))
                .height(Length::Fixed(TAB_ICON_SLOT))
                .into()
        } else {
            base.into()
        }
    };
    // Split pane-count chip: a small rounded pill shown right after the
    // icon (offset from it) on a split tab, e.g. "2". Tinted with the tab
    // text color so it reads in both active and inactive states.
    let count_chip: Option<Element<'_, Message>> = (pane_count > 1).then(|| {
        // Two-digit counts (10+ panes) are vanishingly rare; they'd just
        // fill the disc a little tighter.
        container(
            text(pane_count.to_string())
                .size(10)
                .line_height(1.0)
                .font(SYSTEM_UI_SEMIBOLD)
                .color(fg),
        )
        .center_x(Length::Fixed(COUNT_DISC))
        .center_y(Length::Fixed(COUNT_DISC))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.16, ..fg })),
            border: Border {
                radius: Radius::from(COUNT_DISC / 2.0),
                color: Color { a: 0.35, ..fg },
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
    });
    let close_btn = || -> Element<'_, Message> {
        let icon_color = if is_active {
            active_fg
        } else {
            OryxisColors::t().text_secondary
        };
        // A real `button` (not a bare `MouseArea`) so the tab only closes on
        // release-over-the-button: pressing then dragging off cancels the
        // close, and `Status::Hovered`/`Pressed` give the highlight for free.
        button(
            container(iced_fonts::lucide::x().size(11).color(icon_color))
                .center_x(Length::Fixed(TAB_ICON_SLOT))
                .center_y(Length::Fixed(TAB_ICON_SLOT)),
        )
        .padding(0)
        .style(move |_, status| {
            // At rest the inactive tab carries a subtle fill so the X reads
            // as a button; the active tab stays transparent. Hover/press tint
            // toward the error colour to signal the destructive action.
            let rest = if is_active {
                Color::TRANSPARENT
            } else {
                OryxisColors::t().bg_hover
            };
            let bg = match status {
                BtnStatus::Hovered => Color { a: 0.18, ..OryxisColors::t().error },
                BtnStatus::Pressed => Color { a: 0.34, ..OryxisColors::t().error },
                _ => rest,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .on_press(Message::Tabs(TabsMessage::CloseTab(idx)))
        .into()
    };

    // Leading slot follows the Termius behaviour by default (X replaces
    // badge on hover/active). When close-on-right is set, the badge
    // always stays leading and the X joins as a separate trailing slot.
    let leading_slot: Element<'_, Message> = if close_on_right || !show_close {
        os_badge
    } else {
        close_btn()
    };

    let label_text = text(display_label.clone())
        .size(12)
        .line_height(1.0)
        .wrapping(iced::widget::text::Wrapping::None)
        .font(SYSTEM_UI_SEMIBOLD)
        .color(fg)
        .width(if is_active { Length::Fill } else { Length::Shrink });

    // Second-line host address, shown below the label when enabled.
    let address_row: Option<Element<'_, Message>> = address.map(|addr| {
        container(
            text(addr)
                .size(10)
                .line_height(1.0)
                .wrapping(iced::widget::text::Wrapping::None)
                .font(SYSTEM_UI_SEMIBOLD)
                .color(Color { a: 0.50, ..fg }),
        )
        .into()
    });

    // Hybrid mode glyph: shows the tab's current surface, clicking
    // flips it (shared with the pinned chip form).
    let mode_chip: Option<Element<'_, Message>> =
        files_mode.map(|fm| tab_mode_chip(idx, fm, fg));

    let inner_row: Element<'_, Message> = {
        let mut items: Vec<Element<'_, Message>> = vec![leading_slot];
        // Pane-count chip sits just after the icon, offset by a small gap.
        if let Some(chip) = count_chip {
            items.push(Space::new().width(4).into());
            items.push(chip);
        }
        // Hybrid mode glyph follows the badge/count cluster.
        if let Some(chip) = mode_chip {
            items.push(Space::new().width(4).into());
            items.push(chip);
        }
        items.push(Space::new().width(5).into());
        // When an address line is present, stack label + address vertically.
        let label_column: Element<'_, Message> = if let Some(addr) = address_row {
            iced::widget::Column::with_children(vec![label_text.into(), addr])
                .spacing(1.0)
                .width(if is_active { Length::Fill } else { Length::Shrink })
                .into()
        } else {
            label_text.into()
        };
        items.push(label_column);
        if close_on_right {
            // Push the trailing close button to the right edge.
            items.push(Space::new().width(Length::Fill).into());
            // Trailing slot reserves its width even when the X isn't
            // currently shown, so the label position doesn't jump on hover.
            let trailing_slot: Element<'_, Message> = if show_close {
                close_btn()
            } else {
                Space::new().width(TAB_ICON_SLOT).height(TAB_ICON_SLOT).into()
            };
            items.push(Space::new().width(4).into());
            items.push(trailing_slot);
        }
        crate::widgets::dir_row(items)
            .align_y(iced::Alignment::Center)
            .into()
    };

    // Inactive-tab separation style (issue #87): applies only to the
    // inactive, non-pinned chips (the active tab has its accent fill and
    // pinned tabs their own accent outline). `Border` rides the button's
    // own border; `Underline` is drawn as an edge overlay below.
    let inactive_style = if is_active || pinned {
        crate::views::tab_bar::InactiveTabStyle::None
    } else {
        crate::views::tab_bar::inactive_tab_style()
    };
    let inactive_border = inactive_style == crate::views::tab_bar::InactiveTabStyle::Border;

    let tab_btn = button(
        container(inner_row)
            .center_y(Length::Fixed(TAB_HEIGHT))
            .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 2.0 }),
    )
    .width(Length::Fixed(width))
    .clip(true)
    .on_press(Message::Tabs(TabsMessage::SelectTab(idx)))
    .style(move |_, status| {
        let hover_bg: Background = match status {
            BtnStatus::Hovered if !is_active => {
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
            }
            _ => bg,
        };
        // Full-style pinned tabs get a distinct accent outline; an
        // inactive chip under the Border style gets a subtle neutral one.
        let border = if pinned {
            Border { radius: Radius::from(6.0), color: effective_accent, width: 1.5 }
        } else if inactive_border {
            Border {
                radius: Radius::from(6.0),
                color: crate::views::tab_bar::InactiveTabStyle::Border.cue_color(),
                width: 1.0,
            }
        } else {
            Border { radius: Radius::from(6.0), ..Default::default() }
        };
        button::Style {
            background: Some(hover_bg),
            border,
            ..Default::default()
        }
    });

    // OSC 9;4 progress: a border that grows clockwise around the tab,
    // proportional to 0..100%. Layered over the button via a Stack; the canvas
    // doesn't handle input, so clicks still reach the button underneath.
    let tab_el: Element<'_, Message> = match progress {
        Some(p) if p.value > 0 => {
            let color = match p.state {
                2 => OryxisColors::t().error,         // error
                4 => Color::from_rgb(0.95, 0.66, 0.13), // warning (amber)
                _ => effective_accent,                // normal / indeterminate
            };
            let bar = iced::widget::canvas(TabProgressBorder {
                fraction: p.value as f32 / 100.0,
                color,
            })
            .width(Length::Fixed(width))
            .height(Length::Fixed(TAB_ROW_HEIGHT));
            iced::widget::Stack::new()
                .width(Length::Fixed(width))
                .height(Length::Fixed(TAB_ROW_HEIGHT))
                .push(tab_btn)
                .push(bar)
                .into()
        }
        _ => tab_btn.into(),
    };

    // Underline style: a neutral hairline on the chip's edge, laid over
    // the button so it never shifts the layout. Horizontal strips put it
    // on the content-facing edge; the vertical docks keep it horizontal
    // (see `inactive_edge_line`), where it separates stacked chips.
    let tab_el: Element<'_, Message> =
        if inactive_style == crate::views::tab_bar::InactiveTabStyle::Underline {
            iced::widget::Stack::new()
                .width(Length::Fixed(width))
                .height(Length::Fixed(TAB_ROW_HEIGHT))
                .push(tab_el)
                .push(inactive_edge_line(width, inactive_style.cue_color()))
                .into()
        } else {
            tab_el
        };

    MouseArea::new(tab_el)
        .on_enter(Message::Tabs(TabsMessage::TabHovered(idx)))
        .on_exit(Message::Tabs(TabsMessage::TabUnhovered))
        .on_right_press(Message::Tabs(TabsMessage::ShowTabMenu(idx)))
        .into()
}

/// Canvas that draws a tab's OSC 9;4 progress as a border filling clockwise
/// from the top-left, `fraction` of the perimeter (0..1).
struct TabProgressBorder {
    fraction: f32,
    color: Color,
}

impl iced::widget::canvas::Program<Message, iced::Theme> for TabProgressBorder {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{stroke, Frame, Path, Stroke};
        use iced::Point;
        use std::f32::consts::FRAC_PI_2;

        let mut frame = Frame::new(renderer, bounds.size());
        let t = 2.0_f32; // stroke thickness
        // Inset by half the stroke so the line sits fully inside the bounds,
        // and round the corners to the same 6 px radius as the tab button.
        let inset = t / 2.0;
        let (ox, oy) = (inset, inset);
        let w = (bounds.width - 2.0 * inset).max(0.0);
        let h = (bounds.height - 2.0 * inset).max(0.0);
        let r = 6.0_f32.min(w / 2.0).min(h / 2.0).max(0.0);

        let arc = FRAC_PI_2 * r; // length of one rounded corner
        let edge_top = (w - 2.0 * r).max(0.0);
        let edge_side = (h - 2.0 * r).max(0.0);
        let perim = 2.0 * edge_top + 2.0 * edge_side + 4.0 * arc;
        if perim <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let filled = (self.fraction.clamp(0.0, 1.0) * perim).min(perim);

        // Cumulative segment thresholds, clockwise from the top edge start.
        let (t1, t2) = (edge_top, edge_top + arc);
        let (t3, t4) = (t2 + edge_side, t2 + edge_side + arc);
        let (t5, t6) = (t4 + edge_top, t4 + edge_top + arc);
        let t7 = t6 + edge_side;
        // Point at perimeter distance `d` (handles edges + corner arcs).
        let point_at = |d: f32| -> Point {
            let on_arc = |cx: f32, cy: f32, base: f32, d0: f32| {
                let th = base + (d - d0) / r;
                Point::new(ox + cx + r * th.cos(), oy + cy + r * th.sin())
            };
            if d <= t1 {
                Point::new(ox + r + d, oy)
            } else if d <= t2 {
                on_arc(w - r, r, -FRAC_PI_2, t1)
            } else if d <= t3 {
                Point::new(ox + w, oy + r + (d - t2))
            } else if d <= t4 {
                on_arc(w - r, h - r, 0.0, t3)
            } else if d <= t5 {
                Point::new(ox + w - r - (d - t4), oy + h)
            } else if d <= t6 {
                on_arc(r, h - r, FRAC_PI_2, t5)
            } else if d <= t7 {
                Point::new(ox, oy + h - r - (d - t6))
            } else {
                on_arc(r, r, FRAC_PI_2 * 2.0, t7)
            }
        };

        // Trace the contour as a short-segment polyline (arcs approximated;
        // the corners are tiny so it reads as a smooth rounded border).
        let path = Path::new(|b| {
            b.move_to(point_at(0.0));
            let step = 1.5_f32;
            let mut d = step;
            while d < filled {
                b.line_to(point_at(d));
                d += step;
            }
            b.line_to(point_at(filled));
        });
        frame.stroke(
            &path,
            Stroke {
                style: stroke::Style::Solid(self.color),
                width: t,
                line_cap: stroke::LineCap::Round,
                line_join: stroke::LineJoin::Round,
                ..Stroke::default()
            },
        );
        vec![frame.into_geometry()]
    }
}

/// The hybrid mode chip: a small square bordered button showing the
/// tab's CURRENT surface (`>_` terminal / folder files); clicking flips
/// it. A real `button` (hover/press feedback per the house convention)
/// nested inside the tab button, same pattern as the close X; tooltip
/// since it's icon-only. Shared by the full tab and the pinned chip.
pub(crate) fn tab_mode_chip<'a>(idx: usize, fm: bool, fg: Color) -> Element<'a, Message> {
    const MODE_CHIP: f32 = 16.0;
    let glyph: Element<'a, Message> = if fm {
        iced_fonts::lucide::folder_tree().size(10).color(fg).into()
    } else {
        text(">_")
            .size(9)
            .line_height(1.0)
            .font(iced::Font::MONOSPACE)
            .color(fg)
            .into()
    };
    let chip = button(
        container(glyph)
            .center_x(Length::Fixed(MODE_CHIP))
            .center_y(Length::Fixed(MODE_CHIP)),
    )
    .padding(0)
    .on_press(Message::Tabs(TabsMessage::ToggleTabFilesMode(idx)))
    .style(move |_, status| {
        // The app's dark surface as the chip fill (owner QA: the
        // translucent accent wash read washed-out over the active
        // tab's gradient); hover keeps the standard bg_hover feedback.
        let bg = match status {
            BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
            _ => OryxisColors::t().bg_sidebar,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(4.0),
                color: Color { a: 0.35, ..fg },
                width: 1.0,
            },
            ..Default::default()
        }
    });
    let tip = if fm {
        crate::i18n::t("tab_show_terminal")
    } else {
        crate::i18n::t("tab_show_files")
    };
    iced::widget::tooltip(
        chip,
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

/// Width of a compact pinned chip: the base icon square, plus the mode
/// chip's slot when the tab is hybrid (has an SFTP session).
pub(crate) fn pinned_chip_width(files_mode: Option<bool>) -> f32 {
    if files_mode.is_some() {
        CHIP_W + 20.0
    } else {
        CHIP_W
    }
}

/// A small filled disc pinned to a right corner of the badge slot,
/// ringed with the sidebar background so it pops off the glyph under it.
/// Physical right on purpose: badges and their dots don't mirror under
/// RTL (same rule as the OS glyph itself).
fn corner_dot<'a>(
    color: Color,
    size: f32,
    ring: f32,
    y: iced::alignment::Vertical,
) -> Element<'a, Message> {
    let disc = container(Space::new().width(size).height(size)).style(move |_| {
        container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: Radius::from(size),
                color: OryxisColors::t().bg_sidebar,
                width: ring,
            },
            ..Default::default()
        }
    });
    container(disc)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(y)
        .into()
}

/// Compact (Chrome-style) pinned tab: an icon-only chip at a fixed width,
/// with the same OS / host / session-group badge as a full tab. Select on
/// click, right-click opens the same context menu (to unpin, etc.).
#[allow(clippy::too_many_arguments)]
pub(crate) fn pinned_tab_chip<'a>(
    idx: usize,
    detected_os: Option<&str>,
    is_active: bool,
    host_accent: Option<Color>,
    host_icon_style: crate::widgets::HostIconStyle,
    custom_icon: Option<&'a str>,
    custom_color: Option<Color>,
    status_dot: Option<Color>,
    attention_dot: Option<Color>,
    // `tab_accent_text` setting: when false the mode-chip glyph renders
    // in the theme's neutral text colours instead of the host accent.
    accent_text: bool,
    solid_fill: bool,
    // Hybrid tab: `Some(state)` widens the chip to carry the mode glyph
    // (a pinned hybrid must not lose its toggle).
    files_mode: Option<bool>,
) -> Element<'a, Message> {
    // Contrast validator (issue #79): the accent tints the mode-chip
    // glyph and the active gradient wash, both over the strip; the OS
    // badge below keeps the raw brand colour.
    let accent = crate::theme::readable_accent_on(
        host_accent.unwrap_or_else(|| OryxisColors::t().accent),
        OryxisColors::t().bg_sidebar,
    );
    let fallback = OryxisColors::t().accent;
    let (glyph, badge_color) = if let Some(name) = custom_icon {
        (crate::os_icon::custom_icon_glyph(name), custom_color.unwrap_or(fallback))
    } else {
        crate::os_icon::resolve_icon(detected_os, fallback)
    };
    let glyph_el: Element<'_, Message> = glyph.view(13.0, Color::WHITE);
    let base = crate::widgets::host_icon(host_icon_style, badge_color, "", Some(glyph_el), TAB_ICON_SLOT);
    let badge: Element<'_, Message> = if status_dot.is_some() || attention_dot.is_some() {
        let mut stack = iced::widget::Stack::new().push(
            container(base)
                .center_x(Length::Fixed(TAB_ICON_SLOT))
                .center_y(Length::Fixed(TAB_ICON_SLOT)),
        );
        if let Some(c) = status_dot {
            stack = stack.push(corner_dot(c, 6.0, 1.0, iced::alignment::Vertical::Bottom));
        }
        if let Some(c) = attention_dot {
            stack = stack.push(corner_dot(c, 6.0, 1.0, iced::alignment::Vertical::Top));
        }
        stack
            .width(Length::Fixed(TAB_ICON_SLOT))
            .height(Length::Fixed(TAB_ICON_SLOT))
            .into()
    } else {
        base
    };
    // A hybrid chip carries the mode glyph beside the badge (and widens
    // to fit); a plain pinned chip stays the icon-only square.
    let chip_w = pinned_chip_width(files_mode);
    let inner: Element<'_, Message> = match files_mode {
        Some(fm) => {
            let fg = if is_active {
                if accent_text { accent } else { OryxisColors::t().text_primary }
            } else {
                OryxisColors::t().text_muted
            };
            crate::widgets::dir_row(vec![
                badge,
                Space::new().width(4).into(),
                tab_mode_chip(idx, fm, fg),
            ])
            .align_y(iced::Alignment::Center)
            .into()
        }
        None => badge,
    };
    let btn = button(
        container(inner)
            .center_x(Length::Fixed(chip_w))
            .center_y(Length::Fixed(TAB_HEIGHT)),
    )
    .width(Length::Fixed(chip_w))
    .on_press(Message::Tabs(TabsMessage::SelectTab(idx)))
    .style(move |_, status| {
        // Active chip paints the same "lit from above" gradient as the
        // other tabs (one visual language for "active" in the strip);
        // the icon-only chip shape is already the pin affordance, so
        // the old 1.5 px accent outline just read as a different kind
        // of element.
        let bg = match status {
            _ if is_active => active_tab_bg(accent, solid_fill),
            BtnStatus::Hovered => Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06)),
            _ => Background::Color(Color::TRANSPARENT),
        };
        let border = Border { radius: Radius::from(6.0), ..Default::default() };
        button::Style { background: Some(bg), border, ..Default::default() }
    });
    MouseArea::new(btn)
        .on_enter(Message::Tabs(TabsMessage::TabHovered(idx)))
        .on_exit(Message::Tabs(TabsMessage::TabUnhovered))
        .on_right_press(Message::Tabs(TabsMessage::ShowTabMenu(idx)))
        .into()
}
