//! Tab bar + window chrome.
//!
//! Tabs render as pill-shaped chips with an OS-coloured icon badge on the
//! left that morphs into an X on hover/active (Termius-style close
//! affordance). The right-hand cluster, `[+]`, `[⋯]`, then the window
//! chrome (minimize / maximize / close), is pinned to the window edge and
//! never gets pushed off when many tabs are open. Tabs themselves shrink
//! uniformly to a minimum width as the bar fills, while the active tab
//! keeps its natural width so its label stays fully readable.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, container, row, scrollable, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::state::View;
use crate::theme::{OryxisColors, SYSTEM_UI_SEMIBOLD};

const TAB_HEIGHT: f32 = 26.0;
const TAB_ICON_SLOT: f32 = 16.0;

/// Maximum width a tab claims when it has the room. Sized to fit a typical
/// label like "user@hostname.example.com" without truncation. The active
/// tab always uses this; inactives only when there's space.
const TAB_NATURAL_WIDTH: f32 = 200.0;
/// Floor below which we don't shrink, once a tab gets this narrow the
/// label is mostly ellipses anyway, and going lower kills hit-target ergonomics.
/// Picked to fit "OS-badge + ~8 chars + ellipsis" comfortably.
const TAB_MIN_WIDTH: f32 = 110.0;

/// Approximate per-character width at the tab label's font/size combo
/// (12 px SemiBold). Used to figure out how many chars fit in a compact
/// tab before the truncation kicks in.
const TAB_CHAR_WIDTH: f32 = 7.0;

/// Spacing between tabs, extracted into a constant so the width math
/// can subtract it without drifting from the actual `row.spacing()`.
const TAB_SPACING: f32 = 6.0;

/// Total height of the tab bar. Sized to comfortably fit a session tab
/// (whose inner row already includes the OS-icon badge at 18 px plus
/// padding) without feeling cramped, and tall enough that the chrome
/// buttons read as proper hit targets when filled corner-to-corner.
const BAR_HEIGHT: f32 = 40.0;

const SIDEBAR_TOGGLE_WIDTH: f32 = 28.0;
// `+` and `⋯` (jump-to) live in the right cluster next to the chrome
// buttons, so they share the chrome width, gives the whole strip a
// uniform 46×BAR_HEIGHT cell rhythm.
const PLUS_BUTTON_WIDTH: f32 = 46.0;
const DOTS_BUTTON_WIDTH: f32 = 46.0;
const SEARCH_BUTTON_WIDTH: f32 = 46.0;
const CHROME_BUTTON_WIDTH: f32 = 46.0;
const CHROME_TOTAL_WIDTH: f32 = CHROME_BUTTON_WIDTH * 3.0;

impl Oryxis {
    pub(crate) fn view_tab_bar(&self) -> Element<'_, Message> {
        let n_tabs = self.tabs.len();
        let active_idx = self.active_tab;

        // For compaction we need a rough estimate of the strip's width
        // (active tab natural, inactives shrink to fit). The exact
        // value isn't critical, `scrollable` is the safety net for
        // any miscalculation. Subtract everything else on the row.
        // RIGHT_CLUSTER_WIDTH = +(28) + 2 + ⋯(28) + 2 + chrome(3*46)
        const RIGHT_CLUSTER_WIDTH: f32 = SEARCH_BUTTON_WIDTH
            + 2.0
            + PLUS_BUTTON_WIDTH
            + 2.0
            + DOTS_BUTTON_WIDTH
            + 2.0
            + CHROME_TOTAL_WIDTH;
        // Workspace mode prepends area tabs (Hosts, SFTP) that consume
        // strip width before the connection tabs even start; subtract
        // a rough estimate so the connection-tab allocator and the
        // scroll_mode trigger see the actual budget. Each area tab is
        // roughly icon(16) + gap(6) + label(~50) + padding(20) ~= 90 px.
        let area_tab_count = if self.setting_layout_mode == "workspace" {
            1 + (self.sftp_enabled as u32)
        } else {
            0
        };
        const AREA_TAB_APPROX_WIDTH: f32 = 100.0;
        let area_tabs_total = area_tab_count as f32
            * (AREA_TAB_APPROX_WIDTH + TAB_SPACING);
        // Burger menu button (SIDEBAR_TOGGLE_WIDTH) also lives on the
        // leading edge in Workspace mode; sidebar toggle is always
        // there in both modes.
        let burger_width = SIDEBAR_TOGGLE_WIDTH;
        // Logo only renders in Workspace mode; subtract its slot
        // (same SIDEBAR_TOGGLE_WIDTH as burger/toggle) so the strip
        // math stays aligned with the actual leading row.
        let logo_width = if self.setting_layout_mode == "workspace" {
            SIDEBAR_TOGGLE_WIDTH
        } else {
            0.0
        };
        let approx_strip_width = (self.window_size.width
            - SIDEBAR_TOGGLE_WIDTH
            - burger_width
            - logo_width
            - RIGHT_CLUSTER_WIDTH
            - area_tabs_total
            - 12.0)
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

