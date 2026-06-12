use crate::backend::TerminalBackend;
use crate::colors::TerminalPalette;
use crate::mouse::{self as mouse_report, Mods as ReportMods, MouseButton as ReportButton, MouseEventKind};
use crate::pty::PtyHandle;

/// Common result type for terminal operations.
pub type TerminalResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::CursorShape;

use iced::alignment;
use iced::widget::canvas::{self, Action as CanvasAction, Frame, Geometry, Text as CanvasText};
use iced::{keyboard, mouse, Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Bundled glyph-fallback font for the Unicode Private Use Area
/// (Powerline / Font Awesome / Devicons / Octicons / Codicons /
/// Material). Points at Symbols Nerd Font (loaded into the fontdb
/// in `main.rs` via `include_bytes!`) rather than SauceCodePro Nerd
/// Font: cosmic-text's canvas `font:` parameter is a hard pick, not
/// a fallback chain, so any PUA codepoint SauceCodePro happens to
/// miss (Material Design Icons + some Codicons in certain patched
/// builds) would render as tofu instead of falling through. Symbols
/// Nerd Font is the official NF "symbols-only" drop-in built for
/// universal PUA coverage, so we route every PUA codepoint to it.
const NERD_FONT: Font = Font::new("Symbols Nerd Font");

/// Bracketed-paste start marker (`ESC [ 200 ~`).
const PASTE_START: &[u8] = b"\x1b[200~";
/// Bracketed-paste end marker (`ESC [ 201 ~`).
const PASTE_END: &[u8] = b"\x1b[201~";

/// Prepare clipboard text for writing to a terminal session.
///
/// When `bracketed` is true (the focused app enabled DECSET 2004), wrap the
/// payload in `ESC [ 200 ~` ... `ESC [ 201 ~` so readline / TUI programs
/// (bash, zsh, Codex CLI, ...) treat the whole block as one paste and only
/// submit when the user presses Enter, instead of one submit per embedded
/// newline. Any marker already present in the clipboard is stripped first so
/// the payload can't prematurely close (or reopen) the bracket.
///
/// When `bracketed` is false the text is returned unchanged, so plain shells
/// that never requested the mode are unaffected.
pub fn wrap_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }
    let sanitized = text.replace("\x1b[200~", "").replace("\x1b[201~", "");
    let mut out = Vec::with_capacity(sanitized.len() + PASTE_START.len() + PASTE_END.len());
    out.extend_from_slice(PASTE_START);
    out.extend_from_slice(sanitized.as_bytes());
    out.extend_from_slice(PASTE_END);
    out
}

// ---------------------------------------------------------------------------
// Terminal State
// ---------------------------------------------------------------------------

pub struct TerminalState {
    pub backend: TerminalBackend,
    pub pty: Option<PtyHandle>,
    pub palette: TerminalPalette,
    /// When this state is attached to an SSH session, resize events are
    /// forwarded here so the remote shell sees `window-change` and apps
    /// like `top`/`vim` re-layout instead of wrapping into our local grid.
    remote_resize_tx: Option<mpsc::UnboundedSender<(u16, u16)>>,
}

impl TerminalState {
    pub fn new(
        cols: u16,
        rows: u16,
    ) -> TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        let backend = TerminalBackend::new(cols, rows);
        let (pty, rx) = PtyHandle::spawn(cols, rows, &backend.event_proxy)?;
        let palette = TerminalPalette::default();
        Ok((Self { backend, pty: Some(pty), palette, remote_resize_tx: None }, rx))
    }

    /// Like `new` but spawns an explicit program (e.g. PowerShell or
    /// `wsl.exe -d Ubuntu`) instead of the OS default shell. Used by
    /// the Local Shell picker on Windows.
    pub fn new_with_command(
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
    ) -> TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        let backend = TerminalBackend::new(cols, rows);
        let (pty, rx) = PtyHandle::spawn_command(
            cols, rows, Some(program), args, &backend.event_proxy,
        )?;
        let palette = TerminalPalette::default();
        Ok((Self { backend, pty: Some(pty), palette, remote_resize_tx: None }, rx))
    }

    pub fn new_no_pty(
        cols: u16,
        rows: u16,
    ) -> TerminalResult<Self> {
        let backend = TerminalBackend::new(cols, rows);
        let palette = TerminalPalette::default();
        Ok(Self { backend, pty: None, palette, remote_resize_tx: None })
    }

    /// Wire a remote resize sender, called from the app once an SSH
    /// session attaches to this state, so subsequent `resize()` calls
    /// also notify the server of the new viewport.
    pub fn set_remote_resize_sender(
        &mut self,
        tx: mpsc::UnboundedSender<(u16, u16)>,
    ) {
        self.remote_resize_tx = Some(tx);
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.backend.process(bytes);
    }

    pub fn write(&mut self, data: &[u8]) {
        if let Some(ref pty) = self.pty
            && let Err(e) = pty.write(data) {
                tracing::error!("PTY write error: {}", e);
            }
    }

    /// True when the focused application has enabled bracketed paste mode
    /// (DECSET 2004, `ESC [ ? 2004 h`). Callers wrap pasted clipboard text
    /// in bracket markers so embedded newlines arrive as literal characters
    /// instead of one Enter per line. The backend tracks this even over SSH
    /// because remote output is fed through `process()` into the same term.
    pub fn bracketed_paste_enabled(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        self.backend.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        if cols == self.backend.cols() && rows == self.backend.rows() {
            return false;
        }
        if cols < 2 || rows < 2 {
            return false;
        }
        self.backend.resize(cols, rows);
        if let Some(ref pty) = self.pty {
            let _ = pty.resize(cols, rows);
        }
        if let Some(ref tx) = self.remote_resize_tx {
            let _ = tx.send((cols, rows));
        }
        true
    }

    pub fn cols(&self) -> u16 { self.backend.cols() }
    pub fn rows(&self) -> u16 { self.backend.rows() }

    /// Visible cursor cell as `(column, line)`, 0-based from the top-left of
    /// the active screen. Used to anchor the OS IME candidate window near the
    /// caret. Ignores the widget's scrollback offset (during composition the
    /// view sits at the bottom), so it is exact while typing and only
    /// approximate if the user has scrolled into history.
    pub fn cursor_cell(&self) -> (u16, u16) {
        let p = self.backend.term.renderable_content().cursor.point;
        (p.column.0 as u16, p.line.0.max(0) as u16)
    }

    /// Extract text from a selection range.
    pub fn get_selection_text(&self, sel: &Selection) -> String {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        let grid = self.backend.term.grid();
        let topmost = grid.topmost_line();
        let bottommost = grid.bottommost_line();
        let cols = grid.columns();
        let last_col = cols.saturating_sub(1) as u16;

        // Block (column) selection: every row takes the same column slice.
        // The slice is kept verbatim, including trailing spaces, so the
        // rectangle preserves its column alignment (trimming would ragged
        // a multi-column block, e.g. two columns of a table).
        if sel.block {
            let (c0, c1, l0, l1) = sel.block_bounds();
            let mut rows: Vec<String> = Vec::new();
            for line_idx in l0..=l1 {
                let line = Line(line_idx);
                if !(topmost..=bottommost).contains(&line) {
                    rows.push(String::new());
                    continue;
                }
                let row = &grid[line];
                let mut line_str = String::new();
                for c in c0..=c1.min(last_col) {
                    let cell = &row[Column(c as usize)];
                    if cell.c != '\0' {
                        line_str.push(cell.c);
                    }
                }
                rows.push(line_str);
            }
            return rows.join("\n");
        }

        let (start, end) = sel.ordered();
        // Iterate over the line range manually, selection lines are in
        // grid coordinates (negative for scrollback) which `display_iter`
        // alone wouldn't reach unless we mutated the display offset.
        // Each row is trimmed of trailing whitespace before joining, the
        // standard terminal behaviour so a wrapped/multi-line copy doesn't
        // carry the blank padding out to the right margin.
        let mut rows: Vec<String> = Vec::new();
        for line_idx in start.1..=end.1 {
            let line = Line(line_idx);
            if line < topmost || line > bottommost {
                continue;
            }
            let row = &grid[line];
            let (start_col, end_col) = if start.1 == end.1 {
                (start.0, end.0)
            } else if line_idx == start.1 {
                (start.0, last_col)
            } else if line_idx == end.1 {
                (0, end.0)
            } else {
                (0, last_col)
            };
            let mut line_str = String::new();
            for c in start_col..=end_col {
                let cell = &row[Column(c as usize)];
                if cell.c != '\0' {
                    line_str.push(cell.c);
                }
            }
            rows.push(line_str.trim_end().to_string());
        }

        rows.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Selection tracking
// ---------------------------------------------------------------------------

/// A selection range stored in **grid-line coordinates** (alacritty `Line`,
/// signed: negative = scrollback, 0..screen_lines = live screen). Storing
/// in line-space means the selection follows the content as the user
/// scrolls, at draw time we translate line → visible row using the
/// current scroll_offset.
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub start: (u16, i32), // (col, line)
    pub end: (u16, i32),
    /// When true this is a rectangular (column / block) selection
    /// (Alt+drag): every row takes the same `[min_col, max_col]` slice,
    /// instead of the flowing line-by-line range. Carried so the draw
    /// highlight and text extraction agree.
    pub block: bool,
}

