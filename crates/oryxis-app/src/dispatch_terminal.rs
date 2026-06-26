//! `Oryxis::handle_terminal`, match arms for terminal I/O: PTY bytes
//! coming back, keyboard events routed to the active tab, mouse moves
//! tracked for hit-testing, window resize/drag/min/max/close.

#![allow(clippy::result_large_err)]

use iced::keyboard;
use iced::Task;

use crate::app::{Message, Oryxis};
use crate::util::{ctrl_key_bytes, key_to_named_bytes};

/// Flush a pane's recorded-output buffer to the vault once it reaches
/// this size, so a burst (e.g. an `apt upgrade` dump) doesn't sit in
/// RAM unbounded between the periodic flush ticks.
const SESSION_LOG_FLUSH_BYTES: usize = 64 * 1024;

impl Oryxis {
    /// Drain every pane's recorded-output buffer into the vault, one
    /// append per pane. Driven by the size threshold, the flush tick,
    /// disconnect, and window close, so the vault sees batched writes
    /// instead of one per SSH chunk (the old per-chunk path rewrote the
    /// whole growing blob and hammered the disk).
    ///
    /// Secrets/PII are scrubbed per flushed chunk (`session_redact`).
    /// Patterns can't match across chunk boundaries, so the periodic
    /// (non-final) flush holds back everything after the buffer's last
    /// newline; the partial line rides along to the next flush unless
    /// the buffer is oversized anyway.
    pub(crate) fn flush_session_logs(&mut self) {
        self.flush_session_logs_inner(false);
    }

    /// Flush including trailing partial lines. Use when the pane, tab,
    /// session, or window is going away (or the log is about to be
    /// read), so the recorded tail isn't lost.
    pub(crate) fn flush_session_logs_final(&mut self) {
        self.flush_session_logs_inner(true);
    }

    /// Tear down every SSH session in a tab. Dropping the pane alone
    /// is not enough: the connect stream task holds its own Arc to the
    /// session, so without an explicit close() the engine tasks, the
    /// channel, and any per-connection port-forward listeners keep
    /// running (and generating UI messages) forever.
    pub(crate) fn close_tab_ssh_sessions(tab: &crate::state::TerminalTab) {
        for pane in tab.pane_grid.panes.values() {
            if let Some(session) = &pane.ssh_session {
                session.close();
            }
        }
    }

    fn flush_session_logs_inner(&mut self, final_flush: bool) {
        let mut pending: Vec<(uuid::Uuid, Vec<u8>)> = Vec::new();
        for tab in &mut self.tabs {
            for pane in tab.pane_grid.panes.values_mut() {
                if let Some(log_id) = pane.session_log_id
                    && !pane.session_log_buf.is_empty()
                {
                    let buf = &mut pane.session_log_buf;
                    let take = if final_flush || buf.len() >= SESSION_LOG_FLUSH_BYTES {
                        buf.len()
                    } else {
                        // Hold back the partial trailing line so a secret
                        // mid-echo isn't split across redaction chunks.
                        match buf.iter().rposition(|&b| b == b'\n') {
                            Some(pos) => pos + 1,
                            None => 0,
                        }
                    };
                    if take == 0 {
                        continue;
                    }
                    let tail = buf.split_off(take);
                    let head = std::mem::replace(buf, tail);
                    pending.push((log_id, head));
                }
            }
        }
        if pending.is_empty() {
            return;
        }
        if let Some(vault) = &self.vault {
            for (log_id, bytes) in pending {
                let scrubbed = crate::session_redact::redact_secrets(&bytes);
                if let Err(e) = vault.append_session_data(&log_id, &scrubbed) {
                    tracing::warn!("session log append failed for {log_id}: {e}");
                }
            }
        }
    }

    /// Paste `text` into the active tab's session, wrapping it for
    /// bracketed-paste when the focused app enabled it. Routes to the SSH
    /// session when one is attached, otherwise the local PTY. Shared by the
    /// clipboard (right-click / Ctrl+Shift+V) and PRIMARY (middle-click)
    /// paste paths.
    pub(crate) fn paste_text_into_active(&mut self, text: &str) {
        if let Some(tab_idx) = self.active_tab
            && let Some(tab) = self.tabs.get(tab_idx)
        {
            let bracketed = tab
                .active()
                .terminal
                .lock()
                .map(|s| s.bracketed_paste_enabled())
                .unwrap_or(false);
            let payload = oryxis_terminal::wrap_paste(text, bracketed);
            if let Some(ref ssh) = tab.active().ssh_session {
                let _ = ssh.write(&payload);
            } else if let Ok(mut state) = tab.active().terminal.lock() {
                state.write(&payload);
            }
        }
    }

