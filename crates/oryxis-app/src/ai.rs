use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful terminal assistant embedded in Oryxis SSH client. You can execute bash commands in the user's active SSH session using the execute_command tool. Be concise and practical. When the user asks you to do something on the server, use the tool. You also receive the last lines of terminal output for context.";

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub provider: String,   // see PROVIDERS ids below
    pub model: String,
    pub api_key: String,
    pub api_url: Option<String>,
    pub system_prompt: Option<String>, // additional system instructions
}

// ---------------------------------------------------------------------------
// Provider registry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    Gemini,
    OpenAiCompat,
    Custom,
}

pub struct ProviderInfo {
    pub id: &'static str,
    pub display: &'static str,
    pub default_url: &'static str,
    pub default_model: &'static str,
    pub kind: ProviderKind,
}

// Ordered list used to populate the provider picker. Anthropic / OpenAI /
// Gemini have dedicated codepaths; everything else is OpenAI-compat and
// reuses `send_openai` with a different base URL.
pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "anthropic",
        display: "Anthropic",
        default_url: "https://api.anthropic.com/v1/messages",
        default_model: "claude-sonnet-4-20250514",
        kind: ProviderKind::Anthropic,
    },
    ProviderInfo {
        id: "openai",
        display: "OpenAI",
        default_url: "https://api.openai.com/v1/chat/completions",
        default_model: "gpt-4o",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "gemini",
        display: "Google Gemini",
        default_url: "",
        default_model: "gemini-2.5-flash",
        kind: ProviderKind::Gemini,
    },
    ProviderInfo {
        id: "openrouter",
        display: "OpenRouter",
        default_url: "https://openrouter.ai/api/v1/chat/completions",
        default_model: "anthropic/claude-3.5-sonnet",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "groq",
        display: "Groq",
        default_url: "https://api.groq.com/openai/v1/chat/completions",
        default_model: "llama-3.3-70b-versatile",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "together",
        display: "Together",
        default_url: "https://api.together.xyz/v1/chat/completions",
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "deepseek",
        display: "DeepSeek",
        default_url: "https://api.deepseek.com/v1/chat/completions",
        default_model: "deepseek-chat",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "xai",
        display: "xAI Grok",
        default_url: "https://api.x.ai/v1/chat/completions",
        default_model: "grok-4",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "mistral",
        display: "Mistral",
        default_url: "https://api.mistral.ai/v1/chat/completions",
        default_model: "mistral-large-latest",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "perplexity",
        display: "Perplexity",
        default_url: "https://api.perplexity.ai/chat/completions",
        default_model: "sonar-pro",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "fireworks",
        display: "Fireworks",
        default_url: "https://api.fireworks.ai/inference/v1/chat/completions",
        default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "cerebras",
        display: "Cerebras",
        default_url: "https://api.cerebras.ai/v1/chat/completions",
        default_model: "llama-3.3-70b",
        kind: ProviderKind::OpenAiCompat,
    },
    ProviderInfo {
        id: "custom",
        display: "Custom",
        default_url: "",
        default_model: "",
        kind: ProviderKind::Custom,
    },
];

pub fn provider_info(id: &str) -> &'static ProviderInfo {
    PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .unwrap_or(&PROVIDERS[0])
}

pub fn provider_from_display(display: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.display == display)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMsg {
    pub role: String,     // "user", "assistant", "system"
    pub content: serde_json::Value, // string or array of content blocks
}

/// The bash execution tool definition (Anthropic format).
fn bash_tool() -> serde_json::Value {
    serde_json::json!({
        "name": "execute_command",
        "description": "Execute a bash command in the connected terminal session. The command will be typed into the terminal and executed. Returns the output.",
        "input_schema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                }
            },
            "required": ["command"]
        }
    })
}

/// Incremental events produced by `send_chat_stream`. The handler
/// accumulates `Text` deltas into the active assistant bubble and
/// dispatches `ToolUse` (which is only emitted after the model has
/// fully committed to a tool call — partial argument JSON is kept
/// internal to the parser).
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Append this slice to the current assistant message.
    Text(String),
    /// Model committed to running the bash tool with this command.
    ToolUse { command: String },
    /// Stream completed cleanly. No more chunks will follow.
    Done,
    /// Provider/network error. User-facing message; stream stops here.
    Error(String),
}

