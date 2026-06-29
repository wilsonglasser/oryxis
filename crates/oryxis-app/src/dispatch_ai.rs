//! `Oryxis::handle_ai`, match arms for the AI side of the app:
//! provider/model/api-key Settings panel knobs, and the chat sidebar
//! conversation flow (send, receive, retry, tool exec).

#![allow(clippy::result_large_err)]

use iced::widget::text_editor;
use iced::Task;

use std::sync::Arc;

use crate::app::{Message, Oryxis};
use crate::state::{ChatMessage, ChatRole};
use crate::util::chat_scroll_to_end;

/// Cap on consecutive AI-auto-executed commands per user turn. Past this,
/// further auto-exec is refused and the command is surfaced for explicit
/// approval. A backstop for runaway loops of *different* commands;
/// exact-repeat loops are caught earlier by `chat_auto_run_history`.
const CHAT_AUTO_RUN_STREAK_MAX: usize = 12;

/// Flags that decide how a proposed AI tool call is gated. Pulled out of
/// `Oryxis` state so the decision is a pure function (see
/// [`classify_tool_gate`]) and can be unit-tested.
struct ToolGateInput {
    /// First token is on the tab's "always run" allow-list.
    allowed: bool,
    /// Command chains / pipes / redirects / substitutes (e.g. `ls; rm`).
    has_chaining: bool,
    /// The model self-classified the command as `safe`.
    risk_safe: bool,
    /// Command matches the deterministic catastrophic-command floor.
    obviously_destructive: bool,
    /// The per-turn auto-run streak has reached `CHAT_AUTO_RUN_STREAK_MAX`.
    streak_exceeded: bool,
    /// This exact command was already auto-run earlier in the turn.
    already_auto_ran: bool,
}

/// What to do with a proposed tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolGate {
    /// Run it immediately, no prompt.
    AutoExec,
    /// Surface it for explicit user approval (loop guard or destructive floor).
    Confirm,
    /// Hand to the independent auto-exec judge.
    Judge,
    /// Queue a pending-tool bubble (risky / unclassified).
    Prompt,
}

/// Decide how a proposed tool call is gated. Order matters only between
/// `AutoExec` and the guards, every guard collapses to `Confirm`, so the
/// relative order of the destructive floor and the loop guards is
/// irrelevant to the outcome.
fn classify_tool_gate(i: ToolGateInput) -> ToolGate {
    // Allow-listed simple command: runs unattended, but even a trusted
    // command can't loop forever, so the streak backstop still applies.
    // Exact-repeat is deliberately NOT applied to allow-listed commands
    // (the user may legitimately want `ls` run more than once).
    if i.allowed && !i.has_chaining {
        return if i.streak_exceeded {
            ToolGate::Confirm
        } else {
            ToolGate::AutoExec
        };
    }
    // Deterministic catastrophic-command floor: always prompt, never judged.
    if i.risk_safe && i.obviously_destructive {
        return ToolGate::Confirm;
    }
    // Loop guards on the judge path: a repeated command or an over-long
    // streak is refused auto-exec and surfaced instead. This is what breaks
    // the runaway loop with no user action.
    if i.risk_safe && (i.streak_exceeded || i.already_auto_ran) {
        return ToolGate::Confirm;
    }
    // Model-claimed safe and nothing objected: let the judge decide.
    if i.risk_safe {
        return ToolGate::Judge;
    }
    // Risky or unclassified: explicit prompt.
    ToolGate::Prompt
}

impl Oryxis {
    /// Abort the in-flight chat stream (if any) and forget its handle.
    /// Aborting the iced stream drops the receiver feeding it, which makes
    /// the detached tool-followup task's `tx.send` fail so it stops too.
    pub(crate) fn abort_chat_task(&mut self) {
        if let Some(handle) = self.chat_task.take() {
            handle.abort();
        }
    }

    /// Replace the tracked chat task: abort whatever was running, make the
    /// new task abortable, store its handle, and return the wrapped task to
    /// hand back to iced. Funnel every chat-stream / judge task through
    /// this so a single Stop / close / reset can cancel the live work.
    fn track_chat_task(&mut self, task: Task<Message>) -> Task<Message> {
        self.abort_chat_task();
        let (task, handle) = task.abortable();
        self.chat_task = Some(handle);
        task
    }

    /// Clear the per-turn auto-exec guard state on the active tab. Called
    /// whenever the user retakes control (sends a message, resets, or
    /// explicitly approves a command) so a fresh turn starts with a clean
    /// streak and repeat history.
    fn reset_chat_auto_run_guard(&mut self) {
        if let Some(idx) = self.active_tab
            && let Some(tab) = self.tabs.get_mut(idx)
        {
            tab.chat_auto_run_history.clear();
            tab.chat_auto_run_streak = 0;
        }
    }

