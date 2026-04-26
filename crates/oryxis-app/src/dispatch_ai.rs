//! `Oryxis::handle_ai` — match arms for the AI side of the app:
//! provider/model/api-key Settings panel knobs, and the chat sidebar
//! conversation flow (send, receive, retry, tool exec).

#![allow(clippy::result_large_err)]

use iced::widget::text_editor;
use iced::Task;

use std::sync::Arc;

use crate::app::{Message, Oryxis};
use crate::state::{ChatMessage, ChatRole};
use crate::util::chat_scroll_to_end;

impl Oryxis {
    pub(crate) fn handle_ai(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // ── AI settings ──
            Message::ToggleAiEnabled => {
                self.ai_enabled = !self.ai_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_enabled", if self.ai_enabled { "true" } else { "false" });
                }
            }
            Message::AiProviderChanged(provider) => {
                // Accept either a display name (from the dropdown) or the
                // internal id. Fall back to keeping the current provider if
                // the value can't be resolved.
                let info = crate::ai::provider_from_display(&provider)
                    .unwrap_or_else(|| crate::ai::provider_info(&provider));
                self.ai_provider = info.id.to_string();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_provider", &self.ai_provider);
                }
                // Suggest the provider's default model when the user hasn't
                // picked one. For Custom we keep whatever model is set.
                if !info.default_model.is_empty() {
                    self.ai_model = info.default_model.into();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("ai_model", &self.ai_model);
                    }
                }
                // Presets always use their bundled URL; clear any stale
                // override so Save doesn't carry it across providers.
                if info.kind != crate::ai::ProviderKind::Custom {
                    self.ai_api_url.clear();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("ai_api_url", "");
                    }
                }
            }
            Message::AiModelChanged(model) => {
                self.ai_model = model;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_model", &self.ai_model);
                }
            }
            Message::AiApiKeyChanged(key) => {
                self.ai_api_key = key;
            }
            Message::AiApiUrlChanged(url) => {
                self.ai_api_url = url;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("ai_api_url", &self.ai_api_url);
                }
            }
            Message::AiSystemPromptAction(action) => {
                let was_edit = action.is_edit();
                self.ai_system_prompt.perform(action);
                if was_edit
                    && let Some(vault) = &self.vault
                {
                    let _ = vault.set_setting("ai_system_prompt", &self.ai_system_prompt.text());
                }
            }
            Message::SaveAiApiKey => {
                if !self.ai_api_key.is_empty()
                    && let Some(vault) = &self.vault
                    && vault.set_ai_api_key(&self.ai_api_key).is_ok() {
                        self.ai_api_key.clear();
                        self.ai_api_key_set = true;
                }
            }

            // ── AI chat sidebar ──
            Message::ToggleChatSidebar => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_visible = !tab.chat_visible;
                }
            }
            Message::ChatInputAction(action) => {
                self.chat_input.perform(action);
            }
            Message::ChatScrolled(relative_y) => {
                // Strict end check (not "near end") — relative_offset.y
                // becomes 1.0 when the user is exactly at the bottom.
                // Tiny epsilon covers f32 rounding from the layout pass.
                self.chat_scroll_at_bottom = relative_y >= 0.999;
            }
            Message::ChatResetConversation => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                {
                    tab.chat_history.clear();
                }
                self.chat_loading = false;
                self.chat_scroll_at_bottom = true;
            }
            Message::ChatSidebarResizeStart => {
                // Capture cursor x and current width — the MouseMoved
                // handler computes the delta against these.
                self.chat_sidebar_drag = Some((self.mouse_position.x, self.chat_sidebar_width));
            }
            Message::ChatSidebarResizeStop => {
                self.chat_sidebar_drag = None;
                // Same global Left-release event also ends an internal
                // SFTP drag — if the drag was active, dispatch the
                // transfer; if not, just clear (it was a plain click).
                if let Some(drag) = self.sftp.drag.take()
                    && drag.active
                {
                    return Ok(self.handle_internal_drag_drop(drag));
                }
            }
            Message::SendChat => {
                let input = self.chat_input.text().trim().to_string();
                if input.is_empty() || !self.ai_enabled {
                    return Ok(Task::none());
                }
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::User,
                            content: input,
                            timestamp: chrono::Utc::now(),
                            parsed_md: Vec::new(),
                        });
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
                            provider: self.ai_provider.clone(),
                            model: self.ai_model.clone(),
                            api_key,
                            api_url: if self.ai_api_url.is_empty() {
                                None
                            } else {
                                Some(self.ai_api_url.clone())
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
                        // slots streaming chunks land in — sending an
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
                            timestamp: chrono::Utc::now(),
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
                    // Re-parse markdown on every delta so code blocks
                    // and lists render progressively. Cheap on the
                    // sub-2KB messages we typically see; if this ever
                    // shows up in profiling, throttle to every Nth
                    // chunk or batch by token count.
                    last.parsed_md =
                        iced::widget::markdown::parse(&last.content).collect();
                }
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
            }
            Message::ChatStreamDone => {
                // Empty assistant placeholders are filtered out at the
                // view layer and excluded from the message-builder when
                // we send to the model — so we don't try to pop them
                // here. (Popping was racy when a tool followup pushed
                // its own placeholder before the original stream's Done
                // arrived.)
                self.chat_loading = false;
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
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
                        timestamp: chrono::Utc::now(),
                        parsed_md: Vec::new(),
                    });
                }
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
                    // Pop trailing Error / Assistant entries — the
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
                if risk == "safe" || allowed {
                    return Ok(Task::done(Message::ChatToolExec(command)));
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
                        timestamp: chrono::Utc::now(),
                        parsed_md: Vec::new(),
                    });
                }
                self.chat_loading = false;
                if self.chat_scroll_at_bottom {
                    return Ok(chat_scroll_to_end());
                }
            }
            Message::ChatToolApprove(command) => {
                tracing::info!("ChatToolApprove fired: {}", command);
                // Pop the pending bubble that triggered this approval.
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx)
                    && let Some(last) = tab.chat_history.last()
                    && last.role == ChatRole::PendingTool
                {
                    tab.chat_history.pop();
                }
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
                }
                return Ok(Task::done(Message::ChatToolExec(command)));
            }
            Message::ChatToolDeny(_command) => {
                // User said no. Drop the pending bubble and stop —
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
                            timestamp: chrono::Utc::now(),
                            parsed_md: Vec::new(),
                        });

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
                            provider: self.ai_provider.clone(),
                            model: self.ai_model.clone(),
                            api_key,
                            api_url: if self.ai_api_url.is_empty() { None } else { Some(self.ai_api_url.clone()) },
                            system_prompt: extra_prompt,
                        };

                        // Build message history including the tool result.
                        // Errors are skipped — they're a UI-only concern,
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
                            timestamp: chrono::Utc::now(),
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
                        return Ok(Task::stream(stream).map(|chunk| match chunk {
                            crate::ai::StreamChunk::Text(t) => Message::ChatStreamChunk(t),
                            crate::ai::StreamChunk::ToolUse { command, risk } => {
                                Message::ChatToolProposed { command, risk }
                            }
                            crate::ai::StreamChunk::Done => Message::ChatStreamDone,
                            crate::ai::StreamChunk::Error(e) => Message::ChatError(e),
                        }));
                }
            }
            Message::ChatToolResult(output) => {
                if let Some(idx) = self.active_tab
                    && let Some(tab) = self.tabs.get_mut(idx) {
                        tab.chat_history.push(ChatMessage {
                            role: ChatRole::System,
                            content: output,
                            timestamp: chrono::Utc::now(),
                            parsed_md: Vec::new(),
                        });
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
