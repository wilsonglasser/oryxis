#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use serde_json::{json, Value};
    use tempfile::NamedTempFile;

    use oryxis_core::models::connection::Connection;
    use oryxis_core::models::group::Group;
    use oryxis_core::models::key::{KeyAlgorithm, SshKey};
    use oryxis_vault::VaultStore;

    use crate::handlers::{dial_signature, resolve_dial_plan, reuse_signature};
    use crate::server::Server;
    use crate::stdio;
    use crate::tools::tool_definitions;

    /// A cancel handle nobody will ever pull.
    fn no_cancel() -> tokio::sync::watch::Receiver<bool> {
        let (tx, rx) = tokio::sync::watch::channel(false);
        std::mem::forget(tx);
        rx
    }

    fn test_vault() -> VaultStore {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        std::mem::forget(tmp);
        let mut vault = VaultStore::open(&path).unwrap();
        vault.set_master_password("test").unwrap();
        let _ = vault.set_setting("mcp_server_enabled", "true");
        vault
    }

    /// A host behind a bastion that sits behind another bastion dials
    /// the whole route over MCP, the way a tab does: the chain is
    /// expanded (issue #184), and every hop carries its own credentials,
    /// not the target's. A bastion need not be exposed to MCP itself.
    #[tokio::test]
    async fn a_jump_route_is_resolved_with_each_hops_credentials() {
        let vault = test_vault();
        let outer = Connection::new("outer", "outer.example");
        let mut inner = Connection::new("inner", "inner.example");
        inner.jump_chain = vec![outer.id];
        let mut target = Connection::new("target", "target.example");
        target.jump_chain = vec![inner.id];
        target.mcp_enabled = true;
        vault.save_connection(&outer, Some("outer-pw")).unwrap();
        vault.save_connection(&inner, Some("inner-pw")).unwrap();
        vault.save_connection(&target, Some("target-pw")).unwrap();
        vault.set_connection_totp_secret(&target.id, Some("JBSWY3DPEHPK3PXP")).unwrap();
        vault.set_connection_totp_secret(&inner.id, Some("GEZDGNBVGY3TQOJQ")).unwrap();

        let Ok(plan) = resolve_dial_plan(&vault, target.id, Default::default()) else {
            panic!("the target is exposed to MCP and must resolve");
        };
        assert_eq!(plan.auth_conn.jump_chain, vec![outer.id, inner.id]);
        assert_eq!(plan.password.as_deref(), Some("target-pw"));
        let resolver = plan.resolver.expect("a routed host carries a resolver");
        let hops: Vec<uuid::Uuid> = resolver.connections.iter().map(|c| c.id).collect();
        assert_eq!(hops, vec![outer.id, inner.id]);
        assert_eq!(resolver.passwords.get(&outer.id).map(String::as_str), Some("outer-pw"));
        assert_eq!(resolver.passwords.get(&inner.id).map(String::as_str), Some("inner-pw"));
        // Each hop carries its own second factor, and only its own: the
        // target's secret never lands on a bastion.
        assert_eq!(
            resolver.totp_secrets.get(&inner.id).map(String::as_str),
            Some("GEZDGNBVGY3TQOJQ")
        );
        assert!(!resolver.totp_secrets.contains_key(&outer.id));
        assert!(!resolver.totp_secrets.values().any(|s| s == "JBSWY3DPEHPK3PXP"));

        // A direct host needs no resolver at all.
        let mut direct = Connection::new("direct", "direct.example");
        direct.mcp_enabled = true;
        vault.save_connection(&direct, None).unwrap();
        let Ok(plan) = resolve_dial_plan(&vault, direct.id, Default::default()) else {
            panic!("the direct host must resolve");
        };
        assert!(plan.resolver.is_none());
    }

    #[test]
    fn tool_definitions_lists_every_tool() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 8);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"accept_host_key"));
        assert!(names.contains(&"create_host"));
        assert!(names.contains(&"update_host"));
        assert!(names.contains(&"list_hosts"));
        assert!(names.contains(&"get_host"));
        assert!(names.contains(&"ssh_execute"));
        assert!(names.contains(&"list_groups"));
        assert!(names.contains(&"list_keys"));
    }

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let server = Server::new(test_vault());
        let resp = server.handle_request("initialize", json!(1), None, no_cancel()).await;
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "oryxis-mcp");
        assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
        // No requested version in params: answer with the latest supported.
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn initialize_echoes_supported_requested_version() {
        let server = Server::new(test_vault());
        for requested in ["2024-11-05", "2025-03-26", "2025-06-18"] {
            let params = json!({
                "protocolVersion": requested,
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            });
            let resp = server.handle_request("initialize", json!(1), Some(&params), no_cancel()).await;
            let result = resp.result.unwrap();
            assert_eq!(result["protocolVersion"], requested);
        }
    }

    #[tokio::test]
    async fn initialize_unknown_version_falls_back_to_latest() {
        let server = Server::new(test_vault());
        let params = json!({
            "protocolVersion": "2099-01-01",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1.0"}
        });
        let resp = server.handle_request("initialize", json!(1), Some(&params), no_cancel()).await;
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2025-06-18");
    }

    #[tokio::test]
    async fn ping_returns_empty_result() {
        let server = Server::new(test_vault());
        let resp = server.handle_request("ping", json!(11), None, no_cancel()).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn tools_list_returns_all_tools() {
        let server = Server::new(test_vault());
        let resp = server.handle_request("tools/list", json!(2), None, no_cancel()).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), tool_definitions().len());
    }

    #[tokio::test]
    async fn list_hosts_empty_vault() {
        let server = Server::new(test_vault());
        let resp = server.handle_request(
            "tools/call",
            json!(3),
            Some(&json!({"name": "list_hosts", "arguments": {}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert!(hosts.is_empty());
    }

    #[tokio::test]
    async fn list_hosts_returns_mcp_enabled_only() {
        let server = Server::new(test_vault());

        let mut c1 = Connection::new("enabled-host", "10.0.0.1");
        c1.mcp_enabled = true;
        server.vault().save_connection(&c1, None).unwrap();

        let mut c2 = Connection::new("disabled-host", "10.0.0.2");
        c2.mcp_enabled = false;
        server.vault().save_connection(&c2, None).unwrap();

        let resp = server.handle_request(
            "tools/call",
            json!(4),
            Some(&json!({"name": "list_hosts", "arguments": {}})),
            no_cancel(),
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
        let server = Server::new(test_vault());
        let conn = Connection::new("my-server", "192.168.1.100");
        server.vault().save_connection(&conn, None).unwrap();

        let resp = server.handle_request(
            "tools/call",
            json!(5),
            Some(&json!({"name": "get_host", "arguments": {"id": conn.id.to_string()}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let host: Value = serde_json::from_str(text).unwrap();
        assert_eq!(host["label"], "my-server");
        assert_eq!(host["hostname"], "192.168.1.100");
        assert_eq!(host["port"], 22);
    }

    /// A host that leaves its user to its folder lists the login the
    /// dial will use beside the (empty) field, and the two tools and the
    /// dial plan give one answer.
    #[tokio::test]
    async fn the_listing_names_the_login_the_dial_uses() {
        let server = Server::new(test_vault());
        let mut group = Group::new("prod");
        group.defaults = Some(oryxis_core::models::group::GroupDefaults {
            username: Some("deploy".into()),
            ..Default::default()
        });
        server.vault().save_group(&group).unwrap();
        let mut inherits = Connection::new("inherits", "10.0.0.1");
        inherits.group_id = Some(group.id);
        inherits.mcp_enabled = true;
        server.vault().save_connection(&inherits, None).unwrap();
        let mut bare = Connection::new("bare", "10.0.0.2");
        bare.mcp_enabled = true;
        server.vault().save_connection(&bare, None).unwrap();

        let resp = server.handle_request(
            "tools/call",
            json!(40),
            Some(&json!({"name": "list_hosts", "arguments": {}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        let hosts: Vec<Value> =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        let by_label = |label: &str| hosts.iter().find(|h| h["label"] == label).unwrap();
        assert!(by_label("inherits")["username"].is_null());
        assert_eq!(by_label("inherits")["effective_username"], "deploy");
        // Nothing names a user: the engine's fallback, said out loud.
        assert_eq!(by_label("bare")["effective_username"], "root");

        let resp = server.handle_request(
            "tools/call",
            json!(41),
            Some(&json!({"name": "get_host", "arguments": {"id": inherits.id.to_string()}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        let host: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(host["username"].is_null());
        assert_eq!(host["effective_username"], "deploy");

        let Ok(plan) = resolve_dial_plan(&server.vault(), inherits.id, Default::default()) else {
            panic!("the host is exposed to MCP and must resolve");
        };
        assert_eq!(plan.auth_conn.username.as_deref(), Some("deploy"));
    }

    #[tokio::test]
    async fn get_host_not_found() {
        let server = Server::new(test_vault());
        let resp = server.handle_request(
            "tools/call",
            json!(6),
            Some(&json!({"name": "get_host", "arguments": {"id": "00000000-0000-0000-0000-000000000000"}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn list_groups_works() {
        let server = Server::new(test_vault());
        let g = Group::new("Production");
        server.vault().save_group(&g).unwrap();

        let resp = server.handle_request(
            "tools/call",
            json!(7),
            Some(&json!({"name": "list_groups", "arguments": {}})),
            no_cancel(),
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
        let server = Server::new(test_vault());
        let key = SshKey::new("my-key", KeyAlgorithm::Ed25519);
        server.vault().save_key(&key, Some("PRIVATE_KEY_DATA")).unwrap();

        let resp = server.handle_request(
            "tools/call",
            json!(8),
            Some(&json!({"name": "list_keys", "arguments": {}})),
            no_cancel(),
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
        let server = Server::new(test_vault());
        let resp = server.handle_request("nonexistent/method", json!(9), None, no_cancel()).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let server = Server::new(test_vault());
        let resp = server.handle_request(
            "tools/call",
            json!(10),
            Some(&json!({"name": "nonexistent_tool", "arguments": {}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn mcp_disabled_rejects_calls() {
        let server = Server::new(test_vault());
        let _ = server.vault().set_setting("mcp_server_enabled", "false");

        let resp = server.handle_request(
            "tools/call",
            json!(11),
            Some(&json!({"name": "list_hosts", "arguments": {}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap_or(false));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("disabled"));
    }

    #[tokio::test]
    async fn list_hosts_filter_by_tag() {
        let server = Server::new(test_vault());

        let mut c1 = Connection::new("web", "10.0.0.1");
        c1.tags = vec!["production".into()];
        server.vault().save_connection(&c1, None).unwrap();

        let mut c2 = Connection::new("db", "10.0.0.2");
        c2.tags = vec!["staging".into()];
        server.vault().save_connection(&c2, None).unwrap();

        let resp = server.handle_request(
            "tools/call",
            json!(12),
            Some(&json!({"name": "list_hosts", "arguments": {"tag": "production"}})),
            no_cancel(),
        )
        .await;
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0]["label"], "web");
    }

    #[test]
    fn dial_signature_ignores_recency_and_detection() {
        let mut a = Connection::new("web", "10.0.0.1");
        let before = dial_signature(&a);
        // The two narrow updates the app makes on every connect.
        a.last_used = Some(chrono::Utc::now());
        a.detected_os = Some("linux".into());
        a.updated_at = chrono::Utc::now();
        a.notes = Some("edited".into());
        assert_eq!(dial_signature(&a), before);
    }

    #[test]
    fn dial_signature_follows_the_dial_fields() {
        let base = Connection::new("web", "10.0.0.1");
        let before = dial_signature(&base);

        let mut moved = base.clone();
        moved.hostname = "10.0.0.2".into();
        assert_ne!(dial_signature(&moved), before);

        let mut reported = base.clone();
        reported.port = 2222;
        assert_ne!(dial_signature(&reported), before);

        let mut renamed = base.clone();
        renamed.username = Some("deploy".into());
        assert_ne!(dial_signature(&renamed), before);

        let mut rekeyed = base.clone();
        rekeyed.key_id = Some(uuid::Uuid::new_v4());
        assert_ne!(dial_signature(&rekeyed), before);

        let mut hopped = base.clone();
        hopped.jump_chain = vec![uuid::Uuid::new_v4()];
        assert_ne!(dial_signature(&hopped), before);
    }

    fn pin(host: &str, port: u16, key_type: &str, fingerprint: &str) -> oryxis_core::models::KnownHost {
        let now = chrono::Utc::now();
        oryxis_core::models::KnownHost {
            id: uuid::Uuid::new_v4(),
            hostname: host.into(),
            port,
            key_type: key_type.into(),
            fingerprint: fingerprint.into(),
            first_seen: now,
            last_seen: now,
            updated_at: now,
        }
    }

    /// Withdrawing trust from a route stops the pool reusing a
    /// connection dialled under it: removing or replacing the host's
    /// pin, or revoking its command proxy. An unrelated pin does not.
    #[test]
    fn reuse_signature_follows_the_trust_the_dial_needed() {
        use oryxis_core::models::connection::{proxy_command_fingerprint, ProxyConfig, ProxyType};
        use std::collections::HashSet;

        let conn = Connection::new("web", "10.0.0.1");
        let pins = vec![pin("10.0.0.1", 22, "ssh-ed25519", "SHA256:aaa")];
        let none = HashSet::new();
        let before = reuse_signature(&conn, &[], &pins, &none, None);

        let mut other = pins.clone();
        other.push(pin("10.0.0.9", 22, "ssh-ed25519", "SHA256:zzz"));
        assert_eq!(reuse_signature(&conn, &[], &other, &none, None), before);

        assert_ne!(reuse_signature(&conn, &[], &[], &none, None), before, "pin removed");
        let changed = vec![pin("10.0.0.1", 22, "ssh-ed25519", "SHA256:bbb")];
        assert_ne!(reuse_signature(&conn, &[], &changed, &none, None), before, "pin replaced");

        let line = "ssh -W %h:%p bastion";
        let mut proxied = conn.clone();
        proxied.proxy = Some(ProxyConfig {
            proxy_type: ProxyType::Command(line.into()),
            host: String::new(),
            port: 0,
            username: None,
            password: None,
        });
        let trusted: HashSet<String> = [proxy_command_fingerprint(line)].into();
        assert_ne!(
            reuse_signature(&proxied, &[], &pins, &trusted, None),
            reuse_signature(&proxied, &[], &pins, &none, None),
            "revoking the command proxy"
        );
    }

    /// With a jump route the engine dials through the FIRST hop's proxy,
    /// so revoking that hop's command line must withdraw the route too.
    #[test]
    fn reuse_signature_follows_the_first_hops_command_proxy() {
        use oryxis_core::models::connection::{proxy_command_fingerprint, ProxyConfig, ProxyType};
        use std::collections::HashSet;

        let conn = Connection::new("db", "10.0.0.1");
        let hops = [("bastion.example", 22u16)];
        let line = "ssh -W %h:%p jumpbox";
        let hop_proxy = ProxyConfig {
            proxy_type: ProxyType::Command(line.into()),
            host: String::new(),
            port: 0,
            username: None,
            password: None,
        };
        let none = HashSet::new();
        let trusted: HashSet<String> = [proxy_command_fingerprint(line)].into();
        assert_ne!(
            reuse_signature(&conn, &hops, &[], &trusted, Some(&hop_proxy)),
            reuse_signature(&conn, &hops, &[], &none, Some(&hop_proxy)),
            "revoking the first hop's command proxy"
        );
        // A route whose first hop has no command proxy is unaffected by
        // the approval list.
        assert_eq!(
            reuse_signature(&conn, &hops, &[], &trusted, None),
            reuse_signature(&conn, &hops, &[], &none, None),
        );
    }

    /// A pin withdrawn from a bastion of the jump chain withdraws the
    /// route too; a pin of a host that is not on the route does not.
    #[test]
    fn reuse_signature_follows_the_pins_of_every_hop() {
        use std::collections::HashSet;

        let conn = Connection::new("db", "10.0.0.1");
        let hops = [("bastion.example", 22u16)];
        let none = HashSet::new();
        let pins = vec![
            pin("10.0.0.1", 22, "ssh-ed25519", "SHA256:aaa"),
            pin("bastion.example", 22, "ssh-ed25519", "SHA256:bbb"),
        ];
        let before = reuse_signature(&conn, &hops, &pins, &none, None);
        let mut unrelated = pins.clone();
        unrelated.push(pin("elsewhere", 22, "ssh-ed25519", "SHA256:zzz"));
        assert_eq!(reuse_signature(&conn, &hops, &unrelated, &none, None), before);
        let bastion_gone = vec![pin("10.0.0.1", 22, "ssh-ed25519", "SHA256:aaa")];
        assert_ne!(
            reuse_signature(&conn, &hops, &bastion_gone, &none, None),
            before,
            "bastion pin removed"
        );
        let bastion_changed = vec![
            pin("10.0.0.1", 22, "ssh-ed25519", "SHA256:aaa"),
            pin("bastion.example", 22, "ssh-ed25519", "SHA256:ccc"),
        ];
        assert_ne!(
            reuse_signature(&conn, &hops, &bastion_changed, &none, None),
            before,
            "bastion pin replaced"
        );
    }

    async fn send(w: &mut tokio::io::DuplexStream, v: Value) {
        use tokio::io::AsyncWriteExt;
        w.write_all(format!("{}\n", v).as_bytes()).await.unwrap();
    }

    async fn next_json(
        lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::io::DuplexStream>>,
        within: std::time::Duration,
    ) -> Option<Value> {
        match tokio::time::timeout(within, lines.next_line()).await {
            Ok(Ok(Some(line))) => Some(serde_json::from_str(&line).unwrap()),
            Ok(_) => panic!("output closed"),
            Err(_) => None,
        }
    }

    /// The property the loop exists for: a tool call stuck in its dial
    /// leaves the pipe free. A `ping` sent behind it answers at once, a
    /// cancel for it is honoured (no response ever goes out for that
    /// id), and the loop keeps serving afterwards.
    #[tokio::test]
    async fn a_dial_in_flight_does_not_block_the_loop_and_a_cancel_silences_it() {
        use std::sync::Arc;
        use tokio::io::AsyncBufReadExt;

        let server = Server::new(test_vault());
        // Listening but never speaking: the TCP handshake completes and
        // the dial then waits on an SSH banner that never comes.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut conn = Connection::new("silent", "127.0.0.1");
        conn.port = port;
        conn.mcp_enabled = true;
        server.vault().save_connection(&conn, None).unwrap();

        let (mut client_w, server_r) = tokio::io::duplex(1 << 16);
        let (server_w, client_r) = tokio::io::duplex(1 << 16);
        let loop_task = tokio::spawn(stdio::serve(Arc::clone(&server), server_r, server_w));
        let mut lines = tokio::io::BufReader::new(client_r).lines();

        send(
            &mut client_w,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "ssh_execute", "arguments": {
                    "id": conn.id.to_string(), "command": "echo hello"
                }}
            }),
        )
        .await;
        send(&mut client_w, json!({"jsonrpc": "2.0", "id": 2, "method": "ping"})).await;

        let pong = next_json(&mut lines, std::time::Duration::from_secs(3))
            .await
            .expect("ping answered while a dial is in flight");
        assert_eq!(pong["id"], 2);
        assert_eq!(pong["result"], json!({}));

        send(
            &mut client_w,
            json!({
                "jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": {"requestId": 1, "reason": "user gave up"}
            }),
        )
        .await;
        // Nothing for the cancelled call, in either direction.
        assert!(
            next_json(&mut lines, std::time::Duration::from_secs(2)).await.is_none(),
            "a cancelled request must not be answered"
        );

        send(&mut client_w, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"})).await;
        let listed = next_json(&mut lines, std::time::Duration::from_secs(3))
            .await
            .expect("the loop keeps serving after a cancel");
        assert_eq!(listed["id"], 3);
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), tool_definitions().len());

        // A line that is not JSON gets the parse error, and nothing else
        // is disturbed by it.
        use tokio::io::AsyncWriteExt;
        client_w.write_all(b"not json\n").await.unwrap();
        let parse = next_json(&mut lines, std::time::Duration::from_secs(3))
            .await
            .expect("parse errors are answered");
        assert_eq!(parse["error"]["code"], -32700);

        drop(client_w);
        tokio::time::timeout(std::time::Duration::from_secs(5), loop_task)
            .await
            .expect("the loop ends when its input closes")
            .unwrap();
        drop(listener);
    }
    /// A client that writes and closes its end at once still reads the
    /// answers: EOF flushes the writer instead of aborting it.
    #[tokio::test]
    async fn answers_queued_at_eof_still_go_out() {
        use std::sync::Arc;
        use tokio::io::AsyncBufReadExt;

        let server = Server::new(test_vault());
        let (mut client_w, server_r) = tokio::io::duplex(1 << 16);
        let (server_w, client_r) = tokio::io::duplex(1 << 16);
        let loop_task = tokio::spawn(stdio::serve(Arc::clone(&server), server_r, server_w));
        send(&mut client_w, json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})).await;
        send(&mut client_w, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
        drop(client_w);
        tokio::time::timeout(std::time::Duration::from_secs(5), loop_task)
            .await
            .expect("the loop ends when its input closes")
            .unwrap();
        let mut lines = tokio::io::BufReader::new(client_r).lines();
        let mut ids = Vec::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let v: Value = serde_json::from_str(&line).unwrap();
            ids.push(v["id"].as_i64().unwrap());
        }
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2]);
    }

    // ── accept_host_key ──

    async fn call(server: &Server, name: &str, args: Value) -> (bool, String) {
        let resp = server
            .handle_request(
                "tools/call",
                json!(9),
                Some(&json!({"name": name, "arguments": args})),
                no_cancel(),
            )
            .await;
        let result = resp.result.unwrap();
        let is_error = result["isError"].as_bool().unwrap_or(false);
        (is_error, result["content"][0]["text"].as_str().unwrap().to_string())
    }

    /// A key the dial refused as unknown is pinned with the exact values
    /// the refusal recorded, once; the pin lands in the vault and the
    /// second call is an idempotent yes.
    #[tokio::test]
    async fn an_unknown_key_the_dial_refused_can_be_pinned_once() {
        let server = Server::new(test_vault());
        server
            .refusals
            .record("10.0.0.5", 22, "ssh-ed25519", "SHA256:abc", None);
        let args = json!({"host": "10.0.0.5", "port": 22, "key_type": "ssh-ed25519", "fingerprint": "SHA256:abc"});
        let (err, text) = call(&server, "accept_host_key", args.clone()).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["pinned"], true);
        assert_eq!(v["already_pinned"], false);
        let pins = server.vault().list_known_hosts().unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].fingerprint, "SHA256:abc");
        assert_eq!(pins[0].key_type, "ssh-ed25519");

        // The refusal is consumed, but the vault row makes a repeat a yes.
        server
            .refusals
            .record("10.0.0.5", 22, "ssh-ed25519", "SHA256:abc", None);
        let (err, text) = call(&server, "accept_host_key", args).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["already_pinned"], true);
        assert_eq!(server.vault().list_known_hosts().unwrap().len(), 1);
    }

    /// Only what a refused dial reported: a fingerprint nobody saw, or a
    /// host no dial touched, is turned away and nothing is written.
    #[tokio::test]
    async fn a_key_no_dial_reported_is_refused() {
        let server = Server::new(test_vault());
        let (err, text) = call(
            &server,
            "accept_host_key",
            json!({"host": "10.0.0.5", "port": 22, "key_type": "ssh-ed25519", "fingerprint": "SHA256:abc"}),
        )
        .await;
        assert!(err);
        assert!(text.contains("No refused dial"), "{text}");

        server
            .refusals
            .record("10.0.0.5", 22, "ssh-ed25519", "SHA256:real", None);
        let (err, text) = call(
            &server,
            "accept_host_key",
            json!({"host": "10.0.0.5", "port": 22, "key_type": "ssh-ed25519", "fingerprint": "SHA256:forged"}),
        )
        .await;
        assert!(err);
        assert!(text.contains("does not match"), "{text}");
        assert!(server.vault().list_known_hosts().unwrap().is_empty());
    }

    /// A key that CHANGED against the vault's pin is never accepted here,
    /// and the pin stays what it was.
    #[tokio::test]
    async fn a_changed_key_is_never_pinned_over_the_old_one() {
        let server = Server::new(test_vault());
        server
            .vault()
            .save_known_host(&pin("10.0.0.5", 22, "ssh-ed25519", "SHA256:old"))
            .unwrap();
        server
            .refusals
            .record("10.0.0.5", 22, "ssh-ed25519", "SHA256:new", Some("SHA256:old"));
        let (err, text) = call(
            &server,
            "accept_host_key",
            json!({"host": "10.0.0.5", "port": 22, "key_type": "ssh-ed25519", "fingerprint": "SHA256:new"}),
        )
        .await;
        assert!(err);
        assert!(text.contains("changed"), "{text}");
        let pins = server.vault().list_known_hosts().unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].fingerprint, "SHA256:old");
    }

    /// The vault is re-read at the moment of writing: a pin the app made
    /// meanwhile for a different key wins, the tool does not replace it.
    #[tokio::test]
    async fn a_pin_made_meanwhile_is_not_replaced() {
        let server = Server::new(test_vault());
        server
            .refusals
            .record("10.0.0.5", 22, "ssh-ed25519", "SHA256:abc", None);
        server
            .vault()
            .save_known_host(&pin("10.0.0.5", 22, "ssh-ed25519", "SHA256:other"))
            .unwrap();
        let (err, text) = call(
            &server,
            "accept_host_key",
            json!({"host": "10.0.0.5", "port": 22, "key_type": "ssh-ed25519", "fingerprint": "SHA256:abc"}),
        )
        .await;
        assert!(err);
        assert!(text.contains("different"), "{text}");
        assert_eq!(server.vault().list_known_hosts().unwrap()[0].fingerprint, "SHA256:other");
    }

    /// What the agent reads after a refused dial: the four values, the
    /// tool to call for an unknown key, and a plain refusal for a changed
    /// one. A dial whose endpoints have no refusal keeps the plain error.
    #[test]
    fn the_refused_dial_answer_names_the_key_and_the_door() {
        let log = crate::hostkey::RefusalLog::default();
        let target = vec![("bastion.example".to_string(), 22), ("10.0.0.5".to_string(), 2222)];
        assert!(crate::hostkey::describe_refusals(&log, &target).is_none());

        log.record("10.0.0.5", 2222, "ssh-ed25519", "SHA256:abc", None);
        let text = crate::hostkey::describe_refusals(&log, &target).unwrap();
        assert!(text.contains("10.0.0.5:2222"), "{text}");
        assert!(text.contains("SHA256:abc"));
        assert!(text.contains("accept_host_key"));
        assert!(text.contains("\"status\": \"unknown\""));
        // A refusal for a host this dial never touched is not reported.
        assert!(!text.contains("bastion.example:22"));

        log.record("bastion.example", 22, "ssh-rsa", "SHA256:new", Some("SHA256:old"));
        let text = crate::hostkey::describe_refusals(&log, &target).unwrap();
        assert!(text.contains("HOST KEY CHANGED for bastion.example:22"), "{text}");
        assert!(text.contains("SHA256:old") && text.contains("SHA256:new"));
        assert!(text.contains("\"status\": \"changed\""));
    }

    // ── create_host / update_host ──

    fn allow_writes(server: &Server) {
        server
            .vault()
            .set_setting(crate::writes::ALLOW_WRITES_SETTING, "true")
            .unwrap();
    }

    /// Off by default: with the switch off nothing is written, and the
    /// answer says where the switch is.
    #[tokio::test]
    async fn writes_are_off_until_the_user_allows_them() {
        let server = Server::new(test_vault());
        let (err, text) = call(
            &server,
            "create_host",
            json!({"label": "web", "hostname": "10.0.0.1"}),
        )
        .await;
        assert!(err);
        assert!(text.contains("Settings > MCP Server"), "{text}");
        assert!(server.vault().list_connections().unwrap().is_empty());
    }

    /// A created host lands as an SSH row with a fresh stamp, the
    /// password encrypted beside it and never in the answer, and the
    /// answer is get_host's shape.
    #[tokio::test]
    async fn create_host_writes_the_row_and_answers_like_get_host() {
        let server = Server::new(test_vault());
        allow_writes(&server);
        let folder = Group::new("Prod");
        server.vault().save_group(&folder).unwrap();
        let before = chrono::Utc::now();
        let (err, text) = call(
            &server,
            "create_host",
            json!({
                "label": "web", "hostname": "10.0.0.1", "port": 2222,
                "username": "deploy", "auth_method": "password", "password": "s3cret",
                "group": "Prod", "tags": ["web", "prod"], "notes": "made over MCP"
            }),
        )
        .await;
        assert!(!err, "{text}");
        assert!(!text.contains("s3cret"));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["label"], "web");
        assert_eq!(v["port"], 2222);
        assert_eq!(v["effective_username"], "deploy");
        assert_eq!(v["auth_method"], "Password");
        assert_eq!(v["group_id"], folder.id.to_string());
        let id = uuid::Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
        let rows = server.vault().list_connections().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].tags, vec!["web", "prod"]);
        assert!(rows[0].updated_at >= before);
        assert!(rows[0].mcp_enabled);
        assert_eq!(
            server.vault().get_connection_password(&id).unwrap().as_deref(),
            Some("s3cret")
        );
    }

    /// A folder is resolved, never created: a typo refuses the write
    /// and names what exists, and no row or folder appears.
    #[tokio::test]
    async fn a_folder_typo_mints_nothing() {
        let server = Server::new(test_vault());
        allow_writes(&server);
        server.vault().save_group(&Group::new("Prod")).unwrap();
        let (err, text) = call(
            &server,
            "create_host",
            json!({"label": "web", "hostname": "10.0.0.1", "group": "Prd"}),
        )
        .await;
        assert!(err);
        assert!(text.contains("Prod"), "{text}");
        assert!(server.vault().list_connections().unwrap().is_empty());
        assert_eq!(server.vault().list_groups().unwrap().len(), 1);
    }

    /// update_host changes only what it is given: the rest of the row,
    /// the stored password included, stays; `null` clears a field, and a
    /// null password clears the stored one.
    #[tokio::test]
    async fn update_host_touches_only_the_fields_given() {
        let server = Server::new(test_vault());
        allow_writes(&server);
        let mut host = Connection::new("web", "10.0.0.1");
        host.username = Some("deploy".into());
        host.notes = Some("keep me".into());
        host.tags = vec!["a".into()];
        server.vault().save_connection(&host, Some("pw")).unwrap();

        let (err, text) = call(
            &server,
            "update_host",
            json!({"id": host.id.to_string(), "hostname": "10.0.0.2", "username": null}),
        )
        .await;
        assert!(!err, "{text}");
        let row = server.vault().list_connections().unwrap().remove(0);
        assert_eq!(row.hostname, "10.0.0.2");
        assert_eq!(row.username, None, "null clears");
        assert_eq!(row.notes.as_deref(), Some("keep me"), "unnamed fields stay");
        assert_eq!(row.tags, vec!["a"]);
        assert_eq!(
            server.vault().get_connection_password(&host.id).unwrap().as_deref(),
            Some("pw"),
            "an absent password keeps the stored one"
        );

        let (err, _) = call(
            &server,
            "update_host",
            json!({"id": host.id.to_string(), "password": null}),
        )
        .await;
        assert!(!err);
        assert_eq!(server.vault().get_connection_password(&host.id).unwrap(), None);
    }

    /// Names are resolved, not guessed: two keys with one label need the
    /// id, a host is never its own hop, and a proxy is refused outright.
    #[tokio::test]
    async fn names_resolve_or_refuse() {
        let server = Server::new(test_vault());
        allow_writes(&server);
        let k1 = SshKey::new("deploy", KeyAlgorithm::Ed25519);
        let k2 = SshKey::new("deploy", KeyAlgorithm::Ed25519);
        server.vault().save_key(&k1, Some("PEM")).unwrap();
        server.vault().save_key(&k2, Some("PEM")).unwrap();
        let (err, text) = call(
            &server,
            "create_host",
            json!({"label": "web", "hostname": "10.0.0.1", "key": "deploy"}),
        )
        .await;
        assert!(err);
        assert!(text.contains("pass the id"), "{text}");
        let (err, text) = call(
            &server,
            "create_host",
            json!({"label": "web", "hostname": "10.0.0.1", "key": k1.id.to_string()}),
        )
        .await;
        assert!(!err, "{text}");
        let web: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(web["key_id"], k1.id.to_string());

        let (err, text) = call(
            &server,
            "update_host",
            json!({"id": web["id"], "jump_chain": ["web"]}),
        )
        .await;
        assert!(err);
        assert!(text.contains("own jump hop"), "{text}");

        let (err, text) = call(
            &server,
            "update_host",
            json!({"id": web["id"], "proxy": {"type": "command", "command": "nc"}}),
        )
        .await;
        assert!(err);
        assert!(text.contains("Proxies are not set over MCP"), "{text}");
    }
}
