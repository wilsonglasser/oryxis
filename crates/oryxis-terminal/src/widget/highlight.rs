use super::*;

#[derive(Clone, Copy, PartialEq)]
enum HighlightKind {
    Url,
    Ip,
    Path,
    Number,
}

pub(crate) struct Highlight {
    row: u16,
    start_col: u16,
    end_col: u16, // inclusive
    color: Color,
    kind: HighlightKind,
}

/// Scan row text for IPv4 addresses, URLs, and Unix file paths (no regex).
/// Takes `(row, non-blank cells)` pairs; rows with no printable chars are
/// simply absent (the draw pass builds this per frame, so a dense Vec
/// beats re-hashing every row into a map).
pub(crate) fn detect_highlights(
    row_chars: &[(u16, Vec<(u16, char)>)],
    palette: &TerminalPalette,
) -> Vec<Highlight> {
    let ip_color = palette.ansi[5];   // magenta
    let url_color = palette.ansi[4];  // blue
    let path_color = palette.ansi[6]; // cyan
    let num_color = palette.ansi[5];  // magenta, same as IP, easy scan

    let mut highlights = Vec::new();

    for (row, cols) in row_chars {
        let row = *row;
        let max_col = cols.iter().map(|(c, _)| *c).max().unwrap_or(0) as usize;
        let mut chars = vec![' '; max_col + 1];
        for &(col, ch) in cols {
            if (col as usize) <= max_col {
                chars[col as usize] = ch;
            }
        }
        let row_str: String = chars.iter().collect();
        let bytes = row_str.as_bytes();
        let len = bytes.len();

        // --- URLs: "http://" or "https://" followed by non-whitespace ---
        {
            let mut i = 0;
            while i < len {
                // Only slice at ASCII 'h', guaranteed char boundary. Skipping this
                // guard panics when i lands mid-UTF-8 (e.g. typing "ç" crashed the app).
                if bytes[i] != b'h' {
                    i += 1;
                    continue;
                }
                let rest = &row_str[i..];
                if rest.starts_with("http://") || rest.starts_with("https://") {
                    let start = i;
                    let mut end = i;
                    for ch in row_str[i..].chars() {
                        if ch.is_whitespace() || ch == '\0' {
                            break;
                        }
                        end += ch.len_utf8();
                    }
                    if end > start {
                        while end > start {
                            let last = bytes[end - 1];
                            if last == b')' || last == b']' || last == b'>'
                                || last == b',' || last == b'.' || last == b';'
                            {
                                end -= 1;
                            } else {
                                break;
                            }
                        }
                        highlights.push(Highlight {
                            row,
                            start_col: start as u16,
                            end_col: (end - 1) as u16,
                            color: url_color,
                            kind: HighlightKind::Url,
                        });
                        i = end;
                        continue;
                    }
                }
                i += 1;
            }
        }

        // --- IPv4: digit groups separated by dots (4 groups, each 0-255) ---
        {
            let mut i = 0;
            while i < len {
                if bytes[i].is_ascii_digit() {
                    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.') {
                        i += 1;
                        continue;
                    }
                    let start = i;
                    let mut groups = 0u8;
                    let mut j = i;
                    loop {
                        let group_start = j;
                        while j < len && bytes[j].is_ascii_digit() {
                            j += 1;
                        }
                        let group_len = j - group_start;
                        if group_len == 0 || group_len > 3 {
                            break;
                        }
                        if let Ok(val) = row_str[group_start..j].parse::<u16>() {
                            if val > 255 { break; }
                        } else {
                            break;
                        }
                        groups += 1;
                        if groups == 4 { break; }
                        if j < len && bytes[j] == b'.' {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if groups == 4 {
                        if j < len && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'.') {
                            i += 1;
                            continue;
                        }
                        let dominated = highlights.iter().any(|h| {
                            h.row == row && start as u16 >= h.start_col && (start as u16) <= h.end_col
                        });
                        if !dominated {
                            highlights.push(Highlight {
                                row,
                                start_col: start as u16,
                                end_col: (j - 1) as u16,
                                color: ip_color,
                                kind: HighlightKind::Ip,
                            });
                        }
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
        }

        // --- Unix file paths: "/" followed by alphanumeric/dot/dash/underscore/slash ---
        {
            let mut i = 0;
            while i < len {
                if bytes[i] == b'/' {
                    if i > 0 {
                        let prev = bytes[i - 1];
                        if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'-' || prev == b'.' {
                            i += 1;
                            continue;
                        }
                    }
                    let start = i;
                    let mut j = i + 1;
                    while j < len {
                        let b = bytes[j];
                        if b.is_ascii_alphanumeric()
                            || b == b'.' || b == b'-' || b == b'_' || b == b'/' || b == b'~'
                        {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if j - start >= 3 {
                        while j > start + 1 && (bytes[j - 1] == b'.' || bytes[j - 1] == b'/') {
                            j -= 1;
                        }
                        let dominated = highlights.iter().any(|h| {
                            h.row == row && start as u16 >= h.start_col && (start as u16) <= h.end_col
                        });
                        if !dominated && j - start >= 3 {
                            highlights.push(Highlight {
                                row,
                                start_col: start as u16,
                                end_col: (j - 1) as u16,
                                color: path_color,
                                kind: HighlightKind::Path,
                            });
                        }
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
        }

        // --- Standalone numbers: int/float, optional minus, optional %.
        // Examples: 1634, -273.1, 23.3%, 0.0. Skipped when the run is part
        // of an existing highlight (IP/path/URL) or is inside a word.
        {
            let mut i = 0;
            while i < len {
                let b = bytes[i];
                let is_start = b.is_ascii_digit()
                    || (b == b'-'
                        && i + 1 < len
                        && bytes[i + 1].is_ascii_digit()
                        && (i == 0 || !is_word_byte(bytes[i - 1])));
                if !is_start {
                    i += 1;
                    continue;
                }
                // Reject when prefixed by a word character (e.g. "abc123",
                // version strings), those should keep the surrounding fg.
                if i > 0 && b.is_ascii_digit() && is_word_byte(bytes[i - 1]) {
                    i += 1;
                    continue;
                }
                let start = i;
                let mut j = i;
                if bytes[j] == b'-' {
                    j += 1;
                }
                while j < len && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                // Optional decimal part, must be `.<digit>+`.
                if j + 1 < len && bytes[j] == b'.' && bytes[j + 1].is_ascii_digit() {
                    j += 1;
                    while j < len && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                // Optional trailing percent.
                if j < len && bytes[j] == b'%' {
                    j += 1;
                }
                // Reject when followed by a letter (e.g. "10.0.0.1",
                // "v1.2-rc", the IP path already handled the first; we
                // also avoid colouring "rc" parts).
                if j < len && is_word_byte(bytes[j]) {
                    i = j;
                    continue;
                }
                let dominated = highlights.iter().any(|h| {
                    h.row == row
                        && start as u16 >= h.start_col
                        && (start as u16) <= h.end_col
                });
                if !dominated && j > start {
                    highlights.push(Highlight {
                        row,
                        start_col: start as u16,
                        end_col: (j - 1) as u16,
                        color: num_color,
                        kind: HighlightKind::Number,
                    });
                }
                i = j;
            }
        }
    }

    highlights
}

/// WCAG 2.x relative luminance for an sRGB colour in `[0, 1]`. Used by
/// the smart-contrast fallback to decide whether a too-close cell
/// should flip its foreground to white or near-black.
pub(crate) fn relative_luminance(c: Color) -> f32 {
    fn channel(v: f32) -> f32 {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// WCAG contrast ratio between two opaque colours: 1.0 = identical,
/// 21.0 = white-on-black. We trip the smart-contrast fallback below
/// `2.5`, well under the AA-body threshold of `4.5` so we only act
/// on visually disappearing pairs and leave merely-low-contrast
/// styling alone.
pub(crate) fn contrast_ratio(a: Color, b: Color) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Check if a cell position falls within any highlight, returning the color.
#[inline]
pub(crate) fn highlight_color_at(highlights: &[Highlight], row: u16, col: u16) -> Option<Color> {
    for h in highlights {
        if h.row == row && col >= h.start_col && col <= h.end_col {
            return Some(h.color);
        }
    }
    None
}

/// Returns true when the given cell is part of a URL highlight, used by the
/// draw pass to paint an underline under clickable links.
#[inline]
/// Find the URL highlight that contains a specific cell, used by the
/// draw pass to underline only the URL the cursor is over (instead of
/// every URL in the viewport, which made even un-hovered links look
/// "linkable" with no Ctrl-click feedback).
pub(crate) fn hovered_url_range(
    highlights: &[Highlight],
    row: u16,
    col: u16,
) -> Option<(u16, u16, u16)> {
    highlights
        .iter()
        .find(|h| {
            h.kind == HighlightKind::Url
                && h.row == row
                && col >= h.start_col
                && col <= h.end_col
        })
        .map(|h| (h.row, h.start_col, h.end_col))
}

/// Extract the URL string at a given cell from the current viewport, if any.
/// Walks the row the cursor is on, finds the URL highlight that covers the
/// column, and returns the full URL text. Returns `None` when the click
/// lands outside any URL.
pub(crate) fn url_at_cell(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    target_line: i32,
    target_col: u16,
) -> Option<String> {
    use alacritty_terminal::index::{Column, Line};
    // Index the one grid row directly (the way `smart_span_at` does)
    // instead of walking the whole viewport display iterator to pick
    // a single row out of it. `target_line` is a grid line (scroll
    // adjusted, negative for scrollback), not an on-screen row, so
    // Ctrl+click and hover stay correct when scrolled into history.
    let grid = term.grid();
    let line = Line(target_line);
    if line < grid.topmost_line() || line > grid.bottommost_line() {
        return None;
    }
    let row_data = &grid[line];
    let ncols = grid.columns();
    let mut row_chars: Vec<(u16, char)> = Vec::with_capacity(ncols);
    for ci in 0..ncols {
        let c = row_data[Column(ci)].c;
        if c != ' ' && c != '\0' {
            row_chars.push((ci as u16, c));
        }
    }
    if row_chars.is_empty() {
        return None;
    }

    let max_col = row_chars.iter().map(|(c, _)| *c).max().unwrap_or(0) as usize;
    let mut chars = vec![' '; max_col + 1];
    for &(col, ch) in &row_chars {
        if (col as usize) <= max_col {
            chars[col as usize] = ch;
        }
    }
    let row_str: String = chars.iter().collect();
    let bytes = row_str.as_bytes();
    let len = bytes.len();

    let mut i = 0;
    while i < len {
        if bytes[i] != b'h' {
            i += 1;
            continue;
        }
        let rest = &row_str[i..];
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let start = i;
            let mut end = i;
            for ch in row_str[i..].chars() {
                if ch.is_whitespace() || ch == '\0' {
                    break;
                }
                end += ch.len_utf8();
            }
            if end > start {
                while end > start {
                    let last = bytes[end - 1];
                    if last == b')' || last == b']' || last == b'>'
                        || last == b',' || last == b'.' || last == b';'
                    {
                        end -= 1;
                    } else {
                        break;
                    }
                }
                if (start as u16) <= target_col && target_col <= (end - 1) as u16 {
                    return Some(row_str[start..end].to_string());
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    None
}

/// Smart-select span for double-click: if the cell at grid-line `line`,
/// column `col` falls inside a detected URL / IP / path token, return its
/// `(start_col, end_col)` (inclusive). Returns `None` otherwise (caller
/// falls back to delimiter-word selection). Numbers are excluded, they are
/// too granular to be a useful "word" target. Reads the grid directly by
/// line so it stays correct when scrolled into history (unlike
/// `url_at_cell`, which indexes by on-screen row number and so only
/// matches the live screen).
pub(crate) fn smart_span_at(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    palette: &TerminalPalette,
    line: i32,
    col: u16,
) -> Option<(u16, u16)> {
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line};
    let grid = term.grid();
    let l = Line(line);
    if l < grid.topmost_line() || l > grid.bottommost_line() {
        return None;
    }
    let row = &grid[l];
    let ncols = grid.columns();
    let mut present = vec![false; ncols];
    let mut cols: Vec<(u16, char)> = Vec::new();
    for ci in 0..ncols {
        let c = row[Column(ci)].c;
        if c != ' ' && c != '\0' {
            present[ci] = true;
            cols.push((ci as u16, c));
        }
    }
    if cols.is_empty() || !present.get(col as usize).copied().unwrap_or(false) {
        return None;
    }
    // Expand to the whitespace-bounded token containing the click.
    let mut left = col;
    while left > 0 && present[left as usize - 1] {
        left -= 1;
    }
    let mut right = col;
    while (right as usize + 1) < ncols && present[right as usize + 1] {
        right += 1;
    }
    // Trigger only when that token overlaps a detected URL / IP / path
    // highlight, so plain prose words still fall through to delimiter-word
    // selection. The highlighter's own URL span may be shorter than the
    // token (its matcher is loose), hence the overlap test rather than a
    // containment test. `detect_highlights` takes (row, cells) pairs; a
    // single synthetic row 0 is enough as long as we match on the same key.
    let rows = [(0u16, cols)];
    let hit = detect_highlights(&rows, palette).into_iter().any(|h| {
        h.row == 0
            && h.kind != HighlightKind::Number
            && h.start_col <= right
            && h.end_col >= left
    });
    hit.then_some((left, right))
}
