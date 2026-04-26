//! `Oryxis::handle_settings` — match arms for the Settings panel:
//! terminal/SFTP/SSH knobs, app theme, language, auto-reconnect tick,
//! OS-detection toggles, vault lock, font size adjustments.

#![allow(clippy::result_large_err)]

use iced::Task;

use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;

use oryxis_terminal::widget::TerminalState;

use crate::app::{Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
use crate::state::{TerminalTab, VaultState, View};
use crate::util::sanitize_uint;

impl Oryxis {
    pub(crate) fn handle_settings(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Settings --
            Message::TerminalThemeChanged(name) => {
                if let Some(theme) = oryxis_terminal::TerminalTheme::ALL.iter().find(|t| t.name() == name) {
                    self.terminal_theme = *theme;
                    // Apply to all open terminals
                    for tab in &self.tabs {
                        if let Ok(mut state) = tab.active().terminal.lock() {
                            state.palette = theme.palette();
                        }
                    }
                }
            }
            Message::LanguageChanged(name) => {
                use crate::i18n::Language;
                if let Some(lang) = Language::ALL.iter().find(|l| l.name() == name) {
                    Language::set_active(*lang);
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("language", lang.code());
                    }
                }
            }
            Message::AppThemeChanged(name) => {
                use crate::theme::AppTheme;
                if let Some(theme) = AppTheme::ALL.iter().find(|t| t.name() == name) {
                    AppTheme::set_active(*theme);
                    // Persist so the choice survives the next boot —
                    // previously the theme reverted on every restart.
                    self.persist_setting("app_theme", theme.name());
                    // Map app theme to terminal palette
                    let term_theme = match theme {
                        AppTheme::OryxisDark => oryxis_terminal::TerminalTheme::OryxisDark,
                        AppTheme::OryxisLight => oryxis_terminal::TerminalTheme::OryxisDark,
                        AppTheme::Termius => oryxis_terminal::TerminalTheme::OryxisDark,
                        AppTheme::Darcula => oryxis_terminal::TerminalTheme::Dracula,
                        AppTheme::IslandsDark => oryxis_terminal::TerminalTheme::Dracula,
                        AppTheme::Dracula => oryxis_terminal::TerminalTheme::Dracula,
                        AppTheme::Monokai => oryxis_terminal::TerminalTheme::Monokai,
                        AppTheme::HackerGreen => oryxis_terminal::TerminalTheme::HackerGreen,
                        AppTheme::Nord => oryxis_terminal::TerminalTheme::Nord,
                        AppTheme::NordLight => oryxis_terminal::TerminalTheme::Nord,
                        AppTheme::SolarizedLight => oryxis_terminal::TerminalTheme::SolarizedDark,
                        AppTheme::PaperLight => oryxis_terminal::TerminalTheme::OryxisDark,
                    };
                    self.terminal_theme = term_theme;
                    for tab in &self.tabs {
                        if let Ok(mut state) = tab.active().terminal.lock() {
                            state.palette = term_theme.palette();
                        }
                    }
                }
            }
            Message::TerminalFontSizeIncrease => {
                self.terminal_font_size = (self.terminal_font_size + 1.0).min(24.0);
            }
            Message::TerminalFontSizeDecrease => {
                self.terminal_font_size = (self.terminal_font_size - 1.0).max(10.0);
            }
            Message::TerminalFontChanged(name) => {
                self.terminal_font_name = name;
            }
            Message::ChangeSettingsSection(section) => {
                self.settings_section = section;
            }
            Message::ToggleCopyOnSelect => {
                self.setting_copy_on_select = !self.setting_copy_on_select;
                self.persist_setting(
                    "copy_on_select",
                    if self.setting_copy_on_select { "true" } else { "false" },
                );
            }
            Message::ToggleBoldIsBright => {
                self.setting_bold_is_bright = !self.setting_bold_is_bright;
                self.persist_setting(
                    "bold_is_bright",
                    if self.setting_bold_is_bright { "true" } else { "false" },
                );
            }
            Message::ToggleKeywordHighlight => {
                self.setting_keyword_highlight = !self.setting_keyword_highlight;
                self.persist_setting(
                    "keyword_highlight",
                    if self.setting_keyword_highlight { "true" } else { "false" },
                );
            }
            Message::SettingKeepaliveChanged(val) => {
                // Accept only digits; cap at 86_400 (1 day) so users can't
                // accidentally type a runaway value.
                self.setting_keepalive_interval = sanitize_uint(&val, 86_400);
                self.persist_setting("keepalive_interval", &self.setting_keepalive_interval);
            }
            Message::SettingScrollbackChanged(val) => {
                // Cap at 1M rows — alacritty allocates lazily but >1M is
                // both unreasonable and a foot-gun for memory pressure.
                self.setting_scrollback_rows = sanitize_uint(&val, 1_000_000);
                self.persist_setting("scrollback_rows", &self.setting_scrollback_rows);
            }
            Message::SettingSftpConcurrencyChanged(val) => {
                // Cap at 8 — beyond that the SSH channel multiplexer
                // overhead outweighs the throughput gain on most links.
                self.setting_sftp_concurrency = sanitize_uint(&val, 8);
                if self.setting_sftp_concurrency == "0" {
                    self.setting_sftp_concurrency = "1".into();
                }
                self.persist_setting("sftp_concurrency", &self.setting_sftp_concurrency);
            }
            Message::SettingSftpConnectTimeoutChanged(val) => {
                self.setting_sftp_connect_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_connect_timeout == "0" {
                    self.setting_sftp_connect_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_connect_timeout",
                    &self.setting_sftp_connect_timeout,
                );
            }
            Message::SettingSftpAuthTimeoutChanged(val) => {
                self.setting_sftp_auth_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_auth_timeout == "0" {
                    self.setting_sftp_auth_timeout = "1".into();
                }
                self.persist_setting("sftp_auth_timeout", &self.setting_sftp_auth_timeout);
            }
            Message::SettingSftpSessionTimeoutChanged(val) => {
                self.setting_sftp_session_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_session_timeout == "0" {
                    self.setting_sftp_session_timeout = "1".into();
                }
                self.persist_setting(
                    "sftp_session_timeout",
                    &self.setting_sftp_session_timeout,
                );
            }
            Message::SettingSftpOpTimeoutChanged(val) => {
                self.setting_sftp_op_timeout = sanitize_uint(&val, 600);
                if self.setting_sftp_op_timeout == "0" {
                    self.setting_sftp_op_timeout = "1".into();
                }
                // Apply live to the active SFTP client so the user
                // doesn't have to reconnect to feel the change.
                if let Some(client) = &self.sftp.client {
                    client.set_op_timeout(self.sftp_op_timeout());
                }
                self.persist_setting("sftp_op_timeout", &self.setting_sftp_op_timeout);
            }
            Message::SettingToggleAutoReconnect => {
                self.setting_auto_reconnect = !self.setting_auto_reconnect;
                self.persist_setting(
                    "auto_reconnect",
                    if self.setting_auto_reconnect { "true" } else { "false" },
                );
            }
            Message::SettingMaxReconnectChanged(val) => {
                self.setting_max_reconnect_attempts = sanitize_uint(&val, 100);
                self.persist_setting(
                    "max_reconnect_attempts",
                    &self.setting_max_reconnect_attempts,
                );
            }
            Message::ConnectAnimTick => {
                self.connect_anim_tick = self.connect_anim_tick.wrapping_add(1);
            }
            Message::AutoReconnectTick => {
                if !self.setting_auto_reconnect {
                    // fall through, nothing to do
                } else {
                    let max_attempts: u32 =
                        self.setting_max_reconnect_attempts.parse().unwrap_or(5);
                    // Find the first disconnected SSH tab whose counter is under the limit.
                    // Only reconnect one per tick to avoid thrashing; next tick picks up
                    // the next candidate.
                    let candidate: Option<usize> = (0..self.tabs.len()).find(|&i| {
                        let tab = &self.tabs[i];
                        if !tab.label.ends_with(" (disconnected)") {
                            return false;
                        }
                        let base = tab.label.trim_end_matches(" (disconnected)");
                        let Some(conn) = self.connections.iter().find(|c| c.label == base) else {
                            return false;
                        };
                        let attempts = self.reconnect_counters.get(&conn.id).copied().unwrap_or(0);
                        attempts < max_attempts
                    });
                    if let Some(tab_idx) = candidate {
                        let base = self.tabs[tab_idx]
                            .label
                            .trim_end_matches(" (disconnected)")
                            .to_string();
                        if let Some(conn) = self.connections.iter().find(|c| c.label == base) {
                            let entry = self.reconnect_counters.entry(conn.id).or_insert(0);
                            *entry += 1;
                        }
                        return Ok(Task::done(Message::ReconnectTab(tab_idx)));
                    }
                }
            }
            Message::LockVault => {
                if let Some(vault) = &mut self.vault {
                    vault.lock();
                    if self.vault_has_user_password {
                        self.vault_state = VaultState::Locked;
                        self.connections.clear();
                        self.keys.clear();
                        self.snippets.clear();
                        self.groups.clear();
                        self.tabs.clear();
                        self.active_tab = None;
                        self.active_view = View::Dashboard;
                    } else {
                        // No user password: re-open immediately
                        let _ = vault.open_without_password();
                    }
                }
            }

            Message::OpenLocalShell => {
                self.connecting = None; // Clear any pending SSH connection progress
                match TerminalState::new(DEFAULT_TERM_COLS as u16, DEFAULT_TERM_ROWS as u16) {
                    Ok((mut state, rx)) => {
                        state.palette = self.terminal_theme.palette();
                        let tab_idx = self.tabs.len();
                        self.tabs.push(TerminalTab::new_single(
                            "Local Shell".into(),
                            Arc::new(Mutex::new(state)),
                        ));
                        self.active_tab = Some(tab_idx);
                        self.active_view = View::Terminal;

                        let stream = UnboundedReceiverStream::new(rx);
                        return Ok(Task::batch(vec![
                            self.tab_scroll_to_active(),
                            Task::stream(stream)
                                .map(move |bytes| Message::PtyOutput(tab_idx, bytes)),
                        ]));
                    }
                    Err(e) => {
                        tracing::error!("Failed to spawn local shell: {}", e);
                    }
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
