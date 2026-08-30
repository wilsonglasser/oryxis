//! `Oryxis::handle_history`: settings-panel-independent dispatch arms for the
//! history area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;

use crate::app::{HistoryMessage, CommandHistoryMessage, Message, Oryxis};

impl Oryxis {
    pub(crate) fn handle_history(
        &mut self,
        message: HistoryMessage,
    ) -> Task<Message> {
        match message {
            // -- History --
            // Clear now wipes both feeds the unified History timeline
            // mixes (failed-connect log rows + recorded session rows)
            // so the user gets a true "empty list" instead of seeing
            // every previously recorded session reappear after the
            // wipe finishes.
            HistoryMessage::RequestClearHistory => {
                // Close the `…` overflow menu before the confirm dialog
                // rises (no-op when triggered from the inline button).
                self.overlay = None;
                self.clear_history_confirm = true;
            }
            HistoryMessage::CancelClearHistory => {
                self.clear_history_confirm = false;
            }
            HistoryMessage::ClearLogs => {
                self.clear_history_confirm = false;
                if let Some(vault) = &self.vault {
                    let _ = vault.clear_logs();
                    let _ = vault.clear_session_logs();
                    // "Clear all" clears the whole timeline, and saved
                    // conversations are rows in it.
                    let _ = vault.clear_chat_conversations();
                    self.logs_page = 0;
                    self.session_logs_page = 0;
                    self.load_data_from_vault();
                }
                // The wipe pulled the recording out from under any open
                // viewer / player; drop them with it, and retire any
                // content-search results that pointed into the wiped rows.
                self.viewing_session_log = None;
                self.session_player = None;
                self.chat_ui.viewer = None;
                // A tab still appending to a wiped conversation must start a
                // fresh row instead of resurrecting a deleted one.
                for tab in &mut self.tabs {
                    tab.chat_saved_id = None;
                    tab.chat_persisted = 0;
                }
                self.history_content_reset();
            }
            HistoryMessage::LogsPageNext => {
                let max_page = (self.logs_total.saturating_sub(1)) / 50;
                if self.logs_page < max_page {
                    self.logs_page += 1;
                    if let Some(vault) = &self.vault {
                        self.logs = vault
                            .list_logs_page(self.logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            HistoryMessage::LogsPagePrev => {
                if self.logs_page > 0 {
                    self.logs_page -= 1;
                    if let Some(vault) = &self.vault {
                        self.logs = vault
                            .list_logs_page(self.logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            HistoryMessage::ViewSessionLog(log_id) => {
                return self.open_session_log_viewer(log_id, None);
            }
            HistoryMessage::ToggleSessionViewerMode => {
                let Some(viewer) = &self.viewing_session_log else {
                    return Task::none();
                };
                let (log_id, mode) = (viewer.log_id, viewer.mode.toggled());
                // Rebuilt from the vault rather than kept in two emulators:
                // a recording is fed once and never mutated, so holding a
                // second full copy for a switch the user makes rarely is
                // memory for nothing.
                return self.open_session_log_viewer(log_id, Some(mode));
            }
            HistoryMessage::CloseSessionLogView => {
                self.viewing_session_log = None;
            }
            HistoryMessage::ShowSessionViewerContextMenu(x, y, selection) => {
                // Right-click scheme = Menu over the transcript body:
                // anchor the read-only copy menu at the click point
                // (window-absolute, same space as every menu).
                self.overlay = Some(crate::state::OverlayState {
                    content: crate::state::OverlayContent::SessionLogViewerContext(selection),
                    x,
                    y,
                });
            }
            HistoryMessage::SessionViewerCopyAll => {
                self.overlay = None;
                if let Some(viewer) = &self.viewing_session_log
                    && let Ok(state) = viewer.terminal.lock()
                {
                    let text = state.all_text();
                    drop(state);
                    if !text.is_empty() {
                        return crate::dispatch_global::write_clipboard_text(text);
                    }
                }
            }
            HistoryMessage::ShowSessionLogViewerMenu(idx) => {
                use crate::state::{OverlayContent, OverlayState};
                // Toggle, mirroring the row kebab below.
                let already = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::SessionLogViewerActions(i)) if *i == idx
                );
                if already {
                    self.overlay = None;
                } else {
                    let anchor = self.keynav_take_menu_anchor();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SessionLogViewerActions(idx),
                        x: anchor.0,
                        y: anchor.1,
                    });
                }
            }
            HistoryMessage::ShowSessionLogMenu(idx) => {
                use crate::state::{OverlayContent, OverlayState};
                // Toggle, mirroring the other card kebabs.
                let already = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::SessionLogActions(i)) if *i == idx
                );
                if already {
                    self.overlay = None;
                } else {
                    let anchor = self.keynav_take_menu_anchor();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SessionLogActions(idx),
                        x: anchor.0,
                        y: anchor.1,
                    });
                }
            }
            HistoryMessage::ExportSessionCast(log_id) => {
                self.overlay = None;
                // Flush first so an in-progress session exports complete.
                self.flush_session_logs_final();
                let Some(entry) = self.session_logs.iter().find(|e| e.id == log_id) else {
                    return Task::none();
                };
                let Some(vault) = &self.vault else {
                    return Task::none();
                };
                let events = match vault.get_session_events(&log_id) {
                    Ok(ev) => ev,
                    Err(e) => {
                        return self.show_toast(
                            crate::i18n::t("history_export_failed")
                                .replace("{error}", &e.to_string()),
                        );
                    }
                };
                // Header term.type mirrors what the PTY actually requested:
                // the connection's terminal_type, or the engine's default.
                // A deleted / quick-connect host falls back the same way.
                // The embedded theme resolves like the live pane did:
                // per-host override first, then the global theme.
                let conn = self
                    .connections
                    .iter()
                    .find(|c| c.id == entry.connection_id);
                let term = conn
                    .and_then(|c| c.terminal_type.as_deref())
                    .unwrap_or("xterm-256color");
                let palette = conn
                    .map(|c| self.resolve_terminal_palette_for_connection(c))
                    .unwrap_or_else(|| self.resolve_global_terminal_palette());
                let body =
                    build_asciicast(&entry.label, entry.started_at, term, &palette, &events);
                let default_name = format!(
                    "oryxis-{}-{}.cast",
                    crate::util::sanitize_file_stem(&entry.label),
                    entry.started_at.format("%Y%m%d-%H%M%S"),
                );
                return save_text_file_task(body, default_name, "cast");
            }
            HistoryMessage::ExportSessionTranscript(log_id) => {
                self.overlay = None;
                self.flush_session_logs_final();
                let Some(entry) = self.session_logs.iter().find(|e| e.id == log_id) else {
                    return Task::none();
                };
                let Some(vault) = &self.vault else {
                    return Task::none();
                };
                let data = match vault.get_session_data(&log_id) {
                    Ok(Some(d)) => d,
                    Ok(None) => return Task::none(),
                    Err(e) => {
                        return self.show_toast(
                            crate::i18n::t("history_export_failed")
                                .replace("{error}", &e.to_string()),
                        );
                    }
                };
                // Same pipeline as the in-app viewer: CR overwrites and
                // erase-line resolved, OSC/SGR stripped; what remains is
                // the text a human saw.
                let palette = self.resolve_global_terminal_palette();
                let body: String = crate::ansi_render::render(&data, &palette)
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect();
                let default_name = format!(
                    "oryxis-{}-{}.txt",
                    crate::util::sanitize_file_stem(&entry.label),
                    entry.started_at.format("%Y%m%d-%H%M%S"),
                );
                return save_text_file_task(body, default_name, "txt");
            }
            HistoryMessage::ExportSessionCommands(log_id) => {
                self.overlay = None;
                // 'c' rows are written at capture time (never buffered),
                // so no flush is needed here.
                let Some(entry) = self.session_logs.iter().find(|e| e.id == log_id) else {
                    return Task::none();
                };
                let Some(vault) = &self.vault else {
                    return Task::none();
                };
                let events = match vault.get_session_commands(&log_id) {
                    Ok(ev) => ev,
                    Err(e) => {
                        return self.show_toast(
                            crate::i18n::t("history_export_failed")
                                .replace("{error}", &e.to_string()),
                        );
                    }
                };
                // Pre-feature recordings (and sessions where nothing was
                // typed at a prompt) have no command rows; say so instead
                // of silently writing an empty file.
                if events.is_empty() {
                    return self.show_toast(
                        crate::i18n::t("session_export_commands_empty").to_string(),
                    );
                }
                let body = build_command_export(&entry.label, entry.started_at, &events);
                let default_name = format!(
                    "oryxis-{}-{}-input.txt",
                    crate::util::sanitize_file_stem(&entry.label),
                    entry.started_at.format("%Y%m%d-%H%M%S"),
                );
                return save_text_file_task(body, default_name, "txt");
            }
            HistoryMessage::ExportSessionGif(log_id) => {
                self.overlay = None;
                if self.gif_export.running {
                    return self
                        .show_toast(crate::i18n::t("gif_export_started").to_string());
                }
                // Plugin not installed: there is no download path
                // anymore, so tell the user the binary is missing and
                // keep the export parked nowhere.
                let Some(binary) = crate::gif_export::resolve_binary() else {
                    return self.show_toast(
                        crate::i18n::t("plugin_err_binary_not_found").to_string(),
                    );
                };
                // Same source as the .cast export: flush first, resolve
                // the terminal type + theme like the live pane did (the
                // embedded theme is what colors the GIF; agg reads it
                // from the header, no plumbing across the process).
                self.flush_session_logs_final();
                let Some(entry) = self.session_logs.iter().find(|e| e.id == log_id) else {
                    return Task::none();
                };
                let Some(vault) = &self.vault else {
                    return Task::none();
                };
                let events = match vault.get_session_events(&log_id) {
                    Ok(ev) => ev,
                    Err(e) => {
                        return self.show_toast(
                            crate::i18n::t("history_export_failed")
                                .replace("{error}", &e.to_string()),
                        );
                    }
                };
                let conn = self
                    .connections
                    .iter()
                    .find(|c| c.id == entry.connection_id);
                let term = conn
                    .and_then(|c| c.terminal_type.as_deref())
                    .unwrap_or("xterm-256color");
                let palette = conn
                    .map(|c| self.resolve_terminal_palette_for_connection(c))
                    .unwrap_or_else(|| self.resolve_global_terminal_palette());
                let body =
                    build_asciicast(&entry.label, entry.started_at, term, &palette, &events);
                let default_name = format!(
                    "oryxis-{}-{}.gif",
                    crate::util::sanitize_file_stem(&entry.label),
                    entry.started_at.format("%Y%m%d-%H%M%S"),
                );
                self.gif_export.running = true;
                let start_toast = self
                    .show_toast_secs(crate::i18n::t("gif_export_started").to_string(), 4);
                let render = Task::perform(
                    async move {
                        // Save dialog off the UI thread; a dismissed
                        // dialog reports nothing (None).
                        let picked = tokio::task::spawn_blocking(move || {
                            rfd::FileDialog::new()
                                .set_file_name(&default_name)
                                .add_filter("gif", &["gif"])
                                .save_file()
                        })
                        .await
                        .ok()
                        .flatten();
                        match picked {
                            None => None,
                            Some(path) => Some(
                                crate::gif_export::render(binary, body, path).await,
                            ),
                        }
                    },
                    |v| Message::History(HistoryMessage::GifExportFinished(v)),
                );
                return Task::batch([start_toast, render]);
            }
            HistoryMessage::GifExportFinished(outcome) => {
                self.gif_export.running = false;
                match outcome {
                    None => {}
                    Some(Ok(path)) => {
                        return self.show_toast(
                            crate::i18n::t("history_export_done")
                                .replace("{path}", &path),
                        );
                    }
                    Some(Err(cause)) => {
                        return self.show_toast(
                            crate::i18n::t("history_export_failed")
                                .replace("{error}", &cause),
                        );
                    }
                }
            }
            HistoryMessage::RequestDeleteSessionLog(idx) => {
                // Reached from the row kebab; drop it before the dialog.
                self.overlay = None;
                let label = self
                    .session_logs
                    .get(idx)
                    .map(|e| e.label.clone())
                    .unwrap_or_default();
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("log_delete_confirm_title").to_string(),
                    body: format!(
                        "{label}: {}",
                        crate::i18n::t("log_delete_confirm_body")
                    ),
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("delete").to_string(),
                        message: Box::new(Message::History(HistoryMessage::DeleteSessionLog(idx))),
                        danger: true,
                    }),
                });
            }

            HistoryMessage::ShowChatConversationMenu(idx) => {
                use crate::state::{OverlayContent, OverlayState};
                // Toggle, mirroring the session-log kebab beside it.
                let already = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::ChatConversationActions(i)) if *i == idx
                );
                if already {
                    self.overlay = None;
                } else {
                    let anchor = self.keynav_take_menu_anchor();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::ChatConversationActions(idx),
                        x: anchor.0,
                        y: anchor.1,
                    });
                }
            }
            HistoryMessage::OpenChatConversation(id) => {
                self.overlay = None;
                // One reader at a time in the History slot, the same rule
                // the recording viewer and the player already follow.
                self.viewing_session_log = None;
                self.session_player = None;
                // Flush the live conversation first, so opening the one you
                // are still having shows its latest turns rather than what
                // happened to be saved at the last settle point.
                if let Some(idx) = self.active_tab {
                    self.flush_chat_history(idx);
                }
                let Some(vault) = &self.vault else {
                    return Task::none();
                };
                let messages = match vault.chat_messages(&id) {
                    Ok(m) => m,
                    Err(e) => {
                        return self
                            .show_toast(format!("{}: {e}", crate::i18n::t("chat_open_failed")));
                    }
                };
                let label = self
                    .chat_ui.conversations
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.label.clone())
                    .unwrap_or_default();
                self.chat_ui.viewer = Some(crate::state::ChatViewer {
                    conversation_id: id,
                    label,
                    messages,
                });
            }
            HistoryMessage::CloseChatConversation => {
                self.chat_ui.viewer = None;
            }
            HistoryMessage::RequestDeleteChatConversation(idx) => {
                // Reached from the row kebab; drop it before the dialog.
                self.overlay = None;
                let label = self
                    .chat_ui.conversations
                    .get(idx)
                    .map(|e| e.label.clone())
                    .unwrap_or_default();
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("chat_delete_confirm_title").to_string(),
                    body: format!(
                        "{label}: {}",
                        crate::i18n::t("chat_delete_confirm_body")
                    ),
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("delete").to_string(),
                        message: Box::new(Message::History(
                            HistoryMessage::DeleteChatConversation(idx),
                        )),
                        danger: true,
                    }),
                });
            }
            HistoryMessage::DeleteChatConversation(idx) => {
                let Some(id) = self.chat_ui.conversations.get(idx).map(|e| e.id) else {
                    return Task::none();
                };
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_chat_conversation(&id);
                    self.chat_ui.conversations =
                        vault.list_chat_conversations().unwrap_or_default();
                }
                // The reader cannot outlive what it is reading.
                if self.chat_ui.viewer.as_ref().is_some_and(|v| v.conversation_id == id) {
                    self.chat_ui.viewer = None;
                }
                // A tab still appending to this conversation must not
                // resurrect it: detach so its next turn starts a new row.
                for tab in &mut self.tabs {
                    if tab.chat_saved_id == Some(id) {
                        tab.chat_saved_id = None;
                        tab.chat_persisted = 0;
                    }
                }
            }

            HistoryMessage::LogRowHovered(id) => {
                self.hover.log_row = Some(id);
            }
            HistoryMessage::LogRowUnhovered(id) => {
                self.hover.leave_log_row(id);
            }
            HistoryMessage::DeleteSessionLog(idx) => {
                if let Some(entry) = self.session_logs.get(idx) {
                    let id = entry.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_session_log(&id);
                        self.session_logs_total =
                            vault.count_session_logs().unwrap_or(0);
                        // Stepping a page back when the current one is now
                        // empty keeps the prev/next pair from leaving the
                        // user staring at "0 of N" with rows further back.
                        let max_page = self
                            .session_logs_total
                            .saturating_sub(1)
                            / 50;
                        if self.session_logs_page > max_page {
                            self.session_logs_page = max_page;
                        }
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                    // The window reload above dropped any rows the
                    // content search had pulled in from beyond it;
                    // re-attach the survivors (the deleted one falls
                    // out of the results here too).
                    self.history_reattach_extra_logs();
                    // Its decrypted excerpt must not outlive the
                    // recording in app state; a queued scan for it is
                    // retired as done (the fetch would only error).
                    self.history_content.log_matches.remove(&id);
                    let hc = &mut self.history_content;
                    let queued = hc.queue.len();
                    hc.queue.retain(|q| *q != id);
                    hc.scan_done += queued - hc.queue.len();
                }
                // Close viewer / player if we deleted the one being shown
                if let Some(viewer) = &self.viewing_session_log
                    && self.session_logs.iter().all(|s| s.id != viewer.log_id) {
                        self.viewing_session_log = None;
                }
                if let Some(p) = &self.session_player
                    && self.session_logs.iter().all(|s| s.id != p.log_id) {
                        self.session_player = None;
                }
            }
            HistoryMessage::ClearSessionLogs => {
                if let Some(vault) = &self.vault {
                    let _ = vault.clear_session_logs();
                    self.session_logs_page = 0;
                    self.load_data_from_vault();
                }
                self.viewing_session_log = None;
                self.session_player = None;
                // Same rationale as ClearLogs: the wipe pulled the
                // recordings out from under any content-search
                // results still pointing at them.
                self.history_content_reset();
            }
            HistoryMessage::SessionLogsPageNext => {
                let max_page = self.session_logs_total.saturating_sub(1) / 50;
                if self.session_logs_page < max_page {
                    self.session_logs_page += 1;
                    if let Some(vault) = &self.vault {
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }
            HistoryMessage::SessionLogsPagePrev => {
                if self.session_logs_page > 0 {
                    self.session_logs_page -= 1;
                    if let Some(vault) = &self.vault {
                        self.session_logs = vault
                            .list_session_logs_page(self.session_logs_page * 50, 50)
                            .unwrap_or_default();
                    }
                }
            }

            HistoryMessage::CopyHostSshUrl(idx) => {
                // Card action: canonical ssh:// URL for the host. Closes
                // the context menu itself (CopyToClipboard stays free of
                // menu state; it is also dispatched from inside overlays).
                self.card_context_menu = None;
                self.overlay = None;
                let Some(conn) = self.connections.get(idx) else {
                    return Task::none();
                };
                let url = self.host_ssh_url(conn);
                return self.update(Message::CopyToClipboard(url));
            }

            HistoryMessage::WakeOnLan(idx) => {
                // Card action: broadcast the magic packet. The menu only
                // offers this when a MAC is stored, and saves normalize
                // it, so a parse failure here means a hand-edited vault
                // row; surface it rather than silently doing nothing.
                self.card_context_menu = None;
                self.overlay = None;
                let Some(conn) = self.connections.get(idx) else {
                    return Task::none();
                };
                let Some(mac) = conn.mac_address.as_deref().and_then(crate::wol::parse_mac)
                else {
                    return self.show_toast(crate::i18n::t("host_mac_invalid").to_string());
                };
                return match crate::wol::send(mac) {
                    Ok(()) => self.show_toast(
                        crate::i18n::t("wol_sent")
                            .replace("{mac}", &crate::wol::format_mac(mac)),
                    ),
                    Err(e) => self.show_toast(
                        crate::i18n::t("wol_failed").replace("{error}", &e.to_string()),
                    ),
                };
            }

            // -- Content search (commands + recorded output) --
            HistoryMessage::SearchContentToggled => {
                self.history_search_content = !self.history_search_content;
                if self.history_search_content {
                    return self.history_content_debounce();
                }
                // Toggled off: the maps hold decrypted session excerpts,
                // drop them and let the plain filter take over.
                self.history_content_reset();
            }
            HistoryMessage::SearchContentDebounce(generation) => {
                // A newer keystroke re-armed the timer; this tick is stale.
                if generation != self.history_content.generation
                    || !self.history_search_content
                {
                    return Task::none();
                }
                return self.history_content_search_start();
            }
            HistoryMessage::SearchContentScanned { generation, log_id, snippet } => {
                if generation != self.history_content.generation {
                    // A newer search owns the pump now; drop result AND pump.
                    return Task::none();
                }
                self.history_content.scan_done += 1;
                self.history_content.scanning = false;
                if let Some(snippet) = snippet {
                    self.history_content.log_matches.insert(log_id, snippet);
                    // The scan covers the whole table; a hit beyond
                    // the loaded page window needs its row pulled in
                    // for the timeline to render it.
                    self.history_ensure_log_loaded(log_id);
                }
                return self.history_scan_step();
            }

            // -- Host-tag filter over the timeline --
            HistoryMessage::ShowHistoryTagFilterMenu => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::HistoryTagFilter)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor under the tag-filter button (bounds reported
                    // every draw), mirroring the dashboard's dropdown;
                    // cursor fallback before the first draw populates it.
                    let b = self.history_tag_filter_btn_bounds.get();
                    let (x, y) = if b.width > 0.0 {
                        let lead = if crate::i18n::is_rtl_layout() {
                            b.x + b.width
                        } else {
                            b.x
                        };
                        (lead, b.y + b.height + 6.0)
                    } else {
                        (self.mouse_position.x, self.mouse_position.y + 26.0)
                    };
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::HistoryTagFilter,
                        x,
                        y,
                    });
                }
            }
            HistoryMessage::ToggleHistoryTagFilterTag(tag) => {
                // Multi-select: the dropdown stays open so several tags
                // can be picked in one visit; the backdrop closes it.
                match self
                    .history_filter_tags
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case(&tag))
                {
                    Some(i) => {
                        self.history_filter_tags.remove(i);
                    }
                    None => self.history_filter_tags.push(tag),
                }
                // Filter changed the visible set; drop the keyboard
                // selection so Enter can't open a now-hidden row.
                self.keynav.focus = None;
            }
            HistoryMessage::ClearHistoryTagFilter => {
                self.history_filter_tags.clear();
                self.overlay = None;
                self.keynav.focus = None;
            }
        }
        Task::none()
    }

    /// Whether the History tag-filter affordance should render: at
    /// least one host is tagged, or a (possibly dangling) filter is
    /// active and needs a way to be cleared. Mirrors
    /// `host_tag_filter_available`.
    pub(crate) fn history_tag_filter_available(&self) -> bool {
        !self.history_filter_tags.is_empty()
            || self.connections.iter().any(|c| !c.tags.is_empty())
    }

    /// Drop every content-search result (they hold decrypted session
    /// excerpts) while advancing the generation, so in-flight debounce
    /// ticks and scan steps retire instead of resurrecting stale data.
    /// Called on toggle-off, on a query too short to scan, on vault
    /// lock and on history wipes.
    pub(crate) fn history_content_reset(&mut self) {
        // Timeline rows loaded from beyond the page window ride out
        // with the results that justified them.
        self.history_drop_extra_logs();
        let generation = self.history_content.generation + 1;
        self.history_content = Default::default();
        self.history_content.generation = generation;
    }

    /// Remove the timeline rows the content search pulled into
    /// `session_logs` from beyond the loaded page window. Called
    /// before a new search recomputes its matches (the new needle
    /// doesn't necessarily match them) and from
    /// `history_content_reset`.
    fn history_drop_extra_logs(&mut self) {
        if self.history_content.extra_logs.is_empty() {
            return;
        }
        let extra = std::mem::take(&mut self.history_content.extra_logs);
        self.session_logs.retain(|e| !extra.contains(&e.id));
    }

    /// Make sure a matched session has a row in `session_logs` for
    /// the timeline to render: the content search covers the whole
    /// table, so a hit can live beyond the loaded page window. Such
    /// rows are fetched on demand and remembered in `extra_logs` so
    /// `history_content_reset` drops them with the results. A row
    /// deleted since the queue was built simply stays absent.
    fn history_ensure_log_loaded(&mut self, log_id: uuid::Uuid) {
        if self.session_logs.iter().any(|e| e.id == log_id) {
            return;
        }
        let Some(vault) = &self.vault else {
            return;
        };
        let Ok(Some(entry)) = vault.get_session_log(&log_id) else {
            return;
        };
        self.session_logs.push(entry);
        self.history_content.extra_logs.push(log_id);
    }

    /// Re-attach the content-search extra rows after `session_logs`
    /// was reloaded from the page window (a delete steps the window
    /// under them). Rows gone from the vault drop out of the results
    /// here; rows the reload happened to pull inside the window stop
    /// being tracked as extras.
    fn history_reattach_extra_logs(&mut self) {
        let extra = std::mem::take(&mut self.history_content.extra_logs);
        for id in extra {
            self.history_ensure_log_loaded(id);
        }
    }

    /// Arm the content-search debounce for the current query: bump the
    /// generation (retiring any previous timer or in-flight scan) and
    /// schedule the actual search shortly, so scanning never runs per
    /// keystroke. Called on every History search edit while the
    /// content toggle is on, and on toggle-on itself.
    pub(crate) fn history_content_debounce(&mut self) -> Task<Message> {
        let needle = self.history_search.trim().to_lowercase();
        if needle.chars().count() < 2 {
            // Too short to scan usefully; fall back to the plain
            // label/hostname filter.
            self.history_content_reset();
            return Task::none();
        }
        self.history_content.generation += 1;
        let generation = self.history_content.generation;
        Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            },
            move |()| Message::History(HistoryMessage::SearchContentDebounce(generation)),
        )
    }

    /// Run the cheap search tiers synchronously (typed-command records
    /// per session + per-host command history, tiny data), then queue
    /// the recorded-output scan for the sessions those tiers didn't
    /// already answer and kick its first step. Every tier covers the
    /// WHOLE session-log table, not just the page of rows the UI has
    /// loaded (`session_logs` holds a 50-row window): matched sessions
    /// beyond the window are pulled in via `history_ensure_log_loaded`
    /// so the timeline has a row to light up, and the scan queue /
    /// "Scanning recordings X/Y" counter account for every recording.
    fn history_content_search_start(&mut self) -> Task<Message> {
        let needle = self.history_search.trim().to_lowercase();
        // Rows a previous query pulled in don't necessarily match this
        // one; drop them before recomputing.
        self.history_drop_extra_logs();
        let Some(vault) = &self.vault else {
            return Task::none();
        };
        let cmd_hits = vault.search_session_commands(&needle).unwrap_or_default();
        let host_hits = vault.search_command_history(&needle).unwrap_or_default();
        let log_matches: std::collections::HashMap<uuid::Uuid, String> =
            cmd_hits.into_iter().collect();
        // Walk every recording (same coverage as the command tier
        // above). Sessions the command tier already matched and
        // sessions the plain filter answers anyway (label/hostname
        // carries the match) skip the output scan, scanning them would
        // only spend decrypt time to restate the same hit, but both
        // kinds still need their row loaded when they live beyond the
        // page window; everything else queues for the output scan.
        let hostname_by_id: std::collections::HashMap<uuid::Uuid, &str> = self
            .connections
            .iter()
            .map(|c| (c.id, c.hostname.as_str()))
            .collect();
        let refs = vault.list_session_log_scan_meta().unwrap_or_default();
        let mut queue: Vec<uuid::Uuid> = Vec::new();
        let mut surfaced: Vec<uuid::Uuid> = Vec::new();
        for (id, connection_id, label) in &refs {
            let plain_hit = label.to_lowercase().contains(&needle)
                || hostname_by_id
                    .get(connection_id)
                    .is_some_and(|h| h.to_lowercase().contains(&needle));
            if log_matches.contains_key(id) || plain_hit {
                surfaced.push(*id);
            } else {
                queue.push(*id);
            }
        }
        let hc = &mut self.history_content;
        hc.needle = needle;
        hc.log_matches = log_matches;
        hc.conn_matches = host_hits;
        hc.scan_total = queue.len();
        hc.scan_done = 0;
        hc.queue = queue;
        hc.scanning = false;
        for id in surfaced {
            self.history_ensure_log_loaded(id);
        }
        self.history_scan_step()
    }

    /// Launch the next output-scan step: pop one session, read its
    /// output still sealed (SQL only, cheap on the UI thread) and hand
    /// the decrypt + render + match work to a blocking worker; the
    /// result comes back through `SearchContentScanned`, which pumps
    /// the next step. One step in flight at a time keeps the UI free
    /// and lands results incrementally.
    fn history_scan_step(&mut self) -> Task<Message> {
        loop {
            if self.history_content.queue.is_empty() {
                self.history_content.scanning = false;
                return Task::none();
            }
            let log_id = self.history_content.queue.remove(0);
            let Some(vault) = &self.vault else {
                // Vault gone mid-scan (lock): stop; the lock path also
                // resets the results.
                self.history_content.scanning = false;
                self.history_content.queue.clear();
                return Task::none();
            };
            // Scan-bounded reader: caps how much of one recording is
            // copied and decrypted, so a runaway session can't balloon
            // a search step (exports and the viewer keep the unbounded
            // reader).
            let sealed = match vault.sealed_session_output_scan(&log_id) {
                Ok(sealed) => sealed,
                Err(_) => {
                    // Row deleted mid-scan: count it done and move on.
                    self.history_content.scan_done += 1;
                    continue;
                }
            };
            self.history_content.scanning = true;
            let generation = self.history_content.generation;
            let needle = self.history_content.needle.clone();
            let palette = self.resolve_global_terminal_palette();
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let data = sealed.open();
                        // Same pipeline as the transcript export: CR
                        // overwrites and erase-line resolved, so the
                        // search sees the text a human saw.
                        let text: String = crate::ansi_render::render(&data, &palette)
                            .iter()
                            .map(|s| s.text.as_str())
                            .collect();
                        content_match_snippet(&text, &needle)
                    })
                    .await
                    .ok()
                    .flatten()
                },
                move |snippet| {
                    Message::History(HistoryMessage::SearchContentScanned {
                        generation,
                        log_id,
                        snippet,
                    })
                },
            );
        }
    }
}

