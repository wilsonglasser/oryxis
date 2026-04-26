//! Tab bar + window chrome.
//!
//! Tabs render as pill-shaped chips with an OS-coloured icon badge on the
//! left that morphs into an X on hover/active (Termius-style close
//! affordance). The right-hand cluster — `[+]`, `[⋯]`, then the window
//! chrome (minimize / maximize / close) — is pinned to the window edge and
//! never gets pushed off when many tabs are open. Tabs themselves shrink
//! uniformly to a minimum width as the bar fills, while the active tab
//! keeps its natural width so its label stays fully readable.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, row, scrollable, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::{OryxisColors, SYSTEM_UI_SEMIBOLD};

const TAB_HEIGHT: f32 = 26.0;
const TAB_ICON_SLOT: f32 = 16.0;

/// Maximum width a tab claims when it has the room. Sized to fit a typical
/// label like "user@hostname.example.com" without truncation. The active
/// tab always uses this; inactives only when there's space.
const TAB_NATURAL_WIDTH: f32 = 200.0;
/// Floor below which we don't shrink — once a tab gets this narrow the
/// label is mostly ellipses anyway, and going lower kills hit-target ergonomics.
/// Picked to fit "OS-badge + ~8 chars + ellipsis" comfortably.
const TAB_MIN_WIDTH: f32 = 110.0;

/// Approximate per-character width at the tab label's font/size combo
/// (12 px SemiBold). Used to figure out how many chars fit in a compact
/// tab before the truncation kicks in.
const TAB_CHAR_WIDTH: f32 = 7.0;

/// Spacing between tabs — extracted into a constant so the width math
/// can subtract it without drifting from the actual `row.spacing()`.
const TAB_SPACING: f32 = 6.0;

/// Total height of the tab bar. Sized to comfortably fit a session tab
/// (whose inner row already includes the OS-icon badge at 18 px plus
/// padding) without feeling cramped, and tall enough that the chrome
/// buttons read as proper hit targets when filled corner-to-corner.
const BAR_HEIGHT: f32 = 40.0;

const SIDEBAR_TOGGLE_WIDTH: f32 = 28.0;
// `+` and `⋯` (jump-to) live in the right cluster next to the chrome
// buttons, so they share the chrome width — gives the whole strip a
// uniform 46×BAR_HEIGHT cell rhythm.
const PLUS_BUTTON_WIDTH: f32 = 46.0;
const DOTS_BUTTON_WIDTH: f32 = 46.0;
const CHROME_BUTTON_WIDTH: f32 = 46.0;
const CHROME_TOTAL_WIDTH: f32 = CHROME_BUTTON_WIDTH * 3.0;

impl Oryxis {
    pub(crate) fn view_tab_bar(&self) -> Element<'_, Message> {
        let n_tabs = self.tabs.len();
        let active_idx = self.active_tab;

        // For compaction we need a rough estimate of the strip's width
        // (active tab natural, inactives shrink to fit). The exact
        // value isn't critical — `scrollable` is the safety net for
        // any miscalculation. Subtract everything else on the row.
        // RIGHT_CLUSTER_WIDTH = +(28) + 2 + ⋯(28) + 2 + chrome(3*46)
        const RIGHT_CLUSTER_WIDTH: f32 = PLUS_BUTTON_WIDTH
            + 2.0
            + DOTS_BUTTON_WIDTH
            + 2.0
            + CHROME_TOTAL_WIDTH;
        let approx_strip_width =
            (self.window_size.width - SIDEBAR_TOGGLE_WIDTH - RIGHT_CLUSTER_WIDTH - 12.0)
                .max(120.0);

        // Per-tab width allocation. The active tab always wants its natural
        // width so its label stays readable; the inactives split whatever's
        // left. With many tabs they shrink uniformly down to TAB_MIN_WIDTH.
        let (active_width, inactive_width) =
            allocate_tab_widths(n_tabs, approx_strip_width);

        // Scroll-mode trigger: tabs at their natural width plus
        // inter-tab spacing wouldn't fit in the strip. Same shape as
        // `container - (tabs_total + margin*(n-1)) < 0`.
        let n_f = n_tabs as f32;
        let natural_total =
            n_f * TAB_NATURAL_WIDTH + (n_f - 1.0).max(0.0) * TAB_SPACING;
        let scroll_mode = natural_total > approx_strip_width;

