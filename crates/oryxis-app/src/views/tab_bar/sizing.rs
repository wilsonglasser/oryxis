//! Tab bar: sizing. Split out of views/tab_bar/mod.rs.

use super::*;
/// Decide how much horizontal space each tab gets. Returns
/// `(active_width, inactive_width)`. The active tab claims its natural
/// width when it fits; inactives split whatever's left, clamped to the
/// minimum so they don't disappear.
pub(crate) fn allocate_tab_widths(n: usize, available: f32) -> (f32, f32) {
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

/// Width an inactive tab needs to show its full label without
/// truncation, clamped to `[TAB_MIN_WIDTH, TAB_NATURAL_WIDTH]`. The
/// reserved portion mirrors `truncate_label`'s (icon slot + gaps +
/// button padding) plus a couple px of slack so a content-sized tab
/// never ellipsizes its own label, with the trailing close slot and
/// the split-count chip added when those variants are present.
pub(crate) fn tab_content_width(label: &str, close_on_right: bool, has_count_chip: bool) -> f32 {
    let base = label.trim_end_matches(" (disconnected)");
    let chars = base.chars().count() as f32;
    // 29 = TAB_ICON_SLOT + 5 (gap) + 4 + 4 (truncate_label's reserve);
    // +6 slack so the last glyph isn't flush against the edge.
    let mut reserved = TAB_ICON_SLOT + 5.0 + 4.0 + 4.0 + 6.0;
    if close_on_right {
        // Trailing close slot reserves its own width (see session_tab).
        reserved += TAB_ICON_SLOT + 4.0;
    }
    if has_count_chip {
        // Split pane-count pill (COUNT_DISC) + its leading gap.
        reserved += 15.0 + 4.0;
    }
    (reserved + chars * TAB_CHAR_WIDTH).clamp(TAB_MIN_WIDTH, TAB_NATURAL_WIDTH)
}

/// Truncate a label to fit visually within `width` px at the tab font
/// size. Falls back to a single character + ellipsis on extreme shrink
/// so the user still sees something.
pub(crate) fn truncate_label(label: &str, width: f32) -> String {
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
/// Background for an active tab or area chip. By default it paints the
/// "lit from above" vertical accent gradient (a saturated tint at the top
/// fading to near-transparent at the bottom). When `solid_fill` is set
/// (Settings -> Interface -> Tab fill style = Solid color), it paints a
/// single flat accent tint instead, so the active tab reads as a uniform
/// chip. Shared by every tab/chip renderer (and the Settings preview) so
/// the choice stays consistent and the preview can't drift from the strip.
pub(crate) fn active_tab_bg(accent: Color, solid_fill: bool) -> Background {
    if solid_fill {
        return Background::Color(Color { a: 0.16, ..accent });
    }
    let top = Color { a: 0.28, ..accent };
    let bot = Color { a: 0.04, ..accent };
    Background::Gradient(iced::Gradient::Linear(
        iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
            .add_stop(0.0, top)
            .add_stop(1.0, bot),
    ))
}
