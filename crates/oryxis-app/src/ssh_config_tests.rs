//! Tests for the ssh_config parser. Covers the syntactic shapes we
//! actually see in real configs (per-line, indented, `=` separator,
//! quoted values, wildcard hosts, comments).

use super::*;

#[test]
fn parses_simple_host_block() {
    let input = "
Host bastion
    HostName bastion.example.com
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_ed25519
";
    let hosts = parse(input);
    assert_eq!(hosts.len(), 1);
    let h = &hosts[0];
    assert_eq!(h.alias, "bastion");
    assert_eq!(h.hostname.as_deref(), Some("bastion.example.com"));
    assert_eq!(h.port, Some(2222));
    assert_eq!(h.user.as_deref(), Some("admin"));
    assert!(h.identity_file.is_some());
}

#[test]
fn skips_wildcard_hosts() {
    // The `Host *` block is the global default in real configs —
    // never importable as a concrete server.
    let input = "
Host *
    ServerAliveInterval 30

Host real
    HostName 10.0.0.1
";
    let hosts = parse(input);
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "real");
}

#[test]
fn supports_equals_separator_and_quoted_values() {
    let input = "
Host quoted
    HostName=\"db.example.com\"
    Port=3306
";
    let hosts = parse(input);
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].hostname.as_deref(), Some("db.example.com"));
    assert_eq!(hosts[0].port, Some(3306));
}

#[test]
fn ignores_comments_and_blanks() {
    let input = "
# this is a comment
Host alpha
    # inline-style
    HostName alpha.local

Host beta
    HostName beta.local
";
    let hosts = parse(input);
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].alias, "alpha");
    assert_eq!(hosts[1].alias, "beta");
}

#[test]
fn captures_proxy_jump_chain() {
    let input = "
Host inner
    HostName 10.1.1.1
    ProxyJump bastion
";
    let hosts = parse(input);
    assert_eq!(hosts[0].proxy_jump.as_deref(), Some("bastion"));
}

#[test]
fn captures_forward_agent_only_when_yes() {
    // OpenSSH defaults to `ForwardAgent no`; only an explicit `yes`
    // flips agent forwarding on, so the parser should treat any other
    // value (or its absence) as off.
    let input = "
Host with-fwd
    HostName fwd.example.com
    ForwardAgent yes

Host without-fwd
    HostName plain.example.com

Host explicit-no
    HostName off.example.com
    ForwardAgent no
";
    let hosts = parse(input);
    let by_alias = |a: &str| hosts.iter().find(|h| h.alias == a).unwrap();
    assert!(by_alias("with-fwd").forward_agent);
    assert!(!by_alias("without-fwd").forward_agent);
    assert!(!by_alias("explicit-no").forward_agent);
    // Round-trip onto Connection — flag should propagate.
    let conn = to_connection(by_alias("with-fwd"));
    assert!(conn.agent_forwarding);
}

#[test]
fn first_alias_wins_for_multi_alias_host() {
    // `Host a b c` — pick the first as the canonical name.
    let input = "
Host primary alt1 alt2
    HostName primary.example.com
";
    let hosts = parse(input);
    assert_eq!(hosts[0].alias, "primary");
}

#[test]
fn to_connection_maps_fields() {
    let input = "
Host srv
    HostName srv.local
    User wilson
    Port 22
    IdentityFile ~/.ssh/id_ed25519
";
    let hosts = parse(input);
    let conn = to_connection(&hosts[0]);
    assert_eq!(conn.label, "srv");
    assert_eq!(conn.hostname, "srv.local");
    assert_eq!(conn.username.as_deref(), Some("wilson"));
    assert_eq!(conn.port, 22);
    assert_eq!(conn.auth_method, oryxis_core::models::connection::AuthMethod::Key);
    // Notes carries the import provenance.
    assert!(conn.notes.as_deref().unwrap_or("").contains("ssh_config"));
}

#[test]
fn to_connection_falls_back_to_alias_when_no_hostname() {
    let input = "
Host bare
    User root
";
    let hosts = parse(input);
    let conn = to_connection(&hosts[0]);
    // No HostName given → use alias as the address. Common for short
    // SSH config aliases that happen to be valid hostnames already.
    assert_eq!(conn.hostname, "bare");
    assert_eq!(conn.auth_method, oryxis_core::models::connection::AuthMethod::Auto);
}

#[test]
fn handles_indentation_variants() {
    // SSH config tolerates tab-indented, no-indent, mixed — parser
    // should match.
    let input = "Host noindent\nHostName x.local\n\nHost tabindent\n\tHostName y.local\n";
    let hosts = parse(input);
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].hostname.as_deref(), Some("x.local"));
    assert_eq!(hosts[1].hostname.as_deref(), Some("y.local"));
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_parse_never_panics(input in ".{0,500}") {
        // Pure smoke — random input should never crash the parser.
        // Real ssh_config files are user-edited and we get whatever
        // they wrote.
        let _ = parse(&input);
    }

    #[test]
    fn prop_parse_block_count_matches_host_lines(
        // Generate N "Host alias\n" prefix lines, no body. Parser
        // should report exactly N concrete hosts (none wildcarded).
        aliases in proptest::collection::vec("[a-z][a-z0-9-]{0,15}", 1..8),
    ) {
        let mut input = String::new();
        for alias in &aliases {
            input.push_str(&format!("Host {}\n", alias));
        }
        let hosts = parse(&input);
        prop_assert_eq!(hosts.len(), aliases.len());
        for (i, alias) in aliases.iter().enumerate() {
            prop_assert_eq!(&hosts[i].alias, alias);
        }
    }

    #[test]
    fn prop_wildcard_aliases_always_skipped(
        suffix in "[a-z][a-z0-9-]{0,10}",
    ) {
        let inputs = [
            "Host *\n  HostName x\n".to_string(),
            format!("Host *.{}\n  HostName x\n", suffix),
            format!("Host {}?\n  HostName x\n", suffix),
        ];
        for input in inputs {
            let hosts = parse(&input);
            prop_assert!(hosts.is_empty(), "expected wildcard to be skipped: {input}");
        }
    }

    #[test]
    fn prop_to_connection_label_matches_alias(
        alias in "[a-z][a-zA-Z0-9_.-]{0,30}",
        port in 1u16..65535,
    ) {
        let host = SshConfigHost {
            alias: alias.clone(),
            hostname: Some("h.example.com".into()),
            port: Some(port),
            user: Some("u".into()),
            identity_file: None,
            proxy_jump: None,
            forward_agent: false,
        };
        let conn = to_connection(&host);
        // Invariant: label always carries the alias verbatim — it's
        // the user-facing identifier.
        prop_assert_eq!(conn.label, alias);
        prop_assert_eq!(conn.port, port);
    }
}
