//! `Oryxis::handle_snippets`: settings-panel-independent dispatch arms for the
//! snippets area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{Message, Oryxis};

impl Oryxis {
    pub(crate) fn handle_snippets(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Local shell --
            // -- Snippets --
            Message::ShowSnippetPanel => {
                self.overlay = None;
                self.show_snippet_panel = true;
                self.snippet_label.clear();
                self.snippet_command = iced::widget::text_editor::Content::new();
                self.snippet_editing_id = None;
                self.snippet_error = None;
            }
            Message::HideSnippetPanel => {
                self.show_snippet_panel = false;
            }
            Message::SnippetLabelChanged(v) => self.snippet_label = v,
            Message::SnippetCommandAction(action) => self.snippet_command.perform(action),
            Message::ShowSnippetMenu(idx) => {
                use crate::state::{OverlayContent, OverlayState};
                // Toggle: clicking the kebab again (or on the same card)
                // dismisses the popup, mirroring the host-card menu.
                if self.snippet_context_menu == Some(idx) {
                    self.snippet_context_menu = None;
                    self.overlay = None;
                } else {
                    self.snippet_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SnippetActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::EditSnippet(idx) => {
                // Reached from the card kebab menu, close the popup.
                self.snippet_context_menu = None;
                self.overlay = None;
                if let Some(snip) = self.snippets.get(idx) {
                    self.show_snippet_panel = true;
                    self.snippet_label = snip.label.clone();
                    self.snippet_command =
                        iced::widget::text_editor::Content::with_text(&snip.command);
                    self.snippet_editing_id = Some(snip.id);
                    self.snippet_error = None;
                }
            }
            Message::SaveSnippet => {
                if self.snippet_label.is_empty() || self.snippet_command.text().trim().is_empty() {
                    self.snippet_error = Some("Label and command are required".into());
                    return Ok(Task::none());
                }
                let mut snip = if let Some(id) = self.snippet_editing_id {
                    self.snippets.iter().find(|s| s.id == id).cloned()
                        .unwrap_or_else(|| oryxis_core::models::snippet::Snippet::new("", ""))
                } else {
                    oryxis_core::models::snippet::Snippet::new("", "")
                };
                snip.label = self.snippet_label.clone();
                snip.command = self.snippet_command.text().trim_end().to_string();
                if let Some(vault) = &self.vault {
                    match vault.save_snippet(&snip) {
                        Ok(()) => {
                            self.show_snippet_panel = false;
                            self.snippet_error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => self.snippet_error = Some(e.to_string()),
                    }
                }
            }
            Message::RequestDeleteSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let name = snip.label.clone();
                    self.confirm_remove(name, Message::DeleteSnippet(idx));
                }
            }
            Message::DeleteSnippet(idx) => {
                // Reached from the card kebab menu or the edit panel,
                // close the popup either way.
                self.snippet_context_menu = None;
                self.overlay = None;
                if let Some(snip) = self.snippets.get(idx) {
                    let id = snip.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_snippet(&id);
                        self.show_snippet_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }
            Message::RunSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let cmd = snip.command.clone();
                    if let Some(tab_idx) = self.snippet_injection_tab()
                        && let Some(tab) = self.tabs.get(tab_idx)
                    {
                        // Bracket the body (so a multi-line snippet inserts as
                        // one block under bracketed paste), then append the
                        // submit newline OUTSIDE the bracket so it runs once.
                        // With the mode off this collapses to the old
                        // `command\n` raw write.
                        let bracketed = tab
                            .active()
                            .terminal
                            .lock()
                            .map(|s| s.bracketed_paste_enabled())
                            .unwrap_or(false);
                        let mut payload = oryxis_terminal::wrap_paste(&cmd, bracketed);
                        payload.push(b'\n');
                        if let Some(ref ssh) = tab.active().ssh_session {
                            let _ = ssh.write(&payload);
                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                            state.write(&payload);
                        }
                    }
                }
            }
            Message::ApplySudoPassword => {
                // Resolve the active terminal's connection by label, decrypt
                // its stored password, and type it + Enter. The password is
                // never logged (only PTY output is recorded, and sudo turns
                // echo off) nor shown in the toast.
                let toast_key = (|| {
                    let tab_idx = self.snippet_injection_tab()?;
                    let label = self.tabs.get(tab_idx)?.label.clone();
                    let conn_id = self
                        .connections
                        .iter()
                        .find(|c| c.label == label)
                        .map(|c| c.id)?;
                    let pw = self
                        .vault
                        .as_ref()
                        .and_then(|v| v.get_connection_password(&conn_id).ok().flatten())
                        .filter(|p| !p.is_empty())?;
                    let data = format!("{pw}\n");
                    if let Some(tab) = self.tabs.get(tab_idx) {
                        if let Some(ref ssh) = tab.active().ssh_session {
                            let _ = ssh.write(data.as_bytes());
                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                            state.write(data.as_bytes());
                        }
                    }
                    Some("sudo_password_sent")
                })()
                .unwrap_or("no_stored_password");
                self.toast = Some(crate::i18n::t(toast_key).to_string());
                return Ok(Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                    },
                    |_| Message::ToastClear,
                ));
            }
            Message::PasteSnippet(idx) => {
                // Same injection path as RunSnippet, but without the trailing
                // newline so the user reviews and presses Enter themselves.
                if let Some(snip) = self.snippets.get(idx) {
                    let cmd = snip.command.clone();
                    if let Some(tab_idx) = self.snippet_injection_tab()
                        && let Some(tab) = self.tabs.get(tab_idx)
                    {
                        // Wrap in bracketed paste when the focused app asked
                        // for it, so a multi-line snippet inserts as one block
                        // instead of auto-submitting on every embedded newline.
                        let bracketed = tab
                            .active()
                            .terminal
                            .lock()
                            .map(|s| s.bracketed_paste_enabled())
                            .unwrap_or(false);
                        let payload = oryxis_terminal::wrap_paste(&cmd, bracketed);
                        if let Some(ref ssh) = tab.active().ssh_session {
                            let _ = ssh.write(&payload);
                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                            state.write(&payload);
                        }
                    }
                }
            }


            m => return Err(m),
        }
        Ok(Task::none())
    }
}