        let mut tab_items: Vec<Element<'_, Message>> = Vec::new();
        for (idx, tab) in self.tabs.iter().enumerate() {
            let is_active = active_idx == Some(idx);
            let is_hovered = self.hovered_tab == Some(idx);
            let base_label = tab.label.trim_end_matches(" (disconnected)");
            let detected_os = self
                .connections
                .iter()
                .find(|c| c.label == base_label)
                .and_then(|c| c.detected_os.clone());
            let width = if is_active { active_width } else { inactive_width };
            tab_items.push(session_tab(
                idx,
                &tab.label,
                is_active,
                is_hovered,
                detected_os.as_deref(),
                width,
            ));
        }

        // The tab strip lives in an auto-width container — Length::Fill
        // so the row gives it whatever's left after the sidebar toggle
        // and right cluster claim their Shrink widths. The scrollable
        // inside is the safety net: tabs that don't fit at min width
        // overflow into a horizontal scroll (mouse-wheel works, the
        // scrollbar itself is zeroed out so it's invisible). The
        // surrounding MouseArea makes the empty area of the strip a
        // window-drag handle, since we no longer have a separate drag
        // sibling in the row.
        let tab_strip_inner = scrollable(
            row(tab_items)
                .spacing(TAB_SPACING)
                .align_y(iced::Alignment::Center),
        )
        .id(iced::widget::Id::new("tab-scroll"))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(0.0)
                .scroller_width(0.0),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT));

        let tab_strip: Element<'_, Message> = MouseArea::new(
            container(tab_strip_inner)
                .width(Length::Fill)
                .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 6.0 }),
        )
        .on_press(Message::WindowDrag)
        // Vertical wheel translates to horizontal scroll on the tab
        // strip. The horizontal scrollable inside doesn't capture a
        // pure-y wheel event (iced only steers wheel along the
        // direction the scrollable can actually scroll), so this
        // MouseArea picks it up and routes a scroll_by command.
        .on_scroll(|delta| {
            let y = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            Message::TabBarWheel(y)
        })
        .into();

        // Right cluster — never pushed off. Always contains `+` and the
        // window chrome; the `⋯` jump-to button only joins when the bar
        // is actually overflowing (scroll_mode), positioned BEFORE the
        // `+` so the natural reading order is "scroll-related controls,
        // then add a new tab, then window chrome".
        let plus_btn = new_tab_btn();
        let dots_btn: Option<Element<'_, Message>> =
            if scroll_mode { Some(tab_jump_btn()) } else { None };

        let max_icon = if self.window_maximized {
            iced_fonts::codicon::chrome_restore()
        } else {
            iced_fonts::codicon::chrome_maximize()
        };
        let chrome_row = row![
            window_btn(
                iced_fonts::codicon::chrome_minimize(),
                Message::WindowMinimize,
                OryxisColors::t().text_secondary,
            ),
            window_btn(
                max_icon,
                Message::WindowMaximizeToggle,
                OryxisColors::t().text_secondary,
            ),
            window_btn(
                iced_fonts::codicon::chrome_close(),
                Message::WindowClose,
                OryxisColors::t().error,
            ),
        ]
        .align_y(iced::Alignment::Center);

        let mut right_row = row![].align_y(iced::Alignment::Center);
        if let Some(dots) = dots_btn {
            right_row = right_row
                .push(dots)
                .push(Space::new().width(2));
        }
        right_row = right_row
            .push(plus_btn)
            .push(Space::new().width(2))
            .push(chrome_row);
        let right_cluster: Element<'_, Message> = right_row.into();

        let sidebar_toggle =
            super::sidebar::sidebar_toggle_btn(!self.sidebar_collapsed);

        // Three-block row: [sidebar_toggle] [tab_strip(Fill)] [right_cluster].
        // sidebar_toggle and right_cluster are Length::Shrink so iced
        // gives them their content width first; tab_strip is the
        // remaining Fill area in between. No manual width math, no
        // separate drag region — the strip's MouseArea handles drag.
        container(
            row![sidebar_toggle, tab_strip, right_cluster]
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            ..Default::default()
        })
        .into()
    }
}

