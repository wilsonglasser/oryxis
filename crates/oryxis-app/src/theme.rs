use iced::Color;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global app theme index.
static ACTIVE_THEME: AtomicUsize = AtomicUsize::new(0);

/// Available app themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTheme {
    OryxisDark,
    OryxisLight,
    Termius,
    Darcula,
    IslandsDark,
    Dracula,
    Monokai,
    HackerGreen,
    Nord,
    NordLight,
    SolarizedLight,
    PaperLight,
}

impl AppTheme {
    pub const ALL: &[AppTheme] = &[
        Self::OryxisDark,
        Self::OryxisLight,
        Self::Termius,
        Self::Darcula,
        Self::IslandsDark,
        Self::Dracula,
        Self::Monokai,
        Self::HackerGreen,
        Self::Nord,
        Self::NordLight,
        Self::SolarizedLight,
        Self::PaperLight,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::OryxisDark => "Oryxis Dark",
            Self::OryxisLight => "Oryxis Light",
            Self::Termius => "Termius",
            Self::Darcula => "Darcula",
            Self::IslandsDark => "Islands Dark",
            Self::Dracula => "Dracula",
            Self::Monokai => "Monokai",
            Self::HackerGreen => "Hacker Green",
            Self::Nord => "Nord",
            Self::NordLight => "Nord Light",
            Self::SolarizedLight => "Solarized Light",
            Self::PaperLight => "Paper Light",
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
            AppTheme::Termius => &TERMIUS,
            AppTheme::Darcula => &DARCULA,
            AppTheme::IslandsDark => &ISLANDS_DARK,
            AppTheme::Dracula => &DRACULA,
            AppTheme::Monokai => &MONOKAI,
            AppTheme::HackerGreen => &HACKER_GREEN,
            AppTheme::Nord => &NORD,
            AppTheme::NordLight => &NORD_LIGHT,
            AppTheme::SolarizedLight => &SOLARIZED_LIGHT,
            AppTheme::PaperLight => &PAPER_LIGHT,
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

/// Termius — neutral dark navy with cyan accent. Re-tuned off real screenshots:
/// surfaces are gray-blue (not navy-heavy), accent matches the "+ HOST" button.
pub const TERMIUS: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(27, 31, 40),        // #1B1F28
    bg_sidebar: Color::from_rgb8(20, 24, 31),        // #14181F
    bg_surface: Color::from_rgb8(42, 47, 56),        // #2A2F38 — gray-blue card
    bg_hover: Color::from_rgb8(49, 55, 65),          // #313741
    bg_selected: Color::from_rgb8(56, 63, 74),       // #383F4A
    text_primary: Color::from_rgb8(237, 240, 245),   // #EDF0F5
    text_secondary: Color::from_rgb8(176, 184, 196), // #B0B8C4
    text_muted: Color::from_rgb8(122, 132, 146),     // #7A8492
    accent: Color::from_rgb8(43, 194, 208),          // #2BC2D0 — Termius cyan
    accent_hover: Color::from_rgb8(80, 214, 226),    // #50D6E2
    success: Color::from_rgb8(95, 211, 101),         // #5FD365 — Connect green
    warning: Color::from_rgb8(231, 171, 82),
    error: Color::from_rgb8(232, 98, 98),
    terminal_bg: Color::from_rgb8(22, 26, 33),
    terminal_fg: Color::from_rgb8(224, 229, 237),
    terminal_cursor: Color::from_rgb8(43, 194, 208),
    border: Color::from_rgb8(46, 52, 66),            // #2E3442
    border_focus: Color::from_rgb8(43, 194, 208),
};

/// Darcula — JetBrains' signature dark theme (editor bg `#2B2B2B`, tool window
/// bg `#3C3F41`, keyword orange, string green, selection blue).
pub const DARCULA: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(43, 43, 43),        // #2B2B2B — editor bg
    bg_sidebar: Color::from_rgb8(60, 63, 65),        // #3C3F41 — tool window
    bg_surface: Color::from_rgb8(49, 51, 53),        // #313335 — panel surface
    bg_hover: Color::from_rgb8(65, 69, 71),          // #414547
    bg_selected: Color::from_rgb8(75, 110, 175),     // #4B6EAF — selection blue
    text_primary: Color::from_rgb8(169, 183, 198),   // #A9B7C6 — editor fg
    text_secondary: Color::from_rgb8(187, 181, 159), // #BBB59F
    text_muted: Color::from_rgb8(128, 128, 128),     // #808080
    accent: Color::from_rgb8(204, 120, 50),          // #CC7832 — keyword orange
    accent_hover: Color::from_rgb8(255, 198, 109),   // #FFC66D
    success: Color::from_rgb8(106, 135, 89),         // #6A8759 — string green
    warning: Color::from_rgb8(255, 198, 109),        // #FFC66D
    error: Color::from_rgb8(207, 91, 86),            // #CF5B56
    terminal_bg: Color::from_rgb8(43, 43, 43),
    terminal_fg: Color::from_rgb8(169, 183, 198),
    terminal_cursor: Color::from_rgb8(204, 120, 50),
    border: Color::from_rgb8(81, 81, 81),            // #515151
    border_focus: Color::from_rgb8(204, 120, 50),
};