impl Oryxis {
    /// Open (or re-open) the transcript viewer over a recording.
    ///
    /// `mode` is `None` on the first open, where it is decided by how
    /// much of the recording ran on the alternate screen: a session
    /// spent inside tmux (or a pager) replays faithfully into a single
    /// final frame with nothing to scroll, which is the shape a reporter
    /// read as "the session was not recorded" (#92). Those open in
    /// [`TranscriptMode::Linear`]; the header switches either way.
    fn open_session_log_viewer(
        &mut self,
        log_id: uuid::Uuid,
        mode: Option<crate::state::TranscriptMode>,
    ) -> Task<Message> {
        // Flush buffered output first so viewing a still-active session
        // shows everything recorded up to this moment, not just what was
        // last persisted.
        self.flush_session_logs_final();
        // The History slot holds one reader at a time.
        self.chat_ui.viewer = None;
        let Some(vault) = &self.vault else {
            return Task::none();
        };
        let events = match vault.get_session_events(&log_id) {
            Ok(ev) => ev,
            Err(e) => {
                return self.show_toast(
                    crate::i18n::t("history_export_failed")
                        .replace("{error}", &e.to_string()),
                );
            }
        };
        // The transcript wears the same colors the live pane wore:
        // per-host terminal theme override first, then the global theme
        // (mirrors the player and the `.cast` export).
        let palette = self
            .session_logs
            .iter()
            .find(|e| e.id == log_id)
            .and_then(|e| self.connections.iter().find(|c| c.id == e.connection_id))
            .map(|c| self.resolve_terminal_palette_for_connection(c))
            .unwrap_or_else(|| self.resolve_global_terminal_palette());
        let mode = mode.unwrap_or_else(|| {
            if crate::state::alt_screen_share(&events) >= crate::state::LINEAR_ALT_SHARE {
                crate::state::TranscriptMode::Linear
            } else {
                crate::state::TranscriptMode::Rendered
            }
        });
        match crate::state::SessionLogViewer::build(log_id, &events, palette, mode) {
            Ok(viewer) => {
                self.viewing_session_log = Some(viewer);
                // Mutually exclusive with the player surface.
                self.session_player = None;
            }
            Err(e) => {
                return self.show_toast(
                    crate::i18n::t("history_export_failed")
                        .replace("{error}", &e.to_string()),
                );
            }
        }
        Task::none()
    }
}

