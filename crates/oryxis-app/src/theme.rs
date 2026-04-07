use iced::Color;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global app theme index.
static ACTIVE_THEME: AtomicUsize = AtomicUsize::new(0);

/// Available app themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTheme {
    OryxisDark,
    OryxisLight,
    Dracula,
    Nord,
}

impl AppTheme {
    pub const ALL: &[AppTheme] = &[
        Self::OryxisDark,
        Self::OryxisLight,
        Self::Dracula,
        Self::Nord,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::OryxisDark => "Oryxis Dark",
            Self::OryxisLight => "Oryxis Light",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
        }
    }

    pub fn set_active(theme: AppTheme) {
        let idx = Self::ALL.iter().position(|t| *t == theme).unwrap_or(0);
        ACTIVE_THEME.store(idx, Ordering::Relaxed);
    }

    pub fn active() -> AppTheme {
        let idx = ACTIVE_THEME.load(Ordering::Relaxed);
        Self::ALL.get(idx).copied().unwrap_or(AppTheme::OryxisDark)
    }
}

/// Oryxis UI colors — resolved from the active theme.
/// All methods are static lookups so existing `OryxisColors::ACCENT` style calls
/// can be migrated to `OryxisColors::accent()` etc.
/// For now, `const` versions remain for backward compat; theme-aware versions
/// use function calls.
pub struct OryxisColors;

#[allow(dead_code)]
impl OryxisColors {
    // ── Theme-aware accessors ──
    pub fn t() -> &'static ThemeColors {
        match AppTheme::active() {
            AppTheme::OryxisDark => &ORYXIS_DARK,
            AppTheme::OryxisLight => &ORYXIS_LIGHT,
            AppTheme::Dracula => &DRACULA,
            AppTheme::Nord => &NORD,
        }
    }

    // Keep const aliases for backward compat (default theme)
    // These will be used until all call sites are migrated

    // ── Backgrounds ──
    pub const BG_PRIMARY: Color = ORYXIS_DARK.bg_primary;
    pub const BG_SIDEBAR: Color = ORYXIS_DARK.bg_sidebar;
    pub const BG_SURFACE: Color = ORYXIS_DARK.bg_surface;
    pub const BG_HOVER: Color = ORYXIS_DARK.bg_hover;
    pub const BG_SELECTED: Color = ORYXIS_DARK.bg_selected;

    // ── Text ──
    pub const TEXT_PRIMARY: Color = ORYXIS_DARK.text_primary;
    pub const TEXT_SECONDARY: Color = ORYXIS_DARK.text_secondary;
    pub const TEXT_MUTED: Color = ORYXIS_DARK.text_muted;

    // ── Accent ──
    pub const ACCENT: Color = ORYXIS_DARK.accent;
    pub const ACCENT_HOVER: Color = ORYXIS_DARK.accent_hover;

    // ── Semantic ──
    pub const SUCCESS: Color = ORYXIS_DARK.success;
    pub const WARNING: Color = ORYXIS_DARK.warning;
    pub const ERROR: Color = ORYXIS_DARK.error;

    // ── Terminal ──
    pub const TERMINAL_BG: Color = ORYXIS_DARK.terminal_bg;
    pub const TERMINAL_FG: Color = ORYXIS_DARK.terminal_fg;
    pub const TERMINAL_CURSOR: Color = ORYXIS_DARK.terminal_cursor;

    // ── Borders ──
    pub const BORDER: Color = ORYXIS_DARK.border;
    pub const BORDER_FOCUS: Color = ORYXIS_DARK.border_focus;

    // ── Alpha variants ──
    pub const ACCENT_SUBTLE: Color = Color::from_rgba(0.133, 0.60, 0.569, 0.15);
    pub const ACCENT_PRESSED: Color = Color::from_rgba(0.133, 0.60, 0.569, 0.25);
    pub const ERROR_SUBTLE: Color = Color::from_rgba(0.92, 0.33, 0.38, 0.15);
    pub const WARNING_SUBTLE: Color = Color::from_rgba(0.95, 0.73, 0.25, 0.15);
    pub const WHITE_SUBTLE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
}

/// Raw color data for a theme.
pub struct ThemeColors {
    pub bg_primary: Color,
    pub bg_sidebar: Color,
    pub bg_surface: Color,
    pub bg_hover: Color,
    pub bg_selected: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub terminal_bg: Color,
    pub terminal_fg: Color,
    pub terminal_cursor: Color,
    pub border: Color,
    pub border_focus: Color,
}