/// Islands Dark — JetBrains' New UI Islands variant. Same palette as Darcula
/// but with noticeably lifted surfaces to produce the "floating panel" look
/// (bg is slightly darker, cards are distinctly brighter than the frame).
pub const ISLANDS_DARK: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(30, 31, 34),        // #1E1F22 — outer frame
    bg_sidebar: Color::from_rgb8(30, 31, 34),        // #1E1F22 — same as frame
    bg_surface: Color::from_rgb8(49, 51, 56),        // #313338 — raised island
    bg_hover: Color::from_rgb8(60, 62, 66),          // #3C3E42
    bg_selected: Color::from_rgb8(71, 97, 151),      // #476197 — softer blue
    text_primary: Color::from_rgb8(223, 225, 229),   // #DFE1E5 — crisper text
    text_secondary: Color::from_rgb8(178, 181, 188), // #B2B5BC
    text_muted: Color::from_rgb8(123, 127, 137),     // #7B7F89
    accent: Color::from_rgb8(117, 163, 255),         // #75A3FF — UI blue
    accent_hover: Color::from_rgb8(140, 180, 255),   // #8CB4FF
    success: Color::from_rgb8(98, 174, 108),         // #62AE6C
    warning: Color::from_rgb8(233, 174, 76),         // #E9AE4C
    error: Color::from_rgb8(221, 92, 92),            // #DD5C5C
    terminal_bg: Color::from_rgb8(30, 31, 34),
    terminal_fg: Color::from_rgb8(223, 225, 229),
    terminal_cursor: Color::from_rgb8(117, 163, 255),
    border: Color::from_rgb8(46, 48, 53),            // #2E3035 — faint ring
    border_focus: Color::from_rgb8(117, 163, 255),
};

/// Nord Light — Snow Storm base (snow-white bg, frost-blue accent). Pairs
/// with the dark Nord theme as its light counterpart.
pub const NORD_LIGHT: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(236, 239, 244),   // #ECEFF4 — nord6
    bg_sidebar: Color::from_rgb8(229, 233, 240),   // #E5E9F0 — nord5
    bg_surface: Color::from_rgb8(252, 253, 254),   // near-white card
    bg_hover: Color::from_rgb8(222, 226, 234),     // subtle hover
    bg_selected: Color::from_rgb8(209, 216, 228),
    text_primary: Color::from_rgb8(46, 52, 64),    // #2E3440 — nord0
    text_secondary: Color::from_rgb8(59, 66, 82),  // #3B4252 — nord1
    text_muted: Color::from_rgb8(100, 111, 130),   // readable mid tone
    accent: Color::from_rgb8(94, 129, 172),        // #5E81AC — nord10 frost
    accent_hover: Color::from_rgb8(129, 161, 193), // #81A1C1 — nord9
    success: Color::from_rgb8(163, 190, 140),      // nord14
    warning: Color::from_rgb8(235, 203, 139),      // nord13
    error: Color::from_rgb8(191, 97, 106),         // nord11
    terminal_bg: Color::from_rgb8(252, 253, 254),
    terminal_fg: Color::from_rgb8(46, 52, 64),
    terminal_cursor: Color::from_rgb8(94, 129, 172),
    border: Color::from_rgb8(209, 216, 228),
    border_focus: Color::from_rgb8(94, 129, 172),
};

