//! Tab bar: sizing. Split out of views/tab_bar/mod.rs.

use super::*;

/// Where the tab strip docks (Settings -> Interface -> Tab bar
/// position). `Top` / `Bottom` are the horizontal strips; `Left` /
/// `Right` dock a vertical tab list on that side of the window
/// (issue #87). Left / right are PHYSICAL sides, not logical ones:
/// the user picked an explicit edge, so RTL must not flip it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TabBarPos {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl TabBarPos {
    /// Parse the `tab_bar_position` setting value; anything
    /// unrecognized falls back to `Top`, mirroring the dispatch
    /// normalization.
    pub(crate) fn from_setting(v: &str) -> Self {
        match v {
            "bottom" => Self::Bottom,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::Top,
        }
    }

    /// A vertical (left / right docked) strip.
    pub(crate) fn is_side(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Process-wide tab-strip dock position gate, mirroring the
/// `AUTO_TITLE` gate in `state/tabs.rs`: `active_tab_bg` is a free fn
/// called from every tab/chip renderer, so threading the setting through
/// each signature would touch the whole family for one gradient flip.
/// Set from boot + the Settings dispatch; read at render time.
static TAB_BAR_POS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub(crate) fn set_tab_bar_pos(pos: TabBarPos) {
    let v = match pos {
        TabBarPos::Top => 0,
        TabBarPos::Bottom => 1,
        TabBarPos::Left => 2,
        TabBarPos::Right => 3,
    };
    TAB_BAR_POS.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn tab_bar_pos() -> TabBarPos {
    match TAB_BAR_POS.load(std::sync::atomic::Ordering::Relaxed) {
        1 => TabBarPos::Bottom,
        2 => TabBarPos::Left,
        3 => TabBarPos::Right,
        _ => TabBarPos::Top,
    }
}

/// Whether `cursor` falls inside the tab strip's geometric band for the
/// current dock `pos`. This is the backstop that gates arming a reorder
/// drag (issue #87): the `hovered_tab` flag is the primary signal, but
/// its MouseArea exit can be lost when the cursor slides straight into
/// the terminal canvas, after which any press would arm a phantom drag.
/// The band must track the strip's actual dock, not assume the top: the
/// original `y <= BAR_HEIGHT` guard silently disabled drag-reorder on
/// every non-top dock (bottom / left / right), which is the reported
/// "can't move tabs on the left side". Side docks also accept the top
/// `BAR_HEIGHT` band, where `pinned_tabs_top_bar` parks the pinned
/// chips (so those stay draggable), but only when that top bar is
/// actually present, so a press into the content area on a
/// hidden-top-bar side dock can't sneak in.
pub(crate) fn cursor_in_tab_strip_band(
    pos: TabBarPos,
    cursor: iced::Point,
    window: iced::Size,
    pins_in_top_bar: bool,
) -> bool {
    match pos {
        TabBarPos::Top => cursor.y <= BAR_HEIGHT,
        // The bottom strip sits above the (content-sized) status bar;
        // the slack reaches past the strip + a typical status-bar band
        // without leaking far into the content above it.
        TabBarPos::Bottom => cursor.y >= window.height - BAR_HEIGHT - 40.0,
        TabBarPos::Left => {
            cursor.x <= SIDE_STRIP_WIDTH + 2.0
                || (pins_in_top_bar && cursor.y <= BAR_HEIGHT)
        }
        TabBarPos::Right => {
            cursor.x >= window.width - SIDE_STRIP_WIDTH - 2.0
                || (pins_in_top_bar && cursor.y <= BAR_HEIGHT)
        }
    }
}
/// Visual treatment applied to INACTIVE tabs so they read as distinct
/// chips instead of blending into the strip (issue #87, sosokun's
/// "borders or underline or some kind of visual separating"). `Border`
/// draws a full subtle outline (identical in every dock); `Underline`
/// draws an accent hairline on each tab's INNER edge, which is what
/// makes it adapt to the dock: bottom on a top strip, top on a bottom
/// strip, and the content-facing side on the vertical strips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum InactiveTabStyle {
    #[default]
    None,
    Border,
    Underline,
}

impl InactiveTabStyle {
    pub(crate) fn from_setting(v: &str) -> Self {
        match v {
            "border" => Self::Border,
            "underline" => Self::Underline,
            _ => Self::None,
        }
    }

    /// Tint for this style's separation cue, mixed from the strip
    /// surface toward the theme's secondary text so it lands at a
    /// readable distance from the bar on dark AND light themes.
    /// `ThemeColors::border` (the first cut) is tuned for panel edges
    /// over `bg_surface`; over the darker `bg_sidebar` it measured
    /// within 10/255 of the strip and the cue read as a smudge.
    ///
    /// The rule is a 2 px hairline and the outline traces the whole
    /// chip, so the outline sits lower on the same scale: matching
    /// their tints would make the Border style shout.
    pub(crate) fn cue_color(self) -> Color {
        let strength = match self {
            Self::Underline => 0.50,
            _ => 0.30,
        };
        crate::theme::mix(
            OryxisColors::t().bg_sidebar,
            OryxisColors::t().text_secondary,
            strength,
        )
    }
}

/// Process-wide inactive-tab-style gate, same shape + rationale as
/// `TAB_BAR_POS`: `session_tab` is a free fn reached from every strip,
/// so the setting rides an atomic read at render time instead of a
/// param threaded through the whole tab-renderer family.
static INACTIVE_TAB_STYLE: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(0);

pub(crate) fn set_inactive_tab_style(style: InactiveTabStyle) {
    let v = match style {
        InactiveTabStyle::None => 0,
        InactiveTabStyle::Border => 1,
        InactiveTabStyle::Underline => 2,
    };
    INACTIVE_TAB_STYLE.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn inactive_tab_style() -> InactiveTabStyle {
    match INACTIVE_TAB_STYLE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => InactiveTabStyle::Border,
        2 => InactiveTabStyle::Underline,
        _ => InactiveTabStyle::None,
    }
}

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

/// Natural width of the Settings tab (issue #120). Its X always occupies
/// the trailing slot (there is no hover-reveal), so the slot is reserved
/// whatever the close-button-side setting says; `settings_tab` subtracts
/// the same amount before truncating.
pub(crate) fn settings_tab_width(label: &str) -> f32 {
    tab_content_width(label, true, false)
}

/// Truncate a label to fit visually within `width` px at the tab font
/// size. Falls back to a single character + ellipsis on extreme shrink
/// so the user still sees something.
pub(crate) fn truncate_label(label: &str, width: f32) -> String {
    let reserved = TAB_ICON_SLOT + 5.0 + 4.0 + 4.0; // icon + gap + padding
    let usable = (width - reserved).max(0.0);
    let max_px = usable;
    if max_px <= 0.0 {
        return String::new();
    }
    // Count display width: CJK / fullwidth characters are roughly 2x
    // the Latin character width (TAB_CHAR_WIDTH). Reserve one Latin
    // char width for the ellipsis before measuring.
    let ellipsis_px = TAB_CHAR_WIDTH;
    let mut px: f32 = 0.0;
    let mut cut_at: usize = 0;
    for (i, ch) in label.char_indices() {
        let ch_w = if ch > '\u{2e80}' { TAB_CHAR_WIDTH * 2.0 } else { TAB_CHAR_WIDTH };
        if px + ch_w + ellipsis_px > max_px {
            break;
        }
        px += ch_w;
        cut_at = i + ch.len_utf8();
    }
    if cut_at >= label.len() {
        return label.to_string();
    }
    format!("{}…", &label[..cut_at])
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
    let hi = Color { a: 0.28, ..accent };
    let lo = Color { a: 0.04, ..accent };
    // The wash always runs along the chip's SHORT axis (vertically),
    // "lit from above" like every other chip in the app. A chip is
    // ~36 px tall and up to 200 px wide, so a wash along the long axis
    // spends most of the chip at the fade-out alpha and the fill stops
    // reading as a fill: on the side docks the right half of the active
    // tab measured within 6/255 of the bare strip surface. Only the
    // bottom strip flips the stops, so the saturated edge still hugs
    // the window frame there. The side docks deliberately do NOT light
    // from their outer edge (their chips are list rows, the same reason
    // `inactive_edge_line` keeps its rule horizontal), and the pinned
    // chips the side docks park in the slim top bar get the top strip's
    // gradient for free instead of a sideways wash on a horizontal bar.
    let (angle, start, end) = match tab_bar_pos() {
        TabBarPos::Bottom => (std::f32::consts::PI, lo, hi),
        TabBarPos::Top | TabBarPos::Left | TabBarPos::Right => {
            (std::f32::consts::PI, hi, lo)
        }
    };
    Background::Gradient(iced::Gradient::Linear(
        iced::gradient::Linear::new(iced::Radians(angle))
            .add_stop(0.0, start)
            .add_stop(1.0, end),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Size};

    const WIN: Size = Size { width: 1600.0, height: 900.0 };

    #[test]
    fn top_dock_band_is_the_top_strip_only() {
        let p = TabBarPos::Top;
        assert!(cursor_in_tab_strip_band(p, Point::new(800.0, 10.0), WIN, false));
        // A press deep in the content below the strip is rejected.
        assert!(!cursor_in_tab_strip_band(p, Point::new(800.0, 500.0), WIN, false));
    }

    #[test]
    fn bottom_dock_band_tracks_the_bottom_edge() {
        let p = TabBarPos::Bottom;
        // The old `y <= BAR_HEIGHT` guard rejected this, silently
        // breaking bottom-dock reorder; the edge-aware band accepts it.
        assert!(cursor_in_tab_strip_band(p, Point::new(800.0, 880.0), WIN, false));
        assert!(!cursor_in_tab_strip_band(p, Point::new(800.0, 400.0), WIN, false));
    }

    #[test]
    fn left_dock_band_is_the_leading_column() {
        let p = TabBarPos::Left;
        // A tab lower down the left strip (the exact reported failure).
        assert!(cursor_in_tab_strip_band(p, Point::new(100.0, 600.0), WIN, false));
        // A press in the content to the right is rejected.
        assert!(!cursor_in_tab_strip_band(p, Point::new(800.0, 600.0), WIN, false));
    }

    #[test]
    fn right_dock_band_is_the_trailing_column() {
        let p = TabBarPos::Right;
        assert!(cursor_in_tab_strip_band(
            p,
            Point::new(WIN.width - 100.0, 600.0),
            WIN,
            false
        ));
        assert!(!cursor_in_tab_strip_band(p, Point::new(800.0, 600.0), WIN, false));
    }

    #[test]
    fn every_dock_lights_the_active_chip_along_its_short_axis() {
        // The wash must stay vertical in EVERY dock (issue #87): the
        // side docks' 200 px wide chips spent most of their width at
        // the fade-out alpha under the old sideways gradient, and the
        // pinned chips a side dock parks in the slim top bar carried
        // that sideways wash onto a horizontal bar. Only the stop
        // order flips, so the bottom strip still lights from its frame.
        let saved = tab_bar_pos();
        for (pos, hi_first) in [
            (TabBarPos::Top, true),
            (TabBarPos::Bottom, false),
            (TabBarPos::Left, true),
            (TabBarPos::Right, true),
        ] {
            set_tab_bar_pos(pos);
            let Background::Gradient(iced::Gradient::Linear(g)) =
                active_tab_bg(Color::WHITE, false)
            else {
                panic!("{pos:?}: expected a gradient fill");
            };
            assert_eq!(g.angle, iced::Radians(std::f32::consts::PI), "{pos:?}");
            let first = g.stops[0].expect("first stop").color.a;
            let last = g.stops[1].expect("second stop").color.a;
            assert_eq!(first > last, hi_first, "{pos:?}: stop order");
        }
        set_tab_bar_pos(saved);
    }

    #[test]
    fn side_dock_top_band_only_when_pins_dock_there() {
        // Docked pins live in the top bar: a press up there must arm
        // their drag, but only while that top bar is actually present.
        let top = Point::new(800.0, 12.0);
        assert!(cursor_in_tab_strip_band(TabBarPos::Left, top, WIN, true));
        assert!(!cursor_in_tab_strip_band(TabBarPos::Left, top, WIN, false));
    }
}
