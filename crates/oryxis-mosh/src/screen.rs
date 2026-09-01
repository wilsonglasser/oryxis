//! The screen mosh keeps its states on, backed by the SAME emulator the
//! pane draws with.
//!
//! This exists so Oryxis carries ONE terminal emulator rather than two.
//! `mosh-rs` ships a screen on the `vt100` crate and it would work, but
//! it would also mean two implementations holding two opinions about
//! the same screen, and the diff this client sends to the pane is
//! computed against one of them and drawn by the other. Where they
//! disagree, the pane shows something the model never described.
//!
//! That is not a hypothetical. The two crates already disagree about
//! `ESC [ ? 1049 h`: `vt100` moves the cursor home on the switch to the
//! alternate screen, xterm and alacritty leave it where it was.
//!
//! `mosh_rs::Screen` is what the emulator has to be able to do;
//! [`DiffScreen`] is what turns two of them into the escape bytes that
//! carry a terminal from one to the other. `vt100` gets that from its
//! own `contents_diff`; alacritty has no equivalent, so it is here, and
//! it is the reason this module is more than the ~150 lines an embedder
//! that draws its own grid would need.

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::{self, Color as AnsiColor, NamedColor, Rgb};

use mosh_rs::screen::{Cell, Color, DiffScreen, OverlayCell, OverlayCursor, Rendition, Screen};

/// Where the emulator reports what it cannot put in the grid.
///
/// The title is the one that matters and the one that is easy to lose:
/// alacritty has no public getter for it, so a listener that throws
/// events away throws the window title away with them, silently.
#[derive(Clone, Default)]
struct Events {
    title: Arc<Mutex<Option<String>>>,
}

impl EventListener for Events {
    fn send_event(&self, event: Event) {
        if let Event::Title(title) = event
            && let Ok(mut held) = self.title.lock()
        {
            *held = Some(title);
        }
    }
}

#[derive(Clone, Copy)]
struct Size {
    cols: u16,
    rows: u16,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

/// A mosh screen on `alacritty_terminal`.
pub struct AlacrittyScreen {
    term: Term<Events>,
    processor: ansi::Processor,
    events: Events,
    /// Carried so a [`Clone`] measures text the way the original does.
    ambiguous_width_wide: bool,
}

impl AlacrittyScreen {
    /// A blank screen of the given shape.
    ///
    /// `ambiguous_width_wide` must be the SAME answer the pane's own
    /// emulator gives, and that is the whole reason it is a parameter
    /// here: this screen decides where the server's bytes land, the pane
    /// draws the diff computed from it, and two emulators disagreeing
    /// about how wide `│` is would make the pane show something this
    /// model never described.
    ///
    /// It is also FIXED for the life of the session, since there is no
    /// path to reconfigure the screen once the protocol owns it. The
    /// caller pins the pane's own answer at handover for that reason;
    /// editing the host takes effect on the next connect.
    pub fn new(rows: u16, cols: u16, ambiguous_width_wide: bool) -> Self {
        let size = Size { cols, rows };
        let config = TermConfig {
            // No scrollback. mosh synchronizes the VISIBLE screen and
            // nothing else; the pane's own terminal keeps the history,
            // and a second copy here would be a second copy of every
            // state the session holds.
            scrolling_history: 0,
            ambiguous_width_wide,
            ..Default::default()
        };
        let events = Events::default();
        Self {
            term: Term::new(config, &size, events.clone()),
            processor: ansi::Processor::new(),
            events,
            ambiguous_width_wide,
        }
    }

