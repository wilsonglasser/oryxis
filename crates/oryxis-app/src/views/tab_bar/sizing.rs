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

/// Tab numbering (`tab_number_style` setting): off, a `"12. "` prefix on
/// the label, or the number drawn in the host badge's slot instead of the
/// OS / host glyph.
///
/// The number is the tab's 1-based position in the STRIP (pinned first,
/// SFTP and Settings tabs included), which is the order the user sees and
/// the one `ordered_tab_refs` walks. It is deliberately NOT capped at 9:
/// it identifies a tab, it does not advertise a chord (Ctrl+1 is the Home
/// area tab, so the strip's Nth tab answers to Ctrl+N+1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TabNumberStyle {
    #[default]
    Off,
    Prefix,
    Icon,
}

impl TabNumberStyle {
    pub(crate) fn from_setting(v: &str) -> Self {
        match v {
            "prefix" => Self::Prefix,
            "icon" => Self::Icon,
            _ => Self::Off,
        }
    }
}

/// A tab's number and where to draw it, resolved once per frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TabNumber {
    /// 1-based position in the strip.
    pub(crate) value: usize,
    /// Draw it in the host badge's slot instead of prefixing the label.
    pub(crate) in_icon: bool,
}

impl TabNumber {
    /// The rendered prefix, e.g. `"12. "`.
    pub(crate) fn prefix(&self) -> String {
        format!("{}. ", self.value)
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
///
/// `number_px` is the room the tab-number prefix needs (0 when numbering
/// is off or drawn in the badge). It is the width of the WIDEST number in
/// the strip, not this tab's own, so every chip reserves the same amount
/// and the labels stay aligned instead of stepping when the strip crosses
/// ten tabs.
pub(crate) fn tab_content_width(
    label: &str,
    close_on_right: bool,
    has_count_chip: bool,
    number_px: f32,
) -> f32 {
    let base = label.trim_end_matches(" (disconnected)");
    // Pixels, not codepoints: the sizing half and the truncation half
    // must measure the same way or a CJK label is sized for 7 px glyphs
    // and then cut at 14 px ones. That disagreement is the #108 bug
    // class (see COUNT_GAP), just with scripts instead of the pill.
    let content = label_px_width(base);
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
    (reserved + number_px + content).clamp(TAB_MIN_WIDTH, TAB_NATURAL_WIDTH)
}

/// Estimated rendered width of one codepoint at the tab label's font /
/// size combo (12 px SemiBold).
///
/// Counting codepoints at `TAB_CHAR_WIDTH` each is only right for
/// Latin: East Asian Wide and Fullwidth glyphs render at about double
/// the advance, so a ten-ideograph label is twenty Latin chars wide and
/// used to spill past the chip edge and cover the close button (the
/// mixed Latin/CJK overflow reported on PR #110).
///
/// The wide set is the UAX #11 W / F blocks the bundled CJK fonts
/// actually cover, plus the emoji planes. Two deliberate exclusions:
///
/// - Halfwidth forms (U+FF61..U+FFDC) sit right after the Fullwidth
///   block but render NARROW; treating them as wide would truncate
///   halfwidth-katakana labels at half their real capacity.
/// - Combining marks, zero-width joiners and variation selectors carry
///   no advance of their own, so a decomposed "é" or an emoji ZWJ
///   sequence must not be billed twice.
pub(crate) fn char_px_width(ch: char) -> f32 {
    match u32::from(ch) {
        // Zero advance: combining diacritics, the invisible formatting
        // codepoints of emoji sequences, and skin-tone modifiers.
        0x0300..=0x036F
        | 0x200B..=0x200F
        | 0xFE00..=0xFE0F
        | 0x1F3FB..=0x1F3FF => 0.0,
        // East Asian Wide / Fullwidth.
        0x1100..=0x115F        // Hangul Jamo, initial consonants
        | 0x2E80..=0x303E      // CJK radicals, Kangxi, CJK symbols
        | 0x3041..=0x33FF      // kana, Hangul compat jamo, CJK compat
        | 0x3400..=0x4DBF      // CJK unified ext A
        | 0x4E00..=0x9FFF      // CJK unified
        | 0xA000..=0xA4CF      // Yi
        | 0xAC00..=0xD7A3      // Hangul syllables
        | 0xF900..=0xFAFF      // CJK compat ideographs
        | 0xFE10..=0xFE19      // vertical forms
        | 0xFE30..=0xFE6F      // CJK compat forms
        | 0xFF00..=0xFF60      // fullwidth forms
        | 0xFFE0..=0xFFE6      // fullwidth signs
        | 0x1F300..=0x1FAFF    // emoji and pictographs
        | 0x20000..=0x3FFFD    // CJK unified ext B and beyond
        => TAB_CHAR_WIDTH * 2.0,
        _ => TAB_CHAR_WIDTH,
    }
}

/// Estimated rendered width of a whole label, in pixels. The single
/// measurement both `tab_content_width` (how wide the chip gets) and
/// `truncate_label` (where the ellipsis lands) are built on.
pub(crate) fn label_px_width(label: &str) -> f32 {
    label.chars().map(char_px_width).sum()
}

/// Natural width of a panel tab (issue #120 sized the first one, the
/// Settings chip). Its X always occupies the trailing slot (there is no
/// hover-reveal), so the slot is reserved whatever the close-button-side
/// setting says; `panel_tab` subtracts the same amount before truncating.
pub(crate) fn panel_tab_width(label: &str, number_px: f32) -> f32 {
    tab_content_width(label, true, false, number_px)
}

/// Truncate a label to fit visually within `width` px at the tab font
/// size, measuring in PIXELS (`label_px_width`) rather than counting
/// codepoints. Falls back to a bare ellipsis on extreme shrink so the
/// user still sees that something was cut.
pub(crate) fn truncate_label(label: &str, width: f32) -> String {
    let reserved = TAB_ICON_SLOT + 5.0 + 4.0 + 4.0; // icon + gap + padding
    let max_px = (width - reserved).max(0.0);
    if max_px <= 0.0 {
        return String::new();
    }
    // Measure the WHOLE label first: the ellipsis only costs width when
    // it actually gets drawn. Reserving it unconditionally would shave a
    // glyph off every label that fills its chip exactly, truncating text
    // that fits.
    if label_px_width(label) <= max_px {
        return label.to_string();
    }
    let ellipsis_px = char_px_width('…');
    let mut px = 0.0;
    let mut cut_at = 0;
    for (i, ch) in label.char_indices() {
        let next = px + char_px_width(ch);
        if next + ellipsis_px > max_px {
            break;
        }
        px = next;
        cut_at = i + ch.len_utf8();
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

    /// The invariant the two halves of the label math must hold: a chip
    /// sized by `tab_content_width` never ellipsizes its own label. It
    /// broke for CJK because sizing counted codepoints while truncation
    /// measured pixels, the same disagreement that spilled grouped-tab
    /// labels past the chip edge in #108.
    #[test]
    fn a_chip_never_truncates_the_label_it_was_sized_for() {
        for label in [
            "root@server",
            "web-01.prod.example",     // Latin, near the natural width
            "生産サーバー",             // kana + ideographs
            "prod-데이터베이스",         // mixed Latin + Hangul
            "监控 monitor",             // mixed ideographs + Latin
            "ﾊﾝｶｸ katakana",           // halfwidth: narrow, not wide
        ] {
            for close_on_right in [false, true] {
                let w = tab_content_width(label, close_on_right, false, 0.0);
                assert_eq!(
                    truncate_label(label, w),
                    label,
                    "{label:?} (close_on_right={close_on_right}) ellipsized inside its own chip"
                );
            }
        }
    }

    #[test]
    fn wide_scripts_are_billed_at_double_the_latin_advance() {
        assert_eq!(char_px_width('a'), TAB_CHAR_WIDTH);
        assert_eq!(char_px_width('生'), TAB_CHAR_WIDTH * 2.0);
        assert_eq!(char_px_width('サ'), TAB_CHAR_WIDTH * 2.0);
        assert_eq!(char_px_width('데'), TAB_CHAR_WIDTH * 2.0);
        // Halfwidth katakana sits next to the fullwidth block but
        // renders narrow; billing it as wide would truncate Japanese
        // halfwidth labels at half their capacity.
        assert_eq!(char_px_width('ｱ'), TAB_CHAR_WIDTH);
        // Cyrillic / Greek / Arabic are narrow too, and all sit below
        // the first wide range, which the old `ch > '\u{2e80}'` cut
        // happened to get right and this one must not regress.
        for ch in ['д', 'λ', 'ع', 'א', 'ก'] {
            assert_eq!(char_px_width(ch), TAB_CHAR_WIDTH, "{ch:?}");
        }
        // Combining marks ride the previous glyph, so they cost nothing.
        assert_eq!(char_px_width('\u{0301}'), 0.0);
    }

    #[test]
    fn truncation_cuts_cjk_at_half_the_codepoints_of_latin() {
        // Room for ten Latin glyphs once the icon slot and padding are
        // out: ten Latin chars survive, but only four ideographs plus
        // the ellipsis fit in the same box.
        let width = TAB_ICON_SLOT + 5.0 + 4.0 + 4.0 + 10.0 * TAB_CHAR_WIDTH;
        assert_eq!(truncate_label("abcdefghij", width), "abcdefghij");
        assert_eq!(truncate_label("abcdefghijkl", width), "abcdefghi…");
        assert_eq!(truncate_label("生産管理", width), "生産管理");
        assert_eq!(truncate_label("生産管理サーバー", width), "生産管理…");
    }

    #[test]
    fn extreme_shrink_still_shows_the_cut_marker() {
        // Narrower than one glyph: an ellipsis says "there was more",
        // an empty string says the tab has no label at all.
        let width = TAB_ICON_SLOT + 5.0 + 4.0 + 4.0 + 3.0;
        assert_eq!(truncate_label("server", width), "…");
        // No room for content whatsoever.
        assert_eq!(truncate_label("server", TAB_ICON_SLOT), "");
    }

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
