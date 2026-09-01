use super::*;

impl<Message> TerminalView<Message> {
    /// Grid dimensions (cols, rows) that fit the given canvas size at this
    /// view's measured cell metrics. Uses the real per-font cell width so the
    /// column count matches the glyphs actually drawn (a font wider than the
    /// old `0.6` ratio would otherwise be told it has more columns than fit,
    /// and wrap early).
    pub(super) fn grid_size(&self, width: f32, height: f32) -> (u16, u16) {
        let cell_width = self.cell_width.max(1.0);
        let cell_height = self.cell_height.max(1.0);
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
    pub(super) fn visible_row_to_line(visible_row: u16, scroll_offset: i32) -> i32 {
        visible_row as i32 - scroll_offset
    }

    /// Compute a word- or line-granularity selection around `cell` using
    /// alacritty's native semantic / line search. `cell` is `(col, line)`
    /// in grid-line coordinates (negative line = scrollback). The current
    /// delimiter set is synced into the backend first (a cheap no-op when
    /// unchanged).
    pub(super) fn semantic_selection(
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
    /// `None` to let the normal local handlers run. The caller
    /// guarantees the app has mouse tracking on and Shift isn't held,
    /// except for the release of a tracked press (see
    /// `release_completes_tracked_press`), which is let through with
    /// Shift down so the report sequence stays balanced.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_mouse_report(
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
        // C5: a host with mouse reporting disabled never emits a report,
        // regardless of the remote's mouse mode; the caller falls through
        // to local selection / paste. Single chokepoint so no report path
        // (press / release / wheel / motion) can leak.
        if !self.mouse_reporting {
            return None;
        }
        let kbd = widget_state.modifiers;
        let ctrl = kbd.control();
        // Shift is the local-selection bypass, so the caller only reaches
        // here with it released (or completing a tracked press, whose
        // release must stay consistent with the Shift-less press); never
        // fold it into the report.
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
                // New drag: re-arm the per-drag hint guard so Always mode
                // can fire once for this gesture too.
                widget_state.mouse_hint_emitted = false;
                let bytes =
                    mouse_report::encode(mode, MouseEventKind::Press, rb, col, row, mods)?;
                // The one gesture outcome a report can't distinguish from a
                // broken app: a right / middle click the remote program asked
                // for is the same "nothing happened" on screen as a paste that
                // never fired (issue #181). Presses only, so a tracked drag
                // can't turn the log into a firehose.
                tracing::debug!(?btn, col, row, "mouse press reported to the remote app");
                Some(self.emit_input(bytes))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(btn)) => {
                let rb = Self::iced_to_report_button(*btn)?;
                // Only report the release of a press WE reported. A release
                // whose press never reached the app (it landed on a sibling
                // widget) must stay local: this arm captures what it
                // consumes, and sibling `button`s fire on release, so
                // reporting unconditionally made every sidebar click dead
                // while a full-screen app (mc, htop) held mouse tracking,
                // forcing the Shift bypass for plain UI clicks.
                if widget_state.report_button != Some(rb) {
                    return None;
                }
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
                // A left-button drag while the app holds the mouse is the
                // user trying to select text that mouse tracking is
                // swallowing. Surface the Shift bypass once per pane, on
                // the first such drag. Dropping this single motion report
                // (we return before encoding) is harmless: the next move
                // reports the new cell.
                if !widget_state.mouse_hint_emitted
                    && widget_state.report_button == Some(ReportButton::Left)
                    && let Some(cb) = &self.on_mouse_capture_hint
                {
                    widget_state.mouse_hint_emitted = true;
                    widget_state.report_cell = Some((col, row));
                    return Some(CanvasAction::publish(cb()).and_capture());
                }
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
                // A fractional `Lines` delta is a high-resolution wheel
                // reporting part of a detent (#150): accumulated into
                // whole notches, or the remote app would get one report
                // per fragment and scroll a multiple of what the user
                // turned. `Pixels` accumulates the same way on the
                // cell scale: each fragment is a few pixels, and the
                // per-notch minimum below would turn every one into a
                // full wheel report, flooding a tracking TUI with
                // several times the gesture.
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => {
                        widget_state.scroll_px_residual.set(0.0);
                        Self::whole_notches(widget_state, *y) as f32
                    }
                    mouse::ScrollDelta::Pixels { y, .. } => {
                        widget_state.scroll_line_residual.set(0.0);
                        Self::whole_cells_px(widget_state, *y, self.cell_height) as f32
                    }
                };
                if dy == 0.0 {
                    // The fragment only grew a residual. Still consume
                    // it: while the app holds mouse tracking the wheel
                    // belongs to the report path, and falling through
                    // would hand the fragment to the local-scrollback
                    // arm (which shares the pixel residual) to scroll a
                    // buffer the TUI is covering.
                    return Some(CanvasAction::capture());
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

    /// Whether copy-on-select's auto-copy at release is deferred to a
    /// right-click. The deferral is the `right_click_copy` sub-option,
    /// which only exists under the Paste scheme; a stale `true` left
    /// behind after switching to Menu / Extend (Settings hides the
    /// toggle there) must not silently kill copy-on-select.
    pub(super) fn defers_copy_to_right_click(&self) -> bool {
        self.right_click_copy && self.right_click_action == RightClickAction::Paste
    }

    /// True when `event` is the release of a button whose press WAS
    /// reported to the remote app. Such a release must still be
    /// reported even while Shift is held: the Shift bypass exists so
    /// NEW gestures can select locally, but letting it swallow the
    /// release that completes a tracked press would leave the app
    /// holding a phantom button and `report_button` stuck at
    /// `Some(..)`, turning every later motion into a drag report.
    pub(super) fn release_completes_tracked_press(
        widget_state: &TerminalWidgetState,
        event: &iced::Event,
    ) -> bool {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonReleased(btn)) => {
                Self::iced_to_report_button(*btn)
                    .is_some_and(|rb| widget_state.report_button == Some(rb))
            }
            _ => false,
        }
    }

