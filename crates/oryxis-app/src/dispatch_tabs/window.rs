//! Window-chrome + pointer handlers split out of `dispatch_tabs`:
//! mouse-move drag tracking, window resize / move / focus, and the
//! custom title-bar drag / minimize / maximize / close / fullscreen
//! actions. Called from `handle_tabs`.

#![allow(clippy::result_large_err)]

use iced::{Point, Task};

use crate::app::{TabsMessage, Message, Oryxis};

impl Oryxis {
    pub(super) fn handle_mouse_moved(&mut self, pos: iced::Point) -> Task<Message> {
        // Spatial debounce: mouse-move events fire 60+ times per
        // second. Re-stating `mouse_position` on every event forces
        // a view() pass each time, which on dense pages (keychain
        // grid, SFTP listing) can take long enough to back up
        // iced's subscription channel and trigger
        // `TrySendError { kind: Full }` warnings. Quantising the
        // stored position to a 2 px grid means consecutive moves
        // within the same cell don't re-state the field at all,
        // so the view doesn't reflow. Same trick the
        // `WindowResized` handler uses below.
        const SNAP: f32 = 2.0;
        let snapped = iced::Point {
            x: (pos.x / SNAP).round() * SNAP,
            y: (pos.y / SNAP).round() * SNAP,
        };
        let needs_drag_update = self.chat_ui.sidebar_drag.is_some()
            || self.panel_resize_drag.is_some()
            || self.sftp_chrome.split_drag.is_some()
            || self.sftp_chrome.log_drag.is_some()
            || self.sftp_chrome.col_resize.is_some()
            || self.sftp_chrome.col_drag.is_some()
            || self.sftp.drag.is_some()
            || self.tab_drag.is_some();
        // Promote an armed tab drag to active once the cursor moves
        // past a small threshold, so a plain click never reorders.
        if let Some(drag) = self.tab_drag.as_mut()
            && !drag.active
        {
            const TAB_DRAG_THRESHOLD: f32 = 6.0;
            let dx = pos.x - drag.start.x;
            let dy = pos.y - drag.start.y;
            if (dx * dx + dy * dy).sqrt() > TAB_DRAG_THRESHOLD {
                drag.active = true;
            }
        }
        let changed = (snapped.x - self.mouse_position.x).abs() > 0.5
            || (snapped.y - self.mouse_position.y).abs() > 0.5;
        if !changed && !needs_drag_update {
            return Task::none();
        }
        self.mouse_position = if needs_drag_update { pos } else { snapped };
        // A real mouse move restores the hover highlight that keyboard
        // navigation muted (no-op when it wasn't suppressed).
        if changed {
            self.sftp.suppress_hover = false;
        }
        // Promote an armed SFTP internal drag once the cursor crosses
        // into the *opposite* pane, driven by cursor geometry (which
        // IS delivered during a button-hold here, the same signal the
        // divider / column-resize drags rely on). Runs after
        // `mouse_position` is updated so the hit-test sees the fresh
        // coord. This is the primary activation; row-hover
        // (SftpRowEnter) is only a fallback, since it can be disrupted
        // by tooltips / row gaps and is why cross-pane drag used to
        // fail intermittently.
        if let Some(drag) = self.sftp.drag.as_ref()
            && !drag.active
        {
            use crate::state::SftpPaneSide::{Left, Right};
            let dx = pos.x - drag.press_pos.x;
            let dy = pos.y - drag.press_pos.y;
            let moved = (dx * dx + dy * dy).sqrt() > 6.0;
            let over_opposite = match drag.origin_side {
                Left => self.is_cursor_over_remote_pane(),
                Right => self.is_cursor_over_local_pane(),
            };
            if moved && over_opposite && let Some(d) = self.sftp.drag.as_mut() {
                d.active = true;
            }
        }
        // While a sidebar-region resize handle is held down, that
        // region's width tracks the cursor. The handle sits on the
        // region's inner edge, so the drag direction that grows it
        // flips with the region's side (issue #102): the right region
        // grows dragging left, the left region grows dragging right.
        // Clamp to a sane band so the user can't accidentally make it
        // unusable.
        if let Some((side, start_x, start_width)) = self.chat_ui.sidebar_drag {
            let delta = pos.x - start_x;
            let signed = match side {
                crate::state::SidebarSide::Left => delta,
                crate::state::SidebarSide::Right => -delta,
            };
            let new_width = (start_width + signed).clamp(260.0, 700.0);
            self.chat_ui.sidebar_width[side.idx()] = new_width;
        }
        // While the side-panel editor drawer's edge handle is held
        // down, the drawer width tracks the cursor. The drawer sits on
        // the trailing edge (physical right under LTR, left under RTL),
        // so the direction that grows it flips with the layout
        // direction. Clamp to the band that keeps both the form and the
        // content next to it usable.
        if let Some((start_x, start_width)) = self.panel_resize_drag {
            let delta = pos.x - start_x;
            let signed = if crate::i18n::is_rtl_layout() { delta } else { -delta };
            self.panel_width = (start_width + signed)
                .clamp(crate::app::PANEL_WIDTH_MIN, crate::app::PANEL_WIDTH_MAX);
        }
        // SFTP center divider: the ratio tracks the cursor across the
        // content area (window minus the nav rail; the chat sidebar is
        // terminal-only so it isn't subtracted here). Clamp so neither
        // pane can collapse.
        if let Some((start_x, start_ratio)) = self.sftp_chrome.split_drag {
            let content_w = (self.window_size.width
                - self.vault_rail_width()
                - self.side_strip_reserve())
            .max(1.0);
            let new_ratio =
                (start_ratio + (pos.x - start_x) / content_w).clamp(0.15, 0.85);
            self.sftp_chrome.split_ratio = new_ratio;
        }
        // SFTP message-log panel height: the divider sits above the
        // panel, so dragging up (smaller y) grows it.
        if let Some((start_y, start_h)) = self.sftp_chrome.log_drag {
            self.sftp.log_height = (start_h - (pos.y - start_y))
                .clamp(crate::state::SFTP_LOG_MIN_H, crate::state::SFTP_LOG_MAX_H);
        }
        // SFTP column resize: the dragged column's width tracks the
        // cursor (clamped inside the setters). The total row width
        // grows; the other columns keep their size.
        if let Some((side, col, start_x, start_w)) = self.sftp_chrome.col_resize {
            let new_w = start_w + (pos.x - start_x);
            self.sftp.pane_mut(side).columns.width.set(col, new_w);
        }
        // Promote a column reorder drag to active past a small
        // threshold so a plain header click still sorts.
        if let Some(drag) = self.sftp_chrome.col_drag.as_mut()
            && !drag.active
            && (pos.x - drag.press_x).abs() > 5.0
        {
            drag.active = true;
        }
        // Promote a pending press to an active drag once the
        // cursor moves past the threshold. Below the threshold
        // we leave it pending so the click handler still fires
        // for plain clicks (jitter < 5px).
        if let Some(drag) = self.sftp.drag.as_mut()
            && !drag.active
        {
            let dx = pos.x - drag.press_pos.x;
            let dy = pos.y - drag.press_pos.y;
            if (dx * dx + dy * dy).sqrt() > 5.0 {
                drag.active = true;
            }
        }
        Task::none()
    }

