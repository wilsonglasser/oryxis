use iced::Color;
use alacritty_terminal::vte::ansi::{self, NamedColor};

/// Terminal color theme name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalTheme {
    OryxisDark,
    HackerGreen,
    Dracula,
    SolarizedDark,
    Monokai,
    Nord,
}

impl TerminalTheme {
    pub const ALL: &[TerminalTheme] = &[
        Self::OryxisDark,
        Self::HackerGreen,
        Self::Dracula,
        Self::SolarizedDark,
        Self::Monokai,
        Self::Nord,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::OryxisDark => "Oryxis Dark",
            Self::HackerGreen => "Hacker Green",
            Self::Dracula => "Dracula",
            Self::SolarizedDark => "Solarized Dark",
            Self::Monokai => "Monokai",
            Self::Nord => "Nord",
        }
    }

    pub fn palette(&self) -> TerminalPalette {
        match self {
            Self::OryxisDark => TerminalPalette::oryxis_dark(),
            Self::HackerGreen => TerminalPalette::hacker_green(),
            Self::Dracula => TerminalPalette::dracula(),
            Self::SolarizedDark => TerminalPalette::solarized_dark(),
            Self::Monokai => TerminalPalette::monokai(),
            Self::Nord => TerminalPalette::nord(),
        }
    }
}

/// Terminal color palette.
pub struct TerminalPalette {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    pub ansi: [Color; 16],
}

impl Default for TerminalPalette {
    fn default() -> Self {
        Self::oryxis_dark()
    }
}

impl TerminalPalette {
    /// Oryxis Dark — teal accent, dark neutral background
    pub fn oryxis_dark() -> Self {
        Self {
            foreground: Color::from_rgb(0.85, 0.87, 0.85),
            background: Color::from_rgb(0.055, 0.071, 0.067),
            cursor: Color::from_rgb(0.133, 0.60, 0.569),
            ansi: [
                Color::from_rgb(0.18, 0.20, 0.19),  // Black
                Color::from_rgb(0.92, 0.33, 0.38),   // Red
                Color::from_rgb(0.30, 0.78, 0.55),   // Green (teal-ish)
                Color::from_rgb(0.95, 0.73, 0.25),   // Yellow
                Color::from_rgb(0.133, 0.60, 0.569),  // Blue → teal
                Color::from_rgb(0.80, 0.62, 0.95),   // Magenta
                Color::from_rgb(0.20, 0.70, 0.667),   // Cyan (teal accent)
                Color::from_rgb(0.73, 0.75, 0.73),   // White
                Color::from_rgb(0.36, 0.38, 0.37),   // Bright Black
                Color::from_rgb(0.95, 0.55, 0.55),   // Bright Red
                Color::from_rgb(0.40, 0.85, 0.65),   // Bright Green
                Color::from_rgb(0.98, 0.82, 0.52),   // Bright Yellow
                Color::from_rgb(0.20, 0.70, 0.667),   // Bright Blue → teal
                Color::from_rgb(0.85, 0.70, 0.98),   // Bright Magenta
                Color::from_rgb(0.33, 0.80, 0.75),   // Bright Cyan
                Color::from_rgb(0.90, 0.91, 0.90),   // Bright White
            ],
        }
    }

    /// Hacker Green — classic green-on-black
    pub fn hacker_green() -> Self {
        Self {
            foreground: Color::from_rgb(0.0, 0.87, 0.0),
            background: Color::from_rgb(0.0, 0.04, 0.0),
            cursor: Color::from_rgb(0.0, 1.0, 0.0),
            ansi: [
                Color::from_rgb(0.0, 0.0, 0.0),       // Black
                Color::from_rgb(0.80, 0.0, 0.0),       // Red
                Color::from_rgb(0.0, 0.80, 0.0),       // Green
                Color::from_rgb(0.80, 0.80, 0.0),      // Yellow
                Color::from_rgb(0.0, 0.50, 0.0),       // Blue → dark green
                Color::from_rgb(0.50, 0.0, 0.50),      // Magenta
                Color::from_rgb(0.0, 0.80, 0.50),      // Cyan
                Color::from_rgb(0.75, 0.75, 0.75),     // White
                Color::from_rgb(0.30, 0.30, 0.30),     // Bright Black
                Color::from_rgb(1.0, 0.0, 0.0),        // Bright Red
                Color::from_rgb(0.0, 1.0, 0.0),        // Bright Green
                Color::from_rgb(1.0, 1.0, 0.0),        // Bright Yellow
                Color::from_rgb(0.0, 0.70, 0.0),       // Bright Blue → green
                Color::from_rgb(0.70, 0.0, 0.70),      // Bright Magenta
                Color::from_rgb(0.0, 1.0, 0.70),       // Bright Cyan
                Color::from_rgb(1.0, 1.0, 1.0),        // Bright White
            ],
        }
    }

