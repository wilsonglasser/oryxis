use super::*;

/// The OSC 8 hyperlink the pointer is currently over, surfaced to the app so
/// it can render a target-reveal chip (anti-spoofing: the visible label of an
/// OSC 8 link need not match its target). `allowed` is the scheme-allowlist
/// verdict (see `highlight::osc8_scheme_allowed`): when `false` the app shows
/// a "link type not allowed" chip instead of the target, and the widget
/// suppresses the pointer / underline / open affordance entirely. Written by
/// the widget under the render lock at hover time; read by the app in `view()`
/// via a non-blocking `try_lock`.
#[derive(Clone, Debug, PartialEq)]
pub struct HoveredLink {
    pub target: String,
    pub allowed: bool,
}

pub struct TerminalState {
    pub backend: TerminalBackend,
    pub pty: Option<PtyHandle>,
    pub palette: TerminalPalette,
    /// When this state is attached to an SSH session, resize events are
    /// forwarded here so the remote shell sees `window-change` and apps
    /// like `top`/`vim` re-layout instead of wrapping into our local grid.
    remote_resize_tx: Option<mpsc::UnboundedSender<(u16, u16)>>,
    /// Monotonic revision of anything that changes what the terminal would
    /// render (PTY output applied, synchronized-update flush, palette
    /// swap). The canvas widget folds this into its `RenderKey` so a draw
    /// triggered by unrelated UI churn (a hover elsewhere, a tab-title
    /// update, a toast) hits the geometry cache instead of re-tessellating
    /// the whole grid. Resizes are intentionally NOT counted here: a grid
    /// resize only happens on a bounds or font change, both of which the
    /// canvas cache already invalidates on directly.
    render_epoch: u64,
    /// Scrollback search (C1). `Some` while the find bar is open over
    /// this pane; `None` otherwise. Held here so the draw pass can read
    /// the match highlights under the same lock, and so a step / rebuild
    /// survives across frames.
    pub search: Option<crate::widget::search::BufferSearch>,
    /// A scroll-back offset the widget should snap to on the next draw
    /// (C1: center the active search match). `Cell` so the immutable
    /// draw pass can consume it, mirroring `reset_scroll_on_output`.
    pub pending_scroll: std::cell::Cell<Option<i32>>,
    /// The scrollback offset of the viewport most recently drawn by the
    /// widget. Kept on the terminal state so app-level actions can export
    /// exactly what the user is looking at, rather than the whole buffer.
    /// The widget owns the live value; this is a copy, only as fresh as the
    /// pane's last frame, which is why the menu entry reading it is offered
    /// only while the pane is on screen.
    viewport_scroll_offset: i32,
    /// The OSC 8 hyperlink under the pointer (C3), for the app's reveal
    /// chip. `None` when the pointer is over no explicit link. Updated by
    /// the widget's hover handler under the render lock.
    pub hovered_link: Option<HoveredLink>,
    /// IME preedit (composition) text for this pane, e.g. the pinyin
    /// syllables while a CJK input method is composing. The app stores it
    /// from `InputMethod::Preedit` events; the `ime_host` widget reports it
    /// back to the iced runtime so the over-the-spot overlay can draw it at
    /// the caret. Empty string means "no active composition".
    preedit: String,
}

