#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use tempfile::NamedTempFile;

    use oryxis_core::models::connection::Connection;
    use oryxis_core::models::group::Group;
    use oryxis_core::models::key::{KeyAlgorithm, SshKey};
    use oryxis_vault::VaultStore;

    use crate::server::handle_request;
    use crate::tools::tool_definitions;

    fn test_vault() -> VaultStore {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        std::mem::forget(tmp);
        let mut vault = VaultStore::open(&path).unwrap();
        vault.set_master_password("test").unwrap();
        let _ = vault.set_setting("mcp_server_enabled", "true");
        vault
    }

    #[test]
    fn tool_definitions_has_five_tools() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"list_hosts"));
        assert!(names.contains(&"get_host"));
        assert!(names.contains(&"ssh_execute"));
        assert!(names.contains(&"list_groups"));
        assert!(names.contains(&"list_keys"));
    }

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let vault = test_vault();
        let resp = handle_request("initialize", json!(1), None, &vault).await;
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "oryxis-mcp");
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn tools_list_returns_all_tools() {
        let vault = test_vault();
        let resp = handle_request("tools/list", json!(2), None, &vault).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
    }

    #[tokio::test]
    async fn list_hosts_empty_vault() {
        let vault = test_vault();
        let resp = handle_request(
            "tools/call",
            json!(3),
            Some(&json!({"name": "list_hosts", "arguments": {}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert!(hosts.is_empty());
    }

    #[tokio::test]
    async fn list_hosts_returns_mcp_enabled_only() {
        let vault = test_vault();

        let mut c1 = Connection::new("enabled-host", "10.0.0.1");
        c1.mcp_enabled = true;
        vault.save_connection(&c1, None).unwrap();

        let mut c2 = Connection::new("disabled-host", "10.0.0.2");
        c2.mcp_enabled = false;
        vault.save_connection(&c2, None).unwrap();

        let resp = handle_request(
            "tools/call",
            json!(4),
            Some(&json!({"name": "list_hosts", "arguments": {}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0]["label"], "enabled-host");
    }

    #[tokio::test]
    async fn get_host_returns_details() {
        let vault = test_vault();
        let conn = Connection::new("my-server", "192.168.1.100");
        vault.save_connection(&conn, None).unwrap();

        let resp = handle_request(
            "tools/call",
            json!(5),
            Some(&json!({"name": "get_host", "arguments": {"id": conn.id.to_string()}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let host: Value = serde_json::from_str(text).unwrap();
        assert_eq!(host["label"], "my-server");
        assert_eq!(host["hostname"], "192.168.1.100");
        assert_eq!(host["port"], 22);
    }

    #[tokio::test]
    async fn get_host_not_found() {
        let vault = test_vault();
        let resp = handle_request(
            "tools/call",
            json!(6),
            Some(&json!({"name": "get_host", "arguments": {"id": "00000000-0000-0000-0000-000000000000"}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn list_groups_works() {
        let vault = test_vault();
        let g = Group::new("Production");
        vault.save_group(&g).unwrap();

        let resp = handle_request(
            "tools/call",
            json!(7),
            Some(&json!({"name": "list_groups", "arguments": {}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let groups: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["label"], "Production");
    }

    #[tokio::test]
    async fn list_keys_no_private_data() {
        let vault = test_vault();
        let key = SshKey::new("my-key", KeyAlgorithm::Ed25519);
        vault.save_key(&key, Some("PRIVATE_KEY_DATA")).unwrap();

        let resp = handle_request(
            "tools/call",
            json!(8),
            Some(&json!({"name": "list_keys", "arguments": {}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        // Verify no private key data leaked
        assert!(!text.contains("PRIVATE_KEY_DATA"));
        let keys: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["label"], "my-key");
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let vault = test_vault();
        let resp = handle_request("nonexistent/method", json!(9), None, &vault).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let vault = test_vault();
        let resp = handle_request(
            "tools/call",
            json!(10),
            Some(&json!({"name": "nonexistent_tool", "arguments": {}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn mcp_disabled_rejects_calls() {
        let vault = test_vault();
        let _ = vault.set_setting("mcp_server_enabled", "false");

        let resp = handle_request(
            "tools/call",
            json!(11),
            Some(&json!({"name": "list_hosts", "arguments": {}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap_or(false));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("disabled"));
    }

    #[tokio::test]
    async fn list_hosts_filter_by_tag() {
        let vault = test_vault();

        let mut c1 = Connection::new("web", "10.0.0.1");
        c1.tags = vec!["production".into()];
        vault.save_connection(&c1, None).unwrap();

        let mut c2 = Connection::new("db", "10.0.0.2");
        c2.tags = vec!["staging".into()];
        vault.save_connection(&c2, None).unwrap();

        let resp = handle_request(
            "tools/call",
            json!(12),
            Some(&json!({"name": "list_hosts", "arguments": {"tag": "production"}})),
            &vault,
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0]["label"], "web");
    }
}