        // Workspace mode promotes the navigation areas (Hosts and SFTP)
        // into top-level tabs that sit before the connection tabs. In
        // Classic mode the sidebar still owns navigation, so we skip
        // them. Settings stays out of the strip on purpose - it lives
        // in the burger menu so it doesn't take a permanent slot.
        if self.setting_layout_mode == "workspace" {
            let nav_active = self.active_tab.is_none();
            tab_items.push(area_tab(
                crate::i18n::t("hosts"),
                iced_fonts::lucide::server(),
                View::Dashboard,
                nav_active && self.active_view == View::Dashboard,
            ));
            if self.sftp_enabled {
                tab_items.push(area_tab(
                    crate::i18n::t("sftp"),
                    iced_fonts::lucide::folder_tree(),
                    View::Sftp,
                    nav_active && self.active_view == View::Sftp,
                ));
            }
        }

        for (idx, tab) in self.tabs.iter().enumerate() {
            let is_active = active_idx == Some(idx);
            let is_hovered = self.hovered_tab == Some(idx);
            let base_label = tab.label.trim_end_matches(" (disconnected)");
            let detected_os = self
                .connections
                .iter()
                .find(|c| c.label == base_label)
                .and_then(|c| c.detected_os.clone())
                // Fall back to deriving the OS from the tab label
                // for Local Shell tabs (`"Ubuntu (WSL)"`,
                // `"PowerShell"`, `"Command Prompt"`, …), those
                // never go through the SSH `detect_os` round-trip.
                .or_else(|| crate::os_icon::local_shell_os_hint(base_label));
            let width = if is_active { active_width } else { inactive_width };
            // Per-host accent override: when this tab points at a
            // saved connection that has a custom `color`, tint the
            // active-tab fill and the tab text with that color
            // (JetBrains-style "respiração"). Otherwise the active
            // tab keeps the global accent.
            let base_label = tab.label.trim_end_matches(" (disconnected)");
            let host_accent: Option<Color> = self.connections.iter()
                .find(|c| c.label == base_label)
                .and_then(|c| c.color.as_deref())
                .and_then(crate::widgets::parse_hex_color);
            // Resolve the per-host icon style override against the
            // global default so the badge shape on the tab matches
            // the one on the dashboard card. Local-shell tabs (no
            // matching connection row) fall through to the global
            // default like fresh hosts do.
            let host_icon_style = {
                let per_host = self.connections.iter()
                    .find(|c| c.label == base_label)
                    .and_then(|c| c.icon_style.as_deref());
                crate::widgets::resolve_host_icon_style(per_host, &self.setting_default_host_icon)
            };
            // Connection-state dot color. Connecting beats every other
            // signal because a tab that's currently dialing isn't yet
            // "disconnected" in the user's mental model. Local-shell
            // tabs (no SSH session, not labeled disconnected) get no
            // dot, the OS badge already says what they are.
            let status_dot: Option<Color> = if self.setting_show_tab_status_dot {
                let is_connecting = self.connecting.as_ref().map(|cp| cp.tab_idx) == Some(idx);
                let is_disconnected = tab.label.ends_with(" (disconnected)");
                let is_ssh = tab.active().ssh_session.is_some();
                if is_connecting {
                    Some(OryxisColors::t().warning)
                } else if is_disconnected {
                    Some(OryxisColors::t().error)
                } else if is_ssh {
                    Some(OryxisColors::t().success)
                } else {
                    None
                }
            } else {
                None
            };
            tab_items.push(session_tab(
                idx,
                &tab.label,
                is_active,
                is_hovered,
                detected_os.as_deref(),
                width,
                self.setting_tab_close_button_side == "right",
                status_dot,
                host_accent,
                host_icon_style,
            ));
        }

