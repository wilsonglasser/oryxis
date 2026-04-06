use iced::Color;

/// Oryxis theme colors.
///
/// All UI colors are defined here so switching between dark/light themes
/// only requires changing these constants.
pub struct OryxisColors;

#[allow(dead_code)]
impl OryxisColors {
    // ── Backgrounds ──
    pub const BG_PRIMARY: Color = Color::from_rgb(0.086, 0.102, 0.098);    // #161A19
    pub const BG_SIDEBAR: Color = Color::from_rgb(0.067, 0.082, 0.078);    // #111514
    pub const BG_SURFACE: Color = Color::from_rgb(0.118, 0.137, 0.133);    // #1E2322
    pub const BG_HOVER: Color = Color::from_rgb(0.145, 0.169, 0.165);      // #252B2A
    pub const BG_SELECTED: Color = Color::from_rgb(0.173, 0.200, 0.196);   // #2C3332

    // ── Text ──
    pub const TEXT_PRIMARY: Color = Color::from_rgb(0.90, 0.91, 0.90);      // #E6E8E6
    pub const TEXT_SECONDARY: Color = Color::from_rgb(0.55, 0.57, 0.56);    // #8C918F
    pub const TEXT_MUTED: Color = Color::from_rgb(0.35, 0.37, 0.36);        // #595E5C

    // ── Accent ──
    pub const ACCENT: Color = Color::from_rgb(0.133, 0.60, 0.569);         // #229991
    pub const ACCENT_HOVER: Color = Color::from_rgb(0.20, 0.70, 0.667);    // #33B3AA

    // ── Semantic ──
    pub const SUCCESS: Color = Color::from_rgb(0.30, 0.78, 0.55);          // #4DC78C
    pub const WARNING: Color = Color::from_rgb(0.95, 0.73, 0.25);          // #F2BA40
    pub const ERROR: Color = Color::from_rgb(0.92, 0.33, 0.38);            // #EB5461

    // ── Terminal ──
    pub const TERMINAL_BG: Color = Color::from_rgb(0.055, 0.071, 0.067);   // #0E1211
    pub const TERMINAL_FG: Color = Color::from_rgb(0.85, 0.87, 0.85);      // #D9DED9
    pub const TERMINAL_CURSOR: Color = Color::from_rgb(0.133, 0.60, 0.569);// #229991

    // ── Borders ──
    pub const BORDER: Color = Color::from_rgb(0.157, 0.196, 0.192);        // #283231
    pub const BORDER_FOCUS: Color = Color::from_rgb(0.133, 0.60, 0.569);   // #229991

    // ── Alpha variants (for overlays, hover states) ──
    pub const ACCENT_SUBTLE: Color = Color::from_rgba(0.133, 0.60, 0.569, 0.15);
    pub const ACCENT_PRESSED: Color = Color::from_rgba(0.133, 0.60, 0.569, 0.25);
    pub const ERROR_SUBTLE: Color = Color::from_rgba(0.92, 0.33, 0.38, 0.15);
    pub const WARNING_SUBTLE: Color = Color::from_rgba(0.95, 0.73, 0.25, 0.15);
    pub const WHITE_SUBTLE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
}