    /// Read the system clipboard and paste it into the active session.
    /// Shared by the Ctrl+V, Ctrl+Shift+V and Cmd+V (macOS) key paths so
    /// the bracketed-paste handling lives in exactly one place.
    fn paste_clipboard_into_active(&mut self) {
        if let Ok(mut clip) = arboard::Clipboard::new()
            && let Ok(text) = clip.get_text()
        {
            self.paste_text_into_active(&text);
        }
    }

    pub(crate) fn handle_terminal(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Split panes --
            Message::FocusPane(pane) => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    tab.focused = pane;
                }
            }
            Message::ResizePane(ev) => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                {
                    tab.pane_grid.resize(ev.split, ev.ratio);
                }
            }
            Message::SplitPane(axis) => {
                // Open the connection picker to choose what fills the new
                // pane (a host, or a local shell). The selection routes into
                // a split via `pending_pane_split` instead of a new tab.
                self.overlay = None; // dismiss the `+` hover popover if open
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get(tab_idx)
                {
                    self.pending_pane_split = Some((tab_idx, tab.focused, axis));
                    self.show_new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    self.new_tab_picker_group = None;
                }
            }
            Message::SplitTabPane(tab_idx, axis) => {
                // From a tab's right-click menu: focus that tab first, then
                // open the picker to fill the new split pane.
                self.overlay = None;
                if let Some(tab) = self.tabs.get(tab_idx) {
                    let target = tab.focused;
                    self.active_tab = Some(tab_idx);
                    self.active_view = crate::state::View::Terminal;
                    self.remember_terminal_tab_focus(tab_idx);
                    self.pending_pane_split = Some((tab_idx, target, axis));
                    self.show_new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    self.new_tab_picker_group = None;
                }
            }
            Message::ClosePane => {
                let Some(tab_idx) = self.active_tab else {
                    return Ok(Task::none());
                };
                // Last pane in the tab: closing it closes the whole tab.
                if self.tabs[tab_idx].pane_grid.panes.len() <= 1 {
                    return Ok(self.update(Message::CloseTab(tab_idx)));
                }
                // Persist the closing pane's recorded output before it goes.
                self.flush_session_logs_final();
                let tab = &mut self.tabs[tab_idx];
                let target = tab.focused;
                // Tear down the pane's SSH session (the connect stream
                // holds its own Arc; see close_tab_ssh_sessions).
                if let Some(pane) = tab.pane_grid.panes.get(&target)
                    && let Some(session) = &pane.ssh_session
                {
                    session.close();
                }
                if let Some((_closed, sibling)) = tab.pane_grid.close(target) {
                    tab.focused = sibling;
                }
            }
            Message::FocusPaneDir(dir) => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(tab_idx)
                    && let Some(adj) = tab.pane_grid.adjacent(tab.focused, dir)
                {
                    tab.focused = adj;
                }
            }
            // -- Terminal I/O --
            Message::PtyOutput(pane_id, bytes) => {
                // Route to the specific pane (a tab may have several, each
                // with its own PTY). Scan is trivial at these counts.
                let mut over_threshold = false;
                let mut schedule_flush: Option<std::time::Duration> = None;
                // Snapshot the (Copy) bell mode before borrowing self.tabs; the
                // bell action runs while the pane is borrowed.
                let bell_mode = self.setting_bell_mode;
                let mut flash_pane: Option<uuid::Uuid> = None;
                if let Some(pane) = self
                    .tabs
                    .iter_mut()
                    .flat_map(|t| t.pane_grid.panes.values_mut())
                    .find(|p| p.id == pane_id)
                {
                    let mut sync_deadline = None;
                    let mut new_title = None;
                    let mut bell_rang = false;
                    let mut new_cwd = None;
                    let mut new_marks = Vec::new();
                    if let Ok(mut state) = pane.terminal.lock() {
                        state.process(&bytes);
                        // A buffering DEC ?2026 update reports its abort
                        // deadline here; read it while still locked.
                        sync_deadline = state.sync_timeout();
                        // OSC 0/2 title set by the shell this batch (or an
                        // empty string for ResetTitle). Captured unconditionally;
                        // the auto-title setting only gates display.
                        new_title = state.take_title();
                        bell_rang = state.take_bell();
                        // OSC 7 working directory + OSC 133 shell-integration
                        // marks (the latter stored as command-history groundwork).
                        new_cwd = state.take_cwd();
                        new_marks = state.take_shell_marks();
                    }
                    if let Some(cwd) = new_cwd {
                        pane.cwd = Some(cwd);
                    }
                    if !new_marks.is_empty() {
                        pane.shell_marks.extend(new_marks);
                        let len = pane.shell_marks.len();
                        if len > 256 {
                            pane.shell_marks.drain(0..len - 256);
                        }
                    }
                    if let Some(title) = new_title {
                        // Stored raw: when auto-title is on it's opt-in emulator
                        // behavior, so the tab shows exactly what the shell set
                        // (`user@host: ~`, `vim file`, …), like gnome-terminal /
                        // iTerm / Windows Terminal do.
                        let trimmed = title.trim();
                        pane.osc_title = (!trimmed.is_empty()).then(|| trimmed.to_string());
                    }
                    if bell_rang {
                        match bell_mode {
                            crate::util::BellMode::Off => {}
                            crate::util::BellMode::Beep => crate::util::play_system_beep(),
                            crate::util::BellMode::Flash => {
                                pane.bell_flash = true;
                                flash_pane = Some(pane_id);
                            }
                        }
                    }
                    // Rising edge only: arm one flush timer per update, not
                    // one per coalesced output batch. The flag clears when the
                    // update closes normally (deadline gone) or when the
                    // `TerminalSyncFlush` handler fires.
                    match sync_deadline {
                        Some(deadline) if !pane.sync_flush_scheduled => {
                            pane.sync_flush_scheduled = true;
                            schedule_flush = Some(deadline.saturating_duration_since(
                                std::time::Instant::now(),
                            ));
                        }
                        None => pane.sync_flush_scheduled = false,
                        _ => {}
                    }
                    // Buffer the bytes; the vault write is batched (see
                    // `flush_session_logs`). Flush early once the buffer
                    // grows large so a burst doesn't balloon in RAM.
                    if pane.session_log_id.is_some() {
                        pane.session_log_buf.extend_from_slice(&bytes);
                        over_threshold =
                            pane.session_log_buf.len() >= SESSION_LOG_FLUSH_BYTES;
                    }
                }
                if over_threshold {
                    self.flush_session_logs();
                }
                // Session-group per-pane startup script for LOCAL panes. SSH
                // panes inject on `SshConnected`, but a local shell has no
                // such ready event, so we gate on its first output (the
                // prompt) to be sure the shell is reading stdin.
                if self.pane_script_overrides.contains_key(&pane_id) {
                    let is_local = self
                        .tabs
                        .iter()
                        .flat_map(|t| t.pane_grid.panes.values())
                        .find(|p| p.id == pane_id)
                        .map(|p| matches!(p.origin, crate::state::PaneOrigin::Local(_)))
                        .unwrap_or(false);
                    if is_local
                        && let Some(script) = self.pane_script_overrides.remove(&pane_id)
                        && let Some(pane) = self
                            .tabs
                            .iter()
                            .flat_map(|t| t.pane_grid.panes.values())
                            .find(|p| p.id == pane_id)
                        && let Ok(mut state) = pane.terminal.lock()
                    {
                        state.write(format!("{script}\n").as_bytes());
                    }
                }
                // Arm the one-shot flush for a synchronized update that
                // stalled with output buffered. Fires `flush_sync` at the
                // 150 ms deadline so a never-closed `?2026` can't leave the
                // screen frozen (see `TerminalSyncFlush`).
                let mut tasks: Vec<iced::Task<Message>> = Vec::new();
                if let Some(remaining) = schedule_flush {
                    tasks.push(Task::perform(
                        async move {
                            tokio::time::sleep(remaining).await;
                        },
                        move |_| Message::TerminalSyncFlush(pane_id),
                    ));
                }
                if let Some(fp) = flash_pane {
                    // Clear the visual-bell flash after a brief window.
                    tasks.push(Task::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                        },
                        move |_| Message::TerminalBellFlashEnd(fp),
                    ));
                }
                if !tasks.is_empty() {
                    return Ok(Task::batch(tasks));
                }
            }
            Message::TerminalBellFlashEnd(pane_id) => {
                if let Some(pane) = self
                    .tabs
                    .iter_mut()
                    .flat_map(|t| t.pane_grid.panes.values_mut())
                    .find(|p| p.id == pane_id)
                {
                    pane.bell_flash = false;
                }
            }
            Message::TerminalSyncFlush(pane_id) => {
                if let Some(pane) = self
                    .tabs
                    .iter_mut()
                    .flat_map(|t| t.pane_grid.panes.values_mut())
                    .find(|p| p.id == pane_id)
                {
                    pane.sync_flush_scheduled = false;
                    let mut reschedule: Option<std::time::Duration> = None;
                    if let Ok(mut state) = pane.terminal.lock() {
                        match state.sync_timeout() {
                            // The app extended the update past our deadline
                            // (a fresh BSU reset vte's 150 ms timer): re-arm
                            // for the new deadline instead of flushing
                            // mid-update, matching alacritty's behavior.
                            Some(deadline) if deadline > std::time::Instant::now() => {
                                reschedule = Some(deadline.saturating_duration_since(
                                    std::time::Instant::now(),
                                ));
                            }
                            // Deadline reached, update still open: force the
                            // buffered frame onto the grid.
                            Some(_) => state.flush_sync(),
                            // Closed normally in the meantime; nothing to do.
                            None => {}
                        }
                    }
                    if let Some(remaining) = reschedule {
                        pane.sync_flush_scheduled = true;
                        return Ok(Task::perform(
                            async move {
                                tokio::time::sleep(remaining).await;
                            },
                            move |_| Message::TerminalSyncFlush(pane_id),
                        ));
                    }
                }
            }
            // Periodic batched write of recorded output. Only mounted by
            // the subscription while at least one pane is recording.
            Message::SessionLogFlushTick => {
                self.flush_session_logs();
            }
            // Right-click paste from the terminal widget. Mirrors the
            // Ctrl+Shift+V path below: SSH session if active, local PTY
            // otherwise. Without this, the widget's fallback write only
            // reached the local PTY and right-click looked broken on
            // every SSH tab.
            Message::TerminalPasteFromClipboard => {
                if let Ok(mut clip) = arboard::Clipboard::new()
                    && let Ok(text) = clip.get_text()
                {
                    self.paste_text_into_active(&text);
                }
            }
            // Synthesized input from the terminal widget: mouse-tracking
            // reports (tmux `mouse on`, vim `mouse=a`, htop, ...) and the
            // wheel-to-arrow translation in alt-screen. Same SSH-or-local
            // routing as keystrokes; without this the widget's local-PTY
            // fallback would never reach the remote session.
            Message::TerminalInput(bytes) => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get(tab_idx)
                {
                    if let Some(ref ssh) = tab.active().ssh_session {
                        let _ = ssh.write(&bytes);
                    } else if let Ok(mut state) = tab.active().terminal.lock() {
                        state.write(&bytes);
                    }
                }
            }
            Message::KeyboardEvent(event) => {
                // Track modifier state for downstream consumers (SFTP
                // ctrl/shift-click selection). Always update first so
                // every later branch in this handler sees fresh state.
                if let keyboard::Event::ModifiersChanged(m) = &event {
                    self.modifiers = *m;
                }
                // PrintScreen -> open the Windows snip overlay (region
                // capture), matching the OS default. winit delivers the
                // key to the focused window without forwarding it to
                // DefWindowProc, so Windows' own PrintScreen handler never
                // fires while Oryxis is focused; we remap it explicitly.
                // VK_SNAPSHOT classically emits only WM_KEYUP, so accept a
                // press or a release, debounced so a paired press+release
                // doesn't launch the overlay twice. Handled before the
                // modal / chat / PTY gates so a screenshot always works.
                #[cfg(target_os = "windows")]
                {
                    let is_printscreen = matches!(
                        &event,
                        keyboard::Event::KeyPressed {
                            key: keyboard::Key::Named(keyboard::key::Named::PrintScreen),
                            ..
                        } | keyboard::Event::KeyReleased {
                            key: keyboard::Key::Named(keyboard::key::Named::PrintScreen),
                            ..
                        }
                    );
                    if is_printscreen {
                        let now = std::time::Instant::now();
                        let recent = self
                            .last_printscreen
                            .map(|t| {
                                now.duration_since(t) < std::time::Duration::from_millis(400)
                            })
                            .unwrap_or(false);
                        if !recent {
                            self.last_printscreen = Some(now);
                            crate::util::open_screenshot_tool();
                        }
                        return Ok(Task::none());
                    }
                }
                // Host editor panel open -> Tab / Shift+Tab move focus
                // between form fields like a browser, instead of falling
                // through to the PTY (which would emit a literal \t) or a
                // hotkey binding. focus_next / focus_previous walk iced's
                // real focus chain, so click-then-Tab works too.
                if self.show_host_panel
                    && let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab))
                {
                    return Ok(if modifiers.shift() {
                        iced::widget::operation::focus_previous()
                    } else {
                        iced::widget::operation::focus_next()
                    });
                }
                // Ctrl+Tab / Ctrl+Shift+Tab: move the strip focus one slot
                // forward / backward through [Home, pinned, tabs], wrapping
                // around (Home is part of the cycle). Positional, not MRU, so
                // it's fully deterministic. Handled here rather than via the
                // configurable hotkey table so it works from any surface (the
                // Home/vault views included). Consumed unconditionally so the
                // combo never leaks a literal \t into the PTY.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab))
                    && modifiers.control()
                    && !self.show_host_panel
                    && !self.any_modal_blocks_input()
                {
                    return Ok(self.cycle_strip_focus(!modifiers.shift()));
                }
                // Dashboard keyboard navigation: from the search field,
                // Tab / arrows move a selection across the visible host
                // cards and Enter connects (or connects the top result
                // while searching). Plain keys only, so Ctrl/Alt combos
                // still reach the hotkey table below. Gated to the Home /
                // Hosts surface with no editor panel or modal open.
                if self.active_view == crate::state::View::Dashboard
                    && self.active_tab.is_none()
                    && !self.show_host_panel
                    && !self.show_session_group_panel
                    && !self.cloud_dynamic_form_visible
                    && !self.cloud_discover_visible
                    && !self.any_modal_blocks_input()
                    && let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && !modifiers.control()
                    && !modifiers.alt()
                    && !modifiers.logo()
                {
                    use crate::app::DashNavItem;
                    use keyboard::key::Named;
                    let keyboard::Key::Named(named) = key else {
                        // Non-named keys (typing) fall through to the search.
                        if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                            && let Some(task) = self.handle_hotkey_keypress(key, modifiers)
                        {
                            return Ok(task);
                        }
                        return Ok(Task::none());
                    };
                    // Snapshot of the navigable items as visual rows
                    // (recorded during the last render).
                    let rows = self.dashboard_nav.borrow().clone();
                    let flat: Vec<DashNavItem> = rows.iter().flatten().copied().collect();
                    let list_mode = self.setting_host_list_view;
                    // Current (row, col) of the selection within `rows`.
                    let cur = self.selected_nav.and_then(|sel| {
                        rows.iter().enumerate().find_map(|(r, row)| {
                            row.iter().position(|&n| n == sel).map(|c| (r, c))
                        })
                    });
                    let flat_pos = self
                        .selected_nav
                        .and_then(|sel| flat.iter().position(|&n| n == sel));

                    // Enter / Escape act on the current selection.
                    if matches!(named, Named::Enter) {
                        let target = self.selected_nav.or_else(|| {
                            if self.host_search.is_empty() {
                                None
                            } else {
                                flat.first().copied()
                            }
                        });
                        if let Some(item) = target {
                            self.selected_nav = None;
                            let msg = match item {
                                DashNavItem::Group(gid) => Message::OpenGroup(gid),
                                DashNavItem::SessionGroup(i) => Message::OpenSessionGroup(i),
                                DashNavItem::Host(i) => Message::ConnectSsh(i),
                            };
                            return Ok(self.update(msg));
                        }
                        return Ok(Task::none());
                    }
                    if matches!(named, Named::Escape) {
                        if self.selected_nav.is_some() {
                            self.selected_nav = None;
                            return Ok(Task::none());
                        }
                        // No selection: let Esc fall through (close modals etc).
                        if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                            && let Some(task) = self.handle_hotkey_keypress(key, modifiers)
                        {
                            return Ok(task);
                        }
                        return Ok(Task::none());
                    }

                    if flat.is_empty() {
                        // Nothing to navigate; ignore the movement keys.
                        return Ok(Task::none());
                    }

                    // Movement is cyclic (wraps last↔first). ←/→ (and Tab)
                    // move linearly; ↓/↑ move by a grid row (or linearly in
                    // single-column list mode).
                    let n = flat.len();
                    // Linear forward / backward with wrap-around.
                    let fwd = flat[flat_pos.map_or(0, |p| (p + 1) % n)];
                    let back = flat[flat_pos.map_or(n - 1, |p| (p + n - 1) % n)];
                    let nrows = rows.len();
                    let new_sel: Option<DashNavItem> = match named {
                        Named::Tab if modifiers.shift() => Some(back),
                        Named::Tab => Some(fwd),
                        Named::ArrowRight => Some(fwd),
                        Named::ArrowLeft => Some(back),
                        Named::ArrowDown if list_mode => Some(fwd),
                        Named::ArrowUp if list_mode => Some(back),
                        Named::ArrowDown => Some(match cur {
                            Some((r, c)) => {
                                let nr = (r + 1) % nrows;
                                rows[nr][c.min(rows[nr].len() - 1)]
                            }
                            None => flat[0],
                        }),
                        Named::ArrowUp => Some(match cur {
                            Some((r, c)) => {
                                let nr = (r + nrows - 1) % nrows;
                                rows[nr][c.min(rows[nr].len() - 1)]
                            }
                            None => *flat.last().unwrap(),
                        }),
                        _ => None,
                    };

                    if let Some(sel) = new_sel {
                        let entering = self.selected_nav.is_none();
                        self.selected_nav = Some(sel);
                        // Scroll only enough to keep the selected row in
                        // view: rows that already fit on the first screen
                        // don't scroll; later rows scroll so the selected
                        // one sits at the bottom edge. Row/viewport heights
                        // are estimates (iced doesn't expose item bounds
                        // here), tuned to the card metrics.
                        let sel_row = rows
                            .iter()
                            .position(|row| row.contains(&sel))
                            .unwrap_or(0) as f32;
                        let row_h = if self.setting_host_list_view { 56.0 } else { 60.0 };
                        let viewport = (self.window_size.height - 115.0).max(row_h);
                        let visible_rows = (viewport / row_h).floor().max(1.0);
                        let max_scroll_rows = (rows.len() as f32 - visible_rows).max(1.0);
                        let offset_rows = (sel_row - visible_rows + 1.0).max(0.0);
                        let y = (offset_rows / max_scroll_rows).clamp(0.0, 1.0);
                        let scroll = iced::widget::operation::snap_to(
                            iced::widget::Id::new("dashboard-grid-scroll"),
                            iced::widget::operation::RelativeOffset { x: None, y: Some(y) },
                        );
                        // First move drops focus from the search input so
                        // we're clearly in "card selection" mode. Focusing a
                        // non-existent id unfocuses every focusable (the
                        // `focus` operation blurs all non-matching widgets).
                        if entering {
                            return Ok(Task::batch([
                                iced::widget::operation::focus(iced::widget::Id::new(
                                    "__dashboard_nav_blur__",
                                )),
                                scroll,
                            ]));
                        }
                        return Ok(scroll);
                    }
                }
                // Hotkey dispatch + capture mode live in `shortcuts.rs`
                // (`handle_hotkey_keypress`). Returns a Task when the
                // event was consumed by a binding (or by the Settings
                // editor's capture mode), `None` to fall through to
                // the legacy PTY-routing block below.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && let Some(task) = self.handle_hotkey_keypress(key, modifiers)
                {
                    return Ok(task);
                }
                // When the AI chat sidebar is open and the cursor is over
                // it, the user is interacting with the textarea, drop the
                // event so it doesn't double-dispatch into the terminal
                // session running underneath.
                let cursor_in_chat_sidebar = self
                    .active_tab
                    .and_then(|i| self.tabs.get(i))
                    .map(|t| t.chat_visible)
                    .unwrap_or(false)
                    && self.mouse_position.x
                        > (self.window_size.width - self.chat_sidebar_width);
                if cursor_in_chat_sidebar {
                    return Ok(Task::none());
                }
                // A global picker / modal (new-tab picker, tab jump,
                // icon / theme / jump-host pickers, folder rename or
                // delete) owns the keyboard while open. Its own search
                // field consumes the keystroke via iced focus; without
                // this gate the same press also falls through to the
                // PTY below, so typing in the picker echoes into the
                // terminal. Esc still closes the modal because that's
                // handled earlier in `handle_hotkey_keypress`.
                if self.any_modal_blocks_input() {
                    return Ok(Task::none());
                }

                if let Some(tab_idx) = self.active_tab
                    && self.connecting.is_none()
                    && let keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        text: text_opt,
                        location,
                        ..
                    } = event
                    {
                        // Application-cursor-keys mode (DECCKM) of the active
                        // pane decides whether arrows / Home / End go out in
                        // SS3 (`ESC O A`) or CSI (`ESC [ A`) form. Read once
                        // here; the same flag is tracked for local PTY and SSH
                        // panes alike.
                        let app_cursor = self
                            .tabs
                            .get(tab_idx)
                            .and_then(|t| {
                                t.active().terminal.lock().ok().map(|s| s.application_cursor_keys())
                            })
                            .unwrap_or(false);
                        // macOS: Command (Cmd) is the clipboard modifier, not
                        // Ctrl. Cmd+V pastes; Cmd+C / Cmd+A are copy /
                        // select-all owned by the terminal widget (it holds the
                        // selection state). The Canvas widget and this global
                        // key subscription are independent paths, so the widget
                        // copying does NOT stop the bare character from echoing
                        // here. Swallow every unregistered Cmd combo so it never
                        // leaks into the PTY as text. Registered Cmd shortcuts
                        // (Cmd+K, Cmd+T, ...) were already consumed upstream by
                        // `handle_hotkey_keypress`. Ctrl keeps its Unix meaning
                        // (Ctrl+C = SIGINT) on every platform, including macOS.
                        if cfg!(target_os = "macos")
                            && modifiers.logo()
                            && !modifiers.control()
                            && !modifiers.alt()
                        {
                            if let keyboard::Key::Character(ref c) = key
                                && c.as_str().eq_ignore_ascii_case("v")
                            {
                                self.paste_clipboard_into_active();
                            }
                            // Cmd+C / Cmd+A and any other Cmd combo fall out
                            // here without writing, so nothing echoes.
                        }
                        // Ctrl+V → paste from clipboard (not raw Ctrl+V byte)
                        else if modifiers.control() && !modifiers.shift() {
                            if let keyboard::Key::Character(ref c) = key {
                                if c.as_str().eq_ignore_ascii_case("v") {
                                    self.paste_clipboard_into_active();
                                    // Don't fall through to the normal key handler
                                } else if c.as_str().eq_ignore_ascii_case("c") {
                                    // Ctrl+C → send interrupt (byte 3)
                                    if let Some(tab) = self.tabs.get(tab_idx) {
                                        if let Some(ref ssh) = tab.active().ssh_session {
                                            let _ = ssh.write(&[3]);
                                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                                            state.write(&[3]);
                                        }
                                    }
                                } else if c.as_str().eq_ignore_ascii_case("d")
                                    && self
                                        .tabs
                                        .get(tab_idx)
                                        .is_some_and(|t| t.label.ends_with(" (disconnected)"))
                                {
                                    // A logged-out session (SSH / exec tab relabelled
                                    // "(disconnected)") has no shell left to receive EOF,
                                    // so Ctrl+D would be swallowed. Treat it as "close this
                                    // dead tab" instead, matching the muscle memory of
                                    // dismissing an exited shell. Only single-pane tabs
                                    // carry the suffix, so siblings are never nuked.
                                    return Ok(self.update(Message::CloseTab(tab_idx)));
                                } else if let Some(bytes) = ctrl_key_bytes(&key) {
                                    // Other Ctrl+key combinations
                                    if let Some(tab) = self.tabs.get(tab_idx) {
                                        if let Some(ref ssh) = tab.active().ssh_session {
                                            let _ = ssh.write(&bytes);
                                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                                            state.write(&bytes);
                                        }
                                    }
                                }
                            } else if let Some(bytes) = key_to_named_bytes(&key, &modifiers, app_cursor) {
                                // Ctrl + named key (e.g. Ctrl+Home)
                                if let Some(tab) = self.tabs.get(tab_idx) {
                                    if let Some(ref ssh) = tab.active().ssh_session {
                                        let _ = ssh.write(&bytes);
                                    } else if let Ok(mut state) = tab.active().terminal.lock() {
                                        state.write(&bytes);
                                    }
                                }
                            }
                        } else if modifiers.shift() && modifiers.control() {
                            // Ctrl+Shift+V → paste from clipboard into SSH or local PTY.
                            // Copy (Ctrl+Shift+C) stays in the terminal widget since it
                            // owns the selection state.
                            if let keyboard::Key::Character(ref c) = key
                                && c.as_str().eq_ignore_ascii_case("v")
                            {
                                self.paste_clipboard_into_active();
                            }
                        } else {
                            // Normal keys (no Ctrl).
                            // Iced's `key` is the key WITHOUT modifiers, so a numpad
                            // keypress with NumLock on still shows up as Named::Home /
                            // ArrowUp / etc. while the OS-produced `text` is "7" / "8".
                            // Prefer the text on numpad so NumLock-on sends digits.
                            let numpad_text = if location == keyboard::Location::Numpad {
                                text_opt.as_ref().filter(|t| !t.is_empty())
                                    .map(|t| t.as_bytes().to_vec())
                            } else {
                                None
                            };
                            let mut bytes = numpad_text
                                .or_else(|| key_to_named_bytes(&key, &modifiers, app_cursor))
                                .or_else(|| text_opt.as_ref().map(|t| t.as_bytes().to_vec()));

                            // Meta-sends-escape: Alt+<char> (without Ctrl, so
                            // AltGr, reported as Ctrl+Alt on Windows and not as
                            // Alt on Linux, still composes text) is the
                            // ESC-prefixed character, the form readline / bash /
                            // zsh / vim / emacs / tmux bind their Meta keymaps to
                            // (Alt+b = back one word, Alt+f = forward, Alt+. =
                            // last arg, …). Named keys already fold their
                            // modifier in via key_to_named_bytes, so this only
                            // touches literal characters.
                            if modifiers.alt()
                                && !modifiers.control()
                                && let keyboard::Key::Character(c) = &key
                            {
                                // Some platforms drop `text` while Alt is held;
                                // fall back to the key's base character so
                                // Alt+b still emits `ESC b`.
                                let ch = bytes
                                    .filter(|b| !b.is_empty())
                                    .unwrap_or_else(|| c.as_str().as_bytes().to_vec());
                                let mut esc = Vec::with_capacity(ch.len() + 1);
                                esc.push(0x1b);
                                esc.extend_from_slice(&ch);
                                bytes = Some(esc);
                            }

                            if let Some(bytes) = bytes
                                && !bytes.is_empty()
                                && let Some(tab) = self.tabs.get(tab_idx)
                            {
                                if let Some(ref ssh) = tab.active().ssh_session {
                                    let _ = ssh.write(&bytes);
                                } else if let Ok(mut state) = tab.active().terminal.lock() {
                                    state.write(&bytes);
                                }
                            }
                        }
                    }
            }
            // Text committed by the OS IME (composed CJK characters, etc.),
            // delivered by the global subscription separately from
            // KeyboardEvent. Forward to the PTY under the same conditions as
            // a keystroke: no host editor panel or modal stealing focus, and
            // the cursor not over the chat sidebar. Deliberately does NOT
            // gate on active_view: in workspace mode a focused terminal runs
            // under the Dashboard view, not a dedicated Terminal view, so the
            // KeyboardEvent path doesn't check it either. When a text_input is
            // focused it handles its own Commit and inserts the text itself;
            // the host-panel / modal guards keep that from also hitting the
            // session.
            Message::TerminalImeCommit(text) => {
                if text.is_empty() || self.show_host_panel || self.any_modal_blocks_input() {
                    return Ok(Task::none());
                }
                let cursor_in_chat_sidebar = self
                    .active_tab
                    .and_then(|i| self.tabs.get(i))
                    .map(|t| t.chat_visible)
                    .unwrap_or(false)
                    && self.mouse_position.x
                        > (self.window_size.width - self.chat_sidebar_width);
                if cursor_in_chat_sidebar {
                    return Ok(Task::none());
                }
                if let Some(tab_idx) = self.active_tab
                    && self.connecting.is_none()
                    && let Some(tab) = self.tabs.get(tab_idx)
                {
                    let bytes = text.into_bytes();
                    if let Some(ref ssh) = tab.active().ssh_session {
                        let _ = ssh.write(&bytes);
                    } else if let Ok(mut state) = tab.active().terminal.lock() {
                        state.write(&bytes);
                    }
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }

    /// Index of the tab whose grid contains the pane with `pane_id`.
    /// Used to route per-pane session events (connect / disconnect).
    pub(crate) fn pane_tab_index(&self, pane_id: uuid::Uuid) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.pane_grid.panes.values().any(|p| p.id == pane_id))
    }
}
