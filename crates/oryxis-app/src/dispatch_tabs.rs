//! `Oryxis::handle_tabs`, match arms for the tab strip + tab modals
//! (new-tab picker, tab-jump, icon picker), card hover/menu, folder
//! actions, window chrome (drag/resize/min/max/close).

#![allow(clippy::result_large_err)]

use iced::{Point, Task};

use crate::app::{Message, Oryxis};
use crate::state::{OverlayContent, OverlayState, View};

/// Smallest gap between two `WindowDrag` / `WindowResizeDrag`
/// presses we'll accept. iced's `MouseArea` re-fires `on_press` on
/// the second click of a double-click before `on_double_click` lands;
/// honouring that second drag races our `toggle_maximize` /
/// `WindowExpand*` follow-up. `300ms` is wider than any realistic
/// double-click and short enough that a deliberate two-quick-clicks-
/// to-drag still feels responsive.
const WINDOW_PRESS_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

impl Oryxis {
    /// Returns `true` when this press should be forwarded to the OS.
    /// Returns `false` when the previous press was within
    /// [`WINDOW_PRESS_DEBOUNCE`], swallowing the spurious second
    /// `on_press` that a double-click emits.
    pub(crate) fn consume_window_press(&mut self) -> bool {
        let now = std::time::Instant::now();
        let allow = self
            .last_window_press_at
            .is_none_or(|prev| now.duration_since(prev) >= WINDOW_PRESS_DEBOUNCE);
        if allow {
            self.last_window_press_at = Some(now);
        }
        allow
    }