    /// Whole wheel notches in a `ScrollDelta::Lines` value, carrying
    /// the sub-notch remainder on the widget state.
    ///
    /// One detent is not one event on a high-resolution wheel: the
    /// device reports fractions of a click (Wayland `axis_value120`,
    /// Windows `WM_MOUSEWHEEL`, both on a 120-per-detent scale the
    /// platform divides out before winit sees it), so `y` arrives as
    /// 0.125 or 0.25 and a plain `as i32` truncated every one of them
    /// to zero: the wheel did nothing at all (issue #150). Accumulate
    /// instead, emit the notches the total now covers, and keep the
    /// rest for the next event. A sign flip drops the stale residual so
    /// a reversal responds on its first fragment rather than spending
    /// it undoing the accumulated one.
    ///
    /// A conventional wheel is unaffected: `y` is already ±1, which
    /// accumulates to exactly one notch and leaves no remainder.
    ///
    /// A horizontal-only event (`y == 0.0`, what a tilt wheel sends)
    /// means two different things to the two guards around the
    /// residuals, and the asymmetry is deliberate. It carries no
    /// DIRECTION, so the sign-flip test below skips it rather than
    /// reading `+0.0` as "upward" and discarding the accumulated
    /// fraction. It does still name a device KIND, which is why both
    /// call sites clear the OTHER kind's residual unconditionally
    /// before calling here: a tilt on a wheel proves the touchpad's
    /// sub-cell pixel fraction is stale, and vice versa.
    pub(super) fn whole_notches(widget_state: &TerminalWidgetState, y: f32) -> i32 {
        let prev = widget_state.scroll_line_residual.get();
        // Direction only: see the doc comment on the `y == 0.0` split.
        let acc = if prev != 0.0 && y != 0.0 && prev.signum() != y.signum() {
            y
        } else {
            prev + y
        };
        let whole = acc.trunc();
        widget_state.scroll_line_residual.set(acc - whole);
        whole as i32
    }

    /// Whole CELLS in a `ScrollDelta::Pixels` value, carrying the
    /// sub-cell pixel remainder on the widget state: the pixel twin of
    /// [`Self::whole_notches`], shared by the local-scrollback arm and
    /// the mouse-report arm. A precision touchpad delivers a few
    /// pixels per event, below one cell, and both consumers must
    /// accumulate: flooring floored every event to zero and scrollback
    /// never moved (#91), while the report arm's ceil turned every
    /// fragment into at least one wheel notch, so a slow two-finger
    /// scroll over a tracking TUI (tmux `mouse on`, htop) flooded it
    /// with several times the gesture (#150's shape, other arm). The
    /// same zero guard as `whole_notches` applies: a horizontal-only
    /// event names a device but not a direction, so it must not read
    /// as a reversal.
    pub(super) fn whole_cells_px(
        widget_state: &TerminalWidgetState,
        y: f32,
        cell_height: f32,
    ) -> i32 {
        let prev = widget_state.scroll_px_residual.get();
        let acc = if prev != 0.0 && y != 0.0 && prev.signum() != y.signum() {
            y
        } else {
            prev + y
        };
        let cells = (acc / cell_height).trunc();
        widget_state.scroll_px_residual.set(acc - cells * cell_height);
        cells as i32
    }