/// Case-insensitive search of `needle` (already lowercased) inside a
/// rendered transcript, returning a display excerpt around the first
/// match: the matching line, windowed with ellipses when it is long.
/// Case folding is per-char (1:1) so excerpt offsets stay aligned with
/// the original text; multi-char foldings (ß → ss) would shift them.
/// The vault command tiers fold with full Unicode `to_lowercase()`
/// instead, so such needles can match there but not here; intentional
/// divergence, documented at both sites (see
/// `search_session_commands` / `search_command_history`).
pub(crate) fn content_match_snippet(text: &str, needle: &str) -> Option<String> {
    fn lower1(c: char) -> char {
        c.to_lowercase().next().unwrap_or(c)
    }
    // Per-line work bound: rendered grid lines are terminal-width
    // short, but the raw fallback for lines the renderer passed
    // through can be arbitrarily long and unbroken, and each line
    // costs two Vec<char> allocations here. A match past the cap is
    // missed, same best-effort contract as the vault-side scan cap
    // (CONTENT_SEARCH_MAX_SCAN_BYTES).
    const MAX_LINE_SCAN_CHARS: usize = 4096;
    let needle_chars: Vec<char> = needle.chars().map(lower1).collect();
    if needle_chars.is_empty() {
        return None;
    }
    for line in text.lines() {
        let chars: Vec<char> = line.chars().take(MAX_LINE_SCAN_CHARS).collect();
        let lowered: Vec<char> = chars.iter().copied().map(lower1).collect();
        let Some(pos) = lowered
            .windows(needle_chars.len())
            .position(|w| w == needle_chars)
        else {
            continue;
        };
        // The match plus ~40 chars of context each side, ellipsized
        // where the line continues.
        const CTX: usize = 40;
        let start = pos.saturating_sub(CTX);
        let end = (pos + needle_chars.len() + CTX).min(chars.len());
        let mut out = String::new();
        if start > 0 {
            out.push('\u{2026}');
        }
        out.extend(chars[start..end].iter());
        if end < chars.len() {
            out.push('\u{2026}');
        }
        return Some(out.trim().to_string());
    }
    None
}

