use super::*;

impl<Message> TerminalView<Message> {
    pub fn new(state: Arc<Mutex<TerminalState>>) -> Self {
        let font_size = 14.0;
        Self {
            state,
            chords: None,
            chords_unfocused: false,
            font_size,
            cell_width: cell_advance(Font::MONOSPACE, font_size),
            cell_height: font_size * 1.15,
            font: Font::MONOSPACE,
            text_dilation: 0.0,
            copy_on_select: true,
            right_click_copy: false,
            mouse_bindings: None,
            right_click_action: RightClickAction::default(),
            reset_scroll_on_output: false,
            bold_is_bright: true,
            keyword_highlight: true,
            highlight_rules: std::sync::Arc::default(),
            performance: false,
            perf_overlay: false,
            net_hud: None,
            privacy: false,
            privacy_terms: Vec::new(),
            privacy_classes: PrivacyClasses::default(),
            smart_contrast: true,
            transparent_bg: false,
            mouse_reporting: true,
            word_delimiters: crate::backend::DEFAULT_WORD_DELIMITERS.to_string(),
            on_font_size_increase: None,
            on_font_size_decrease: None,
            on_paste_request: None,
            on_paste_selection: None,
            on_context_menu: None,
            on_terminal_input: None,
            on_mouse_capture_hint: None,
            on_link_click_hint: None,
            on_link_opened: None,
            on_link_activate: None,
            focused: true,
            resize_margins: (0.0, 0.0, 0.0, 0.0),
            bell_flash: false,
            fixed_grid: false,
        }
    }

    /// Mark whether this pane is focused. Only the focused pane emits
    /// mouse-tracking reports (see the `focused` field).
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Per-edge strip this pane hands back to its container, in pixels:
    /// `(top, right, bottom, left)`. See `resize_margins` on the widget.
    /// Only set a non-zero value on an edge that borders a sibling pane;
    /// on an outer edge it would just eat selectable area.
    pub fn with_resize_margins(mut self, margins: (f32, f32, f32, f32)) -> Self {
        self.resize_margins = margins;
        self
    }

    /// Show the visual-bell flash overlay this frame.
    pub fn with_bell_flash(mut self, on: bool) -> Self {
        self.bell_flash = on;
        self
    }

    /// Pin the grid to the backend's current geometry instead of
    /// auto-fitting the canvas bounds (see the `fixed_grid` field).
    pub fn with_fixed_grid(mut self, on: bool) -> Self {
        self.fixed_grid = on;
        self
    }

    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        // Recompute from the current font so the result is correct regardless
        // of whether `with_font_name` ran before or after this setter.
        self.cell_width = cell_advance(self.font, size);
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

    /// Wire the user's chord bindings for copy / select-all /
    /// scrollback paging. See [`ChordResolver`].
    pub fn with_terminal_chords(mut self, resolver: ChordResolver) -> Self {
        self.chords = Some(resolver);
        self
    }

    /// Let the chords fire on a widget that is never rendered focused
    /// (see the `chords_unfocused` field). Only correct for a surface
    /// that is the only terminal on screen.
    pub fn with_chords_unfocused(mut self, on: bool) -> Self {
        self.chords_unfocused = on;
        self
    }

    /// Wire the user's MOUSE bindings (X11-style middle-click paste out
    /// of the box). See [`MouseResolver`].
    pub fn with_mouse_bindings(mut self, resolver: MouseResolver<Message>) -> Self {
        self.mouse_bindings = Some(resolver);
        self
    }

    /// Set the right-click scheme (Menu / Paste / Extend).
    pub fn with_right_click_action(mut self, action: RightClickAction) -> Self {
        self.right_click_action = action;
        self
    }

    /// PuTTY "reset scrollback on display activity": jump to the live
    /// edge on new terminal output.
    pub fn with_reset_scroll_on_output(mut self, on: bool) -> Self {
        self.reset_scroll_on_output = on;
        self
    }