    pub(super) fn is_in_selection(sel: &Selection, col: u16, line: i32) -> bool {
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

impl<Message> TerminalView<Message>
where
    Message: Clone,
{
    pub(super) fn on_event(
        &self,
        widget_state: &mut TerminalWidgetState,
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

        // Focus latch, read by the drop-the-highlight rule below. It turns
        // true the first time this widget is rendered focused and never
        // goes back, which is what separates a pane that LOST focus (its
        // highlight must go) from a surface rendered unfocused by
        // construction: the session player's replay stage is never focused
        // (its keys are transport controls, not terminal input), so a plain
        // `!focused` test swept the selection on the very next event of the
        // drag that made it and nothing could be copied out of a recording.
        // Set before any early return, or a press in the resize margin
        // would leave it false on a pane that is focused.
        if self.focused {
            widget_state.ever_focused = true;
        }

        // Presses inside a resize strip belong to whatever contains this
        // pane, not to the terminal: they are how a `pane_grid` divider is
        // grabbed. Declining here (rather than handling and forwarding) is
        // what stops the drag from also painting a text selection, and it
        // is why the panes can sit flush with no gutter and still be
        // resizable. Only edges that border a sibling carry a margin, so
        // selecting from column 0 at the grid's outer edge still works.
        if matches!(
            event,
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && self.cursor_in_resize_margin(bounds, cursor)
        {
            return None;
        }

        // A pane that no longer has focus drops its highlight. The
        // selection lives in this widget's own tree state, so nothing
        // outside can reach it, and without this every pane you ever
        // selected in keeps its block lit: split a tab three ways and you
        // are looking at three highlights with no way to tell which one
        // the next copy would take (field report).
        //
        // Only the HIGHLIGHT goes. `primary_selection` is deliberately
        // kept, so middle-click paste and the paste-selection action
        // still hand back the last thing selected, in whichever pane it
        // was selected; copy-on-select has already copied by now anyway.
        // Events are broadcast to every widget, so the click that moves
        // focus is itself what clears the pane being left, and the same
        // holds when the whole tab changes.
        //
        // Gated on `ever_focused` so this reads as "lost focus" rather
        // than "is not focused": a display-only surface that never takes
        // focus keeps its selection (see the latch above).
        if !self.focused
            && widget_state.ever_focused
            && (widget_state.selection.is_some() || widget_state.selecting)
        {
            widget_state.selection = None;
            widget_state.selecting = false;
            return Some(CanvasAction::request_redraw());
        }

        // A left press on the perf HUD toggles its compact <-> full-name
        // metric labels, the canvas overlay's stand-in for tooltips
        // (issue #69). Checked before mouse reporting and selection: the
        // HUD draws above everything, so it hit-tests above everything
        // too (and the press must not reach a remote app holding mouse
        // tracking; the report path only reports releases whose press it
        // reported, so the matching release stays local for free).
        if (self.perf_overlay || perf_overlay_enabled())
            && matches!(
                event,
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
            && let Some(pos) = cursor.position_in(bounds)
            && widget_state.hud_rect.get().is_some_and(|hud| hud.contains(pos))
        {
            widget_state.hud_pressed = true;
            toggle_hud_wide();
            return Some(CanvasAction::request_redraw().and_capture());
        }

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
        // The Shift bypass only blocks NEW gestures; the release of a
        // press that was already reported must go through regardless,
        // see `release_completes_tracked_press`.
        if let Some((mode, grid_cols, grid_rows)) = report_ctx
            && (!widget_state.modifiers.shift()
                || Self::release_completes_tracked_press(widget_state, event))
            && let Some(action) =
                self.handle_mouse_report(widget_state, event, bounds, cursor, mode, grid_cols, grid_rows)
        {
            return Some(action);
        }

        match event {
            // Mouse press, scrollbar interaction takes priority, then
            // URL open, then text selection.
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                return self.on_left_press(widget_state, bounds, cursor);
            }
            // Mouse move, drag scrollbar thumb or extend selection.
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                return self.on_cursor_moved(widget_state, bounds, cursor, hover_changed);
            }
            // Mouse release, end selection or scrollbar drag.
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                return self.on_left_release(widget_state, bounds, cursor);
            }
            // A mouse button the user bound to a terminal gesture (the app
            // owns the binding model and hands the matcher down, see
            // [`MouseResolver`]). The factory case is the X11 middle-click
            // paste (xterm / PuTTY tradition), which is why the gesture
            // isn't gated on `copy_on_select`.
            //
            // The POSITION of this arm is load-bearing: it sits after the
            // mouse-report path, so when the remote app holds mouse
            // tracking the report has already consumed the press (Shift
            // bypasses, as everywhere) and a binding can never take a
            // button away from a TUI. Left and Right never resolve to a
            // gesture (the bindable set excludes them), so their own arms
            // below still own those buttons.
            iced::Event::Mouse(mouse::Event::ButtonPressed(button))
                if cursor.position_in(bounds).is_some()
                    && self
                        .mouse_bindings
                        .as_ref()
                        .and_then(|f| f(*button, &widget_state.modifiers))
                        .is_some() =>
            {
                // Re-resolve without unwrapping, same shape as the chord
                // arm: the guard already proved this is Some.
                let gesture = self
                    .mouse_bindings
                    .as_ref()
                    .and_then(|f| f(*button, &widget_state.modifiers))?;
                return match gesture {
                    MouseGesture::Widget(action) => {
                        self.perform_chord_action(action, widget_state)
                    }
                    // Everything the app owns (paste into an SSH session,
                    // split a pane, ...). Captured either way: the button
                    // was spoken for.
                    MouseGesture::Publish(msg) => {
                        Some(CanvasAction::publish(msg).and_capture())
                    }
                };
            }
            // `on_paste_request` callback we delegate the actual paste to
            // the app dispatcher so it can target the SSH session (the
            // local-PTY write below only reaches local-shell tabs). The
            // fallback covers callers that don't set the hook. Gated on
            // `copy_on_select`: that setting bundles "select to copy & right
            // click to paste", so right-click does nothing when it's off.
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                if cursor.position_in(bounds).is_some() =>
            {
                return self.on_right_press(widget_state, bounds, cursor);
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
                return self.on_wheel_scroll(widget_state, delta);
            }
            // Modifier tracking for the URL Ctrl+Click gate. iced
            // doesn't pass the current modifier mask on mouse events,
            // so we mirror it from the dedicated change event.
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                widget_state.modifiers = *m;
            }
            // The widget's own chords: copy, select-all and scrollback
            // paging, all user-rebindable (the app hands the matcher down,
            // see `ChordResolver`). Paste is NOT here: it lives in the app
            // so it can reach an SSH session, since `state.write` only
            // targets a local PTY.
            //
            // This arm MUST stay ahead of the "any other key press" arm
            // below, which drops the selection and honours PuTTY's
            // reset-scrollback-on-keypress. Landing there would wipe the
            // selection out from under a copy, and snap the view straight
            // back to the live edge after a page-up.
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if (self.focused || self.chords_unfocused)
                    && self
                        .chords
                        .as_ref()
                        .and_then(|f| f(key, modifiers))
                        .is_some() =>
            {
                // Re-resolve without unwrapping: the guard above already
                // proved this is Some, so the `?` never fires.
                let action = self.chords.as_ref().and_then(|f| f(key, modifiers))?;
                return self.perform_chord_action(action, widget_state);
            }
            // Any other key press dismisses a live selection, matching
            // xterm / iTerm where typing or navigating clears the highlight
            // (otherwise a stale selection lingers as a tinted band, e.g.
            // over a full-screen TUI like mc that took over the screen after
            // the selection was made). The keystroke is NOT captured: it must
            // still reach the PTY through the global key subscription (an
            // independent path), so we only drop the selection and redraw.
            // Bare modifier presses (Ctrl / Shift / Alt / Super) must NOT
            // trigger it, otherwise the first key of a copy chord (Ctrl, then
            // Shift+C) wipes the selection before the copy fires. The copy /
            // select-all chords are handled by earlier arms that return
            // first, so a copy is never treated as a terminal keystroke here.
            //
            // Scroll-on-input (PuTTY's "reset scrollback on keypress")
            // deliberately does NOT live here: a canvas program is handed
            // every key event even while a sibling `text_input` owns the
            // focus (`Canvas::update` calls the program unconditionally), so
            // typing in the sidebar's chat or search box would yank the
            // terminal to the live edge under the reader. The host queues it
            // on its own input funnel instead, where it follows the bytes the
            // PTY actually receives (issue #111).
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                if self.focused
                    && !matches!(
                        key,
                        keyboard::Key::Named(
                            keyboard::key::Named::Control
                                | keyboard::key::Named::Shift
                                | keyboard::key::Named::Alt
                                | keyboard::key::Named::Super
                                | keyboard::key::Named::Hyper
                                | keyboard::key::Named::Meta
                        )
                    )
                    && (widget_state.selection.is_some()
                        || widget_state.select_anchor.is_some()) =>
            {
                widget_state.selection = None;
                widget_state.select_anchor = None;
                widget_state.selecting = false;
                return Some(CanvasAction::request_redraw());
            }
            _ => {}
        }
        None
    }