impl TerminalState {
    pub fn new(
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
    ) -> TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        Self::new_with_env(cols, rows, cwd, &[])
    }

    /// `new` with extra environment variables for the child process
    /// (a saved local host's `env_vars`).
    pub fn new_with_env(
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        let backend = TerminalBackend::new(cols, rows);
        let (pty, rx) =
            PtyHandle::spawn_command(cols, rows, None, &[], cwd, env, &backend.event_proxy)?;
        let palette = TerminalPalette::default();
        Ok((Self { backend, pty: Some(pty), palette, remote_resize_tx: None, render_epoch: 0, search: None, pending_scroll: std::cell::Cell::new(None), viewport_scroll_offset: 0, hovered_link: None, preedit: String::new() }, rx))
    }

    /// Like `new` but spawns an explicit program (e.g. PowerShell or
    /// `wsl.exe -d Ubuntu`) instead of the OS default shell. Used by
    /// the Local Shell picker on Windows.
    pub fn new_with_command(
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        Self::new_with_command_env(cols, rows, program, args, cwd, &[])
    }

    /// `new_with_command` with extra environment variables for the
    /// child process (a saved local host's `env_vars`).
    pub fn new_with_command_env(
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> TerminalResult<(Self, mpsc::UnboundedReceiver<Vec<u8>>)>
    {
        let backend = TerminalBackend::new(cols, rows);
        let (pty, rx) = PtyHandle::spawn_command(
            cols, rows, Some(program), args, cwd, env, &backend.event_proxy,
        )?;
        let palette = TerminalPalette::default();
        Ok((Self { backend, pty: Some(pty), palette, remote_resize_tx: None, render_epoch: 0, search: None, pending_scroll: std::cell::Cell::new(None), viewport_scroll_offset: 0, hovered_link: None, preedit: String::new() }, rx))
    }

    pub fn new_no_pty(
        cols: u16,
        rows: u16,
    ) -> TerminalResult<Self> {
        let backend = TerminalBackend::new(cols, rows);
        let palette = TerminalPalette::default();
        Ok(Self { backend, pty: None, palette, remote_resize_tx: None, render_epoch: 0, search: None, pending_scroll: std::cell::Cell::new(None), viewport_scroll_offset: 0, hovered_link: None, preedit: String::new() })
    }

    /// A PTY-less state with an explicit scrollback budget, for the
    /// session-log transcript viewer: the whole recording is fed at once
    /// (no clock), so the history must hold every line the session
    /// scrolled past, not just the user's live `scrollback_rows`.
    pub fn new_no_pty_with_scrollback(
        cols: u16,
        rows: u16,
        scrollback: usize,
    ) -> TerminalResult<Self> {
        let backend = TerminalBackend::new_with_scrollback(cols, rows, scrollback);
        let palette = TerminalPalette::default();
        Ok(Self { backend, pty: None, palette, remote_resize_tx: None, render_epoch: 0, search: None, pending_scroll: std::cell::Cell::new(None), viewport_scroll_offset: 0, hovered_link: None, preedit: String::new() })
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

    /// Wire the emulator's query-reply back-channel to a remote session's
    /// input, called from the app alongside `set_remote_resize_sender`.
    /// The emulator answers in-band queries (DSR `\x1b[6n` cursor position,
    /// DA `\x1b[c`, DECRQM `\x1b[?..$p`, ...) by emitting `Event::PtyWrite`;
    /// local PTYs wire the same slot in `PtyHandle::spawn_command`. Remote
    /// programs (docker compose's raw-mode `[y/N]` prompt) block on these
    /// replies, so dropping them freezes the session for the user: raw mode
    /// means no echo and no Ctrl+C, and the blocked program prints nothing.
    pub fn set_remote_reply_sender(
        &mut self,
        tx: mpsc::UnboundedSender<Vec<u8>>,
    ) {
        self.backend.event_proxy.set_pty_write_tx(tx);
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.backend.process(bytes);
        // A batch reached the emulator: even a pure cursor move or a
        // query-only sequence can change what a frame would draw, so bump
        // unconditionally. The cost of an occasional needless rebuild is
        // negligible next to the win of skipping the rebuild entirely when
        // no output arrived at all.
        self.render_epoch = self.render_epoch.wrapping_add(1);
    }

    /// Current render revision. See [`TerminalState::render_epoch`] (field).
    pub fn render_epoch(&self) -> u64 {
        self.render_epoch
    }

    // ── Scrollback search (C1) ──

    /// Open the find bar over this pane (idempotent). Returns the search
    /// generation so the caller can invalidate any cached frame.
    pub fn search_open(&mut self) {
        if self.search.is_none() {
            self.search = Some(crate::widget::search::BufferSearch::default());
        }
    }

    /// Close the find bar and drop the match set.
    pub fn search_close(&mut self) {
        self.search = None;
    }

    /// Whether the find bar is open over this pane.
    pub fn search_active(&self) -> bool {
        self.search.is_some()
    }

    /// Set the find needle and rebuild matches. Auto-scrolls the active
    /// match into view. No-op when the bar isn't open.
    pub fn search_set_query(&mut self, query: &str) {
        let epoch = self.render_epoch;
        if let Some(search) = self.search.as_mut() {
            search.set_query(query, &self.backend.term, epoch);
            let target = search.active_match();
            self.queue_scroll_to_match(target);
        }
    }

    /// Step the active match forward / backward, scrolling it into view.
    pub fn search_step(&mut self, forward: bool) {
        // Re-scan first if new output landed since the last build, so
        // stepping never lands on a stale coordinate.
        let epoch = self.render_epoch;
        if let Some(search) = self.search.as_mut()
            && search.scanned_epoch != epoch
        {
            search.rebuild(&self.backend.term, epoch);
        }
        let target = self
            .search
            .as_mut()
            .and_then(|s| s.step(forward));
        self.queue_scroll_to_match(target);
    }

    /// `(current, total)` for the count label (`current` is 1-based, 0
    /// when there are no matches). `None` when the bar isn't open.
    pub fn search_count(&self) -> Option<(usize, usize)> {
        self.search.as_ref().map(|s| {
            let total = s.matches.len();
            let current = if total == 0 { 0 } else { s.active + 1 };
            (current, total)
        })
    }

    /// The search generation (bumped on every query change / step), for
    /// the widget's `RenderKey`. 0 when the bar isn't open.
    pub fn search_generation(&self) -> u64 {
        self.search.as_ref().map(|s| s.generation).unwrap_or(0)
    }

    /// Translate a match's start line into a scroll-back offset that
    /// centers it in the viewport, and queue it for the next draw. A
    /// match already on the visible screen (line >= 0) with the viewport
    /// at the bottom needs no scroll.
    fn queue_scroll_to_match(&self, m: Option<crate::widget::search::SearchMatch>) {
        let Some(m) = m else { return };
        let rows = self.backend.rows() as i32;
        // The draw maps grid line ← visible_row: `line = visible_row -
        // scroll_offset`, so a cell at grid line L shows at
        // `visible_row = L + scroll_offset`. To land the match's line near
        // the middle row we solve `rows/2 = L + scroll_offset`, i.e.
        // `scroll_offset = rows/2 - L` (L is negative in scrollback, so this
        // is a positive scroll-up). Clamped ≥ 0; a match already on the
        // visible screen with the viewport at the bottom needs no scroll.
        let desired = rows / 2 - m.start_line;
        self.pending_scroll.set(Some(desired.max(0)));
    }

    /// Swap the palette and bump the render epoch so the canvas cache
    /// re-tessellates with the new colors. Callers that mutate `palette`
    /// through this method (theme switch, per-connection palette resolve)
    /// keep the cache correct; a raw field assignment would leave a stale
    /// cached frame in the old theme.
    pub fn set_palette(&mut self, palette: TerminalPalette) {
        self.palette = palette;
        self.render_epoch = self.render_epoch.wrapping_add(1);
    }

    /// Per-instance OSC 52 clipboard overrides (C5 per-host quirk). `None`
    /// inherits the global policy for that direction; `Some(bool)` forces
    /// it. Read is only ever forced off per-host.
    pub fn set_osc52_override(&self, write: Option<bool>, read: Option<bool>) {
        self.backend.event_proxy.set_osc52_override(write, read);
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
        // Buffered bytes from a stalled synchronized update were just
        // applied to the grid, so the next frame's content differs.
        self.render_epoch = self.render_epoch.wrapping_add(1);
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

    /// Take the pending window title set by the shell via OSC 0/2, draining
    /// the slot so each change is reported exactly once. An OSC ResetTitle
    /// surfaces as `Some("")` so the caller can fall back to its default
    /// label; `None` means nothing changed since the last call.
    pub fn take_title(&self) -> Option<String> {
        self.backend
            .event_proxy
            .title
            .lock()
            .ok()
            .and_then(|mut t| t.take())
    }

    /// Drain the pending bell flag, returning true at most once per ring.
    /// The app maps a true to the user's chosen bell action.
    pub fn take_bell(&self) -> bool {
        self.backend
            .event_proxy
            .bell
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    /// Drain the latest working directory the shell reported via OSC 7.
    pub fn take_cwd(&mut self) -> Option<String> {
        self.backend.osc.take_cwd()
    }

    /// Drain the latest OSC 9 notification text, if any.
    pub fn take_notification(&mut self) -> Option<String> {
        self.backend.osc.take_notification()
    }

    /// Current OSC 9;4 progress report, if the app set one.
    pub fn progress(&self) -> Option<crate::osc::Progress> {
        self.backend.osc.progress()
    }

    /// Drain the OSC 133 shell-integration marks captured since the last
    /// call, each stamped with the cursor position at emission time.
    pub fn take_shell_marks(&mut self) -> Vec<crate::osc::PositionedShellMark> {
        self.backend.take_marks()
    }

    /// Drain the command lines reported by `OSC 633 ; E` since the last
    /// call, keyed by the id their [`crate::osc::ShellMark::CommandLine`]
    /// marks carry. Drain it in the same breath as
    /// [`Self::take_shell_marks`], so a batch's marks and texts resolve
    /// against each other.
    pub fn take_shell_command_lines(&mut self) -> Vec<(u32, String)> {
        self.backend.osc.take_command_lines()
    }

    /// Install the compiled highlight rules this pane matches against.
    /// The widget paints from the same set, so a rule's colour and its
    /// action always refer to the same pattern.
    pub fn set_highlight_rules(
        &mut self,
        rules: std::sync::Arc<crate::highlight_rules::CompiledRules>,
    ) {
        self.backend.set_highlight_rules(rules);
    }

    /// Whether this pane measures Unicode "Ambiguous" width characters as
    /// two cells. Set from the host's setting on every output batch; see
    /// [`crate::backend::TerminalBackend::set_ambiguous_width_wide`].
    pub fn set_ambiguous_width_wide(&mut self, wide: bool) {
        self.backend.set_ambiguous_width_wide(wide);
    }

    /// Drain the highlight rules that fired on this pane's output since
    /// the last call.
    pub fn take_trigger_hits(&mut self) -> Vec<crate::trigger::TriggerHit> {
        self.backend.take_trigger_hits()
    }

    /// Require `nonce` on every `OSC 633 ; E` this pane accepts (the value
    /// baked into the shell-integration snippet installed on the host).
    pub fn set_shell_command_nonce(&mut self, nonce: Option<String>) {
        self.backend.osc.set_command_nonce(nonce);
    }

    /// True while the alternate screen buffer is active (vim, htop, less...).
    /// The command-history capture ignores everything typed there.
    pub fn is_alt_screen(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        self.backend.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// The password prompt printed in front of the cursor, if any
    /// (issue #117). See [`crate::backend::TerminalBackend::password_prompt_at_cursor`].
    pub fn password_prompt_at_cursor(&self) -> Option<crate::prompt_detect::PasswordPrompt> {
        self.backend.password_prompt_at_cursor()
    }

    /// Text of the logical (wrap-joined) line the cursor sits on, from
    /// column 0 of its first physical row (prompt included). Used by the
    /// command-history capture's heuristic path on hosts without OSC 133.
    /// Returns `None` on the alternate screen.
    pub fn cursor_logical_line(&self) -> Option<String> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
        if self.is_alt_screen() {
            return None;
        }
        let grid = self.backend.term.grid();
        let cols = grid.columns();
        let topmost = grid.topmost_line().0;
        let cursor_line = grid.cursor.point.line.0;

        // Walk up to the first row of the wrapped chain. Bounded so a
        // pathological full-width wrap chain can't stall the UI thread;
        // 64 rows of a wide terminal is far beyond any real command line.
        let mut first = cursor_line;
        let mut walked = 0;
        while first > topmost && walked < 64 {
            let prev = &grid[Line(first - 1)];
            if !prev[Column(cols - 1)].flags.contains(CellFlags::WRAPLINE) {
                break;
            }
            first -= 1;
            walked += 1;
        }
        Some(self.read_logical_line(first, 0))
    }

    /// Text of the logical (wrap-joined) line starting at physical row
    /// `abs_line` (absolute index: `history_size + visible line`, the
    /// coordinate space of [`crate::osc::PositionedShellMark`]) column
    /// `start_col`. Returns `None` on the alternate screen or when the row
    /// has left the addressable grid (scrollback ring saturated and rotated
    /// past it), so a stale mark can never read unrelated rows. This is how
    /// the capture reads the command the shell echoed after its OSC 133
    /// `PromptEnd` mark.
    pub fn logical_line_from_abs(&self, abs_line: i64, start_col: u16) -> Option<String> {
        use alacritty_terminal::grid::Dimensions;
        if self.is_alt_screen() {
            return None;
        }
        let grid = self.backend.term.grid();
        let rel = abs_line - grid.history_size() as i64;
        if rel < i64::from(grid.topmost_line().0) || rel > i64::from(grid.bottommost_line().0) {
            return None;
        }
        Some(self.read_logical_line(rel as i32, start_col as usize))
    }

    /// Join the soft-wrapped chain that starts at physical row `first`
    /// (grid-relative), reading from `start_col` on that first row and from
    /// column 0 on continuations. Wide-char spacers are skipped, trailing
    /// whitespace trimmed. When the result is a single physical row, a run
    /// of 8+ interior spaces truncates it: that gap is a zsh RPROMPT sitting
    /// on the right edge of the prompt row, not command text (a real command
    /// with 8 literal spaces inside is vanishingly rare next to how common
    /// right prompts are).
    fn read_logical_line(&self, first: i32, start_col: usize) -> String {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
        let grid = self.backend.term.grid();
        let cols = grid.columns();
        let mut text = String::new();
        let mut line = first;
        let mut rows = 0;
        loop {
            let row = &grid[Line(line)];
            let from = if line == first { start_col.min(cols) } else { 0 };
            for c in from..cols {
                let cell = &row[Column(c)];
                if cell.c != '\0' && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    text.push(cell.c);
                }
            }
            rows += 1;
            if !row[Column(cols - 1)].flags.contains(CellFlags::WRAPLINE)
                || line >= grid.bottommost_line().0
                || rows >= 64
            {
                break;
            }
            line += 1;
        }
        let mut text = text.trim_end().to_string();
        if rows == 1
            && let Some(gap) = text.find("        ")
        {
            text.truncate(gap);
        }
        text
    }

    /// True when the focused application has enabled application cursor keys
    /// mode (DECCKM, `ESC [ ? 1 h`, emitted by the terminfo `smkx`
    /// capability). In this mode the arrow and Home/End keys must be sent in
    /// their SS3 form (`ESC O A` …) instead of the default CSI form
    /// (`ESC [ A` …), which is what every full-screen TUI binds its
    /// navigation to (mc, vim, less, …). Tracked by the backend over both
    /// local PTY and SSH because remote output flows through the same
    /// `process()` into the term.
    pub fn application_cursor_keys(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        self.backend.term.mode().contains(TermMode::APP_CURSOR)
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
        // A resize reflows the grid, so search matches at old coordinates
        // are stale: rebuild them against the new layout (C1).
        let epoch = self.render_epoch;
        if let Some(search) = self.search.as_mut() {
            search.rebuild(&self.backend.term, epoch);
        }
        true
    }

    pub fn cols(&self) -> u16 { self.backend.cols() }
    pub fn rows(&self) -> u16 { self.backend.rows() }

    /// The whole buffer (scrollback + screen) as text, trailing blank
    /// lines trimmed. Backs the "Copy All" context-menu action, which is
    /// app-driven and so can't reach the widget's live selection state.
    /// Reuses the selection extractor over a full-buffer range.
    pub fn all_text(&self) -> String {
        use alacritty_terminal::grid::Dimensions;
        let grid = self.backend.term.grid();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let last_col = grid.columns().saturating_sub(1) as u16;
        let sel = Selection {
            start: (0, top),
            end: (last_col, bottom),
            block: false,
        };
        self.get_selection_text(&sel).trim_end().to_string()
    }

    /// Text in the viewport currently shown by the terminal widget, with no
    /// rows outside it from scrollback. This is intentionally distinct from
    /// [`Self::all_text`], which includes the whole buffer.
    ///
    /// The offset is re-clamped here with the draw's own bound rather than
    /// trusted as stored: a resize or a `clear_scrollback` between frames
    /// shrinks the history the value was measured against, and a range past
    /// the top of the grid would silently read as a shorter screen.
    pub fn visible_text(&self) -> String {
        use alacritty_terminal::grid::Dimensions;
        let grid = self.backend.term.grid();
        let last_col = grid.columns().saturating_sub(1) as u16;
        let last_visible_line = grid.screen_lines().saturating_sub(1) as i32;
        let max_scroll = grid.total_lines().saturating_sub(grid.screen_lines()) as i32;
        let offset = self.viewport_scroll_offset.clamp(0, max_scroll);
        let selection = Selection {
            start: (0, -offset),
            end: (last_col, last_visible_line - offset),
            block: false,
        };
        self.get_selection_text(&selection).trim_end().to_string()
    }

    /// Store the viewport offset resolved by the widget for the frame it is
    /// drawing, which is what [`Self::visible_text`] reads back.
    pub(crate) fn set_viewport_scroll_offset(&mut self, offset: i32) {
        self.viewport_scroll_offset = offset;
    }

    /// Drop the scrollback history, keeping the visible screen (the PuTTY
    /// / Windows Terminal "Clear Scrollback" action). No-op when there is
    /// no history.
    pub fn clear_scrollback(&mut self) {
        self.backend.term.grid_mut().clear_history();
    }

    /// Serialize the visible screen (the active grid's viewport, no
    /// scrollback) back into SGR-styled bytes that reproduce it when fed
    /// to a fresh emulator: text, palette / indexed / RGB colors and the
    /// visual attribute flags all round-trip. The session-log viewer uses
    /// this to materialize the final alternate-screen frame (top / vim /
    /// less left open at disconnect) into the primary buffer before
    /// leaving the alt screen, so the frame survives as part of the
    /// transcript instead of vanishing with the alt buffer (#91).
    ///
    /// Lines are emitted as hard `\r\n` rows (a frame is a visual
    /// snapshot, so the re-fed copy may re-wrap if the grid narrows) and
    /// trailing blank cells with no visible styling are trimmed. The
    /// underline variants keep their exact style (`4:2`..`4:5` colon
    /// subparameters) and color (SGR 58): the only consumer is our own
    /// vte parser, which round-trips them all.
    pub fn screen_as_ansi(&self) -> Vec<u8> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
        use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

        // Style bits a re-fed emulator can reproduce; grid bookkeeping
        // (wrap markers, wide-char spacers) is deliberately excluded.
        const STYLE: CellFlags = CellFlags::INVERSE
            .union(CellFlags::BOLD)
            .union(CellFlags::ITALIC)
            .union(CellFlags::DIM)
            .union(CellFlags::HIDDEN)
            .union(CellFlags::STRIKEOUT)
            .union(CellFlags::ALL_UNDERLINES);

        // SGR parameter selecting `color` ("31", "48;5;n", "38;2;r;g;b"),
        // or `None` for the default fore/background (the leading reset
        // already restores those).
        fn color_params(color: AnsiColor, bg: bool) -> Option<String> {
            match color {
                AnsiColor::Named(n) => {
                    // The dim named slots reduce to their base color; the
                    // DIM flag travels separately.
                    let idx = match n {
                        NamedColor::DimBlack => 0,
                        NamedColor::DimRed => 1,
                        NamedColor::DimGreen => 2,
                        NamedColor::DimYellow => 3,
                        NamedColor::DimBlue => 4,
                        NamedColor::DimMagenta => 5,
                        NamedColor::DimCyan => 6,
                        NamedColor::DimWhite => 7,
                        _ => n as usize,
                    };
                    match idx {
                        0..=7 => Some((if bg { 40 } else { 30 } + idx).to_string()),
                        // Bright 8..=15 map to the aixterm 90/100 range.
                        8..=15 => Some((if bg { 92 } else { 82 } + idx).to_string()),
                        _ => None,
                    }
                }
                AnsiColor::Indexed(i) => {
                    Some(format!("{};5;{i}", if bg { 48 } else { 38 }))
                }
                AnsiColor::Spec(rgb) => Some(format!(
                    "{};2;{};{};{}",
                    if bg { 48 } else { 38 },
                    rgb.r,
                    rgb.g,
                    rgb.b
                )),
            }
        }

        // Full SGR for a style run: reset, then every attribute, so runs
        // never inherit stale state from their predecessor.
        fn sgr(
            fg: AnsiColor,
            bg: AnsiColor,
            flags: CellFlags,
            underline_color: Option<AnsiColor>,
        ) -> String {
            let mut s = String::from("\x1b[0");
            if flags.contains(CellFlags::BOLD) {
                s.push_str(";1");
            }
            if flags.contains(CellFlags::DIM) {
                s.push_str(";2");
            }
            if flags.contains(CellFlags::ITALIC) {
                s.push_str(";3");
            }
            // Each underline variant keeps its exact style; the parser
            // stores at most one per cell, so the chain is exclusive.
            if flags.contains(CellFlags::UNDERLINE) {
                s.push_str(";4");
            } else if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
                s.push_str(";4:2");
            } else if flags.contains(CellFlags::UNDERCURL) {
                s.push_str(";4:3");
            } else if flags.contains(CellFlags::DOTTED_UNDERLINE) {
                s.push_str(";4:4");
            } else if flags.contains(CellFlags::DASHED_UNDERLINE) {
                s.push_str(";4:5");
            }
            if flags.contains(CellFlags::INVERSE) {
                s.push_str(";7");
            }
            if flags.contains(CellFlags::HIDDEN) {
                s.push_str(";8");
            }
            if flags.contains(CellFlags::STRIKEOUT) {
                s.push_str(";9");
            }
            if let Some(p) = color_params(fg, false) {
                s.push(';');
                s.push_str(&p);
            }
            if let Some(p) = color_params(bg, true) {
                s.push(';');
                s.push_str(&p);
            }
            // Underline color (SGR 58). Only the indexed / RGB forms
            // exist on the wire; a named color cannot be expressed, so
            // it falls back to the default (the leading reset).
            match underline_color {
                Some(AnsiColor::Indexed(i)) => {
                    s.push_str(&format!(";58;5;{i}"));
                }
                Some(AnsiColor::Spec(rgb)) => {
                    s.push_str(&format!(";58;2;{};{};{}", rgb.r, rgb.g, rgb.b));
                }
                Some(AnsiColor::Named(_)) | None => {}
            }
            s.push('m');
            s
        }

        let grid = self.backend.term.grid();
        let rows = grid.screen_lines();
        let cols = grid.columns();
        let mut lines_out: Vec<Vec<u8>> = Vec::with_capacity(rows);
        for r in 0..rows {
            let row = &grid[Line(r as i32)];
            // Last column worth emitting: trailing blanks are dropped
            // unless something makes them visible (a colored background
            // bar, inverse video, an underline / strikeout run).
            let mut last: Option<usize> = None;
            for c in (0..cols).rev() {
                let cell = &row[Column(c)];
                let invisible = (cell.c == ' ' || cell.c == '\0')
                    && cell.zerowidth().is_none()
                    && cell.bg == AnsiColor::Named(NamedColor::Background)
                    && !cell.flags.intersects(
                        CellFlags::INVERSE
                            | CellFlags::STRIKEOUT
                            | CellFlags::ALL_UNDERLINES,
                    );
                if !invisible {
                    last = Some(c);
                    break;
                }
            }
            let mut out: Vec<u8> = Vec::new();
            let mut style: Option<(AnsiColor, AnsiColor, CellFlags, Option<AnsiColor>)> =
                None;
            if let Some(last) = last {
                let mut buf = [0u8; 4];
                for c in 0..=last {
                    let cell = &row[Column(c)];
                    // The wide-char spacers are grid bookkeeping: the wide
                    // glyph itself already advances two columns when
                    // re-fed, so emitting them would shift the row.
                    if cell.flags.intersects(
                        CellFlags::WIDE_CHAR_SPACER
                            | CellFlags::LEADING_WIDE_CHAR_SPACER,
                    ) {
                        continue;
                    }
                    let cur =
                        (cell.fg, cell.bg, cell.flags & STYLE, cell.underline_color());
                    if style != Some(cur) {
                        out.extend_from_slice(sgr(cur.0, cur.1, cur.2, cur.3).as_bytes());
                        style = Some(cur);
                    }
                    let ch = if cell.c == '\0' { ' ' } else { cell.c };
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    if let Some(zw) = cell.zerowidth() {
                        for z in zw {
                            out.extend_from_slice(z.encode_utf8(&mut buf).as_bytes());
                        }
                    }
                }
            }
            lines_out.push(out);
        }
        while lines_out.last().is_some_and(|l| l.is_empty()) {
            lines_out.pop();
        }
        let mut bytes = Vec::new();
        for (i, l) in lines_out.iter().enumerate() {
            if i > 0 {
                bytes.extend_from_slice(b"\r\n");
            }
            bytes.extend_from_slice(l);
            // Reset before the newline so a colored background can't
            // bleed past the end of its row.
            if !l.is_empty() {
                bytes.extend_from_slice(b"\x1b[0m");
            }
        }
        bytes
    }

    /// Visible cursor cell as `(column, line)`, 0-based from the top-left of
    /// the active screen. Used to anchor the OS IME candidate window near the
    /// caret. Ignores the widget's scrollback offset (during composition the
    /// view sits at the bottom), so it is exact while typing and only
    /// approximate if the user has scrolled into history.
    pub fn cursor_cell(&self) -> (u16, u16) {
        let p = self.backend.term.renderable_content().cursor.point;
        (p.column.0 as u16, p.line.0.max(0) as u16)
    }

    /// Store the OS IME preedit (composition) text, e.g. pinyin syllables
    /// while a CJK method is composing. Empty means "no active composition".
    /// Drawn inline on the grid by the widget (terminal font, at the
    /// caret); the `ime_host` decorator only anchors the OS candidate
    /// window. Bumps the render epoch so the canvas geometry cache
    /// re-tessellates with the new composition (it is part of what a
    /// frame draws).
    pub fn set_preedit(&mut self, text: String) {
        self.preedit = text;
        self.render_epoch = self.render_epoch.wrapping_add(1);
    }

    /// The current IME preedit text (empty when idle).
    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// Extract text from a selection range.
    pub fn get_selection_text(&self, sel: &Selection) -> String {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
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
                    // The trailing cell of a wide (CJK) glyph is a spacer
                    // whose `c` is a space; skip it so a double-width char
                    // doesn't copy out as "char + space".
                    if cell.c != '\0' && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
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
        let mut text = String::new();
        let mut prev_line: Option<i32> = None;
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
                // Skip wide-char spacer cells: the trailing half of a CJK
                // glyph (WIDE_CHAR_SPACER), and the ghost cell left at the
                // end of a row when a wide char doesn't fit and wraps whole
                // (LEADING_WIDE_CHAR_SPACER). Either would copy out as a
                // stray space; the leading one matters now that wrapped rows
                // keep their tail untrimmed below.
                if cell.c != '\0'
                    && !cell.flags.intersects(
                        CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER,
                    )
                {
                    line_str.push(cell.c);
                }
            }
            // A row whose predecessor ended in a soft wrap (WRAPLINE on its
            // last cell) is the continuation of one logical line, so it joins
            // WITHOUT a newline: the tmux / xterm behaviour that lets a
            // wrapped long URL copy out intact. Real line breaks still get
            // `\n` between the physical rows.
            if let Some(prev) = prev_line {
                let wrapped = grid[Line(prev)][Column(last_col as usize)]
                    .flags
                    .contains(CellFlags::WRAPLINE);
                if !wrapped {
                    text.push('\n');
                }
            }
            prev_line = Some(line_idx);
            // A soft-wrapped row is full to the margin, so its trailing cells
            // are interior content of the logical line (spaces straddling the
            // wrap point are real). Never trim it, mirroring alacritty's
            // `Row::line_length`, which reports the full width under
            // WRAPLINE; only rows that end a logical line get trimmed.
            let row_wrapped = row[Column(last_col as usize)]
                .flags
                .contains(CellFlags::WRAPLINE);
            if row_wrapped {
                text.push_str(&line_str);
            } else {
                text.push_str(line_str.trim_end());
            }
        }

        text
    }

    /// Last `n_lines` rows of the terminal buffer as text, **including
    /// scrollback history** (not just the visible viewport). Each grid row is
    /// one line; wide-char spacer cells are dropped and trailing whitespace is
    /// trimmed, and the blank rows below the last output are skipped so the
    /// tail ends on real content. Internal blank lines (e.g. between blocks of
    /// output) are preserved. Used to feed recent terminal output to the AI
    /// assistant, which previously saw only the on-screen rows and silently
    /// lost anything that had scrolled off.
    pub fn tail_text(&self, n_lines: usize) -> Vec<String> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
        if n_lines == 0 {
            return Vec::new();
        }
        let grid = self.backend.term.grid();
        let cols = grid.columns();
        let top = grid.topmost_line().0;
        let bot = grid.bottommost_line().0;
        let line_text = |li: i32| -> String {
            let row = &grid[Line(li)];
            let mut s = String::new();
            for c in 0..cols {
                let cell = &row[Column(c)];
                // Skip wide-char spacer cells (the trailing half of a CJK
                // glyph); otherwise each copies out as an extra space.
                if cell.c != '\0' && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    s.push(cell.c);
                }
            }
            s.trim_end().to_string()
        };
        // Skip the blank rows below the last real output so the tail ends on
        // content, then take the last `n_lines` rows ending there (reaching
        // up into history when the viewport doesn't hold that many).
        let mut end = bot;
        while end > top && line_text(end).is_empty() {
            end -= 1;
        }
        let start = (end - (n_lines as i32 - 1)).max(top);
        (start..=end).map(line_text).collect()
    }

    /// Absolute row (`history_size + visible line`, the coordinate space of
    /// [`crate::osc::PositionedShellMark`]) of the FIRST physical row of the
    /// logical line the cursor sits on. `None` on the alternate screen, where
    /// rows are repainted in place and an anchor into them means nothing.
    ///
    /// Taken before a command is written to the PTY, this is the row the
    /// shell will echo it on, so it anchors [`Self::text_from_abs`] to the
    /// output of that one command instead of whatever the tail of the buffer
    /// happens to be. The walk up the wrap chain is what makes it survive a
    /// half-typed line that had already wrapped (the `Ctrl+U` sent with the
    /// command clears it back to the prompt row).
    pub fn abs_cursor_logical_start(&self) -> Option<i64> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
        if self.is_alt_screen() {
            return None;
        }
        let grid = self.backend.term.grid();
        let cols = grid.columns();
        let topmost = grid.topmost_line().0;
        let mut first = grid.cursor.point.line.0;
        let mut walked = 0;
        while first > topmost && walked < 64 {
            let prev = &grid[Line(first - 1)];
            if !prev[Column(cols - 1)].flags.contains(CellFlags::WRAPLINE) {
                break;
            }
            first -= 1;
            walked += 1;
        }
        Some(grid.history_size() as i64 + i64::from(first))
    }

    /// Every logical (wrap-joined) line from absolute row `abs_line` down to
    /// the last row carrying content. Returns `None` on the alternate screen
    /// or when the row has left the addressable grid (scrollback ring
    /// saturated and rotated past it), same contract as
    /// [`Self::logical_line_from_abs`], so a caller with a stale anchor can
    /// fall back instead of reading unrelated rows.
    ///
    /// Unlike [`Self::logical_line_from_abs`] this does NOT apply the RPROMPT
    /// gap heuristic: command output is full of column runs (`ls -l`, `df -h`,
    /// `docker ps`) that the heuristic would truncate mid-row.
    ///
    /// At most `max_lines` lines are returned; a longer region drops rows from
    /// the MIDDLE (a `find` puts its answer at the head, a build puts its
    /// error at the tail) and reports how many in [`RegionText::dropped`].
    ///
    /// The region also reports which of its lines the cursor sits on
    /// ([`RegionText::cursor_line`]), because that is the one signal that
    /// tells a shell prompt from output text: the text alone cannot
    /// (`➜  ~ ` and `Progress: 100%` are the same shape).
    pub fn text_from_abs(&self, abs_line: i64, max_lines: usize) -> Option<RegionText> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;
        if self.is_alt_screen() {
            return None;
        }
        let grid = self.backend.term.grid();
        let cols = grid.columns();
        let rel = abs_line - grid.history_size() as i64;
        if rel < i64::from(grid.topmost_line().0) || rel > i64::from(grid.bottommost_line().0) {
            return None;
        }
        let row_text = |li: i32| -> String {
            let row = &grid[Line(li)];
            let mut s = String::new();
            for c in 0..cols {
                let cell = &row[Column(c)];
                if cell.c != '\0' && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    s.push(cell.c);
                }
            }
            s
        };
        let wraps = |li: i32| -> bool {
            grid[Line(li)][Column(cols - 1)]
                .flags
                .contains(CellFlags::WRAPLINE)
        };
        // Trailing blank rows are the unwritten part of the screen, not
        // output: end the region on the last row that carries content.
        let bottom = grid.bottommost_line().0;
        let mut end = bottom;
        while end > rel as i32 && row_text(end).trim().is_empty() {
            end -= 1;
        }
        // Where the cursor is, in the same viewport-relative space the rows
        // are indexed in. It lands OUTSIDE the region whenever the shell has
        // written a newline it hasn't drawn anything under yet, which is
        // exactly the state a running command leaves the screen in.
        let cursor_row = grid.cursor.point.line.0;
        let mut cursor_line = None;
        let mut lines: Vec<String> = Vec::new();
        let mut li = rel as i32;
        while li <= end {
            let first = li;
            let mut text = row_text(li);
            let mut rows = 1;
            while wraps(li) && li < end && rows < 64 {
                li += 1;
                text.push_str(&row_text(li));
                rows += 1;
            }
            if cursor_row >= first && cursor_row <= li {
                cursor_line = Some(lines.len());
            }
            lines.push(text.trim_end().to_string());
            li += 1;
        }
        let mut dropped = 0;
        let mut head_len = lines.len();
        if max_lines > 0 && lines.len() > max_lines {
            head_len = max_lines / 2;
            let tail_len = max_lines - head_len;
            dropped = lines.len() - max_lines;
            lines.drain(head_len..head_len + dropped);
            debug_assert_eq!(lines.len(), head_len + tail_len);
            // The elision renumbers everything below it, and a cursor inside
            // the dropped span no longer has a line to point at.
            cursor_line = cursor_line.and_then(|i| match i {
                i if i < head_len => Some(i),
                i if i < head_len + dropped => None,
                i => Some(i - dropped),
            });
        }
        Some(RegionText { lines, head_len, dropped, cursor_line })
    }
}