        // The tab strip lives in an auto-width container, Length::Fill
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
        // Native title-bar convention: double-click the drag area to
        // toggle maximize.
        .on_double_click(Message::WindowMaximizeToggle)
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

        // Right cluster, never pushed off. Always contains `+` and the
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
        // Window controls live in their own dir_row so the close button
        // ends up on the leading edge under RTL, matches how macOS and
        // GNOME flip traffic-light buttons when the locale flips.
        let chrome_row = crate::widgets::dir_row(vec![
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
        ])
        .align_y(iced::Alignment::Center);

        // The right cluster sits on the trailing edge of the tab bar.
        // Build it in reading order ([extras] then chrome) and let
        // `dir_row` flip the order in RTL so chrome lands on the
        // outer edge there too.
        let mut cluster_items: Vec<Element<'_, Message>> = Vec::new();
        cluster_items.push(search_btn());
        cluster_items.push(Space::new().width(2).into());
        if let Some(dots) = dots_btn {
            cluster_items.push(dots);
            cluster_items.push(Space::new().width(2).into());
        }
        cluster_items.push(plus_btn);
        cluster_items.push(Space::new().width(2).into());
        cluster_items.push(chrome_row.into());
        let right_cluster: Element<'_, Message> = crate::widgets::dir_row(cluster_items)
            .align_y(iced::Alignment::Center)
            .into();

        let sidebar_toggle =
            super::sidebar::sidebar_toggle_btn(!self.sidebar_collapsed);

        // Burger menu trigger. Mirrors the Termius `☰` strip on the
        // leading edge: opens a top-left dropdown with Settings,
        // Updates, About, Exit.
        let burger_btn = burger_menu_btn(self.show_burger_menu);

        // Workspace mode also shows the Oryxis product logo on the
        // far leading edge so the chrome carries product identity in
        // the JetBrains style. Classic mode keeps the logo on the
        // sidebar header where it always lived.
        let mut leading: Vec<Element<'_, Message>> = Vec::new();
        if self.setting_layout_mode == "workspace" {
            leading.push(product_logo(self.logo_small_handle.clone()));
        }
        leading.push(burger_btn);
        leading.push(sidebar_toggle);
        leading.push(tab_strip);
        leading.push(right_cluster);

