use serde_json::{json, Value};

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_hosts",
            "description": "List all MCP-enabled SSH hosts in the vault. `username` is the host's own field (null when it leaves the user to its group or identity); `effective_username` is the login ssh_execute will use.",
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
            "description": "Get detailed information about a specific SSH host. `username` is the host's own field (null when inherited); `effective_username` is the login ssh_execute will use.",
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
                        "description": "Timeout in seconds for the command (default: 30, max: 180)"
                    }
                },
                "required": ["id", "command"]
            }
        }),
        json!({
            "name": "accept_host_key",
            "description": "Pin a server host key that a previous ssh_execute refused as not yet trusted. Pass exactly the host, port, key_type and fingerprint that refusal reported; only a key this server saw refused as unknown is accepted, and a key that CHANGED against the vault's pin is never accepted here (a person reviews it in Oryxis > Known Hosts). Ask the user to confirm the fingerprint against what the server's administrator published before calling.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "Hostname or IP exactly as the refusal reported it" },
                    "port": { "type": "integer", "description": "SSH port the refusal reported" },
                    "key_type": { "type": "string", "description": "Key algorithm, e.g. ssh-ed25519" },
                    "fingerprint": { "type": "string", "description": "SHA256:... fingerprint the refusal reported" }
                },
                "required": ["host", "port", "key_type", "fingerprint"]
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
