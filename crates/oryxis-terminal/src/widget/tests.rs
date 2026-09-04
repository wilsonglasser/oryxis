    use super::*;

    fn view_and_state() -> (TerminalView<()>, TerminalWidgetState) {
        let term = TerminalState::new_no_pty(80, 24).unwrap();
        let view = TerminalView::new(Arc::new(Mutex::new(term)));
        (view, TerminalWidgetState::default())
    }

    fn bounds() -> Rectangle {
        Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 480.0))
    }

    /// SGR click tracking (mc, htop). Regression for the "must hold
    /// Shift to click the sidebar" report: a release whose press was
    /// never reported (it landed on a sibling widget, so the cursor is
    /// outside the canvas and no press is tracked) must NOT be consumed
    /// by the report path; capturing it starves sibling `button`s,
    /// which fire on release.
    #[test]
    fn untracked_release_is_not_reported() {
        use alacritty_terminal::term::TermMode;
        let (view, mut ws) = view_and_state();
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let ev = iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        // Cursor over the sidebar (outside the canvas), no tracked press.
        let cursor = mouse::Cursor::Available(Point::new(2000.0, 100.0));
        assert!(ws.report_button.is_none());
        let action = view.handle_mouse_report(&mut ws, &ev, bounds(), cursor, mode, 80, 24);
        assert!(action.is_none(), "sidebar release must stay local");
    }

    /// The other half of issue #150: with the remote app holding mouse
    /// tracking (tmux `mouse on`, htop), a high-resolution wheel's
    /// fragments must accumulate into whole detents here too. Reporting
    /// each fragment as a notch — what `ceil()` alone did — scrolled the
    /// remote app eight times per click of the wheel. A residual-only
    /// fragment is still CONSUMED (publishing nothing): while the app
    /// holds tracking the wheel belongs to the report path, and falling
    /// through would hand the fragment to the local-scrollback arm,
    /// which shares the residual and would double-count it.
    #[test]
    fn fractional_line_wheel_reports_one_notch_per_detent() {
        use alacritty_terminal::term::TermMode;
        let (view, mut ws) = view_and_state();
        let view = view.on_terminal_input(|_| ());
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));
        let frag = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 0.125 },
        });

        for _ in 0..7 {
            let action = view
                .handle_mouse_report(&mut ws, &frag, bounds(), cursor, mode, 80, 24)
                .expect("a partial detent is still consumed by the report path");
            let (msg, _, _) = action.into_inner();
            assert!(msg.is_none(), "a partial detent reports nothing");
        }
        let action = view
            .handle_mouse_report(&mut ws, &frag, bounds(), cursor, mode, 80, 24)
            .expect("the completed detent is consumed");
        let (msg, _, _) = action.into_inner();
        assert!(msg.is_some(), "the completed detent reports once");
    }

    /// The touchpad twin: `ScrollDelta::Pixels` fragments arrive a few
    /// pixels at a time, below one cell, and must accumulate on the
    /// cell scale before reporting. Ceiling each fragment to a notch
    /// flooded a tracking TUI with several times the gesture (a slow
    /// two-finger scroll became ~30 wheel reports where ~6 lines were
    /// scrolled), while the same gesture over local scrollback, which
    /// already accumulated, scrolled correctly.
    #[test]
    fn fractional_pixel_wheel_reports_whole_cells_only() {
        use alacritty_terminal::term::TermMode;
        let (view, mut ws) = view_and_state();
        let view = view.on_terminal_input(|_| ());
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));
        // Four fragments of a quarter-cell each: only the fourth
        // completes a cell and may report.
        let frag = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: view.cell_height / 4.0 },
        });

        for _ in 0..3 {
            let action = view
                .handle_mouse_report(&mut ws, &frag, bounds(), cursor, mode, 80, 24)
                .expect("a sub-cell fragment is still consumed by the report path");
            let (msg, _, _) = action.into_inner();
            assert!(msg.is_none(), "a sub-cell fragment reports nothing");
        }
        let action = view
            .handle_mouse_report(&mut ws, &frag, bounds(), cursor, mode, 80, 24)
            .expect("the completed cell is consumed");
        let (msg, _, _) = action.into_inner();
        assert!(msg.is_some(), "the completed cell reports once");
    }

    /// The canvas-originated press → drag off-canvas → release flow must
    /// still report the release (apps need the button-up to end a drag),
    /// falling back to the last reported cell.
    #[test]
    fn tracked_release_still_reports_after_leaving_canvas() {
        use alacritty_terminal::term::TermMode;
        let (view, mut ws) = view_and_state();
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;

        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let inside = mouse::Cursor::Available(Point::new(40.0, 40.0));
        let action = view.handle_mouse_report(&mut ws, &press, bounds(), inside, mode, 80, 24);
        assert!(action.is_some(), "on-canvas press must be reported");
        assert_eq!(ws.report_button, Some(ReportButton::Left));

        let release = iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let outside = mouse::Cursor::Available(Point::new(2000.0, 100.0));
        let action = view.handle_mouse_report(&mut ws, &release, bounds(), outside, mode, 80, 24);
        assert!(action.is_some(), "release of a reported press must land");
        assert!(ws.report_button.is_none(), "press tracking cleared on release");
    }

    /// Pressing Shift AFTER a reported press must not swallow the
    /// release: `release_completes_tracked_press` lets it through the
    /// Shift bypass, so the app gets its button-up and `report_button`
    /// clears instead of sticking at `Some(Left)` (phantom held button,
    /// every later motion misread as a drag).
    #[test]
    fn shift_at_release_does_not_swallow_tracked_release() {
        use alacritty_terminal::term::TermMode;
        let (view, mut ws) = view_and_state();
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;

        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let inside = mouse::Cursor::Available(Point::new(40.0, 40.0));
        let action = view.handle_mouse_report(&mut ws, &press, bounds(), inside, mode, 80, 24);
        assert!(action.is_some(), "press without Shift must be reported");
        assert_eq!(ws.report_button, Some(ReportButton::Left));

        // Shift lands between press and release.
        ws.modifiers = iced::keyboard::Modifiers::SHIFT;
        let release = iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        assert!(
            TerminalView::<()>::release_completes_tracked_press(&ws, &release),
            "tracked release must pierce the Shift bypass"
        );
        let action = view.handle_mouse_report(&mut ws, &release, bounds(), inside, mode, 80, 24);
        assert!(action.is_some(), "release of a tracked press reports despite Shift");
        assert!(ws.report_button.is_none(), "press tracking cleared on release");
    }

    /// The Shift bypass must keep blocking NEW gestures: with no
    /// tracked press, neither a Shift+press nor its release qualifies
    /// as completing a tracked press, so local selection stays in
    /// charge for the whole gesture.
    #[test]
    fn shift_bypass_still_blocks_new_gestures() {
        let (_view, ws) = view_and_state();
        assert!(ws.report_button.is_none());
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        assert!(
            !TerminalView::<()>::release_completes_tracked_press(&ws, &press),
            "a press never qualifies"
        );
        assert!(
            !TerminalView::<()>::release_completes_tracked_press(&ws, &release),
            "a release with no tracked press never qualifies"
        );
    }

    /// `right_click_copy` is a Paste-scheme sub-option: a stale `true`
    /// under Menu / Extend (Settings hides the toggle there, so the
    /// user can't see or clear it) must not defer, i.e. suppress, the
    /// copy-on-select auto-copy.
    #[test]
    fn right_click_copy_only_defers_auto_copy_under_paste_scheme() {
        let (view, _) = view_and_state();
        let paste = view.with_right_click_copy(true).with_right_click_action(RightClickAction::Paste);
        assert!(paste.defers_copy_to_right_click(), "Paste scheme honours the deferral");

        let (view, _) = view_and_state();
        let menu = view.with_right_click_copy(true).with_right_click_action(RightClickAction::Menu);
        assert!(!menu.defers_copy_to_right_click(), "stale flag under Menu must not defer");

        let (view, _) = view_and_state();
        let extend = view.with_right_click_copy(true).with_right_click_action(RightClickAction::Extend);
        assert!(!extend.defers_copy_to_right_click(), "stale flag under Extend must not defer");

        let (view, _) = view_and_state();
        let off = view.with_right_click_action(RightClickAction::Paste);
        assert!(!off.defers_copy_to_right_click(), "flag off never defers");
    }

    /// Build a view over a terminal with `lines` rows of scrollback, so
    /// there is somewhere to scroll to.
    fn scrolled_view(lines: usize) -> (TerminalView<()>, TerminalWidgetState) {
        let mut term = TerminalState::new_no_pty(80, 24).unwrap();
        for _ in 0..lines {
            term.process(b"line\r\n");
        }
        (
            TerminalView::new(Arc::new(Mutex::new(term))),
            TerminalWidgetState::default(),
        )
    }

    /// A scrolled-back terminal driven only by `ScrollDelta::Pixels`
    /// deltas smaller than one cell (Windows precision touchpads and
    /// high-res wheels deliver a few pixels per notch): the pre-#91
    /// handler floored each `y / cell_height` to zero, so scrollback
    /// never moved and the transcript viewer (no output to snap it back)
    /// was frozen. The residual accumulator now carries the sub-cell
    /// remainder across events and emits a whole line once the pixels
    /// cross a cell.
    #[test]
    fn subcell_pixel_wheel_accumulates_into_scroll() {
        let (view, mut ws) = scrolled_view(200);
        // Cursor over the canvas; start at the live edge (offset 0).
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));
        assert_eq!(ws.scroll_offset.get(), 0);

        // cell_height defaults to 14.0 * 1.15 = 16.1, so a 10px notch is
        // sub-cell: one alone must not move (correct), but the second
        // crosses a cell boundary and advances exactly one line, where
        // the old truncation stayed pinned at zero forever.
        let notch = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: 10.0 },
        });
        let action = view.on_event(&mut ws, &notch, bounds(), cursor);
        assert!(action.is_some(), "the canvas consumes the wheel event");
        assert_eq!(ws.scroll_offset.get(), 0, "one sub-cell notch must not move");

        view.on_event(&mut ws, &notch, bounds(), cursor);
        assert_eq!(ws.scroll_offset.get(), 1, "two sub-cell notches cross a cell");

        // Five more keep it climbing, proving the residual never stalls.
        for _ in 0..5 {
            view.on_event(&mut ws, &notch, bounds(), cursor);
        }
        assert!(
            ws.scroll_offset.get() >= 4,
            "sub-cell pixel wheel keeps advancing, got {}",
            ws.scroll_offset.get()
        );
    }

    /// A `ScrollDelta::Lines` notch still moves whole lines and clears
    /// any carried pixel residual, so switching devices (touchpad →
    /// discrete wheel) can't leave a stale sub-cell fraction fighting the
    /// next notch.
    #[test]
    fn line_wheel_moves_and_clears_pixel_residual() {
        let (view, mut ws) = scrolled_view(200);
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));

        // Leave a sub-cell residual behind from a pixel notch.
        let px = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: 10.0 },
        });
        view.on_event(&mut ws, &px, bounds(), cursor);
        assert_ne!(ws.scroll_px_residual.get(), 0.0, "pixel notch left a residual");

        // A line notch scrolls 3 lines (y * 3) and wipes the residual.
        let ln = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });
        view.on_event(&mut ws, &ln, bounds(), cursor);
        assert_eq!(ws.scroll_offset.get(), 3, "one line notch scrolls 3 lines");
        assert_eq!(ws.scroll_px_residual.get(), 0.0, "line notch clears the residual");
    }

    /// A high-resolution wheel reports FRACTIONS of a detent, and the
    /// platform hands them through as `ScrollDelta::Lines` on the same
    /// 120-per-detent scale it already divided out: Wayland's
    /// `axis_value120` (which winit only started honouring once the
    /// toolkit began binding `wl_seat` v9, in the 0.31 bump that shipped
    /// with 0.13.0) and Windows' `WM_MOUSEWHEEL`. `y as i32` truncated
    /// every fragment to zero and then swallowed the event, so the wheel
    /// did nothing at all on those devices (issue #150). The notch
    /// residual accumulates them into whole detents instead.
    #[test]
    fn fractional_line_wheel_accumulates_into_scroll() {
        let (view, mut ws) = scrolled_view(200);
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));
        assert_eq!(ws.scroll_offset.get(), 0);

        // An eighth of a detent, the `value120 = 15` fragment.
        let frag = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 0.125 },
        });
        for _ in 0..7 {
            let action = view.on_event(&mut ws, &frag, bounds(), cursor);
            assert!(action.is_some(), "the canvas consumes every fragment");
        }
        assert_eq!(ws.scroll_offset.get(), 0, "a partial detent must not move");

        // The eighth fragment completes the detent: 3 lines, once.
        view.on_event(&mut ws, &frag, bounds(), cursor);
        assert_eq!(ws.scroll_offset.get(), 3, "a whole detent scrolls 3 lines");

        // And it keeps going, which is what the truncation never did.
        for _ in 0..8 {
            view.on_event(&mut ws, &frag, bounds(), cursor);
        }
        assert_eq!(ws.scroll_offset.get(), 6, "the residual never stalls");
    }

    /// A direction reversal mid-detent responds on its first fragment
    /// instead of spending it unwinding the accumulated one; a
    /// horizontal-only event (a tilt wheel, `y == 0.0`) is NOT a
    /// reversal and must leave the vertical residual alone.
    #[test]
    fn fractional_line_wheel_reversal_and_tilt() {
        let (view, mut ws) = scrolled_view(200);
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));
        let wheel = |y: f32, x: f32| {
            iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x, y },
            })
        };

        // Half a detent up, then a tilt: the residual survives it.
        view.on_event(&mut ws, &wheel(0.5, 0.0), bounds(), cursor);
        view.on_event(&mut ws, &wheel(0.0, 1.0), bounds(), cursor);
        assert_eq!(ws.scroll_line_residual.get(), 0.5, "a tilt is not a reversal");

        // Reversing drops the stale residual, so the next half-detent
        // down is a fresh 0.5 rather than cancelling back to zero.
        view.on_event(&mut ws, &wheel(-0.5, 0.0), bounds(), cursor);
        assert_eq!(ws.scroll_line_residual.get(), -0.5, "a reversal starts over");
    }

    /// `screen_as_ansi` must reproduce the visible screen when fed to a
    /// fresh emulator: text, named / indexed / RGB colors, wide (CJK)
    /// glyphs and the visual attribute flags all round-trip cell-exact.
    /// Backs the transcript viewer's final-alt-frame materialization.
    #[test]
    fn screen_as_ansi_roundtrips_the_visible_screen() {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Flags as CellFlags;

        let mut a = TerminalState::new_no_pty(24, 7).unwrap();
        a.process(b"\x1b[31mred\x1b[0m plain\r\n");
        a.process(b"\x1b[1;44mB on blue\x1b[0m\r\n");
        a.process(b"\x1b[38;2;1;2;3mrgb\x1b[0m \x1b[7minv\x1b[0m \x1b[38;5;123midx\x1b[0m\r\n");
        a.process("wide 漢字 ok\r\n".as_bytes());
        // A background bar with trailing colored blanks (the top header
        // pattern) followed by default-styled text.
        a.process(b"\x1b[4mu\x1b[0m\x1b[42m  \x1b[0mtail\r\n");
        // Every underline variant, plus an RGB underline color: each
        // must keep its exact style through the round-trip, not reduce
        // to plain underline.
        a.process(b"\x1b[4:2md\x1b[0m\x1b[4:3mc\x1b[0m\x1b[4:4mo\x1b[0m\x1b[4:5ma\x1b[0m\r\n");
        a.process(b"\x1b[4m\x1b[58;2;10;20;30mUC\x1b[0m");

        let bytes = a.screen_as_ansi();
        let mut b = TerminalState::new_no_pty(24, 7).unwrap();
        b.process(&bytes);

        let style = CellFlags::INVERSE
            | CellFlags::BOLD
            | CellFlags::ITALIC
            | CellFlags::DIM
            | CellFlags::HIDDEN
            | CellFlags::STRIKEOUT
            | CellFlags::ALL_UNDERLINES;
        let ga = a.backend.term.grid();
        let gb = b.backend.term.grid();
        assert_eq!(ga.screen_lines(), gb.screen_lines());
        for r in 0..ga.screen_lines() as i32 {
            for c in 0..ga.columns() {
                let ca = &ga[Line(r)][Column(c)];
                let cb = &gb[Line(r)][Column(c)];
                let norm = |ch: char| if ch == '\0' { ' ' } else { ch };
                assert_eq!(norm(ca.c), norm(cb.c), "char at {r},{c}");
                assert_eq!(ca.fg, cb.fg, "fg at {r},{c}");
                assert_eq!(ca.bg, cb.bg, "bg at {r},{c}");
                assert_eq!(ca.flags & style, cb.flags & style, "flags at {r},{c}");
                assert_eq!(
                    ca.underline_color(),
                    cb.underline_color(),
                    "underline color at {r},{c}"
                );
            }
        }
    }

    /// A surface rendered unfocused BY CONSTRUCTION keeps its selection.
    /// The session player replays into such a widget (its keys are
    /// transport controls, so it never takes focus), and while the
    /// lose-focus sweep tested `!focused` alone, the first mouse motion
    /// of the drag that made a selection wiped it: nothing in a
    /// recording could be selected, let alone copied.
    #[test]
    fn a_never_focused_surface_keeps_its_selection() {
        let (view, mut ws) = view_and_state();
        let view = view.focused(false);
        ws.selection = Some(Selection { start: (0, 0), end: (5, 0), block: false });
        let ev = iced::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(40.0, 40.0),
        });
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));
        view.on_event(&mut ws, &ev, bounds(), cursor);
        assert!(
            ws.selection.is_some(),
            "a display-only surface never had focus to lose"
        );
    }

    /// The other half of the same rule: a pane that WAS focused drops its
    /// highlight once it isn't. Split a tab three ways and every pane you
    /// ever selected in would otherwise stay lit, with nothing saying
    /// which one the next copy takes.
    #[test]
    fn a_pane_that_loses_focus_drops_its_highlight() {
        let term = Arc::new(Mutex::new(TerminalState::new_no_pty(80, 24).unwrap()));
        let focused: TerminalView<()> = TerminalView::new(Arc::clone(&term)).focused(true);
        let unfocused: TerminalView<()> = TerminalView::new(term).focused(false);
        let mut ws = TerminalWidgetState {
            selection: Some(Selection { start: (0, 0), end: (5, 0), block: false }),
            ..Default::default()
        };
        let ev = iced::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(40.0, 40.0),
        });
        let cursor = mouse::Cursor::Available(Point::new(40.0, 40.0));

        focused.on_event(&mut ws, &ev, bounds(), cursor);
        assert!(ws.selection.is_some(), "the focused pane keeps its highlight");

        unfocused.on_event(&mut ws, &ev, bounds(), cursor);
        assert!(ws.selection.is_none(), "the pane being left drops its highlight");
    }

    /// A key event carrying `key`, with no modifiers: enough for the
    /// chord arm, which resolves through the app's matcher rather than
    /// reading the modifiers itself.
    fn key_press(key: keyboard::Key) -> iced::Event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    /// The session player renders its stage unfocused (its keys are the
    /// transport, so it must not take the "typing clears the highlight"
    /// path), which used to mean its chords never fired either: a
    /// recording could be selected with the mouse but not copied from
    /// the keyboard. `with_chords_unfocused` is the opt-in for a surface
    /// that is the only terminal on screen.
    #[test]
    fn chords_fire_unfocused_when_the_surface_opts_in() {
        let (view, mut ws) = view_and_state();
        let view = view
            .focused(false)
            .with_terminal_chords(Box::new(|_, _| Some(TerminalChordAction::SelectAll)))
            .with_chords_unfocused(true);
        view.on_event(
            &mut ws,
            &key_press(keyboard::Key::Character("a".into())),
            bounds(),
            mouse::Cursor::Unavailable,
        );
        assert!(ws.selection.is_some(), "select-all must reach the replay");
    }

    /// The other side of the same gate: without the opt-in an unfocused
    /// widget ignores the chords. Key events reach every widget in the
    /// tree, so this is what keeps a three-way split from running the
    /// copy chord three times.
    #[test]
    fn chords_stay_focus_gated_by_default() {
        let (view, mut ws) = view_and_state();
        let view = view
            .focused(false)
            .with_terminal_chords(Box::new(|_, _| Some(TerminalChordAction::SelectAll)));
        view.on_event(
            &mut ws,
            &key_press(keyboard::Key::Character("a".into())),
            bounds(),
            mouse::Cursor::Unavailable,
        );
        assert!(ws.selection.is_none(), "an unfocused pane declines the chord");
    }

    /// Everything a dead session can leave armed, in one pane, so the
    /// reset is asserted against the state it exists for.
    fn state_with_stale_modes() -> TerminalState {
        let mut term = TerminalState::new_no_pty(80, 24).unwrap();
        // What a killed tmux / vim leaves behind: any-motion tracking
        // (1003) with both encodings (1005/1006), focus reporting (1004),
        // bracketed paste (2004), application cursor keys (1), autowrap
        // off (7) and a hidden cursor (25).
        term.process(b"\x1b[?1;1003;1004;1005;1006;2004h\x1b[?7;25l");
        term
    }

    /// The reset the app feeds on disconnect and on every fresh session
    /// (`SESSION_MODE_RESET`) must clear every mode the widget's
    /// mouse-report gate reads. Guard for the reconnect-garbage bug: stale
    /// 1000/1002/1003/1006 left by a dead session made the widget keep
    /// synthesizing SGR reports into a shell that never asked for them,
    /// and the shell's echo of those reports landed on screen as text.
    /// Regression at the gate level: with the modes cleared, a pointer
    /// move must produce NO report.
    #[test]
    fn session_reset_clears_mouse_tracking_and_blocks_reports() {
        use alacritty_terminal::term::TermMode;

        let mut term = state_with_stale_modes();
        assert!(
            term.backend.term.mode().intersects(TermMode::MOUSE_MODE),
            "precondition: stale mouse tracking armed"
        );

        term.process(crate::SESSION_MODE_RESET);

        let mode = *term.backend.term.mode();
        assert!(
            !mode.intersects(TermMode::MOUSE_MODE),
            "mouse tracking cleared"
        );
        assert!(!mode.contains(TermMode::SGR_MOUSE), "SGR encoding cleared");
        assert!(!mode.contains(TermMode::UTF8_MOUSE), "UTF-8 encoding cleared");
        assert!(
            !mode.contains(TermMode::FOCUS_IN_OUT),
            "focus reporting cleared"
        );
        assert!(
            !mode.contains(TermMode::BRACKETED_PASTE),
            "bracketed paste cleared"
        );
        assert!(
            !mode.contains(TermMode::APP_CURSOR),
            "application cursor keys cleared"
        );
        assert!(mode.contains(TermMode::LINE_WRAP), "autowrap back on");
        assert!(mode.contains(TermMode::SHOW_CURSOR), "cursor shown again");

        // The widget's report gate reads the mode back from the state: a
        // pointer move must not synthesize a report any more.
        let view = TerminalView::<()>::new(Arc::new(Mutex::new(term)));
        let mut ws = TerminalWidgetState::default();
        let ev = iced::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(40.0, 40.0),
        });
        let action = view.handle_mouse_report(
            &mut ws,
            &ev,
            bounds(),
            mouse::Cursor::Available(Point::new(40.0, 40.0)),
            mode,
            80,
            24,
        );
        assert!(action.is_none(), "no SGR report after the session reset");
    }

    /// The scrolling region a dead full-screen app left behind would pin
    /// the new shell's output inside its band, and DECSTBM homes the
    /// cursor, which is why the reset wraps it in DECSC/DECRC: the
    /// region goes back to the whole screen and the cursor stays where
    /// the session left it.
    #[test]
    fn session_reset_restores_the_scrolling_region_without_moving_the_cursor() {
        use alacritty_terminal::index::{Column, Line};

        let mut term = TerminalState::new_no_pty(80, 24).unwrap();
        // A band over the top 5 lines, cursor parked inside the pane.
        term.process(b"\x1b[1;5r\x1b[10;3H");
        let before = term.backend.term.grid().cursor.point;
        assert_eq!(before, alacritty_terminal::index::Point::new(Line(9), Column(2)));

        term.process(crate::SESSION_MODE_RESET);
        assert_eq!(
            term.backend.term.grid().cursor.point, before,
            "the region reset must not home the cursor"
        );

        // One line per row, no trailing newline: with the band still
        // armed the text would scroll inside rows 1-5 and the bottom of
        // the screen would stay empty.
        term.process(b"\x1b[H");
        for i in 0..24 {
            term.process(format!("L{i}").as_bytes());
            if i < 23 {
                term.process(b"\r\n");
            }
        }
        let last = (0..3)
            .map(|c| term.backend.term.grid()[Line(23)][Column(c)].c)
            .collect::<String>();
        assert_eq!(last, "L23", "output must reach the bottom row again");
    }

    /// A connection killed inside tmux / vim leaves the pane on the
    /// alternate screen. `LEAVE_ALT_SCREEN` puts it back on the real
    /// buffer (with its scrollback) exactly as the app's own clean exit
    /// would have, and is a no-op on a pane that never entered.
    #[test]
    fn leave_alt_screen_restores_the_primary_buffer() {
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::TermMode;

        let mut term = TerminalState::new_no_pty(80, 24).unwrap();
        term.process(b"shell output");
        // A full-screen app takes over and dies mid-frame.
        term.process(b"\x1b[?1049h\x1b[Happ frame");
        assert!(term.backend.term.mode().contains(TermMode::ALT_SCREEN));

        term.process(crate::LEAVE_ALT_SCREEN);

        assert!(
            !term.backend.term.mode().contains(TermMode::ALT_SCREEN),
            "back on the primary buffer"
        );
        let row0 = (0..12)
            .map(|c| term.backend.term.grid()[Line(0)][Column(c)].c)
            .collect::<String>();
        assert_eq!(row0, "shell output", "the real buffer is back");

        // Idempotent: a pane that never entered stays put.
        term.process(crate::LEAVE_ALT_SCREEN);
        assert!(!term.backend.term.mode().contains(TermMode::ALT_SCREEN));
        let row0 = (0..12)
            .map(|c| term.backend.term.grid()[Line(0)][Column(c)].c)
            .collect::<String>();
        assert_eq!(row0, "shell output");
    }

    // ── Scrolled viewport pinning across output ──

    /// A viewport scrolled up must keep showing the same rows while new
    /// output arrives: `scroll_offset` counts lines above the live edge,
    /// so without compensation each new line drags the view one row
    /// toward the bottom (and the scrollbar thumb with it).
    #[test]
    fn scrolled_viewport_pins_through_output() {
        // 40 lines above the live edge of a 100-line history.
        let (offset, anchor_history) = pin_scrolled_offset(40, Some(100), 100);
        assert_eq!((offset, anchor_history), (40, Some(100)), "idle draw is a no-op");

        // 25 more lines of output between draws: the offset rises by the
        // growth so the same absolute rows stay on screen.
        let (offset, anchor_history) = pin_scrolled_offset(offset, anchor_history, 125);
        assert_eq!(
            (offset, anchor_history),
            (65, Some(125)),
            "offset must track the history growth"
        );

        // A draw with nothing new between keeps the position.
        let (offset, anchor_history) = pin_scrolled_offset(offset, anchor_history, 125);
        assert_eq!((offset, anchor_history), (65, Some(125)));

        // Back at the live edge the pin is dropped and output follows.
        let (offset, anchor_history) = pin_scrolled_offset(0, anchor_history, 140);
        assert_eq!((offset, anchor_history), (0, None));
    }

    /// Snapping back to the live edge (the reset-on-output yank, a
    /// keypress snap) must also clear a stale anchor: a later scroll-up
    /// pins from the present history, never from before the snap.
    #[test]
    fn live_edge_clears_a_stale_anchor() {
        let (offset, anchor_history) = pin_scrolled_offset(0, Some(100), 140);
        assert_eq!(
            (offset, anchor_history),
            (0, None),
            "offset 0 always clears the anchor"
        );
    }

    /// The very first scroll has no anchor yet: it pins from that draw
    /// onward instead of guessing at older content.
    #[test]
    fn first_scroll_establishes_the_anchor() {
        let (offset, anchor_history) = pin_scrolled_offset(10, None, 100);
        assert_eq!((offset, anchor_history), (10, Some(100)));

        let (offset, _) = pin_scrolled_offset(offset, anchor_history, 120);
        assert_eq!(offset, 30, "subsequent output is compensated");
    }

    /// A history that shrank (clear-scrollback, resize, alt screen) has
    /// no old lines left to pin: the anchor is dropped and the offset
    /// counts as freshly set, so later growth is measured from the
    /// current history instead of bridging the gap.
    #[test]
    fn pinned_viewport_drops_its_anchor_when_history_shrinks() {
        let (offset, anchor_history) = pin_scrolled_offset(40, Some(100), 40);
        assert_eq!(
            (offset, anchor_history),
            (40, None),
            "shrink keeps the offset but drops the stale anchor"
        );

        // The next draw after a shrink establishes a fresh anchor: growth
        // is compensated from this point on, never across the shrink.
        let (offset, anchor_history) = pin_scrolled_offset(offset, anchor_history, 42);
        assert_eq!((offset, anchor_history), (40, Some(42)));

        // And growth after that is compensated normally.
        let (offset, _) = pin_scrolled_offset(offset, anchor_history, 45);
        assert_eq!(offset, 43);
    }

    /// An offset held through the alternate screen (where the caller
    /// clears the anchor, since the primary history is not visible there)
    /// must not be compensated over the buffer that grew while the alt
    /// app owned the screen: with no anchor the jump lands as a fresh
    /// pin, not a pull to the top.
    #[test]
    fn unanchored_offset_never_bridges_a_history_gap() {
        // First frame back on the primary screen, history 5000 after a
        // compile ran while vim was open.
        let (offset, anchor_history) = pin_scrolled_offset(40, None, 5000);
        assert_eq!(
            (offset, anchor_history),
            (40, Some(5000)),
            "the gap must not be compensated across the alt-screen round trip"
        );

        // Only output from here on is compensated.
        let (offset, _) = pin_scrolled_offset(offset, anchor_history, 5010);
        assert_eq!(offset, 50);
    }