/// A slice of the grid read by [`TerminalState::text_from_abs`]: logical
/// lines, with the middle elided when the region is longer than the cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionText {
    /// The kept lines: `lines[..head_len]` from the top of the region,
    /// `lines[head_len..]` from its bottom.
    pub lines: Vec<String>,
    /// Where the tail slice starts. Equals `lines.len()` when nothing was
    /// dropped.
    pub head_len: usize,
    /// How many lines were removed from the middle. `0` = the region is
    /// complete.
    pub dropped: usize,
    /// Index into `lines` of the line the cursor sits on, `None` when the
    /// cursor is outside the region (above the anchor, or on the blank rows
    /// under the last line) or when its line fell into the elided middle.
    ///
    /// A shell that has drawn its prompt parks the cursor on it, so this is
    /// what distinguishes "the shell is waiting for input" from "a line of
    /// output that happens to look like a prompt".
    pub cursor_line: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The IME preedit round-trip the `ime_host` overlay depends on: the app
    /// stores composition text on the state, the overlay reads it back on the
    /// next redraw, and an empty string (commit / IME close) hides it again.
    #[test]
    fn preedit_round_trips_and_empty_clears() {
        let mut state = TerminalState::new_no_pty(24, 80).expect("headless state");
        assert!(state.preedit().is_empty(), "idle state has no preedit");

        state.set_preedit("nihao".to_string());
        assert_eq!(state.preedit(), "nihao");

        state.set_preedit(String::new());
        assert!(state.preedit().is_empty(), "empty preedit clears the overlay");
    }

    /// The canvas geometry cache keys off `render_epoch`: a stale epoch
    /// after real output or a palette swap would leave the terminal showing
    /// last frame's grid. Guard both bumps structurally.
    #[test]
    fn render_epoch_advances_on_output_and_palette() {
        let mut state = TerminalState::new_no_pty(24, 80).expect("headless state");

        let e0 = state.render_epoch();
        state.process(b"hello");
        let e1 = state.render_epoch();
        assert!(e1 > e0, "process() must advance the render epoch");

        state.set_palette(TerminalPalette::default());
        assert!(
            state.render_epoch() > e1,
            "set_palette() must advance the render epoch"
        );
    }

    #[test]
    fn visible_text_exports_only_the_drawn_viewport() {
        let mut state = TerminalState::new_no_pty_with_scrollback(24, 3, 100)
            .expect("headless state");
        state.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");

        // The live edge shows the last three rows; no earlier scrollback
        // belongs in a visible-screen export.
        assert_eq!(state.visible_text(), "three\nfour\nfive");

        // A scrolled viewport follows the exact three rows that the widget
        // would draw rather than silently snapping back to the live edge.
        state.set_viewport_scroll_offset(2);
        assert_eq!(state.visible_text(), "one\ntwo\nthree");
    }

    #[test]
    fn visible_text_clamps_an_offset_the_grid_no_longer_holds() {
        let mut state =
            TerminalState::new_no_pty_with_scrollback(24, 3, 100).expect("headless state");
        state.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");

        // An offset past the top of the buffer reads as the oldest screen the
        // grid can show, never as a range half outside it.
        state.set_viewport_scroll_offset(9);
        assert_eq!(state.visible_text(), "one\ntwo\nthree");

        // Dropping the history is the case that arrives without a frame in
        // between: the stored offset outlives the lines it pointed at.
        state.clear_scrollback();
        assert_eq!(state.visible_text(), "three\nfour\nfive");
    }

    // ── Anchored region reads (AI tool capture) ──

    /// The whole point of the anchor: what was already on screen when the
    /// command was written stays out of the region.
    #[test]
    fn text_from_abs_starts_at_the_anchor() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"Welcome to Ubuntu\r\nadmin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        state.process(b"uptime\r\n 07:55:44 up 3 days\r\nadmin@db:~$ ");
        let region = state.text_from_abs(anchor, 200).expect("anchor still readable");
        assert!(!region.lines.iter().any(|l| l.contains("Welcome to Ubuntu")));
        assert_eq!(region.lines[0], "admin@db:~$ uptime");
        assert_eq!(region.lines[1], " 07:55:44 up 3 days");
        assert_eq!(region.dropped, 0);
    }

    /// Column runs are output, not a zsh right prompt: `logical_line_from_abs`
    /// truncates at 8 interior spaces and this reader must not, or every
    /// `ls -l` / `df -h` table would reach the model cut off at its first gap.
    #[test]
    fn text_from_abs_keeps_column_runs() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"admin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        state.process(b"df -h\r\n/dev/sda1        47G   19G   26G  41% /\r\n");
        let region = state.text_from_abs(anchor, 200).expect("anchor still readable");
        assert!(
            region.lines[1].ends_with("41% /"),
            "the whole row survives: {:?}",
            region.lines[1]
        );
    }

    /// Past the cap the MIDDLE goes, so a `find`'s first hits and a build's
    /// final error both survive, and the elision is reported rather than
    /// silent.
    #[test]
    fn text_from_abs_elides_the_middle_past_the_cap() {
        let mut state = TerminalState::new_no_pty(80, 200).expect("headless state");
        state.process(b"admin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        state.process(b"seq 1 40\r\n");
        for i in 1..=40 {
            state.process(format!("line{i}\r\n").as_bytes());
        }
        // 1 echo + 40 rows = 41 lines of content, capped at 10.
        let region = state.text_from_abs(anchor, 10).expect("anchor still readable");
        assert_eq!(region.lines.len(), 10);
        assert_eq!(region.head_len, 5);
        assert_eq!(region.dropped, 31);
        assert_eq!(region.lines[0], "admin@db:~$ seq 1 40");
        assert_eq!(region.lines[4], "line4");
        assert_eq!(region.lines[5], "line36");
        assert_eq!(region.lines[9], "line40");
    }

    /// A command long enough to wrap must stay ONE entry even when the wrap
    /// chain is the last thing on screen: a second entry would read as output
    /// and drop the poll's patience from "wait it out" to a short silence,
    /// which is the give-up this whole change exists to remove.
    #[test]
    fn text_from_abs_joins_an_echo_that_wraps_to_the_last_row() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"admin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        let long = format!("find / -name \"{}\" 2>/dev/null", "x".repeat(90));
        state.process(long.as_bytes());
        let region = state.text_from_abs(anchor, 200).expect("anchor still readable");
        assert_eq!(region.lines.len(), 1, "the wrapped echo is one line: {:?}", region.lines);
        assert!(region.lines[0].ends_with("2>/dev/null"));
    }

    /// The cursor is the only thing that tells a drawn prompt from a line of
    /// output shaped like one, so the region has to report where it is: on
    /// the prompt the shell just drew, and nowhere at all while a command is
    /// still printing (the newline puts it on a blank row the region trims).
    #[test]
    fn text_from_abs_reports_the_line_the_cursor_sits_on() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"admin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        state.process(b"uptime\r\n 07:55:44 up 3 days\r\n");
        let region = state.text_from_abs(anchor, 200).expect("anchor still readable");
        assert_eq!(region.lines.len(), 2);
        assert_eq!(region.cursor_line, None, "still running: the cursor is under the output");

        state.process(b"admin@db:~$ ");
        let region = state.text_from_abs(anchor, 200).expect("anchor still readable");
        assert_eq!(region.cursor_line, Some(2), "the shell drew its prompt: {:?}", region.lines);
    }

    /// A wrapped prompt is one logical line, and the cursor sits on its LAST
    /// physical row: the index has to point at the joined line either way.
    #[test]
    fn text_from_abs_maps_the_cursor_through_a_wrap() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"admin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        state.process(b"echo hi\r\nhi\r\n");
        state.process(format!("{}$ ", "d".repeat(90)).as_bytes());
        let region = state.text_from_abs(anchor, 200).expect("anchor still readable");
        assert_eq!(region.lines.len(), 3, "{:?}", region.lines);
        assert_eq!(region.cursor_line, Some(2));
    }

    /// Past the cap the middle goes, and the indices below it shift: a
    /// cursor line reported against the pre-elision numbering would point at
    /// an unrelated row.
    #[test]
    fn text_from_abs_renumbers_the_cursor_line_after_eliding() {
        let mut state = TerminalState::new_no_pty(80, 200).expect("headless state");
        state.process(b"admin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        state.process(b"seq 1 40\r\n");
        for i in 1..=40 {
            state.process(format!("line{i}\r\n").as_bytes());
        }
        state.process(b"admin@db:~$ ");
        let region = state.text_from_abs(anchor, 10).expect("anchor still readable");
        assert_eq!(region.lines.len(), 10);
        assert_eq!(region.cursor_line, Some(9));
        assert_eq!(region.lines[9], "admin@db:~$");
    }

    /// The alternate screen repaints rows in place, so an anchor into it
    /// addresses nothing stable: both halves refuse it and the caller falls
    /// back to the undelimited tail.
    #[test]
    fn the_alternate_screen_has_no_anchor() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"admin@db:~$ ");
        let anchor = state.abs_cursor_logical_start().expect("primary screen");
        state.process(b"\x1b[?1049h");
        assert!(state.abs_cursor_logical_start().is_none());
        assert!(state.text_from_abs(anchor, 200).is_none());
    }

    // ── Scrollback search (C1) ──

    /// A needle on the visible screen is found; the count is 1-based.
    #[test]
    fn search_finds_visible_match() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"needle-alpha here\r\n");
        state.search_open();
        state.search_set_query("needle-alpha");
        assert_eq!(state.search_count(), Some((1, 1)));
        let m = state.search.as_ref().unwrap().matches[0];
        // On the visible screen, line 0 is the first row.
        assert_eq!(m.start_line, 0);
        assert_eq!(m.start_col, 0);
    }

    /// A match that scrolled off the top lives at a negative grid line.
    #[test]
    fn search_finds_scrollback_match() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        // Print the needle, then push it well above the screen.
        state.process(b"needle-top\r\n");
        for i in 0..40 {
            state.process(format!("filler line {i}\r\n").as_bytes());
        }
        state.search_open();
        state.search_set_query("needle-top");
        assert_eq!(state.search_count(), Some((1, 1)));
        let m = state.search.as_ref().unwrap().matches[0];
        assert!(m.start_line < 0, "scrollback match must be a negative line, got {}", m.start_line);
        // The queued scroll must bring that scrollback line into the visible
        // window: with the draw's `visible_row = line + scroll_offset`, the
        // resulting row has to land inside [0, rows). Regression guard for a
        // sign slip that scrolled the match off the top.
        let offset = state.pending_scroll.get().expect("scroll queued");
        let visible_row = m.start_line + offset;
        assert!(
            (0..24).contains(&visible_row),
            "match at line {} + offset {} = row {} must be on screen",
            m.start_line,
            offset,
            visible_row,
        );
    }

    /// Literal search: the needle is escaped, so `a.b` does not match `axb`.
    #[test]
    fn search_is_literal_not_regex() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"axb and a.b\r\n");
        state.search_open();
        state.search_set_query("a.b");
        // Only the literal `a.b` matches, not `axb`.
        assert_eq!(state.search_count(), Some((1, 1)));
    }

    /// Stepping wraps around and reports the right 1-based index.
    #[test]
    fn search_step_wraps() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"x x x\r\n");
        state.search_open();
        state.search_set_query("x");
        assert_eq!(state.search_count(), Some((1, 3)));
        state.search_step(true);
        assert_eq!(state.search_count(), Some((2, 3)));
        state.search_step(true);
        assert_eq!(state.search_count(), Some((3, 3)));
        state.search_step(true); // wrap
        assert_eq!(state.search_count(), Some((1, 3)));
        state.search_step(false); // wrap backward
        assert_eq!(state.search_count(), Some((3, 3)));
    }

    /// An empty query clears the matches; closing drops the state.
    #[test]
    fn search_empty_and_close() {
        let mut state = TerminalState::new_no_pty(80, 24).expect("headless state");
        state.process(b"hello world\r\n");
        state.search_open();
        state.search_set_query("hello");
        assert_eq!(state.search_count(), Some((1, 1)));
        state.search_set_query("");
        assert_eq!(state.search_count(), Some((0, 0)));
        state.search_close();
        assert!(!state.search_active());
        assert_eq!(state.search_count(), None);
    }
}