    /// The wheel over the grid: scrollback in the OS-natural
    /// direction, or arrow keys when the remote app is on the
    /// alternate screen (`top`, `vim`, `less`), where our own
    /// scrollback is empty and paging belongs to the app.
    fn on_wheel_scroll(
        &self,
        widget_state: &mut TerminalWidgetState,
        delta: &mouse::ScrollDelta,
    ) -> Option<CanvasAction<Message>> {
            let lines = match delta {
                // A notch wheel, but not necessarily a WHOLE notch: a
                // high-resolution wheel reports fractions of a detent
                // (#150), so this accumulates exactly like the pixel
                // arm below. Reset the pixel remainder so a device
                // switch can't leave a stale fraction behind.
                mouse::ScrollDelta::Lines { y, .. } => {
                    widget_state.scroll_px_residual.set(0.0);
                    Self::whole_notches(widget_state, *y) * 3
                }
                // Pixel deltas (Windows precision touchpads / high-res
                // wheels) arrive a few pixels at a time, below one cell
                // per event. Truncating each to whole cells floored
                // every one to zero and scrollback never moved (#91).
                // Accumulate into a residual, emit the whole cells it
                // now covers, and keep the sub-cell remainder for the
                // next event; a sign flip drops the stale residual so a
                // reversal responds at once.
                mouse::ScrollDelta::Pixels { y, .. } => {
                    widget_state.scroll_line_residual.set(0.0);
                    Self::whole_cells_px(widget_state, *y, self.cell_height)
                }
            };
            // A delta that only grew a residual (no whole cell / notch
            // yet) still belongs to this canvas: consume it so it can't
            // bleed into a sibling scrollable, but skip the lock and the
            // redraw since nothing moved.
            if lines == 0 {
                return Some(CanvasAction::capture());
            }
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
                    (in_alt, grid.total_lines().saturating_sub(grid.screen_lines()) as i32)
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
            widget_state.scroll_offset
                .set((widget_state.scroll_offset.get() + lines).max(0).min(max_scroll));
            Some(CanvasAction::request_redraw().and_capture())
    }