    /// Dracula
    pub fn dracula() -> Self {
        Self {
            foreground: Color::from_rgb8(248, 248, 242),
            background: Color::from_rgb8(40, 42, 54),
            cursor: Color::from_rgb8(248, 248, 242),
            ansi: [
                Color::from_rgb8(33, 34, 44),     // Black
                Color::from_rgb8(255, 85, 85),    // Red
                Color::from_rgb8(80, 250, 123),   // Green
                Color::from_rgb8(241, 250, 140),  // Yellow
                Color::from_rgb8(189, 147, 249),  // Blue (purple)
                Color::from_rgb8(255, 121, 198),  // Magenta (pink)
                Color::from_rgb8(139, 233, 253),  // Cyan
                Color::from_rgb8(248, 248, 242),  // White
                Color::from_rgb8(98, 114, 164),   // Bright Black
                Color::from_rgb8(255, 110, 110),  // Bright Red
                Color::from_rgb8(105, 255, 148),  // Bright Green
                Color::from_rgb8(255, 255, 165),  // Bright Yellow
                Color::from_rgb8(210, 170, 255),  // Bright Blue
                Color::from_rgb8(255, 146, 213),  // Bright Magenta
                Color::from_rgb8(164, 255, 255),  // Bright Cyan
                Color::from_rgb8(255, 255, 255),  // Bright White
            ],
        }
    }

    /// Solarized Dark
    pub fn solarized_dark() -> Self {
        Self {
            foreground: Color::from_rgb8(131, 148, 150),
            background: Color::from_rgb8(0, 43, 54),
            cursor: Color::from_rgb8(131, 148, 150),
            ansi: [
                Color::from_rgb8(7, 54, 66),      // Black
                Color::from_rgb8(220, 50, 47),    // Red
                Color::from_rgb8(133, 153, 0),    // Green
                Color::from_rgb8(181, 137, 0),    // Yellow
                Color::from_rgb8(38, 139, 210),   // Blue
                Color::from_rgb8(211, 54, 130),   // Magenta
                Color::from_rgb8(42, 161, 152),   // Cyan
                Color::from_rgb8(238, 232, 213),  // White
                Color::from_rgb8(0, 43, 54),      // Bright Black
                Color::from_rgb8(203, 75, 22),    // Bright Red (orange)
                Color::from_rgb8(88, 110, 117),   // Bright Green
                Color::from_rgb8(101, 123, 131),  // Bright Yellow
                Color::from_rgb8(131, 148, 150),  // Bright Blue
                Color::from_rgb8(108, 113, 196),  // Bright Magenta (violet)
                Color::from_rgb8(147, 161, 161),  // Bright Cyan
                Color::from_rgb8(253, 246, 227),  // Bright White
            ],
        }
    }

    /// Monokai
    pub fn monokai() -> Self {
        Self {
            foreground: Color::from_rgb8(248, 248, 242),
            background: Color::from_rgb8(39, 40, 34),
            cursor: Color::from_rgb8(248, 248, 240),
            ansi: [
                Color::from_rgb8(39, 40, 34),     // Black
                Color::from_rgb8(249, 38, 114),   // Red (pink)
                Color::from_rgb8(166, 226, 46),   // Green
                Color::from_rgb8(244, 191, 117),  // Yellow
                Color::from_rgb8(102, 217, 239),  // Blue (cyan)
                Color::from_rgb8(174, 129, 255),  // Magenta (purple)
                Color::from_rgb8(161, 239, 228),  // Cyan
                Color::from_rgb8(248, 248, 242),  // White
                Color::from_rgb8(117, 113, 94),   // Bright Black
                Color::from_rgb8(249, 38, 114),   // Bright Red
                Color::from_rgb8(166, 226, 46),   // Bright Green
                Color::from_rgb8(244, 191, 117),  // Bright Yellow
                Color::from_rgb8(102, 217, 239),  // Bright Blue
                Color::from_rgb8(174, 129, 255),  // Bright Magenta
                Color::from_rgb8(161, 239, 228),  // Bright Cyan
                Color::from_rgb8(248, 248, 242),  // Bright White
            ],
        }
    }

