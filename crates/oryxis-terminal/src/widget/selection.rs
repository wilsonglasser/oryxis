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
pub(crate) enum SelectGranularity {
    Word,
    Line,
    Paragraph,
}

/// Next click count given the previous count and whether this press is
/// consecutive (within the time + distance window of the last one). Cycles
/// 1->2->3->4->1 so a fifth rapid click restarts at single-click, matching
/// the iTerm / GNOME behaviour. Pure so the classification can be tested
/// without driving real mouse timing.
pub(crate) fn next_click_count(prev: Option<u8>, consecutive: bool) -> u8 {
    match prev {
        Some(c) if consecutive => (c % 4) + 1,
        _ => 1,
    }
}

/// Combine two selections into the range spanning from the earliest
/// ordered start to the latest ordered end. Used to extend a word/line
/// drag: the anchor's word/line unioned with the cursor's word/line.
pub(crate) fn union_selection(a: Selection, b: Selection) -> Selection {
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

#[cfg(test)]
mod selection_tests {
    use super::Selection;
    use crate::widget::TerminalState;

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
    fn flowing_selection_skips_wide_char_spacers() {
        // Each CJK glyph occupies two grid cells: the leading cell holds the
        // character, the trailing cell is a WIDE_CHAR_SPACER whose `c` is a
        // space. Copying must emit the glyph once, not "glyph + space".
        // (GitHub issue #51: extra spaces in the middle of copied content.)
        let state = state_with(&["专用发票"]);
        // 4 glyphs span columns 0..=7 (two cells each).
        let sel = Selection { start: (0, 0), end: (7, 0), block: false };
        assert_eq!(state.get_selection_text(&sel), "专用发票");
    }

    #[test]
    fn block_selection_skips_wide_char_spacers() {
        // The block branch must drop spacer cells too.
        let state = state_with(&["专用发票"]);
        let sel = Selection { start: (0, 0), end: (7, 0), block: true };
        assert_eq!(state.get_selection_text(&sel), "专用发票");
    }

    #[test]
    fn smart_select_grabs_whole_url() {
        // A click anywhere inside the URL returns the full token span,
        // even though "/" and ":" are word delimiters that would split it.
        let state = state_with(&["see https://example.com x"]);
        let (c0, c1) =
            crate::widget::smart_span_at(&state.backend.term, &state.palette, 0, 8)
                .expect("URL should be detected");
        let sel = Selection { start: (c0, 0), end: (c1, 0), block: false };
        assert_eq!(state.get_selection_text(&sel), "https://example.com");
    }

    #[test]
    fn smart_select_misses_plain_word() {
        // A click on a non-token word returns None so the caller falls
        // back to delimiter-word selection.
        let state = state_with(&["see https://example.com x"]);
        assert!(crate::widget::smart_span_at(&state.backend.term, &state.palette, 0, 1).is_none());
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
        let view = crate::widget::TerminalView::<()>::new(Arc::clone(&arc));
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