    /// Wire the context-menu request (fired on right-click when the
    /// scheme is `Menu`). `f` receives window-absolute (x, y) and the
    /// live selection's text (`None` when there is no selection).
    pub fn on_context_menu(
        mut self,
        f: impl Fn(f32, f32, Option<String>) -> Message + 'static,
    ) -> Self {
        self.on_context_menu = Some(Box::new(f));
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

    /// Hand the background fill to whatever sits behind this widget:
    /// the host container (translucent terminal) or the [`Backdrop`]
    /// canvas (background picture). Pass `true` only when that layer
    /// really paints it; see `transparent_bg` on the widget for why the
    /// two must never both paint it.
    pub fn with_transparent_bg(mut self, on: bool) -> Self {
        self.transparent_bg = on;
        self
    }

    /// Whether remote mouse tracking is honoured (C5). Pass `false` for a
    /// host with `disable_mouse_reporting` so clicks always select / paste
    /// locally regardless of the remote's mouse mode.
    pub fn with_mouse_reporting(mut self, on: bool) -> Self {
        self.mouse_reporting = on;
        self
    }

    pub fn with_privacy(mut self, on: bool) -> Self {
        self.privacy = on;
        self
    }

    /// Per-class Privacy Mode gates (issue #78): which detector
    /// classes may mask. No-op while privacy is off.
    pub fn with_privacy_classes(mut self, classes: PrivacyClasses) -> Self {
        self.privacy_classes = classes;
        self
    }

    /// Extra strings Privacy Mode must mask wherever they appear, on top
    /// of the shape-based IP / `user@host` / home-dir detection. The app
    /// passes the vault's saved hostnames so plain DNS names are hidden
    /// too. Stored lowercase (matching is case-insensitive and
    /// token-bounded); very short terms are dropped, masking every "web"
    /// or "db1" in sight would be noise, not privacy. No-op while
    /// privacy is off.
    pub fn with_privacy_terms(mut self, terms: &[String]) -> Self {
        self.privacy_terms = terms
            .iter()
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| t.len() >= 4)
            .collect();
        self
    }

    pub fn with_keyword_highlight(mut self, on: bool) -> Self {
        self.keyword_highlight = on;
        self
    }

    /// The user's own highlight rules. See
    /// [`TerminalView::highlight_rules`]; pass the same set the pane's
    /// backend was given.
    pub fn with_highlight_rules(
        mut self,
        rules: std::sync::Arc<crate::highlight_rules::CompiledRules>,
    ) -> Self {
        self.highlight_rules = rules;
        self
    }

    /// Performance mode. See [`TerminalView::performance`].
    pub fn with_performance(mut self, on: bool) -> Self {
        self.performance = on;
        self
    }

    /// Show the per-pane perf HUD (also forced by `ORYXIS_TERM_PERF`).
    pub fn with_perf_overlay(mut self, on: bool) -> Self {
        self.perf_overlay = on;
        self
    }

