use crate::backend::TerminalBackend;
use crate::colors::TerminalPalette;
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
        let (pty, rx) = PtyHandle::spawn(cols, rows)?;
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

    /// Wire a remote resize sender — called from the app once an SSH
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
        if let Some(ref mut pty) = self.pty
            && let Err(e) = pty.write(data) {
                tracing::error!("PTY write error: {}", e);
            }
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

    /// Extract text from a selection range.
    pub fn get_selection_text(&self, sel: &Selection) -> String {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        let grid = self.backend.term.grid();
        let topmost = grid.topmost_line();
        let bottommost = grid.bottommost_line();
        let cols = grid.columns();
        let last_col = cols.saturating_sub(1) as u16;
        let (start, end) = sel.ordered();
        let mut result = String::new();

        // Iterate over the line range manually — selection lines are in
        // grid coordinates (negative for scrollback) which `display_iter`
        // alone wouldn't reach unless we mutated the display offset.
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
            for c in start_col..=end_col {
                let cell = &row[Column(c as usize)];
                if cell.c != '\0' {
                    result.push(cell.c);
                }
            }
            if line_idx < end.1 {
                result.push('\n');
            }
        }

        result.trim_end().to_string()
    }
}

// ---------------------------------------------------------------------------
// Selection tracking
// ---------------------------------------------------------------------------

/// A selection range stored in **grid-line coordinates** (alacritty `Line`,
/// signed: negative = scrollback, 0..screen_lines = live screen). Storing
/// in line-space means the selection follows the content as the user
/// scrolls — at draw time we translate line → visible row using the
/// current scroll_offset.
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub start: (u16, i32), // (col, line)
    pub end: (u16, i32),
}