    /// The armed drag-out's payload came back (issue #167). It does NOT
    /// start the OS drag: it parks the resolved payload in the gesture,
    /// which `advance_drag_out` hands over once the cursor leaves the
    /// window. An arm that is gone by now means the user released mid
    /// round trip; dropping the payload closes the remote handles it
    /// opened.
    fn handle_drag_out_ready(
        &mut self,
        result: Result<crate::drag_out::Prepared, String>,
    ) -> Task<Message> {
        match result {
            Ok(prepared) => {
                if let Some(arm) = self.drag_out_arm.as_mut()
                    && matches!(arm.stage, crate::drag_out::DragOutStage::Resolving)
                {
                    arm.stage = crate::drag_out::DragOutStage::Ready(prepared);
                }
                // The cursor may ALREADY be outside by the time the
                // payload lands, and a cursor parked out there produces
                // no further events to notice it on. This arrival is
                // the last one the gesture is guaranteed, so the
                // escalation check runs here too.
                self.advance_drag_out().unwrap_or_else(Task::none)
            }
            Err(e) => {
                // The open failed (file vanished, channel died): the
                // toast is the same surface every other one-shot op
                // reports through. The gesture is over either way.
                self.drag_out_arm = None;
                self.show_toast_secs(e, 3)
            }
        }
    }

