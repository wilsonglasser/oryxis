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
        const SQUARE: f32 = TAB_HEIGHT + 10.0;
        button(
            container(glyph.size(16).color(fg))
                .center_x(Length::Fixed(SQUARE))
                .center_y(Length::Fixed(SQUARE)),
        )
        .padding(0)
        .on_press(Message::ChangeView(view))
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
        .on_press(Message::ChangeView(view))
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
    label: &'a str,
    is_active: bool,
    width: f32,
    host_accent: Option<Color>,
    pinned: bool,
    solid_fill: bool,
) -> Element<'a, Message> {
    let effective_accent = host_accent.unwrap_or_else(|| OryxisColors::t().accent);
    let fg = if is_active {
        effective_accent
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
            background: Some(Background::Color(effective_accent)),
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
                    effective_accent
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
        .on_press(Message::CloseSftpTab(idx))
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
    .on_press(Message::SelectSftpTab(idx))
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
        .on_enter(Message::SftpTabHovered(idx))
        .on_exit(Message::SftpTabUnhovered)
        .on_right_press(Message::ShowSftpTabMenu(idx))
        .into()
}

/// Compact (Chrome-style) pinned SFTP tab: icon-only folder chip at a fixed
/// width. Select on click, right-click opens the context menu. Mirrors
/// `pinned_tab_chip` for the SFTP side.
pub(crate) fn sftp_pinned_chip<'a>(idx: usize, is_active: bool, host_accent: Option<Color>, solid_fill: bool) -> Element<'a, Message> {
    let accent = host_accent.unwrap_or_else(|| OryxisColors::t().accent);
    // Folder glyph (SFTP identity) tinted with the host color.
    let badge = container(iced_fonts::lucide::folder_tree().size(12).color(Color::WHITE))
        .center_x(Length::Fixed(TAB_ICON_SLOT))
        .center_y(Length::Fixed(TAB_ICON_SLOT))
        .style(move |_| container::Style {
            background: Some(Background::Color(accent)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        });
    let bg: Background = if is_active {
        active_tab_bg(accent, solid_fill)
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
    .on_press(Message::SelectSftpTab(idx))
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
        .on_enter(Message::SftpTabHovered(idx))
        .on_exit(Message::SftpTabUnhovered)
        .on_right_press(Message::ShowSftpTabMenu(idx))
        .into()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn session_tab<'a>(
    idx: usize,
    label: &'a str,
    pane_count: usize,
    is_active: bool,
    is_hovered: bool,
    detected_os: Option<&str>,
    width: f32,
    close_on_right: bool,
    status_dot: Option<Color>,
    host_accent: Option<Color>,
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
) -> Element<'a, Message> {
    let effective_accent = host_accent.unwrap_or_else(|| OryxisColors::t().accent);
    let fg = if is_active {
        effective_accent
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
    let display_label_full = label.trim_end_matches(" (disconnected)").to_string();
    // When the close X gets its own trailing slot, the label has less
    // horizontal room. Reserve the X's slot + a small gap so the
    // truncation kicks in earlier instead of the X clipping over the
    // last few characters.
    let label_width = if close_on_right {
        (width - TAB_ICON_SLOT - 4.0).max(0.0)
    } else {
        width
    };
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
        // Status dot (bottom-right) stacked over the badge says the
        // connection state. The split pane-count is a separate inline chip
        // after the icon (built below), so it stays legible instead of
        // crowding the glyph.
        if let Some(dot_color) = status_dot {
            let dot_disc = container(Space::new().width(7).height(7))
                .style(move |_| container::Style {
                    background: Some(Background::Color(dot_color)),
                    border: Border {
                        radius: Radius::from(4.0),
                        color: OryxisColors::t().bg_sidebar,
                        width: 1.5,
                    },
                    ..Default::default()
                });
            let dot_pin = container(dot_disc)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom);
            iced::widget::Stack::new()
                .push(base)
                .push(dot_pin)
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
        // Fixed square so a single digit renders as a true circle rather
        // than an oval. Two-digit counts (10+ panes) are vanishingly rare;
        // they'd just fill the disc a little tighter.
        const COUNT_DISC: f32 = 15.0;
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
            effective_accent
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
        .on_press(Message::CloseTab(idx))
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

    let label_text = text(display_label)
        .size(12)
        .line_height(1.0)
        .wrapping(iced::widget::text::Wrapping::None)
        .font(SYSTEM_UI_SEMIBOLD)
        .color(fg)
        .width(Length::Fill);

    let inner_row: Element<'_, Message> = {
        let mut items: Vec<Element<'_, Message>> = vec![leading_slot];
        // Pane-count chip sits just after the icon, offset by a small gap.
        if let Some(chip) = count_chip {
            items.push(Space::new().width(4).into());
            items.push(chip);
        }
        items.push(Space::new().width(5).into());
        items.push(label_text.into());
        if close_on_right {
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

    let tab_btn = button(
        container(inner_row)
            .center_y(Length::Fixed(TAB_HEIGHT))
            .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 2.0 }),
    )
    .width(Length::Fixed(width))
    .on_press(Message::SelectTab(idx))
    .style(move |_, status| {
        let hover_bg: Background = match status {
            BtnStatus::Hovered if !is_active => {
                Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
            }
            _ => bg,
        };
        // Full-style pinned tabs get a distinct accent outline.
        let border = if pinned {
            Border { radius: Radius::from(6.0), color: effective_accent, width: 1.5 }
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
            .height(Length::Fixed(TAB_HEIGHT));
            iced::widget::Stack::new()
                .width(Length::Fixed(width))
                .height(Length::Fixed(TAB_HEIGHT))
                .push(tab_btn)
                .push(bar)
                .into()
        }
        _ => tab_btn.into(),
    };

    MouseArea::new(tab_el)
        .on_enter(Message::TabHovered(idx))
        .on_exit(Message::TabUnhovered)
        .on_right_press(Message::ShowTabMenu(idx))
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
    solid_fill: bool,
) -> Element<'a, Message> {
    let accent = host_accent.unwrap_or_else(|| OryxisColors::t().accent);
    let fallback = OryxisColors::t().accent;
    let (glyph, badge_color) = if let Some(name) = custom_icon {
        (crate::os_icon::custom_icon_glyph(name), custom_color.unwrap_or(fallback))
    } else {
        crate::os_icon::resolve_icon(detected_os, fallback)
    };
    let glyph_el: Element<'_, Message> = glyph.view(13.0, Color::WHITE);
    let base = crate::widgets::host_icon(host_icon_style, badge_color, "", Some(glyph_el), TAB_ICON_SLOT);
    let badge: Element<'_, Message> = if let Some(dot_color) = status_dot {
        let dot_disc = container(Space::new().width(6).height(6)).style(move |_| container::Style {
            background: Some(Background::Color(dot_color)),
            border: Border { radius: Radius::from(3.0), color: OryxisColors::t().bg_sidebar, width: 1.0 },
            ..Default::default()
        });
        let dot_pin = container(dot_disc)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom);
        iced::widget::Stack::new()
            .push(
                container(base)
                    .center_x(Length::Fixed(TAB_ICON_SLOT))
                    .center_y(Length::Fixed(TAB_ICON_SLOT)),
            )
            .push(dot_pin)
            .width(Length::Fixed(TAB_ICON_SLOT))
            .height(Length::Fixed(TAB_ICON_SLOT))
            .into()
    } else {
        base
    };
    let btn = button(
        container(badge)
            .center_x(Length::Fixed(CHIP_W))
            .center_y(Length::Fixed(TAB_HEIGHT)),
    )
    .width(Length::Fixed(CHIP_W))
    .on_press(Message::SelectTab(idx))
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
        .on_enter(Message::TabHovered(idx))
        .on_exit(Message::TabUnhovered)
        .on_right_press(Message::ShowTabMenu(idx))
        .into()
}