impl Selection {
    /// Returns (start, end) ordered top-left to bottom-right.
    pub fn ordered(&self) -> ((u16, i32), (u16, i32)) {
        if self.start.1 < self.end.1
            || (self.start.1 == self.end.1 && self.start.0 <= self.end.0)
        {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
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
fn detect_highlights(
    row_chars: &std::collections::HashMap<u16, Vec<(u16, char)>>,
    palette: &TerminalPalette,
) -> Vec<Highlight> {
    let ip_color = palette.ansi[5];   // magenta
    let url_color = palette.ansi[4];  // blue
    let path_color = palette.ansi[6]; // cyan
    let num_color = palette.ansi[5];  // magenta — same as IP, easy scan

    let mut highlights = Vec::new();

    for (&row, cols) in row_chars {
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
                // Only slice at ASCII 'h' — guaranteed char boundary. Skipping this
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
                // version strings) — those should keep the surrounding fg.
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
                // Optional decimal part — must be `.<digit>+`.
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
                // "v1.2-rc" — the IP path already handled the first; we
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

/// Returns true when the given cell is part of a URL highlight — used by the
/// draw pass to paint an underline under clickable links.
#[inline]
fn is_url_cell(highlights: &[Highlight], row: u16, col: u16) -> bool {
    highlights.iter().any(|h| {
        h.kind == HighlightKind::Url
            && h.row == row
            && col >= h.start_col
            && col <= h.end_col
    })
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
    let content = term.renderable_content();
    let mut row_chars: Vec<(u16, char)> = Vec::new();
    for item in content.display_iter {
        let row = item.point.line.0 as u16;
        if row != target_row {
            continue;
        }
        let col = item.point.column.0 as u16;
        let c = item.cell.c;
        if c != ' ' && c != '\0' {
            row_chars.push((col, c));
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

/// Best-effort spawn of the OS default handler for a URL. Runs detached; the
/// terminal widget never blocks on it and errors are swallowed — a failed
/// launch just means nothing happens visibly, same as any other click miss.
fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
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

pub struct TerminalView {
    state: Arc<Mutex<TerminalState>>,
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
    font: Font,
    /// When true, completing a mouse selection auto-copies it to the
    /// system clipboard — same UX as XTerm / iTerm "copy on select".
    copy_on_select: bool,
    /// When true, ANSI bold flag promotes the named foreground color to
    /// its bright variant (red → bright red, etc).
    bold_is_bright: bool,
    /// When true, the terminal scans visible rows for URLs / IPs / paths
    /// and tints them. Disable to recover frame time in dense UIs.
    keyword_highlight: bool,
}

/// Padding around the terminal content (in pixels).
const TERM_PAD: f32 = 10.0;

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
    let track_y = TERM_PAD;
    let track_h = (bounds.height - TERM_PAD * 2.0).max(0.0);
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

impl TerminalView {
    pub fn new(state: Arc<Mutex<TerminalState>>) -> Self {
        let font_size = 14.0;
        Self {
            state,
            font_size,
            cell_width: font_size * 0.6,
            cell_height: font_size * 1.15,
            font: Font::MONOSPACE,
            copy_on_select: true,
            bold_is_bright: true,
            keyword_highlight: true,
        }
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

    pub fn with_bold_is_bright(mut self, on: bool) -> Self {
        self.bold_is_bright = on;
        self
    }

    pub fn with_keyword_highlight(mut self, on: bool) -> Self {
        self.keyword_highlight = on;
        self
    }

    /// Override the font used for cell rendering. If the font can't be resolved
    /// by cosmic-text, it falls back to the system default monospace.
    pub fn with_font_name(mut self, name: &str) -> Self {
        // Leak the string so Font::with_name can hold a 'static &str. The number
        // of unique names is bounded (~20 from the picker), so the total leak is
        // tiny and amortized across the process lifetime.
        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
        self.font = Font::with_name(leaked);
        self
    }

    pub fn grid_size_for(width: f32, height: f32, font_size: f32) -> (u16, u16) {
        let cell_width = font_size * 0.6;
        let cell_height = font_size * 1.15;
        let usable_w = (width - TERM_PAD * 2.0).max(cell_width);
        let usable_h = (height - TERM_PAD * 2.0).max(cell_height);
        let cols = (usable_w / cell_width).floor().max(1.0) as u16;
        let rows = (usable_h / cell_height).floor().max(1.0) as u16;
        (cols, rows)
    }

    fn pixel_to_cell(&self, pos: Point) -> (u16, u16) {
        let col = ((pos.x - TERM_PAD) / self.cell_width).floor().max(0.0) as u16;
        let row = ((pos.y - TERM_PAD) / self.cell_height).floor().max(0.0) as u16;
        (col, row)
    }

    /// Convert a visible-row index to the alacritty grid-line index, given
    /// the current scroll offset. Visible row 0 is the top of the canvas.
    fn visible_row_to_line(visible_row: u16, scroll_offset: i32) -> i32 {
        visible_row as i32 - scroll_offset
    }

    fn is_in_selection(sel: &Selection, col: u16, line: i32) -> bool {
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

impl<Message> canvas::Program<Message, Theme> for TerminalView
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
        // Refresh hover state for every event we see — drives the
        // scrollbar's reveal-on-hover behaviour. Done before the match so
        // we don't have to repeat it in every arm.
        let new_hover = cursor.position_in(bounds).is_some();
        let hover_changed = widget_state.hover != new_hover;
        widget_state.hover = new_hover;

        match event {
            // Mouse press — scrollbar interaction takes priority, then
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
                    if let Ok(state) = self.state.lock()
                        && let Some(url) = url_at_cell(&state.backend.term, vrow, col)
                    {
                        drop(state);
                        open_url(&url);
                        return Some(CanvasAction::capture());
                    }
                    widget_state.selecting = true;
                    widget_state.selection = Some(Selection {
                        start: (col, line),
                        end: (col, line),
                    });
                    return Some(CanvasAction::request_redraw().and_capture());
                }
            }
            // Mouse move — drag scrollbar thumb or extend selection.
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
                    && let Some(pos) = cursor.position_in(bounds) {
                        let (col, vrow) = self.pixel_to_cell(pos);
                        let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset);
                        if let Some(ref mut sel) = widget_state.selection {
                            sel.end = (col, line);
                        }
                        return Some(CanvasAction::request_redraw().and_capture());
                    }
                if hover_changed {
                    return Some(CanvasAction::request_redraw());
                }
            }
            // Mouse release — end selection or scrollbar drag.
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let was_dragging = widget_state.scrollbar_drag.is_some();
                widget_state.scrollbar_drag = None;
                let was_selecting = widget_state.selecting;
                widget_state.selecting = false;
                // Auto-copy the just-finished selection when the setting is
                // enabled (XTerm / iTerm behaviour). Skip degenerate
                // selections that didn't move (single click).
                if was_selecting
                    && self.copy_on_select
                    && let Some(ref sel) = widget_state.selection
                    && !sel.is_empty()
                    && let Ok(state) = self.state.lock()
                {
                    let text = state.get_selection_text(sel);
                    drop(state);
                    if !text.is_empty()
                        && let Ok(mut clip) = arboard::Clipboard::new()
                    {
                        let _ = clip.set_text(&text);
                    }
                }
                if was_dragging {
                    return Some(CanvasAction::request_redraw().and_capture());
                }
                return Some(CanvasAction::capture());
            }
            // Right-click — paste from clipboard
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                if cursor.position_in(bounds).is_some() =>
            {
                if let Ok(mut clip) = arboard::Clipboard::new()
                    && let Ok(text) = clip.get_text()
                    && let Ok(mut state) = self.state.lock()
                {
                    state.write(text.as_bytes());
                }
                return Some(CanvasAction::capture());
            }
            // Mouse wheel — scrollback in the OS-natural direction:
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
                let in_alt_screen = self
                    .state
                    .lock()
                    .ok()
                    .map(|s| {
                        s.backend
                            .term
                            .mode()
                            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
                    })
                    .unwrap_or(false);
                if in_alt_screen {
                    // Translate wheel into arrow-key bytes for the remote
                    // app — `top`/`vim`/`less` all listen for these.
                    let bytes: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    let count = lines.unsigned_abs().min(10);
                    if let Ok(mut state) = self.state.lock() {
                        for _ in 0..count {
                            state.write(bytes);
                        }
                    }
                    return Some(CanvasAction::capture());
                }
                widget_state.scroll_offset = (widget_state.scroll_offset + lines).max(0);
                if let Ok(state) = self.state.lock() {
                    let grid = state.backend.term.grid();
                    let max_scroll = (grid.total_lines() - grid.screen_lines()) as i32;
                    widget_state.scroll_offset = widget_state.scroll_offset.min(max_scroll);
                }
                return Some(CanvasAction::request_redraw().and_capture());
            }
            // Keyboard — Ctrl+Shift+C copy (paste is handled in app.rs so it can
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
                    if !text.is_empty()
                        && let Ok(mut clip) = arboard::Clipboard::new()
                    {
                        let _ = clip.set_text(&text);
                    }
                }
                return Some(CanvasAction::capture());
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        widget_state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(poisoned) => poisoned.into_inner(),
        };

        // Auto-resize
        let (new_cols, new_rows) = TerminalView::grid_size_for(bounds.width, bounds.height, self.font_size);
        state.resize(new_cols, new_rows);

        // Alt-screen apps (top, vim, less, htop, …) own the entire
        // viewport with cursor positioning — there's no scrollback to
        // page through. Force scroll_offset=0 so the user can't get
        // stuck looking at stale history while the app keeps redrawing.
        let in_alt_screen = state
            .backend
            .term
            .mode()
            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);

        // Clamp scroll offset against the current grid bounds — resizes
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

        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), palette.background);