    fn cursor_visible(&self) -> bool {
        self.term.mode().contains(TermMode::SHOW_CURSOR)
    }
}

impl Clone for AlacrittyScreen {
    /// A state must be copyable, and `Term` is not `Clone` because it
    /// holds a listener. The GRID is, and the grid is the state: a
    /// fresh emulator restored to that grid continues from exactly
    /// where it left off.
    fn clone(&self) -> Self {
        let mut copy = Self::new(
            self.term.screen_lines() as u16,
            self.term.columns() as u16,
            self.ambiguous_width_wide,
        );
        *copy.term.grid_mut() = self.term.grid().clone();
        // Cursor visibility is a MODE, not a grid cell, so a copy that
        // only took the grid would report the cursor shown on a screen
        // that had hidden it, and the diff between them would say to
        // hide it again on every frame.
        if !self.cursor_visible() {
            copy.feed(b"\x1b[?25l");
        }
        // The title is part of the state a copy has to carry: a diff
        // applied to a copy may set a new one, and the copy that
        // becomes the displayed screen has to know the old one.
        if let (Ok(mut theirs), Ok(ours)) = (copy.events.title.lock(), self.events.title.lock()) {
            theirs.clone_from(&ours);
        }
        copy
    }
}

fn color_of(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Spec(Rgb { r, g, b }) => Color::Rgb(r, g, b),
        AnsiColor::Indexed(i) => Color::Indexed(i),
        // The first sixteen named colours ARE the indexed ones, in
        // order. Everything past them (foreground, background, cursor,
        // the dim variants) has no index to give, and saying "default"
        // is what lets a reset carry whatever the terminal uses.
        AnsiColor::Named(named) => match named as usize {
            index if index < 16 => Color::Indexed(index as u8),
            _ => Color::Default,
        },
    }
}

fn ansi_color(color: Color, is_fg: bool) -> AnsiColor {
    match color {
        Color::Default => AnsiColor::Named(if is_fg {
            NamedColor::Foreground
        } else {
            NamedColor::Background
        }),
        Color::Indexed(i) => AnsiColor::Indexed(i),
        Color::Rgb(r, g, b) => AnsiColor::Spec(Rgb { r, g, b }),
    }
}

fn rendition_of(cell: &TermCell) -> Rendition {
    let flags = cell.flags;
    Rendition {
        fg: color_of(cell.fg),
        bg: color_of(cell.bg),
        bold: flags.contains(Flags::BOLD),
        dim: flags.contains(Flags::DIM),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.contains(Flags::UNDERLINE),
        inverse: flags.contains(Flags::INVERSE),
    }
}

/// `ESC [ row ; col H`, one-based as the escape is.
fn push_cup(out: &mut Vec<u8>, row: u16, col: u16) {
    out.extend_from_slice(format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
}

impl Screen for AlacrittyScreen {
    fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.term.resize(Size { cols, rows });
    }

    fn rows(&self) -> u16 {
        self.term.screen_lines() as u16
    }

    fn cols(&self) -> u16 {
        self.term.columns() as u16
    }

    fn cursor(&self) -> (u16, u16) {
        let point = self.term.grid().cursor.point;
        (point.line.0.max(0) as u16, point.column.0 as u16)
    }

    fn cell(&self, row: u16, col: u16) -> Cell {
        if row >= self.rows() || col >= self.cols() {
            return Cell::default();
        }
        let cell = &self.term.grid()[Line(i32::from(row))][Column(usize::from(col))];
        Cell {
            // A blank cell holds a space in alacritty and an empty
            // string in vt100; `Cell::is_blank` treats them the same.
            contents: cell.c.to_string(),
            rendition: rendition_of(cell),
        }
    }

    fn title(&self) -> Option<String> {
        self.events.title.lock().ok()?.clone()
    }

    fn text(&self) -> String {
        let (rows, cols) = (self.rows(), self.cols());
        let mut out = String::new();
        for row in 0..rows {
            let line = &self.term.grid()[Line(i32::from(row))];
            let mut text = String::new();
            for col in 0..cols {
                text.push(line[Column(usize::from(col))].c);
            }
            out.push_str(text.trim_end());
            if row + 1 < rows {
                out.push('\n');
            }
        }
        out
    }

    /// Write the predictions straight into the grid, which is what mosh
    /// does. The escape-byte default has to hold back on the last
    /// column, where a write arms the terminal's pending wrap; owning
    /// the grid, this does not.
    fn draw_overlay(&mut self, cells: &[OverlayCell], cursor: OverlayCursor) {
        let (rows, cols) = (self.rows(), self.cols());
        for painted in cells {
            if painted.row >= rows || painted.col >= cols {
                continue;
            }
            let glyph = painted.cell.glyph().chars().next().unwrap_or(' ');
            let rendition = painted.cell.rendition;
            let target = &mut self.term.grid_mut()[Line(i32::from(painted.row))]
                [Column(usize::from(painted.col))];
            target.c = glyph;
            target.fg = ansi_color(rendition.fg, true);
            target.bg = ansi_color(rendition.bg, false);
            let mut flags = Flags::empty();
            flags.set(Flags::BOLD, rendition.bold);
            flags.set(Flags::DIM, rendition.dim);
            flags.set(Flags::ITALIC, rendition.italic);
            flags.set(Flags::UNDERLINE, rendition.underline || painted.underline);
            flags.set(Flags::INVERSE, rendition.inverse);
            target.flags = flags;
        }
        match cursor {
            OverlayCursor::Unchanged => {}
            OverlayCursor::At(row, col) if row < rows && col < cols => {
                self.term.grid_mut().cursor.point =
                    Point::new(Line(i32::from(row)), Column(usize::from(col)));
            }
            OverlayCursor::At(..) => {}
            // Owning the grid does not help here: cursor visibility is
            // a terminal MODE, and feeding the escape is how it is set.
            OverlayCursor::Hidden => self.feed(b"\x1b[?25l"),
        }
    }
}

