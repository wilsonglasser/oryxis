use serde::{Deserialize, Serialize};

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful terminal assistant embedded in Oryxis SSH client. You can execute bash commands in the user's active SSH session using the execute_command tool. Be concise and practical. When the user asks you to do something on the server, use the tool. You also receive the last lines of terminal output for context.";

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub provider: String,   // "anthropic", "openai", "gemini", "custom"
    pub model: String,
    pub api_key: String,
    pub api_url: Option<String>,
    pub system_prompt: Option<String>, // additional system instructions
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

/// Send a chat request to the AI provider and return the response.
pub async fn send_chat(
    config: &AiConfig,
    messages: &[ChatMsg],
) -> Result<AiResponse, String> {
    match config.provider.as_str() {
        "anthropic" => send_anthropic(config, messages).await,
        "openai" => send_openai(config, messages).await,
        "gemini" => send_gemini(config, messages).await,
        "custom" => send_openai(config, messages).await, // custom uses OpenAI-compatible API
        _ => Err(format!("Unknown provider: {}", config.provider)),
    }
}

#[derive(Debug, Clone)]
pub enum AiResponse {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        command: String,
    },
}

async fn send_anthropic(
    config: &AiConfig,
    messages: &[ChatMsg],
) -> Result<AiResponse, String> {
    let client = reqwest::Client::new();

    let system_prompt = config.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT);

    let body = serde_json::json!({
        "model": config.model,
        "max_tokens": 4096,
        "system": system_prompt,
        "tools": [bash_tool()],
        "messages": messages,
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
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, text));
    }

    let json: serde_json::Value =
        resp.json().await.map_err(|e| format!("JSON parse: {}", e))?;

    // Parse Anthropic response — look for tool_use first, then text
    if let Some(content) = json["content"].as_array() {
        let mut text_parts = Vec::new();
        for block in content {
            if block["type"] == "tool_use" {
                let id = block["id"].as_str().unwrap_or("").to_string();
                let name = block["name"].as_str().unwrap_or("").to_string();
                let command =
                    block["input"]["command"].as_str().unwrap_or("").to_string();
                return Ok(AiResponse::ToolUse { id, name, command });
            }
            if block["type"] == "text" {
                if let Some(text) = block["text"].as_str() {
                    text_parts.push(text.to_string());
                }
            }
        }
        if !text_parts.is_empty() {
            return Ok(AiResponse::Text(text_parts.join("\n")));
        }
    }

    Err("Empty response from Anthropic API".into())
}

async fn send_openai(
    config: &AiConfig,
    messages: &[ChatMsg],
) -> Result<AiResponse, String> {
    let client = reqwest::Client::new();

    // Convert to OpenAI format with system message prepended
    let system_prompt = config.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT);

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
    });

    let url = config
        .api_url
        .as_deref()
        .unwrap_or("https://api.openai.com/v1/chat/completions");

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, text));
    }

    let json: serde_json::Value =
        resp.json().await.map_err(|e| format!("JSON parse: {}", e))?;

    // Parse OpenAI response
    if let Some(choices) = json["choices"].as_array() {
        if let Some(choice) = choices.first() {
            let message = &choice["message"];

            // Check for tool calls
            if let Some(tool_calls) = message["tool_calls"].as_array() {
                if let Some(tc) = tool_calls.first() {
                    let id = tc["id"].as_str().unwrap_or("").to_string();
                    let name =
                        tc["function"]["name"].as_str().unwrap_or("").to_string();
                    let args: serde_json::Value = serde_json::from_str(
                        tc["function"]["arguments"].as_str().unwrap_or("{}"),
                    )
                    .unwrap_or_default();
                    let command =
                        args["command"].as_str().unwrap_or("").to_string();
                    return Ok(AiResponse::ToolUse { id, name, command });
                }
            }

            // Text response
            if let Some(content) = message["content"].as_str() {
                return Ok(AiResponse::Text(content.to_string()));
            }
        }
    }

    Err("Empty response from OpenAI API".into())
}

async fn send_gemini(
    config: &AiConfig,
    messages: &[ChatMsg],
) -> Result<AiResponse, String> {
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

    let system_prompt = config.system_prompt.as_deref().unwrap_or(DEFAULT_SYSTEM_PROMPT);

    let body = serde_json::json!({
        "contents": gemini_contents,
        "systemInstruction": {
            "parts": [{ "text": system_prompt }]
        },
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

    let url = match config.api_url.as_deref() {
        Some(u) if !u.is_empty() => format!("{}?key={}", u, config.api_key),
        _ => format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            config.model, config.api_key
        ),
    };

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini API error {}: {}", status, text));
    }

    let json: serde_json::Value =
        resp.json().await.map_err(|e| format!("JSON parse: {}", e))?;

    if let Some(candidates) = json["candidates"].as_array() {
        if let Some(candidate) = candidates.first() {
            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for part in parts {
                    if let Some(fc) = part.get("functionCall") {
                        let name = fc["name"].as_str().unwrap_or("").to_string();
                        let command = fc["args"]["command"].as_str().unwrap_or("").to_string();
                        return Ok(AiResponse::ToolUse { id: String::new(), name, command });
                    }
                    if let Some(text) = part["text"].as_str() {
                        return Ok(AiResponse::Text(text.to_string()));
                    }
                }
            }
        }
    }

    Err("Empty response from Gemini API".into())
}