        let cell_w = self.cell_width;
        let cell_h = self.cell_height;
        let selection = &widget_state.selection;

        let term_cursor = term.renderable_content().cursor;
        let grid = term.grid();
        let screen_lines = grid.screen_lines();
        let cols_count = grid.columns();
        let topmost = grid.topmost_line();
        let bottommost = grid.bottommost_line();

        // --- Pass 1: collect cell data and build row character map ---
        // Iterate the grid manually using `scroll_offset` as a row offset
        // instead of mutating alacritty's `display_offset` via
        // `scroll_display`. The previous approach yielded `display_iter`
        // entries with negative `point.line.0` for scrollback rows, which
        // when cast to `u16` wrapped to enormous numbers — those cells
        // ended up rendered far off-screen, leaving blank rows in their
        // place. Manual indexing keeps the math sane.
        struct CellData {
            col: u16,
            row: u16,
            c: char,
            fg: Color,
            bg: Color,
            flags: CellFlags,
        }

        let mut cells: Vec<CellData> = Vec::new();
        let mut row_chars: std::collections::HashMap<u16, Vec<(u16, char)>>
            = std::collections::HashMap::new();

        for visible_row in 0..screen_lines {
            let line = alacritty_terminal::index::Line(visible_row as i32 - scroll_offset);
            if line < topmost || line > bottommost {
                continue;
            }
            let row_data = &grid[line];
            for col_i in 0..cols_count {
                let cell = &row_data[alacritty_terminal::index::Column(col_i)];

                if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    continue;
                }

                let col = col_i as u16;
                let row = visible_row as u16;

                let effective_fg = if cell.flags.contains(CellFlags::BOLD) && self.bold_is_bright {
                    brighten_named(&cell.fg)
                } else {
                    cell.fg
                };
                let fg = palette.resolve(&effective_fg, colors);
                let bg = palette.resolve(&cell.bg, colors);

                let c = cell.c;
                if c != ' ' && c != '\0' {
                    row_chars.entry(row).or_default().push((col, c));
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
        }

        // --- Detect syntax highlights ---
        let highlights = if self.keyword_highlight {
            detect_highlights(&row_chars, palette)
        } else {
            Vec::new()
        };

        // --- Pass 2: draw cells with highlight overrides ---
        for cd in &cells {
            let x = cd.col as f32 * cell_w + TERM_PAD;
            let y = cd.row as f32 * cell_h + TERM_PAD;

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

            // Selection highlight — convert visible row to grid-line so
            // the selection follows scrolled content instead of staying
            // glued to viewport coordinates.
            let cell_line = TerminalView::visible_row_to_line(cd.row, scroll_offset);
            let is_selected = selection
                .as_ref()
                .map(|s| TerminalView::is_in_selection(s, cd.col, cell_line))
                .unwrap_or(false);

            if is_selected {
                bg = Color::from_rgba(0.133, 0.60, 0.569, 0.35);
                fg = Color::WHITE;
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

            // Draw character
            if cd.c != ' ' && cd.c != '\0' {
                frame.fill_text(CanvasText {
                    content: cd.c.to_string(),
                    position: Point::new(x, y),
                    color: fg,
                    size: Pixels(self.font_size),
                    font: self.font,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    ..Default::default()
                });
            }

            // Underline — either from cell flags (ANSI SGR) or because the
            // cell is part of a detected URL (auto-linker visual cue).
            let is_url = is_url_cell(&highlights, cd.row, cd.col);
            if cd.flags.intersects(CellFlags::ALL_UNDERLINES) || is_url {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h - 2.0), Size::new(width, 1.0), fg);
            }

            // Strikethrough
            if cd.flags.contains(CellFlags::STRIKEOUT) {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h / 2.0), Size::new(width, 1.0), fg);
            }
        }

        // Cursor — only render when its visible row falls inside the
        // viewport. When the user scrolls into history, the cursor sits
        // below the visible area and shouldn't be drawn.
        let cursor = term_cursor;
        let visible_cursor_row = cursor.point.line.0 + scroll_offset;
        if (0..screen_lines as i32).contains(&visible_cursor_row) {
            let cx = cursor.point.column.0 as f32 * cell_w + TERM_PAD;
            let cy = visible_cursor_row as f32 * cell_h + TERM_PAD;
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

        // Scrollbar — only painted while the cursor is over the canvas
        // (or actively dragging), there's actual history to scroll, and
        // we're not in alt-screen mode (no scrollback there).
        let visible_scrollbar =
            !in_alt_screen && (widget_state.hover || widget_state.scrollbar_drag.is_some());
        if visible_scrollbar
            && let Some(sb) = scrollbar_geom(
                bounds,
                grid.total_lines(),
                grid.screen_lines(),
                scroll_offset,
            )
        {
            // Track — faint background gutter so the user has a visible
            // hit target when clicking above/below the thumb.
            frame.fill_rectangle(
                Point::new(sb.track_x, sb.track_y),
                Size::new(sb.track_w, sb.track_h),
                Color { a: 0.08, ..palette.foreground },
            );
            // Thumb — pops out a little when dragging.
            let thumb_alpha = if widget_state.scrollbar_drag.is_some() { 0.55 } else { 0.35 };
            frame.fill_rectangle(
                Point::new(sb.track_x, sb.thumb_y),
                Size::new(sb.track_w, sb.thumb_h),
                Color { a: thumb_alpha, ..palette.foreground },
            );
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
                other => *other, // already bright or special — keep as-is
            };
            AnsiColor::Named(bright)
        }
        AnsiColor::Indexed(idx) if *idx < 8 => AnsiColor::Indexed(idx + 8),
        other => *other,
    }
}