/// Streaming variant of `send_chat`. Returns immediately with a stream
/// the caller can poll; chunks fire as the provider emits them. The
/// stream always ends with either `Done` (success) or `Error` (failure).
pub fn send_chat_stream(
    config: AiConfig,
    messages: Vec<ChatMsg>,
) -> UnboundedReceiverStream<StreamChunk> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let info = provider_info(&config.provider);
        let result = match info.kind {
            ProviderKind::Anthropic => stream_anthropic(&config, &messages, &tx).await,
            ProviderKind::Gemini => stream_gemini(&config, &messages, &tx).await,
            ProviderKind::OpenAiCompat => {
                let url = config
                    .api_url
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(info.default_url)
                    .to_string();
                stream_openai_at(&url, &config, &messages, &tx).await
            }
            ProviderKind::Custom => {
                let url = config.api_url.as_deref().unwrap_or("");
                if url.is_empty() {
                    let _ = tx.send(StreamChunk::Error(
                        "Custom provider requires an API URL".into(),
                    ));
                    return;
                }
                stream_openai_at(url, &config, &messages, &tx).await
            }
        };
        let _ = match result {
            Ok(()) => tx.send(StreamChunk::Done),
            Err(e) => tx.send(StreamChunk::Error(e)),
        };
    });
    UnboundedReceiverStream::new(rx)
}