/// Solarized Light — Ethan Schoonover's classic cream/beige palette. The "pardo"
/// light: warm tan background, navy ink, distinctive coherent hue progression.
pub const SOLARIZED_LIGHT: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(253, 246, 227),   // #FDF6E3 — base3 (cream)
    bg_sidebar: Color::from_rgb8(238, 232, 213),   // #EEE8D5 — base2
    bg_surface: Color::from_rgb8(253, 246, 227),   // #FDF6E3
    bg_hover: Color::from_rgb8(238, 232, 213),     // #EEE8D5
    bg_selected: Color::from_rgb8(220, 215, 197),  // slight darken
    text_primary: Color::from_rgb8(88, 110, 117),  // #586E75 — base01
    text_secondary: Color::from_rgb8(101, 123, 131), // #657B83 — base00
    text_muted: Color::from_rgb8(147, 161, 161),   // #93A1A1 — base1
    accent: Color::from_rgb8(38, 139, 210),        // #268BD2 — blue
    accent_hover: Color::from_rgb8(42, 161, 152),  // #2AA198 — cyan
    success: Color::from_rgb8(133, 153, 0),        // #859900 — green
    warning: Color::from_rgb8(181, 137, 0),        // #B58900 — yellow
    error: Color::from_rgb8(220, 50, 47),          // #DC322F — red
    terminal_bg: Color::from_rgb8(253, 246, 227),
    terminal_fg: Color::from_rgb8(88, 110, 117),
    terminal_cursor: Color::from_rgb8(38, 139, 210),
    border: Color::from_rgb8(220, 215, 197),
    border_focus: Color::from_rgb8(38, 139, 210),
};

/// Paper Light — minimal warm white with muted teal. Cleaner alternative to
/// Solarized for users who want light but not tan.
pub const PAPER_LIGHT: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(250, 248, 245),   // #FAF8F5 — warm white
    bg_sidebar: Color::from_rgb8(243, 240, 235),   // #F3F0EB
    bg_surface: Color::from_rgb8(255, 255, 255),   // pure white panels
    bg_hover: Color::from_rgb8(240, 237, 232),     // #F0EDE8
    bg_selected: Color::from_rgb8(226, 222, 215),  // stronger pick
    text_primary: Color::from_rgb8(45, 55, 72),    // #2D3748
    text_secondary: Color::from_rgb8(74, 85, 104), // #4A5568
    text_muted: Color::from_rgb8(113, 128, 150),   // #718096
    accent: Color::from_rgb8(44, 122, 123),        // #2C7A7B — deep teal
    accent_hover: Color::from_rgb8(56, 178, 172),  // #38B2AC
    success: Color::from_rgb8(56, 161, 105),       // #38A169
    warning: Color::from_rgb8(214, 158, 46),       // #D69E2E
    error: Color::from_rgb8(197, 48, 48),          // #C53030
    terminal_bg: Color::from_rgb8(255, 255, 255),
    terminal_fg: Color::from_rgb8(45, 55, 72),
    terminal_cursor: Color::from_rgb8(44, 122, 123),
    border: Color::from_rgb8(226, 222, 215),
    border_focus: Color::from_rgb8(44, 122, 123),
};

/// Dracula — Zeno Rocha's classic theme (purple/pink with vivid accents).
/// Distinct from Darcula (JetBrains); both are kept since the look differs.
pub const DRACULA: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(40, 42, 54),        // #282A36
    bg_sidebar: Color::from_rgb8(33, 34, 44),        // #21222C
    bg_surface: Color::from_rgb8(68, 71, 90),        // #44475A — "current line"
    bg_hover: Color::from_rgb8(55, 57, 73),
    bg_selected: Color::from_rgb8(98, 114, 164),     // #6272A4 — comment blue
    text_primary: Color::from_rgb8(248, 248, 242),   // #F8F8F2 — foreground
    text_secondary: Color::from_rgb8(189, 147, 249), // #BD93F9 — purple
    text_muted: Color::from_rgb8(139, 148, 180),     // readable comment tone
    accent: Color::from_rgb8(189, 147, 249),         // #BD93F9 — Dracula purple
    accent_hover: Color::from_rgb8(210, 170, 255),
    success: Color::from_rgb8(80, 250, 123),         // #50FA7B — green
    warning: Color::from_rgb8(241, 250, 140),        // #F1FA8C — yellow
    error: Color::from_rgb8(255, 85, 85),            // #FF5555 — red
    terminal_bg: Color::from_rgb8(40, 42, 54),
    terminal_fg: Color::from_rgb8(248, 248, 242),
    terminal_cursor: Color::from_rgb8(248, 248, 242),
    border: Color::from_rgb8(68, 71, 90),
    border_focus: Color::from_rgb8(189, 147, 249),
};

