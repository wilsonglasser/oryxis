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
/// Fixed width of a compact (Chrome-style) pinned tab chip.
const CHIP_W: f32 = 38.0;

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
const SIDEBAR_BUTTON_WIDTH: f32 = 46.0;
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
        const RIGHT_CLUSTER_WIDTH: f32 = SIDEBAR_BUTTON_WIDTH
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
        // Burger menu button (SIDEBAR_TOGGLE_WIDTH) lives on the
        // leading edge in both modes. The sidebar collapse toggle
        // only renders in Classic mode (Workspace has no sidebar to
        // collapse). Logo only renders in Workspace.
        let burger_width = SIDEBAR_TOGGLE_WIDTH;
        let workspace_mode = self.setting_layout_mode == "workspace";
        let toggle_width = if workspace_mode { 0.0 } else { SIDEBAR_TOGGLE_WIDTH };
        let logo_width = if workspace_mode { SIDEBAR_TOGGLE_WIDTH } else { 0.0 };
        let approx_strip_width = (self.window_size.width
            - toggle_width
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
            // The "Hosts" area tab stays selected across every vault
            // sub-section (Hosts, Keychain, Snippets, Port Forwarding,
            // History), not just the Hosts grid. Those sub-sections
            // live under the contextual sub-nav of this same area, so
            // mirror the `in_vault_area` family used in `layout.rs`.
            let in_vault_area = matches!(
                self.active_view,
                View::Dashboard
                    | View::Keys
                    | View::Snippets
                    | View::PortForwarding
                    | View::History
            );
            tab_items.push(area_tab(
                crate::i18n::t("hosts"),
                iced_fonts::lucide::server(),
                View::Dashboard,
                nav_active && in_vault_area,
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

        // Pinned tabs render first. This is a visual reorder only: the
        // underlying `self.tabs` Vec and every tab's index are unchanged, so
        // SelectTab / CloseTab and the routing stay valid.
        let compact_pins = self.setting_pinned_tab_style == "compact";
        // While a drag is active every tab renders at the inactive width so
        // the strip geometry is uniform. The active-vs-inactive width
        // difference otherwise shifts positions on each live-slide swap and
        // bounces the dragged tab back and forth over a seam.
        let dragging_any = self.tab_drag.map(|d| d.active).unwrap_or(false);
        let mut tab_order: Vec<usize> =
            (0..self.tabs.len()).filter(|&i| self.tabs[i].pinned).collect();
        tab_order.extend((0..self.tabs.len()).filter(|&i| !self.tabs[i].pinned));
        for idx in tab_order {
            let tab = &self.tabs[idx];
            let is_active = active_idx == Some(idx);
            let is_hovered = self.hovered_tab == Some(idx);
            // Reorder drag: the dragged tab gets the accent outline so the
            // user sees which one they picked up.
            let is_dragging = self
                .tab_drag
                .filter(|d| d.active)
                .map(|d| d.from_id == tab._id)
                .unwrap_or(false);
            // A split tab shows the focused pane's label + icon; a single
            // pane shows the tab's own label.
            let display_label = tab.display_label();
            let base_label = display_label.trim_end_matches(" (disconnected)");
            let detected_os = self.tab_detected_os(base_label);
            // During a drag every tab is uniform (inactive width); otherwise
            // the active tab claims its wider width as usual.
            let width = if is_active && !dragging_any {
                active_width
            } else {
                inactive_width
            };
            // Per-host accent override: when this tab points at a
            // saved connection that has a custom `color`, tint the
            // active-tab fill and the tab text with that color
            // (JetBrains-style "respiração"). Otherwise the active
            // tab keeps the global accent.
            let host_accent: Option<Color> = self.connections.iter()
                .find(|c| c.label == base_label)
                // `custom_color` is what the icon picker writes (the
                // user-chosen accent). The legacy `color` field is a
                // dead column today but stays as a fallback so any
                // future code path that fills it still works.
                .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
                .and_then(crate::widgets::parse_hex_color)
                // Cloud-transport tabs (`ECS · ...`, `SSM · ...`,
                // `K8s · ...`) don't match any saved Connection by
                // label, so the per-host color lookup above returns
                // None and the active-tab gradient falls back to the
                // global accent. Derive a brand-coloured accent from
                // the tab label prefix instead so the tab "breathes"
                // the parent dynamic-group color (AWS orange / K8s
                // blue / etc.) the same way a per-host accent does.
                .or_else(|| {
                    crate::os_icon::tab_label_cloud_brand(base_label).map(|brand| {
                        crate::os_icon::provider_icon(brand, OryxisColors::t().accent).1
                    })
                });
            // Tabs always render the badge as a rounded square,
            // independent of the per-host override and the global
            // `default_host_icon` setting. Circular badges read as
            // pills inside the narrow tab strip and the variable
            // shape disrupts the row's vertical rhythm; locking the
            // tab shape keeps the strip uniform while leaving the
            // dashboard card free to honour the user's choice.
            let host_icon_style = crate::widgets::HostIconStyle::Rounded;
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
            // Session-group tabs carry the group's own icon + color.
            let session_group = tab
                .session_group_id
                .and_then(|id| self.session_groups.iter().find(|g| g.id == id));
            let sg_custom_color = session_group
                .and_then(|g| g.color.as_deref())
                .and_then(crate::widgets::parse_hex_color);
            let sg_custom_icon = session_group
                .and_then(|g| g.icon_style.as_deref())
                .filter(|s| !s.is_empty());
            if is_dragging {
                // The dragged tab floats as a ghost following the cursor
                // (built below); leave a same-width gap here that the other
                // tabs slide around as the reorder happens.
                let gap_w = if tab.pinned && compact_pins { CHIP_W } else { width };
                tab_items.push(
                    Space::new()
                        .width(gap_w)
                        .height(TAB_HEIGHT)
                        .into(),
                );
            } else if tab.pinned && compact_pins {
                // Chrome-style: icon-only chip, fixed width, stuck left.
                tab_items.push(pinned_tab_chip(
                    idx,
                    detected_os.as_deref(),
                    is_active,
                    sg_custom_color.or(host_accent),
                    host_icon_style,
                    sg_custom_icon,
                    sg_custom_color,
                    status_dot,
                ));
            } else {
                tab_items.push(session_tab(
                    idx,
                    display_label,
                    tab.pane_count(),
                    is_active,
                    is_hovered,
                    detected_os.as_deref(),
                    width,
                    self.setting_tab_close_button_side == "right",
                    status_dot,
                    sg_custom_color.or(host_accent),
                    host_icon_style,
                    sg_custom_icon,
                    sg_custom_color,
                    tab.pinned,
                ));
            }
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
        let plus_btn = crate::widgets::bounds_reporter(new_tab_btn(), self.plus_btn_bounds.clone());
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
        if let Some(dots) = dots_btn {
            cluster_items.push(dots);
            cluster_items.push(Space::new().width(2).into());
        }
        cluster_items.push(plus_btn);
        cluster_items.push(Space::new().width(2).into());
        cluster_items.push(sidebar_btn());
        cluster_items.push(Space::new().width(2).into());
        cluster_items.push(chrome_row.into());
        let right_cluster: Element<'_, Message> = crate::widgets::dir_row(cluster_items)
            .align_y(iced::Alignment::Center)
            .into();

        let workspace_mode = self.setting_layout_mode == "workspace";

        // Workspace mode also shows the Oryxis product logo on the
        // far leading edge so the chrome carries product identity in
        // the JetBrains style. Classic mode keeps the logo on the
        // sidebar header where it always lived and skips both the
        // burger (sidebar already lists every destination) and the
        // logo (it would compete with the sidebar header).
        let mut leading: Vec<Element<'_, Message>> = Vec::new();
        if workspace_mode {
            // Small breathing space on the very leading edge so the
            // logo doesn't kiss the window border. Matches the gap
            // JetBrains leaves on its product mark.
            leading.push(Space::new().width(4).into());
            leading.push(product_logo(self.logo_small_handle.clone()));
            // Burger only in Workspace mode; Classic already has a
            // full sidebar listing every destination so a dropdown
            // mirroring it would be redundant noise.
            leading.push(burger_menu_btn(self.show_burger_menu));
        } else {
            // Classic mode keeps the sidebar collapse toggle on the
            // leading edge so the user can shrink the sidebar to icons.
            leading.push(super::sidebar::sidebar_toggle_btn(!self.sidebar_collapsed));
        }
        leading.push(tab_strip);
        leading.push(right_cluster);

        // Four-block row: [logo?] [burger] [sidebar_toggle] [tab_strip(Fill)] [right_cluster].
        // Burger / sidebar_toggle / right_cluster are Length::Shrink so iced
        // gives them their content width first; tab_strip is the remaining
        // Fill area in between. `dir_row` flips the row under RTL so the
        // leading-edge controls always sit next to the sidebar (which the
        // outer layout also flips to the trailing edge).
        let bar: Element<'_, Message> = container(
            crate::widgets::dir_row(leading)
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            ..Default::default()
        })
        .into();

        // Floating ghost of the tab being dragged. The bar spans the window's
        // top-left, so window-space cursor x maps directly to bar-local x. The
        // ghost is a plain (non-interactive) container, so the tab MouseAreas
        // underneath still receive the hover events that drive the live-slide.
        if dragging_any
            && let Some(drag) = self.tab_drag
            && let Some(tab) = self.tabs.iter().find(|t| t._id == drag.from_id)
        {
            let base_label = tab
                .display_label()
                .trim_end_matches(" (disconnected)")
                .to_string();
            let detected_os = self.tab_detected_os(&base_label);
            let compact = tab.pinned && compact_pins;
            let session_group = tab
                .session_group_id
                .and_then(|id| self.session_groups.iter().find(|g| g.id == id));
            let sg_color = session_group
                .and_then(|g| g.color.as_deref())
                .and_then(crate::widgets::parse_hex_color);
            let sg_icon = session_group
                .and_then(|g| g.icon_style.as_deref())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let accent = sg_color.unwrap_or_else(|| OryxisColors::t().accent);
            let ghost_w = if compact { CHIP_W } else { inactive_width };
            let ghost = drag_ghost(
                base_label,
                detected_os,
                compact,
                ghost_w,
                accent,
                sg_icon,
                sg_color,
            );
            let gx = (self.mouse_position.x - ghost_w / 2.0).max(0.0);
            let positioned: Element<'_, Message> = iced::widget::Column::new()
                .push(Space::new().height(7.0))
                .push(
                    iced::widget::Row::new()
                        .push(Space::new().width(gx))
                        .push(ghost),
                )
                .into();
            return iced::widget::Stack::new()
                .push(bar)
                .push(positioned)
                .width(Length::Fill)
                .height(Length::Fixed(BAR_HEIGHT))
                .into();
        }
        bar
    }
}