/// Serialize a recorded session as an asciicast v3 document: a JSON
/// header line (`version: 3`, a `term` object carrying geometry,
/// terminal type and the effective color theme, start timestamp,
/// title), then one `[interval_sec, "o"|"r", data]` line per event,
/// timed as the interval since the PREVIOUS event (v3 semantics; the
/// stored offsets are integer milliseconds, so emitted intervals sum
/// exactly and need no rounding-drift correction). The embedded
/// `term.theme` is what lets players and agg reproduce the terminal
/// colors without any side-channel. Output-only by design: input
/// events are never recorded, so the keystroke-leak class doesn't
/// exist here. Chunks recorded before the timing migration
/// (`offset_ms = None`) replay with a small fixed delta so old logs
/// still play, just without real pacing. No `idle_time_limit` on
/// purpose: capping pauses in the file would bake a pacing opinion
/// into a 1:1 recording; players take it as a playback option instead.
fn build_asciicast(
    label: &str,
    started_at: chrono::DateTime<chrono::Utc>,
    term: &str,
    palette: &oryxis_terminal::TerminalPalette,
    events: &[oryxis_vault::SessionLogEvent],
) -> String {
    // Geometry: the first recorded resize (the initial size lands on
    // the first flush); a legacy log without one replays at 80x24.
    let (width, height) = events
        .iter()
        .find(|e| e.kind == 'r')
        .and_then(|e| {
            let s = String::from_utf8_lossy(&e.data);
            let (c, r) = s.split_once('x')?;
            Some((c.parse::<u16>().ok()?, r.parse::<u16>().ok()?))
        })
        .unwrap_or((80, 24));
    let hex = crate::theme::color_to_hex;
    let theme_palette: String = palette
        .ansi
        .iter()
        .map(|c| hex(*c))
        .collect::<Vec<_>>()
        .join(":");
    let mut out = String::new();
    out.push_str(&format!(
        "{}\n",
        serde_json::json!({
            "version": 3,
            "term": {
                "cols": width,
                "rows": height,
                "type": term,
                "theme": {
                    "fg": hex(palette.foreground),
                    "bg": hex(palette.background),
                    "palette": theme_palette,
                },
            },
            "timestamp": started_at.timestamp(),
            "title": label,
        })
    ));
    /// Untimed-chunk replay step (legacy rows), in milliseconds.
    const LEGACY_DELTA_MS: i64 = 50;
    let mut last_ms: i64 = 0;
    for event in events {
        // Typed-command rows feed the input-only .txt export; the cast
        // replay stays output-only (they are not asciicast "i" events:
        // resolved command lines, not keystrokes, and their echo is
        // already in the output stream).
        if event.kind == 'c' {
            continue;
        }
        let ms = match event.offset_ms {
            // Intervals must be >= 0; clamp against interleavings (a
            // resize stamped at flush time can sit a hair before the
            // chunk rows written in the same batch).
            Some(ms) => ms.max(last_ms),
            None => last_ms + LEGACY_DELTA_MS,
        };
        let interval_ms = ms - last_ms;
        last_ms = ms;
        let kind = if event.kind == 'r' { "r" } else { "o" };
        let data = String::from_utf8_lossy(&event.data);
        out.push_str(&format!(
            "{}\n",
            serde_json::json!([interval_ms as f64 / 1000.0, kind, data])
        ));
    }
    out
}

