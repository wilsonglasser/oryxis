//! `Oryxis::handle_terminal`, match arms for terminal I/O: PTY bytes
//! coming back, keyboard events routed to the active tab, mouse moves
//! tracked for hit-testing, window resize/drag/min/max/close.

#![allow(clippy::result_large_err)]

use iced::keyboard;
use iced::Task;

use crate::app::{Message, Oryxis};
use crate::util::{ctrl_key_bytes, key_to_named_bytes};

impl Oryxis {
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
                if let Some(pane) = self
                    .tabs
                    .iter()
                    .flat_map(|t| t.pane_grid.panes.values())
                    .find(|p| p.id == pane_id)
                {
                    if let Ok(mut state) = pane.terminal.lock() {
                        state.process(&bytes);
                    }
                    if let Some(log_id) = pane.session_log_id
                        && let Some(vault) = &self.vault
                    {
                        let _ = vault.append_session_data(&log_id, &bytes);
                    }
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
            // Right-click paste from the terminal widget. Mirrors the
            // Ctrl+Shift+V path below: SSH session if active, local PTY
            // otherwise. Without this, the widget's fallback write only
            // reached the local PTY and right-click looked broken on
            // every SSH tab.
            Message::TerminalPasteFromClipboard => {
                if let Some(tab_idx) = self.active_tab
                    && let Some(tab) = self.tabs.get(tab_idx)
                    && let Ok(mut clip) = arboard::Clipboard::new()
                    && let Ok(text) = clip.get_text()
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
                        crate::util::ime_debug(&format!("wrote {} bytes to SSH", bytes.len()));
                    } else if let Ok(mut state) = tab.active().terminal.lock() {
                        state.write(&bytes);
                        crate::util::ime_debug(&format!("wrote {} bytes to local PTY", bytes.len()));
                    } else {
                        crate::util::ime_debug("guards passed but no ssh and PTY lock failed");
                    }
                } else {
                    crate::util::ime_debug("dropped after sidebar guard: no active_tab or connecting");
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
