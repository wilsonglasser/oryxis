use serde_json::{json, Value};

use oryxis_vault::VaultStore;

use crate::handlers;
use crate::protocol::JsonRpcResponse;
use crate::tools;

pub async fn handle_request(
    method: &str,
    id: Value,
    params: Option<&Value>,
    vault: &VaultStore,
) -> JsonRpcResponse {
    match method {
        "initialize" => JsonRpcResponse::success(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "oryxis-mcp",
                    "version": "0.1.0"
                }
            }),
        ),

        "tools/list" => {
            let tools = tools::tool_definitions();
            JsonRpcResponse::success(
                id,
                json!({ "tools": tools }),
            )
        }

        "tools/call" => {
            let tool_name = params
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = params.and_then(|p| p.get("arguments"));

            // Check if MCP is enabled
            if let Ok(Some(v)) = vault.get_setting("mcp_server_enabled") {
                if v != "true" {
                    return JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": "MCP server is disabled. Enable it in Oryxis Settings > Security."
                            }],
                            "isError": true
                        }),
                    );
                }
            } else {
                // Setting not found — MCP not enabled by default
                return JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": "MCP server is disabled. Enable it in Oryxis Settings > Security."
                        }],
                        "isError": true
                    }),
                );
            }

            let result = match tool_name {
                "list_hosts" => handlers::handle_list_hosts(vault, arguments),
                "get_host" => handlers::handle_get_host(vault, arguments),
                "list_groups" => Ok(handlers::handle_list_groups(vault).unwrap_or(json!([]))),
                "list_keys" => Ok(handlers::handle_list_keys(vault).unwrap_or(json!([]))),
                "ssh_execute" => handlers::handle_ssh_execute(vault, arguments).await,
                _ => Err(format!("Unknown tool: {}", tool_name)),
            };

            match result {
                Ok(data) => {
                    let text = serde_json::to_string_pretty(&data).unwrap_or_default();
                    JsonRpcResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": text
                            }]
                        }),
                    )
                }
                Err(e) => JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": e
                        }],
                        "isError": true
                    }),
                ),
            }
        }

        _ => JsonRpcResponse::error(id, -32601, format!("Method not found: {}", method)),
    }
}
