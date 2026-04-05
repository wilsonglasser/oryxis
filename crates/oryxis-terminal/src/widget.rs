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
// Terminal View
// ---------------------------------------------------------------------------

pub struct TerminalView {
    state: Arc<Mutex<TerminalState>>,
    cell_width: f32,
    cell_height: f32,
}

impl TerminalView {
    pub fn new(state: Arc<Mutex<TerminalState>>) -> Self {
        Self {
            state,
            cell_width: 8.4,
            cell_height: 18.0,
        }
    }

    pub fn grid_size_for(width: f32, height: f32) -> (u16, u16) {
        let cell_width = 8.4_f32;
        let cell_height = 18.0_f32;
        let cols = (width / cell_width).floor().max(1.0) as u16;
        let rows = (height / cell_height).floor().max(1.0) as u16;
        (cols, rows)
    }

    fn pixel_to_cell(&self, pos: Point) -> (u16, u16) {
        let col = (pos.x / self.cell_width).floor().max(0.0) as u16;
        let row = (pos.y / self.cell_height).floor().max(0.0) as u16;
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
            // Mouse release — end selection
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                widget_state.selecting = false;
                return Some(CanvasAction::capture());
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
        let mut state = self.state.lock().expect("terminal state lock poisoned");

        // Auto-resize
        let (new_cols, new_rows) = TerminalView::grid_size_for(bounds.width, bounds.height);
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

        for item in content.display_iter {
            let cell = item.cell;
            let point = item.point;

            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            let col = point.column.0 as u16;
            let row = point.line.0 as u16;
            let x = col as f32 * cell_w;
            let y = row as f32 * cell_h;

            let mut fg = palette.resolve(&cell.fg, colors);
            let mut bg = palette.resolve(&cell.bg, colors);

            if cell.flags.contains(CellFlags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(CellFlags::DIM) {
                fg = Color::from_rgba(fg.r * 0.66, fg.g * 0.66, fg.b * 0.66, fg.a);
            }

            // Selection highlight
            let is_selected = selection
                .as_ref()
                .map(|s| TerminalView::is_in_selection(s, col, row))
                .unwrap_or(false);

            if is_selected {
                bg = Color::from_rgba(0.30, 0.56, 1.0, 0.35); // blue selection
                fg = Color::WHITE;
            }

            // Draw background
            let is_default_bg = !is_selected
                && (bg.r - palette.background.r).abs() < 0.01
                && (bg.g - palette.background.g).abs() < 0.01
                && (bg.b - palette.background.b).abs() < 0.01;

            if !is_default_bg {
                let width = if cell.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y), Size::new(width, cell_h), bg);
            }

            // Draw character
            let c = cell.c;
            if c != ' ' && c != '\0' {
                frame.fill_text(CanvasText {
                    content: c.to_string(),
                    position: Point::new(x, y),
                    color: fg,
                    size: Pixels(14.0),
                    font: Font::MONOSPACE,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    ..Default::default()
                });
            }

            // Underline
            if cell.flags.intersects(CellFlags::ALL_UNDERLINES) {
                let width = if cell.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h - 2.0), Size::new(width, 1.0), fg);
            }

            // Strikethrough
            if cell.flags.contains(CellFlags::STRIKEOUT) {
                let width = if cell.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h / 2.0), Size::new(width, 1.0), fg);
            }
        }

        // Cursor
        let cursor = content.cursor;
        let cx = cursor.point.column.0 as f32 * cell_w;
        let cy = cursor.point.line.0 as f32 * cell_h;

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
