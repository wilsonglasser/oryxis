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
        Ok((Self { backend, pty: Some(pty), palette }, rx))
    }

    pub fn new_no_pty(
        cols: u16,
        rows: u16,
    ) -> TerminalResult<Self> {
        let backend = TerminalBackend::new(cols, rows);
        let palette = TerminalPalette::default();
        Ok(Self { backend, pty: None, palette })
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
        true
    }

    pub fn cols(&self) -> u16 { self.backend.cols() }
    pub fn rows(&self) -> u16 { self.backend.rows() }

    /// Extract text from a selection range.
    pub fn get_selection_text(&self, sel: &Selection) -> String {
        let term = &self.backend.term;
        let content = term.renderable_content();
        let mut result = String::new();

        let (start, end) = sel.ordered();

        for item in content.display_iter {
            let col = item.point.column.0 as u16;
            let line = item.point.line.0;

            let in_selection = if start.1 == end.1 {
                // Single line
                line == start.1 as i32 && col >= start.0 && col <= end.0
            } else if line == start.1 as i32 {
                col >= start.0
            } else if line == end.1 as i32 {
                col <= end.0
            } else {
                line > start.1 as i32 && line < end.1 as i32
            };

            if in_selection {
                let c = item.cell.c;
                if c != '\0' {
                    result.push(c);
                }
            }

            // Add newline at end of each selected line
            if in_selection && col as usize == self.backend.cols() as usize - 1 {
                result.push('\n');
            }
        }

        result.trim_end().to_string()
    }
}

// ---------------------------------------------------------------------------
// Selection tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub start: (u16, u16), // (col, row)
    pub end: (u16, u16),
}

impl Selection {
    /// Returns (start, end) ordered top-left to bottom-right.
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
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
}

// ---------------------------------------------------------------------------
// Syntax highlighting for IPs, URLs, and file paths
// ---------------------------------------------------------------------------

struct Highlight {
    row: u16,
    start_col: u16,
    end_col: u16, // inclusive
    color: Color,
}

/// Scan row text for IPv4 addresses, URLs, and Unix file paths (no regex).
fn detect_highlights(
    row_chars: &std::collections::HashMap<u16, Vec<(u16, char)>>,
    palette: &TerminalPalette,
) -> Vec<Highlight> {
    let ip_color = palette.ansi[5];   // magenta
    let url_color = palette.ansi[4];  // blue
    let path_color = palette.ansi[6]; // cyan

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
                            });
                        }
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
        }
    }

    highlights
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

// ---------------------------------------------------------------------------
// Terminal View
// ---------------------------------------------------------------------------

pub struct TerminalView {
    state: Arc<Mutex<TerminalState>>,
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
}

/// Padding around the terminal content (in pixels).
const TERM_PAD: f32 = 10.0;