/// Monokai — Wimer Hazenberg's classic (pink keyword, green string, dark bg).
pub const MONOKAI: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(39, 40, 34),        // #272822
    bg_sidebar: Color::from_rgb8(29, 31, 26),        // darker
    bg_surface: Color::from_rgb8(52, 53, 46),        // #34352E
    bg_hover: Color::from_rgb8(62, 63, 56),
    bg_selected: Color::from_rgb8(73, 72, 62),       // #49483E
    text_primary: Color::from_rgb8(248, 248, 242),   // #F8F8F2
    text_secondary: Color::from_rgb8(166, 226, 46),  // #A6E22E — green ident
    text_muted: Color::from_rgb8(117, 113, 94),      // #75715E — comment
    accent: Color::from_rgb8(249, 38, 114),          // #F92672 — pink
    accent_hover: Color::from_rgb8(255, 80, 145),
    success: Color::from_rgb8(166, 226, 46),         // #A6E22E
    warning: Color::from_rgb8(230, 219, 116),        // #E6DB74
    error: Color::from_rgb8(249, 38, 114),
    terminal_bg: Color::from_rgb8(39, 40, 34),
    terminal_fg: Color::from_rgb8(248, 248, 242),
    terminal_cursor: Color::from_rgb8(249, 38, 114),
    border: Color::from_rgb8(73, 72, 62),
    border_focus: Color::from_rgb8(249, 38, 114),
};

/// Hacker Green — near-black background, phosphor-green text. "Matrix" vibe.
pub const HACKER_GREEN: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(8, 12, 8),          // near-black with green tint
    bg_sidebar: Color::from_rgb8(3, 6, 3),
    bg_surface: Color::from_rgb8(14, 22, 14),
    bg_hover: Color::from_rgb8(22, 34, 22),
    bg_selected: Color::from_rgb8(30, 50, 30),
    text_primary: Color::from_rgb8(92, 235, 101),    // #5CEB65 — primary green
    text_secondary: Color::from_rgb8(76, 200, 85),
    text_muted: Color::from_rgb8(58, 140, 65),
    accent: Color::from_rgb8(92, 235, 101),
    accent_hover: Color::from_rgb8(130, 255, 140),
    success: Color::from_rgb8(130, 255, 140),
    warning: Color::from_rgb8(235, 220, 92),
    error: Color::from_rgb8(235, 92, 92),
    terminal_bg: Color::from_rgb8(4, 6, 4),
    terminal_fg: Color::from_rgb8(92, 235, 101),
    terminal_cursor: Color::from_rgb8(92, 235, 101),
    border: Color::from_rgb8(34, 70, 34),
    border_focus: Color::from_rgb8(92, 235, 101),
};

pub const NORD: ThemeColors = ThemeColors {
    bg_primary: Color::from_rgb8(46, 52, 64),        // nord0
    bg_sidebar: Color::from_rgb8(59, 66, 82),        // nord1
    bg_surface: Color::from_rgb8(67, 76, 94),        // nord2
    bg_hover: Color::from_rgb8(76, 86, 106),         // nord3
    bg_selected: Color::from_rgb8(94, 105, 128),     // nord3 lifted — distinct from hover
    text_primary: Color::from_rgb8(236, 239, 244),   // nord6 — brightest snow
    text_secondary: Color::from_rgb8(216, 222, 233), // nord4 — general secondary text
    text_muted: Color::from_rgb8(143, 160, 191),     // mid snow — readable on all bg tiers
    accent: Color::from_rgb8(136, 192, 208),         // nord8
    accent_hover: Color::from_rgb8(163, 209, 222),   // nord8 lifted
    success: Color::from_rgb8(163, 190, 140),        // nord14
    warning: Color::from_rgb8(235, 203, 139),        // nord13
    error: Color::from_rgb8(191, 97, 106),           // nord11
    terminal_bg: Color::from_rgb8(46, 52, 64),
    terminal_fg: Color::from_rgb8(216, 222, 233),
    terminal_cursor: Color::from_rgb8(216, 222, 233),
    border: Color::from_rgb8(94, 105, 128),          // visible against bg_hover
    border_focus: Color::from_rgb8(136, 192, 208),
};