/// SSE line iterator over a reqwest byte stream. Buffers chunks until a
/// blank line (event boundary), yielding the assembled `data:` payload
/// (concatenated if the event spanned multiple `data:` lines). Discards
/// `event:` / `id:` / comment lines — providers we hit don't put load-
/// bearing info there.
async fn for_each_sse_event<S, B, E, F>(
    mut byte_stream: S,
    mut on_event: F,
) -> Result<(), String>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
    F: FnMut(&str) -> Result<bool, String>,
{
    let mut buf = String::new();
    let mut data_acc = String::new();
    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream read failed: {e}"))?;
        let s = std::str::from_utf8(chunk.as_ref()).map_err(|e| format!("utf8: {e}"))?;
        buf.push_str(s);
        // SSE separates events with a blank line ("\n\n"). Process all
        // complete events in the buffer; keep the trailing partial line
        // for the next chunk.
        while let Some(boundary) = buf.find("\n\n") {
            let event = buf[..boundary].to_string();
            buf.drain(..boundary + 2);
            data_acc.clear();
            for line in event.lines() {
                if let Some(payload) = line.strip_prefix("data:") {
                    if !data_acc.is_empty() {
                        data_acc.push('\n');
                    }
                    data_acc.push_str(payload.trim_start());
                }
            }
            if data_acc.is_empty() {
                continue;
            }
            if on_event(&data_acc)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn stream_anthropic(
    config: &AiConfig,
    messages: &[ChatMsg],
    tx: &mpsc::UnboundedSender<StreamChunk>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let system_prompt = config
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let body = serde_json::json!({
        "model": config.model,
        "max_tokens": 4096,
        "system": system_prompt,
        "tools": [bash_tool()],
        "messages": messages,
        "stream": true,
    });
    let url = config
        .api_url
        .as_deref()
        .unwrap_or("https://api.anthropic.com/v1/messages");
    let resp = client
        .post(url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    // Per-block scratch: Anthropic streams tool_use input as a series
    // of `input_json_delta` partial-json fragments under the same
    // content-block index, so we accumulate them and parse on
    // `content_block_stop`.
    let mut tool_partial: std::collections::HashMap<u64, String> =
        std::collections::HashMap::new();
    let mut tool_emitted = false;

    let stream = resp.bytes_stream();
    for_each_sse_event(stream, |data| {
        if data == "[DONE]" {
            return Ok(true);
        }
        let v: serde_json::Value = serde_json::from_str(data)
            .map_err(|e| format!("anthropic SSE parse: {e}"))?;
        match v["type"].as_str().unwrap_or("") {
            "content_block_delta" => {
                let delta = &v["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        if let Some(t) = delta["text"].as_str() {
                            let _ = tx.send(StreamChunk::Text(t.to_string()));
                        }
                    }
                    "input_json_delta" => {
                        if let Some(idx) = v["index"].as_u64()
                            && let Some(part) = delta["partial_json"].as_str()
                        {
                            tool_partial.entry(idx).or_default().push_str(part);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(idx) = v["index"].as_u64()
                    && let Some(json) = tool_partial.remove(&idx)
                    && !json.is_empty()
                {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or_default();
                    if let Some(cmd) = parsed["command"].as_str()
                        && !tool_emitted
                    {
                        let _ = tx.send(StreamChunk::ToolUse {
                            command: cmd.to_string(),
                        });
                        tool_emitted = true;
                    }
                }
            }
            "message_stop" => return Ok(true),
            _ => {}
        }
        Ok(false)
    })
    .await
}

async fn stream_openai_at(
    url: &str,
    config: &AiConfig,
    messages: &[ChatMsg],
    tx: &mpsc::UnboundedSender<StreamChunk>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let system_prompt = config
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let openai_messages: Vec<serde_json::Value> = std::iter::once(serde_json::json!({
        "role": "system",
        "content": system_prompt
    }))
    .chain(messages.iter().map(|m| {
        serde_json::json!({
            "role": m.role,
            "content": m.content,
        })
    }))
    .collect();
    let tools = serde_json::json!([{
        "type": "function",
        "function": {
            "name": "execute_command",
            "description": "Execute a bash command in the connected terminal session.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    }
                },
                "required": ["command"]
            }
        }
    }]);
    let body = serde_json::json!({
        "model": config.model,
        "max_tokens": 4096,
        "messages": openai_messages,
        "tools": tools,
        "stream": true,
    });
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {text}"));
    }

    // OpenAI streams tool_calls' arguments as JSON partials under the
    // same `index`, like Anthropic. Buffer per index and parse on
    // finish_reason="tool_calls" / "stop".
    let mut tool_partial: std::collections::HashMap<u64, String> =
        std::collections::HashMap::new();
    let mut tool_emitted = false;

    let stream = resp.bytes_stream();
    for_each_sse_event(stream, |data| {
        if data == "[DONE]" {
            return Ok(true);
        }
        let v: serde_json::Value = serde_json::from_str(data)
            .map_err(|e| format!("openai SSE parse: {e}"))?;
        let Some(choice) = v["choices"].as_array().and_then(|a| a.first()) else {
            return Ok(false);
        };
        let delta = &choice["delta"];
        if let Some(content) = delta["content"].as_str()
            && !content.is_empty()
        {
            let _ = tx.send(StreamChunk::Text(content.to_string()));
        }
        if let Some(tcs) = delta["tool_calls"].as_array() {
            for tc in tcs {
                let idx = tc["index"].as_u64().unwrap_or(0);
                if let Some(args) = tc["function"]["arguments"].as_str() {
                    tool_partial.entry(idx).or_default().push_str(args);
                }
            }
        }
        let finish = choice["finish_reason"].as_str().unwrap_or("");
        if !finish.is_empty() {
            // Drain whatever tool args we accumulated.
            if !tool_emitted {
                for json in tool_partial.values() {
                    if json.is_empty() {
                        continue;
                    }
                    let parsed: serde_json::Value =
                        serde_json::from_str(json).unwrap_or_default();
                    if let Some(cmd) = parsed["command"].as_str() {
                        let _ = tx.send(StreamChunk::ToolUse {
                            command: cmd.to_string(),
                        });
                        tool_emitted = true;
                        break;
                    }
                }
            }
            return Ok(true);
        }
        Ok(false)
    })
    .await
}

async fn stream_gemini(
    config: &AiConfig,
    messages: &[ChatMsg],
    tx: &mpsc::UnboundedSender<StreamChunk>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let gemini_contents: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            let role = if m.role == "assistant" { "model" } else { "user" };
            serde_json::json!({
                "role": role,
                "parts": [{ "text": m.content }]
            })
        })
        .collect();
    let system_prompt = config
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let body = serde_json::json!({
        "contents": gemini_contents,
        "systemInstruction": { "parts": [{ "text": system_prompt }] },
        "tools": [{
            "functionDeclarations": [{
                "name": "execute_command",
                "description": "Execute a bash command in the connected terminal session.",
                "parameters": {
                    "type": "OBJECT",
                    "properties": {
                        "command": { "type": "STRING", "description": "The bash command to execute" }
                    },
                    "required": ["command"]
                }
            }]
        }]
    });

    // The streaming endpoint mirrors generateContent but ends in
    // streamGenerateContent and accepts `alt=sse` for text/event-stream
    // framing (the default returns a JSON array which is harder to
    // incrementally parse).
    let url = match config.api_url.as_deref() {
        Some(u) if !u.is_empty() => format!("{u}?alt=sse&key={}", config.api_key),
        _ => format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            config.model, config.api_key
        ),
    };

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini API error {status}: {text}"));
    }

    let mut tool_emitted = false;
    let stream = resp.bytes_stream();
    for_each_sse_event(stream, |data| {
        let v: serde_json::Value = serde_json::from_str(data)
            .map_err(|e| format!("gemini SSE parse: {e}"))?;
        let Some(parts) = v["candidates"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["content"]["parts"].as_array())
        else {
            return Ok(false);
        };
        for part in parts {
            if let Some(t) = part["text"].as_str()
                && !t.is_empty()
            {
                let _ = tx.send(StreamChunk::Text(t.to_string()));
            }
            if let Some(fc) = part.get("functionCall")
                && !tool_emitted
                && let Some(cmd) = fc["args"]["command"].as_str()
            {
                let _ = tx.send(StreamChunk::ToolUse {
                    command: cmd.to_string(),
                });
                tool_emitted = true;
            }
        }
        Ok(false)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    /// Wrap a fixed list of byte slices into the kind of stream
    /// `for_each_sse_event` accepts (a real one comes from
    /// `reqwest::Response::bytes_stream`).
    fn fake_byte_stream(
        chunks: Vec<&'static [u8]>,
    ) -> impl futures_util::Stream<Item = Result<&'static [u8], std::io::Error>> {
        stream::iter(chunks.into_iter().map(Ok))
    }

    #[tokio::test]
    async fn sse_parser_assembles_events_split_across_chunks() {
        // Real SSE servers chop events at TCP boundaries, so the
        // parser MUST handle a header that arrives in two pieces.
        let s = fake_byte_stream(vec![b"data: hel", b"lo\n\ndata: world\n\n"]);
        let mut seen = Vec::new();
        for_each_sse_event(s, |data| {
            seen.push(data.to_string());
            Ok(false)
        })
        .await
        .unwrap();
        assert_eq!(seen, vec!["hello", "world"]);
    }

    #[tokio::test]
    async fn sse_parser_concatenates_multi_data_lines_per_event() {
        // SSE allows multiple `data:` lines in one event (joined by \n).
        let s = fake_byte_stream(vec![b"data: line1\ndata: line2\n\n"]);
        let mut seen = Vec::new();
        for_each_sse_event(s, |data| {
            seen.push(data.to_string());
            Ok(false)
        })
        .await
        .unwrap();
        assert_eq!(seen, vec!["line1\nline2"]);
    }

    #[tokio::test]
    async fn sse_parser_skips_event_lines_and_comments() {
        // `event:` and comment (`:foo`) lines are valid SSE noise we
        // ignore — only `data:` lines feed the callback.
        let s = fake_byte_stream(vec![
            b"event: ping\n:keepalive\ndata: payload\n\n",
        ]);
        let mut seen = Vec::new();
        for_each_sse_event(s, |data| {
            seen.push(data.to_string());
            Ok(false)
        })
        .await
        .unwrap();
        assert_eq!(seen, vec!["payload"]);
    }

    #[tokio::test]
    async fn sse_parser_stops_when_callback_returns_done() {
        // Callback returning `Ok(true)` is the "stream finished" signal
        // the provider parsers use on `[DONE]` / `message_stop`.
        let s = fake_byte_stream(vec![b"data: a\n\ndata: stop\n\ndata: c\n\n"]);
        let mut seen = Vec::new();
        for_each_sse_event(s, |data| {
            seen.push(data.to_string());
            Ok(data == "stop")
        })
        .await
        .unwrap();
        // "c" must NOT show up — we returned true on "stop".
        assert_eq!(seen, vec!["a".to_string(), "stop".to_string()]);
    }

    #[tokio::test]
    async fn sse_parser_propagates_callback_error() {
        let s = fake_byte_stream(vec![b"data: bad\n\n"]);
        let result = for_each_sse_event(s, |_data| Err("nope".into())).await;
        assert!(result.is_err());
    }
}
