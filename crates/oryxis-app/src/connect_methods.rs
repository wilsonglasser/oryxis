//! `impl Oryxis` block for SSH-connect plumbing — credential resolution,
//! jump-host resolver assembly, and the host-key verification callback.
//! Pulled out of `app.rs` to keep the main module from drifting past
//! ten thousand lines.

use std::sync::{Arc, Mutex};

use oryxis_core::models::connection::{AuthMethod, Connection};

use crate::app::Oryxis;

impl Oryxis {
    /// Resolve `(password, private_key_pem)` for a connection — same
    /// rules as `Message::ConnectSsh`: prefer identity-linked credentials,
    /// fall back to per-connection vault entries.
    pub(crate) fn resolve_credentials(
        &self,
        conn: &Connection,
    ) -> (Option<String>, Option<String>) {
        if let Some(iid) = conn.identity_id {
            let id_pw = self
                .vault
                .as_ref()
                .and_then(|v| v.get_identity_password(&iid).ok().flatten());
            let identity = self.identities.iter().find(|i| i.id == iid);
            let id_key = identity.and_then(|i| i.key_id).and_then(|kid| {
                self.vault
                    .as_ref()
                    .and_then(|v| v.get_key_private(&kid).ok().flatten())
            });
            (id_pw, id_key)
        } else {
            let pw = self
                .vault
                .as_ref()
                .and_then(|v| v.get_connection_password(&conn.id).ok().flatten());
            let pk = if conn.auth_method == AuthMethod::Key
                || conn.auth_method == AuthMethod::Auto
            {
                conn.key_id.and_then(|kid| {
                    self.vault
                        .as_ref()
                        .and_then(|v| v.get_key_private(&kid).ok().flatten())
                })
            } else {
                None
            };
            (pw, pk)
        }
    }

    /// Build a `ConnectionResolver` covering the jump-host chain of the
    /// given connection. `None` when there's no chain.
    pub(crate) fn make_jump_resolver(
        &self,
        conn: &Connection,
    ) -> Option<oryxis_ssh::ConnectionResolver> {
        if conn.jump_chain.is_empty() {
            return None;
        }
        let mut passwords = std::collections::HashMap::new();
        let mut keys = std::collections::HashMap::new();
        let mut proxies = std::collections::HashMap::new();
        for jid in &conn.jump_chain {
            if let Some(vault) = &self.vault
                && let Ok(Some(pw)) = vault.get_connection_password(jid)
            {
                passwords.insert(*jid, pw);
            }
            if let Some(jconn) = self.connections.iter().find(|c| c.id == *jid)
                && let Some(kid) = jconn.key_id
                && let Some(vault) = &self.vault
                && let Ok(Some(pk)) = vault.get_key_private(&kid)
            {
                keys.insert(*jid, pk);
            }
            // Resolve the jump host's effective proxy (identity-based or
            // inline) so the engine's first-hop dial can route through it.
            // Only matters for the first jump but we hydrate every jump's
            // entry — cheap and keeps the resolver self-contained.
            if let Some(jconn) = self.connections.iter().find(|c| c.id == *jid)
                && let Some(vault) = &self.vault
                && let Ok(Some(p)) = vault.resolve_proxy(jconn)
            {
                proxies.insert(*jid, p);
            }
        }
        Some(oryxis_ssh::ConnectionResolver {
            connections: self.connections.clone(),
            passwords,
            private_keys: keys,
            proxies,
        })
    }

    /// Build the host-key verification callback against the in-memory
    /// `known_hosts` snapshot. Read-only — known-host writes still happen
    /// in the connect handler itself.
    pub(crate) fn make_host_key_check(&self) -> oryxis_ssh::HostKeyCheckCallback {
        let snapshot = Arc::new(Mutex::new(self.known_hosts.clone()));
        Arc::new(move |host, port, _key_type, fingerprint| {
            let hosts = match snapshot.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if let Some(existing) = hosts.iter().find(|h| h.hostname == host && h.port == port) {
                if existing.fingerprint != fingerprint {
                    return oryxis_ssh::HostKeyStatus::Changed {
                        old_fingerprint: existing.fingerprint.clone(),
                    };
                }
                return oryxis_ssh::HostKeyStatus::Known;
            }
            oryxis_ssh::HostKeyStatus::Unknown
        })
    }
}