impl TerminalView {
    pub fn new(state: Arc<Mutex<TerminalState>>) -> Self {
        let font_size = 14.0;
        Self {
            state,
            font_size,
            cell_width: font_size * 0.6,
            cell_height: font_size * 1.286,
        }
    }

    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self.cell_width = size * 0.6;
        self.cell_height = size * 1.286;
        self
    }

    pub fn grid_size_for(width: f32, height: f32, font_size: f32) -> (u16, u16) {
        let cell_width = font_size * 0.6;
        let cell_height = font_size * 1.286;
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

    fn is_in_selection(sel: &Selection, col: u16, row: u16) -> bool {
        let (start, end) = sel.ordered();
        if start.1 == end.1 {
            row == start.1 && col >= start.0 && col <= end.0
        } else if row == start.1 {
            col >= start.0
        } else if row == end.1 {
            col <= end.0
        } else {
            row > start.1 && row < end.1
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
        match event {
            // Mouse press — start selection
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let cell = self.pixel_to_cell(pos);
                    widget_state.selecting = true;
                    widget_state.selection = Some(Selection {
                        start: cell,
                        end: cell,
                    });
                    return Some(CanvasAction::request_redraw().and_capture());
                }
            }
            // Mouse move — extend selection
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if widget_state.selecting
                    && let Some(pos) = cursor.position_in(bounds) {
                        let cell = self.pixel_to_cell(pos);
                        if let Some(ref mut sel) = widget_state.selection {
                            sel.end = cell;
                        }
                        return Some(CanvasAction::request_redraw().and_capture());
                    }
            }
            // Mouse release — end selection + auto-copy if setting enabled
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                widget_state.selecting = false;
                return Some(CanvasAction::capture());
            }
            // Right-click — paste from clipboard
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if cursor.position_in(bounds).is_some() {
                    if let Ok(mut clip) = arboard::Clipboard::new() {
                        if let Ok(text) = clip.get_text() {
                            if let Ok(mut state) = self.state.lock() {
                                state.write(text.as_bytes());
                            }
                        }
                    }
                    return Some(CanvasAction::capture());
                }
            }
            // Mouse wheel — scrollback
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -*y as i32 * 3,
                    mouse::ScrollDelta::Pixels { y, .. } => -(*y / self.cell_height) as i32,
                };
                widget_state.scroll_offset = (widget_state.scroll_offset + lines).max(0);
                if let Ok(state) = self.state.lock() {
                    let grid = state.backend.term.grid();
                    let max_scroll = (grid.total_lines() - grid.screen_lines()) as i32;
                    widget_state.scroll_offset = widget_state.scroll_offset.min(max_scroll);
                }
                return Some(CanvasAction::request_redraw().and_capture());
            }
            // Keyboard — Ctrl+Shift+C copy, Ctrl+Shift+V paste
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                modifiers,
                ..
            }) => {
                if modifiers.control() && modifiers.shift() {
                    match c.as_str() {
                        "C" | "c" => {
                            if let Some(ref sel) = widget_state.selection
                                && !sel.is_empty()
                                    && let Ok(state) = self.state.lock() {
                                        let text = state.get_selection_text(sel);
                                        if !text.is_empty()
                                            && let Ok(mut clip) = arboard::Clipboard::new() {
                                                let _ = clip.set_text(&text);
                                            }
                                    }
                            return Some(CanvasAction::capture());
                        }
                        "V" | "v" => {
                            if let Ok(mut clip) = arboard::Clipboard::new()
                                && let Ok(text) = clip.get_text()
                                    && let Ok(mut state) = self.state.lock() {
                                        state.write(text.as_bytes());
                                    }
                            return Some(CanvasAction::capture());
                        }
                        _ => {}
                    }
                }
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

        // Apply scroll offset
        let scroll_offset = widget_state.scroll_offset;
        if scroll_offset > 0 {
            let vi = alacritty_terminal::grid::Scroll::Delta(-scroll_offset);
            state.backend.term.scroll_display(vi);
        }

        let term = &state.backend.term;
        let palette = &state.palette;
        let colors = term.colors();

        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), palette.background);

        let cell_w = self.cell_width;
        let cell_h = self.cell_height;
        let selection = &widget_state.selection;

        let content = term.renderable_content();
        let term_cursor = content.cursor;

        // --- Pass 1: collect cell data and build row character map ---
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

        for item in content.display_iter {
            let cell = item.cell;
            let point = item.point;

            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            let col = point.column.0 as u16;
            let row = point.line.0 as u16;

            let effective_fg = if cell.flags.contains(CellFlags::BOLD) {
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

        // --- Detect syntax highlights ---
        let highlights = detect_highlights(&row_chars, palette);

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

            // Selection highlight
            let is_selected = selection
                .as_ref()
                .map(|s| TerminalView::is_in_selection(s, cd.col, cd.row))
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
                    font: Font::MONOSPACE,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    ..Default::default()
                });
            }

            // Underline
            if cd.flags.intersects(CellFlags::ALL_UNDERLINES) {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h - 2.0), Size::new(width, 1.0), fg);
            }

            // Strikethrough
            if cd.flags.contains(CellFlags::STRIKEOUT) {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h / 2.0), Size::new(width, 1.0), fg);
            }
        }

        // Cursor
        let cursor = term_cursor;
        let cx = cursor.point.column.0 as f32 * cell_w + TERM_PAD;
        let cy = cursor.point.line.0 as f32 * cell_h + TERM_PAD;

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

        // Reset scroll for next frame
        if scroll_offset > 0 {
            let vi = alacritty_terminal::grid::Scroll::Delta(scroll_offset);
            state.backend.term.scroll_display(vi);
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