    pub(super) fn handle_window_resized(&mut self, size: iced::Size) -> Task<Message> {
        // Spatial debounce: drag-resize emits one event per pixel.
        // Quantising to an 8 px grid means most consecutive events
        // resolve to the same `window_size` so we don't re-state
        // the field, and view()s that depend on it don't reflow
        // a responsive grid on every frame. Cuts reflow frequency
        // by ~8x during a sustained drag, which keeps iced's
        // subscription channel from filling up and dropping events
        // (the `TrySendError { kind: Full }` warnings).
        const SNAP: f32 = 8.0;
        let snapped = iced::Size {
            width: (size.width / SNAP).round() * SNAP,
            height: (size.height / SNAP).round() * SNAP,
        };
        if (snapped.width - self.window_size.width).abs() > 0.5
            || (snapped.height - self.window_size.height).abs() > 0.5
        {
            self.window_size = snapped;
            // The floating toolbar search / overflow popovers are
            // anchored to a width that just changed, and the inline
            // field may now fit again. Dismiss them so they re-pop
            // at the right place (and the inline field re-mounts
            // without colliding on its widget Id).
            if matches!(
                self.overlay.as_ref().map(|o| &o.content),
                Some(crate::state::OverlayContent::ToolbarSearch)
                    | Some(crate::state::OverlayContent::ToolbarOverflow)
            ) {
                self.overlay = None;
            }
            // Reconcile the optimistic `window_maximized` with the OS
            // truth. Win+Up/Down, aero snap, the taskbar's Restore and
            // dragging the custom title bar down (a restore inside the
            // OS move loop, so no app message ever fires) all change
            // the OS state without `WindowMaximizeToggle`; a stale
            // `true` would hide the edge-resize border and turn
            // `WindowResizeDrag` into a no-op (field report: the
            // window looked windowed but had no edges to grab).
            // `WM_SIZE` has already updated winit's cached state by
            // the time this event reaches us, so the query returns the
            // settled truth, with no race against our own optimistic
            // toggle. The snapped size rides along because the
            // windowed-size tracking (what `persist_window_geometry`
            // restores next launch) must also be judged against the OS
            // truth: recording it here, gated on the optimistic flag,
            // let an OS-side maximize slip its monitor-sized rectangle
            // in as the "windowed" size before the reconcile landed.
            return iced::window::latest().then(move |id_opt| match id_opt {
                Some(id) => iced::window::is_maximized(id).map(move |maximized| {
                    Message::Tabs(TabsMessage::WindowMaximizedSynced(maximized, snapped))
                }),
                None => Task::none(),
            });
        }
        Task::none()
    }

    pub(super) fn handle_window_ensure_on_screen(&mut self) -> Task<Message> {
        // Runs once shortly after boot when a saved position was
        // restored. If that position is on a monitor that no
        // longer exists (undocked laptop, unplugged display),
        // the window would be stranded off-screen with no way to
        // grab its title bar, so pull it back onto the monitor
        // the OS considers nearest. All values are logical
        // coordinates; `monitor_*` return `None` where the
        // platform can't say (Wayland), in which case the WM
        // already placed us somewhere visible and we skip.
        let win_size = self.window_size;
        iced::window::latest().then(move |id_opt| {
            let Some(id) = id_opt else { return Task::none(); };
            iced::window::position(id).then(move |pos_opt| {
                let Some(pos) = pos_opt else { return Task::none(); };
                iced::window::monitor_position(id).then(move |origin_opt| {
                    let Some(origin) = origin_opt else {
                        return Task::none();
                    };
                    iced::window::monitor_size(id).then(move |size_opt| {
                        let Some(monitor) = size_opt else {
                            return Task::none();
                        };
                        // Visible enough = at least 60 px of
                        // horizontal overlap with the nearest
                        // monitor AND the title strip (top
                        // 40 px) vertically inside it. The
                        // nearest monitor is the only one we
                        // can query, but a window that fails
                        // this against its *nearest* monitor
                        // fails it against every other one by
                        // definition.
                        let overlap_x = (pos.x + win_size.width)
                            .min(origin.x + monitor.width)
                            - pos.x.max(origin.x);
                        let title_visible = pos.y >= origin.y - 4.0
                            && pos.y + 40.0 <= origin.y + monitor.height;
                        if overlap_x >= 60.0 && title_visible {
                            return Task::none();
                        }
                        tracing::info!(
                            "restored window position ({}, {}) is off-screen, \
                             recentering on the nearest monitor",
                            pos.x,
                            pos.y
                        );
                        iced::window::move_to(
                            id,
                            Point::new(origin.x + 48.0, origin.y + 48.0),
                        )
                    })
                })
            })
        })
    }