impl Oryxis {
    /// Build a task that snaps the tab strip's scrollable so the active
    /// tab is roughly centered in the visible area. Called whenever a
    /// new tab gets focused (manual select, opening a local shell,
    /// connecting an SSH session, etc.) — without this the new tab
    /// can land off-screen when the strip is in scroll mode.
    pub(crate) fn tab_scroll_to_active(&self) -> iced::Task<Message> {
        let Some(active_idx) = self.active_tab else {
            return iced::Task::none();
        };
        if self.tabs.is_empty() {
            return iced::Task::none();
        }
        // Mirror the layout math in view_tab_bar so the offsets line up.
        const RIGHT_CLUSTER_WIDTH: f32 = PLUS_BUTTON_WIDTH
            + 2.0
            + DOTS_BUTTON_WIDTH
            + 2.0
            + CHROME_TOTAL_WIDTH;
        let approx_strip_width =
            (self.window_size.width - SIDEBAR_TOGGLE_WIDTH - RIGHT_CLUSTER_WIDTH - 12.0)
                .max(120.0);
        let (active_w, inactive_w) =
            allocate_tab_widths(self.tabs.len(), approx_strip_width);
        // Sum widths of all tabs that come before the active one, plus
        // the spacing between them. Active is reached at this offset.
        let preceding = active_idx as f32;
        let mut x = preceding * (inactive_w + TAB_SPACING);
        // Center active in viewport instead of left-aligning so the
        // user has context (the previous + next tabs visible too).
        x = (x - approx_strip_width / 2.0 + active_w / 2.0).max(0.0);
        iced::widget::operation::scroll_to(
            iced::widget::Id::new("tab-scroll"),
            iced::widget::scrollable::AbsoluteOffset { x, y: 0.0 },
        )
    }
}

/// Decide how much horizontal space each tab gets. Returns
/// `(active_width, inactive_width)`. The active tab claims its natural
/// width when it fits; inactives split whatever's left, clamped to the
/// minimum so they don't disappear.
fn allocate_tab_widths(n: usize, available: f32) -> (f32, f32) {
    if n == 0 {
        return (0.0, 0.0);
    }
    let n_f = n as f32;
    let total_spacing = TAB_SPACING * (n_f - 1.0).max(0.0);
    let usable = (available - total_spacing).max(0.0);
    if n == 1 {
        return (usable.clamp(TAB_MIN_WIDTH, TAB_NATURAL_WIDTH), 0.0);
    }
    // Try natural for active + share rest among inactives.
    let active_target = TAB_NATURAL_WIDTH.min(usable);
    let remaining = (usable - active_target).max(0.0);
    let inactive = (remaining / (n_f - 1.0)).clamp(TAB_MIN_WIDTH, TAB_NATURAL_WIDTH);
    // If the inactives end up wider than the active (because total fits
    // generously), level them up so everything reads at the same width.
    let active = active_target.max(inactive);
    (active, inactive)
}

/// Truncate a label to fit visually within `width` px at the tab font
/// size. Falls back to a single character + ellipsis on extreme shrink
/// so the user still sees something.
fn truncate_label(label: &str, width: f32) -> String {
    let reserved = TAB_ICON_SLOT + 5.0 + 4.0 + 4.0; // icon + gap + padding
    let usable = (width - reserved).max(0.0);
    let max_chars = (usable / TAB_CHAR_WIDTH).floor() as usize;
    if max_chars == 0 {
        return String::new();
    }
    let chars: Vec<char> = label.chars().collect();
    if chars.len() <= max_chars {
        return label.to_string();
    }
    let cut: String = chars
        .iter()
        .take(max_chars.saturating_sub(1))
        .collect();
    format!("{}…", cut)
}