pub const ORYXIS_DARK: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb(0.086, 0.102, 0.098),
    bg_sidebar: Color::from_rgb(0.067, 0.082, 0.078),
    bg_surface: Color::from_rgb(0.118, 0.137, 0.133),
    bg_hover: Color::from_rgb(0.145, 0.169, 0.165),
    bg_selected: Color::from_rgb(0.173, 0.200, 0.196),
    text_primary: Color::from_rgb(0.90, 0.91, 0.90),
    text_secondary: Color::from_rgb(0.55, 0.57, 0.56),
    text_muted: Color::from_rgb(0.35, 0.37, 0.36),
    accent: Color::from_rgb(0.133, 0.60, 0.569),
    accent_hover: Color::from_rgb(0.20, 0.70, 0.667),
    success: Color::from_rgb(0.30, 0.78, 0.55),
    warning: Color::from_rgb(0.95, 0.73, 0.25),
    error: Color::from_rgb(0.92, 0.33, 0.38),
    terminal_bg: Color::from_rgb(0.055, 0.071, 0.067),
    terminal_fg: Color::from_rgb(0.85, 0.87, 0.85),
    terminal_cursor: Color::from_rgb(0.133, 0.60, 0.569),
    border: Color::from_rgb(0.157, 0.196, 0.192),
    border_focus: Color::from_rgb(0.133, 0.60, 0.569),
};

pub const ORYXIS_LIGHT: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb(0.95, 0.96, 0.95),
    bg_sidebar: Color::from_rgb(0.92, 0.93, 0.92),
    bg_surface: Color::from_rgb(1.0, 1.0, 1.0),
    bg_hover: Color::from_rgb(0.90, 0.91, 0.90),
    bg_selected: Color::from_rgb(0.85, 0.87, 0.86),
    text_primary: Color::from_rgb(0.12, 0.14, 0.13),
    text_secondary: Color::from_rgb(0.40, 0.42, 0.41),
    text_muted: Color::from_rgb(0.60, 0.62, 0.61),
    accent: Color::from_rgb(0.10, 0.50, 0.47),
    accent_hover: Color::from_rgb(0.133, 0.60, 0.569),
    success: Color::from_rgb(0.20, 0.65, 0.45),
    warning: Color::from_rgb(0.85, 0.63, 0.15),
    error: Color::from_rgb(0.82, 0.23, 0.28),
    terminal_bg: Color::from_rgb(0.98, 0.98, 0.98),
    terminal_fg: Color::from_rgb(0.15, 0.17, 0.16),
    terminal_cursor: Color::from_rgb(0.10, 0.50, 0.47),
    border: Color::from_rgb(0.85, 0.87, 0.86),
    border_focus: Color::from_rgb(0.10, 0.50, 0.47),
};

pub const DRACULA: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(40, 42, 54),
    bg_sidebar: Color::from_rgb8(33, 34, 44),
    bg_surface: Color::from_rgb8(68, 71, 90),
    bg_hover: Color::from_rgb8(55, 57, 73),
    bg_selected: Color::from_rgb8(68, 71, 90),
    text_primary: Color::from_rgb8(248, 248, 242),
    text_secondary: Color::from_rgb8(189, 147, 249),
    text_muted: Color::from_rgb8(98, 114, 164),
    accent: Color::from_rgb8(189, 147, 249),
    accent_hover: Color::from_rgb8(210, 170, 255),
    success: Color::from_rgb8(80, 250, 123),
    warning: Color::from_rgb8(241, 250, 140),
    error: Color::from_rgb8(255, 85, 85),
    terminal_bg: Color::from_rgb8(40, 42, 54),
    terminal_fg: Color::from_rgb8(248, 248, 242),
    terminal_cursor: Color::from_rgb8(248, 248, 242),
    border: Color::from_rgb8(68, 71, 90),
    border_focus: Color::from_rgb8(189, 147, 249),
};

pub const NORD: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(46, 52, 64),
    bg_sidebar: Color::from_rgb8(59, 66, 82),
    bg_surface: Color::from_rgb8(67, 76, 94),
    bg_hover: Color::from_rgb8(76, 86, 106),
    bg_selected: Color::from_rgb8(76, 86, 106),
    text_primary: Color::from_rgb8(216, 222, 233),
    text_secondary: Color::from_rgb8(129, 161, 193),
    text_muted: Color::from_rgb8(76, 86, 106),
    accent: Color::from_rgb8(136, 192, 208),
    accent_hover: Color::from_rgb8(143, 188, 187),
    success: Color::from_rgb8(163, 190, 140),
    warning: Color::from_rgb8(235, 203, 139),
    error: Color::from_rgb8(191, 97, 106),
    terminal_bg: Color::from_rgb8(46, 52, 64),
    terminal_fg: Color::from_rgb8(216, 222, 233),
    terminal_cursor: Color::from_rgb8(216, 222, 233),
    border: Color::from_rgb8(76, 86, 106),
    border_focus: Color::from_rgb8(136, 192, 208),
};
