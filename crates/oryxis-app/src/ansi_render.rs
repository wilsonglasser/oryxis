//! Render recorded terminal output (raw bytes with ANSI escapes) into
//! colored text spans for the session-log viewer.
//!
//! A tiny line-oriented emulator rather than a strip pass: carriage
//! returns overwrite the line (so progress bars and redrawn prompts
//! don't smear into "oot@host~root@host" artifacts), erase-line is
//! honored, OSC/CSI/charset sequences are consumed instead of leaking
//! replacement glyphs, and SGR color codes map onto the active
//! terminal palette so the dump reads like the terminal did.

use iced::Color;
use oryxis_terminal::TerminalPalette;

/// One run of same-colored text. `color: None` means the palette's
/// default foreground (resolved at view time so theme switches while
/// the viewer is open don't strand a stale color).
#[derive(Debug, Clone)]
pub(crate) struct AnsiSpan {
    pub text: String,
    pub color: Option<Color>,
}

type Cell = (char, Option<Color>);

/// Map an SGR-selected color index (0-255) onto a concrete color:
/// 0-15 from the theme palette, 16-231 from the 6x6x6 cube, 232-255
/// from the grayscale ramp.
fn indexed_color(idx: u8, palette: &TerminalPalette) -> Color {
    match idx {
        0..=15 => palette.ansi[idx as usize],
        16..=231 => {
            let i = idx - 16;
            let comp = |v: u8| -> f32 {
                if v == 0 { 0.0 } else { (55 + 40 * v as u16) as f32 / 255.0 }
            };
            Color::from_rgb(comp(i / 36), comp((i / 6) % 6), comp(i % 6))
        }
        _ => {
            let v = (8 + 10 * (idx - 232) as u16) as f32 / 255.0;
            Color::from_rgb(v, v, v)
        }
    }
}

/// Pen state driven by SGR sequences. Bold promotes the 8 base colors
/// to their bright variants, matching most terminal themes.
#[derive(Default, Clone, Copy)]
struct Pen {
    /// Base ANSI index (0-7) when set via 30-37, kept so a later bold
    /// can re-promote it.
    base_idx: Option<u8>,
    color: Option<Color>,
    bold: bool,
}