/// Session tab: icon badge (host icon by default, X on hover) + label.
/// Width is fixed by the caller so the row layout adapts to overflow.
fn session_tab<'a>(
    idx: usize,
    label: &'a str,
    is_active: bool,
    is_hovered: bool,
    detected_os: Option<&str>,
    width: f32,
) -> Element<'a, Message> {
    let fg = if is_active {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_muted
    };
    let bg = if is_active {
        Color { a: 0.15, ..OryxisColors::t().accent }
    } else {
        Color::TRANSPARENT
    };

    let is_disconnected = label.ends_with(" (disconnected)");
    let display_label_full = label.trim_end_matches(" (disconnected)").to_string();
    let display_label = truncate_label(&display_label_full, width);

    let show_close = is_active || is_hovered;
    let icon_slot: Element<'_, Message> = if show_close {
        MouseArea::new(
            container(
                iced_fonts::lucide::x()
                    .size(11)
                    .color(if is_active {
                        OryxisColors::t().accent
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
        .on_press(Message::CloseTab(idx))
        .into()
    } else {
        let fallback = if is_disconnected {
            OryxisColors::t().text_muted
        } else {
            OryxisColors::t().accent
        };
        let (glyph, mut badge_color) = crate::os_icon::resolve_icon(detected_os, fallback);
        if is_disconnected {
            badge_color = OryxisColors::t().text_muted;
        }
        container(glyph.size(10).color(Color::WHITE))
            .center_x(Length::Fixed(TAB_ICON_SLOT))
            .center_y(Length::Fixed(TAB_ICON_SLOT))
            .style(move |_| container::Style {
                background: Some(Background::Color(badge_color)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            })
            .into()
    };

    let tab_btn = button(
        container(
            row![
                icon_slot,
                Space::new().width(5),
                text(display_label)
                    .size(12)
                    .line_height(1.0)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .font(SYSTEM_UI_SEMIBOLD)
                    .color(fg),
            ]
            .align_y(iced::Alignment::Center),
        )
        .center_y(Length::Fixed(TAB_HEIGHT))
        .padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 2.0 }),
    )
    .width(Length::Fixed(width))
    .on_press(Message::SelectTab(idx))
    .style(move |_, status| {
        let hover_bg = match status {
            BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            _ => bg,
        };
        button::Style {
            background: Some(Background::Color(hover_bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });

    MouseArea::new(tab_btn)
        .on_enter(Message::TabHovered(idx))
        .on_exit(Message::TabUnhovered)
        .on_right_press(Message::ShowTabMenu(idx))
        .into()
}

/// Plus button at the end of the tab row — opens the new-tab picker
/// (search + recent connections) as a centered modal overlay. Width +
/// height + border match the window-chrome buttons next to it so the
/// whole right cluster reads as one strip; `PLUS_BUTTON_WIDTH` was
/// only used for the layout-math `RIGHT_CLUSTER_WIDTH` calculation,
/// which still applies because we publish the same constant value.
///
/// Uses `lucide::plus` instead of a literal `+` text character — on
/// Windows, Segoe UI's `+` renders much chunkier than the codicon
/// `−` / `□` / `✕` glyphs right next to it, breaking visual rhythm.
fn new_tab_btn<'a>() -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    button(
        container(iced_fonts::lucide::plus().size(15).color(hover_color))
            .center(Length::Fixed(PLUS_BUTTON_WIDTH))
            .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::ShowNewTabPicker)
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .into()
}

/// Tab-jump button — opens the Termius-style "Jump to" modal listing
/// all open tabs + Quick connect entries. Always visible regardless of
/// how many tabs are open, so the user has a discoverable escape hatch
/// from a packed tab strip.
fn tab_jump_btn<'a>() -> Element<'a, Message> {
    button(
        container(
            text("\u{22EF}") // horizontal ellipsis ⋯
                .size(15)
                .color(OryxisColors::t().text_muted),
        )
        .center_x(Length::Fixed(DOTS_BUTTON_WIDTH))
        .center_y(Length::Fixed(TAB_HEIGHT)),
    )
    .on_press(Message::ShowTabJump)
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Minimize / maximize / close glyph button for the window chrome.
/// Fills the full bar height (no padding) so hover backgrounds reach the
/// very top and bottom edges — same behaviour as Windows / VS Code.
fn window_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    hover_color: Color,
) -> Element<'a, Message> {
    button(
        container(icon.size(15).color(OryxisColors::t().text_secondary))
            .center(Length::Fixed(CHROME_BUTTON_WIDTH))
            .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(msg)
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .into()
}