    pub(super) fn handle_window_focus_changed(&mut self, focused: bool) -> Task<Message> {
        self.window_focused = focused;
        if !focused {
            // A mouse release OUTSIDE the window never reaches us, so a
            // drag that leaves the window would keep its ghost chip
            // floating forever (field report: a stuck tab ghost parked
            // over the title bar). Losing focus is the reliable signal
            // that the gesture ended elsewhere: cancel any in-flight
            // drag state. The live-slide reorder already applied, so
            // cancelling loses nothing but the ghost.
            self.tab_drag = None;
            self.sftp.drag = None;
            self.sftp_chrome.col_drag = None;
            // A drawer-resize release outside the window never reaches
            // us either; the width already applied live, only the drag
            // state (and its pending persist) is dropped.
            self.panel_resize_drag = None;
            // An Alt released outside the window never reaches us; a
            // wedged side would silently turn Option keystrokes into
            // Meta (or vice versa) after refocus.
            self.alt_sides = crate::key_encode::OptionSides::default();
        }
        if focused {
            // Refocusing the window means the active tab is being
            // looked at again, IF the terminal is on screen (returning
            // to the Dashboard / Settings views doesn't show it): its
            // smart-tab attention is consumed. The same helper
            // `view_content` renders by, since a tab opened from the
            // Dashboard never assigns `active_view`.
            if self.terminal_surface_visible()
                && let Some(at) = self.active_tab
                && let Some(tab) = self.tabs.get_mut(at)
            {
                for pane in tab.pane_grid.panes.values_mut() {
                    pane.attention = None;
                }
            }
            // A notification toast raised while the window was unfocused
            // is left up (no auto-dismiss timer) so it isn't gone before
            // you look; clear it a few seconds after you return.
            if self.toast.is_some() {
                return iced::Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    },
                    |_| Message::ToastClear,
                );
            }
        } else {
            // Crash-safe geometry checkpoint: the exit paths all
            // persist, but an OS shutdown or a kill never reaches
            // them. Focus loss is infrequent enough that four tiny
            // row upserts don't matter, and recent enough that the
            // restored geometry stays accurate.
            self.persist_window_geometry();
            // Commit any in-progress Ctrl+Tab run: losing focus with
            // Ctrl held may swallow the release event, and the OS may
            // not deliver a modifier change on blur, so end the run
            // here rather than leave it stranded (which would freeze
            // MRU tracking until the next real Ctrl-release).
            self.commit_tab_cycle();
        }
        Task::none()
    }

    pub(super) fn handle_window_expand_vertical(&mut self) -> Task<Message> {
        if self.window_maximized {
            return Task::none();
        }
        let current_width = self.window_size.width;
        iced::window::latest().then(move |id_opt| {
            let Some(id) = id_opt else { return Task::none(); };
            iced::window::position(id).then(move |pos_opt| {
                let Some(pos) = pos_opt else { return Task::none(); };
                iced::window::monitor_size(id).then(move |size_opt| {
                    let Some(size) = size_opt else { return Task::none(); };
                    iced::window::monitor_position(id).then(move |origin_opt| {
                        // Default to (0, 0) when the platform
                        // can't report the monitor origin so we
                        // at least fall back to the primary
                        // same as the old behaviour.
                        let origin = origin_opt.unwrap_or(Point::ORIGIN);
                        Task::batch([
                            iced::window::move_to(
                                id,
                                Point::new(pos.x, origin.y),
                            ),
                            iced::window::resize(
                                id,
                                iced::Size::new(current_width, size.height),
                            ),
                        ])
                    })
                })
            })
        })
    }

    pub(super) fn handle_window_minimize(&mut self) -> Task<Message> {
        // Custom title bar minimize. Honours
        // setting_minimize_to_tray on Windows by hiding the
        // window outright instead of minimizing (which would
        // leave a taskbar slot). Everywhere else and when
        // the toggle is off we fall through to the real
        // iced::window::minimize call.
        if self.prefs.minimize_to_tray && cfg!(target_os = "windows") {
            self.is_window_hidden = true;
            // Reveal the icon in the same frame the window vanishes.
            // The tray heartbeat would also get there via the signature,
            // but up to 500 ms later, and a window that is gone with no
            // icon yet reads as "the app just quit". No-op on a child
            // process (no tray of its own) and off Windows.
            crate::tray::set_visible(true);
            self.broadcast_ipc_state_if_child();
            return iced::window::oldest()
                .and_then(|id| {
                    iced::window::run(id, |window| {
                        crate::tray::hide_window(window);
                    })
                })
                .discard();
        }
        iced::window::latest().then(|id_opt| match id_opt {
            Some(id) => iced::window::minimize(id, true),
            None => Task::none(),
        })
    }

    pub(super) fn handle_window_close(&mut self) -> Task<Message> {
        // Persist any buffered session-log output before the
        // window goes away (real close or hide-to-tray both).
        self.flush_session_logs_final();
        // A host-editor auto-save still inside its debounce window
        // must not die with the process. Interrupted: closing the
        // window concluded nothing about a half-typed Parent Group
        // name, so it must not become a vault group.
        self.editor_flush_interrupted();
        // Remember size + maximized/fullscreen for the next
        // launch (also on hide-to-tray: a later tray Quit exits
        // without passing through here again).
        self.persist_window_geometry();
        // Honour the close-to-tray setting: when on, the
        // user's "close" verb (custom title bar X, Alt+F4
        // via CloseRequested subscription, etc.) hides the
        // window into the tray instead of quitting. Returns
        // a hide task on Windows where the tray is real; on
        // other platforms the helper is a no-op so we fall
        // through to a real close. Default (off) closes for
        // everyone.
        if self.prefs.close_to_tray && cfg!(target_os = "windows") {
            self.is_window_hidden = true;
            // Same instant-reveal as the minimize path: the icon is the
            // only way back once the window is hidden.
            crate::tray::set_visible(true);
            self.broadcast_ipc_state_if_child();
            return iced::window::oldest()
                .and_then(|id| {
                    iced::window::run(id, |window| {
                        crate::tray::hide_window(window);
                    })
                })
                .discard();
        }
        iced::window::latest().then(|id_opt| match id_opt {
            Some(id) => iced::window::close(id),
            None => Task::none(),
        })
    }

    pub(super) fn handle_window_fullscreen_toggle(&mut self) -> Task<Message> {
        // Optimistic local flip mirrors `WindowMaximizeToggle`,
        // the only way fullscreen changes today is through this
        // handler so the cached bool stays in sync.
        self.window_fullscreen = !self.window_fullscreen;
        // Same crash-safe checkpoint as the maximize toggle.
        self.persist_window_geometry();
        let entering = self.window_fullscreen;
        let next = if entering {
            iced::window::Mode::Fullscreen
        } else {
            iced::window::Mode::Windowed
        };
        let mode_task = iced::window::latest().then(move |id_opt| match id_opt {
            Some(id) => iced::window::set_mode(id, next),
            None => Task::none(),
        });
        // Browser-style on-enter hint: show "Press F11 to
        // exit" for 3 s then auto-hide. Exiting fullscreen
        // also clears the flag in case the user toggled
        // out before the timer fired.
        if entering {
            self.fullscreen_hint_visible = true;
            let hide_task = Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                },
                |_| Message::Tabs(TabsMessage::FullscreenHintHide),
            );
            return Task::batch([mode_task, hide_task]);
        }
        self.fullscreen_hint_visible = false;
        mode_task
    }

}