    pub(crate) fn handle_tabs(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Card interactions --
            Message::CardHovered(idx) => {
                self.hovered_card = Some(idx);
            }
            Message::CardUnhovered => {
                self.hovered_card = None;
            }
            Message::FolderCardHovered(gid) => {
                self.hovered_folder_card = Some(gid);
            }
            Message::FolderCardUnhovered => {
                self.hovered_folder_card = None;
            }
            Message::KeyCardHovered(idx) => {
                self.hovered_key_card = Some(idx);
            }
            Message::KeyCardUnhovered => {
                self.hovered_key_card = None;
            }
            Message::IdentityCardHovered(idx) => {
                self.hovered_identity_card = Some(idx);
            }
            Message::SnippetCardHovered(idx) => {
                self.hovered_snippet_card = Some(idx);
            }
            Message::SnippetCardUnhovered => {
                self.hovered_snippet_card = None;
            }
            Message::IdentityCardUnhovered => {
                self.hovered_identity_card = None;
            }
            Message::MouseMoved(pos) => {
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
                let needs_drag_update = self.chat_sidebar_drag.is_some()
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
                    return Ok(Task::none());
                }
                self.mouse_position = if needs_drag_update { pos } else { snapped };
                // While the chat-sidebar resize handle is held down, the
                // sidebar width tracks the cursor, dragging left grows
                // the panel, dragging right shrinks it. Clamp to a sane
                // band so the user can't accidentally make it unusable.
                if let Some((start_x, start_width)) = self.chat_sidebar_drag {
                    let new_width = (start_width - (pos.x - start_x)).clamp(260.0, 700.0);
                    self.chat_sidebar_width = new_width;
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
            }
            Message::WindowResized(size) => {
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
                }
            }
            Message::WindowFocusChanged(focused) => {
                self.window_focused = focused;
                if focused {
                    // Restore any SSM/ECS terminal the keepalive may have
                    // left at `rows - 1` (it nudges and lets the next
                    // draw snap back, but no draw fires while the tab is
                    // off-screen). Explicit so a refocus is always clean.
                    if let Some((cols, rows)) = self.ssm_keepalive_base.take() {
                        for tab in self.tabs.iter().filter(|t| t.ssm_keepalive) {
                            for pane in tab.pane_grid.panes.values() {
                                if let Ok(mut state) = pane.terminal.lock() {
                                    state.resize(cols, rows);
                                }
                            }
                        }
                    }
                } else {
                    // Anchor the keepalive toggle to the size the window
                    // had when it lost focus. All plugin tabs share the
                    // window, so the first one's size is representative.
                    self.ssm_keepalive_base = self
                        .tabs
                        .iter()
                        .filter(|t| t.ssm_keepalive)
                        .find_map(|t| {
                            t.pane_grid.panes.values().next().and_then(|p| {
                                p.terminal
                                    .lock()
                                    .ok()
                                    .map(|s| (s.cols(), s.rows()))
                            })
                        });
                }
            }
            Message::SsmKeepaliveTick => {
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
            Message::WindowDrag => {
                if !self.consume_window_press() {
                    return Ok(Task::none());
                }
                return Ok(iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::drag(id),
                    None => Task::none(),
                }));
            }
            Message::WindowResizeDrag(direction) => {
                // Ignore resize requests while maximized, the window has no
                // borders to grab and the OS will reject/misbehave on WinIt.
                if self.window_maximized {
                    return Ok(Task::none());
                }
                if !self.consume_window_press() {
                    return Ok(Task::none());
                }
                return Ok(iced::window::latest().then(move |id_opt| match id_opt {
                    Some(id) => iced::window::drag_resize(id, direction),
                    None => Task::none(),
                }));
            }
            Message::WindowExpandVertical => {
                if self.window_maximized {
                    return Ok(Task::none());
                }
                let current_width = self.window_size.width;
                return Ok(iced::window::latest().then(move |id_opt| {
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
                }));
            }
            Message::WindowMinimize => {
                // Custom title bar minimize. Honours
                // setting_minimize_to_tray on Windows by hiding the
                // window outright instead of minimizing (which would
                // leave a taskbar slot). Everywhere else and when
                // the toggle is off we fall through to the real
                // iced::window::minimize call.
                if self.setting_minimize_to_tray && cfg!(target_os = "windows") {
                    self.is_window_hidden = true;
                    self.broadcast_ipc_state_if_child();
                    return Ok(iced::window::oldest()
                        .and_then(|id| {
                            iced::window::run(id, |window| {
                                crate::tray::hide_window(window);
                            })
                        })
                        .discard());
                }
                return Ok(iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::minimize(id, true),
                    None => Task::none(),
                }));
            }
            Message::WindowMaximizeToggle => {
                self.window_maximized = !self.window_maximized;
                return Ok(iced::window::latest().then(|id_opt| match id_opt {
                    Some(id) => iced::window::toggle_maximize(id),
                    None => Task::none(),
                }));
            }
            Message::WindowClose => {
                // Persist any buffered session-log output before the
                // window goes away (real close or hide-to-tray both).
                self.flush_session_logs_final();
                // Honour the close-to-tray setting: when on, the
                // user's "close" verb (custom title bar X, Alt+F4
                // via CloseRequested subscription, etc.) hides the
                // window into the tray instead of quitting. Returns
                // a hide task on Windows where the tray is real; on
                // other platforms the helper is a no-op so we fall
                // through to a real close. Default (off) closes for
                // everyone.
                if self.setting_close_to_tray && cfg!(target_os = "windows") {
                    self.is_window_hidden = true;
                    self.broadcast_ipc_state_if_child();
                    return Ok(iced::window::oldest()
                        .and_then(|id| {
                            iced::window::run(id, |window| {
                                crate::tray::hide_window(window);
                            })
                        })
                        .discard());
                }
                // Real close (not tray-hide): gracefully drain the plugin
                // subprocesses (flush logs / close SDK clients on stdin EOF)
                // before the window closes and the process exits. Providers
                // drain in parallel; the whole thing is time-bounded so a
                // wedged plugin can't hold the app open.
                let providers: Vec<std::sync::Arc<crate::plugins::PluginProvider>> =
                    self.plugin_providers.values().cloned().collect();
                return Ok(Task::perform(
                    async move {
                        let drain = futures_util::future::join_all(
                            providers.iter().map(|p| p.shutdown()),
                        );
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_millis(2000),
                            drain,
                        )
                        .await;
                    },
                    |_: ()| Message::NoOp,
                )
                .then(|_| {
                    iced::window::latest().then(|id_opt| match id_opt {
                        Some(id) => iced::window::close(id),
                        None => Task::none(),
                    })
                }));
            }
            Message::WindowFullscreenToggle => {
                // Optimistic local flip mirrors `WindowMaximizeToggle`,
                // the only way fullscreen changes today is through this
                // handler so the cached bool stays in sync.
                self.window_fullscreen = !self.window_fullscreen;
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
                        |_| Message::FullscreenHintHide,
                    );
                    return Ok(Task::batch([mode_task, hide_task]));
                }
                self.fullscreen_hint_visible = false;
                return Ok(mode_task);
            }
            Message::FullscreenHintHide => {
                self.fullscreen_hint_visible = false;
            }
            Message::SpawnNewWindow => {
                // Burger menu fires this. Drop both the context-menu
                // overlay AND the burger panel itself so the menu
                // doesn't linger on top of the freshly-spawned window.
                // The burger lives in its own `show_burger_menu` flag
                // (not `OverlayState`), so clearing `self.overlay`
                // alone wasn't enough.
                self.overlay = None;
                self.show_burger_menu = false;
                self.spawn_oryxis_child(None);
            }
            Message::ActivateStripSlot(slot) => {
                if let Some(msg) = self.strip_slot_target(slot) {
                    return Ok(Task::done(msg));
                }
            }
            Message::FocusViewSearch => {
                if let Some(id) = self.active_view_search_id() {
                    return Ok(iced::widget::operation::focus(id));
                }
            }
            Message::HideOverlayMenu => {
                self.overlay = None;
                self.card_context_menu = None;
                self.snippet_context_menu = None;
                self.key_context_menu = None;
                self.identity_context_menu = None;
                self.show_keychain_add_menu = false;
            }
            Message::ShowCardMenu(idx) => {
                if self.card_context_menu == Some(idx) {
                    self.card_context_menu = None;
                    self.overlay = None;
                } else {
                    self.card_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::HostActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::HideCardMenu => {
                self.card_context_menu = None;
                self.overlay = None;
            }

            // -- Tabs --
            Message::SelectTab(idx) => {
                if idx < self.tabs.len() {
                    // Lazy reopen: a dormant pinned tab (restored at boot) has
                    // no live session; entering it the first time connects.
                    if self.tabs[idx].pending_reopen.is_some() {
                        return Ok(self.reopen_dormant_tab(idx));
                    }
                    // Switching tabs dismisses the session-group editor (it's
                    // tied to the tab it was opened from).
                    self.show_session_group_panel = false;
                    self.active_tab = Some(idx);
                    self.remember_terminal_tab_focus(idx);
                    self.active_view = View::Terminal;
                    return Ok(self.tab_scroll_to_active());
                }
            }
            Message::TabHovered(idx) => {
                self.hovered_tab = Some(idx);
                // Live-slide: while a drag is active, entering another tab in
                // the same group slides the dragged tab into that slot right
                // away. Stable because after the move the dragged tab sits
                // under the cursor, so it won't re-trigger until the cursor
                // crosses into a genuinely different tab.
                if let Some(drag) = self.tab_drag.filter(|d| d.active)
                    && let Some(from) = self.tabs.iter().position(|t| t._id == drag.from_id)
                    && from != idx
                    && idx < self.tabs.len()
                    && self.tabs[from].pinned == self.tabs[idx].pinned
                {
                    self.move_tab(from, idx);
                }
            }
            Message::TabUnhovered => {
                self.hovered_tab = None;
            }
            Message::ShowNewTabPicker => {
                // Opening the picker from the `+` button always targets a new
                // tab, never a split (only SplitPane sets that).
                self.overlay = None; // dismiss the `+` hover popover if open
                self.pending_pane_split = None;
                self.show_new_tab_picker = true;
                self.new_tab_picker_search.clear();
                self.new_tab_picker_group = None;
            }
            Message::HideNewTabPicker => {
                self.show_new_tab_picker = false;
                self.pending_pane_split = None;
                self.new_tab_picker_group = None;
            }
            Message::NewTabPickerOpenGroup(gid) => {
                // Drill into the group; the search box now filters this
                // group's members instead of the top-level list, so clear
                // the leftover top-level needle.
                self.new_tab_picker_group = Some(gid);
                self.new_tab_picker_search.clear();
                // Cloud-query group: kick off (or refresh) the resolve so
                // the ECS tasks / K8s pods load. Reuses the same TTL gate
                // as the dashboard's OpenGroup so we don't hammer the API.
                if self.dynamic_group_needs_resolve(gid) {
                    return Ok(self
                        .handle_cloud(Message::DynamicGroupResolve(gid))
                        .unwrap_or_else(|_| Task::none()));
                }
            }
            Message::NewTabPickerBack => {
                self.new_tab_picker_group = None;
                self.new_tab_picker_search.clear();
            }
            Message::PickLocalShell => {
                self.show_new_tab_picker = false;
                if let Some((tab_idx, target, axis)) = self.pending_pane_split.take() {
                    return Ok(self.local_shell_into_pane(tab_idx, target, axis));
                }
                // No split pending: open a local shell in a new tab.
                return Ok(self.update(Message::OpenLocalShell));
            }
            Message::ShowTabJump => {
                self.show_tab_jump = true;
                self.tab_jump_search.clear();
            }
            Message::ToggleBurgerMenu => {
                self.show_burger_menu = !self.show_burger_menu;
            }
            Message::HideTabJump => {
                self.show_tab_jump = false;
            }
            Message::TabJumpSearchChanged(v) => {
                self.tab_jump_search = v;
            }
            Message::TabBarWheel(dy) => {
                // Vertical wheel over the tab bar scrolls horizontally
                // iced's horizontal-only scrollable ignores y deltas, so
                // we translate them via scroll_by here. Sign flip so
                // wheel-down brings later tabs into view (matches the
                // direction Chrome/VS Code use).
                return Ok(iced::widget::operation::scroll_by(
                    iced::widget::Id::new("tab-scroll"),
                    iced::widget::scrollable::AbsoluteOffset { x: -dy, y: 0.0 },
                ));
            }
            Message::TabJumpSelect(inner) => {
                self.show_tab_jump = false;
                return Ok(Task::done(*inner));
            }
            Message::NoOp => {}
            Message::NewTabPickerSearchChanged(v) => {
                self.new_tab_picker_search = v;
            }
            Message::ShowIconPicker(conn_id) => {
                // Pre-fill the picker with the icon the user is
                // currently seeing on the host card: custom override
                // first, then auto-detected OS, then the generic
                // "server" fallback as last resort. Using just
                // `custom_icon || "server"` here was buggy: hosts
                // whose icon comes from `detected_os` (Ubuntu, etc.)
                // showed "server" highlighted in the picker, so a
                // user clicking Save (even just to change the color)
                // accidentally overrode the auto-detected icon with
                // the generic stack glyph.
                if let Some(conn) = self.connections.iter().find(|c| c.id == conn_id) {
                    self.icon_picker_icon = conn
                        .custom_icon
                        .clone()
                        .or_else(|| conn.detected_os.clone())
                        .or_else(|| Some("server".to_string()));
                    self.icon_picker_color = conn.custom_color.clone();
                    self.icon_picker_hex_input = conn.custom_color.clone().unwrap_or_default();
                }
                self.icon_picker_icon_search.clear();
                self.icon_color_popover = None;
                self.icon_picker_for = Some(conn_id);
                self.show_icon_picker = true;
            }
            Message::HideIconPicker => {
                self.show_icon_picker = false;
                self.icon_picker_for = None;
                self.icon_picker_for_group_form = false;
                self.icon_picker_for_session_group = false;
                self.icon_picker_icon_search.clear();
                self.icon_color_popover = None;
            }
            Message::IconPickerSelectIcon(name) => {
                self.icon_picker_icon = Some(name);
            }
            Message::IconPickerIconSearchChanged(q) => {
                self.icon_picker_icon_search = q;
            }
            Message::IconPickerOpenColorPopover => {
                self.icon_color_popover = Some(self.mouse_position);
            }
            Message::IconPickerCloseColorPopover => {
                self.icon_color_popover = None;
            }
            Message::IconPickerSelectColor(hex) => {
                self.icon_picker_hex_input = hex.clone();
                self.icon_picker_color = Some(hex);
            }
            Message::IconPickerHexInputChanged(v) => {
                self.icon_picker_hex_input = v.clone();
                // Validate + commit only on well-formed #RRGGBB.
                let trimmed = v.trim().trim_start_matches('#');
                if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    self.icon_picker_color = Some(format!("#{}", trimmed.to_uppercase()));
                }
            }
            Message::IconPickerSave => {
                if self.icon_picker_for_session_group {
                    // Deferred save: flow the choice into the session-group
                    // editor form; the form's own Save persists it.
                    self.editor_session_group.icon_style = self.icon_picker_icon.clone();
                    self.editor_session_group.color = self.icon_picker_color.clone();
                } else if self.icon_picker_for_group_form {
                    // Form-target: flow the choice back to the dynamic
                    // group editor fields. The form's own Save button
                    // persists to the vault, so the icon picker stays
                    // an in-memory editor here.
                    self.cloud_dynamic_form_icon =
                        self.icon_picker_icon.clone().unwrap_or_default();
                    self.cloud_dynamic_form_color =
                        self.icon_picker_color.clone().unwrap_or_default();
                } else if let Some(conn_id) = self.icon_picker_for {
                    let icon = self.icon_picker_icon.clone();
                    let color = self.icon_picker_color.clone();
                    if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                        conn.custom_icon = icon.clone();
                        conn.custom_color = color.clone();
                        // Full save so the row persists (and other fields
                        // aren't accidentally overwritten).
                        if let Some(vault) = &self.vault {
                            let _ = vault.save_connection(conn, None);
                        }
                    }
                }
                self.show_icon_picker = false;
                self.icon_picker_for = None;
                self.icon_picker_for_group_form = false;
                self.icon_picker_for_session_group = false;
                self.icon_picker_icon_search.clear();
                self.icon_color_popover = None;
            }
            Message::IconPickerResetAuto => {
                // Clears the icon/color override, letting OS detection
                // drive the icon again on the next successful connect.
                // (Terminal-theme override is edited separately in the
                // host editor and is not touched here.)
                if self.icon_picker_for_session_group {
                    self.editor_session_group.icon_style = None;
                    self.editor_session_group.color = None;
                } else if self.icon_picker_for_group_form {
                    self.cloud_dynamic_form_icon = String::new();
                    self.cloud_dynamic_form_color = String::new();
                } else if let Some(conn_id) = self.icon_picker_for
                    && let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
                    conn.custom_icon = None;
                    conn.custom_color = None;
                    if let Some(vault) = &self.vault {
                        let _ = vault.save_connection(conn, None);
                    }
                }
                self.show_icon_picker = false;
                self.icon_picker_for = None;
                self.icon_picker_for_group_form = false;
                self.icon_picker_for_session_group = false;
                self.icon_color_popover = None;
            }
            Message::CloseTab(idx) => {
                // Also dismiss any open context menu so the menu doesn't linger
                // after the user clicks Close from it.
                self.overlay = None;
                // Closing a tab dismisses the session-group editor it spawned.
                self.show_session_group_panel = false;
                if idx < self.tabs.len() {
                    // Persist recorded output before the tab (and its
                    // panes' buffers) are dropped.
                    self.flush_session_logs_final();
                    // Actively tear down the tab's SSH sessions; the
                    // connect streams hold their own Arcs, so dropping
                    // the panes alone would leak the live sessions.
                    Self::close_tab_ssh_sessions(&self.tabs[idx]);
                    // Closing a pinned tab drops it from the persisted set.
                    let was_pinned = self.tabs[idx].pinned;
                    self.tabs.remove(idx);
                    if was_pinned {
                        self.persist_pinned_tabs();
                    }
                    // Keep the in-flight connection progress in sync with
                    // the tab list. Closing the connecting tab clears the
                    // progress (otherwise the stale screen, including a
                    // failed/timeout state, leaks into the next session,
                    // e.g. an ECS/SSM tab that doesn't set `connecting`).
                    // Closing an earlier tab shifts the connecting tab's
                    // index down by one so `SshRetry`/`SshCloseProgress`
                    // still target the right `self.tabs[..]` entry.
                    if let Some(ref mut progress) = self.connecting {
                        match progress.tab_idx.cmp(&idx) {
                            std::cmp::Ordering::Equal => self.connecting = None,
                            std::cmp::Ordering::Greater => progress.tab_idx -= 1,
                            std::cmp::Ordering::Less => {}
                        }
                    }
                    self.adjust_last_terminal_tab_after_remove(idx);
                    if self.tabs.is_empty() {
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        let i = idx.min(self.tabs.len() - 1);
                        self.active_tab = Some(i);
                        self.remember_terminal_tab_focus(i);
                    }
                }
            }
            Message::ShowTabMenu(idx) => {
                self.overlay = Some(OverlayState {
                    content: OverlayContent::TabActions(idx),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::ShowSplitMenu => {
                // Hover popover under `+`. Only meaningful with a terminal
                // tab open (something to split); otherwise `+` just opens a
                // new tab on click. Anchored under the cursor (over `+`).
                if self.active_view == View::Terminal
                    && self.active_tab.is_some()
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
            Message::SplitMenuEnter => {
                self.split_menu_hovered = true;
            }
            Message::SplitMenuLeave => {
                // Left the `+` button or the popover. Defer the close briefly
                // so moving from the button INTO the menu (which re-enters
                // via `SplitMenuEnter`) doesn't flap it shut.
                self.split_menu_hovered = false;
                return Ok(Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
                    },
                    |_| Message::SplitMenuCloseIfIdle,
                ));
            }
            Message::SplitMenuCloseIfIdle => {
                if !self.split_menu_hovered
                    && matches!(
                        self.overlay.as_ref().map(|o| &o.content),
                        Some(OverlayContent::SplitMenu)
                    )
                {
                    self.overlay = None;
                }
            }
            Message::ToggleTabPin(idx) => {
                self.overlay = None;
                if let Some(tab) = self.tabs.get_mut(idx) {
                    tab.pinned = !tab.pinned;
                }
                self.persist_pinned_tabs();
            }
            Message::ReconnectTab(idx) => {
                self.overlay = None;
                // Find the connection matching this tab's label; close the tab and
                // dispatch ConnectSsh for that connection index. Dead tabs (no matching
                // connection) are just closed.
                if let Some(tab) = self.tabs.get(idx) {
                    let base_label = tab.label.trim_end_matches(" (disconnected)").to_string();
                    let conn_idx = self.connections.iter().position(|c| c.label == base_label);
                    self.tabs.remove(idx);
                    self.adjust_last_terminal_tab_after_remove(idx);
                    if self.tabs.is_empty() {
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        let i = idx.min(self.tabs.len() - 1);
                        self.active_tab = Some(i);
                        self.remember_terminal_tab_focus(i);
                    }
                    if let Some(ci) = conn_idx {
                        // Toast "Reconnecting..." so the user sees feedback the
                        // moment the attempt actually starts (not when the
                        // disconnect was first detected, up to 30s earlier).
                        self.toast = Some(crate::i18n::t("disconnected_reconnecting").to_string());
                        return Ok(Task::batch(vec![
                            Task::done(Message::ConnectSsh(ci)),
                            Task::perform(
                                async {
                                    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                                },
                                |_| Message::ToastClear,
                            ),
                        ]));
                    }
                }
            }
            Message::DuplicateTab(idx) => {
                self.overlay = None;
                // Local shell tabs aren't backed by a saved connection; for
                // those we just open a fresh shell tab. SSH tabs find their
                // connection by label and dispatch `ConnectSsh` so the user
                // gets a second live session into the same box. Cloud tabs
                // (ECS Exec / kubectl) re-open via the relaunch message
                // stashed on the tab at spawn time.
                if let Some(tab) = self.tabs.get(idx) {
                    let is_local_shell = tab.active().ssh_session.is_none()
                        && tab.label == "Local Shell";
                    if is_local_shell {
                        return Ok(Task::done(Message::OpenLocalShell));
                    }
                    // Cloud tabs with no saved connection (ECS Exec,
                    // kubectl pod) carry the message that re-opens them.
                    if let Some(relaunch) = tab.relaunch.as_deref() {
                        return Ok(Task::done(relaunch.clone()));
                    }
                    // Connection-backed tabs (SSH, InstanceConnect, and
                    // SSM-into-EC2) duplicate by re-finding the host by
                    // label. SSM tabs carry a title prefix, strip it so
                    // the lookup matches; ConnectSsh re-routes to SSM via
                    // the cloud_ref transport check.
                    let base_label = tab
                        .label
                        .trim_end_matches(" (disconnected)")
                        .trim_start_matches(crate::app::SSM_TAB_PREFIX)
                        .to_string();
                    if let Some(ci) = self.connections.iter().position(|c| c.label == base_label) {
                        return Ok(Task::done(Message::ConnectSsh(ci)));
                    }
                }
            }
            Message::DuplicateInNewWindow(idx) => {
                self.overlay = None;
                self.spawn_oryxis_child(Some(idx));
            }
            Message::ShowFolderActions(gid) => {
                // Anchor the menu to the cursor, matches the host-card
                // "..." pattern. The global MouseMoved subscription keeps
                // `mouse_position` fresh.
                self.overlay = Some(OverlayState {
                    content: OverlayContent::FolderActions(gid),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::StartRenameFolder(gid) => {
                self.overlay = None;
                let current = self
                    .groups
                    .iter()
                    .find(|g| g.id == gid)
                    .map(|g| g.label.clone())
                    .unwrap_or_default();
                self.folder_rename = Some((gid, current));
            }
            Message::FolderRenameInput(val) => {
                if let Some((_, ref mut buf)) = self.folder_rename {
                    *buf = val;
                }
            }
            Message::ConfirmRenameFolder => {
                if let Some((gid, new_label)) = self.folder_rename.take() {
                    let trimmed = new_label.trim();
                    if !trimmed.is_empty()
                        && let Some(group) = self.groups.iter_mut().find(|g| g.id == gid)
                    {
                        group.label = trimmed.to_string();
                        group.updated_at = chrono::Utc::now();
                        if let Some(vault) = &self.vault {
                            let _ = vault.save_group(group);
                        }
                    }
                }
            }
            Message::CancelFolderModal => {
                self.folder_rename = None;
                self.folder_delete = None;
            }
            Message::StartDeleteFolder(gid) => {
                self.overlay = None;
                self.folder_delete = Some(gid);
            }
            Message::DeleteFolderKeepHosts => {
                if let Some(gid) = self.folder_delete.take() {
                    // Move every host inside the folder to the root.
                    for conn in self.connections.iter_mut() {
                        if conn.group_id == Some(gid) {
                            conn.group_id = None;
                            conn.updated_at = chrono::Utc::now();
                            if let Some(vault) = &self.vault {
                                let _ = vault.save_connection(conn, None);
                            }
                        }
                    }
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_group(&gid);
                    }
                    self.groups.retain(|g| g.id != gid);
                    if self.active_group == Some(gid) {
                        self.active_group = None;
                    }
                }
            }
            Message::DeleteFolderWithHosts => {
                if let Some(gid) = self.folder_delete.take() {
                    // Drop every host inside the folder, then the folder.
                    let to_drop: Vec<_> = self
                        .connections
                        .iter()
                        .filter(|c| c.group_id == Some(gid))
                        .map(|c| c.id)
                        .collect();
                    if let Some(vault) = &self.vault {
                        for cid in &to_drop {
                            let _ = vault.delete_connection(cid);
                        }
                        let _ = vault.delete_group(&gid);
                    }
                    self.connections.retain(|c| !to_drop.contains(&c.id));
                    self.groups.retain(|g| g.id != gid);
                    if self.active_group == Some(gid) {
                        self.active_group = None;
                    }
                }
            }
            Message::CloseOtherTabs(idx) => {
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
                    self.tabs.retain(|t| t._id == target_id || t.pinned);
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
            Message::CloseAllTabs => {
                self.overlay = None;
                let connecting_id = self
                    .connecting
                    .as_ref()
                    .and_then(|p| self.tabs.get(p.tab_idx))
                    .map(|t| t._id);
                // Pinned tabs survive "close all".
                self.tabs.retain(|t| t.pinned);
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

            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// First select of a dormant pinned tab: drop the placeholder and fire
    /// the saved spec to reopen it (connect host / spawn local shell). The
    /// freshly-opened tab inherits the pin.
    fn reopen_dormant_tab(&mut self, idx: usize) -> Task<Message> {
        use crate::state::PinnedTabSpec;
        let Some(spec) = self
            .tabs
            .get_mut(idx)
            .and_then(|t| t.pending_reopen.take())
        else {
            return Task::none();
        };
        // Resolve the open message fresh (the host id maps to a possibly
        // different index than last session; the connection may be gone).
        // Cloud sessions spawn asynchronously, so they can't ride the
        // synchronous len-check below; flag them instead.
        let mut cloud = false;
        let open = match &spec {
            PinnedTabSpec::Host { id, .. } => self
                .connections
                .iter()
                .position(|c| c.id == *id)
                .map(Message::ConnectSsh),
            PinnedTabSpec::LocalShell { program, args, label } => {
                Some(Message::OpenLocalShellWith {
                    program: program.clone(),
                    args: args.clone(),
                    label: label.clone(),
                })
            }
            PinnedTabSpec::EcsExec {
                group_id,
                task_id,
                task_label,
                container,
                ..
            } => {
                cloud = true;
                Some(Message::ConnectEcsExecTask {
                    group_id: *group_id,
                    task_id: task_id.clone(),
                    task_label: task_label.clone(),
                    container: container.clone(),
                })
            }
            PinnedTabSpec::KubectlExec {
                group_id,
                namespace,
                pod,
                container,
                ..
            } => {
                cloud = true;
                Some(Message::ConnectKubectlExecPod {
                    group_id: *group_id,
                    namespace: namespace.clone(),
                    pod: pod.clone(),
                    container: container.clone(),
                })
            }
        };
        if cloud {
            // Cloud sessions spawn asynchronously. Keep the dormant
            // placeholder in the strip (so its chip doesn't blink out) and let
            // `spawn_plugin_tab` replace it in place by id, inheriting its slot
            // + pin. We don't persist here: the dormant spec stays in the
            // setting as a safety net until the live tab re-persists. Show
            // Hosts during the connect instead of the dormant's placeholder
            // terminal.
            self.pin_next_plugin_tab = Some(self.tabs[idx]._id);
            self.active_tab = None;
            self.active_view = View::Dashboard;
            return open.map(|m| self.update(m)).unwrap_or_else(Task::none);
        }

        // Host / local: the connect appends a live tab synchronously, so
        // remove the placeholder and slot the live tab into its place.
        self.tabs.remove(idx);
        self.adjust_last_terminal_tab_after_remove(idx);

        let before = self.tabs.len();
        let task = open.map(|m| self.update(m)).unwrap_or_else(Task::none);
        if self.tabs.len() > before {
            // A live tab was appended at the end; move it back to the
            // dormant's old slot so reopening doesn't reorder, and pin it.
            let live = self.tabs.pop().expect("a tab was just appended");
            let at = idx.min(self.tabs.len());
            self.tabs.insert(at, live);
            self.tabs[at].pinned = true;
            self.active_tab = Some(at);
            self.remember_terminal_tab_focus(at);
            self.active_view = View::Terminal;
            // ConnectSsh set `connecting.tab_idx` to the append index; the
            // move just shifted it, so retarget the progress overlay.
            if let Some(p) = &mut self.connecting
                && p.tab_idx == before
            {
                p.tab_idx = at;
            }
        } else if self.tabs.is_empty() {
            // Nothing reopened (e.g. the host was deleted) and no tabs left.
            self.active_tab = None;
            self.active_view = View::Dashboard;
        } else {
            // Nothing reopened but other tabs remain: clamp the selection so
            // `active_tab` never dangles past the removed placeholder.
            let i = idx.min(self.tabs.len() - 1);
            self.active_tab = Some(i);
            self.remember_terminal_tab_focus(i);
        }
        self.persist_pinned_tabs();
        Task::batch([task, self.tab_scroll_to_active()])
    }

    /// Reorder a tab by dropping `from` onto `to` (a drag commit). Restricted
    /// to within the same group: a pinned tab can only move among pinned
    /// tabs, a normal tab among normal tabs, so the pinned-first strip layout
    /// stays consistent and the visual index maps to the real index. The
    /// active tab and any in-flight connection follow their tab (by id).
    pub(crate) fn move_tab(&mut self, from: usize, to: usize) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        if self.tabs[from].pinned != self.tabs[to].pinned {
            return;
        }
        let active_id = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t._id);
        let connecting_id = self
            .connecting
            .as_ref()
            .and_then(|p| self.tabs.get(p.tab_idx))
            .map(|t| t._id);
        let tab = self.tabs.remove(from);
        let dest = to.min(self.tabs.len());
        self.tabs.insert(dest, tab);
        // Re-anchor active / connecting to their tabs by id.
        if let Some(aid) = active_id
            && let Some(i) = self.tabs.iter().position(|t| t._id == aid)
        {
            self.active_tab = Some(i);
            self.remember_terminal_tab_focus(i);
        }
        if let Some(cid) = connecting_id
            && let Some(i) = self.tabs.iter().position(|t| t._id == cid)
            && let Some(p) = self.connecting.as_mut()
        {
            p.tab_idx = i;
        }
        // Note: no persist here. Live-slide calls this on every crossed tab
        // during a drag; the pinned order is persisted once on drop.
    }

    /// Re-anchor (or clear) the in-flight connect progress after the tab
    /// list was filtered by close-others / close-all (both keep pinned
    /// tabs). `connecting_id` is the connecting tab's id captured *before*
    /// the filter: if that tab survived, point `tab_idx` at its new slot;
    /// if it was closed, drop the progress so a later SshRetry /
    /// SshCloseProgress can't `remove()` the wrong (surviving / pinned) tab.
    fn reanchor_connecting_after_filter(&mut self, connecting_id: Option<uuid::Uuid>) {
        if self.connecting.is_none() {
            return;
        }
        match connecting_id.and_then(|cid| self.tabs.iter().position(|t| t._id == cid)) {
            Some(i) => {
                if let Some(p) = self.connecting.as_mut() {
                    p.tab_idx = i;
                }
            }
            None => self.connecting = None,
        }
    }
}
