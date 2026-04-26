//! `Oryxis::handle_terminal` — match arms for terminal I/O: PTY bytes
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
            // -- Terminal I/O --
            Message::PtyOutput(tab_idx, bytes) => {
                if let Some(tab) = self.tabs.get(tab_idx) {
                    if let Ok(mut state) = tab.active().terminal.lock() {
                        state.process(&bytes);
                    }
                    // Append to session log for terminal recording
                    if let Some(log_id) = tab.active().session_log_id
                        && let Some(vault) = &self.vault {
                            let _ = vault.append_session_data(&log_id, &bytes);
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
                // Global shortcut: Ctrl+K opens the new-tab picker regardless
                // of which screen or tab is active. Handled before the
                // tab-specific routing so it works on Dashboard / Settings /
                // inside a terminal alike.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("k")
                {
                    self.show_new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    return Ok(Task::none());
                }
                // Ctrl+J — jump to a tab via the Termius-style modal
                // listing all open tabs + Quick connect entries.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("j")
                {
                    self.show_tab_jump = true;
                    self.tab_jump_search.clear();
                    return Ok(Task::none());
                }
                // When the AI chat sidebar is open and the cursor is over
                // it, the user is interacting with the textarea — drop the
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
                                        if let Some(ref ssh) = tab.active().ssh_session {
                                            let _ = ssh.write(text.as_bytes());
                                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                                            state.write(text.as_bytes());
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
                                if let Some(ref ssh) = tab.active().ssh_session {
                                    let _ = ssh.write(text.as_bytes());
                                } else if let Ok(mut state) = tab.active().terminal.lock() {
                                    state.write(text.as_bytes());
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
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