/// Paint every cell of the screen, with no assumptions about what is
/// already there.
fn paint_all(screen: &AlacrittyScreen, out: &mut Vec<u8>) {
    // Home and clear first: a repaint is for a terminal whose state
    // cannot be known, and anything left outside the cells written
    // below would survive.
    out.extend_from_slice(b"\x1b[H\x1b[2J");
    for row in 0..screen.rows() {
        paint_row(screen, None, row, out);
    }
}

/// How many unchanged cells a run will write through rather than break
/// for.
///
/// Breaking costs a cursor move, which is six bytes or more; carrying
/// on costs one byte per cell skipped. So a short gap is cheaper to
/// rewrite than to jump over, and jumping over every one of them turns
/// an ordinary line of text into a run per word, because the spaces
/// between the words were already spaces.
const MAX_RUN_GAP: u16 = 4;

/// The cells of `row` that differ from `previous`, or all of them when
/// there is no previous.
///
/// Adjacent changed cells sharing a rendition are written as one move,
/// one attribute change and one string, so a line of ordinary text
/// costs one of each rather than one per character.
fn paint_row(
    screen: &AlacrittyScreen,
    previous: Option<&AlacrittyScreen>,
    row: u16,
    out: &mut Vec<u8>,
) {
    let cols = screen.cols();
    let same = |col: u16, cell: &Cell| previous.is_some_and(|p| p.cell(row, col) == *cell);

    let mut col = 0;
    while col < cols {
        let cell = screen.cell(row, col);
        if same(col, &cell) {
            col += 1;
            continue;
        }

        // Gather the run. It ends at a change of rendition, at the last
        // column (writing there arms the terminal's pending wrap, so
        // what the cursor does next is not something to reason about),
        // or at a gap of unchanged cells long enough to be worth a move.
        let rendition = cell.rendition;
        let start = col;
        let mut text = String::new();
        let mut gap = 0u16;
        // Where the last cell that actually differed ended, so the run
        // is not left with unchanged cells trailing off it.
        let mut written = 0usize;
        while col < cols {
            let cell = screen.cell(row, col);
            if cell.rendition != rendition {
                break;
            }
            if same(col, &cell) {
                gap += 1;
                if gap > MAX_RUN_GAP {
                    break;
                }
            } else {
                gap = 0;
            }
            // A blank reads as a space; the cell it replaces may have
            // held something, so it is written rather than skipped.
            let glyph = cell.contents.chars().next().unwrap_or(' ');
            text.push(if glyph == '\0' { ' ' } else { glyph });
            if gap == 0 {
                written = text.chars().count();
            }
            col += 1;
            if col == cols {
                break;
            }
        }
        // Trim the unchanged tail: it was carried in case a change
        // followed, and none did.
        text.truncate(text.char_indices().nth(written).map_or(text.len(), |(i, _)| i));
        col = start + written as u16;
        if text.is_empty() {
            col = start + 1;
            continue;
        }
        push_cup(out, row, start);
        out.extend_from_slice(rendition.sgr(false).as_bytes());
        out.extend_from_slice(text.as_bytes());
        // Attributes are not left set for whatever is drawn next.
        out.extend_from_slice(b"\x1b[m");
    }
}

