use super::*;

/// What one grid rebuild cost, handed from `draw_grid` to the perf HUD.
///
/// `built` is the honest verdict on the geometry cache: it can only be
/// true if the closure actually ran, so a cache hit leaves the default
/// here and the HUD reports zeros rather than the previous frame's work.
#[derive(Default, Clone, Copy)]
pub(super) struct DrawTimings {
    pub(super) built: bool,
    pub(super) lock: std::time::Duration,
    pub(super) cells: std::time::Duration,
    pub(super) highlights: std::time::Duration,
}

impl<Message> TerminalView<Message>
where
    Message: Clone,
{
    pub(super) fn render(
        &self,
        widget_state: &TerminalWidgetState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let perf_on = self.perf_overlay || perf_overlay_enabled();
        let draw_start = perf_on.then(std::time::Instant::now);

        let cell_w = self.cell_width;
        let cell_h = self.cell_height;

        // --- Cheap RenderKey gate: decide hit/miss before any snapshot ---
        // The key is built from the content epoch (one very short lock) plus
        // the view/widget flags that change what a grid draws. On a match we
        // skip the whole snapshot + glyph build and reuse the cached
        // geometry; on a miss we clear the cache so `Cache::draw` below
        // re-runs the closure. The perf HUD and the visual-bell flash are
        // NOT part of the key: both are drawn as their own fresh top layers.
        let (content_epoch, search_generation, pending_scroll) = {
            let s = match self.state.lock() {
                Ok(s) => s,
                Err(p) => p.into_inner(),
            };
            // Clamp the queued target to the current scrollback extent so
            // it stays valid after any resize/reflow between when it was
            // queued and this draw. Lets a caller pass `i32::MAX` to mean
            // "scroll to the very top" (the transcript viewer opens there)
            // and resolve it against the post-reflow line count; search
            // targets are already in range, so the clamp is a no-op for
            // them.
            let pending = s.pending_scroll.take().map(|target| {
                use alacritty_terminal::grid::Dimensions;
                let grid = s.backend.term.grid();
                let max = grid.total_lines().saturating_sub(grid.screen_lines()) as i32;
                target.clamp(0, max)
            });
            (s.render_epoch(), s.search_generation(), pending)
        };
        // A search step / open queued a scroll target (the active match's row);
        // apply it before the render key so this frame draws at that offset and
        // the match highlight lands in view. Consumed once (Cell::take).
        if let Some(target) = pending_scroll {
            widget_state.scroll_offset.set(target);
        }
        // PuTTY "reset scrollback on display activity": the render epoch
        // advances only on terminal output (process / sync-flush / palette),
        // never on scroll or cursor blink, so an epoch change since the last
        // draw means new activity. Jump to the live edge before the render
        // key is built so this frame draws at the bottom. Draw is `&self`,
        // hence the `Cell`s. Once-per-epoch by construction (the guard
        // updates `last_draw_epoch`), so the user can still scroll back
        // between two output batches and it sticks until the next one.
        if self.reset_scroll_on_output {
            let changed = widget_state
                .last_draw_epoch
                .get()
                .is_some_and(|e| e != content_epoch);
            if changed && widget_state.scroll_offset.get() != 0 {
                widget_state.scroll_offset.set(0);
            }
        }
        widget_state.last_draw_epoch.set(Some(content_epoch));
        let render_key = RenderKey {
            epoch: content_epoch,
            scroll_offset: widget_state.scroll_offset.get(),
            selection: widget_state.selection,
            // Unfocused panes hide the ghost (see the draw site below), so
            // the cache key has to agree or a pane keeps the cached image
            // that still has the band in it.
            ghost: if widget_state.selection.is_none() && self.focused {
                widget_state.primary_ghost.map(|(s, ..)| s)
            } else {
                None
            },
            hovered_url_cell: widget_state.hovered_url.as_ref().map(|(_, pos)| {
                (
                    ((pos.x - TERM_PAD) / cell_w).max(0.0) as u16,
                    ((pos.y - TERM_PAD_TOP) / cell_h).max(0.0) as u16,
                )
            }),
            hovered_link_spans: hash_link_spans(&widget_state.hovered_link_spans),
            hovered_cell: if self.privacy { widget_state.hovered_cell } else { None },
            hover: widget_state.hover,
            scrollbar_dragging: widget_state.scrollbar_drag.is_some(),
            selecting: widget_state.selecting,
            privacy: self.privacy,
            keyword_highlight: self.keyword_highlight,
            highlight_rules_hash: self.highlight_rules.hash(),
            performance: self.performance,
            smart_contrast: self.smart_contrast,
            bold_is_bright: self.bold_is_bright,
            transparent_bg: self.transparent_bg,
            text_dilation: self.text_dilation,
            privacy_terms_hash: if self.privacy { hash_terms(&self.privacy_terms) } else { 0 },
            privacy_classes: if self.privacy {
                self.privacy_classes
            } else {
                PrivacyClasses::default()
            },
            pinned_privacy_hash: if self.privacy {
                hash_pinned(&widget_state.pinned_privacy)
            } else {
                0
            },
            search_generation,
            font: self.font,
            font_size: self.font_size,
            cell_w,
            cell_h,
        };
        if widget_state.last_render_key.get() != Some(render_key) {
            widget_state.last_render_key.set(Some(render_key));
            widget_state.geometry_cache.clear();
        }

        // Per-phase timings, filled from inside the (possibly skipped)
        // closure so the perf HUD layer below can read them. They stay at
        // the default on a cache hit, which is precisely the signal that
        // the hit avoided the snapshot + build work: `built` can only be
        // true if the closure ran. That is truer than comparing render
        // keys, because the fork's `Cache::draw` also re-runs the closure
        // whenever `bounds` changed (a resize) even when our key matched.
        let mut timings = DrawTimings::default();

        let grid_geometry = widget_state.geometry_cache.draw(
            renderer,
            bounds.size(),
            |frame| timings = self.draw_grid(frame, widget_state, bounds, perf_on),
        );

        let mut geometries = vec![grid_geometry];

        // Perf HUD as its own always-fresh top layer (never cached), so the
        // fps / phase numbers and the cache hit/miss verdict update every
        // frame even while the grid below is served from the cache. Drawn
        // above the grid, its opaque panel covers the glyphs beneath it, so
        // the grid pass no longer needs to reserve those cells.
        if let Some(start) = draw_start {
            geometries.push(
                self.perf_hud_layer(renderer, widget_state, bounds, start, timings),
            );
        }

        // Visual bell: a brief translucent wash over the whole pane, its own
        // top layer so it sits above every glyph. A short timer in the app
        // clears `bell_flash`, ending the flash on the next frame.
        if self.bell_flash {
            // The grid foreground (used for the flash tint) lived inside the
            // cache closure; fetch it directly here.
            let flash_color = match self.state.lock() {
                Ok(s) => s.palette.foreground,
                Err(p) => p.into_inner().palette.foreground,
            };
            let mut flash = Frame::new(renderer, bounds.size());
            flash.fill_rectangle(
                Point::new(0.0, 0.0),
                bounds.size(),
                Color { a: 0.18, ..flash_color },
            );
            geometries.push(flash.into_geometry());
        }
        geometries
    }

    /// Build the grid geometry: snapshot the terminal state under the
    /// lock, detect the syntax / URL highlights, lay out the glyph runs
    /// and paint the cursor, the selection and the scrollbar.
    ///
    /// Runs inside the geometry cache's closure, so it is skipped
    /// entirely on a cache hit. Everything it needs is copied out of the
    /// state mutex up front and the lock is dropped before any geometry
    /// is built, so drawing never contends with `process()` on the
    /// output path.
    fn draw_grid(
        &self,
        frame: &mut Frame,
        widget_state: &TerminalWidgetState,
        bounds: Rectangle,
        perf_on: bool,
    ) -> DrawTimings {
        use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

        let cell_w = self.cell_width;
        let cell_h = self.cell_height;
        // No initial values: this body runs only when the cache missed,
        // and it assigns all three before returning. The neutral reading
        // a cache HIT reports is `DrawTimings::default()`, over in the
        // caller, which is also what keeps `built` honest.
        let lock_dur;
        let cells_dur;
        let built = true;
        let selection = &widget_state.selection;

        let mut cells: Vec<CellData> = DRAW_CELLS.take();
        cells.clear();
        let mut row_chars: Vec<(u16, Vec<(u16, char)>)> = Vec::new();
        // Buffer-search match spans clipped to the visible window, in
        // (visible_row, start_col, end_col_inclusive, is_active) form.
        // Snapshot under the lock so pass 2 can fill each match's cells
        // with a highlight background without re-touching the state.
        let mut search_spans: Vec<(u16, u16, u16, bool)> = Vec::new();

        // --- Snapshot phase, the only part that holds the state mutex ---
        // Everything draw needs (resolved cells, cursor, sizes, palette)
        // is copied out here and the lock is dropped before any text /
        // quad geometry is built, so drawing doesn't contend with
        // `process()` on the output path (see the typing-lag note on
        // `hovered_cell`).
        let lock_start = perf_on.then(std::time::Instant::now);
        let (
            palette,
            term_cursor,
            screen_lines,
            total_lines,
            in_alt_screen,
            scroll_offset,
            ghost,
            preedit,
            cols_count,
        ) = {
            let mut state = match self.state.lock() {
                Ok(s) => s,
                Err(poisoned) => poisoned.into_inner(),
            };
            lock_dur = lock_start.map(|t| t.elapsed()).unwrap_or_default();

            // Auto-resize. A fixed grid (replay surfaces) keeps the
            // backend's geometry: the recording drives resizes, and
            // the host sizes the canvas to match via
            // `grid_pixel_size`, so fitting bounds here would fight
            // the recorded resize events and reflow the replay.
            if !self.fixed_grid {
                let (new_cols, new_rows) = self.grid_size(bounds.width, bounds.height);
                state.resize(new_cols, new_rows);
            }

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
                let max_scroll = grid.total_lines().saturating_sub(grid.screen_lines()) as i32;
                widget_state.scroll_offset.get().clamp(0, max_scroll)
            };
            // Preserve the resolved viewport position for actions outside the
            // widget, such as the tab menu's visible-screen export.
            state.set_viewport_scroll_offset(scroll_offset);

            // Faint PRIMARY ghost: the demoted rectangle of the
            // last selection, shown only when no live highlight is
            // up. Suppressed in alt-screen (the region belongs to
            // the main grid the alt app is covering) and after a
            // resize or a rotation (both move the lines the range
            // points at). NOT gated on copy_on_select: the band
            // means "what you last selected", which under that
            // setting is what the clipboard holds, so it stays an
            // honest cue for the paste gestures in both modes
            // (modulo an external copy replacing the clipboard,
            // which nothing render-side can see).
            // Also suppressed on an UNFOCUSED pane. The band answers
            // "what you last selected", and the paste gestures it
            // hints at act on the pane you are in, so showing it in
            // three panes at once answers a question nobody asked
            // and reads like three live selections. The PRIMARY text
            // itself is untouched: middle-click paste still hands
            // back whichever pane you last selected in.
            let ghost: Option<Selection> = if selection.is_none()
                && !in_alt_screen
                && self.focused
            {
                widget_state
                    .primary_ghost
                    .filter(|(_, cols, total)| {
                        let grid = state.backend.term.grid();
                        *cols as usize == grid.columns()
                            && *total == grid.total_lines()
                    })
                    .map(|(s, ..)| s)
            } else {
                None
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

            // --- Buffer-search overlay spans ---
            // Translate each match's grid lines to visible rows
            // (visible_row = grid_line + scroll_offset) and clip to the
            // screen window. A match may wrap across grid lines, so a
            // multi-line hit yields one span per covered row with the
            // proper leading / middle / trailing column bounds.
            if let Some(search) = state.search.as_ref() {
                let last_col = cols_count.saturating_sub(1) as u16;
                for (i, m) in search.matches.iter().enumerate() {
                    let active = i == search.active;
                    for line in m.start_line..=m.end_line {
                        let vrow = line + scroll_offset;
                        if vrow < 0 || vrow >= screen_lines as i32 {
                            continue;
                        }
                        let sc = if line == m.start_line { m.start_col } else { 0 };
                        let ec = if line == m.end_line { m.end_col } else { last_col };
                        search_spans.push((vrow as u16, sc, ec, active));
                    }
                }
            }

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
                        && !ghost
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
                        underline: cell
                            .underline_color()
                            .map(|uc| palette.resolve(&uc, colors)),
                        link: cell.hyperlink().is_some(),
                    });
                }
                if !chars.is_empty() {
                    row_chars.push((visible_row as u16, chars));
                }
            }

            cells_dur = cells_start.map(|t| t.elapsed()).unwrap_or_default();

            (
                state.palette.clone(),
                term_cursor,
                screen_lines,
                total_lines,
                in_alt_screen,
                scroll_offset,
                ghost,
                state.preedit().to_string(),
                cols_count,
            )
        };
        let palette = &palette;

        // Skipped when something behind this canvas paints the backdrop:
        // the host container (translucent terminal, palette colour at
        // reduced alpha) or the `Backdrop` canvas (background picture).
        // Painting it here too would either composite two translucent
        // layers into a plate the user never asked for, or cover the
        // picture entirely. The picture itself CANNOT be drawn in this
        // frame: within one render layer images always draw on top of
        // every geometry fill regardless of call order, so a picture
        // here would bury the cell backgrounds, the selection, the
        // cursor and the scrollbar (see `widget/backdrop.rs`).
        if !self.transparent_bg {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), palette.background);
        }

        // --- Detect syntax highlights ---
        // Runs when keyword tinting OR Privacy Mode is on; the latter needs
        // the IP / user@host spans to mask even when tinting is off.
        // Performance mode suppresses the tinting scan, but NOT when
        // privacy is active: killing the scan there would unmask every
        // IP / user@host, so privacy always wins over the perf skip.
        let highlights_start = perf_on.then(std::time::Instant::now);
        let scan_for_tint = self.keyword_highlight && !self.performance;
        let highlights = if scan_for_tint || self.privacy {
            detect_highlights(
                &row_chars,
                palette,
                self.privacy,
                &self.privacy_terms,
                self.privacy_classes,
            )
        } else {
            Vec::new()
        };
        // The user's own rules, in their own list. They are gated only by
        // performance mode: a rule is an explicit request, so the
        // automatic detectors' toggle has no say over it.
        let rule_highlights = if !self.performance && !self.highlight_rules.is_empty() {
            detect_rule_highlights(&row_chars, self.highlight_rules.rules())
        } else {
            Vec::new()
        };
        let highlights_dur = highlights_start.map(|t| t.elapsed()).unwrap_or_default();

        // Privacy Mode: the IP / user@host span the cursor is over right
        // now (from the last hovered cell), revealed while the rest stay
        // masked. Mirrors `hovered_url_extent` but keyed off `hovered_cell`
        // so it works without the cursor being over a clickable link.
        let hovered_privacy_extent: Option<(u16, u16, u16)> = if self.privacy {
            widget_state
                .hovered_cell
                .and_then(|(col, vrow)| privacy_span_at(&highlights, vrow, col))
        } else {
            None
        };

        // Spans whose value the user click-pinned visible, resolved per
        // frame against the pinned set so every occurrence of the value
        // (including re-prints and scrolled copies) stays revealed until
        // clicked again.
        let pinned_extents: Vec<(u16, u16, u16)> =
            if self.privacy && !widget_state.pinned_privacy.is_empty() {
                privacy_spans_with_text(&highlights, &row_chars)
                    .into_iter()
                    .filter(|(_, text)| widget_state.pinned_privacy.contains(text))
                    .map(|(ext, _)| ext)
                    .collect()
            } else {
                Vec::new()
            };

        // Resolve which URL (if any) the cursor is over right now,
        // re-derived from the hovered cursor pixel position. We can't
        // trust the column we cached on hover because the grid may
        // have re-flowed since (resize, scroll). Drives the
        // "underline only the hovered URL" rule.
        let hovered_url_extent: Option<(u16, u16, u16)> = if let Some((_, pos)) =
            widget_state.hovered_url
        {
            let col = ((pos.x - TERM_PAD) / cell_w).max(0.0) as u16;
            let row = ((pos.y - TERM_PAD_TOP) / cell_h).max(0.0) as u16;
            hovered_url_range(&highlights, row, col)
        } else {
            None
        };
        // The hovered link's run was captured at hover time, across every
        // row it wraps onto: an OSC 8 link isn't in the regex highlight
        // scan at all, and a scraped URL is scanned one row at a time.
        let hovered_link_spans = &widget_state.hovered_link_spans;

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
        // Stroke widening: the glyph is stamped a second time shifted
        // `text_dilation` px right, so the union of two subpixel phases
        // covers what one leaves partial. Every glyph on the grid goes
        // through here, runs and singles alike, which is what keeps the
        // widening uniform; see `with_text_dilation` for why the raw
        // coverage needs it at all.
        let dilation = self.text_dilation;
        let stamp = move |frame: &mut Frame,
                          content: String,
                          position: Point,
                          color: Color,
                          font: Font| {
            if dilation > 0.0 {
                frame.fill_text(CanvasText {
                    content: content.clone(),
                    position: Point::new(position.x + dilation, position.y),
                    color,
                    size: Pixels(font_size),
                    font,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    ..Default::default()
                });
            }
            frame.fill_text(CanvasText {
                content,
                position,
                color,
                size: Pixels(font_size),
                font,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Top,
                ..Default::default()
            });
        };
        let flush_run = |frame: &mut Frame, run: GlyphRun| {
            stamp(
                frame,
                run.content,
                Point::new(
                    run.start_col as f32 * cell_w + TERM_PAD,
                    run.row as f32 * cell_h + TERM_PAD_TOP,
                ),
                run.fg,
                base_font,
            );
        };
        for cd in &cells {
            let x = cd.col as f32 * cell_w + TERM_PAD;
            let y = cd.row as f32 * cell_h + TERM_PAD_TOP;

            let mut fg = cd.fg;
            let mut bg = cd.bg;
            // The glyph actually drawn for this cell. Privacy Mode swaps it
            // for a block below; everything else draws the real character.
            let mut glyph = cd.c;

            if cd.flags.contains(CellFlags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cd.flags.contains(CellFlags::DIM) {
                fg = Color::from_rgba(fg.r * 0.66, fg.g * 0.66, fg.b * 0.66, fg.a);
            }

            // Syntax highlight override (only when text has default/foreground
            // color). A user rule wins over the automatic detectors: it was
            // asked for by name, they are a heuristic. The automatic half is
            // gated on `keyword_highlight` so Privacy Mode, which also
            // populates `highlights`, doesn't tint tokens when tinting is off.
            if let Some(hl_color) = highlight_color_at(&rule_highlights, cd.row, cd.col).or_else(
                || {
                    self.keyword_highlight
                        .then(|| highlight_color_at(&highlights, cd.row, cd.col))
                        .flatten()
                },
            ) {
                // Only override if the cell isn't already colored by the application
                let fg_is_default =
                    (fg.r - palette.foreground.r).abs() < 0.02
                    && (fg.g - palette.foreground.g).abs() < 0.02
                    && (fg.b - palette.foreground.b).abs() < 0.02;
                if fg_is_default {
                    fg = hl_color;
                }
            }

            // Explicit OSC 8 hyperlink: tint with the URL color (ansi blue),
            // same as a detected URL, but only when the app left the text at
            // the default foreground (don't fight an app that colored its own
            // link). Persistent, the hover underline is added separately.
            if cd.link {
                let fg_is_default = (fg.r - palette.foreground.r).abs() < 0.02
                    && (fg.g - palette.foreground.g).abs() < 0.02
                    && (fg.b - palette.foreground.b).abs() < 0.02;
                if fg_is_default {
                    fg = palette.ansi[4];
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
            } else if ghost
                .as_ref()
                .is_some_and(|s| Self::is_in_selection(s, cd.col, cell_line))
            {
                // PRIMARY ghost: the same hue as the live band at a
                // fraction of its weight, and the glyph colors stay
                // untouched, so it reads as a residue, not a
                // selection.
                bg = Color::from_rgba(0.133, 0.60, 0.569, 0.13);
            }

            // Buffer-search match background. Selection wins when both
            // cover a cell (the user can't be selecting and searching the
            // same cell in practice, but keep selection authoritative).
            // The active match gets a stronger amber; the rest a muted
            // one, matching the find-bar counter's "N of M" cue.
            if !is_selected && !search_spans.is_empty() {
                let hit = search_spans.iter().find(|&&(r, sc, ec, _)| {
                    cd.row == r && cd.col >= sc && cd.col <= ec
                });
                if let Some(&(_, _, _, active)) = hit {
                    bg = if active {
                        Color::from_rgba(0.98, 0.70, 0.10, 0.75)
                    } else {
                        Color::from_rgba(0.85, 0.62, 0.12, 0.35)
                    };
                    if active {
                        fg = Color::from_rgb(0.08, 0.07, 0.05);
                    }
                }
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

            // Privacy Mode masking: cells inside a privacy span (IP,
            // user@host, home-dir username, saved hostname) suppress
            // their glyph. Every cell of the span is masked,
            // separators included: a visible `.` / `@` / `:` would
            // reveal the value's shape (octet count, username
            // length). The redaction bar itself is drawn once per
            // SPAN after the cell loop (rounded rect + eye-slash,
            // issue #78), not per cell. The span the cursor hovers is
            // revealed (same hover-reveal as links), and click-pinned
            // values stay revealed.
            if self.privacy && is_privacy_cell(&highlights, cd.row, cd.col) {
                let in_extent = |&(r, sc, ec): &(u16, u16, u16)| {
                    cd.row == r && cd.col >= sc && cd.col <= ec
                };
                let revealed = hovered_privacy_extent.as_ref().is_some_and(in_extent)
                    || pinned_extents.iter().any(in_extent);
                if !revealed {
                    glyph = ' ';
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
            if glyph != ' ' && glyph != '\0' && glyph != '\t' {
                let cp = glyph as u32;
                // Both Private Use Areas: BMP PUA covers Powerline,
                // Font Awesome, Devicons, Octicons, Codicons and the
                // rest of the legacy Nerd Font ranges; SMP PUA is
                // where Nerd Font v3+ stuffed the Material Design
                // Icons. Regular fonts don't use either area, so we
                // can safely force the bundled Nerd Font across both.
                let is_pua =
                    (0xE000..=0xF8FF).contains(&cp) || (0xF0000..=0xFFFFD).contains(&cp);
                let is_wide = cd.flags.contains(CellFlags::WIDE_CHAR);
                if !is_pua && !is_wide && glyph.is_ascii_graphic() {
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
                        r.content.push(glyph);
                        r.next_col = cd.col + 1;
                    } else {
                        if let Some(r) = run.take() {
                            flush_run(frame, r);
                        }
                        run = Some(GlyphRun {
                            row: cd.row,
                            start_col: cd.col,
                            next_col: cd.col + 1,
                            fg,
                            content: glyph.to_string(),
                        });
                    }
                } else {
                    if let Some(r) = run.take() {
                        flush_run(frame, r);
                    }
                    let font = if is_pua { NERD_FONT } else { self.font };
                    stamp(frame, glyph.to_string(), Point::new(x, y), fg, font);
                }
            }

            // Underline, from explicit ANSI SGR flags, or for URL
            // cells that the cursor is currently hovering over (the
            // visual cue paired with the Pointer cursor).
            // Other URLs in the viewport stay un-underlined to avoid
            // looking like every link is independently clickable.
            let is_hovered_url = hovered_url_extent.is_some_and(|(r, sc, ec)| {
                cd.row == r && cd.col >= sc && cd.col <= ec
            }) || hovered_link_spans.iter().any(|&(r, sc, ec)| {
                cd.row == r && cd.col >= sc && cd.col <= ec
            });
            if cd.flags.intersects(CellFlags::ALL_UNDERLINES) || is_hovered_url {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                // SGR 58 underline color when set; the glyph's
                // (post-effects) foreground otherwise. The hover
                // underline of a URL keeps fg, it is our cue, not
                // the app's styling.
                let ul = if cd.flags.intersects(CellFlags::ALL_UNDERLINES) {
                    cd.underline.unwrap_or(fg)
                } else {
                    fg
                };
                frame.fill_rectangle(Point::new(x, y + cell_h - 2.0), Size::new(width, 1.0), ul);
            }

            // Strikethrough
            if cd.flags.contains(CellFlags::STRIKEOUT) {
                let width = if cd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
                frame.fill_rectangle(Point::new(x, y + cell_h / 2.0), Size::new(width, 1.0), fg);
            }
        }
        if let Some(r) = run.take() {
            flush_run(frame, r);
        }

        // Privacy redaction bars, one per span (issue #78): a rounded
        // bar reads as a deliberate censor mark instead of legitimate
        // reverse-video content, and spans wide enough carry a vector
        // eye-slash cut out of the bar, the "this is masked, hover to
        // peek" affordance no font glyph could give (the canvas draws
        // raw geometry, so there is no bundled-font dependency).
        // Masked cells drew no glyph above, so painting after the
        // text pass covers nothing. The vertical inset keeps stacked
        // masked lines from merging into a wall.
        if self.privacy {
            // Opaque tone blended toward the background, then
            // desaturated to neutral grey: keeping the theme hue makes
            // the mask mimic reverse-video content (on a teal theme it
            // reads as a highlight banner, not a censor mark).
            // Brightness is kept by re-encoding the blend's linear
            // luminance to sRGB.
            let blend = Color {
                r: palette.foreground.r * 0.45 + palette.background.r * 0.55,
                g: palette.foreground.g * 0.45 + palette.background.g * 0.55,
                b: palette.foreground.b * 0.45 + palette.background.b * 0.55,
                a: 1.0,
            };
            let lum = relative_luminance(blend);
            let grey = if lum <= 0.003_130_8 {
                lum * 12.92
            } else {
                1.055 * lum.powf(1.0 / 2.4) - 0.055
            };
            let bar_color = Color { r: grey, g: grey, b: grey, a: 1.0 };
            let mut any_masked = false;
            for ext in privacy_extents(&highlights) {
                let (row, start_col, end_col) = ext;
                let revealed = hovered_privacy_extent == Some(ext)
                    || pinned_extents.contains(&ext);
                if revealed {
                    continue;
                }
                any_masked = true;
                let inset = (cell_h * 0.12).clamp(1.0, 3.0);
                let bx = start_col as f32 * cell_w + TERM_PAD;
                let by = row as f32 * cell_h + TERM_PAD_TOP + inset;
                let bw = (end_col - start_col + 1) as f32 * cell_w;
                let bh = (cell_h - inset * 2.0).max(1.0);
                let radius = (bh * 0.30).clamp(2.0, 5.0);
                frame.fill(
                    &canvas::Path::rounded_rectangle(
                        Point::new(bx, by),
                        Size::new(bw, bh),
                        radius.into(),
                    ),
                    bar_color,
                );
                // Eye-slash, centered, in the theme's text color: the
                // bar leans toward the background, so the foreground
                // is the side with contrast headroom on every theme
                // (a background-colored cutout went near-invisible on
                // near-black themes, issue #78 follow-up). Only when
                // the span has room; short spans keep the bare bar.
                if bw >= cell_w * 5.0 && bh >= 8.0 {
                    let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
                    let eye_h = bh * 0.28;
                    let eye_w = (eye_h * 2.6).min(bw * 0.5);
                    let ink = canvas::Stroke {
                        style: canvas::stroke::Style::Solid(palette.foreground),
                        width: (bh * 0.10).clamp(1.0, 1.6),
                        line_cap: canvas::stroke::LineCap::Round,
                        ..canvas::Stroke::default()
                    };
                    let almond = canvas::Path::new(|b| {
                        b.move_to(Point::new(cx - eye_w / 2.0, cy));
                        b.quadratic_curve_to(
                            Point::new(cx, cy - eye_h * 2.0),
                            Point::new(cx + eye_w / 2.0, cy),
                        );
                        b.quadratic_curve_to(
                            Point::new(cx, cy + eye_h * 2.0),
                            Point::new(cx - eye_w / 2.0, cy),
                        );
                    });
                    frame.stroke(&almond, ink);
                    frame.fill(
                        &canvas::Path::circle(
                            Point::new(cx, cy),
                            (eye_h * 0.45).max(1.0),
                        ),
                        palette.foreground,
                    );
                    let slash = canvas::Path::line(
                        Point::new(cx - eye_w * 0.62, cy + eye_h * 1.5),
                        Point::new(cx + eye_w * 0.62, cy - eye_h * 1.5),
                    );
                    frame.stroke(&slash, ink);
                }
            }
            // First-mask signal for the app's one-shot hint toast
            // (issue #78): the draw pass has no message path, so a
            // process-wide flag the app swaps on its update loop is
            // the channel, same spirit as the bounds-reporter slots.
            if any_masked {
                PRIVACY_MASK_DRAWN.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            // Cache for `mouse_interaction`: pointer over a masked
            // (or just revealed) span signals "click pins the
            // reveal", the same affordance links get.
            widget_state
                .hovered_privacy
                .set(hovered_privacy_extent.is_some());
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
            // Paint the caret shape at an arbitrary cell origin, shared by
            // the normal caret and the end of an inline IME composition.
            let paint_cursor = |frame: &mut Frame, x: f32| match cursor.shape {
                CursorShape::Block => {
                    frame.fill_rectangle(
                        Point::new(x, cy),
                        Size::new(cell_w, cell_h),
                        Color { a: 0.7, ..palette.cursor },
                    );
                }
                CursorShape::Beam => {
                    frame.fill_rectangle(Point::new(x, cy), Size::new(2.0, cell_h), palette.cursor);
                }
                CursorShape::Underline => {
                    frame.fill_rectangle(
                        Point::new(x, cy + cell_h - 2.0),
                        Size::new(cell_w, 2.0),
                        palette.cursor,
                    );
                }
                _ => {
                    frame.fill_rectangle(
                        Point::new(x, cy),
                        Size::new(cell_w, cell_h),
                        Color { a: 0.5, ..palette.cursor },
                    );
                }
            };
            // Inline IME preedit: while a composition is active the caret
            // row shows the composed text in the terminal font, one glyph
            // per cell from the caret (CJK wide glyphs take two cells),
            // underlined to mark the composition region (Windows Terminal /
            // alacritty style). The caret moves to the end of the composed
            // text and the normal cursor is not drawn; the composition
            // clips at the right edge of the grid.
            if !preedit.is_empty() {
                let mut pen_x = cx;
                let mut end_col = cursor.point.column.0;
                for ch in preedit.chars() {
                    let w =
                        unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
                    if end_col + w > cols_count {
                        break;
                    }
                    // Semi-transparent backing so the composition stays
                    // readable over a colourful cell behind it (vim
                    // highlight, a selection, a bright ANSI background):
                    // the composed text is not part of the grid, so it
                    // must not inherit whatever the cell carries. RGB
                    // comes from the palette background (works on
                    // transparent backdrops too, alpha is ours).
                    frame.fill_rectangle(
                        Point::new(pen_x, cy),
                        Size::new(w as f32 * cell_w, cell_h),
                        Color { a: 0.45, ..palette.background },
                    );
                    stamp(
                        frame,
                        ch.to_string(),
                        Point::new(pen_x, cy),
                        palette.foreground,
                        self.font,
                    );
                    // Composition underline at the same height as the
                    // `CursorShape::Underline` caret, so the region reads
                    // as "being edited" rather than a separate element.
                    frame.fill_rectangle(
                        Point::new(pen_x, cy + cell_h - 2.0),
                        Size::new(w as f32 * cell_w, 2.0),
                        Color { a: 0.8, ..palette.foreground },
                    );
                    pen_x += w as f32 * cell_w;
                    end_col += w;
                }
                paint_cursor(frame, pen_x);
            } else {
                paint_cursor(frame, cx);
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
        DrawTimings {
            built,
            lock: lock_dur,
            cells: cells_dur,
            highlights: highlights_dur,
        }
    }

    /// The perf HUD, drawn as its own always-fresh top layer so the fps
    /// and phase numbers keep updating even while the grid below is
    /// served from the cache.
    ///
    /// Its panel is opaque and sits above the glyphs, which is why the
    /// grid pass does not have to reserve the cells underneath it.
    ///
    /// Publishes the panel's rectangle into `widget_state.hud_rect`. That
    /// is the hit-test `on_event` reads to send a press to the HUD's
    /// compact / full-name toggle instead of to the grid, so the write
    /// below is not bookkeeping: drop it and the panel stops being
    /// clickable.
    fn perf_hud_layer(
        &self,
        renderer: &Renderer,
        widget_state: &TerminalWidgetState,
        bounds: Rectangle,
        start: std::time::Instant,
        timings: DrawTimings,
    ) -> Geometry {
        let DrawTimings { built, lock: lock_dur, cells: cells_dur, highlights: highlights_dur } = timings;
        let total = start.elapsed();
        let now = std::time::Instant::now();

        #[allow(clippy::type_complexity)]
        let (
            (avg_total, avg_lock, avg_cells, avg_hl),
            (max_total, max_lock, max_cells, max_hl),
            (cache_pct, busy_pct, over_budget),
            spark_series,
        ): (_, _, (f32, f32, usize), Vec<f32>) = {
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
                built,
            });
            while stats.samples.len() > PERF_WINDOW {
                stats.samples.pop_front();
            }
            (
                (
                    stats.avg_total(),
                    stats.avg_lock(),
                    stats.avg_cells(),
                    stats.avg_highlights(),
                ),
                (
                    stats.max_total(),
                    stats.max_lock(),
                    stats.max_cells(),
                    stats.max_highlights(),
                ),
                (stats.cache_hit_pct(), stats.busy_pct(), stats.over_budget()),
                stats.total_series().collect(),
            )
        };

        // The grid palette lived inside the cache closure and isn't in
        // scope here; grab just the colors the HUD paints with (fg /
        // bg plus the theme's red and yellow for the threshold tints).
        let (hud_bg, hud_fg, hud_red, hud_amber) = match self.state.lock() {
            Ok(s) => (
                s.palette.background,
                s.palette.foreground,
                s.palette.ansi[1],
                s.palette.ansi[3],
            ),
            Err(p) => {
                let s = p.into_inner();
                (
                    s.palette.background,
                    s.palette.foreground,
                    s.palette.ansi[1],
                    s.palette.ansi[3],
                )
            }
        };

        // Frame-timing HUD pinned top-right, market-standard shape:
        // frame COST against the 60 Hz budget rather than fps (an
        // on-demand renderer's redraw cadence tracks activity, not
        // rendering speed, so fps here only ever measured how busy
        // the user was). Row 1 is the current frame (C 0.0 / H 0.0
        // reads as "cache hit skipped the snapshot + build") plus the
        // rolling cache hit-rate; row 2 averages the `PERF_WINDOW`
        // and adds `busy` (draw time over active wall-clock); row 3
        // is the window worst case plus `slow` (frames over budget,
        // the dropped-frame count), so transient spikes, the kind
        // that read as typing lag, stay visible long enough to spot.
        // The optional `net` row carries the session's link quality
        // (RTT probe window): on an SSH client the wire, not the
        // renderer, is what usually makes a session feel slow.
        // Values past their thresholds tint amber / red with the
        // theme's ANSI colors. Clicking the panel swaps the
        // single-letter keys for full metric names (issue #69's
        // tooltip ask; a canvas layer can't host
        // `iced::widget::tooltip`).
        let ms = |d: std::time::Duration| d.as_secs_f32() * 1000.0;
        let wide = hud_wide();
        let tint = |warn: bool, bad: bool| {
            if bad {
                hud_red
            } else if warn {
                hud_amber
            } else {
                hud_fg
            }
        };
        let total_tint = |d: std::time::Duration| tint(d > FRAME_WARN, d > FRAME_BUDGET);
        // Lock time is the typing-lag signal (draw contending with
        // the SSH output path), so it gets its own, tighter bar.
        let lock_tint = |d: std::time::Duration| {
            tint(d.as_secs_f32() > 0.002, d.as_secs_f32() > 0.008)
        };
        type Seg = (String, Color);
        let metrics = |t: std::time::Duration, l, c, h| -> Vec<Seg> {
            if wide {
                vec![
                    ("total".into(), hud_fg),
                    (format!("{:>6.1}ms", ms(t)), total_tint(t)),
                    ("  lock".into(), hud_fg),
                    (format!("{:>6.1}ms", ms(l)), lock_tint(l)),
                    (
                        format!("  cells{:>6.1}ms  highlight{:>6.1}ms", ms(c), ms(h)),
                        hud_fg,
                    ),
                ]
            } else {
                vec![
                    ("T".into(), hud_fg),
                    (format!("{:>5.1}", ms(t)), total_tint(t)),
                    ("  L".into(), hud_fg),
                    (format!("{:>4.1}", ms(l)), lock_tint(l)),
                    (format!("  C{:>4.1}  H{:>4.1}", ms(c), ms(h)), hud_fg),
                ]
            }
        };
        let label = |compact: &str, full: &str| -> Seg {
            if wide {
                (format!("{full:<9}"), hud_fg)
            } else {
                (format!("{compact:<5}"), hud_fg)
            }
        };
        let mut line1 = vec![label("curr", "current")];
        line1.extend(metrics(total, lock_dur, cells_dur, highlights_dur));
        line1.push((format!("   cache {cache_pct:>3.0}%"), hud_fg));
        let mut line2 = vec![label("avg", "average")];
        line2.extend(metrics(avg_total, avg_lock, avg_cells, avg_hl));
        line2.push((format!("   busy {busy_pct:>4.0}%"), hud_fg));
        let mut line3 = vec![label("peak", "peak")];
        line3.extend(metrics(max_total, max_lock, max_cells, max_hl));
        line3.push((
            format!(
                "   {} {over_budget}",
                if wide { "over-budget" } else { "slow" }
            ),
            tint(over_budget > 0, over_budget >= PERF_WINDOW / 20),
        ));
        let mut lines = vec![line1, line2, line3];
        if let Some(net) = self.net_hud {
            let mut line = vec![label("net", "network")];
            if let Some(silent) = net.silent_for_secs {
                // The mosh-style dead-link banner: the server has
                // stopped answering probes entirely.
                line.push((format!("no reply for {silent:.0}s"), hud_red));
            } else if let Some(rtt) = net.rtt_ms {
                let avg = net.avg_rtt_ms.unwrap_or(rtt);
                let peak = net.peak_rtt_ms.unwrap_or(rtt);
                let jit = net.jitter_ms.unwrap_or(0.0);
                // Interactive-SSH thresholds: a 100 ms echo is
                // noticeable, 250 ms is painful. Peak spikes are how
                // TCP loss manifests, hence their own (looser) bar.
                let rtt_tint = tint(rtt >= 100.0, rtt >= 250.0);
                let peak_tint = tint(peak >= 500.0, peak >= 1000.0);
                let jit_tint = tint(jit >= 30.0, jit >= 100.0);
                if wide {
                    line.push(("round-trip".into(), hud_fg));
                    line.push((format!("{rtt:>5.0}ms"), rtt_tint));
                    line.push((format!("  avg{avg:>5.0}ms"), hud_fg));
                    line.push(("  peak".into(), hud_fg));
                    line.push((format!("{peak:>5.0}ms"), peak_tint));
                    line.push(("  jitter".into(), hud_fg));
                    line.push((format!("{jit:>4.0}ms"), jit_tint));
                } else {
                    line.push(("rtt".into(), hud_fg));
                    line.push((format!("{rtt:>5.0}"), rtt_tint));
                    line.push((format!("  avg{avg:>5.0}"), hud_fg));
                    line.push(("  peak".into(), hud_fg));
                    line.push((format!("{peak:>5.0}"), peak_tint));
                    line.push(("  jit".into(), hud_fg));
                    line.push((format!("{jit:>4.0}"), jit_tint));
                    line.push(("  ms".into(), hud_fg));
                }
                if net.lost > 0 {
                    line.push((format!("  lost {}", net.lost), hud_red));
                }
            } else {
                line.push(("measuring...".into(), Color { a: 0.6, ..hud_fg }));
            }
            lines.push(line);
        }

        // Panel sized to the text it holds: the HUD renders with the
        // terminal's monospace font at a fixed 10 px, so the measured
        // advance at that size gives the exact line width (the old
        // fixed 300 px panel let long lines spill past its border and
        // the app edge, issue #69).
        const HUD_FONT_PX: f32 = 10.0;
        const HUD_PAD: f32 = 8.0;
        const HUD_LINE_H: f32 = 13.0;
        const SPARK_H: f32 = 18.0;
        let advance = cell_advance(self.font, HUD_FONT_PX);
        let chars = lines
            .iter()
            .map(|l| l.iter().map(|(s, _)| s.chars().count()).sum::<usize>())
            .max()
            .unwrap_or(0);
        let panel_w = (chars as f32 * advance).ceil() + HUD_PAD * 2.0;
        let text_top = 6.0;
        let spark_top = text_top + lines.len() as f32 * HUD_LINE_H + 3.0;
        let panel_h = spark_top + SPARK_H + 7.0;
        let panel = Rectangle::new(
            Point::new((bounds.width - panel_w - 8.0).max(0.0), 6.0),
            Size::new(panel_w, panel_h),
        );
        // Published for the click hit-test / pointer cursor in
        // `update` / `mouse_interaction`.
        widget_state.hud_rect.set(Some(panel));
        let border = Color { a: 0.5, ..hud_fg };
        let mut hud = Frame::new(renderer, bounds.size());
        hud.fill_rectangle(
            Point::new(panel.x, panel.y),
            Size::new(panel.width, panel.height),
            hud_bg,
        );
        hud.fill_rectangle(Point::new(panel.x, panel.y), Size::new(panel.width, 1.0), border);
        hud.fill_rectangle(
            Point::new(panel.x, panel.y + panel.height - 1.0),
            Size::new(panel.width, 1.0),
            border,
        );
        hud.fill_rectangle(Point::new(panel.x, panel.y), Size::new(1.0, panel.height), border);
        hud.fill_rectangle(
            Point::new(panel.x + panel.width - 1.0, panel.y),
            Size::new(1.0, panel.height),
            border,
        );
        for (i, segs) in lines.into_iter().enumerate() {
            let y = panel.y + text_top + i as f32 * HUD_LINE_H;
            let mut x = panel.x + HUD_PAD;
            for (content, color) in segs {
                // Monospace at a fixed size, so the advance times the
                // char count is the exact segment width.
                let w = content.chars().count() as f32 * advance;
                hud.fill_text(CanvasText {
                    content,
                    position: Point::new(x, y),
                    color,
                    size: Pixels(HUD_FONT_PX),
                    font: self.font,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    ..Default::default()
                });
                x += w;
            }
        }
        // Frame-time sparkline across the window, one slot per
        // sample, newest hugging the right edge; taller = costlier
        // frame. Scaled to the window's worst frame so healthy
        // sub-ms runs still use the full height and their shape is
        // readable (anchoring the scale at the 16.7 ms budget
        // squashed real-world 0.2-5 ms draws into 1 px dust). The
        // faint budget guide only appears once some frame climbs
        // into its range; bars crossing it (red) are dropped
        // frames, past half of it amber, so color, not height,
        // carries the absolute verdict.
        let spark_w = panel.width - HUD_PAD * 2.0;
        let spark_x = panel.x + HUD_PAD;
        let spark_y = panel.y + spark_top;
        let budget_ms = FRAME_BUDGET.as_secs_f32() * 1000.0;
        let scale = (ms(max_total) * 1.1).max(1.0);
        let bar_w = spark_w / PERF_WINDOW as f32;
        let n = spark_series.len();
        if budget_ms <= scale {
            let budget_y = spark_y + SPARK_H - (budget_ms / scale) * SPARK_H;
            hud.fill_rectangle(
                Point::new(spark_x, budget_y),
                Size::new(spark_w, 1.0),
                Color { a: 0.3, ..hud_fg },
            );
        }
        for (i, cost) in spark_series.into_iter().enumerate() {
            let h = ((cost / scale).clamp(0.0, 1.0) * SPARK_H).max(1.0);
            let color = if cost > budget_ms {
                hud_red
            } else if cost > budget_ms / 2.0 {
                hud_amber
            } else {
                hud_fg
            };
            hud.fill_rectangle(
                Point::new(spark_x + spark_w - (n - i) as f32 * bar_w, spark_y + SPARK_H - h),
                Size::new((bar_w - 0.5).max(0.5), h),
                Color { a: 0.65, ..color },
            );
        }
        hud.fill_rectangle(
            Point::new(spark_x, spark_y + SPARK_H),
            Size::new(spark_w, 1.0),
            Color { a: 0.25, ..hud_fg },
        );
        hud.into_geometry()
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