impl Oryxis {
    /// Build a task that snaps the tab strip's scrollable so the active
    /// tab is roughly centered in the visible area. Called whenever a
    /// new tab gets focused (manual select, opening a local shell,
    /// connecting an SSH session, etc.), without this the new tab
    /// can land off-screen when the strip is in scroll mode.
    /// Resolve the OS / brand icon hint for a tab from its (de-suffixed)
    /// label: a saved connection's detected OS, else a local-shell hint, else
    /// the cloud brand parsed from an `ECS · ...` / `K8s · ...` prefix.
    pub(crate) fn tab_detected_os(&self, base_label: &str) -> Option<String> {
        self.connections
            .iter()
            .find(|c| c.label == base_label)
            .and_then(|c| c.detected_os.clone())
            .or_else(|| crate::os_icon::local_shell_os_hint(base_label))
            .or_else(|| {
                crate::os_icon::tab_label_cloud_brand(base_label).map(|s| s.to_string())
            })
    }

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
        const RIGHT_CLUSTER_WIDTH: f32 = SIDEBAR_BUTTON_WIDTH
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
        let workspace_mode = self.setting_layout_mode == "workspace";
        let toggle_width = if workspace_mode { 0.0 } else { SIDEBAR_TOGGLE_WIDTH };
        let logo_width = if workspace_mode { SIDEBAR_TOGGLE_WIDTH } else { 0.0 };
        let approx_strip_width = (self.window_size.width
            - toggle_width
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
    // transparent so hover gets the only visible cue.
    let bg: Background = if is_active {
        let top = Color { a: 0.28, ..effective_accent };
        let bot = Color { a: 0.04, ..effective_accent };
        Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                .add_stop(0.0, top)
                .add_stop(1.0, bot),
        ))
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

