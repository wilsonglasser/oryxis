use iced::Color;
use alacritty_terminal::vte::ansi::{self, NamedColor};

/// Default terminal color palette (matches common dark themes).
pub struct TerminalPalette {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    pub ansi: [Color; 16],
}

impl Default for TerminalPalette {
    fn default() -> Self {
        Self {
            foreground: Color::from_rgb(0.85, 0.87, 0.90),
            background: Color::from_rgb(0.055, 0.071, 0.067),
            cursor: Color::from_rgb(0.133, 0.60, 0.569),
            // Standard 16 ANSI colors (Catppuccin-ish)
            ansi: [
                Color::from_rgb(0.18, 0.19, 0.25), // Black
                Color::from_rgb(0.95, 0.55, 0.55), // Red
                Color::from_rgb(0.65, 0.89, 0.63), // Green
                Color::from_rgb(0.98, 0.82, 0.52), // Yellow
                Color::from_rgb(0.54, 0.71, 0.98), // Blue
                Color::from_rgb(0.80, 0.62, 0.95), // Magenta
                Color::from_rgb(0.58, 0.89, 0.87), // Cyan
                Color::from_rgb(0.73, 0.75, 0.80), // White
                Color::from_rgb(0.36, 0.38, 0.46), // Bright Black
                Color::from_rgb(0.95, 0.55, 0.55), // Bright Red
                Color::from_rgb(0.65, 0.89, 0.63), // Bright Green
                Color::from_rgb(0.98, 0.82, 0.52), // Bright Yellow
                Color::from_rgb(0.54, 0.71, 0.98), // Bright Blue
                Color::from_rgb(0.80, 0.62, 0.95), // Bright Magenta
                Color::from_rgb(0.58, 0.89, 0.87), // Bright Cyan
                Color::from_rgb(0.90, 0.91, 0.93), // Bright White
            ],
        }
    }
}

impl TerminalPalette {
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
                    // 256-color palette: compute from index
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
        // Check if terminal has overridden this color
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
            // Dim variants
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
            // Grayscale: 232-255
            let value = ((idx - 232) as f32 * 10.0 + 8.0) / 255.0;
            return Color::from_rgb(value, value, value);
        }
        // 6x6x6 color cube: 16-231
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