    pub(crate) fn handle_ai(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // ── AI settings ──
            Message::ToggleAiEnabled => {
                self.ai.enabled = !self.ai.enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_enabled", if self.ai.enabled { "true" } else { "false" });
                }
            }
            Message::AiProviderChanged(provider) => {
                // Accept either a display name (from the dropdown) or the
                // internal id. Fall back to keeping the current provider if
                // the value can't be resolved.
                let info = crate::ai::provider_from_display(&provider)
                    .unwrap_or_else(|| crate::ai::provider_info(&provider));
                self.ai.provider = info.id.to_string();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_provider", &self.ai.provider);
                }
                // Suggest the provider's default model when the user hasn't
                // picked one. For Custom we keep whatever model is set.
                if !info.default_model.is_empty() {
                    self.ai.model = info.default_model.into();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("ai_model", &self.ai.model);
                    }
                }
                // Presets always use their bundled URL; clear any stale
                // override so Save doesn't carry it across providers.
                if info.kind != crate::ai::ProviderKind::Custom {
                    self.ai.api_url.clear();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("ai_api_url", "");
                    }
                }
            }
            Message::AiModelChanged(model) => {
                self.ai.model = model;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_model", &self.ai.model);
                }
            }
            Message::AiApiKeyChanged(key) => {
                self.ai.api_key = key;
            }
            Message::AiApiUrlChanged(url) => {
                self.ai.api_url = url;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_api_url", &self.ai.api_url);
                }
            }
            Message::AiSystemPromptAction(action) => {
                let was_edit = action.is_edit();
                self.ai.system_prompt.perform(action);
                if was_edit
                    && let Some(vault) = &self.vault
                {
                    let _ = vault.set_setting("ai_system_prompt", &self.ai.system_prompt.text());
                }
            }
            Message::SaveAiApiKey => {
                if !self.ai.api_key.is_empty()
                    && let Some(vault) = &self.vault
                    && vault.set_ai_api_key(&self.ai.api_key).is_ok() {
                        self.ai.api_key.clear();
                        self.ai.api_key_set = true;
                }
            }

            // ── AI chat sidebar ──
            Message::ToggleChatSidebar => {
                let ai_enabled = self.ai.enabled;
                let mut closing = false;
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_visible = !tab.chat_visible;
                        closing = !tab.chat_visible;
                        // When opening with AI off, the Chat tab is hidden, so
                        // land on Snippets instead of an empty panel.
                        if tab.chat_visible
                            && !ai_enabled
                            && self.terminal_sidebar_tab == crate::state::TerminalSidebarTab::Chat
                        {
                            self.terminal_sidebar_tab = crate::state::TerminalSidebarTab::Snippets;
                        }
                }
                // Closing the panel is the user's "stop it" gesture (the
                // reported bug: a runaway tool loop kept running after the
                // sidebar was closed). Cancel any live chat work so it
                // doesn't keep executing commands in the background.
                if closing {
                    self.abort_chat_task();
                    self.chat_loading = false;
                }
            }
            Message::SelectTerminalSidebarTab(tab) => {
                self.terminal_sidebar_tab = tab;
            }
            Message::SidebarSnippetSearchChanged(v) => {
                self.sidebar_snippet_search = v;
            }
            Message::ToggleSidebarSort => {
                self.sidebar_sort_open = !self.sidebar_sort_open;
                if self.sidebar_sort_open {
                    self.sidebar_search_open = false;
                }
            }
            Message::ToggleSidebarSearch => {
                self.sidebar_search_open = !self.sidebar_search_open;
                self.sidebar_sort_open = false;
                if self.sidebar_search_open {
                    return Ok(iced::widget::operation::focus(iced::widget::Id::new(
                        "sidebar-snippet-search",
                    )));
                }
                // Collapsing clears the needle so the list shows everything.
                self.sidebar_snippet_search.clear();
            }
            Message::ChatInputAction(action) => {
                self.chat_input.perform(action);
            }
            Message::ChatScrolled(relative_y) => {
                // Strict end check (not "near end"), relative_offset.y
                // becomes 1.0 when the user is exactly at the bottom.
                // Tiny epsilon covers f32 rounding from the layout pass.
                self.chat_scroll_at_bottom = relative_y >= 0.999;
            }
            Message::ChatResetConversation => {
                // Cancel any in-flight stream first, otherwise the detached
                // tool-followup task would keep running and re-populate the
                // history we're about to clear.
                self.abort_chat_task();
                self.reset_chat_auto_run_guard();
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    tab.chat_history.clear();
                }
                self.chat_loading = false;
                self.chat_scroll_at_bottom = true;
            }
            Message::ChatSidebarResizeStart => {
                // Capture cursor x and current width, the MouseMoved
                // handler computes the delta against these.
                self.chat_sidebar_drag = Some((self.mouse_position.x, self.chat_sidebar_width));
            }
            Message::ChatSidebarResizeStop => {
                self.chat_sidebar_drag = None;
                // The same global Left-release ends an SFTP divider drag;
                // persist the final ratio so it survives a relaunch.
                if self.sftp_split_drag.take().is_some() {
                    self.persist_setting(
                        "sftp_split_ratio",
                        &format!("{:.4}", self.sftp_split_ratio),
                    );
                }
                // Same Left-release ends a log-panel resize; persist the
                // final height so it survives a relaunch.
                if self.sftp_log_drag.take().is_some() {
                    self.persist_setting(
                        "sftp_log_height",
                        &format!("{:.0}", self.sftp.log_height),
                    );
                }
                // End a column resize: the width was updated live, so just
                // re-seed the template and persist.
                if let Some((side, _, _, _)) = self.sftp_col_resize.take() {
                    self.sftp_columns_template = self.sftp.pane(side).columns.clone();
                    self.persist_sftp_columns();
                }
                // End a column reorder. If the drag went active, move the
                // dragged column before whichever header the cursor is over;
                // a release without movement is a plain click that sorts.
                if let Some(drag) = self.sftp_col_drag.take() {
                    let hovered = self.sftp_hovered_col;
                    self.sftp_hovered_col = None;
                    if drag.active {
                        // Name is never a drop target: nothing can be dropped
                        // onto/before it (so it shows no drop effect and keeps
                        // its slot). It can still be dragged elsewhere itself.
                        if let Some((hside, hcol)) = hovered
                            && hside == drag.side
                            && hcol != drag.col
                            && hcol != crate::state::SftpColumn::Name
                        {
                            self.sftp.pane_mut(drag.side).columns.reorder(drag.col, hcol);
                            self.sftp_columns_template =
                                self.sftp.pane(drag.side).columns.clone();
                            self.persist_sftp_columns();
                        }
                    } else if let Some(sort_col) = drag.col.sort_column() {
                        return Ok(Task::done(Message::SftpSort(drag.side, sort_col)));
                    }
                }
                // Same global Left-release event also ends an internal
                // SFTP drag. If the drag was active, dispatch the transfer;
                // otherwise it was a plain click, which may have armed a
                // slow-click rename (set on the press in SftpSelectRow).
                if let Some(drag) = self.sftp.drag.take()
                    && drag.active
                {
                    self.sftp.pending_rename = None;
                    return Ok(self.handle_internal_drag_drop(drag));
                }
                if let Some((side, path)) = self.sftp.pending_rename.take() {
                    return Ok(Task::done(Message::SftpStartRename(side, path)));
                }
                // And ends a tab reorder drag. The live-slide already moved
                // the tab into place during the drag (see TabHovered); on
                // drop we just persist the new pinned order (if the dragged
                // tab is pinned) and clear. A plain click (never promoted to
                // `active`) clears with no persist.
                if let Some(drag) = self.tab_drag.take()
                    && drag.active
                {
                    // Persist when the dragged tab (terminal or SFTP) is pinned,
                    // so the rearranged pinned order survives a relaunch.
                    let pinned = self
                        .tabs
                        .iter()
                        .find(|t| t._id == drag.from_id)
                        .map(|t| t.pinned)
                        .or_else(|| {
                            self.sftp_tabs
                                .iter()
                                .find(|t| t.id == drag.from_id)
                                .map(|t| t.pinned)
                        })
                        .unwrap_or(false);
                    if pinned {
                        self.persist_pinned_tabs();
                    }
                }
            }
            Message::SendChat => {
                let input = self.chat_input.text().trim().to_string();
                if input.is_empty() || !self.ai.enabled {
                    return Ok(Task::none());
                }
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::User,
                            content: input,
                            parsed_md: Vec::new(),
                        });
                        // A fresh user turn clears the auto-exec guard so the
                        // streak / repeat history from the previous turn
                        // doesn't bleed into this one.
                        tab.chat_auto_run_history.clear();
                        tab.chat_auto_run_streak = 0;
                        self.chat_input = text_editor::Content::new();
                        self.chat_loading = true;
                        // Sending a message snaps focus back to the latest
                        // exchange, so the next assistant response should
                        // also follow (until the user scrolls up again).
                        self.chat_scroll_at_bottom = true;

                        // Build AI config
                        let api_key = self.vault.as_ref()
                            .and_then(|v| v.get_ai_api_key().ok().flatten())
                            .unwrap_or_default();

                        // Get additional system prompt from settings
                        let extra_prompt = self.vault.as_ref()
                            .and_then(|v| v.get_setting("ai_system_prompt").ok().flatten());

                        let config = crate::ai::AiConfig {
                            provider: self.ai.provider.clone(),
                            model: self.ai.model.clone(),
                            api_key,
                            api_url: if self.ai.api_url.is_empty() {
                                None
                            } else {
                                Some(self.ai.api_url.clone())
                            },
                            system_prompt: extra_prompt,
                        };

                        // Get last ~50 lines of terminal output for context
                        let terminal_context = if let Ok(state) = tab.active().terminal.lock() {
                            let term = &state.backend.term;
                            let content = term.renderable_content();
                            let mut lines: Vec<String> = Vec::new();
                            let mut current_line = String::new();
                            let mut last_row = 0i32;
                            for item in content.display_iter {
                                let row = item.point.line.0;
                                if row != last_row && !current_line.is_empty() {
                                    lines.push(std::mem::take(&mut current_line));
                                    last_row = row;
                                }
                                let c = item.cell.c;
                                if c != '\0' {
                                    current_line.push(c);
                                }
                            }
                            if !current_line.is_empty() {
                                lines.push(current_line);
                            }
                            // Take last 50 lines
                            let start = lines.len().saturating_sub(50);
                            lines[start..].join("\n")
                        } else {
                            String::new()
                        };

                        // Build messages: inject terminal context as first user message
                        let mut messages: Vec<crate::ai::ChatMsg> = Vec::new();
                        if !terminal_context.is_empty() {
                            messages.push(crate::ai::ChatMsg {
                                role: "user".into(),
                                content: serde_json::Value::String(format!(
                                    "[Current terminal output (last ~50 lines)]\n```\n{}\n```",
                                    terminal_context
                                )),
                            });
                            messages.push(crate::ai::ChatMsg {
                                role: "assistant".into(),
                                content: serde_json::Value::String(
                                    "I can see the terminal output. How can I help?".into()
                                ),
                            });
                        }
                        // Add chat history. Skip Error bubbles (UI-only)
                        // and empty assistant placeholders (the staging
                        // slots streaming chunks land in, sending an
                        // empty `assistant: ""` upsets some providers).
                        messages.extend(
                            tab.chat_history
                                .iter()
                                .filter(|m| {
                                    !(matches!(m.role, ChatRole::Error | ChatRole::PendingTool)
                                        || (m.role == ChatRole::Assistant
                                            && m.content.is_empty()))
                                })
                                .map(|m| crate::ai::ChatMsg {
                                    role: match m.role {
                                        ChatRole::User => "user".into(),
                                        ChatRole::Assistant => "assistant".into(),
                                        ChatRole::System => "user".into(),
                                        ChatRole::Error | ChatRole::PendingTool => unreachable!(),
                                    },
                                    content: serde_json::Value::String(m.content.clone()),
                                }),
                        );

                        // Insert an empty assistant placeholder so
                        // streamed text deltas have a bubble to land in.
                        // The view filters out empty assistant bubbles
                        // (they look like a glitch); the message builder
                        // above skips them too, so they remain in
                        // history harmlessly until the next reset.
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::Assistant,
                            content: String::new(),
                            parsed_md: Vec::new(),
                        });

                        let stream_task = Task::stream(
                            crate::ai::send_chat_stream(config, messages),
                        )
                        .map(|chunk| match chunk {
                            crate::ai::StreamChunk::Text(t) => Message::ChatStreamChunk(t),
                            crate::ai::StreamChunk::ToolUse { command, risk } => {
                                Message::ChatToolProposed { command, risk }
                            }
                            crate::ai::StreamChunk::Done => Message::ChatStreamDone,
                            crate::ai::StreamChunk::Error(e) => Message::ChatError(e),
                        });
                        // Track the stream so Stop / close / reset can abort it.
                        let stream_task = self.track_chat_task(stream_task);
                        return Ok(Task::batch(vec![chat_scroll_to_end(), stream_task]));
                }
            }
            Message::ChatStreamChunk(delta) => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                    && let Some(last) = tab.chat_history.last_mut()
                    && last.role == ChatRole::Assistant
                {
                    last.content.push_str(&delta);
                    // Markdown parse is O(content), so re-parsing on
                    // every token makes a long streamed reply O(n^2).
                    // Throttle to ~10 parses/s; `ChatStreamDone` does
                    // the final authoritative parse. Single static is
                    // enough: one chat stream runs at a time.
                    static LAST_MD_PARSE: std::sync::Mutex<Option<std::time::Instant>> =
                        std::sync::Mutex::new(None);
                    let now = std::time::Instant::now();
                    let mut guard = LAST_MD_PARSE.lock().unwrap();
                    let due = guard
                        .map(|t| now.duration_since(t).as_millis() >= 100)
                        .unwrap_or(true);
                    if due {
                        *guard = Some(now);
                        drop(guard);
                        last.parsed_md =
                            iced::widget::markdown::parse(&last.content).collect();
                    }
                }
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
            }
            Message::ChatStreamDone => {
                // Final parse so the rendered markdown can't lag behind
                // the throttled streaming parses above.
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                    && let Some(last) = tab.chat_history.last_mut()
                    && last.role == ChatRole::Assistant
                {
                    last.parsed_md =
                        iced::widget::markdown::parse(&last.content).collect();
                }
                // Empty assistant placeholders are filtered out at the
                // view layer and excluded from the message-builder when
                // we send to the model, so we don't try to pop them
                // here. (Popping was racy when a tool followup pushed
                // its own placeholder before the original stream's Done
                // arrived.)
                // The stream finished on its own; drop the now-spent abort
                // handle so a later Stop doesn't try to cancel nothing.
                self.chat_task = None;
                self.chat_loading = false;
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
            }
            Message::ChatStop => {
                // User asked to stop. Abort the live stream (and the
                // detached tool-followup pipeline it feeds) and freeze the
                // auto-exec guard where it is, so nothing else runs until
                // the user sends the next message.
                self.abort_chat_task();
                self.chat_loading = false;
            }
            Message::ChatError(e) => {
                // Provider/network failures get their own role so the
                // bubble can render with an error treatment + Retry,
                // instead of being indistinguishable from a real reply.
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    // If the stream errored before the model wrote any
                    // text, drop the empty assistant placeholder so we
                    // don't render a blank bubble above the error.
                    if let Some(last) = tab.chat_history.last()
                        && last.role == ChatRole::Assistant
                        && last.content.is_empty()
                    {
                        tab.chat_history.pop();
                    }
                    tab.chat_history.push(ChatMessage {
                        role: ChatRole::Error,
                        content: e,
                        parsed_md: Vec::new(),
                    });
                }
                self.chat_task = None;
                self.chat_loading = false;
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
            }
            Message::ChatRetry => {
                // Strip the trailing error bubble + the user message
                // that led to it, then re-dispatch SendChat so the
                // existing pipeline pushes a fresh user message and
                // re-sends. Without popping the user msg too, retry
                // would duplicate it in history.
                let Some(idx) = self.active_tab else {
                    return Ok(Task::none());
                };
                let last_user: Option<String> = {
                    let Some(tab) = self.tabs.get_mut(idx) else {
                        return Ok(Task::none());
                    };
                    // Pop trailing Error / Assistant entries, the
                    // Assistants are partial-stream remnants (could be
                    // empty placeholders or text that arrived before
                    // the error). Then pop the user message so the
                    // re-dispatch pushes it fresh.
                    while matches!(
                        tab.chat_history.last().map(|m| &m.role),
                        Some(ChatRole::Error) | Some(ChatRole::Assistant)
                    ) {
                        tab.chat_history.pop();
                    }
                    if matches!(
                        tab.chat_history.last().map(|m| &m.role),
                        Some(ChatRole::User)
                    ) {
                        tab.chat_history.pop().map(|m| m.content)
                    } else {
                        None
                    }
                };
                if let Some(text) = last_user {
                    self.chat_input = text_editor::Content::with_text(&text);
                    return Ok(Task::done(Message::SendChat));
                }
            }
            Message::ChatToolProposed { command, risk } => {
                // Gate the tool call: safe commands run immediately;
                // risky ones become a `PendingTool` bubble waiting on
                // a RUN / ALWAYS RUN / DENY click. The first whitespace
                // token is matched against the tab's allow-list so the
                // user's "always run X" decisions stick across the
                // session.
                let first_token = command
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let allowed = self
                    .active_tab
                    .and_then(|i| self.tabs.get(i))
                    .map(|tab| {
                        tab.chat_always_run_commands
                            .iter()
                            .any(|c| c == &first_token)
                    })
                    .unwrap_or(false);
                // Loop guards, read from the active tab's per-turn auto-exec
                // state. `streak_exceeded` catches a long run of *different*
                // auto-executed commands; `already_auto_ran` catches the
                // model re-proposing the exact same command (the reported
                // `docker --version` loop). Both convert an auto-exec into a
                // confirmation prompt so the loop can't run unattended.
                let (streak_exceeded, already_auto_ran) = self
                    .active_tab
                    .and_then(|i| self.tabs.get(i))
                    .map(|tab| {
                        (
                            tab.chat_auto_run_streak >= CHAT_AUTO_RUN_STREAK_MAX,
                            tab.chat_auto_run_history.iter().any(|c| c == &command),
                        )
                    })
                    .unwrap_or((false, false));
                // Decide how this tool call is gated. The branching (allow-list
                // bypass, destructive floor, loop guards, judge, prompt) is a
                // pure function of these flags so it can be unit-tested without
                // a live `Oryxis`, see `classify_tool_gate`.
                let gate = classify_tool_gate(ToolGateInput {
                    allowed,
                    has_chaining: crate::ai::has_shell_chaining(&command),
                    risk_safe: risk == "safe",
                    obviously_destructive: crate::ai::is_obviously_destructive(&command),
                    streak_exceeded,
                    already_auto_ran,
                });
                match gate {
                    // Allow-listed simple command under the streak cap: run now.
                    ToolGate::AutoExec => {
                        return Ok(Task::done(Message::ChatToolExec(command)));
                    }
                    // Loop guard tripped, or the deterministic destructive
                    // floor fired: surface it for explicit approval instead of
                    // running it unattended. This is what stops the reported
                    // runaway loop on its own.
                    ToolGate::Confirm => {
                        return Ok(Task::done(Message::ChatToolGuardBlocked { command }));
                    }
                    // Model-claimed `safe` and nothing above objected: hand to
                    // the independent auto-exec judge, which can only escalate
                    // to a prompt, never approve, and fails safe on error.
                    ToolGate::Judge => {
                        let api_key = self
                            .vault
                            .as_ref()
                            .and_then(|v| v.get_ai_api_key().ok().flatten())
                            .unwrap_or_default();
                        let config = crate::ai::AiConfig {
                            provider: self.ai.provider.clone(),
                            model: self.ai.model.clone(),
                            api_key,
                            api_url: if self.ai.api_url.is_empty() {
                                None
                            } else {
                                Some(self.ai.api_url.clone())
                            },
                            system_prompt: None,
                        };
                        self.chat_loading = true;
                        let cmd_for_judge = command.clone();
                        let judge = Task::perform(
                            crate::ai::judge_auto_exec(config, cmd_for_judge),
                            move |allow| {
                                if allow {
                                    Message::ChatToolExec(command.clone())
                                } else {
                                    Message::ChatToolGuardBlocked {
                                        command: command.clone(),
                                    }
                                }
                            },
                        );
                        // Track the judge call so Stop cancels a pending
                        // auto-exec decision before it can fire ChatToolExec
                        // and run a command behind the user's back.
                        return Ok(self.track_chat_task(judge));
                    }
                    // Risky / unclassified: fall through to the pending bubble.
                    ToolGate::Prompt => {}
                }
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    // Drop the empty assistant placeholder if the model
                    // went straight to a tool call without any text.
                    if let Some(last) = tab.chat_history.last()
                        && last.role == ChatRole::Assistant
                        && last.content.is_empty()
                    {
                        tab.chat_history.pop();
                    }
                    tab.chat_history.push(ChatMessage {
                        role: ChatRole::PendingTool,
                        content: command,
                        parsed_md: Vec::new(),
                    });
                }
                self.chat_loading = false;
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
            }
            Message::ChatToolGuardBlocked { command } => {
                // The independent judge declined to auto-run this
                // model-claimed `safe` command, so surface it for explicit
                // approval exactly like a risky one.
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    if let Some(last) = tab.chat_history.last()
                        && last.role == ChatRole::Assistant
                        && last.content.is_empty()
                    {
                        tab.chat_history.pop();
                    }
                    tab.chat_history.push(ChatMessage {
                        role: ChatRole::PendingTool,
                        content: command,
                        parsed_md: Vec::new(),
                    });
                }
                self.chat_loading = false;
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
            }
            Message::ChatToolApprove(command) => {
                // Pop the pending bubble that triggered this approval.
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                    && let Some(last) = tab.chat_history.last()
                    && last.role == ChatRole::PendingTool
                {
                    tab.chat_history.pop();
                }
                // The user just retook control, so a command they approve
                // starts a fresh auto-exec chain (clears the streak / repeat
                // history the loop guard accumulated).
                self.reset_chat_auto_run_guard();
                return Ok(Task::done(Message::ChatToolExec(command)));
            }
            Message::ChatToolApproveAlways(command) => {
                let first_token = command
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    if !first_token.is_empty()
                        && !tab
                            .chat_always_run_commands
                            .iter()
                            .any(|c| c == &first_token)
                    {
                        tab.chat_always_run_commands.push(first_token);
                    }
                    if let Some(last) = tab.chat_history.last()
                        && last.role == ChatRole::PendingTool
                    {
                        tab.chat_history.pop();
                    }
                    // User retook control: start a fresh auto-exec chain.
                    tab.chat_auto_run_history.clear();
                    tab.chat_auto_run_streak = 0;
                }
                return Ok(Task::done(Message::ChatToolExec(command)));
            }
            Message::ChatToolDeny(_command) => {
                // User said no. Drop the pending bubble and stop
                // the AI doesn't get a callback; the user can write
                // their own follow-up if they want a textual answer.
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                    && let Some(last) = tab.chat_history.last()
                    && last.role == ChatRole::PendingTool
                {
                    tab.chat_history.pop();
                }
                self.chat_loading = false;
            }
            Message::ChatToolExec(command) => {
                // AI requested to execute a command in the terminal
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::System,
                            content: format!("$ {}", command),
                            parsed_md: Vec::new(),
                        });

                        // Record this execution against the per-turn loop
                        // guard. A later proposal of the same command (or one
                        // past the streak cap) is then refused auto-exec by
                        // `ChatToolProposed`. User-approval paths reset this
                        // first, so the count only ever reflects commands run
                        // since the user last took control. Cap the history
                        // length so a long agentic turn can't grow it without
                        // bound; the repeated command stays within the window.
                        tab.chat_auto_run_history.push(command.clone());
                        if tab.chat_auto_run_history.len() > 50 {
                            tab.chat_auto_run_history.remove(0);
                        }
                        tab.chat_auto_run_streak += 1;

                        // Write the command to the terminal
                        let cmd_bytes = format!("{}\n", command);
                        if let Some(ref ssh) = tab.active().ssh_session {
                            let _ = ssh.write(cmd_bytes.as_bytes());
                        } else if let Ok(mut state) = tab.active().terminal.lock() {
                            state.write(cmd_bytes.as_bytes());
                        }

                        // Wait 1.5s for output, then capture terminal and send back to AI
                        let terminal = Arc::clone(&tab.active().terminal);
                        let api_key = self.vault.as_ref()
                            .and_then(|v| v.get_ai_api_key().ok().flatten())
                            .unwrap_or_default();
                        let extra_prompt = self.vault.as_ref()
                            .and_then(|v| v.get_setting("ai_system_prompt").ok().flatten());

                        let config = crate::ai::AiConfig {
                            provider: self.ai.provider.clone(),
                            model: self.ai.model.clone(),
                            api_key,
                            api_url: if self.ai.api_url.is_empty() { None } else { Some(self.ai.api_url.clone()) },
                            system_prompt: extra_prompt,
                        };

                        // Build message history including the tool result.
                        // Errors are skipped, they're a UI-only concern,
                        // not part of the conversation we want to send
                        // back to the model.
                        let mut messages: Vec<crate::ai::ChatMsg> = tab
                            .chat_history
                            .iter()
                            .filter(|m| {
                                !(matches!(m.role, ChatRole::Error | ChatRole::PendingTool)
                                    || (m.role == ChatRole::Assistant
                                        && m.content.is_empty()))
                            })
                            .map(|m| crate::ai::ChatMsg {
                                role: match m.role {
                                    ChatRole::User => "user".into(),
                                    ChatRole::Assistant => "assistant".into(),
                                    ChatRole::System => "user".into(),
                                    ChatRole::Error | ChatRole::PendingTool => unreachable!(),
                                },
                                content: serde_json::Value::String(m.content.clone()),
                            })
                            .collect();

                        let cmd_clone = command.clone();

                        // Push an empty assistant placeholder so the
                        // followup stream's text deltas have a bubble
                        // to land in (mirrors SendChat's flow).
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::Assistant,
                            content: String::new(),
                            parsed_md: Vec::new(),
                        });

                        // Spawn a single tokio task that owns the whole
                        // pipeline: terminal poll → append tool-result
                        // user message → forward streaming chunks. The
                        // outer mpsc lets us turn this into a single
                        // `Task::stream` so dispatch sees one stream of
                        // chunks, regardless of which provider is wired.
                        use futures_util::StreamExt as _;
                        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<crate::ai::StreamChunk>();
                        tokio::spawn(async move {
                            // Poll terminal until output stabilizes (no
                            // change for 800ms) or timeout after 15s.
                            let poll_interval = std::time::Duration::from_millis(300);
                            let stable_threshold = std::time::Duration::from_millis(800);
                            let max_wait = std::time::Duration::from_secs(15);
                            let start_time = std::time::Instant::now();
                            let mut last_snapshot = String::new();
                            let mut stable_since = std::time::Instant::now();
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            loop {
                                let snapshot = if let Ok(state) = terminal.lock() {
                                    let term = &state.backend.term;
                                    let content = term.renderable_content();
                                    let mut lines: Vec<String> = Vec::new();
                                    let mut current_line = String::new();
                                    let mut last_row = 0i32;
                                    for item in content.display_iter {
                                        let row = item.point.line.0;
                                        if row != last_row && !current_line.is_empty() {
                                            lines.push(std::mem::take(&mut current_line));
                                            last_row = row;
                                        }
                                        let c = item.cell.c;
                                        if c != '\0' { current_line.push(c); }
                                    }
                                    if !current_line.is_empty() { lines.push(current_line); }
                                    let start = lines.len().saturating_sub(40);
                                    lines[start..].join("\n")
                                } else {
                                    break;
                                };
                                if snapshot != last_snapshot {
                                    last_snapshot = snapshot;
                                    stable_since = std::time::Instant::now();
                                } else if stable_since.elapsed() >= stable_threshold {
                                    break;
                                }
                                if start_time.elapsed() >= max_wait {
                                    break;
                                }
                                tokio::time::sleep(poll_interval).await;
                            }

                            messages.push(crate::ai::ChatMsg {
                                role: "user".into(),
                                content: serde_json::Value::String(format!(
                                    "[Command executed: `{}`]\nOutput:\n```\n{}\n```\nPlease analyze the output and respond.",
                                    cmd_clone, last_snapshot
                                )),
                            });

                            let mut inner = crate::ai::send_chat_stream(config, messages);
                            while let Some(chunk) = inner.next().await {
                                if tx.send(chunk).is_err() {
                                    break;
                                }
                            }
                        });

                        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
                        let followup = Task::stream(stream).map(|chunk| match chunk {
                            crate::ai::StreamChunk::Text(t) => Message::ChatStreamChunk(t),
                            crate::ai::StreamChunk::ToolUse { command, risk } => {
                                Message::ChatToolProposed { command, risk }
                            }
                            crate::ai::StreamChunk::Done => Message::ChatStreamDone,
                            crate::ai::StreamChunk::Error(e) => Message::ChatError(e),
                        });
                        // Track the followup stream too: it's the part that
                        // keeps the tool loop going, so Stop / close / reset
                        // must be able to abort it. This supersedes the
                        // original send stream's handle (already spent).
                        return Ok(self.track_chat_task(followup));
                }
            }
            Message::ChatToolResult(output) => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::System,
                            content: output,
                            parsed_md: Vec::new(),
                        });
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_tool_gate, ToolGate, ToolGateInput};

    /// Convenience builder: everything off (a plain risky/unclassified call).
    fn input() -> ToolGateInput {
        ToolGateInput {
            allowed: false,
            has_chaining: false,
            risk_safe: false,
            obviously_destructive: false,
            streak_exceeded: false,
            already_auto_ran: false,
        }
    }

    #[test]
    fn unclassified_command_prompts() {
        assert_eq!(classify_tool_gate(input()), ToolGate::Prompt);
    }

    #[test]
    fn safe_command_goes_to_judge() {
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Judge);
    }

    #[test]
    fn allow_listed_simple_command_auto_execs() {
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::AutoExec);
    }

    #[test]
    fn allow_listed_chained_command_falls_through_to_prompt() {
        // A trusted first token can't smuggle a chained command past the gate.
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            has_chaining: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Prompt);
    }

    #[test]
    fn repeated_safe_command_is_refused_auto_exec() {
        // The reported bug: the model re-proposing `docker --version`. A
        // safe command already auto-run this turn must be surfaced, not
        // auto-run again, so the loop can't continue unattended.
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            already_auto_ran: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }

    #[test]
    fn over_long_streak_is_refused_even_when_safe() {
        // Backstop for a run of *different* safe commands.
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            streak_exceeded: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }

    #[test]
    fn streak_cap_also_blocks_allow_listed_commands() {
        // A loop of an always-run command is still a loop.
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            streak_exceeded: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }

    #[test]
    fn allow_listed_repeat_without_streak_still_auto_execs() {
        // Exact-repeat is NOT applied to allow-listed commands: the user
        // may legitimately want `ls` run more than once in a turn.
        let gate = classify_tool_gate(ToolGateInput {
            allowed: true,
            already_auto_ran: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::AutoExec);
    }

    #[test]
    fn destructive_safe_command_is_refused_before_judge() {
        // The deterministic floor fires regardless of the judge.
        let gate = classify_tool_gate(ToolGateInput {
            risk_safe: true,
            obviously_destructive: true,
            ..input()
        });
        assert_eq!(gate, ToolGate::Confirm);
    }
}