impl Pen {
    fn apply_sgr(&mut self, params: &[u16], palette: &TerminalPalette) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => *self = Pen::default(),
                1 => {
                    self.bold = true;
                    if let Some(b) = self.base_idx {
                        self.color = Some(palette.ansi[(b + 8) as usize]);
                    }
                }
                22 => {
                    self.bold = false;
                    if let Some(b) = self.base_idx {
                        self.color = Some(palette.ansi[b as usize]);
                    }
                }
                30..=37 => {
                    let b = (params[i] - 30) as u8;
                    self.base_idx = Some(b);
                    let idx = if self.bold { b + 8 } else { b };
                    self.color = Some(palette.ansi[idx as usize]);
                }
                90..=97 => {
                    let b = (params[i] - 90) as u8;
                    self.base_idx = Some(b);
                    self.color = Some(palette.ansi[(b + 8) as usize]);
                }
                39 => {
                    self.base_idx = None;
                    self.color = None;
                }
                38 => {
                    // Extended fg: 38;5;n or 38;2;r;g;b.
                    self.base_idx = None;
                    if params.get(i + 1) == Some(&5)
                        && let Some(&n) = params.get(i + 2)
                    {
                        self.color = Some(indexed_color(n as u8, palette));
                        i += 2;
                    } else if params.get(i + 1) == Some(&2)
                        && let (Some(&r), Some(&g), Some(&b)) =
                            (params.get(i + 2), params.get(i + 3), params.get(i + 4))
                    {
                        self.color = Some(Color::from_rgb8(r as u8, g as u8, b as u8));
                        i += 4;
                    }
                }
                48 => {
                    // Extended bg: consume its arguments, ignore the color
                    // (the viewer renders foreground only).
                    if params.get(i + 1) == Some(&5) {
                        i += 2;
                    } else if params.get(i + 1) == Some(&2) {
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// Parse recorded bytes into colored spans. The cursor model is
/// one-dimensional per line: `\r` rewinds, printable chars overwrite,
/// `\n` commits. Full-screen cursor addressing (TUIs) degrades to
/// appended lines, which is the best a linear dump can do.
pub(crate) fn render(data: &[u8], palette: &TerminalPalette) -> Vec<AnsiSpan> {
    let text = String::from_utf8_lossy(data);
    let mut chars = text.chars().peekable();

    let mut lines: Vec<Vec<Cell>> = Vec::new();
    let mut line: Vec<Cell> = Vec::new();
    let mut col: usize = 0;
    let mut pen = Pen::default();

    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    // CSI: numeric/; params then a final byte 0x40-0x7e.
                    let mut params: Vec<u16> = Vec::new();
                    let mut cur: Option<u16> = None;
                    let mut fin = '\0';
                    for c in chars.by_ref() {
                        match c {
                            '0'..='9' => {
                                let d = c as u16 - '0' as u16;
                                cur = Some(cur.unwrap_or(0).saturating_mul(10).saturating_add(d));
                            }
                            ';' | ':' => {
                                params.push(cur.take().unwrap_or(0));
                            }
                            '?' | '>' | '<' | '=' | ' ' | '!' | '"' | '#' | '$' | '%' => {}
                            c if ('\u{40}'..='\u{7e}').contains(&c) => {
                                fin = c;
                                break;
                            }
                            _ => break,
                        }
                    }
                    if let Some(v) = cur.take() {
                        params.push(v);
                    }
                    match fin {
                        'm' => {
                            if params.is_empty() {
                                params.push(0);
                            }
                            pen.apply_sgr(&params, palette);
                        }
                        // Erase in line: 0/default = cursor to end.
                        'K' if params.first().copied().unwrap_or(0) == 0 => {
                            line.truncate(col);
                        }
                        'K' if params.first() == Some(&2) => {
                            line.clear();
                            col = 0;
                        }
                        // Cursor forward: pad with spaces when past the end.
                        'C' => {
                            col += params.first().copied().unwrap_or(1).max(1) as usize;
                        }
                        _ => {}
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: terminated by BEL or ST (ESC \).
                    let mut prev_esc = false;
                    for c in chars.by_ref() {
                        if c == '\x07' || (prev_esc && c == '\\') {
                            break;
                        }
                        prev_esc = c == '\x1b';
                    }
                }
                Some('(') | Some(')') => {
                    // Charset designation: ESC ( B etc., two chars total.
                    chars.next();
                    chars.next();
                }
                _ => {
                    // Other ESC x: consume the single following char.
                    chars.next();
                }
            },
            '\n' => {
                lines.push(std::mem::take(&mut line));
                col = 0;
            }
            '\r' => col = 0,
            '\x08' => col = col.saturating_sub(1),
            '\t' => col = (col / 8 + 1) * 8,
            c if c.is_control() => {}
            c => {
                while line.len() < col {
                    line.push((' ', None));
                }
                if col < line.len() {
                    line[col] = (c, pen.color);
                } else {
                    line.push((c, pen.color));
                }
                col += 1;
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }

    // Compress cell rows into same-color spans (newlines included in
    // the span text so the viewer renders one Rich text).
    let mut spans: Vec<AnsiSpan> = Vec::new();
    let push = |text: String, color: Option<Color>, spans: &mut Vec<AnsiSpan>| {
        if text.is_empty() {
            return;
        }
        if let Some(last) = spans.last_mut()
            && last.color.map(color_key) == color.map(color_key)
        {
            last.text.push_str(&text);
            return;
        }
        spans.push(AnsiSpan { text, color });
    };
    for row in &lines {
        let mut run = String::new();
        let mut run_color: Option<Color> = None;
        for (c, color) in row {
            if color.map(color_key) != run_color.map(color_key) && !run.is_empty() {
                push(std::mem::take(&mut run), run_color, &mut spans);
            }
            run_color = *color;
            run.push(*c);
        }
        run.push('\n');
        push(run, run_color, &mut spans);
    }
    spans
}

/// Comparable key for a Color (f32 fields aren't Eq).
fn color_key(c: Color) -> [u32; 4] {
    [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(data: &[u8]) -> String {
        let palette = TerminalPalette::default();
        render(data, &palette).iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn carriage_return_overwrites_the_line() {
        // The shell redraws the prompt over itself; the dump must not
        // smear both renders together.
        assert_eq!(
            flat(b"root@host:~# \rroot@host:~# ls\n"),
            "root@host:~# ls\n"
        );
        // Progress-bar style updates keep only the final state.
        assert_eq!(flat(b"10%\r50%\r100%\n"), "100%\n");
    }

    #[test]
    fn erase_line_truncates() {
        assert_eq!(flat(b"hello world\r\x1b[Khi\n"), "hi\n");
    }

    #[test]
    fn osc_and_charset_sequences_vanish() {
        assert_eq!(flat(b"\x1b]0;window title\x07ok\n"), "ok\n");
        assert_eq!(flat(b"\x1b(Bok\x1b)0\n"), "ok\n");
    }

    #[test]
    fn sgr_colors_map_to_palette() {
        let palette = TerminalPalette::default();
        let spans = render(b"\x1b[31mred\x1b[0m plain\n", &palette);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "red");
        assert_eq!(spans[0].color.map(color_key), Some(color_key(palette.ansi[1])));
        assert_eq!(spans[1].text, " plain\n");
        assert!(spans[1].color.is_none());
    }

    #[test]
    fn bold_promotes_to_bright() {
        let palette = TerminalPalette::default();
        let spans = render(b"\x1b[1;32mok\n", &palette);
        assert_eq!(spans[0].color.map(color_key), Some(color_key(palette.ansi[10])));
    }
}
