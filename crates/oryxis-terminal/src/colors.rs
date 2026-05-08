use iced::Color;
use alacritty_terminal::vte::ansi::{self, NamedColor};

/// Terminal color theme name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalTheme {
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
    SolarizedDark,
    SolarizedLight,
    PaperLight,
}

impl TerminalTheme {
    pub const ALL: &[TerminalTheme] = &[
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
        Self::SolarizedDark,
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
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::PaperLight => "Paper Light",
        }
    }

    pub fn palette(&self) -> TerminalPalette {
        match self {
            Self::OryxisDark => TerminalPalette::oryxis_dark(),
            Self::OryxisLight => TerminalPalette::oryxis_light(),
            Self::Termius => TerminalPalette::termius(),
            Self::Darcula => TerminalPalette::darcula(),
            Self::IslandsDark => TerminalPalette::islands_dark(),
            Self::Dracula => TerminalPalette::dracula(),
            Self::Monokai => TerminalPalette::monokai(),
            Self::HackerGreen => TerminalPalette::hacker_green(),
            Self::Nord => TerminalPalette::nord(),
            Self::NordLight => TerminalPalette::nord_light(),
            Self::SolarizedDark => TerminalPalette::solarized_dark(),
            Self::SolarizedLight => TerminalPalette::solarized_light(),
            Self::PaperLight => TerminalPalette::paper_light(),
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
    /// Oryxis Dark — like Termius Dark: white text, teal cursor/accent, vivid ANSI colors
    pub fn oryxis_dark() -> Self {
        Self {
            foreground: Color::from_rgb(0.133, 0.60, 0.569), // teal (like Termius Dark green)
            background: Color::from_rgb(0.055, 0.071, 0.067),
            cursor: Color::from_rgb(0.133, 0.60, 0.569),     // teal cursor
            ansi: [
                Color::from_rgb(0.18, 0.20, 0.19),    // 0 Black
                Color::from_rgb(0.95, 0.40, 0.42),    // 1 Red (vivid)
                Color::from_rgb(0.30, 0.82, 0.55),    // 2 Green (vivid)
                Color::from_rgb(0.95, 0.78, 0.30),    // 3 Yellow (vivid)
                Color::from_rgb(0.45, 0.65, 0.95),    // 4 Blue (vivid)
                Color::from_rgb(0.75, 0.55, 0.90),    // 5 Magenta
                Color::from_rgb(0.20, 0.75, 0.70),    // 6 Cyan (teal)
                Color::from_rgb(0.80, 0.82, 0.80),    // 7 White
                Color::from_rgb(0.40, 0.42, 0.40),    // 8 Bright Black
                Color::from_rgb(1.0, 0.55, 0.55),     // 9 Bright Red
                Color::from_rgb(0.40, 0.90, 0.65),    // 10 Bright Green
                Color::from_rgb(1.0, 0.88, 0.45),     // 11 Bright Yellow
                Color::from_rgb(0.55, 0.75, 1.0),     // 12 Bright Blue
                Color::from_rgb(0.85, 0.68, 0.98),    // 13 Bright Magenta
                Color::from_rgb(0.33, 0.85, 0.78),    // 14 Bright Cyan
                Color::from_rgb(0.93, 0.94, 0.93),    // 15 Bright White
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

    /// Oryxis Light — light counterpart of `oryxis_dark`. White paper
    /// surface, deep teal foreground, slightly desaturated ANSI so
    /// the colours don't strobe against the bright background.
    pub fn oryxis_light() -> Self {
        Self {
            foreground: Color::from_rgb8(33, 56, 66),     // deep teal-grey
            background: Color::from_rgb8(248, 250, 250),
            cursor: Color::from_rgb8(34, 153, 144),        // teal accent
            ansi: [
                Color::from_rgb8(60, 64, 64),     // Black
                Color::from_rgb8(193, 60, 60),    // Red
                Color::from_rgb8(46, 138, 87),    // Green
                Color::from_rgb8(170, 124, 22),   // Yellow / amber
                Color::from_rgb8(45, 102, 168),   // Blue
                Color::from_rgb8(140, 90, 175),   // Magenta
                Color::from_rgb8(33, 142, 134),   // Cyan / teal
                Color::from_rgb8(214, 217, 215),  // White
                Color::from_rgb8(110, 116, 116),  // Bright Black
                Color::from_rgb8(220, 90, 90),    // Bright Red
                Color::from_rgb8(70, 165, 110),   // Bright Green
                Color::from_rgb8(200, 152, 50),   // Bright Yellow
                Color::from_rgb8(75, 132, 198),   // Bright Blue
                Color::from_rgb8(170, 120, 200),  // Bright Magenta
                Color::from_rgb8(64, 174, 166),   // Bright Cyan
                Color::from_rgb8(244, 246, 244),  // Bright White
            ],
        }
    }

    /// Termius — neutral dark navy with cyan accent matching the app
    /// theme of the same name.
    pub fn termius() -> Self {
        Self {
            foreground: Color::from_rgb8(224, 229, 237),
            background: Color::from_rgb8(22, 26, 33),
            cursor: Color::from_rgb8(43, 194, 208),       // Termius cyan
            ansi: [
                Color::from_rgb8(38, 44, 56),
                Color::from_rgb8(232, 98, 98),
                Color::from_rgb8(95, 211, 101),
                Color::from_rgb8(231, 171, 82),
                Color::from_rgb8(91, 162, 232),
                Color::from_rgb8(178, 130, 220),
                Color::from_rgb8(43, 194, 208),
                Color::from_rgb8(207, 213, 222),
                Color::from_rgb8(70, 78, 92),
                Color::from_rgb8(255, 121, 121),
                Color::from_rgb8(120, 230, 130),
                Color::from_rgb8(255, 197, 102),
                Color::from_rgb8(120, 184, 250),
                Color::from_rgb8(206, 162, 240),
                Color::from_rgb8(80, 214, 226),
                Color::from_rgb8(237, 240, 245),
            ],
        }
    }

    /// Darcula — JetBrains' classic dark editor palette: bg `#2B2B2B`,
    /// orange keywords, green strings, blue selection.
    pub fn darcula() -> Self {
        Self {
            foreground: Color::from_rgb8(169, 183, 198),
            background: Color::from_rgb8(43, 43, 43),
            cursor: Color::from_rgb8(187, 181, 159),
            ansi: [
                Color::from_rgb8(43, 43, 43),
                Color::from_rgb8(207, 91, 86),
                Color::from_rgb8(106, 135, 89),    // string green
                Color::from_rgb8(204, 120, 50),    // keyword orange
                Color::from_rgb8(104, 151, 187),
                Color::from_rgb8(155, 110, 165),
                Color::from_rgb8(96, 156, 156),
                Color::from_rgb8(169, 183, 198),
                Color::from_rgb8(89, 89, 89),
                Color::from_rgb8(229, 130, 124),
                Color::from_rgb8(149, 174, 124),
                Color::from_rgb8(255, 198, 109),
                Color::from_rgb8(151, 195, 232),
                Color::from_rgb8(199, 159, 209),
                Color::from_rgb8(135, 195, 195),
                Color::from_rgb8(232, 232, 232),
            ],
        }
    }

    /// Islands Dark — JetBrains' New UI variant. Cooler outer frame,
    /// brighter foreground than Darcula, blue accent.
    pub fn islands_dark() -> Self {
        Self {
            foreground: Color::from_rgb8(223, 225, 229),
            background: Color::from_rgb8(30, 31, 34),
            cursor: Color::from_rgb8(117, 163, 255),
            ansi: [
                Color::from_rgb8(46, 48, 53),
                Color::from_rgb8(221, 92, 92),
                Color::from_rgb8(98, 174, 108),
                Color::from_rgb8(233, 174, 76),
                Color::from_rgb8(117, 163, 255),
                Color::from_rgb8(189, 147, 249),
                Color::from_rgb8(96, 196, 196),
                Color::from_rgb8(206, 209, 214),
                Color::from_rgb8(80, 84, 92),
                Color::from_rgb8(244, 124, 124),
                Color::from_rgb8(125, 198, 135),
                Color::from_rgb8(255, 200, 110),
                Color::from_rgb8(140, 180, 255),
                Color::from_rgb8(208, 175, 255),
                Color::from_rgb8(135, 215, 215),
                Color::from_rgb8(238, 240, 245),
            ],
        }
    }

    /// Nord Light — Snow Storm base. Light counterpart of `nord()`,
    /// keeps the same Frost / Aurora hues but on a near-white surface.
    pub fn nord_light() -> Self {
        Self {
            foreground: Color::from_rgb8(46, 52, 64),
            background: Color::from_rgb8(236, 239, 244),
            cursor: Color::from_rgb8(94, 129, 172),       // Frost blue
            ansi: [
                Color::from_rgb8(59, 66, 82),
                Color::from_rgb8(191, 97, 106),
                Color::from_rgb8(163, 190, 140),
                Color::from_rgb8(208, 165, 86),
                Color::from_rgb8(94, 129, 172),
                Color::from_rgb8(180, 142, 173),
                Color::from_rgb8(136, 192, 208),
                Color::from_rgb8(216, 222, 233),
                Color::from_rgb8(76, 86, 106),
                Color::from_rgb8(208, 116, 124),
                Color::from_rgb8(180, 205, 162),
                Color::from_rgb8(220, 178, 100),
                Color::from_rgb8(129, 161, 193),
                Color::from_rgb8(196, 162, 188),
                Color::from_rgb8(143, 188, 187),
                Color::from_rgb8(229, 233, 240),
            ],
        }
    }

    /// Solarized Light — Ethan Schoonover's bright variant. Same
    /// accent ramp as Solarized Dark, mirrored against the cream
    /// `#FDF6E3` paper.
    pub fn solarized_light() -> Self {
        Self {
            foreground: Color::from_rgb8(101, 123, 131),    // base00
            background: Color::from_rgb8(253, 246, 227),    // base3
            cursor: Color::from_rgb8(101, 123, 131),
            ansi: [
                Color::from_rgb8(7, 54, 66),       // base02
                Color::from_rgb8(220, 50, 47),     // red
                Color::from_rgb8(133, 153, 0),     // green
                Color::from_rgb8(181, 137, 0),     // yellow
                Color::from_rgb8(38, 139, 210),    // blue
                Color::from_rgb8(211, 54, 130),    // magenta
                Color::from_rgb8(42, 161, 152),    // cyan
                Color::from_rgb8(238, 232, 213),   // base2
                Color::from_rgb8(0, 43, 54),       // base03
                Color::from_rgb8(203, 75, 22),     // orange
                Color::from_rgb8(88, 110, 117),    // base01
                Color::from_rgb8(101, 123, 131),   // base00
                Color::from_rgb8(131, 148, 150),   // base0
                Color::from_rgb8(108, 113, 196),   // violet
                Color::from_rgb8(147, 161, 161),   // base1
                Color::from_rgb8(253, 246, 227),   // base3
            ],
        }
    }

    /// Paper Light — neutral high-contrast light theme. Pure-ish
    /// paper background, near-black text, restrained ANSI for
    /// long-form readability (matches the app's `Paper Light` UI).
    pub fn paper_light() -> Self {
        Self {
            foreground: Color::from_rgb8(34, 34, 34),
            background: Color::from_rgb8(250, 250, 250),
            cursor: Color::from_rgb8(34, 34, 34),
            ansi: [
                Color::from_rgb8(34, 34, 34),
                Color::from_rgb8(170, 50, 50),
                Color::from_rgb8(50, 130, 80),
                Color::from_rgb8(160, 110, 30),
                Color::from_rgb8(45, 95, 165),
                Color::from_rgb8(140, 90, 175),
                Color::from_rgb8(40, 130, 130),
                Color::from_rgb8(230, 230, 230),
                Color::from_rgb8(90, 90, 90),
                Color::from_rgb8(200, 70, 70),
                Color::from_rgb8(70, 160, 100),
                Color::from_rgb8(190, 140, 50),
                Color::from_rgb8(70, 125, 195),
                Color::from_rgb8(170, 120, 200),
                Color::from_rgb8(60, 160, 160),
                Color::from_rgb8(245, 245, 245),
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
