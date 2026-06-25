use super::*;

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

    /// Deadline of a buffering DEC `?2026` synchronized update, if any.
    /// See `TerminalBackend::sync_timeout`.
    pub fn sync_timeout(&self) -> Option<std::time::Instant> {
        self.backend.sync_timeout()
    }

    /// Force-apply a stalled synchronized update to the grid.
    /// See `TerminalBackend::flush_sync`.
    pub fn flush_sync(&mut self) {
        self.backend.flush_sync();
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
            // Clamp to the last valid column: `pixel_to_cell` floors the
            // column low but not high, so a drag into the right padding can
            // push `end.0`/`start.0` to `cols`, which would panic on the
            // `row[Column(..)]` index below (the block branch above already
            // clamps with `c1.min(last_col)`).
            let (start_col, end_col) = (start_col.min(last_col), end_col.min(last_col));
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