    /// A right press, under whichever of the three schemes the
    /// user picked: context menu, paste, or extend the selection.
    ///
    /// The paste is delegated to the host rather than written
    /// here, because a local `state.write` only reaches a local
    /// PTY and would silently do nothing over SSH.
    fn on_right_press(
        &self,
        widget_state: &mut TerminalWidgetState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<CanvasAction<Message>> {
            // The right-click scheme (PuTTY's Menu / Paste / Extend) is
            // the single authority for this gesture. Unlike the old
            // path it is NOT gated on `copy_on_select`: an explicit
            // "Paste" scheme that silently did nothing with copy-on-
            // select off would be a surprise.
            match self.right_click_action {
                RightClickAction::Menu => {
                    if let Some(cb) = &self.on_context_menu {
                        // Window-absolute position for the app's overlay
                        // (same coordinate space as every other menu
                        // anchor). `position()` is the viewport point.
                        let abs = cursor.position().unwrap_or_default();
                        // Capture the live selection's text now, so the
                        // app-rendered "Copy" row can offer it (the
                        // selection state is unreachable from the app).
                        let sel_text = widget_state
                            .selection
                            .as_ref()
                            .filter(|s| !s.is_empty())
                            .and_then(|sel| {
                                self.state.lock().ok().and_then(|state| {
                                    let t = state.get_selection_text(sel);
                                    (!t.is_empty()).then_some(t)
                                })
                            });
                        return Some(
                            CanvasAction::publish(cb(abs.x, abs.y, sel_text)).and_capture(),
                        );
                    }
                    Some(CanvasAction::capture())
                }
                RightClickAction::Extend => {
                    // xterm extend: move the selection's NEARER boundary
                    // to the click point, keeping the far anchor fixed,
                    // then copy. A no-op when there is nothing to extend
                    // (or when the live selection is a block).
                    if let Some(pos) = cursor.position_in(bounds) {
                        let (col, vrow) = self.pixel_to_cell(pos);
                        let line =
                            Self::visible_row_to_line(vrow, widget_state.scroll_offset.get());
                        if let Some(sel) = widget_state.selection.as_ref().filter(|s| !s.block)
                        {
                            let extended = sel.extended_to((col, line));
                            widget_state.selection = Some(extended);
                            if let Ok(state) = self.state.lock() {
                                let text = state.get_selection_text(&extended);
                                drop(state);
                                if !text.is_empty() {
                                    set_clipboard_text(&text);
                                }
                            }
                            return Some(CanvasAction::request_redraw().and_capture());
                        }
                    }
                    Some(CanvasAction::capture())
                }
                RightClickAction::Paste => {
                    // copy_on_select + right_click_copy: a right-click
                    // over a live selection copies it instead of pasting,
                    // then clears the selection so the next right-click
                    // pastes. The copy is written straight to the
                    // clipboard here (mirroring Ctrl+Shift+C), not via
                    // `on_paste_request` (the paste hook).
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
                    crate::host_clipboard::paste_into(Arc::clone(&self.state));
                    Some(CanvasAction::capture())
                }
            }
    }

    /// A left release: finish a selection (copy-on-select fires
    /// here) or end a scrollbar drag.
    ///
    /// Only captures releases it actually owns. Capturing every
    /// release is what once made sibling `button`s in the sidebar
    /// look dead, since a button fires on release.
    fn on_left_release(
        &self,
        widget_state: &mut TerminalWidgetState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<CanvasAction<Message>> {
            // The press was consumed by the perf HUD toggle; swallow
            // the matching release so it can't privacy-pin or
            // click-classify the cells underneath the panel.
            if widget_state.hud_pressed {
                widget_state.hud_pressed = false;
                return Some(CanvasAction::capture());
            }
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
            // Text of the selection that just finished, read once and
            // shared by the two things that want it: the PRIMARY
            // selection below and the optional auto-copy. Degenerate
            // selections that never moved (a single click) don't count,
            // but a double/triple-click one does even when it lands on a
            // one-character word. The grid width rides along for the
            // ghost's resize guard.
            let finished = if was_selecting
                && let Some(sel) = widget_state.selection
                && (!sel.is_empty() || was_semantic)
                && let Ok(state) = self.state.lock()
            {
                use alacritty_terminal::grid::Dimensions;
                let grid = state.backend.term.grid();
                let cols = grid.columns() as u16;
                let total = grid.total_lines();
                let text = state.get_selection_text(&sel);
                drop(state);
                (!text.is_empty()).then_some((text, sel, cols, total))
            } else {
                None
            };
            // X11 PRIMARY selection: selecting text IS the act that sets
            // it, so this is not gated on any setting, and it survives
            // the highlight being cleared on the next keystroke. That
            // separation is the whole point of the buffer: it is what
            // lets "select a path, type `cd `, paste it" work, which a
            // highlight-bound read cannot do. Independent of the system
            // clipboard by design (`copy_on_select` is the setting for
            // people who want selections there too). The range is kept
            // alongside the text: once the live highlight is gone the
            // draw pass shows it as a faint ghost band, illustrating
            // what a PRIMARY paste will insert.
            if let Some((ref text, sel, cols, total)) = finished {
                widget_state.primary_selection = Some(text.clone());
                widget_state.primary_ghost = Some((sel, cols, total));
                // Where the platform has a real PRIMARY selection, hand it
                // the same text: selecting here then middle-clicking in any
                // other window is what a Linux user expects, and it is the
                // half of the buffer we could never provide on our own.
                crate::host_clipboard::write_primary_text(text);
            }
            // Auto-copy the just-finished selection when the setting is
            // enabled (XTerm / iTerm behaviour). When `right_click_copy`
            // is on the copy is deferred to a right-click instead, so
            // skip it here; the deferral is Paste-scheme-only (see
            // `defers_copy_to_right_click`).
            if let Some((ref text, ..)) = finished
                && self.copy_on_select
                && !self.defers_copy_to_right_click()
            {
                set_clipboard_text(text);
            }
            if was_dragging {
                return Some(CanvasAction::request_redraw().and_capture());
            }
            // Plain click (no drag, no word/line select) on a masked
            // privacy span toggles a pinned reveal for its value: the
            // mask is undone for every occurrence of that value until
            // it's clicked again. Keyed by the span text, not its
            // cells, so the reveal survives scrolling and re-prints.
            if self.privacy
                && was_selecting
                && !was_semantic
                && widget_state.selection.as_ref().is_some_and(|s| s.is_empty())
                && let Some(pos) = cursor.position_in(bounds)
            {
                let (col, vrow) = self.pixel_to_cell(pos);
                let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset.get());
                let value = self.state.lock().ok().and_then(|state| {
                    privacy_value_at_cell(
                        &state.backend.term,
                        &state.palette,
                        &self.privacy_terms,
                        self.privacy_classes,
                        line,
                        col,
                    )
                });
                if let Some(value) = value {
                    if !widget_state.pinned_privacy.remove(&value) {
                        widget_state.pinned_privacy.insert(value);
                    }
                    return Some(CanvasAction::request_redraw().and_capture());
                }
            }
            // Plain click (no Ctrl, no drag, no word/line select) on a
            // URL: the user likely expected the link to open, but plain
            // clicks select (Termius-style, see the press handler). Let
            // the app surface the "hold Ctrl and click" toast at the
            // exact moment the gesture missed. Ctrl+Click never reaches
            // here as a click: the press handler opens the URL without
            // starting a selection, so `was_selecting` is false.
            if !widget_state.modifiers.control()
                && was_selecting
                && !was_semantic
                && widget_state.selection.as_ref().is_some_and(|s| s.is_empty())
                && let Some(cb) = &self.on_link_click_hint
                && let Some(pos) = cursor.position_in(bounds)
            {
                let (col, vrow) = self.pixel_to_cell(pos);
                let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset.get());
                let on_url = self.state.lock().is_ok_and(|state| {
                    // Same OSC 8 discriminator: a blocked-scheme link is
                    // not openable, so it must not draw the "hold Ctrl to
                    // click" hint (the affordance would lie).
                    match osc8_link_at_cell(&state.backend.term, line, col) {
                        Some((uri, _, _)) => osc8_scheme_allowed(&uri),
                        None => url_at_cell(&state.backend.term, line, col).is_some(),
                    }
                });
                if on_url {
                    return Some(CanvasAction::publish(cb()).and_capture());
                }
            }
            // Only swallow the release when it belongs to this terminal:
            // a finishing selection, or a release physically over the
            // canvas. A stray release that lands on a sibling widget
            // (e.g. a button in the terminal sidebar) must pass through,
            // otherwise that widget never sees its release and its
            // `on_press` never fires (iced buttons act on release).
            if was_selecting || was_semantic || cursor.position_in(bounds).is_some() {
                return Some(CanvasAction::capture());
            }
            None
    }

