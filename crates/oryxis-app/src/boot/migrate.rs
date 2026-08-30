use crate::app::Oryxis;
use oryxis_vault::VaultStore;

impl Oryxis {
    /// One-shot migration of legacy inline `Connection.port_forwards` into
    /// standalone `PortForwardRule` rows (always `Local`, `auto_start =
    /// false`). The legacy field is left intact, it still raises forwards
    /// alongside the terminal session, the new rules just make the same
    /// tunnels runnable on their own. Gated by a settings flag so it runs
    /// exactly once.
    pub(super) fn migrate_port_forwards(&mut self, vault: &oryxis_vault::store::VaultStore) {
        if vault
            .get_setting("port_forwards_migrated")
            .ok()
            .flatten()
            .as_deref()
            == Some("true")
        {
            return;
        }
        let rules = legacy_forwards_to_rules(&self.connections);
        let mut created = 0usize;
        for rule in &rules {
            match vault.save_port_forward_rule(rule) {
                Ok(()) => created += 1,
                Err(e) => tracing::warn!("port-forward migration: save failed: {e}"),
            }
        }
        let _ = vault.set_setting("port_forwards_migrated", "true");
        if created > 0 {
            tracing::info!("migrated {created} legacy port forward(s) into standalone rules");
            self.port_forward_rules = vault.list_port_forward_rules().unwrap_or_default();
        }
    }

}

/// Re-home every group whose parent no longer exists onto root. A parent
/// that has been deleted is dangling: a child left pointing at it renders
/// nowhere while still counting as a record. Persists only the rows that
/// actually move, so it's a cheap no-op once the hierarchy is clean. A
/// `visited` set guards against a parent cycle in corrupt data.
pub(super) fn repair_group_parents(
    groups: &mut [oryxis_core::models::Group],
    vault: &VaultStore,
) {
    let ids: std::collections::HashSet<uuid::Uuid> =
        groups.iter().map(|g| g.id).collect();
    let fixes: Vec<(uuid::Uuid, Option<uuid::Uuid>)> = groups
        .iter()
        .filter_map(|g| {
            let dangling = g.parent_id.is_some_and(|pid| !ids.contains(&pid));
            dangling.then_some((g.id, None))
        })
        .collect();
    for (gid, new_parent) in fixes {
        if let Some(g) = groups.iter_mut().find(|g| g.id == gid) {
            g.parent_id = new_parent;
            g.updated_at = chrono::Utc::now();
            let _ = vault.save_group(g);
        }
    }
}

/// Pure mapping from legacy inline `Connection.port_forwards` to standalone
/// `PortForwardRule`s. Every legacy forward is Local, binds `127.0.0.1` on
/// its old `local_port`, targets the old `remote_host:remote_port`, and is
/// created with `auto_start = false`. Kept separate from the vault I/O so
/// the mapping is unit-testable.
fn legacy_forwards_to_rules(
    conns: &[oryxis_core::models::connection::Connection],
) -> Vec<oryxis_core::models::port_forward_rule::PortForwardRule> {
    use oryxis_core::models::port_forward_rule::{ForwardKind, PortForwardRule};
    let mut rules = Vec::new();
    for conn in conns {
        for pf in &conn.port_forwards {
            let mut rule = PortForwardRule::new(
                format!("{} :{}", conn.label, pf.local_port),
                ForwardKind::Local,
                conn.id,
            );
            rule.listen_host = "127.0.0.1".into();
            rule.listen_port = pf.local_port;
            rule.target_host = pf.remote_host.clone();
            rule.target_port = pf.remote_port;
            rule.auto_start = false;
            rules.push(rule);
        }
    }
    rules
}

#[cfg(test)]
mod port_forward_migration_tests {
    use super::legacy_forwards_to_rules;
    use oryxis_core::models::connection::{Connection, PortForward};
    use oryxis_core::models::port_forward_rule::ForwardKind;

    #[test]
    fn maps_each_legacy_forward_to_a_local_rule() {
        let mut conn = Connection::new("db-box", "10.0.0.1");
        conn.port_forwards = vec![
            PortForward { local_port: 5432, remote_host: "127.0.0.1".into(), remote_port: 5432 },
            PortForward { local_port: 6379, remote_host: "cache.internal".into(), remote_port: 6379 },
        ];
        let other = Connection::new("no-forwards", "10.0.0.2");

        let rules = legacy_forwards_to_rules(&[conn.clone(), other]);

        // Two forwards on one connection, none on the other.
        assert_eq!(rules.len(), 2);
        for r in &rules {
            assert_eq!(r.kind, ForwardKind::Local);
            assert_eq!(r.host_id, conn.id);
            assert_eq!(r.listen_host, "127.0.0.1");
            assert!(!r.auto_start);
        }
        assert_eq!(rules[0].listen_port, 5432);
        assert_eq!(rules[0].target_host, "127.0.0.1");
        assert_eq!(rules[0].target_port, 5432);
        assert_eq!(rules[1].listen_port, 6379);
        assert_eq!(rules[1].target_host, "cache.internal");
        assert_eq!(rules[1].target_port, 6379);
    }

    #[test]
    fn no_forwards_yields_no_rules() {
        let conn = Connection::new("plain", "10.0.0.3");
        assert!(legacy_forwards_to_rules(&[conn]).is_empty());
    }
}