/// Input-only export body: a small header, then one line per typed
/// command in capture order. Timed rows (full-detail recording)
/// prefix the absolute timestamp, tab-separated, mirroring the
/// per-host command-history export; untimed rows are bare. Multi-line
/// commands indent their continuation lines, same convention as the
/// live-append log.
fn build_command_export(
    label: &str,
    started_at: chrono::DateTime<chrono::Utc>,
    events: &[oryxis_vault::SessionLogEvent],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Oryxis session input: {label}\n# Session started {}\n\n",
        started_at.to_rfc3339()
    ));
    for event in events {
        let cmd = String::from_utf8_lossy(&event.data);
        let cmd_one = cmd.replace('\n', "\n    ");
        match event.offset_ms {
            Some(ms) => {
                let at = started_at + chrono::Duration::milliseconds(ms);
                out.push_str(&format!("{}\t{}\n", at.to_rfc3339(), cmd_one));
            }
            None => out.push_str(&format!("{cmd_one}\n")),
        }
    }
    out
}

/// Save-dialog + write for a text export, off the UI thread. Reports
/// through the shared "Exported to {path}" / failure toast; a
/// dismissed dialog reports nothing.
fn save_text_file_task(
    body: String,
    default_name: String,
    ext: &'static str,
) -> Task<Message> {
    Task::perform(
        tokio::task::spawn_blocking(move || {
            let path = rfd::FileDialog::new()
                .set_file_name(&default_name)
                .add_filter(ext, &[ext])
                .save_file()?;
            Some(
                std::fs::write(&path, body)
                    .map(|_| path.display().to_string())
                    .map_err(|e| e.to_string()),
            )
        }),
        |res| match res {
            Ok(Some(outcome)) => Message::CommandHistory(CommandHistoryMessage::CommandHistoryExported(outcome)),
            _ => Message::NoOp,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::build_asciicast;
    use oryxis_vault::SessionLogEvent;

    fn ev(offset_ms: Option<i64>, kind: char, data: &[u8]) -> SessionLogEvent {
        SessionLogEvent { offset_ms, kind, data: data.to_vec() }
    }

    fn palette() -> oryxis_terminal::TerminalPalette {
        oryxis_terminal::TerminalPalette::default()
    }

    #[test]
    fn asciicast_header_reads_geometry_from_the_first_resize() {
        let started = chrono::DateTime::parse_from_rfc3339("2026-07-04T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let cast = build_asciicast(
            "host-a",
            started,
            "xterm-256color",
            &palette(),
            &[ev(Some(0), 'r', b"120x30"), ev(Some(100), 'o', b"hi")],
        );
        let mut lines = cast.lines();
        let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(header["version"], 3);
        assert_eq!(header["term"]["cols"], 120);
        assert_eq!(header["term"]["rows"], 30);
        assert_eq!(header["term"]["type"], "xterm-256color");
        assert_eq!(header["title"], "host-a");
        assert_eq!(header["timestamp"], started.timestamp());
        let first: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(first[1], "r");
        assert_eq!(first[2], "120x30");
        let second: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(second[0], 0.1);
        assert_eq!(second[1], "o");
        assert_eq!(second[2], "hi");
    }

    #[test]
    fn asciicast_header_embeds_the_terminal_theme() {
        let cast = build_asciicast(
            "host-a",
            chrono::Utc::now(),
            "xterm-256color",
            &palette(),
            &[ev(Some(0), 'o', b"hi")],
        );
        let header: serde_json::Value =
            serde_json::from_str(cast.lines().next().unwrap()).unwrap();
        let theme = &header["term"]["theme"];
        let is_hex = |v: &serde_json::Value| {
            let s = v.as_str().unwrap();
            s.len() == 7
                && s.starts_with('#')
                && s[1..].chars().all(|c| c.is_ascii_hexdigit())
        };
        assert!(is_hex(&theme["fg"]), "bad fg: {theme}");
        assert!(is_hex(&theme["bg"]), "bad bg: {theme}");
        // The v3 spec wants 8 or 16 colon-separated #rrggbb entries;
        // we always emit the full 16-color ANSI set.
        let colors: Vec<&str> =
            theme["palette"].as_str().unwrap().split(':').collect();
        assert_eq!(colors.len(), 16, "bad palette: {theme}");
        assert!(colors
            .iter()
            .all(|c| c.len() == 7 && c.starts_with('#')));
    }

    #[test]
    fn asciicast_skips_typed_command_rows() {
        let started = chrono::Utc::now();
        let cast = build_asciicast(
            "host-a",
            started,
            "xterm-256color",
            &palette(),
            &[
                ev(Some(0), 'o', b"prompt$ "),
                ev(Some(50), 'c', b"ls -la"),
                ev(Some(100), 'o', b"total 0"),
            ],
        );
        assert!(
            !cast.contains("ls -la"),
            "command row leaked into the cast: {cast}"
        );
        assert_eq!(cast.lines().count(), 3, "header + 2 output events");
    }

    #[test]
    fn command_export_prefixes_timestamps_and_indents_continuations() {
        let started = chrono::DateTime::parse_from_rfc3339("2026-07-08T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let body = super::build_command_export(
            "host-a",
            started,
            &[
                ev(Some(60_000), 'c', b"ls -la"),
                ev(None, 'c', b"for f in *; do\necho $f\ndone"),
            ],
        );
        assert!(body.starts_with("# Oryxis session input: host-a\n"));
        assert!(
            body.contains("2026-07-08T10:01:00+00:00\tls -la\n"),
            "timed row missing its absolute timestamp: {body}"
        );
        // Untimed rows are bare; continuation lines stay indented so
        // the file remains greppable per entry.
        assert!(
            body.contains("\nfor f in *; do\n    echo $f\n    done\n"),
            "untimed multi-line row malformed: {body}"
        );
    }

    #[test]
    fn untimed_events_replay_with_a_fixed_delta_and_intervals_never_regress() {
        let started = chrono::Utc::now();
        let cast = build_asciicast(
            "legacy",
            started,
            "vt100",
            &palette(),
            &[
                ev(None, 'o', b"one"),
                ev(None, 'o', b"two"),
                // A stamped event earlier than the synthetic clock must
                // clamp forward: v3 intervals are relative to the
                // previous event and can never be negative.
                ev(Some(20), 'o', b"three"),
            ],
        );
        let intervals: Vec<f64> = cast
            .lines()
            .skip(1)
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()[0].as_f64().unwrap())
            .collect();
        assert_eq!(intervals, vec![0.05, 0.05, 0.0]);
        // No resize event anywhere: the header falls back to 80x24.
        let header: serde_json::Value =
            serde_json::from_str(cast.lines().next().unwrap()).unwrap();
        assert_eq!(header["term"]["cols"], 80);
        assert_eq!(header["term"]["rows"], 24);
    }
}

#[cfg(test)]
mod content_search_tests {
    use super::content_match_snippet;

    #[test]
    fn finds_case_insensitive_and_returns_the_line() {
        let text = "prompt$ ls\ntotal 0\nprompt$ KUBECTL get pods\nNAME READY\n";
        assert_eq!(
            content_match_snippet(text, "kubectl").as_deref(),
            Some("prompt$ KUBECTL get pods"),
        );
        assert_eq!(content_match_snippet(text, "terraform"), None);
        assert_eq!(content_match_snippet(text, ""), None);
    }

    #[test]
    fn long_lines_are_windowed_with_ellipses() {
        let line = format!("{}kubectl drain node-1{}", "x".repeat(100), "y".repeat(100));
        let snip = content_match_snippet(&line, "kubectl drain").unwrap();
        assert!(snip.starts_with('\u{2026}') && snip.ends_with('\u{2026}'), "{snip}");
        assert!(snip.contains("kubectl drain node-1"));
        // 40 chars of context each side + the match + the ellipses.
        assert!(snip.chars().count() < 120, "{snip}");
    }

    #[test]
    fn unicode_case_folds_without_shifting_offsets() {
        let text = "echo Ünïcode Test";
        assert_eq!(
            content_match_snippet(text, "ünïcode test").as_deref(),
            Some("echo Ünïcode Test"),
        );
    }
}
