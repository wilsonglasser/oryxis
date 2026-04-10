use serde_json::{json, Value};

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_hosts",
            "description": "List all MCP-enabled SSH hosts in the vault",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group_id": {
                        "type": "string",
                        "description": "Optional group UUID to filter by"
                    },
                    "tag": {
                        "type": "string",
                        "description": "Optional tag to filter by"
                    }
                }
            }
        }),
        json!({
            "name": "get_host",
            "description": "Get detailed information about a specific SSH host",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Connection UUID"
                    }
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "ssh_execute",
            "description": "Execute a command on a remote SSH host and return the output",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Connection UUID"
                    },
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 30, max: 300)"
                    }
                },
                "required": ["id", "command"]
            }
        }),
        json!({
            "name": "list_groups",
            "description": "List all host groups in the vault",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "list_keys",
            "description": "List SSH keys stored in the vault (metadata only, no private key material)",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ]
}