    MouseArea::new(tab_btn)
        .on_enter(Message::TabHovered(idx))
        .on_exit(Message::TabUnhovered)
        .on_right_press(Message::ShowTabMenu(idx))
        .into()
}

/// Compact (Chrome-style) pinned tab: an icon-only chip at a fixed width,
/// with the same OS / host / session-group badge as a full tab. Select on
/// click, right-click opens the same context menu (to unpin, etc.).
#[allow(clippy::too_many_arguments)]
fn pinned_tab_chip<'a>(
    idx: usize,
    detected_os: Option<&str>,
    is_active: bool,
    host_accent: Option<Color>,
    host_icon_style: crate::widgets::HostIconStyle,
    custom_icon: Option<&'a str>,
    custom_color: Option<Color>,
    status_dot: Option<Color>,
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
        let bg = match status {
            _ if is_active => Background::Color(Color { a: 0.18, ..accent }),
            BtnStatus::Hovered => Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06)),
            _ => Background::Color(Color::TRANSPARENT),
        };
        let border = if is_active {
            Border { radius: Radius::from(6.0), color: accent, width: 1.5 }
        } else {
            Border { radius: Radius::from(6.0), ..Default::default() }
        };
        button::Style { background: Some(bg), border, ..Default::default() }
    });
    MouseArea::new(btn)
        .on_enter(Message::TabHovered(idx))
        .on_exit(Message::TabUnhovered)
        .on_right_press(Message::ShowTabMenu(idx))
        .into()
}