    /// Link-quality figures for the perf HUD's `net` row. `None` omits
    /// the row (panes without a probed transport).
    pub fn with_net_hud(mut self, net: Option<NetHud>) -> Self {
        self.net_hud = net;
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
    /// Wire the "that link needs Ctrl + Click" hint. The callback fires
    /// when a plain click (no Ctrl, no drag) lands on a URL, so the app
    /// can show a transient toast teaching the gesture at the moment it
    /// missed (one-time onboarding, see `on_link_opened`).
    pub fn on_link_click_hint(mut self, f: impl Fn() -> Message + 'static) -> Self {
        self.on_link_click_hint = Some(Box::new(f));
        self
    }

    /// Message emitted after a Ctrl+Click opens a URL. Ignored while
    /// [`Self::on_link_activate`] is wired, which takes over the open.
    pub fn on_link_opened(mut self, msg: Message) -> Self {
        self.on_link_opened = Some(msg);
        self
    }

    /// Wire the link ACTIVATION (Ctrl+Click on a URL) to the app,
    /// which then owns opening it.
    ///
    /// The closure receives the resolved target: an allowlisted OSC 8
    /// URI, or the literal `http(s)://` token scraped from the grid
    /// (soft-wrapped rows joined). A host that wires this must open the
    /// URL itself and retire the Ctrl+click hint the way
    /// [`Self::on_link_opened`] would have.
    pub fn on_link_activate(mut self, f: impl Fn(String) -> Message + 'static) -> Self {
        self.on_link_activate = Some(Box::new(f));
        self
    }

    pub fn on_paste_request(mut self, msg: Message) -> Self {
        self.on_paste_request = Some(msg);
        self
    }

    /// Wire the PRIMARY-selection paste (middle-click and the
    /// paste-selection action). The closure receives the remembered
    /// selection text; the app pastes it into this pane, since only the
    /// app reaches an SSH session and owns the paste guards. A closure
    /// (not a plain `Message`) because the text is only known here.
    pub fn on_paste_selection(mut self, f: impl Fn(String) -> Message + 'static) -> Self {
        self.on_paste_selection = Some(Box::new(f));
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

    /// Wire the "mouse tracking is swallowing your selection" hint. The
    /// callback fires once per pane, on the first left-drag while the
    /// remote app holds the mouse, so the app can show a transient
    /// "hold Shift to select" toast at the moment it's relevant.
    pub fn on_mouse_capture_hint(mut self, f: impl Fn() -> Message + 'static) -> Self {
        self.on_mouse_capture_hint = Some(Box::new(f));
        self
    }

    /// Override the font used for cell rendering. If the font can't be resolved
    /// by cosmic-text, it falls back to the system default monospace.
    pub fn with_font_name(mut self, name: &str) -> Self {
        // Keep whatever weight was already set: the two setters are
        // independent and either order must end up at the same font.
        self.font = Font {
            weight: self.font.weight,
            ..Font::new(intern_font_name(name))
        };
        // The cell width depends on the font's advance; recompute it now that
        // the family changed (the width comes from the real metric, not a
        // fixed ratio, so a different font means a different cell width).
        self.cell_width = cell_advance(self.font, self.font_size);
        self
    }

    /// Widen every stroke by re-stamping each glyph shifted `px`
    /// logical pixels to the right (0 disables it).
    ///
    /// This is the stroke widening the platform text stacks apply and
    /// ours does not: macOS runs its glyphs through Core Graphics with
    /// font smoothing on by default, which (in crossfont's own words,
    /// the font crate alacritty uses) "increases the stroke width".
    /// swash rasterizes raw coverage, so the same file at the same size
    /// lands thinner here than in a terminal that renders through the
    /// OS. Stamping the glyph twice a fraction of a pixel apart is the
    /// same operation: the union of two subpixel phases fills what one
    /// leaves at partial coverage.
    ///
    /// It also hides an unevenness of our own. A merged run is laid out
    /// by the shaper at the font's fractional advance, so the same
    /// character lands on a different subpixel phase per column: a
    /// vertical stem measured 0.61 to 1.00 of full coverage depending
    /// on where in the row it fell. The second stamp lifts the low
    /// phases without touching the ones already crisp (measured 0.85 ->
    /// 0.97 mean peak at 0.3 px).
    pub fn with_text_dilation(mut self, px: f32) -> Self {
        self.text_dilation = px.max(0.0);
        self
    }

    /// Override the weight cells are rendered at (issue #155).
    ///
    /// Only a weight the resolved family actually ships changes
    /// anything: cosmic-text has no synthetic emboldening, so a
    /// request a family can't serve resolves to its nearest face. The
    /// app's font picker is what tells the user when that happens; the
    /// widget just asks.
    pub fn with_font_weight(mut self, weight: iced::font::Weight) -> Self {
        self.font.weight = weight;
        // Same reason as the family: a heavier face may advance
        // differently, and the grid is laid out from the measurement.
        self.cell_width = cell_advance(self.font, self.font_size);
        self
    }
}