        // Four-block row: [logo?] [burger] [sidebar_toggle] [tab_strip(Fill)] [right_cluster].
        // Burger / sidebar_toggle / right_cluster are Length::Shrink so iced
        // gives them their content width first; tab_strip is the remaining
        // Fill area in between. `dir_row` flips the row under RTL so the
        // leading-edge controls always sit next to the sidebar (which the
        // outer layout also flips to the trailing edge).
        container(
            crate::widgets::dir_row(leading)
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
    /// connecting an SSH session, etc.), without this the new tab
    /// can land off-screen when the strip is in scroll mode.
    pub(crate) fn tab_scroll_to_active(&self) -> iced::Task<Message> {
        let Some(active_idx) = self.active_tab else {
            return iced::Task::none();
        };
        if self.tabs.is_empty() {
            return iced::Task::none();
        }
        // Mirror the layout math in view_tab_bar so the offsets line up,
        // including the burger button + area tabs that Workspace mode
        // prepends to the strip.
        const RIGHT_CLUSTER_WIDTH: f32 = SEARCH_BUTTON_WIDTH
            + 2.0
            + PLUS_BUTTON_WIDTH
            + 2.0
            + DOTS_BUTTON_WIDTH
            + 2.0
            + CHROME_TOTAL_WIDTH;
        let area_tab_count = if self.setting_layout_mode == "workspace" {
            1 + (self.sftp_enabled as u32)
        } else {
            0
        };
        const AREA_TAB_APPROX_WIDTH: f32 = 100.0;
        let area_tabs_total = area_tab_count as f32
            * (AREA_TAB_APPROX_WIDTH + TAB_SPACING);
        let burger_width = SIDEBAR_TOGGLE_WIDTH;
        // Logo only renders in Workspace mode; subtract its slot
        // (same SIDEBAR_TOGGLE_WIDTH as burger/toggle) so the strip
        // math stays aligned with the actual leading row.
        let logo_width = if self.setting_layout_mode == "workspace" {
            SIDEBAR_TOGGLE_WIDTH
        } else {
            0.0
        };
        let approx_strip_width = (self.window_size.width
            - SIDEBAR_TOGGLE_WIDTH
            - burger_width
            - logo_width
            - RIGHT_CLUSTER_WIDTH
            - area_tabs_total
            - 12.0)
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
        let tab_width = usable.clamp(TAB_MIN_WIDTH, TAB_NATURAL_WIDTH);
        return (tab_width, tab_width);
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
///
/// `close_on_right`: when true the close X gets its own slot at the
/// trailing edge of the tab and the OS badge always stays on the
/// leading edge. When false (the default, Termius-style), the X
/// replaces the OS badge in the leading slot on hover/active.
///
/// `status_dot`: when Some, a small filled circle of that color is
/// stacked over the OS badge's bottom-right corner. None hides the
/// dot entirely (local-shell tabs and users who disabled the setting).
///
/// `host_accent`: per-host accent color resolved from `Connection.color`.
/// When Some, the active-tab fill and label adopt this color instead of
/// the global accent, so each tab "breathes" the color of its host.
///
/// `host_icon_style`: shape the OS badge takes in this tab. Resolved
/// from the per-host override or the global `default_host_icon`
/// setting; defaults to Square here (back-compat with the previous
/// fixed shape) when the caller passes nothing custom.
/// Area tab: navigation entry (Hosts, SFTP, ...) rendered into the
/// top tab strip in Workspace mode. Same height + bg as a session
/// tab so the strip reads as one continuous row, but with a leading
/// glyph instead of a host badge and no close affordance (areas
/// can't be closed). Dispatches `ChangeView` so the existing
/// navigation handler picks it up.
fn area_tab<'a>(
    label: &'a str,
    glyph: iced::widget::Text<'a>,
    view: View,
    is_active: bool,
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
    button(
        container(
            row![
                container(glyph.size(14).color(fg))
                    .center_x(Length::Fixed(TAB_ICON_SLOT))
                    .center_y(Length::Fixed(TAB_ICON_SLOT)),
                Space::new().width(6),
                text(label)
                    .size(12)
                    .line_height(1.0)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .font(SYSTEM_UI_SEMIBOLD)
                    .color(fg),
            ]
            .align_y(iced::Alignment::Center),
        )
        .center_y(Length::Fixed(TAB_HEIGHT))
        .padding(Padding { top: 0.0, right: 10.0, bottom: 0.0, left: 6.0 }),
    )
    .on_press(Message::ChangeView(view))
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
    })
    .into()
}

