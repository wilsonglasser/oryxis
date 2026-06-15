use super::*;

#[test]
fn save_and_list_known_hosts() {
    let vault = unlocked_vault();
    let kh = KnownHost::new("example.com", 22, "ssh-ed25519", "SHA256:abc123");
    vault.save_known_host(&kh).unwrap();

    let hosts = vault.list_known_hosts().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].hostname, "example.com");
    assert_eq!(hosts[0].fingerprint, "SHA256:abc123");
}


#[test]
fn known_host_unique_per_host_port() {
    let vault = unlocked_vault();
    let kh1 = KnownHost::new("server.com", 22, "ssh-ed25519", "SHA256:first");
    vault.save_known_host(&kh1).unwrap();

    let kh2 = KnownHost::new("server.com", 22, "ssh-rsa", "SHA256:second");
    vault.save_known_host(&kh2).unwrap();

    let hosts = vault.list_known_hosts().unwrap();
    assert_eq!(hosts.len(), 1); // UNIQUE constraint
}


#[test]
fn delete_known_host() {
    let vault = unlocked_vault();
    let kh = KnownHost::new("host.test", 22, "ed25519", "SHA256:xyz");
    vault.save_known_host(&kh).unwrap();
    vault.delete_known_host(&kh.id).unwrap();
    assert_eq!(vault.list_known_hosts().unwrap().len(), 0);
}

// ── Logs ──


#[test]
fn known_host_has_updated_at() {
    let vault = unlocked_vault();
    let kh = KnownHost::new("host.test", 22, "ed25519", "SHA256:xyz");
    assert!(kh.updated_at.timestamp() > 0);
    vault.save_known_host(&kh).unwrap();

    let hosts = vault.list_known_hosts().unwrap();
    assert_eq!(hosts.len(), 1);
    assert!(hosts[0].updated_at.timestamp() > 0);
}

// ── Export / Import ──


#[test]
fn port_forward_rule_round_trip() {
    use oryxis_core::models::port_forward_rule::{ForwardKind, PortForwardRule};
    let vault = unlocked_vault();
    let host_id = Uuid::new_v4();
    let mut rule = PortForwardRule::new("db tunnel", ForwardKind::Local, host_id);
    rule.listen_host = "0.0.0.0".into();
    rule.listen_port = 5432;
    rule.target_host = "10.0.0.5".into();
    rule.target_port = 5432;
    rule.auto_start = true;
    vault.save_port_forward_rule(&rule).unwrap();

    let listed = vault.list_port_forward_rules().unwrap();
    assert_eq!(listed.len(), 1);
    let got = &listed[0];
    assert_eq!(got.id, rule.id);
    assert_eq!(got.kind, ForwardKind::Local);
    assert_eq!(got.host_id, host_id);
    assert_eq!(got.listen_host, "0.0.0.0");
    assert_eq!(got.listen_port, 5432);
    assert_eq!(got.target_port, 5432);
    assert!(got.auto_start);

    vault.delete_port_forward_rule(&rule.id).unwrap();
    assert!(vault.list_port_forward_rules().unwrap().is_empty());
}