    /// Pointer motion: drag the scrollbar thumb, extend a live
    /// selection, or just re-run the hover detection that lights
    /// URLs and drives the scrollbar's reveal.
    ///
    /// The hottest path in the widget (dozens of events a second
    /// while dragging), which is why it redraws only on a real
    /// change rather than on every event.
    fn on_cursor_moved(
        &self,
        widget_state: &mut TerminalWidgetState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        hover_changed: bool,
    ) -> Option<CanvasAction<Message>> {
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
                    widget_state.scroll_offset
                        .set((start_offset - doffset).clamp(0, sb.history_size));
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
                            widget_state.scroll_offset
                                .set((widget_state.scroll_offset.get() + step).min(history));
                        } else {
                            widget_state.scroll_offset
                                .set((widget_state.scroll_offset.get() - step).max(0));
                        }
                    }
                    // Clamp back into the widget for cell mapping (the
                    // pointer may be outside the bounds now).
                    let clamped = Point::new(
                        rel.x.clamp(0.0, bounds.width),
                        rel.y.clamp(0.0, bounds.height),
                    );
                    let (col, vrow) = self.pixel_to_cell(clamped);
                    let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset.get());
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
            let cell_changed;
            // Whether `hovered_link` (the app's reveal / blocked chip)
            // changed this move. A BLOCKED link returns no `hovered_url`
            // (no pointer, no scraped fallback), so `url_changed` stays
            // false, without this the blocked chip would rely on an
            // incidental cursor-blink repaint to appear or clear.
            let mut link_changed = false;
            let new_hover_url = if let Some(pos) = cursor.position_in(bounds) {
                let (col, vrow) = self.pixel_to_cell(pos);
                let same_cell = widget_state.hovered_cell == Some((col, vrow));
                cell_changed = !same_cell;
                widget_state.hovered_cell = Some((col, vrow));
                if same_cell {
                    widget_state
                        .hovered_url
                        .as_ref()
                        .map(|(u, _)| (u.clone(), pos))
                } else if let Ok(mut state) = self.state.lock() {
                    let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset.get());
                    // OSC 8 discriminator (same rule as the open + hint
                    // paths): a cell with an explicit link never falls back
                    // to a scraped URL. A disallowed scheme suppresses the
                    // pointer + underline but still records the blocked
                    // target so the app can show a "not allowed" chip; an
                    // allowed one drives the underline + the reveal chip.
                    let offset = widget_state.scroll_offset.get();
                    // Segments are grid lines; the underline is drawn in
                    // on-screen rows.
                    let on_screen = |segments: Vec<LinkSegment>| -> Vec<(u16, u16, u16)> {
                        segments
                            .into_iter()
                            .map(|(gl, sc, ec)| ((gl + offset) as u16, sc, ec))
                            .collect()
                    };
                    match osc8_link_run(&state.backend.term, line, col) {
                        Some((uri, segments)) => {
                            let allowed = osc8_scheme_allowed(&uri);
                            // Underline every wrapped row only for an
                            // allowed link.
                            widget_state.hovered_link_spans = if allowed {
                                on_screen(segments)
                            } else {
                                Vec::new()
                            };
                            let new_link = Some(HoveredLink {
                                target: uri.clone(),
                                allowed,
                            });
                            link_changed = state.hovered_link != new_link;
                            state.hovered_link = new_link;
                            allowed.then_some((uri, pos))
                        }
                        None => {
                            link_changed = state.hovered_link.is_some();
                            state.hovered_link = None;
                            // A scraped URL underlines the same way: its
                            // own row-local highlight stops at the wrap,
                            // so the tail rows would otherwise sit under
                            // the pointer with no cue.
                            match url_run_at_cell(&state.backend.term, line, col) {
                                Some((url, segments)) => {
                                    widget_state.hovered_link_spans = on_screen(segments);
                                    Some((url, pos))
                                }
                                None => {
                                    widget_state.hovered_link_spans.clear();
                                    None
                                }
                            }
                        }
                    }
                } else {
                    None
                }
            } else {
                // Cursor left the canvas: a revealed privacy span must
                // re-mask, so flag a cell change when one was tracked.
                cell_changed = widget_state.hovered_cell.is_some();
                widget_state.hovered_cell = None;
                widget_state.hovered_link_spans.clear();
                // Retract any link-reveal chip (allowed or blocked).
                if let Ok(mut state) = self.state.lock() {
                    link_changed = state.hovered_link.is_some();
                    state.hovered_link = None;
                }
                None
            };
            let url_changed = match (&widget_state.hovered_url, &new_hover_url) {
                (Some((a, _)), Some((b, _))) => a != b,
                (None, None) => false,
                _ => true,
            };
            widget_state.hovered_url = new_hover_url;
            // Under Privacy Mode a cell change can move the revealed
            // span even when no URL is involved, so repaint on any cell
            // change too (otherwise hovering an IP wouldn't reveal it).
            if hover_changed || url_changed || link_changed || (self.privacy && cell_changed) {
                return Some(CanvasAction::request_redraw());
            }
        None
    }

    /// A left press: the scrollbar first, then a Ctrl+click on a
    /// detected URL, then plain text selection.
    ///
    /// Split out of `on_event`, whose arm order is load-bearing;
    /// the guard that picks this stayed there.
    fn on_left_press(
        &self,
        widget_state: &mut TerminalWidgetState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<CanvasAction<Message>> {
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
                        widget_state.scroll_offset.get(),
                    ) && pos.x >= sb.track_x - 2.0
                        && pos.x <= sb.track_x + sb.track_w + 2.0
                        && pos.y >= sb.track_y
                        && pos.y <= sb.track_y + sb.track_h
                    {
                        let page = grid.screen_lines() as i32;
                        if pos.y >= sb.thumb_y && pos.y <= sb.thumb_y + sb.thumb_h {
                            widget_state.scrollbar_drag =
                                Some((pos.y, widget_state.scroll_offset.get()));
                        } else if pos.y < sb.thumb_y {
                            widget_state.scroll_offset
                                .set((widget_state.scroll_offset.get() + page).min(sb.history_size));
                        } else {
                            widget_state.scroll_offset
                                .set((widget_state.scroll_offset.get() - page).max(0));
                        }
                        return Some(CanvasAction::request_redraw().and_capture());
                    }
                }
                let (col, vrow) = self.pixel_to_cell(pos);
                let line = Self::visible_row_to_line(vrow, widget_state.scroll_offset.get());
                // Only follow URLs on Ctrl+Click, plain clicks
                // start a selection, matching Termius. Without
                // the modifier gate, every click on a logged URL
                // would lose the selection start.
                if widget_state.modifiers.control()
                    && let Ok(state) = self.state.lock()
                {
                    // Discriminator (shared by the hover + hint paths): a
                    // cell carrying an OSC 8 attribute follows OSC 8 rules
                    // ONLY, it opens iff its scheme is allowlisted and it
                    // NEVER falls back to a scraped URL. Otherwise a spoof
                    // whose visible label reads `https://real.com` but
                    // whose OSC 8 target is `javascript:...` would still
                    // open through the scraped arm. Only a cell with no
                    // explicit link is scraped for a literal `http(s)://`.
                    let target = match osc8_link_at_cell(&state.backend.term, line, col) {
                        Some((uri, _, _)) => osc8_scheme_allowed(&uri).then_some(uri),
                        None => url_at_cell(&state.backend.term, line, col),
                    };
                    drop(state);
                    if let Some(url) = target {
                        // Hand the URL to the app when it wants it: the
                        // confirmation prompt and the loopback-callback
                        // tunnel both need the PANE's session, which this
                        // crate can't see. The app also retires the hover
                        // hint from there, so the gesture still only
                        // publishes one message. Unwired (the SFTP
                        // console, the session player), the widget opens
                        // the URL itself exactly as before.
                        if let Some(cb) = &self.on_link_activate {
                            return Some(CanvasAction::publish(cb(url)).and_capture());
                        }
                        let _ = open_url(&url);
                        // Tell the app the gesture landed so the
                        // one-time hover hint can retire itself.
                        if let Some(msg) = self.on_link_opened.clone() {
                            return Some(CanvasAction::publish(msg).and_capture());
                        }
                        return Some(CanvasAction::capture());
                    }
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
        None
    }

    /// Run one of the widget-side gestures. Shared by the keyboard
    /// chord arm and the mouse-binding arm below, so a gesture behaves
    /// identically whichever input the user bound it to.
    fn perform_chord_action(
        &self,
        action: TerminalChordAction,
        widget_state: &mut TerminalWidgetState,
    ) -> Option<CanvasAction<Message>> {
        match action {
            TerminalChordAction::Copy => {
                if let Some(ref sel) = widget_state.selection
                    && !sel.is_empty()
                    && let Ok(state) = self.state.lock()
                {
                    let text = state.get_selection_text(sel);
                    if !text.is_empty() {
                        set_clipboard_text(&text);
                    }
                }
                Some(CanvasAction::capture())
            }
            // Keyboard twin of middle-click: paste the PRIMARY
            // selection (Shift+Insert by default, the xterm / kitty
            // `paste_from_selection` / Alacritty `PasteSelection`
            // convention). Reads the remembered text, not the live
            // highlight, so it still works after the keystrokes that
            // cleared the highlight ("select a path, type `cd `,
            // paste it"). Leaves the highlight alone, because in X11
            // a PRIMARY paste does not consume the selection, and
            // never touches the clipboard.
            //
            // Same fallbacks as middle-click, and for the same
            // reasons: under `copy_on_select` (the PuTTY
            // single-buffer model) every paste gesture reads the
            // clipboard, and a pane where nothing was ever selected
            // pastes the clipboard rather than dead-keying, which is
            // exactly what Shift+Insert did before this action
            // owned the chord.
            TerminalChordAction::PasteSelection => {
                // Same demote as middle-click: pasting consumes
                // the live highlight, the ghost carries on.
                widget_state.selection = None;
                widget_state.select_anchor = None;
                widget_state.selecting = false;
                // Where the platform owns a PRIMARY selection the host
                // resolves the text (system PRIMARY, then this pane's
                // remembered selection, then the clipboard), so publish
                // even with nothing remembered: the system buffer may hold
                // a selection made in another window, which is the whole
                // point of the gesture there. That holds under
                // `copy_on_select` too, whose single-buffer model is about
                // the CLIPBOARD, not about ignoring the desktop's own
                // selection. Everywhere else the buffer is ours alone: no
                // remembered text means the clipboard fallback below.
                let remembered = if crate::host_clipboard::has_primary_selection() {
                    Some(widget_state.primary_selection.clone().unwrap_or_default())
                } else if self.copy_on_select {
                    None
                } else {
                    widget_state.primary_selection.clone()
                };

                if let Some(text) = remembered
                    && let Some(to_message) = self.on_paste_selection.as_ref()
                {
                    return Some(CanvasAction::publish(to_message(text)).and_capture());
                }
                if let Some(msg) = self.on_paste_request.clone() {
                    return Some(CanvasAction::publish(msg).and_capture());
                }
                crate::host_clipboard::paste_into(Arc::clone(&self.state));
                Some(CanvasAction::capture())
            }
            // Selects the entire buffer (scrollback + screen); copy
            // stays a separate gesture (the copy chord, or
            // copy-on-select on the next release).
            TerminalChordAction::SelectAll => {
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
                Some(CanvasAction::request_redraw().and_capture())
            }
            TerminalChordAction::ScrollPageUp | TerminalChordAction::ScrollPageDown => {
                // One lock for the alt-screen test and the clamp,
                // like the wheel handler above.
                let (in_alt_screen, max_scroll, page) = match self.state.lock() {
                    Ok(s) => {
                        use alacritty_terminal::grid::Dimensions;
                        let in_alt = s
                            .backend
                            .term
                            .mode()
                            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);
                        let grid = s.backend.term.grid();
                        let screen = grid.screen_lines();
                        (
                            in_alt,
                            grid.total_lines().saturating_sub(screen) as i32,
                            // A page is a screen minus one row of
                            // overlap, the convention every terminal
                            // uses so a line stays visible across the
                            // jump. Never zero, or a short pane would
                            // stop paging.
                            (screen.saturating_sub(1)).max(1) as i32,
                        )
                    }
                    Err(_) => (false, i32::MAX, 1),
                };
                // No scrollback on the alternate screen: vim / less /
                // htop page themselves, so the key belongs to them.
                // The app's router skips these actions under the same
                // condition, which is what lets the key fall through
                // to the PTY writer. Both sides must agree: if only
                // one gated, PageUp would either be eaten with nothing
                // to show for it or fire twice.
                if in_alt_screen {
                    return None;
                }
                let lines = if matches!(action, TerminalChordAction::ScrollPageUp) {
                    page
                } else {
                    -page
                };
                widget_state.scroll_offset.set(
                    (widget_state.scroll_offset.get() + lines)
                        .max(0)
                        .min(max_scroll),
                );
                Some(CanvasAction::request_redraw().and_capture())
            }
        }
    }

    /// Whether the cursor sits in one of the strips this pane hands back
    /// to its container (see `resize_margins`).
    pub(super) fn cursor_in_resize_margin(
        &self,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> bool {
        let (top, right, bottom, left) = self.resize_margins;
        let Some(p) = cursor.position_in(bounds) else {
            return false;
        };
        (top > 0.0 && p.y <= top)
            || (bottom > 0.0 && p.y >= bounds.height - bottom)
            || (left > 0.0 && p.x <= left)
            || (right > 0.0 && p.x >= bounds.width - right)
    }

    pub(super) fn mouse_interaction_impl(
        &self,
        state: &TerminalWidgetState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !cursor.is_over(bounds) {
            return mouse::Interaction::default();
        }
        // Same handover for the cursor shape: staying neutral in the strip
        // lets the container's own resize cursor show through, which is
        // the only hint that the seam between two panes can be dragged.
        if self.cursor_in_resize_margin(bounds, cursor) {
            return mouse::Interaction::default();
        }
        // Pointer over the perf HUD panel: it's clickable (toggles the
        // compact / full-name labels), and the cursor change is the only
        // discoverability affordance a canvas layer can offer.
        if (self.perf_overlay || perf_overlay_enabled())
            && let Some(pos) = cursor.position_in(bounds)
            && state.hud_rect.get().is_some_and(|hud| hud.contains(pos))
        {
            return mouse::Interaction::Pointer;
        }
        // Pointer cursor over a URL, same as the browser hover affordance
        // and clear visual cue that "click does something different here".
        // Only when Ctrl is held does the click actually open the link.
        if state.hovered_url.is_some() {
            return mouse::Interaction::Pointer;
        }
        // Pointer over a privacy span (issue #78): hovering already
        // peeks, the pointer says a click pins the reveal.
        if self.privacy && state.hovered_privacy.get() {
            return mouse::Interaction::Pointer;
        }
        mouse::Interaction::Text
    }
}