impl Oryxis {
    /// The window-chrome arms of the tabs domain.
    ///
    /// They live beside the handlers they call rather than in a
    /// module of their own: every one of these is one line into a
    /// `handle_window_*` defined above.
    pub(super) fn handle_tabs_window(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            TabsMessage::MouseMoved(pos) => return self.handle_mouse_moved(pos),
            TabsMessage::DragOutReady(result) => return self.handle_drag_out_ready(result),
            TabsMessage::WindowResized(size) => return self.handle_window_resized(size),
            TabsMessage::WindowMoved(pos) => {
                // Same skip rule as the windowed-size tracking above:
                // maximize / fullscreen park the window at the monitor
                // origin, and the optimistic flags flip before that
                // Moved event arrives. The second filter drops the
                // (-32000, -32000) sentinel Windows reports for
                // minimized windows (scaled by DPI when converted to
                // logical, hence the generous threshold: no real
                // monitor layout puts a window beyond -8000 on both
                // axes at once).
                let minimized_sentinel = pos.x <= -8000.0 && pos.y <= -8000.0;
                if !self.window_maximized
                    && !self.window_fullscreen
                    && !minimized_sentinel
                {
                    // Keep the previous value around: an OS-side
                    // maximize parks the window at the monitor origin
                    // while `window_maximized` is still stale-false,
                    // so that Moved passes this guard and overwrites
                    // the real windowed position. When the
                    // `WindowMaximizedSynced` reconcile then detects
                    // the drift, it rolls back to this slot.
                    self.window_windowed_pos_prev = self.window_windowed_pos;
                    self.window_windowed_pos = Some(pos);
                }
            }
            TabsMessage::WindowEnsureOnScreen => return self.handle_window_ensure_on_screen(),
            TabsMessage::WindowFocusChanged(focused) => return self.handle_window_focus_changed(focused),
            TabsMessage::WindowDrag => {
                if !self.consume_window_press() {
                    return Task::none();
                }
                return iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::drag(id),
                    None => Task::none(),
                });
            }
            TabsMessage::WindowResizeDrag(direction) => {
                // Ignore resize requests while maximized, the window has no
                // borders to grab and the OS will reject/misbehave on WinIt.
                if self.window_maximized {
                    return Task::none();
                }
                if !self.consume_window_press() {
                    return Task::none();
                }
                return iced::window::latest().then(move |id_opt| match id_opt {
                    Some(id) => iced::window::drag_resize(id, direction),
                    None => Task::none(),
                });
            }
            TabsMessage::SidePanelResizeStart => {
                // Capture cursor x plus the current width; the
                // MouseMoved handler computes the delta against these.
                self.panel_resize_drag =
                    Some((self.mouse_position.x, self.panel_width));
            }
            TabsMessage::WindowExpandVertical => return self.handle_window_expand_vertical(),
            TabsMessage::WindowMinimize => return self.handle_window_minimize(),
            TabsMessage::WindowMaximizedSynced(maximized, size) => {
                // Deferred windowed-size commit: `size` is the snapped
                // size of the `WindowResized` that triggered this
                // query, recorded only now that the OS has said
                // whether that rectangle was a real windowed size or a
                // maximize transition's monitor-sized one. The
                // fullscreen flag needs no such reconcile: F11 is the
                // only path that changes it, so the optimistic flip
                // always lands before the fullscreen resize arrives.
                if !maximized && !self.window_fullscreen {
                    self.window_windowed_size = size;
                }
                // Reconcile the optimistic flag with the OS truth
                // (see `WindowMaximizedSynced`).
                if self.window_maximized != maximized {
                    if maximized {
                        // OS-side maximize: the Moved that parked the
                        // window at the monitor origin was recorded
                        // while the flag was still stale-false. Roll
                        // the windowed position back to the value it
                        // overwrote.
                        self.window_windowed_pos = self.window_windowed_pos_prev;
                    }
                    self.window_maximized = maximized;
                    // Same rationale as `WindowMaximizeToggle`: cheap
                    // write, and it keeps the restored state accurate
                    // even when the process later dies without
                    // reaching an exit path (OS shutdown, kill).
                    self.persist_window_geometry();
                }
            }
            TabsMessage::WindowMaximizeToggle => {
                self.window_maximized = !self.window_maximized;
                // Cheap write, and it keeps the restored state accurate
                // even when the process later dies without reaching an
                // exit path (OS shutdown, kill).
                self.persist_window_geometry();
                return iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::toggle_maximize(id),
                    None => Task::none(),
                });
            }
            TabsMessage::WindowClose => return self.handle_window_close(),
            TabsMessage::WindowFullscreenToggle => return self.handle_window_fullscreen_toggle(),
            TabsMessage::FullscreenHintHide => {
                self.fullscreen_hint_visible = false;
            }
            TabsMessage::SpawnNewWindow => {
                // Burger menu fires this. Drop both the context-menu
                // overlay AND the burger panel itself so the menu
                // doesn't linger on top of the freshly-spawned window.
                // The burger lives in its own `show_burger_menu` flag
                // (not `OverlayState`), so clearing `self.overlay`
                // alone wasn't enough.
                self.overlay = None;
                self.panels.burger_menu = false;
                self.spawn_oryxis_child(None);
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