#[allow(clippy::too_many_arguments)]
fn session_tab<'a>(
    idx: usize,
    label: &'a str,
    is_active: bool,
    is_hovered: bool,
    detected_os: Option<&str>,
    width: f32,
    close_on_right: bool,
    status_dot: Option<Color>,
    host_accent: Option<Color>,
    host_icon_style: crate::widgets::HostIconStyle,
) -> Element<'a, Message> {
    let effective_accent = host_accent.unwrap_or_else(|| OryxisColors::t().accent);
    let fg = if is_active {
        effective_accent
    } else {
        OryxisColors::t().text_muted
    };
    let bg = if is_active {
        Color { a: 0.15, ..effective_accent }
    } else {
        Color::TRANSPARENT
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
        let (glyph, mut badge_color) = crate::os_icon::resolve_icon(detected_os, fallback);
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
        if let Some(dot_color) = status_dot {
            // Small filled circle stacked over the badge's bottom-right
            // corner. The Stack child uses Length::Fill so we can pin
            // it with align_x + align_y; the visible disc is the inner
            // container's actual 8 px square with a 50% radius.
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
    let close_btn = || -> Element<'_, Message> {
        MouseArea::new(
            container(
                iced_fonts::lucide::x()
                    .size(11)
                    .color(if is_active {
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

    let inner_row: Element<'_, Message> = if close_on_right {
        // Trailing slot reserves its width even when the X isn't
        // currently shown, so the label position doesn't jump on hover.
        let trailing_slot: Element<'_, Message> = if show_close {
            close_btn()
        } else {
            Space::new().width(TAB_ICON_SLOT).height(TAB_ICON_SLOT).into()
        };
        row![
            leading_slot,
            Space::new().width(5),
            label_text,
            Space::new().width(4),
            trailing_slot,
        ]
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        row![
            leading_slot,
            Space::new().width(5),
            label_text,
        ]
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

/// Plus button at the end of the tab row, opens the new-tab picker
/// (search + recent connections) as a centered modal overlay. Width +
/// height + border match the window-chrome buttons next to it so the
/// whole right cluster reads as one strip; `PLUS_BUTTON_WIDTH` was
/// only used for the layout-math `RIGHT_CLUSTER_WIDTH` calculation,
/// which still applies because we publish the same constant value.
///
/// Uses `lucide::plus` instead of a literal `+` text character, on
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

/// Tab-jump button, opens the Termius-style "Jump to" modal listing
/// all open tabs + Quick connect entries. Always visible regardless of
/// how many tabs are open, so the user has a discoverable escape hatch
/// from a packed tab strip.
fn tab_jump_btn<'a>() -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    button(
        container(
            text("\u{22EF}") // horizontal ellipsis ⋯
                .size(15)
                .color(hover_color),
        )
        .center(Length::Fixed(DOTS_BUTTON_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::ShowTabJump)
    .padding(0)
    .style(move |_, status| {
        // Match the window-chrome / new-tab buttons' subtle hover
        // and squared corners so the right cluster reads as one
        // continuous strip.
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

/// Global host search trigger. Opens the same overlay Ctrl+K / Ctrl+F1
/// open, just as a click affordance for users who prefer the chrome
/// over keyboard shortcuts. Lives at the leading edge of the right
/// cluster so it reads as a peer to `+ new tab` and the chrome
/// buttons next to it.
fn search_btn<'a>() -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    button(
        container(
            iced_fonts::lucide::search().size(14).color(hover_color),
        )
        .center(Length::Fixed(SEARCH_BUTTON_WIDTH))
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

/// Product logo on the leading edge of the tab bar (Workspace mode
/// only). Same trick JetBrains uses: a small product mark anchored
/// in the chrome carries identity even though the whole window is
/// otherwise unbranded. Sized to match the burger / sidebar-toggle
/// neighbours so the strip reads as one uniform row of controls.
fn product_logo<'a>(handle: iced::widget::image::Handle) -> Element<'a, Message> {
    container(
        iced::widget::image(handle)
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(22.0)),
    )
    .center_x(Length::Fixed(SIDEBAR_TOGGLE_WIDTH))
    .center_y(Length::Fixed(BAR_HEIGHT))
    .into()
}

/// Burger menu trigger at the leading edge of the tab bar. When the
/// menu is open the button paints with the accent hover state so the
/// click affordance reads as "active control" instead of a stray glyph.
fn burger_menu_btn<'a>(is_open: bool) -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    let resting_bg = if is_open {
        Color { a: 0.2, ..hover_color }
    } else {
        Color::TRANSPARENT
    };
    button(
        container(
            iced_fonts::lucide::menu().size(15).color(hover_color),
        )
        .center(Length::Fixed(SIDEBAR_TOGGLE_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::ToggleBurgerMenu)
    .padding(0)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.2, ..hover_color },
            BtnStatus::Pressed => Color { a: 0.35, ..hover_color },
            _ => resting_bg,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            ..Default::default()
        }
    })
    .into()
}

/// Minimize / maximize / close glyph button for the window chrome.
/// Fills the full bar height (no padding) so hover backgrounds reach the
/// very top and bottom edges, same behaviour as Windows / VS Code.
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