impl DiffScreen for AlacrittyScreen {
    fn diff_from(&self, previous: &Self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.rows() != previous.rows() || self.cols() != previous.cols() {
            // A reshaped screen shares no coordinates with the old one,
            // so there is nothing to diff against.
            paint_all(self, &mut out);
        } else {
            for row in 0..self.rows() {
                paint_row(self, Some(previous), row, &mut out);
            }
        }

        // The cursor goes last, so it ends where this screen says it is
        // rather than after whatever was written.
        let (row, col) = self.cursor();
        if out.is_empty()
            && previous.cursor() == (row, col)
            && previous.cursor_visible() == self.cursor_visible()
        {
            // Nothing changed at all, which is the common case and the
            // one that has to cost nothing.
            return out;
        }
        push_cup(&mut out, row, col);
        if self.cursor_visible() != previous.cursor_visible() {
            out.extend_from_slice(if self.cursor_visible() {
                b"\x1b[?25h"
            } else {
                b"\x1b[?25l"
            });
        }
        out
    }

    fn repaint(&self) -> Vec<u8> {
        let mut out = Vec::new();
        paint_all(self, &mut out);
        let (row, col) = self.cursor();
        push_cup(&mut out, row, col);
        // Restated rather than diffed: a repaint is for a terminal
        // whose state cannot be assumed, and that includes this.
        out.extend_from_slice(if self.cursor_visible() {
            b"\x1b[?25h"
        } else {
            b"\x1b[?25l"
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(rows: u16, cols: u16, bytes: &[u8]) -> AlacrittyScreen {
        let mut s = AlacrittyScreen::new(rows, cols, false);
        s.feed(bytes);
        s
    }

    /// The property the whole module exists for: what `diff_from`
    /// produces, applied to the screen it was computed FROM, gives the
    /// screen it was computed against.
    ///
    /// Checked by replaying the bytes into a real emulator rather than
    /// by reading the escapes, because the escapes being plausible is
    /// not the claim. It is also why this is a round trip and not a
    /// comparison against a recorded string: a differ that emits
    /// something different but equivalent is not wrong.
    fn round_trips(before: &[u8], after: &[u8]) {
        let (rows, cols) = (6u16, 20u16);
        let previous = screen(rows, cols, before);
        let target = screen(rows, cols, after);

        let mut applied = previous.clone();
        applied.feed(&target.diff_from(&previous));

        assert_eq!(
            applied.text(),
            target.text(),
            "\nfrom {before:?}\n  to {after:?}\ndiff {:?}",
            String::from_utf8_lossy(&target.diff_from(&previous))
        );
        assert_eq!(applied.cursor(), target.cursor(), "the cursor ended elsewhere");
        for row in 0..rows {
            for col in 0..cols {
                assert_eq!(
                    applied.cell(row, col),
                    target.cell(row, col),
                    "cell ({row}, {col}) differs"
                );
            }
        }
    }

    #[test]
    fn text_appearing_on_a_blank_screen() {
        round_trips(b"", b"hello");
    }

    #[test]
    fn text_replaced_by_other_text() {
        round_trips(b"hello world", b"\x1b[Hgoodbye");
    }

    #[test]
    fn text_erased() {
        round_trips(b"hello world", b"\x1b[H\x1b[2J");
    }

    #[test]
    fn colour_and_attributes_survive_the_trip() {
        round_trips(
            b"plain",
            b"\x1b[H\x1b[1;31;44mbold red on blue\x1b[m",
        );
    }

    #[test]
    fn truecolour_survives_the_trip() {
        round_trips(b"", b"\x1b[38;2;10;200;30mgreenish\x1b[m");
    }

    #[test]
    fn several_rows_at_once() {
        round_trips(b"", b"one\r\ntwo\r\nthree\r\nfour");
    }

    /// The last column arms the terminal's pending wrap, so a run that
    /// ran through it and kept writing would put the next character on
    /// the following line.
    #[test]
    fn writing_the_last_column_does_not_drag_the_next_row_with_it() {
        // Exactly 20 wide, then something on the row below.
        round_trips(b"", b"12345678901234567890\r\nnext");
    }

    #[test]
    fn a_screen_that_did_not_change_costs_nothing() {
        let s = screen(4, 10, b"steady");
        assert!(
            s.diff_from(&s.clone()).is_empty(),
            "an unchanged screen has to produce no bytes at all"
        );
    }

    #[test]
    fn a_cursor_that_moved_is_a_change_even_with_the_same_text() {
        let previous = screen(4, 10, b"abc");
        let mut target = previous.clone();
        target.feed(b"\x1b[3;5H");
        let diff = target.diff_from(&previous);
        assert!(!diff.is_empty(), "the cursor moved and nothing said so");

        let mut applied = previous.clone();
        applied.feed(&diff);
        assert_eq!(applied.cursor(), target.cursor());
    }

    #[test]
    fn hiding_and_showing_the_cursor_is_carried() {
        let previous = screen(4, 10, b"x");
        let mut hidden = previous.clone();
        hidden.feed(b"\x1b[?25l");
        assert!(!hidden.cursor_visible());

        let mut applied = previous.clone();
        applied.feed(&hidden.diff_from(&previous));
        assert!(!applied.cursor_visible(), "the hide never travelled");

        // And back, which is the direction a copy that forgot the mode
        // would get wrong on every frame.
        let mut applied_back = hidden.clone();
        applied_back.feed(&previous.diff_from(&hidden));
        assert!(applied_back.cursor_visible(), "the show never travelled");
    }

    #[test]
    fn a_clone_carries_the_hidden_cursor() {
        let mut s = screen(4, 10, b"x");
        s.feed(b"\x1b[?25l");
        assert!(!s.clone().cursor_visible(), "a copy reported it shown");
    }

    #[test]
    fn a_reshaped_screen_is_repainted_rather_than_diffed() {
        let previous = screen(4, 10, b"before");
        let mut target = AlacrittyScreen::new(6, 30, false);
        target.feed(b"after");
        let diff = target.diff_from(&previous);
        // A repaint clears first, because nothing outside what it
        // writes can be assumed.
        assert!(
            diff.windows(4).any(|w| w == b"\x1b[2J"),
            "a reshape has to clear: {:?}",
            String::from_utf8_lossy(&diff)
        );
    }

    #[test]
    fn a_repaint_stands_on_its_own() {
        let target = screen(4, 12, b"\x1b[1;32mgreen\x1b[m\r\nsecond");
        // Applied to a screen holding something else entirely, with no
        // relationship to it: that is what a repaint is for.
        let mut applied = screen(4, 12, b"junk\r\nmore junk\r\nand more");
        applied.feed(&target.repaint());
        assert_eq!(applied.text(), target.text());
        assert_eq!(applied.cursor(), target.cursor());
    }

    #[test]
    fn the_title_is_read_and_carried_through_a_copy() {
        let s = screen(3, 10, b"\x1b]0;a window\x07hi");
        assert_eq!(s.title().as_deref(), Some("a window"));
        assert_eq!(s.clone().title().as_deref(), Some("a window"));
    }
}
