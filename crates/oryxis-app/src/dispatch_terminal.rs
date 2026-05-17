//! `Oryxis::handle_terminal`, match arms for terminal I/O: PTY bytes
//! coming back, keyboard events routed to the active tab, mouse moves
//! tracked for hit-testing, window resize/drag/min/max/close.

#![allow(clippy::result_large_err)]

use iced::keyboard;
use iced::Task;

use crate::app::{Message, Oryxis};
use crate::state::View;
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
                    if let Some(ref ssh) = tab.active().ssh_session {
                        let _ = ssh.write(text.as_bytes());
                    } else if let Ok(mut state) = tab.active().terminal.lock() {
                        state.write(text.as_bytes());
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
                // Ctrl+F1 is an alternate binding for the new-tab
                // picker, surfaced for users who think of it as
                // "global host search" rather than "open a new tab".
                // Same underlying overlay; the trigger glyph on the
                // tab bar (lucide::search) also dispatches it.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::F1))
                {
                    self.show_new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    return Ok(Task::none());
                }
                // Ctrl+J, jump to a tab via the Termius-style modal
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
                // Ctrl+T opens the new-tab picker. Synonym for Ctrl+K,
                // added for Termius-muscle-memory users. Skipped on
                // the Terminal view so bash's readline transpose-chars
                // (Ctrl+T) still works inside the shell; user can still
                // reach the picker via Ctrl+K (which is intentionally
                // global) or by switching out of the terminal first.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && !modifiers.shift()
                    && self.active_view != View::Terminal
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("t")
                {
                    self.show_new_tab_picker = true;
                    self.new_tab_picker_search.clear();
                    return Ok(Task::none());
                }
                // Ctrl+L opens a fresh local-shell tab. Skipped on the
                // Terminal view so the shell's clear-screen still works.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && !modifiers.shift()
                    && self.active_view != View::Terminal
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("l")
                {
                    return Ok(Task::done(Message::OpenLocalShell));
                }
                // Ctrl+P opens the host editor for the currently active
                // tab's saved connection. Lands on the editor's Port
                // Forwards section once the v0.8 panel ships; for now
                // the editor opens at its default scroll so the user
                // can scroll to Port Forwards manually. Skipped on the
                // Terminal view so bash's readline previous-history
                // (Ctrl+P) still works inside the shell.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && !modifiers.shift()
                    && self.active_view != View::Terminal
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("p")
                    && let Some(idx) = self.active_tab_connection_idx()
                {
                    return Ok(Task::done(Message::EditConnection(idx)));
                }
                // Ctrl+Shift+W closes the active tab. No-op when no tab
                // is focused; the CloseTab handler is itself safe against
                // a stale index but skipping the dispatch keeps the
                // tab-bar's scroll position from twitching.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && modifiers.shift()
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("w")
                    && let Some(idx) = self.active_tab
                {
                    return Ok(Task::done(Message::CloseTab(idx)));
                }
                // Alt+Left / Alt+Right cycle through terminal tabs.
                // Wraps at the ends so heavy users don't have to think
                // about boundary positions.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.alt()
                    && !modifiers.control()
                    && !self.tabs.is_empty()
                {
                    let n = self.tabs.len();
                    let current = self.active_tab.unwrap_or(0);
                    let next = match key {
                        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some((current + 1) % n),
                        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some((current + n - 1) % n),
                        _ => None,
                    };
                    if let Some(idx) = next {
                        return Ok(Task::done(Message::SelectTab(idx)));
                    }
                }
                // Ctrl+1..9, activate the Nth slot of the visual tab
                // strip. Slot order matches `tab_bar.rs`: Workspace mode
                // puts Hosts at slot 0 and SFTP at slot 1 (when enabled)
                // before terminal tabs; Classic mode skips straight to
                // terminal tabs since the strip doesn't carry nav areas.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && !modifiers.shift()
                    && !modifiers.alt()
                    && let keyboard::Key::Character(c) = key
                    && let Some(d) = c.as_str().chars().next().and_then(|ch| ch.to_digit(10))
                    && (1..=9).contains(&d)
                {
                    return Ok(Task::done(Message::ActivateStripSlot(d as usize - 1)));
                }
                // Ctrl+, opens Settings. Common cross-app convention
                // (VS Code, Slack, browsers). Captured before the PTY
                // routing so the byte doesn't leak into the shell.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && !modifiers.shift()
                    && !modifiers.alt()
                    && let keyboard::Key::Character(c) = key
                    && c.as_str() == ","
                {
                    return Ok(Task::done(Message::ChangeView(View::Settings)));
                }
                // Ctrl+Shift+N spawns a fresh top-level Oryxis window.
                // Same machinery as the "Duplicate in New Window" menu
                // item, minus the source-tab argument.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && modifiers.shift()
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("n")
                {
                    return Ok(Task::done(Message::SpawnNewWindow));
                }
                // F11 toggles native fullscreen on the current window.
                if let keyboard::Event::KeyPressed { key, .. } = &event
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::F11))
                {
                    return Ok(Task::done(Message::WindowFullscreenToggle));
                }
                // Ctrl+F focuses the current view's primary search input.
                // Skipped on the Terminal view so apps that bind Ctrl+F
                // (less, vim, fzf) still receive it.
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && !modifiers.shift()
                    && self.active_view != View::Terminal
                    && let keyboard::Key::Character(c) = key
                    && c.as_str().eq_ignore_ascii_case("f")
                {
                    return Ok(Task::done(Message::FocusViewSearch));
                }
                // Esc closes the topmost open modal. Only fires when at
                // least one modal flag is set, so terminal apps that
                // rely on raw Esc (vim, less) keep getting the byte.
                if let keyboard::Event::KeyPressed { key, .. } = &event
                    && matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
                    && self.close_topmost_modal()
                {
                    return Ok(Task::none());
                }
                // Ctrl + (= | + | - | 0), terminal font zoom. Matches
                // alacritty / kitty / gnome-terminal convention. Captured
                // before the PTY routing so the bytes don't leak into
                // the shell. `+` covers Ctrl+Shift+= on US layouts.
                // `_` is intentionally NOT bound because Ctrl+_ already
                // produces a meaningful control byte (0x1f).
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = &event
                    && modifiers.control()
                    && let keyboard::Key::Character(c) = key
                {
                    let new_size = match c.as_str() {
                        "=" | "+" => Some((self.terminal_font_size + 1.0).min(24.0)),
                        "-" => Some((self.terminal_font_size - 1.0).max(10.0)),
                        "0" => Some(14.0),
                        _ => None,
                    };
                    if let Some(size) = new_size {
                        self.terminal_font_size = size;
                        self.persist_setting(
                            "terminal_font_size",
                            &format!("{}", size),
                        );
                        return Ok(Task::none());
                    }
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
