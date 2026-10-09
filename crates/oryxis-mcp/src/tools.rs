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
            "name": "create_host",
            "description": "Create a new SSH host in the Oryxis vault. Requires \"Allow the MCP server to add and edit hosts\" in Oryxis Settings > MCP Server. A folder (`group`) must already exist (path like \"Prod / Web\" or a label); keys, identities and jump hops are named by label or id. The password is stored encrypted and never returned. Proxies cannot be set here.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "label": { "type": "string", "description": "Display name" },
                    "hostname": { "type": "string", "description": "Hostname or IP" },
                    "port": { "type": "integer", "description": "SSH port (default 22)" },
                    "username": { "type": "string", "description": "Login user; omit to inherit from the folder or identity" },
                    "auth_method": { "type": "string", "description": "auto | password | key | agent | interactive | password_prompt (default auto)" },
                    "password": { "type": "string", "description": "Password to store encrypted" },
                    "key": { "type": "string", "description": "Vault SSH key, by label or id" },
                    "identity": { "type": "string", "description": "Saved identity, by label or id" },
                    "group": { "type": "string", "description": "Existing folder, by path or label" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "notes": { "type": "string" },
                    "jump_chain": { "type": "array", "items": { "type": "string" }, "description": "Bastions to hop through, by label or id, in order" },
                    "mcp_enabled": { "type": "boolean", "description": "Expose the host to MCP (default true)" }
                },
                "required": ["label", "hostname"]
            }
        }),
        json!({
            "name": "update_host",
            "description": "Change fields of an existing MCP-enabled SSH host. Only the fields given change; pass null to clear an optional field (username, group, key, identity, notes, password). Same rules and the same setting as create_host.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Connection UUID" },
                    "label": { "type": "string" },
                    "hostname": { "type": "string" },
                    "port": { "type": "integer" },
                    "username": { "type": ["string", "null"] },
                    "auth_method": { "type": "string" },
                    "password": { "type": ["string", "null"], "description": "New password, or null to clear the stored one" },
                    "key": { "type": ["string", "null"] },
                    "identity": { "type": ["string", "null"] },
                    "group": { "type": ["string", "null"], "description": "Existing folder by path or label; null moves the host to the top level" },
                    "tags": { "type": ["array", "null"], "items": { "type": "string" } },
                    "notes": { "type": ["string", "null"] },
                    "jump_chain": { "type": ["array", "null"], "items": { "type": "string" } },
                    "mcp_enabled": { "type": "boolean" }
                },
                "required": ["id"]
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