    /// Nord
    pub fn nord() -> Self {
        Self {
            foreground: Color::from_rgb8(216, 222, 233),
            background: Color::from_rgb8(46, 52, 64),
            cursor: Color::from_rgb8(216, 222, 233),
            ansi: [
                Color::from_rgb8(59, 66, 82),     // Black
                Color::from_rgb8(191, 97, 106),   // Red
                Color::from_rgb8(163, 190, 140),  // Green
                Color::from_rgb8(235, 203, 139),  // Yellow
                Color::from_rgb8(129, 161, 193),  // Blue
                Color::from_rgb8(180, 142, 173),  // Magenta
                Color::from_rgb8(136, 192, 208),  // Cyan
                Color::from_rgb8(229, 233, 240),  // White
                Color::from_rgb8(76, 86, 106),    // Bright Black
                Color::from_rgb8(191, 97, 106),   // Bright Red
                Color::from_rgb8(163, 190, 140),  // Bright Green
                Color::from_rgb8(235, 203, 139),  // Bright Yellow
                Color::from_rgb8(129, 161, 193),  // Bright Blue
                Color::from_rgb8(180, 142, 173),  // Bright Magenta
                Color::from_rgb8(143, 188, 187),  // Bright Cyan
                Color::from_rgb8(236, 239, 244),  // Bright White
            ],
        }
    }

    /// Resolve an alacritty Color to an iced Color.
    pub fn resolve(
        &self,
        color: &ansi::Color,
        term_colors: &alacritty_terminal::term::color::Colors,
    ) -> Color {
        match color {
            ansi::Color::Named(named) => self.resolve_named(*named, term_colors),
            ansi::Color::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
            ansi::Color::Indexed(idx) => {
                if let Some(rgb) = term_colors[*idx as usize] {
                    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
                } else if (*idx as usize) < 16 {
                    self.ansi[*idx as usize]
                } else {
                    self.color_from_256(*idx)
                }
            }
        }
    }

    fn resolve_named(
        &self,
        named: NamedColor,
        term_colors: &alacritty_terminal::term::color::Colors,
    ) -> Color {
        let idx = named as usize;
        if let Some(rgb) = term_colors[idx] {
            return Color::from_rgb8(rgb.r, rgb.g, rgb.b);
        }

        match named {
            NamedColor::Black => self.ansi[0],
            NamedColor::Red => self.ansi[1],
            NamedColor::Green => self.ansi[2],
            NamedColor::Yellow => self.ansi[3],
            NamedColor::Blue => self.ansi[4],
            NamedColor::Magenta => self.ansi[5],
            NamedColor::Cyan => self.ansi[6],
            NamedColor::White => self.ansi[7],
            NamedColor::BrightBlack => self.ansi[8],
            NamedColor::BrightRed => self.ansi[9],
            NamedColor::BrightGreen => self.ansi[10],
            NamedColor::BrightYellow => self.ansi[11],
            NamedColor::BrightBlue => self.ansi[12],
            NamedColor::BrightMagenta => self.ansi[13],
            NamedColor::BrightCyan => self.ansi[14],
            NamedColor::BrightWhite => self.ansi[15],
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            NamedColor::DimBlack => dim(self.ansi[0]),
            NamedColor::DimRed => dim(self.ansi[1]),
            NamedColor::DimGreen => dim(self.ansi[2]),
            NamedColor::DimYellow => dim(self.ansi[3]),
            NamedColor::DimBlue => dim(self.ansi[4]),
            NamedColor::DimMagenta => dim(self.ansi[5]),
            NamedColor::DimCyan => dim(self.ansi[6]),
            NamedColor::DimWhite => dim(self.ansi[7]),
            _ => self.foreground,
        }
    }

    fn color_from_256(&self, idx: u8) -> Color {
        if idx < 16 {
            return self.ansi[idx as usize];
        }
        if idx >= 232 {
            let value = ((idx - 232) as f32 * 10.0 + 8.0) / 255.0;
            return Color::from_rgb(value, value, value);
        }
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_f = |v: u8| if v == 0 { 0.0 } else { (v as f32 * 40.0 + 55.0) / 255.0 };
        Color::from_rgb(to_f(r), to_f(g), to_f(b))
    }
}

fn dim(color: Color) -> Color {
    Color::from_rgba(color.r * 0.66, color.g * 0.66, color.b * 0.66, color.a)
}
