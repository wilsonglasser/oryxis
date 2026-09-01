//! The tab strip: select, close, pin, duplicate, reorder, and the
//! per-tab menus.
//!
//! Closing is the arm with teeth: a grouped tab tears down every pane in
//! it, so it confirms first, and the confirm lives in `handle_close_tab`
//! rather than at each call site (strip X, context menu, Ctrl+W and the
//! terminal's own path all reach it).

use super::*;

impl Oryxis {
    pub(super) fn handle_tabs_strip(&mut self, message: TabsMessage) -> Task<Message> {
        match message {
            TabsMessage::SelectTab(idx) => return self.handle_select_tab(idx),
            TabsMessage::CloseTab(idx) => return self.handle_close_tab(idx),
            // The strip's X, which is the only close the mouse performs
            // in a row: stamp the streak so the chip that slides under
            // the cursor next shows its own X on arrival instead of
            // asking for another dwell (issue #186).
            TabsMessage::CloseTabFromStrip(idx) => {
                self.hover.tab_close_click_at = Some(std::time::Instant::now());
                return self.handle_close_tab(idx);
            }
            TabsMessage::ConfirmCloseGroupedTab(idx) => return self.close_tab_now(idx),
            TabsMessage::ReopenClosedTab => return self.handle_reopen_closed_tab(),
            TabsMessage::CloseOtherTabs(idx) => {
                self.overlay = None;
                if idx < self.tabs.len() {
                    // Keep the clicked tab and every pinned tab (pinned tabs
                    // survive "close others", like a browser).
                    let target_id = self.tabs[idx]._id;
                    // Capture the connecting tab's id before filtering, so the
                    // progress state can be re-anchored / dropped afterwards.
                    let connecting_id = self
                        .connecting
                        .as_ref()
                        .and_then(|p| self.tabs.get(p.tab_idx))
                        .map(|t| t._id);
                    // Tear each one down instead of dropping it: a bare
                    // `retain` discards the struct while the connect
                    // stream keeps its own Arc on the session, so the
                    // channel, the engine tasks and the per-connection
                    // port forwards all outlive the chip (see
                    // `close_tab_sessions`). Same reason the recorded
                    // output has to be flushed and a live AI stream
                    // aborted first: closing four tabs at once must cost
                    // exactly what closing them one by one costs.
                    // Reverse order so each index is still valid when its
                    // turn comes.
                    for i in (0..self.tabs.len()).rev() {
                        if self.tabs[i]._id != target_id && !self.tabs[i].pinned {
                            // Each one lands on the reopen stack, exactly
                            // as if it had been closed on its own: a
                            // "close others" that drops a screenful is
                            // the case an undo is most wanted for.
                            self.remember_closed_tab(i);
                            self.teardown_tab_at(i);
                        }
                    }
                    let new_active = self
                        .tabs
                        .iter()
                        .position(|t| t._id == target_id)
                        .unwrap_or(0);
                    self.active_tab = Some(new_active);
                    self.remember_terminal_tab_focus(new_active);
                    self.reanchor_connecting_after_filter(connecting_id);
                }
            }
            TabsMessage::CloseAllTabs => {
                self.overlay = None;
                let connecting_id = self
                    .connecting
                    .as_ref()
                    .and_then(|p| self.tabs.get(p.tab_idx))
                    .map(|t| t._id);
                // Pinned tabs survive "close all". Torn down one by one
                // for the reason in `CloseOtherTabs` above.
                for i in (0..self.tabs.len()).rev() {
                    if !self.tabs[i].pinned {
                        self.remember_closed_tab(i);
                        self.teardown_tab_at(i);
                    }
                }
                if self.tabs.is_empty() {
                    self.active_tab = None;
                    self.clear_terminal_tab_memory();
                    self.active_view = View::Dashboard;
                    self.connecting = None;
                } else {
                    self.active_tab = Some(0);
                    self.remember_terminal_tab_focus(0);
                    self.reanchor_connecting_after_filter(connecting_id);
                }
            }
            TabsMessage::ClosePanelTab(kind) => {
                return self.close_panel_tab(kind);
            }
            TabsMessage::ToggleTabPin(idx) => {
                self.overlay = None;
                if let Some(tab) = self.tabs.get_mut(idx) {
                    tab.pinned = !tab.pinned;
                    // An explicit re-pin is the one gesture that retires an
                    // inherited SFTP pin (H5): from here on this tab is
                    // pinned as the terminal it now is, not as the SFTP tab
                    // it absorbed.
                    tab.inherited_pin = None;
                }
                self.persist_pinned_tabs();
            }
            TabsMessage::ReconnectTab(idx) => return self.handle_reconnect_tab(idx),
            TabsMessage::DuplicateTab(idx) => return self.handle_duplicate_tab(idx),
            TabsMessage::DuplicateInNewWindow(idx) => {
                self.overlay = None;
                self.spawn_oryxis_child(Some(idx));
            }
            TabsMessage::TabDragToEnd => {
                // Trailing drop zone: the live-slide only ever moves the
                // dragged tab to *before* a hovered tab, so the slot after the
                // last tab is unreachable by hovering. Entering the `+` area
                // during an active drag fills that gap.
                if let Some(drag) = self.tab_drag.filter(|d| d.active) {
                    self.slide_tab_to_partition_end(drag.from_id);
                }
            }
            TabsMessage::TabBarWheel(dy) => {
                // Vertical wheel over the tab bar scrolls horizontally
                // iced's horizontal-only scrollable ignores y deltas, so
                // we translate them via scroll_by here. Sign flip so
                // wheel-down brings later tabs into view (matches the
                // direction Chrome/VS Code use).
                return iced::widget::operation::scroll_by(
                    iced::widget::Id::new("tab-scroll"),
                    iced::widget::scrollable::AbsoluteOffset { x: -dy, y: 0.0 },
                );
            }
            TabsMessage::ShowTabMenu(idx) => {
                let anchor = self.keynav_take_menu_anchor();
                self.overlay = Some(OverlayState {
                    content: OverlayContent::TabActions(idx),
                    x: anchor.0,
                    y: anchor.1,
                });
            }
            TabsMessage::ShowTabBarMenu => {
                // The strip's own menu, at the cursor. A chip's right
                // press is captured by the chip, so reaching here means
                // the click landed on empty strip (issue #186).
                let anchor = self.keynav_take_menu_anchor();
                self.overlay = Some(OverlayState {
                    content: OverlayContent::TabBarActions,
                    x: anchor.0,
                    y: anchor.1,
                });
            }
            TabsMessage::ShowSplitMenu => {
                // Hover popover under `+`, anchored under the button.
                // It opens when it has something to offer beyond what
                // clicking `+` already does: a terminal tab to split, or
                // a closed tab to bring back (issue #186, which is also
                // why the second half matters: after closing the last
                // tab there is no tab to split, and that is exactly the
                // moment the reopen is being looked for).
                // An open tab is the whole split test: the `active_view`
                // half this used to also require is never assigned by the
                // ordinary path (opening a host from the Dashboard pushes
                // a tab and leaves the view where it was), so the popover
                // was dead exactly where splitting is most wanted.
                if (self.active_tab.is_some() || !self.closed_tabs.is_empty())
                    && !matches!(
                        self.overlay.as_ref().map(|o| &o.content),
                        Some(OverlayContent::SplitMenu)
                    )
                {
                    // Anchor under the `+` button at a fixed position (its
                    // reported bounds), not the cursor, so the popover lines
                    // up cleanly with the button.
                    let b = self.plus_btn_bounds.get();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SplitMenu,
                        x: b.x,
                        y: b.y + b.height,
                    });
                }
            }
            TabsMessage::SplitMenuEnter => {
                self.split_menu_hovered = true;
            }
            TabsMessage::SplitMenuLeave => {
                // Left the `+` button or the popover. Defer the close briefly
                // so moving from the button INTO the menu (which re-enters
                // via `SplitMenuEnter`) doesn't flap it shut.
                self.split_menu_hovered = false;
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
                    },
                    |_| Message::Tabs(TabsMessage::SplitMenuCloseIfIdle),
                );
            }
            TabsMessage::SplitMenuCloseIfIdle => {
                if !self.split_menu_hovered
                    && matches!(
                        self.overlay.as_ref().map(|o| &o.content),
                        Some(OverlayContent::SplitMenu)
                    )
                {
                    self.overlay = None;
                }
            }
            TabsMessage::ActivateStripSlot(slot) => {
                if let Some(msg) = self.strip_slot_target(slot) {
                    return Task::done(msg);
                }
            }
            TabsMessage::CopyTabAddress(idx) => {
                self.overlay = None;
                // Resolve through the focused pane's origin, not the tab
                // label: a split tab can hold panes on different hosts and
                // the label may be renamed. `CopyToClipboard` owns the write
                // (one clipboard access per process) and toasts only once the
                // runtime confirms it landed.
                let address = self
                    .tabs
                    .get(idx)
                    .map(|t| t.active().id)
                    .and_then(|pane_id| self.pane_origin_connection(pane_id))
                    .map(|c| c.hostname.clone());
                if let Some(address) = address {
                    return self.update(Message::CopyToClipboard(address));
                }
            }
            TabsMessage::ToggleTabFilesMode(idx) => return self.handle_toggle_tab_files_mode(idx),
            TabsMessage::ShowTabSurface(idx, surface) => {
                // The chip and the segments live on the strip, so the
                // overlay a right-click left open goes with them.
                self.overlay = None;
                return self.show_tab_surface(idx, surface);
            }
            TabsMessage::DetachTabSftp(idx) => return self.handle_detach_tab_sftp(idx),
            TabsMessage::CloseTabSftpSession(idx) => return self.handle_close_tab_sftp_session(idx),
            TabsMessage::OpenTerminalForSftpTab(idx) => return self.handle_open_terminal_for_sftp_tab(idx),
            TabsMessage::SsmKeepaliveTick => {
                // Toggle each SSM/ECS terminal between `base` and
                // `base - 1` rows. Every tick is therefore a genuine size
                // change, which fires a SIGWINCH the plugin forwards to
                // SSM as a resize event, and resize events reset the
                // server's idle timer. No base means we're focused (the
                // ticker shouldn't be mounted then), so it's a no-op.
                if let Some((base_cols, base_rows)) = self.ssm_keepalive_base {
                    let shrunk = base_rows.saturating_sub(1).max(2);
                    for tab in self.tabs.iter().filter(|t| t.ssm_keepalive) {
                        for pane in tab.pane_grid.panes.values() {
                            if let Ok(mut state) = pane.terminal.lock() {
                                let target = if state.rows() == base_rows {
                                    shrunk
                                } else {
                                    base_rows
                                };
                                state.resize(base_cols, target);
                            }
                        }
                    }
                }
            }
            TabsMessage::BusyAnimTick => {
                // The increment IS the re-render: the strip derives the
                // marching-dots frame from this counter (issue #146).
                self.busy_anim_tick = self.busy_anim_tick.wrapping_add(1);
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