impl Selection {
    /// Returns (start, end) ordered top-left to bottom-right. Only
    /// meaningful for flowing (non-block) selections; block selections
    /// use [`Selection::block_bounds`] instead.
    pub fn ordered(&self) -> ((u16, i32), (u16, i32)) {
        if self.start.1 < self.end.1
            || (self.start.1 == self.end.1 && self.start.0 <= self.end.0)
        {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    /// `(min_col, max_col, min_line, max_line)` of the rectangle spanned
    /// by the two corners. Used for block selections, where each axis is
    /// ordered independently.
    pub fn block_bounds(&self) -> (u16, u16, i32, i32) {
        (
            self.start.0.min(self.end.0),
            self.start.0.max(self.end.0),
            self.start.1.min(self.end.1),
            self.start.1.max(self.end.1),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Granularity of a double/triple-click selection. A plain single-click
/// drag uses per-cell granularity and is represented by `None` in the
/// widget's `select_anchor`, so this enum only needs the two expanded
/// modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectGranularity {
    Word,
    Line,
    Paragraph,
}

/// Next click count given the previous count and whether this press is
/// consecutive (within the time + distance window of the last one). Cycles
/// 1->2->3->4->1 so a fifth rapid click restarts at single-click, matching
/// the iTerm / GNOME behaviour. Pure so the classification can be tested
/// without driving real mouse timing.
fn next_click_count(prev: Option<u8>, consecutive: bool) -> u8 {
    match prev {
        Some(c) if consecutive => (c % 4) + 1,
        _ => 1,
    }
}

/// Combine two selections into the range spanning from the earliest
/// ordered start to the latest ordered end. Used to extend a word/line
/// drag: the anchor's word/line unioned with the cursor's word/line.
fn union_selection(a: Selection, b: Selection) -> Selection {
    let (a0, a1) = a.ordered();
    let (b0, b1) = b.ordered();
    // Compare cells in (line, col) order.
    let lt = |x: (u16, i32), y: (u16, i32)| x.1 < y.1 || (x.1 == y.1 && x.0 < y.0);
    Selection {
        start: if lt(a0, b0) { a0 } else { b0 },
        end: if lt(a1, b1) { b1 } else { a1 },
        block: false,
    }
}

// ---------------------------------------------------------------------------
// Canvas widget state (per-instance, managed by Iced)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct TerminalWidgetState {
    selecting: bool,
    selection: Option<Selection>,
    scroll_offset: i32, // lines scrolled back (0 = bottom)
    /// True while the cursor is somewhere over the terminal canvas. Drives
    /// the scrollbar's hover-to-reveal visibility.
    hover: bool,
    /// `Some((cursor_y_at_press, scroll_offset_at_press))` while the user
    /// is dragging the scrollbar thumb.
    scrollbar_drag: Option<(f32, i32)>,
    /// Latest known modifier mask, refreshed on every keyboard event.
    /// Drives the Ctrl+Click-to-open-link UX (Termius-style: plain
    /// clicks select, Ctrl+Click follows the URL).
    modifiers: iced::keyboard::Modifiers,
    /// Currently hovered URL + the cursor pixel position. Used by the
    /// canvas to underline only the hovered URL (not all of them) and
    /// by the parent to render the "Ctrl+Click to open" tooltip.
    hovered_url: Option<(String, iced::Point)>,
    /// Last `(col, row)` the URL hover detection ran for. Used to skip
    /// the lock + per-cell scan on sub-cell mouse moves, at typical
    /// font sizes the cursor crosses many pixels per cell, and running
    /// the full URL scan on every pixel contends with `state.process`
    /// when the SSH echo lands at the same time, showing up as typing
    /// lag.
    hovered_cell: Option<(u16, u16)>,
    /// Button currently held down while the remote app has mouse
    /// tracking on. Drives drag-motion reports (which carry the held
    /// button) and the matching release report. `None` when no button
    /// is down or the app isn't tracking the mouse.
    report_button: Option<ReportButton>,
    /// Last `(col, row)` reported to the remote app, used to suppress
    /// duplicate motion reports while the cursor stays inside one cell.
    report_cell: Option<(u16, u16)>,
    /// Previous left-click as `(time, position, count)`, used to classify
    /// the next press as single / double / triple / quad (300 ms / 6 px
    /// window). Rolled here rather than via `iced`'s `mouse::Click` because
    /// that caps at triple and we need a fourth count for paragraph select.
    last_click: Option<(std::time::Instant, Point, u8)>,
    /// `Some((granularity, anchor_cell))` while a double/triple-click
    /// selection is active, so a drag extends by whole words/lines
    /// instead of by cell. `None` for a plain single-click drag.
    select_anchor: Option<(SelectGranularity, (u16, i32))>,
    /// Last grid cell the word/line drag recomputed against. Throttles
    /// the union recompute to one per cell crossing (the recompute locks
    /// the mutex + runs two semantic searches; running it per pixel
    /// would contend with the SSH echo path, see the URL-hover note).
    last_extend_cell: Option<(u16, i32)>,
    /// Time of the last edge auto-scroll step. Rate-limits the scroll so
    /// its speed is tied to wall-clock, not the (very high) mouse-move
    /// event rate, which otherwise made the buffer rocket past the edge.
    last_autoscroll: Option<std::time::Instant>,
}

// ---------------------------------------------------------------------------
// Syntax highlighting for IPs, URLs, and file paths
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum HighlightKind {
    Url,
    Ip,
    Path,
    Number,
}

struct Highlight {
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
fn detect_highlights(
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
fn relative_luminance(c: Color) -> f32 {
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
fn contrast_ratio(a: Color, b: Color) -> f32 {
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
fn highlight_color_at(highlights: &[Highlight], row: u16, col: u16) -> Option<Color> {
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
fn hovered_url_range(
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
fn url_at_cell(
    term: &alacritty_terminal::Term<crate::backend::EventProxy>,
    target_row: u16,
    target_col: u16,
) -> Option<String> {
    use alacritty_terminal::index::{Column, Line};
    // Index the one grid row directly (the way `smart_span_at` does)
    // instead of walking the whole viewport display iterator to pick
    // a single row out of it.
    let grid = term.grid();
    let line = Line(target_row as i32);
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
fn smart_span_at(
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

/// Write `text` to the system clipboard, best-effort. Errors are swallowed
/// (a backend may be unavailable on a headless box or under a compositor
/// without the data-control protocol); a failed copy should never panic
/// the UI. Shared by the copy-on-select, right-click-copy and Ctrl+Shift+C
/// paths so the three sites stay in sync.
fn set_clipboard_text(text: &str) {
    if let Ok(mut clip) = arboard::Clipboard::new() {
        let _ = clip.set_text(text);
    }
}

/// Best-effort spawn of the OS default handler for a URL. Runs detached; the
/// terminal widget never blocks on it and errors are swallowed, a failed
/// launch just means nothing happens visibly, same as any other click miss.
fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW so the `cmd /C start` shim doesn't flash a
        // console window on the GUI-subsystem app.
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

// ---------------------------------------------------------------------------
// Terminal View
// ---------------------------------------------------------------------------

pub struct TerminalView<Message = ()> {
    state: Arc<Mutex<TerminalState>>,
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
    font: Font,
    /// When true, completing a mouse selection auto-copies it to the
    /// system clipboard, same UX as XTerm / iTerm "copy on select".
    copy_on_select: bool,
    /// Only consulted when `copy_on_select` is on. When true the selection
    /// no longer auto-copies on release; instead a right-click over a live
    /// selection copies it (the Windows console "QuickEdit" model), and a
    /// right-click with no selection still pastes.
    right_click_copy: bool,
    /// When true, ANSI bold flag promotes the named foreground color to
    /// its bright variant (red → bright red, etc).
    bold_is_bright: bool,
    /// When true, the terminal scans visible rows for URLs / IPs / paths
    /// and tints them. Disable to recover frame time in dense UIs.
    keyword_highlight: bool,
    /// When true, cells whose foreground and background end up
    /// perceptually too close (e.g. PowerShell's `$PSStyle.FileInfo
    /// .Directory` blue-on-blue, LS_COLORS' `ow` green-on-green) get
    /// their foreground swapped for a high-contrast alternative so
    /// the text stays legible. Off paints the cell exactly as the
    /// emulator asked, which a few colour-precise tools rely on.
    smart_contrast: bool,
    /// Characters that terminate a word for double-click selection
    /// (the semantic-escape / "word delimiters" set). Threaded from the
    /// user's Terminal setting each frame and synced into the backend on
    /// the next word-select. Defaults to [`crate::backend::DEFAULT_WORD_DELIMITERS`].
    word_delimiters: String,
    /// Optional callback messages for Ctrl+Wheel font zoom. When unset,
    /// Ctrl+Wheel still gets captured but produces no state change.
    on_font_size_increase: Option<Message>,
    on_font_size_decrease: Option<Message>,
    /// Optional callback for right-click paste. When set, the widget
    /// emits this message instead of writing the clipboard text directly
    /// to the local PTY, so the app dispatcher can route to the SSH
    /// session (mirroring the Ctrl+Shift+V path).
    on_paste_request: Option<Message>,
    /// Optional callback for raw input bytes the widget synthesizes
    /// (mouse-tracking reports, wheel-to-arrow translation). Like
    /// `on_paste_request`, this routes the bytes through the dispatcher
    /// so they reach the active SSH session; without it the widget
    /// falls back to a local-PTY write, which is dead on SSH tabs.
    on_terminal_input: Option<Box<dyn Fn(Vec<u8>) -> Message>>,
    /// Localized "Ctrl + Click to open the link" tooltip text. `None`
    /// disables the hover hint entirely (the app stops passing it once
    /// the user has ctrl-clicked a link for the first time).
    link_hint_text: Option<String>,
    /// Emitted after a Ctrl+Click successfully opens a URL, so the app
    /// can persist "the user knows the gesture" and drop the hint.
    on_link_opened: Option<Message>,
    /// Whether this pane currently has focus. Only the focused pane emits
    /// mouse-tracking reports, so a click that merely focuses an inactive
    /// split pane (e.g. one running htop, which leaves mouse mode on)
    /// doesn't inject a stray report into that shell. Defaults to `true`
    /// so the single-pane path is unchanged.
    focused: bool,
}

/// Horizontal padding around the terminal content (left/right).
/// Termius uses ~8 px so the first column doesn't kiss the window
/// border, matched here.
const TERM_PAD: f32 = 8.0;
/// Vertical padding above the first row. Mirrors `TERM_PAD` so
/// horizontal and vertical breathing are symmetric, again matching
/// the Termius spacing. If the canvas still looks padded above the
/// first row of output, the gap isn't coming from here; likely the
/// remote session emits a leading clear / cursor-move sequence that
/// blanks the top rows.
const TERM_PAD_TOP: f32 = 8.0;

/// Screen-space rectangle for the OS IME candidate window, anchored at the
/// terminal caret. `bounds` is the widget's on-screen rect, `font_size` the
/// configured terminal font size, `cell` the cursor cell from
/// [`TerminalState::cursor_cell`]. Mirrors the cursor-rendering math in
/// `draw` so the candidate window lines up with the block cursor.
pub fn ime_caret_rect(bounds: Rectangle, font_size: f32, cell: (u16, u16)) -> Rectangle {
    let cell_w = font_size * 0.6;
    let cell_h = font_size * 1.15;
    let (col, row) = cell;
    let x = bounds.x + col as f32 * cell_w + TERM_PAD;
    let y = bounds.y + row as f32 * cell_h + TERM_PAD_TOP;
    Rectangle::new(Point::new(x, y), Size::new(cell_w.max(1.0), cell_h))
}

/// Rolling per-frame samples for the perf overlay. We track the
/// **max** of each phase over a short window so transient spikes
/// (the kind that actually feel like lag) stay visible for a beat
/// instead of being averaged away.
struct PerfStats {
    /// Last few frames of each phase. Old entries are dropped after
    /// `WINDOW` so the max reflects recent activity, not the whole
    /// session.
    samples: std::collections::VecDeque<PerfSample>,
    /// Wall-clock of the previous draw, used so the overlay can
    /// avoid double-counting frames within a single redraw cycle.
    last_draw_at: Option<std::time::Instant>,
}

#[derive(Clone, Copy)]
struct PerfSample {
    frame_gap: std::time::Duration,
    lock: std::time::Duration,
    cells: std::time::Duration,
    highlights: std::time::Duration,
    total: std::time::Duration,
}

/// Frames retained for the rolling max / fps. ~2s of activity at
/// 60 fps; long enough to catch a typing burst, short enough that
/// the HUD recovers when things calm down.
const PERF_WINDOW: usize = 120;

impl PerfStats {
    fn fps(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let total: std::time::Duration =
            self.samples.iter().map(|s| s.frame_gap).sum();
        let avg = total / self.samples.len() as u32;
        if avg.as_secs_f32() == 0.0 {
            0.0
        } else {
            1.0 / avg.as_secs_f32()
        }
    }

    fn max_lock(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.lock)
            .max()
            .unwrap_or_default()
    }
    fn max_cells(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.cells)
            .max()
            .unwrap_or_default()
    }
    fn max_highlights(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.highlights)
            .max()
            .unwrap_or_default()
    }
    fn max_total(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.total)
            .max()
            .unwrap_or_default()
    }
}

fn perf_stats() -> &'static std::sync::Mutex<PerfStats> {
    static STATS: std::sync::OnceLock<std::sync::Mutex<PerfStats>> =
        std::sync::OnceLock::new();
    STATS.get_or_init(|| {
        std::sync::Mutex::new(PerfStats {
            samples: std::collections::VecDeque::with_capacity(PERF_WINDOW),
            last_draw_at: None,
        })
    })
}

/// Reads the `ORYXIS_TERM_PERF` env var once and caches it. Set to `1`
/// (or any non-empty value) to render a small FPS/timing HUD in the
/// top-right of every terminal canvas.
fn perf_overlay_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("ORYXIS_TERM_PERF")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

/// Visual layout of the scrollbar gutter for a given grid state.
struct ScrollbarGeom {
    track_x: f32,
    track_y: f32,
    track_w: f32,
    track_h: f32,
    thumb_y: f32,
    thumb_h: f32,
    history_size: i32,
}

/// Compute the scrollbar geometry for the given canvas bounds and current
/// grid + scroll state. Returns `None` when there's no history to scroll.
fn scrollbar_geom(
    bounds: Rectangle,
    total_lines: usize,
    screen_lines: usize,
    scroll_offset: i32,
) -> Option<ScrollbarGeom> {
    let history_size = (total_lines.saturating_sub(screen_lines)) as i32;
    if history_size <= 0 {
        return None;
    }
    let track_x = bounds.width - 8.0;
    let track_w = 6.0;
    let track_y = TERM_PAD_TOP;
    let track_h = (bounds.height - TERM_PAD_TOP - TERM_PAD).max(0.0);
    let total = total_lines as f32;
    let visible = screen_lines as f32;
    let thumb_h = (track_h * (visible / total)).max(24.0).min(track_h);
    let progress = scroll_offset as f32 / history_size as f32;
    let thumb_y = track_y + (track_h - thumb_h) * (1.0 - progress);
    Some(ScrollbarGeom {
        track_x,
        track_y,
        track_w,
        track_h,
        thumb_y,
        thumb_h,
        history_size,
    })
}

/// Process-wide font-name interner. `iced::Font::new` needs a
/// `&'static str`, so each unique family name is leaked exactly once
/// and the cached reference is handed back on every later call. The
/// previous approach leaked a fresh copy per view pass per pane, which
/// added up over a long session.
fn intern_font_name(name: &str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static FONT_NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut map = FONT_NAMES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(interned) = map.get(name) {
        return interned;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    map.insert(name.to_string(), leaked);
    leaked
}

impl<Message> TerminalView<Message> {
    pub fn new(state: Arc<Mutex<TerminalState>>) -> Self {
        let font_size = 14.0;
        Self {
            state,
            font_size,
            cell_width: font_size * 0.6,
            cell_height: font_size * 1.15,
            font: Font::MONOSPACE,
            copy_on_select: true,
            right_click_copy: false,
            bold_is_bright: true,
            keyword_highlight: true,
            smart_contrast: true,
            word_delimiters: crate::backend::DEFAULT_WORD_DELIMITERS.to_string(),
            on_font_size_increase: None,
            on_font_size_decrease: None,
            on_paste_request: None,
            on_terminal_input: None,
            link_hint_text: None,
            on_link_opened: None,
            focused: true,
        }
    }

    /// Mark whether this pane is focused. Only the focused pane emits
    /// mouse-tracking reports (see the `focused` field).
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self.cell_width = size * 0.6;
        self.cell_height = size * 1.15;
        self
    }

    pub fn with_copy_on_select(mut self, on: bool) -> Self {
        self.copy_on_select = on;
        self
    }

    /// When on (and `copy_on_select` is also on), the selection waits for a
    /// right-click to copy instead of copying on release. No-op while
    /// `copy_on_select` is off.
    pub fn with_right_click_copy(mut self, on: bool) -> Self {
        self.right_click_copy = on;
        self
    }

    pub fn with_bold_is_bright(mut self, on: bool) -> Self {
        self.bold_is_bright = on;
        self
    }

    pub fn with_smart_contrast(mut self, on: bool) -> Self {
        self.smart_contrast = on;
        self
    }

    pub fn with_keyword_highlight(mut self, on: bool) -> Self {
        self.keyword_highlight = on;
        self
    }

    /// Set the word-delimiter set used for double-click word selection.
    /// Empty means no character terminates a word (double-click then
    /// grabs the whole logical line, like triple-click).
    pub fn with_word_delimiters(mut self, delimiters: &str) -> Self {
        self.word_delimiters = delimiters.to_string();
        self
    }

    /// Wire a message that fires when the user does Ctrl+Wheel-up over
    /// the terminal canvas.
    pub fn on_font_size_increase(mut self, msg: Message) -> Self {
        self.on_font_size_increase = Some(msg);
        self
    }

    /// Wire a message that fires when the user does Ctrl+Wheel-down over
    /// the terminal canvas.
    pub fn on_font_size_decrease(mut self, msg: Message) -> Self {
        self.on_font_size_decrease = Some(msg);
        self
    }

    /// Wire a message that fires on right-click over the terminal. The
    /// app dispatcher should read the clipboard and write the text to
    /// the active SSH session (or local PTY as fallback), the same path
    /// Ctrl+Shift+V takes. Without this hook, the widget falls back to
    /// writing the clipboard text directly to the local PTY, which only
    /// works for local-shell tabs.
    /// Localized hover-hint text for ctrl-clickable URLs; `None` hides
    /// the hint (one-time onboarding, see `on_link_opened`).
    pub fn with_link_hint(mut self, hint: Option<String>) -> Self {
        self.link_hint_text = hint;
        self
    }

    /// Message emitted after a Ctrl+Click opens a URL.
    pub fn on_link_opened(mut self, msg: Message) -> Self {
        self.on_link_opened = Some(msg);
        self
    }

    pub fn on_paste_request(mut self, msg: Message) -> Self {
        self.on_paste_request = Some(msg);
        self
    }

    /// Wire a callback for synthesized input bytes (mouse-tracking
    /// reports and wheel-to-arrow translation). The dispatcher should
    /// route the bytes to the active SSH session, falling back to the
    /// local PTY, exactly like the keyboard / paste paths. Without this
    /// hook the widget writes to the local PTY directly, which is a
    /// no-op on SSH tabs (their `TerminalState` has no PTY).
    pub fn on_terminal_input(
        mut self,
        f: impl Fn(Vec<u8>) -> Message + 'static,
    ) -> Self {
        self.on_terminal_input = Some(Box::new(f));
        self
    }

    /// Override the font used for cell rendering. If the font can't be resolved
    /// by cosmic-text, it falls back to the system default monospace.
    pub fn with_font_name(mut self, name: &str) -> Self {
        self.font = Font::new(intern_font_name(name));
        self
    }

    pub fn grid_size_for(width: f32, height: f32, font_size: f32) -> (u16, u16) {
        let cell_width = font_size * 0.6;
        let cell_height = font_size * 1.15;
        let usable_w = (width - TERM_PAD * 2.0).max(cell_width);
        let usable_h = (height - TERM_PAD_TOP - TERM_PAD).max(cell_height);
        let cols = (usable_w / cell_width).floor().max(1.0) as u16;
        let rows = (usable_h / cell_height).floor().max(1.0) as u16;
        (cols, rows)
    }

    fn pixel_to_cell(&self, pos: Point) -> (u16, u16) {
        let col = ((pos.x - TERM_PAD) / self.cell_width).floor().max(0.0) as u16;
        let row = ((pos.y - TERM_PAD_TOP) / self.cell_height).floor().max(0.0) as u16;
        (col, row)
    }

    /// Convert a visible-row index to the alacritty grid-line index, given
    /// the current scroll offset. Visible row 0 is the top of the canvas.
    fn visible_row_to_line(visible_row: u16, scroll_offset: i32) -> i32 {
        visible_row as i32 - scroll_offset
    }

    /// Compute a word- or line-granularity selection around `cell` using
    /// alacritty's native semantic / line search. `cell` is `(col, line)`
    /// in grid-line coordinates (negative line = scrollback). The current
    /// delimiter set is synced into the backend first (a cheap no-op when
    /// unchanged).
    fn semantic_selection(
        &self,
        backend: &mut TerminalBackend,
        cell: (u16, i32),
        gran: SelectGranularity,
    ) -> Selection {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line, Point as TermPoint};
        backend.set_word_delimiters(&self.word_delimiters);
        let term = &backend.term;
        let grid = term.grid();
        // Clamp into the grid before building the point: the semantic /
        // line search routines index `grid[point]` up front and only
        // clamp the lower line bound, so an edge click (col >= cols or a
        // line past the last row, neither of which `pixel_to_cell`
        // clamps high) would panic.
        let line = cell.1.clamp(grid.topmost_line().0, grid.bottommost_line().0);
        let col = (cell.0 as usize).min(grid.columns().saturating_sub(1));
        let point = TermPoint::new(Line(line), Column(col));
        let (l, r) = match gran {
            SelectGranularity::Word => {
                (term.semantic_search_left(point), term.semantic_search_right(point))
            }
            SelectGranularity::Line => {
                (term.line_search_left(point), term.line_search_right(point))
            }
            SelectGranularity::Paragraph => {
                // Expand to the run of non-blank lines around the click,
                // bounded by blank rows (all spaces / NUL). Full width.
                let last_col = grid.columns().saturating_sub(1) as u16;
                let top_lim = grid.topmost_line().0;
                let bot_lim = grid.bottommost_line().0;
                let is_blank = |li: i32| {
                    let r = &grid[Line(li)];
                    (0..grid.columns()).all(|c| matches!(r[Column(c)].c, ' ' | '\0'))
                };
                let mut top = line;
                while top > top_lim && !is_blank(top - 1) {
                    top -= 1;
                }
                let mut bot = line;
                while bot < bot_lim && !is_blank(bot + 1) {
                    bot += 1;
                }
                return Selection {
                    start: (0, top),
                    end: (last_col, bot),
                    block: false,
                };
            }
        };
        Selection {
            start: (l.column.0 as u16, l.line.0),
            end: (r.column.0 as u16, r.line.0),
            block: false,
        }
    }

    /// Map an iced mouse button to its mouse-report button, or `None`
    /// for buttons the xterm protocol doesn't encode (Back / Forward /
    /// Other).
    fn iced_to_report_button(btn: mouse::Button) -> Option<ReportButton> {
        match btn {
            mouse::Button::Left => Some(ReportButton::Left),
            mouse::Button::Middle => Some(ReportButton::Middle),
            mouse::Button::Right => Some(ReportButton::Right),
            _ => None,
        }
    }

    /// Send synthesized input bytes (mouse reports, wheel-to-arrow) to the
    /// dispatcher so they reach the active SSH session. Falls back to a
    /// direct local-PTY write when no callback is wired (local-shell
    /// tabs). Always captures the originating event.
    fn emit_input(&self, bytes: Vec<u8>) -> CanvasAction<Message> {
        if let Some(cb) = &self.on_terminal_input {
            CanvasAction::publish(cb(bytes)).and_capture()
        } else {
            if let Ok(mut state) = self.state.lock() {
                state.write(&bytes);
            }
            CanvasAction::capture()
        }
    }

    /// Translate a pointer event into a mouse-tracking report for the
    /// remote app. Returns `Some(action)` when the event was consumed,
    /// `None` to let the normal local handlers run. The caller guarantees
    /// the app has mouse tracking on and Shift isn't held.
    #[allow(clippy::too_many_arguments)]
    fn handle_mouse_report(
        &self,
        widget_state: &mut TerminalWidgetState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        mode: alacritty_terminal::term::TermMode,
        grid_cols: u16,
        grid_rows: u16,
    ) -> Option<CanvasAction<Message>> {
        use alacritty_terminal::term::TermMode;
        let kbd = widget_state.modifiers;
        let ctrl = kbd.control();
        // Shift is the local-selection bypass, so the caller only reaches
        // here with it released; never fold it into the report.
        let mods = ReportMods { shift: false, alt: kbd.alt(), ctrl };

        // Resolve a pixel position to a clamped, zero-based cell.
        let cell = |pos: Point| -> (u16, u16) {
            let (c, r) = self.pixel_to_cell(pos);
            (
                c.min(grid_cols.saturating_sub(1)),
                r.min(grid_rows.saturating_sub(1)),
            )
        };

        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(btn)) => {
                let pos = cursor.position_in(bounds)?;
                let rb = Self::iced_to_report_button(*btn)?;
                let (col, row) = cell(pos);
                widget_state.report_button = Some(rb);
                widget_state.report_cell = Some((col, row));
                let bytes =
                    mouse_report::encode(mode, MouseEventKind::Press, rb, col, row, mods)?;
                Some(self.emit_input(bytes))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(btn)) => {
                let rb = Self::iced_to_report_button(*btn)?;
                // A drag can end with the pointer off the canvas; fall back
                // to the last reported cell so the release still lands.
                let (col, row) = match cursor.position_in(bounds) {
                    Some(pos) => cell(pos),
                    None => widget_state.report_cell.unwrap_or((0, 0)),
                };
                widget_state.report_button = None;
                let bytes =
                    mouse_report::encode(mode, MouseEventKind::Release, rb, col, row, mods)?;
                Some(self.emit_input(bytes))
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let pos = cursor.position_in(bounds)?;
                let (col, row) = cell(pos);
                // Suppress repeats while the cursor stays inside one cell.
                if widget_state.report_cell == Some((col, row)) {
                    return None;
                }
                // Drag tracking (1002) reports motion only while a button is
                // held; any-motion tracking (1003) reports bare motion via
                // the "no button" sentinel.
                let btn = match widget_state.report_button {
                    Some(b) => b,
                    None if mode.contains(TermMode::MOUSE_MOTION) => ReportButton::None,
                    None => return None,
                };
                let bytes =
                    mouse_report::encode(mode, MouseEventKind::Motion, btn, col, row, mods)?;
                widget_state.report_cell = Some((col, row));
                Some(self.emit_input(bytes))
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                // Ctrl+wheel stays a local font-zoom affordance; let it
                // reach the dedicated handler instead of reporting it.
                if ctrl {
                    return None;
                }
                let pos = cursor.position_in(bounds)?;
                let (col, row) = cell(pos);
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / self.cell_height,
                };
                if dy == 0.0 {
                    return None;
                }
                let btn = if dy > 0.0 {
                    ReportButton::WheelUp
                } else {
                    ReportButton::WheelDown
                };
                // One report per notch, capped so a fast flick can't flood
                // the session, concatenated into a single write.
                let notches = (dy.abs().ceil() as u32).clamp(1, 5);
                let mut bytes = Vec::new();
                for _ in 0..notches {
                    if let Some(seq) =
                        mouse_report::encode(mode, MouseEventKind::Press, btn, col, row, mods)
                    {
                        bytes.extend_from_slice(&seq);
                    }
                }
                if bytes.is_empty() {
                    return None;
                }
                Some(self.emit_input(bytes))
            }
            _ => None,
        }
    }

    fn is_in_selection(sel: &Selection, col: u16, line: i32) -> bool {
        if sel.block {
            let (c0, c1, l0, l1) = sel.block_bounds();
            return line >= l0 && line <= l1 && col >= c0 && col <= c1;
        }
        let (start, end) = sel.ordered();
        if start.1 == end.1 {
            line == start.1 && col >= start.0 && col <= end.0
        } else if line == start.1 {
            col >= start.0
        } else if line == end.1 {
            col <= end.0
        } else {
            line > start.1 && line < end.1
        }
    }
}

/// Per-cell snapshot taken in `draw()` while the state mutex is held.
/// Pass 2 renders from these without touching the mutex, so geometry
/// building never contends with `process()` on the output path.
struct CellData {
    col: u16,
    row: u16,
    c: char,
    fg: Color,
    bg: Color,
    flags: CellFlags,
}

thread_local! {
    /// Reusable cell-snapshot buffer for `draw()` (which always runs on
    /// the renderer thread). Taken out for the duration of a frame and
    /// put back afterwards so its capacity survives across frames and
    /// panes instead of reallocating per draw.
    static DRAW_CELLS: std::cell::RefCell<Vec<CellData>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl<Message> canvas::Program<Message, Theme> for TerminalView<Message>
where
    Message: Clone,
{
    type State = TerminalWidgetState;

    fn update(
        &self,
        widget_state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<CanvasAction<Message>> {
        // Refresh hover state for every event we see, drives the
        // scrollbar's reveal-on-hover behaviour. Done before the match so
        // we don't have to repeat it in every arm.
        let new_hover = cursor.position_in(bounds).is_some();
        let hover_changed = widget_state.hover != new_hover;
        widget_state.hover = new_hover;

        // When the remote app has mouse tracking on (tmux `mouse on`,
        // vim `mouse=a`, htop, ...) pointer events are reported to it
        // instead of driving local selection / scrollback. We snapshot
        // the relevant `TermMode` + grid size once per mouse event (the
        // lock is a cheap flag read; skipped for keyboard events so the
        // typing path never contends on it). Holding Shift bypasses
        // reporting and restores local selection, the universal escape
        // hatch every terminal honours.
        // Only the focused pane reports mouse events to its app. Otherwise
        // a click that just focuses an inactive split pane (one still in
        // mouse mode, e.g. running htop) would inject a stray SGR report
        // like `\x1b[<0;1;1m` into that shell.
        let report_ctx = if self.focused && matches!(event, iced::Event::Mouse(_)) {
            self.state.lock().ok().and_then(|s| {
                let mode = *s.backend.term.mode();
                mode.intersects(alacritty_terminal::term::TermMode::MOUSE_MODE)
                    .then(|| (mode, s.cols(), s.rows()))
            })
        } else {
            None
        };
        if let Some((mode, grid_cols, grid_rows)) = report_ctx
            && !widget_state.modifiers.shift()
            && let Some(action) =
                self.handle_mouse_report(widget_state, event, bounds, cursor, mode, grid_cols, grid_rows)
        {
            return Some(action);
        }

        match event {
            // Mouse press, scrollbar interaction takes priority, then
            // URL open, then text selection.
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    // Scrollbar: thumb drag start, or page-up/down on the
                    // empty track area. Only meaningful when there's
                    // actual scrollback.
                    if let Ok(state) = self.state.lock() {
                        let grid = state.backend.term.grid();
                        if let Some(sb) = scrollbar_geom(
                            bounds,
                            grid.total_lines(),
                            grid.screen_lines(),
                            widget_state.scroll_offset,
                        ) && pos.x >= sb.track_x - 2.0
                            && pos.x <= sb.track_x + sb.track_w + 2.0
                            && pos.y >= sb.track_y
                            && pos.y <= sb.track_y + sb.track_h
                        {
                            let page = grid.screen_lines() as i32;
                            if pos.y >= sb.thumb_y && pos.y <= sb.thumb_y + sb.thumb_h {
                                widget_state.scrollbar_drag =
                                    Some((pos.y, widget_state.scroll_offset));
                            } else if pos.y < sb.thumb_y {
                                widget_state.scroll_offset =
                                    (widget_state.scroll_offset + page).min(sb.history_size);
                            } else {
                                widget_state.scroll_offset =
                                    (widget_state.scroll_offset - page).max(0);
                            }
                            return Some(CanvasAction::request_redraw().and_capture());
                        }
                    }
                    let (col, vrow) = self.pixel_to_cell(pos);
                    let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset);
                    // Only follow URLs on Ctrl+Click, plain clicks
                    // start a selection, matching Termius. Without
                    // the modifier gate, every click on a logged URL
                    // would lose the selection start.
                    if widget_state.modifiers.control()
                        && let Ok(state) = self.state.lock()
                        && let Some(url) = url_at_cell(&state.backend.term, vrow, col)
                    {
                        drop(state);
                        open_url(&url);
                        // Tell the app the gesture landed so the
                        // one-time hover hint can retire itself.
                        if let Some(msg) = self.on_link_opened.clone() {
                            return Some(CanvasAction::publish(msg).and_capture());
                        }
                        return Some(CanvasAction::capture());
                    }
                    // Shift+Click extends the current selection from its
                    // existing anchor instead of starting a new one (xterm
                    // behaviour). Handled before click-kind classification so
                    // a quick shift+click can't be misread as a double-click
                    // word grab. Block-ness carries over.
                    if widget_state.modifiers.shift()
                        && let Some(prev) = widget_state.selection
                    {
                        widget_state.select_anchor = None;
                        widget_state.selecting = true;
                        widget_state.last_extend_cell = Some((col, line));
                        widget_state.selection = Some(Selection {
                            start: prev.start,
                            end: (col, line),
                            block: prev.block,
                        });
                        return Some(CanvasAction::request_redraw().and_capture());
                    }
                    // Classify the press as single / double / triple / quad
                    // (300 ms / 6 px window). 1=cell (Alt=block), 2=word
                    // (smart-select on URL/IP/path), 3=line, 4=paragraph.
                    let now = std::time::Instant::now();
                    let consecutive = widget_state
                        .last_click
                        .map(|(t, p, _)| {
                            now.duration_since(t) <= std::time::Duration::from_millis(300)
                                && p.distance(pos) < 6.0
                        })
                        .unwrap_or(false);
                    let count = next_click_count(
                        widget_state.last_click.map(|(_, _, c)| c),
                        consecutive,
                    );
                    widget_state.last_click = Some((now, pos, count));
                    widget_state.selecting = true;
                    widget_state.last_extend_cell = Some((col, line));
                    match count {
                        1 => {
                            widget_state.select_anchor = None;
                            // Alt+drag starts a rectangular (column) selection.
                            widget_state.selection = Some(Selection {
                                start: (col, line),
                                end: (col, line),
                                block: widget_state.modifiers.alt(),
                            });
                        }
                        2 => {
                            if let Ok(mut state) = self.state.lock() {
                                // Smart-select: a double-click inside a URL /
                                // IP / path grabs the whole token instead of
                                // the delimiter word. Falls back to word.
                                if let Some((c0, c1)) = smart_span_at(
                                    &state.backend.term,
                                    &state.palette,
                                    line,
                                    col,
                                ) {
                                    widget_state.select_anchor = None;
                                    widget_state.selection = Some(Selection {
                                        start: (c0, line),
                                        end: (c1, line),
                                        block: false,
                                    });
                                } else {
                                    widget_state.select_anchor =
                                        Some((SelectGranularity::Word, (col, line)));
                                    widget_state.selection = Some(self.semantic_selection(
                                        &mut state.backend,
                                        (col, line),
                                        SelectGranularity::Word,
                                    ));
                                }
                            }
                        }
                        3 => {
                            widget_state.select_anchor =
                                Some((SelectGranularity::Line, (col, line)));
                            if let Ok(mut state) = self.state.lock() {
                                widget_state.selection = Some(self.semantic_selection(
                                    &mut state.backend,
                                    (col, line),
                                    SelectGranularity::Line,
                                ));
                            }
                        }
                        // 4 (and the cycle restarts after): paragraph.
                        _ => {
                            widget_state.select_anchor =
                                Some((SelectGranularity::Paragraph, (col, line)));
                            if let Ok(mut state) = self.state.lock() {
                                widget_state.selection = Some(self.semantic_selection(
                                    &mut state.backend,
                                    (col, line),
                                    SelectGranularity::Paragraph,
                                ));
                            }
                        }
                    }
                    return Some(CanvasAction::request_redraw().and_capture());
                }
            }
            // Mouse move, drag scrollbar thumb or extend selection.
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some((start_y, start_offset)) = widget_state.scrollbar_drag
                    && let Some(pos) = cursor.position_in(bounds)
                    && let Ok(state) = self.state.lock()
                {
                    let grid = state.backend.term.grid();
                    if let Some(sb) = scrollbar_geom(
                        bounds,
                        grid.total_lines(),
                        grid.screen_lines(),
                        start_offset,
                    ) {
                        let dy = pos.y - start_y;
                        let track_range = (sb.track_h - sb.thumb_h).max(1.0);
                        let dprogress = dy / track_range;
                        let doffset = (dprogress * sb.history_size as f32) as i32;
                        // Thumb moves down → progress decreases → offset decreases.
                        widget_state.scroll_offset =
                            (start_offset - doffset).clamp(0, sb.history_size);
                        return Some(CanvasAction::request_redraw().and_capture());
                    }
                }
                if widget_state.selecting
                    && let Some(abs) = cursor.position() {
                        // Use the absolute cursor position (not
                        // `position_in`, which is `None` outside the widget)
                        // so a drag that leaves the widget but stays in the
                        // window still extends + auto-scrolls, matching other
                        // terminals. Once the pointer leaves the window the OS
                        // stops sending events, which we can't work around
                        // without a pointer grab iced doesn't expose.
                        let rel = Point::new(abs.x - bounds.x, abs.y - bounds.y);
                        // Auto-scroll when the drag passes the top/bottom
                        // edge so the selection extends into scrollback. The
                        // step grows with how far past the edge the cursor is
                        // (deliberately aggressive: 2 lines per overshoot
                        // cell). Events only fire on motion, so this follows
                        // the mouse rather than ticking while held still.
                        let top_edge = TERM_PAD_TOP;
                        let bot_edge = (bounds.height - TERM_PAD).max(top_edge);
                        // Rate-limit to one step per ~40 ms so the scroll
                        // speed tracks wall-clock instead of the mouse-move
                        // event rate (dozens per second at the edge), which
                        // is what made it feel like it rocketed.
                        let now = std::time::Instant::now();
                        let due = widget_state
                            .last_autoscroll
                            .map(|t| {
                                now.duration_since(t)
                                    >= std::time::Duration::from_millis(40)
                            })
                            .unwrap_or(true);
                        if (rel.y < top_edge || rel.y > bot_edge)
                            && due
                            && let Ok(state) = self.state.lock()
                        {
                            use alacritty_terminal::grid::Dimensions;
                            let grid = state.backend.term.grid();
                            let history = (grid
                                .total_lines()
                                .saturating_sub(grid.screen_lines()))
                                as i32;
                            let past = if rel.y < top_edge {
                                top_edge - rel.y
                            } else {
                                rel.y - bot_edge
                            };
                            // 1 line per tick at the edge, +1 per cell of
                            // overshoot, capped so a far pointer stays sane.
                            let step =
                                ((past / self.cell_height).floor() as i32 + 1).clamp(1, 4);
                            widget_state.last_autoscroll = Some(now);
                            if rel.y < top_edge {
                                widget_state.scroll_offset =
                                    (widget_state.scroll_offset + step).min(history);
                            } else {
                                widget_state.scroll_offset =
                                    (widget_state.scroll_offset - step).max(0);
                            }
                        }
                        // Clamp back into the widget for cell mapping (the
                        // pointer may be outside the bounds now).
                        let clamped = Point::new(
                            rel.x.clamp(0.0, bounds.width),
                            rel.y.clamp(0.0, bounds.height),
                        );
                        let (col, vrow) = self.pixel_to_cell(clamped);
                        let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset);
                        if let Some((gran, anchor)) = widget_state.select_anchor {
                            // Word/line drag: extend by unioning the anchor's
                            // word/line with the cursor's. Throttle to one
                            // recompute per cell crossing, it locks the mutex
                            // and runs two semantic searches, which must not
                            // happen per pixel (same reasoning as the URL
                            // hover throttle below).
                            if widget_state.last_extend_cell != Some((col, line)) {
                                widget_state.last_extend_cell = Some((col, line));
                                if let Ok(mut state) = self.state.lock() {
                                    let head = self.semantic_selection(
                                        &mut state.backend, anchor, gran,
                                    );
                                    let tail = self.semantic_selection(
                                        &mut state.backend, (col, line), gran,
                                    );
                                    drop(state);
                                    widget_state.selection =
                                        Some(union_selection(head, tail));
                                }
                            }
                        } else if let Some(ref mut sel) = widget_state.selection {
                            sel.end = (col, line);
                        }
                        return Some(CanvasAction::request_redraw().and_capture());
                    }
                // URL hover detection. Skip the lock + grid scan when
                // the cursor is still over the same cell, at typical
                // font sizes a single cell spans many pixels and
                // running the scan on every pixel contended with
                // `state.process` (the SSH echo path), showing up as
                // typing lag.
                let new_hover_url = if let Some(pos) = cursor.position_in(bounds) {
                    let (col, vrow) = self.pixel_to_cell(pos);
                    let same_cell = widget_state.hovered_cell == Some((col, vrow));
                    widget_state.hovered_cell = Some((col, vrow));
                    if same_cell {
                        widget_state
                            .hovered_url
                            .as_ref()
                            .map(|(u, _)| (u.clone(), pos))
                    } else if let Ok(state) = self.state.lock() {
                        url_at_cell(&state.backend.term, vrow, col).map(|u| (u, pos))
                    } else {
                        None
                    }
                } else {
                    widget_state.hovered_cell = None;
                    None
                };
                let url_changed = match (&widget_state.hovered_url, &new_hover_url) {
                    (Some((a, _)), Some((b, _))) => a != b,
                    (None, None) => false,
                    _ => true,
                };
                widget_state.hovered_url = new_hover_url;
                if hover_changed || url_changed {
                    return Some(CanvasAction::request_redraw());
                }
            }
            // Mouse release, end selection or scrollbar drag.
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let was_dragging = widget_state.scrollbar_drag.is_some();
                widget_state.scrollbar_drag = None;
                let was_selecting = widget_state.selecting;
                // A double/triple-click selection is intentional even when
                // it lands on a single cell (a one-character word), so it
                // must still auto-copy despite `is_empty()`.
                let was_semantic = widget_state.select_anchor.is_some();
                widget_state.selecting = false;
                widget_state.select_anchor = None;
                widget_state.last_extend_cell = None;
                // Auto-copy the just-finished selection when the setting is
                // enabled (XTerm / iTerm behaviour). Skip degenerate
                // selections that didn't move (single click). When
                // `right_click_copy` is on the copy is deferred to a
                // right-click instead, so skip the auto-copy here.
                if was_selecting
                    && self.copy_on_select
                    && !self.right_click_copy
                    && let Some(ref sel) = widget_state.selection
                    && (!sel.is_empty() || was_semantic)
                    && let Ok(state) = self.state.lock()
                {
                    let text = state.get_selection_text(sel);
                    drop(state);
                    if !text.is_empty() {
                        set_clipboard_text(&text);
                    }
                }
                if was_dragging {
                    return Some(CanvasAction::request_redraw().and_capture());
                }
                return Some(CanvasAction::capture());
            }
            // Right-click, paste from clipboard. When the host wired an
            // `on_paste_request` callback we delegate the actual paste to
            // the app dispatcher so it can target the SSH session (the
            // local-PTY write below only reaches local-shell tabs). The
            // fallback covers callers that don't set the hook.
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                if cursor.position_in(bounds).is_some() =>
            {
                // State 3 (copy_on_select + right_click_copy): a right-click
                // over a live selection copies it instead of pasting, then
                // clears the selection so the next right-click pastes. With
                // no selection we fall through to the normal paste path. The
                // copy is written straight to the clipboard here (mirroring
                // Ctrl+Shift+C), not via `on_paste_request`, which is the
                // paste hook.
                if self.copy_on_select
                    && self.right_click_copy
                    && let Some(sel) = widget_state.selection
                    && !sel.is_empty()
                {
                    if let Ok(state) = self.state.lock() {
                        let text = state.get_selection_text(&sel);
                        drop(state);
                        if !text.is_empty() {
                            set_clipboard_text(&text);
                        }
                    }
                    widget_state.selection = None;
                    return Some(CanvasAction::request_redraw().and_capture());
                }
                if let Some(msg) = self.on_paste_request.clone() {
                    return Some(CanvasAction::publish(msg).and_capture());
                }
                if let Ok(mut clip) = arboard::Clipboard::new()
                    && let Ok(text) = clip.get_text()
                    && let Ok(mut state) = self.state.lock()
                {
                    let bracketed = state.bracketed_paste_enabled();
                    state.write(&crate::wrap_paste(&text, bracketed));
                }
                return Some(CanvasAction::capture());
            }
            // Ctrl + wheel, adjust terminal font size in the standard
            // alacritty / kitty / gnome-terminal way. Captured before the
            // scrollback handler so it doesn't double-up with paging.
            // The TUI inside the session never sees the wheel event in
            // this branch, so htop / less / vim mouse modes aren't
            // disturbed.
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.position_in(bounds).is_some()
                    && widget_state.modifiers.control() =>
            {
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if dy > 0.0
                    && let Some(msg) = self.on_font_size_increase.clone()
                {
                    return Some(CanvasAction::publish(msg).and_capture());
                }
                if dy < 0.0
                    && let Some(msg) = self.on_font_size_decrease.clone()
                {
                    return Some(CanvasAction::publish(msg).and_capture());
                }
                return Some(CanvasAction::capture());
            }
            // Mouse wheel, scrollback in the OS-natural direction:
            // wheel up shows older content (scroll_offset increases),
            // wheel down returns to the live edge (scroll_offset → 0).
            // Only consume when the cursor is actually over the terminal
            // canvas, otherwise the wheel bleeds into the AI sidebar.
            //
            // When the remote app has switched to the alternate screen
            // (top, vim, less, htop, …) we forward the wheel as cursor
            // arrows so paging works inside those apps, instead of
            // adding to our scrollback buffer (which is empty in alt
            // screen mode anyway).
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.position_in(bounds).is_some() =>
            {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y as i32 * 3,
                    mouse::ScrollDelta::Pixels { y, .. } => (*y / self.cell_height) as i32,
                };
                // One lock for both the alt-screen test and the scroll
                // clamp, this handler fires for every wheel tick and
                // locking twice doubled the contention with `process()`.
                let (in_alt_screen, max_scroll) = match self.state.lock() {
                    Ok(s) => {
                        let in_alt = s
                            .backend
                            .term
                            .mode()
                            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);
                        let grid = s.backend.term.grid();
                        (in_alt, (grid.total_lines() - grid.screen_lines()) as i32)
                    }
                    Err(_) => (false, i32::MAX),
                };
                if in_alt_screen {
                    // Translate wheel into arrow-key bytes for the remote
                    // app, `top`/`vim`/`less` all listen for these. Routed
                    // through `emit_input` so it reaches the SSH session,
                    // a direct `state.write` only hits the local PTY and is
                    // a no-op on SSH tabs (this used to silently do nothing
                    // when scrolling vim / less over SSH).
                    let arrow: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    let count = lines.unsigned_abs().min(10) as usize;
                    let mut bytes = Vec::with_capacity(arrow.len() * count);
                    for _ in 0..count {
                        bytes.extend_from_slice(arrow);
                    }
                    return Some(self.emit_input(bytes));
                }
                widget_state.scroll_offset =
                    (widget_state.scroll_offset + lines).max(0).min(max_scroll);
                return Some(CanvasAction::request_redraw().and_capture());
            }
            // Modifier tracking for the URL Ctrl+Click gate. iced
            // doesn't pass the current modifier mask on mouse events,
            // so we mirror it from the dedicated change event.
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                let was_ctrl = widget_state.modifiers.control();
                widget_state.modifiers = *m;
                let now_ctrl = m.control();
                // Re-render to flip the cursor icon / tooltip text
                // immediately when Ctrl is pressed/released over a URL.
                if widget_state.hovered_url.is_some() && was_ctrl != now_ctrl {
                    return Some(CanvasAction::request_redraw());
                }
            }
            // Keyboard, Ctrl+Shift+C copy (paste is handled in app.rs so it can
            // reach the SSH session; widget.state.write only targets a local PTY).
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                modifiers,
                ..
            }) if modifiers.control() && modifiers.shift() && matches!(c.as_str(), "C" | "c") => {
                if let Some(ref sel) = widget_state.selection
                    && !sel.is_empty()
                    && let Ok(state) = self.state.lock()
                {
                    let text = state.get_selection_text(sel);
                    if !text.is_empty() {
                        set_clipboard_text(&text);
                    }
                }
                return Some(CanvasAction::capture());
            }
            // Keyboard, Ctrl+Shift+A select-all. Selects the entire buffer
            // (scrollback + screen); copy stays a separate gesture
            // (Ctrl+Shift+C or copy-on-select on the next release).
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                modifiers,
                ..
            }) if modifiers.control() && modifiers.shift() && matches!(c.as_str(), "A" | "a") => {
                if let Ok(state) = self.state.lock() {
                    use alacritty_terminal::grid::Dimensions;
                    let grid = state.backend.term.grid();
                    let top = grid.topmost_line().0;
                    let bot = grid.bottommost_line().0;
                    let last_col = grid.columns().saturating_sub(1) as u16;
                    widget_state.selection = Some(Selection {
                        start: (0, top),
                        end: (last_col, bot),
                        block: false,
                    });
                    widget_state.select_anchor = None;
                }
                return Some(CanvasAction::request_redraw().and_capture());
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !cursor.is_over(bounds) {
            return mouse::Interaction::default();
        }
        // Pointer cursor over a URL, same as the browser hover affordance
        // and clear visual cue that "click does something different here".
        // Only when Ctrl is held does the click actually open the link.
        if state.hovered_url.is_some() {
            return mouse::Interaction::Pointer;
        }
        mouse::Interaction::Text
    }

    fn draw(
        &self,
        widget_state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

        let perf_on = perf_overlay_enabled();
        let draw_start = perf_on.then(std::time::Instant::now);

        let cell_w = self.cell_width;
        let cell_h = self.cell_height;
        let selection = &widget_state.selection;

        let mut cells: Vec<CellData> = DRAW_CELLS.take();
        cells.clear();
        let mut row_chars: Vec<(u16, Vec<(u16, char)>)> = Vec::new();

        // --- Snapshot phase, the only part that holds the state mutex ---
        // Everything draw needs (resolved cells, cursor, sizes, palette)
        // is copied out here and the lock is dropped before any text /
        // quad geometry is built, so drawing doesn't contend with
        // `process()` on the output path (see the typing-lag note on
        // `hovered_cell`).
        let lock_start = perf_on.then(std::time::Instant::now);
        let (
            lock_dur,
            cells_dur,
            palette,
            term_cursor,
            screen_lines,
            total_lines,
            in_alt_screen,
            scroll_offset,
        ) = {
            let mut state = match self.state.lock() {
                Ok(s) => s,
                Err(poisoned) => poisoned.into_inner(),
            };
            let lock_dur = lock_start.map(|t| t.elapsed()).unwrap_or_default();

            // Auto-resize
            let (new_cols, new_rows) =
                Self::grid_size_for(bounds.width, bounds.height, self.font_size);
            state.resize(new_cols, new_rows);

            // Alt-screen apps (top, vim, less, htop, …) own the entire
            // viewport with cursor positioning, there's no scrollback to
            // page through. Force scroll_offset=0 so the user can't get
            // stuck looking at stale history while the app keeps redrawing.
            let in_alt_screen = state
                .backend
                .term
                .mode()
                .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);

            // Clamp scroll offset against the current grid bounds, resizes
            // between frames can shrink history, so the offset stored in
            // widget_state may exceed the new max.
            let scroll_offset = if in_alt_screen {
                0
            } else {
                let grid = state.backend.term.grid();
                let max_scroll = (grid.total_lines() - grid.screen_lines()) as i32;
                widget_state.scroll_offset.clamp(0, max_scroll)
            };

            let term = &state.backend.term;
            let palette = &state.palette;
            let colors = term.colors();

            let term_cursor = term.renderable_content().cursor;
            let grid = term.grid();
            let screen_lines = grid.screen_lines();
            let cols_count = grid.columns();
            let total_lines = grid.total_lines();
            let topmost = grid.topmost_line();
            let bottommost = grid.bottommost_line();

            // --- Pass 1: collect cell data and build row character map ---
            // Iterate the grid manually using `scroll_offset` as a row offset
            // instead of mutating alacritty's `display_offset` via
            // `scroll_display`. The previous approach yielded `display_iter`
            // entries with negative `point.line.0` for scrollback rows, which
            // when cast to `u16` wrapped to enormous numbers, those cells
            // ended up rendered far off-screen, leaving blank rows in their
            // place. Manual indexing keeps the math sane.
            let cells_start = perf_on.then(std::time::Instant::now);
            cells.reserve(screen_lines * cols_count);
            row_chars.reserve(screen_lines);

            // Flags that keep an otherwise blank default cell visible:
            // INVERSE swaps the background in, underlines / strikeout
            // paint rules over it.
            let blank_visible_flags =
                CellFlags::INVERSE | CellFlags::ALL_UNDERLINES | CellFlags::STRIKEOUT;

            for visible_row in 0..screen_lines {
                let line =
                    alacritty_terminal::index::Line(visible_row as i32 - scroll_offset);
                if line < topmost || line > bottommost {
                    continue;
                }
                let row_data = &grid[line];
                let mut chars: Vec<(u16, char)> = Vec::new();
                for col_i in 0..cols_count {
                    let cell = &row_data[alacritty_terminal::index::Column(col_i)];

                    if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                        continue;
                    }

                    let col = col_i as u16;
                    let row = visible_row as u16;
                    let c = cell.c;

                    // Skip cells that produce zero geometry: a blank glyph
                    // on the default background with no visible flags and
                    // no selection overlap. On a mostly empty screen this
                    // is the vast majority of the grid. (The cursor is
                    // painted independently of the cell snapshot, so a
                    // blank cell under it can be skipped too.)
                    if (c == ' ' || c == '\0')
                        && cell.bg == AnsiColor::Named(NamedColor::Background)
                        && !cell.flags.intersects(blank_visible_flags)
                        && !selection
                            .as_ref()
                            .is_some_and(|s| Self::is_in_selection(s, col, line.0))
                    {
                        continue;
                    }

                    let effective_fg =
                        if cell.flags.contains(CellFlags::BOLD) && self.bold_is_bright {
                            brighten_named(&cell.fg)
                        } else {
                            cell.fg
                        };
                    let fg = palette.resolve(&effective_fg, colors);
                    let bg = palette.resolve(&cell.bg, colors);

                    if c != ' ' && c != '\0' {
                        chars.push((col, c));
                    }

                    cells.push(CellData {
                        col,
                        row,
                        c,
                        fg,
                        bg,
                        flags: cell.flags,
                    });
                }
                if !chars.is_empty() {
                    row_chars.push((visible_row as u16, chars));
                }
            }

            let cells_dur = cells_start.map(|t| t.elapsed()).unwrap_or_default();

            (
                lock_dur,
                cells_dur,
                state.palette.clone(),
                term_cursor,
                screen_lines,
                total_lines,
                in_alt_screen,
                scroll_offset,
            )
        };
        let palette = &palette;

        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), palette.background);

        // --- Detect syntax highlights ---
        let highlights_start = perf_on.then(std::time::Instant::now);
        let highlights = if self.keyword_highlight {
            detect_highlights(&row_chars, palette)
        } else {
            Vec::new()
        };
        let highlights_dur = highlights_start.map(|t| t.elapsed()).unwrap_or_default();

        // Resolve which URL (if any) the cursor is over right now,
        // re-derived from the hovered cursor pixel position. We can't
        // trust the column we cached on hover because the grid may
        // have re-flowed since (resize, scroll). Drives both the
        // "underline only the hovered URL" rule and the tooltip
        // anchor below.
        let hovered_url_extent: Option<(u16, u16, u16)> = if let Some((_, pos)) =
            widget_state.hovered_url
        {
            let col = ((pos.x - TERM_PAD) / cell_w).max(0.0) as u16;
            let row = ((pos.y - TERM_PAD_TOP) / cell_h).max(0.0) as u16;
            hovered_url_range(&highlights, row, col)
        } else {
            None
        };

        // Reserve the area where the perf HUD will be drawn so cell
        // glyphs underneath don't bleed through. iced wgpu batches
        // canvas draws as `meshes → text`, so a `fill_rectangle`
        // placed *over* prior `fill_text` ends up below it visually
        //, the cleanest fix is to skip those cells in the first
        // place.
        let perf_panel = if perf_on {
            let panel_w = 240.0;
            let panel_h = 38.0;
            let panel_x = (bounds.width - panel_w - 8.0).max(0.0);
            let panel_y = 6.0;
            Some(Rectangle::new(
                Point::new(panel_x, panel_y),
                Size::new(panel_w, panel_h),
            ))
        } else {
            None
        };

        // --- Pass 2: draw cells with highlight overrides ---
        // Consecutive plain ASCII glyphs in a row that share the same
        // foreground (and the base font) are merged into one fill_text
        // run, one String + one shaping pass per run instead of per
        // glyph. This leans on the monospace advance matching the cell
        // width; runs are kept short and re-anchored to the grid so a
        // font whose advance is off by a hair can only drift
        // sub-pixel within one run. Wide chars, PUA symbols and
        // non-ASCII glyphs keep per-cell positioning because their
        // glyphs (often from a fallback font) need not advance by one
        // cell.
        struct GlyphRun {
            row: u16,
            start_col: u16,
            next_col: u16,
            fg: Color,
            content: String,
        }
        // Re-anchor at most every 32 cells; bounds intra-run drift.
        const MAX_RUN_LEN: usize = 32;
        // Bridge small gaps (skipped blank cells) with spaces so a row
        // of short tokens still coalesces into few runs.
        const MAX_RUN_GAP: u16 = 4;
        let mut run: Option<GlyphRun> = None;
        let font_size = self.font_size;
        let base_font = self.font;
        let flush_run = |frame: &mut Frame, run: GlyphRun| {
            frame.fill_text(CanvasText {
                content: run.content,
                position: Point::new(
                    run.start_col as f32 * cell_w + TERM_PAD,
                    run.row as f32 * cell_h + TERM_PAD_TOP,
                ),
                color: run.fg,
                size: Pixels(font_size),
                font: base_font,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Top,
                ..Default::default()
            });
        };
        for cd in &cells {
            let x = cd.col as f32 * cell_w + TERM_PAD;
            let y = cd.row as f32 * cell_h + TERM_PAD_TOP;

            if let Some(panel) = perf_panel
                && x + cell_w > panel.x
                && x < panel.x + panel.width
                && y + cell_h > panel.y
                && y < panel.y + panel.height
            {
                continue;
            }

            let mut fg = cd.fg;
            let mut bg = cd.bg;

            if cd.flags.contains(CellFlags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cd.flags.contains(CellFlags::DIM) {
                fg = Color::from_rgba(fg.r * 0.66, fg.g * 0.66, fg.b * 0.66, fg.a);
            }

            // Syntax highlight override (only when text has default/foreground color)
            if let Some(hl_color) = highlight_color_at(&highlights, cd.row, cd.col) {
                // Only override if the cell isn't already colored by the application
                let fg_is_default =
                    (fg.r - palette.foreground.r).abs() < 0.02
                    && (fg.g - palette.foreground.g).abs() < 0.02
                    && (fg.b - palette.foreground.b).abs() < 0.02;
                if fg_is_default {
                    fg = hl_color;
                }
            }

            // Selection highlight, convert visible row to grid-line so
            // the selection follows scrolled content instead of staying
            // glued to viewport coordinates.
            let cell_line = Self::visible_row_to_line(cd.row, scroll_offset);
            let is_selected = selection
                .as_ref()
                .map(|s| Self::is_in_selection(s, cd.col, cell_line))
                .unwrap_or(false);

            if is_selected {
                bg = Color::from_rgba(0.133, 0.60, 0.569, 0.35);
                fg = Color::WHITE;
            }

            // Smart contrast, when an app picks a colour pair that
            // renders too close to disappear (PowerShell's
            // `$PSStyle.FileInfo.Directory` blue-on-blue, LS_COLORS'
            // `ow` green-on-green over a green palette), swap the
            // foreground for white or near-black depending on the
            // background's luminance. Only kicks in when the cell
            // actually has a non-default background, preserves
            // colour-precise output everywhere else.
            if self.smart_contrast && !is_selected {
                let bg_overrides_default = (bg.r - palette.background.r).abs() >= 0.01
                    || (bg.g - palette.background.g).abs() >= 0.01
                    || (bg.b - palette.background.b).abs() >= 0.01;
                if bg_overrides_default && contrast_ratio(fg, bg) < 2.5 {
                    fg = if relative_luminance(bg) >= 0.4 {
                        Color::from_rgb(0.05, 0.06, 0.07)
                    } else {
                        Color::WHITE
                    };
                }
            }

            // Draw background
            let is_default_bg = !is_selected
                && (bg.r - palette.background.r).abs() < 0.01
                && (bg.g - palette.background.g).abs() < 0.01
                && (bg.b - palette.background.b).abs() < 0.01;

            if !is_default_bg {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y), Size::new(width, cell_h), bg);
            }

            // Draw character. Codepoints in the Unicode Private Use
            // Areas are forced through the bundled SauceCodePro Nerd
            // Font: cosmic-text's auto-fallback tends to pick CJK
            // fonts (which use the PUA for user-defined chars) before
            // our Nerd Font for the F0xx range, so prompts with
            // Powerline / Font Awesome / Devicons would render as
            // tofu or wrong-script glyphs. Forcing the symbol font
            // here is what alacritty/wezterm call a "symbol_map",
            // hard-coded to the bundled family since we ship it in
            // the binary.
            //
            // `\t` is a marker the emulator parks at the *start* of a
            // tab span (see alacritty's `put_tab` in `term/mod.rs`)
            // so clipboard copy can recover the original TAB. It's
            // not a glyph: GNU `ls` in TTY column mode pads with tabs,
            // so rendering it would tofu after every filename.
            if cd.c != ' ' && cd.c != '\0' && cd.c != '\t' {
                let cp = cd.c as u32;
                // Both Private Use Areas: BMP PUA covers Powerline,
                // Font Awesome, Devicons, Octicons, Codicons and the
                // rest of the legacy Nerd Font ranges; SMP PUA is
                // where Nerd Font v3+ stuffed the Material Design
                // Icons. Regular fonts don't use either area, so we
                // can safely force the bundled Nerd Font across both.
                let is_pua =
                    (0xE000..=0xF8FF).contains(&cp) || (0xF0000..=0xFFFFD).contains(&cp);
                let is_wide = cd.flags.contains(CellFlags::WIDE_CHAR);
                if !is_pua && !is_wide && cd.c.is_ascii_graphic() {
                    // Batchable glyph: extend the open run when it lines
                    // up (same row, same color, contiguous or within a
                    // short bridgeable gap), otherwise start a new one.
                    let fits = run.as_ref().is_some_and(|r| {
                        r.row == cd.row
                            && r.fg == fg
                            && cd.col >= r.next_col
                            && cd.col - r.next_col <= MAX_RUN_GAP
                            && r.content.len() < MAX_RUN_LEN
                    });
                    if fits {
                        let r = run.as_mut().expect("checked by fits");
                        for _ in r.next_col..cd.col {
                            r.content.push(' ');
                        }
                        r.content.push(cd.c);
                        r.next_col = cd.col + 1;
                    } else {
                        if let Some(r) = run.take() {
                            flush_run(&mut frame, r);
                        }
                        run = Some(GlyphRun {
                            row: cd.row,
                            start_col: cd.col,
                            next_col: cd.col + 1,
                            fg,
                            content: cd.c.to_string(),
                        });
                    }
                } else {
                    if let Some(r) = run.take() {
                        flush_run(&mut frame, r);
                    }
                    let font = if is_pua { NERD_FONT } else { self.font };
                    frame.fill_text(CanvasText {
                        content: cd.c.to_string(),
                        position: Point::new(x, y),
                        color: fg,
                        size: Pixels(self.font_size),
                        font,
                        align_x: alignment::Horizontal::Left.into(),
                        align_y: alignment::Vertical::Top,
                        ..Default::default()
                    });
                }
            }

            // Underline, from explicit ANSI SGR flags, or for URL
            // cells that the cursor is currently hovering over (the
            // visual cue paired with the Pointer cursor + tooltip).
            // Other URLs in the viewport stay un-underlined to avoid
            // looking like every link is independently clickable.
            let is_hovered_url = hovered_url_extent.is_some_and(|(r, sc, ec)| {
                cd.row == r && cd.col >= sc && cd.col <= ec
            });
            if cd.flags.intersects(CellFlags::ALL_UNDERLINES) || is_hovered_url {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h - 2.0), Size::new(width, 1.0), fg);
            }

            // Strikethrough
            if cd.flags.contains(CellFlags::STRIKEOUT) {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h / 2.0), Size::new(width, 1.0), fg);
            }
        }
        if let Some(r) = run.take() {
            flush_run(&mut frame, r);
        }

        // Hand the cell snapshot buffer back so its capacity is reused
        // by the next frame.
        DRAW_CELLS.set(cells);

        // Cursor, only render when its visible row falls inside the
        // viewport. When the user scrolls into history, the cursor sits
        // below the visible area and shouldn't be drawn.
        let cursor = term_cursor;
        let visible_cursor_row = cursor.point.line.0 + scroll_offset;
        if (0..screen_lines as i32).contains(&visible_cursor_row) {
            let cx = cursor.point.column.0 as f32 * cell_w + TERM_PAD;
            let cy = visible_cursor_row as f32 * cell_h + TERM_PAD_TOP;
            match cursor.shape {
                CursorShape::Block => {
                    frame.fill_rectangle(
                        Point::new(cx, cy),
                        Size::new(cell_w, cell_h),
                        Color { a: 0.7, ..palette.cursor },
                    );
                }
                CursorShape::Beam => {
                    frame.fill_rectangle(Point::new(cx, cy), Size::new(2.0, cell_h), palette.cursor);
                }
                CursorShape::Underline => {
                    frame.fill_rectangle(
                        Point::new(cx, cy + cell_h - 2.0),
                        Size::new(cell_w, 2.0),
                        palette.cursor,
                    );
                }
                _ => {
                    frame.fill_rectangle(
                        Point::new(cx, cy),
                        Size::new(cell_w, cell_h),
                        Color { a: 0.5, ..palette.cursor },
                    );
                }
            }
        }

        // Scrollbar, only painted while the cursor is over the canvas
        // (or actively dragging), there's actual history to scroll, and
        // we're not in alt-screen mode (no scrollback there).
        // Keep the scrollbar visible during an active text-selection drag
        // too, even if the cursor leaves the widget (hover goes false), so
        // it doesn't blink out while auto-scrolling at the edge.
        let visible_scrollbar = !in_alt_screen
            && (widget_state.hover
                || widget_state.scrollbar_drag.is_some()
                || widget_state.selecting);
        if visible_scrollbar
            && let Some(sb) = scrollbar_geom(
                bounds,
                total_lines,
                screen_lines,
                scroll_offset,
            )
        {
            // Track, faint background gutter so the user has a visible
            // hit target when clicking above/below the thumb.
            frame.fill_rectangle(
                Point::new(sb.track_x, sb.track_y),
                Size::new(sb.track_w, sb.track_h),
                Color { a: 0.08, ..palette.foreground },
            );
            // Thumb, pops out a little when dragging.
            let thumb_alpha = if widget_state.scrollbar_drag.is_some() { 0.55 } else { 0.35 };
            frame.fill_rectangle(
                Point::new(sb.track_x, sb.thumb_y),
                Size::new(sb.track_w, sb.thumb_h),
                Color { a: thumb_alpha, ..palette.foreground },
            );
        }

        // "Ctrl + Click to open the link" tooltip, painted near the
        // hovered URL with a small offset so it doesn't sit directly
        // under the cursor. Stays put once anchored to the URL row;
        // we don't follow per-pixel mouse moves to avoid jitter.
        // The text comes localized from the app; `None` means the user
        // already knows the gesture and the hint stays hidden.
        if let (Some(hint), Some((_url, hover_pos))) = (
            self.link_hint_text.as_ref(),
            widget_state.hovered_url.as_ref(),
        ) {
            // Width estimate at ~11 px: ASCII glyphs ~6.2 px, anything
            // else (CJK and friends) ~11 px, plus 8 px padding per side.
            let text_w: f32 = hint
                .chars()
                .map(|c| if c.is_ascii() { 6.2 } else { 11.0 })
                .sum();
            let tip_w = text_w + 16.0;
            let tip_h = 22.0;
            let tip_y_offset = -28.0; // above the cursor
            let tip_x = (hover_pos.x + 6.0).min(bounds.width - tip_w - 4.0).max(4.0);
            let tip_y = (hover_pos.y + tip_y_offset).max(4.0);
            // Solid terminal background under the tooltip, anything
            // less than fully opaque lets the underlying URL text bleed
            // through and makes the label illegible.
            let bg = palette.background;
            let border = Color { a: 0.6, ..palette.foreground };
            frame.fill_rectangle(
                Point::new(tip_x, tip_y),
                Size::new(tip_w, tip_h),
                bg,
            );
            // Lightweight border via a 1px outline (4 strokes).
            frame.fill_rectangle(Point::new(tip_x, tip_y), Size::new(tip_w, 1.0), border);
            frame.fill_rectangle(
                Point::new(tip_x, tip_y + tip_h - 1.0),
                Size::new(tip_w, 1.0),
                border,
            );
            frame.fill_rectangle(Point::new(tip_x, tip_y), Size::new(1.0, tip_h), border);
            frame.fill_rectangle(
                Point::new(tip_x + tip_w - 1.0, tip_y),
                Size::new(1.0, tip_h),
                border,
            );
            frame.fill_text(CanvasText {
                content: hint.clone(),
                position: Point::new(tip_x + 8.0, tip_y + tip_h / 2.0),
                color: palette.foreground,
                size: Pixels(11.0),
                font: self.font,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Center,
                ..Default::default()
            });
        }

        if let (Some(start), true) = (draw_start, perf_on) {
            let total = start.elapsed();
            let now = std::time::Instant::now();

            let (fps, max_lock, max_cells, max_hl, max_total) = {
                let mut stats = perf_stats().lock().unwrap();
                let frame_gap = stats
                    .last_draw_at
                    .map(|prev| now - prev)
                    .unwrap_or_default();
                stats.last_draw_at = Some(now);
                stats.samples.push_back(PerfSample {
                    frame_gap,
                    lock: lock_dur,
                    cells: cells_dur,
                    highlights: highlights_dur,
                    total,
                });
                while stats.samples.len() > PERF_WINDOW {
                    stats.samples.pop_front();
                }
                (
                    stats.fps(),
                    stats.max_lock(),
                    stats.max_cells(),
                    stats.max_highlights(),
                    stats.max_total(),
                )
            };

            // Two-line HUD pinned top-right. Line 1 shows the
            // current frame; line 2 shows the rolling **max** over
            // the last `PERF_WINDOW` frames so transient spikes
            // the kind that read as typing lag, stay visible long
            // enough to spot. Fractional ms because most healthy
            // draws are well under a single millisecond.
            let ms = |d: std::time::Duration| d.as_secs_f32() * 1000.0;
            let line1 = format!(
                "{:>4.0} fps   T{:>5.1}  L{:>4.1}  C{:>4.1}  H{:>4.1}",
                fps,
                ms(total),
                ms(lock_dur),
                ms(cells_dur),
                ms(highlights_dur),
            );
            let line2 = format!(
                "  peak     T{:>5.1}  L{:>4.1}  C{:>4.1}  H{:>4.1}",
                ms(max_total),
                ms(max_lock),
                ms(max_cells),
                ms(max_hl),
            );

            let panel = perf_panel.expect("perf_panel computed when perf_on");
            let border = Color {
                a: 0.5,
                ..palette.foreground
            };
            frame.fill_rectangle(
                Point::new(panel.x, panel.y),
                Size::new(panel.width, panel.height),
                palette.background,
            );
            frame.fill_rectangle(
                Point::new(panel.x, panel.y),
                Size::new(panel.width, 1.0),
                border,
            );
            frame.fill_rectangle(
                Point::new(panel.x, panel.y + panel.height - 1.0),
                Size::new(panel.width, 1.0),
                border,
            );
            frame.fill_rectangle(
                Point::new(panel.x, panel.y),
                Size::new(1.0, panel.height),
                border,
            );
            frame.fill_rectangle(
                Point::new(panel.x + panel.width - 1.0, panel.y),
                Size::new(1.0, panel.height),
                border,
            );
            for (i, content) in [line1, line2].into_iter().enumerate() {
                frame.fill_text(CanvasText {
                    content,
                    position: Point::new(
                        panel.x + 8.0,
                        panel.y + 6.0 + i as f32 * 13.0,
                    ),
                    color: palette.foreground,
                    size: Pixels(10.0),
                    font: self.font,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    ..Default::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }
}

/// For bold text, promote standard ANSI colors (0-7) to their bright variant (8-15).
/// This makes bold text colorful like in other terminal emulators.
fn brighten_named(color: &alacritty_terminal::vte::ansi::Color) -> alacritty_terminal::vte::ansi::Color {
    use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
    match color {
        AnsiColor::Named(named) => {
            let bright = match named {
                NamedColor::Black => NamedColor::BrightBlack,
                NamedColor::Red => NamedColor::BrightRed,
                NamedColor::Green => NamedColor::BrightGreen,
                NamedColor::Yellow => NamedColor::BrightYellow,
                NamedColor::Blue => NamedColor::BrightBlue,
                NamedColor::Magenta => NamedColor::BrightMagenta,
                NamedColor::Cyan => NamedColor::BrightCyan,
                NamedColor::White => NamedColor::BrightWhite,
                other => *other, // already bright or special, keep as-is
            };
            AnsiColor::Named(bright)
        }
        AnsiColor::Indexed(idx) if *idx < 8 => AnsiColor::Indexed(idx + 8),
        other => *other,
    }
}

#[cfg(test)]
mod paste_tests {
    use super::wrap_paste;

    #[test]
    fn raw_when_mode_disabled() {
        let text = "line one\nline two\n";
        assert_eq!(wrap_paste(text, false), text.as_bytes());
    }

    #[test]
    fn wraps_when_mode_enabled() {
        let out = wrap_paste("hello\nworld", true);
        assert_eq!(out, b"\x1b[200~hello\nworld\x1b[201~");
    }

    #[test]
    fn strips_embedded_markers_so_payload_cannot_break_out() {
        // A clipboard carrying its own bracket markers must not be able to
        // close the bracket early or open a nested one.
        let out = wrap_paste("a\x1b[201~b\x1b[200~c", true);
        assert_eq!(out, b"\x1b[200~abc\x1b[201~");
    }
}

#[cfg(test)]
mod selection_tests {
    use super::{Selection, TerminalState};

    fn state_with(lines: &[&str]) -> TerminalState {
        let mut state = TerminalState::new_no_pty(40, 6).unwrap();
        // Each line ends with CRLF so the emulator moves to column 0 of the
        // next row (LF alone only moves down, keeping the column).
        let mut text = String::new();
        for l in lines {
            text.push_str(l);
            text.push_str("\r\n");
        }
        state.process(text.as_bytes());
        state
    }

    #[test]
    fn flowing_selection_spans_full_middle_rows() {
        // Selected from col 3 of row 0 to col 2 of row 2, a flowing
        // selection takes the tail of row 0, the WHOLE middle row, and the
        // head of row 2. The full middle row is what distinguishes it from
        // a block grab. (Trailing whitespace on intermediate rows is
        // pre-existing behaviour, so assert structure, not the exact run.)
        let state = state_with(&["abcde", "fghij", "klmno"]);
        let sel = Selection { start: (3, 0), end: (2, 2), block: false };
        let text = state.get_selection_text(&sel);
        let rows: Vec<&str> = text.split('\n').collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].trim_end(), "de");
        assert_eq!(rows[1].trim_end(), "fghij"); // full middle row
        assert_eq!(rows[2], "klm");
    }

    #[test]
    fn block_selection_takes_same_columns_each_row() {
        // The same two corners as a block grab only the [3..=4] column
        // slice of every row, not the flowing range.
        let state = state_with(&["abcde", "fghij", "klmno"]);
        let sel = Selection { start: (3, 0), end: (2, 2), block: true };
        // cols 2..=3 (min/max of 3 and 2) per row: "cd", "hi", "mn".
        assert_eq!(state.get_selection_text(&sel), "cd\nhi\nmn");
    }

    #[test]
    fn block_selection_preserves_interior_trailing_spaces() {
        // Two "columns" of unequal width: the block must keep each row's
        // slice verbatim (including trailing spaces) so the columns stay
        // aligned. Row 1's slice is shorter, padded with spaces.
        let state = state_with(&["aa  bb", "cc  dd", "x     "]);
        // Grab cols 0..=3: "aa  ", "cc  ", "x   ". Per-row trim would
        // collapse these to "aa", "cc", "x" and break alignment.
        let sel = Selection { start: (0, 0), end: (3, 2), block: true };
        assert_eq!(state.get_selection_text(&sel), "aa  \ncc  \nx   ");
    }

    #[test]
    fn flowing_selection_trims_trailing_per_row() {
        // A flowing single-row selection that runs into trailing blanks
        // drops them (standard terminal copy behaviour).
        let state = state_with(&["hello     ", "world"]);
        let sel = Selection { start: (0, 0), end: (9, 0), block: false };
        assert_eq!(state.get_selection_text(&sel), "hello");
    }

    #[test]
    fn smart_select_grabs_whole_url() {
        // A click anywhere inside the URL returns the full token span,
        // even though "/" and ":" are word delimiters that would split it.
        let state = state_with(&["see https://example.com x"]);
        let (c0, c1) =
            super::smart_span_at(&state.backend.term, &state.palette, 0, 8)
                .expect("URL should be detected");
        let sel = Selection { start: (c0, 0), end: (c1, 0), block: false };
        assert_eq!(state.get_selection_text(&sel), "https://example.com");
    }

    #[test]
    fn smart_select_misses_plain_word() {
        // A click on a non-token word returns None so the caller falls
        // back to delimiter-word selection.
        let state = state_with(&["see https://example.com x"]);
        assert!(super::smart_span_at(&state.backend.term, &state.palette, 0, 1).is_none());
    }

    #[test]
    fn click_count_classification_cycles_1_to_4() {
        use super::next_click_count;
        // No previous click, or a non-consecutive press, is always single.
        assert_eq!(next_click_count(None, false), 1);
        assert_eq!(next_click_count(None, true), 1);
        assert_eq!(next_click_count(Some(2), false), 1);
        assert_eq!(next_click_count(Some(4), false), 1);
        // Consecutive presses advance single -> double -> triple -> quad,
        // then wrap back to single on the fifth.
        assert_eq!(next_click_count(Some(1), true), 2);
        assert_eq!(next_click_count(Some(2), true), 3);
        assert_eq!(next_click_count(Some(3), true), 4);
        assert_eq!(next_click_count(Some(4), true), 1);
    }

    #[test]
    fn paragraph_selection_spans_to_blank_lines() {
        use std::sync::{Arc, Mutex};
        let arc = Arc::new(Mutex::new(TerminalState::new_no_pty(40, 8).unwrap()));
        // Two paragraphs separated by a blank line.
        arc.lock()
            .unwrap()
            .process(b"para one a\r\npara one b\r\n\r\npara two\r\n");
        let view = super::TerminalView::<()>::new(Arc::clone(&arc));
        let sel = {
            let mut guard = arc.lock().unwrap();
            // Click on the second line of the first paragraph.
            view.semantic_selection(
                &mut guard.backend,
                (0, 1),
                super::SelectGranularity::Paragraph,
            )
        };
        let text = arc.lock().unwrap().get_selection_text(&sel);
        assert_eq!(text, "para one a\npara one b");
    }
}