/// Floating chip shown over the strip while a tab is being drag-reordered:
/// a non-interactive copy of the dragged tab that tracks the cursor while
/// the real slot sits empty and the other tabs slide around it. Mirrors the
/// tab's badge (and label, unless it's a compact pinned chip) so the user
/// keeps sight of what they're moving.
#[allow(clippy::too_many_arguments)]
fn drag_ghost<'a>(
    label: String,
    detected_os: Option<String>,
    compact: bool,
    width: f32,
    accent: Color,
    custom_icon: Option<String>,
    custom_color: Option<Color>,
) -> Element<'a, Message> {
    let (glyph, badge_color) = if let Some(name) = custom_icon.as_deref() {
        (crate::os_icon::custom_icon_glyph(name), custom_color.unwrap_or(accent))
    } else {
        crate::os_icon::resolve_icon(detected_os.as_deref(), accent)
    };
    let glyph_el: Element<'_, Message> = glyph.view(12.0, Color::WHITE);
    let badge = crate::widgets::host_icon(
        crate::widgets::HostIconStyle::Rounded,
        badge_color,
        &label,
        Some(glyph_el),
        TAB_ICON_SLOT,
    );
    let content: Element<'a, Message> = if compact {
        container(badge)
            .center_x(Length::Fixed(CHIP_W))
            .center_y(Length::Fixed(TAB_HEIGHT))
            .into()
    } else {
        crate::widgets::dir_row(vec![
            container(badge)
                .center_x(Length::Fixed(TAB_ICON_SLOT))
                .center_y(Length::Fixed(TAB_ICON_SLOT))
                .into(),
            Space::new().width(5).into(),
            text(truncate_label(&label, width))
                .size(12)
                .line_height(1.0)
                .wrapping(iced::widget::text::Wrapping::None)
                .font(SYSTEM_UI_SEMIBOLD)
                .color(accent)
                .into(),
        ])
        .align_y(iced::Alignment::Center)
        .into()
    };
    container(content)
        .center_y(Length::Fixed(TAB_HEIGHT))
        .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 4.0 })
        .width(Length::Fixed(width))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.96, ..OryxisColors::t().bg_hover })),
            border: Border { radius: Radius::from(6.0), color: accent, width: 1.5 },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
            ..Default::default()
        })
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
    let btn = button(
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
    });
    // Click = new tab (default). Hovering reveals the New-Tab / Split
    // popover (no-op unless a terminal tab is open, see `ShowSplitMenu`).
    MouseArea::new(btn)
        .on_enter(Message::ShowSplitMenu)
        .on_exit(Message::SplitMenuLeave)
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

/// Terminal side-panel toggle (Chat / Snippets / History). Sits right of
/// the `+ new tab` button. Replaces the old host-search button, which
/// only duplicated `+`'s "open the new-tab picker" action.
fn sidebar_btn<'a>() -> Element<'a, Message> {
    let hover_color = OryxisColors::t().text_secondary;
    button(
        container(
            iced_fonts::lucide::panel_right().size(15).color(hover_color),
        )
        .center(Length::Fixed(SIDEBAR_BUTTON_WIDTH))
        .height(Length::Fixed(BAR_HEIGHT)),
    )
    .on_press(Message::ToggleChatSidebar)
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
fn product_logo<'a>(handle: iced::widget::svg::Handle) -> Element<'a, Message> {
    container(
        iced::widget::svg(handle)
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
