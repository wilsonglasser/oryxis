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
                let _ = vault.append_session_data(&log_id, &scrubbed);
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
                if let Some(pane) = self
                    .tabs
                    .iter_mut()
                    .flat_map(|t| t.pane_grid.panes.values_mut())
                    .find(|p| p.id == pane_id)
                {
                    if let Ok(mut state) = pane.terminal.lock() {
                        state.process(&bytes);
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
                        // Ctrl+V → paste from clipboard (not raw Ctrl+V byte)
                        if modifiers.control() && !modifiers.shift() {
                            if let keyboard::Key::Character(ref c) = key {
                                if c.as_str().eq_ignore_ascii_case("v") {
                                    if let Ok(mut clip) = arboard::Clipboard::new()
                                        && let Ok(text) = clip.get_text()
                                        && let Some(tab) = self.tabs.get(tab_idx)
                                    {
                                        let bracketed = tab
                                            .active()
                                            .terminal
                                            .lock()
                                            .map(|s| s.bracketed_paste_enabled())
                                            .unwrap_or(false);
                                        let payload = oryxis_terminal::wrap_paste(&text, bracketed);
                                        if let Some(ref ssh) = tab.active().ssh_session {
                                            let _ = ssh.write(&payload);
                                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                                            state.write(&payload);
                                        }
                                    }
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
                            } else if let Some(bytes) = key_to_named_bytes(&key, &modifiers) {
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
                                && let Ok(mut clip) = arboard::Clipboard::new()
                                && let Ok(text) = clip.get_text()
                                && let Some(tab) = self.tabs.get(tab_idx)
                            {
                                let bracketed = tab
                                    .active()
                                    .terminal
                                    .lock()
                                    .map(|s| s.bracketed_paste_enabled())
                                    .unwrap_or(false);
                                let payload = oryxis_terminal::wrap_paste(&text, bracketed);
                                if let Some(ref ssh) = tab.active().ssh_session {
                                    let _ = ssh.write(&payload);
                                } else if let Ok(mut state) = tab.active().terminal.lock() {
                                    state.write(&payload);
                                }
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
                            let bytes = numpad_text
                                .or_else(|| key_to_named_bytes(&key, &modifiers))
                                .or_else(|| text_opt.map(|t| t.as_bytes().to_vec()));

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
