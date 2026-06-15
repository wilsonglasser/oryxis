use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Sync Peers CRUD
    // -----------------------------------------------------------------------

    pub fn save_sync_peer(
        &self,
        peer_id: &uuid::Uuid,
        device_name: &str,
        public_key: &[u8],
        shared_secret: Option<&[u8]>,
        paired_at: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), VaultError> {
        let encrypted_secret = match shared_secret {
            Some(s) => Some(self.encrypt_field(&base64::engine::general_purpose::STANDARD.encode(s))?),
            None => None,
        };
        self.db.execute(
            "INSERT OR REPLACE INTO sync_peers
             (peer_id, device_name, public_key, shared_secret, paired_at, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            params![
                peer_id.to_string(),
                device_name,
                public_key,
                encrypted_secret,
                paired_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn update_sync_peer_endpoint(
        &self,
        peer_id: &uuid::Uuid,
        ip: &str,
        port: u16,
    ) -> Result<(), VaultError> {
        self.db.execute(
            "UPDATE sync_peers SET last_known_ip = ?1, last_known_port = ?2 WHERE peer_id = ?3",
            params![ip, port as i32, peer_id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_sync_peer_last_synced(
        &self,
        peer_id: &uuid::Uuid,
    ) -> Result<(), VaultError> {
        self.db.execute(
            "UPDATE sync_peers SET last_synced_at = ?1 WHERE peer_id = ?2",
            params![chrono::Utc::now().to_rfc3339(), peer_id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_sync_peers(&self) -> Result<Vec<SyncPeerRow>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT peer_id, device_name, public_key, last_known_ip, last_known_port,
                    last_synced_at, paired_at, is_active
             FROM sync_peers ORDER BY paired_at",
        )?;
        let peers = stmt
            .query_map([], |row| {
                Ok(SyncPeerRow {
                    peer_id: uuid::Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    device_name: row.get(1)?,
                    public_key: row.get(2)?,
                    last_known_ip: row.get(3)?,
                    last_known_port: row.get::<_, Option<i32>>(4)?.map(|p| p as u16),
                    last_synced_at: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc)),
                    paired_at: row
                        .get::<_, String>(6)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    is_active: row.get::<_, i32>(7)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(peers)
    }

    pub fn get_sync_peer_shared_secret(
        &self,
        peer_id: &uuid::Uuid,
    ) -> Result<Option<Vec<u8>>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT shared_secret FROM sync_peers WHERE peer_id = ?1",
                params![peer_id.to_string()],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        match data {
            Some(encrypted) => {
                let b64 = self.decrypt_field(&encrypted)?;
                let bytes = base64::engine::general_purpose::STANDARD.decode(&b64)
                    .map_err(|e| VaultError::Crypto(format!("Base64 decode: {}", e)))?;
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    pub fn delete_sync_peer(&self, peer_id: &uuid::Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM sync_peers WHERE peer_id = ?1",
            params![peer_id.to_string()],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Sync Metadata (tombstones)
    // -----------------------------------------------------------------------
    //
    // Every `delete_*` records a tombstone here so the sync engine can
    // propagate the deletion. The peer list itself is per-device, so
    // `delete_sync_peer` deliberately does NOT record one.

    /// Record a deletion tombstone. `entity_type` must be the wire
    /// string used by `oryxis_sync::protocol::EntityType` (its `Display`
    /// impl). Idempotent on the `(entity_type, entity_id)` primary key:
    /// a repeat call just refreshes the timestamp instead of duplicating
    /// the row. `updated_at` doubles as the LWW deletion timestamp the
    /// sync manifest compares against.
    pub fn record_tombstone(
        &self,
        entity_type: &str,
        entity_id: &Uuid,
    ) -> Result<(), VaultError> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT OR REPLACE INTO sync_metadata
             (entity_type, entity_id, updated_at, is_deleted, deleted_at)
             VALUES (?1, ?2, ?3, 1, ?3)",
            params![entity_type, entity_id.to_string(), now],
        )?;
        Ok(())
    }

    /// List every recorded deletion tombstone.
    pub fn list_tombstones(&self) -> Result<Vec<Tombstone>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT entity_type, entity_id, deleted_at
             FROM sync_metadata WHERE is_deleted = 1",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Tombstone {
                    entity_type: row.get(0)?,
                    entity_id: Uuid::parse_str(&row.get::<_, String>(1)?)
                        .unwrap_or_default(),
                    deleted_at: row
                        .get::<_, String>(2)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Drop a tombstone once every peer has acknowledged the deletion
    /// (or it has aged out). No-op if the row is already gone.
    pub fn clear_tombstone(
        &self,
        entity_type: &str,
        entity_id: &Uuid,
    ) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM sync_metadata WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id.to_string()],
        )?;
        Ok(())
    }

    /// Garbage-collect tombstones older than `older_than_days`. Called
    /// periodically from the sync engine to keep `sync_metadata` from
    /// growing without bound. Returns the number of rows removed.
    ///
    /// Two conditions must hold before a tombstone is dropped:
    /// 1. Age >= `older_than_days` (the lifetime cap).
    /// 2. Every active `SyncPeer` row has synced at least once since
    ///    `deleted_at`, so we know the deletion already propagated.
    ///    A peer with `last_synced_at = NULL` (never synced) blocks
    ///    GC of any tombstone. This is the trade-off that fixes the
    ///    "silent deletion resurrection" class of bugs flagged in the
    ///    audit. The price is unbounded tombstone growth when a
    ///    paired peer is gone forever; the [`SyncEvent::PeerStaleWarning`]
    ///    surfaced by the engine nudges the user to remove the dead
    ///    peer well before that becomes a real space issue.
    ///
    /// 30 days is the v1 default lifetime cap.
    pub fn vacuum_tombstones(&self, older_than_days: u32) -> Result<usize, VaultError> {
        let cutoff = (Utc::now() - chrono::Duration::days(older_than_days as i64))
            .to_rfc3339();
        // `NOT EXISTS` here means: only delete the tombstone if no
        // active peer is still behind it. Treats a never-synced peer
        // (last_synced_at IS NULL) as universally behind.
        let removed = self.db.execute(
            "DELETE FROM sync_metadata
             WHERE is_deleted = 1
               AND deleted_at < ?1
               AND NOT EXISTS (
                   SELECT 1 FROM sync_peers
                    WHERE is_active = 1
                      AND (last_synced_at IS NULL OR last_synced_at < sync_metadata.deleted_at)
               )",
            params![cutoff],
        )?;
        Ok(removed)
    }

    // -----------------------------------------------------------------------
    // Sync batch helpers
    // -----------------------------------------------------------------------

    /// Lean `(id, updated_at)` projection of a syncable table. The sync
    /// engine's manifest build only needs LWW stamps, so this skips the
    /// full-row SELECT + JSON decode that the `list_*` methods do.
    /// `table` is matched against an explicit whitelist (never
    /// interpolated from caller input) so the dynamic SQL cannot be
    /// abused for injection.
    pub fn list_entity_stamps(
        &self,
        table: &str,
    ) -> Result<Vec<(Uuid, DateTime<Utc>)>, VaultError> {
        match table {
            "connections" | "keys" | "identities" | "proxy_identities" | "groups"
            | "snippets" | "port_forward_rules" | "known_hosts" | "cloud_profiles"
            | "session_groups" => {}
            other => {
                return Err(VaultError::Database(format!(
                    "list_entity_stamps: unknown table {other}"
                )))
            }
        }
        let mut stmt = self
            .db
            .prepare(&format!("SELECT id, updated_at FROM {table}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    // `updated_at` is nullable on tables where it was
                    // backfilled via ALTER TABLE; fall back to "now"
                    // exactly like the full list_* readers do.
                    row.get::<_, Option<String>>(1)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Open an explicit transaction for a batch of `save_*` /
    /// `delete_*` calls. Without it each call runs as its own implicit
    /// SQLite transaction (one fsync per row, even under WAL), which
    /// makes applying a large sync delta dramatically slower than a
    /// single commit for the whole batch. Pair with
    /// [`Self::commit_batch`] or [`Self::rollback_batch`].
    pub fn begin_batch(&self) -> Result<(), VaultError> {
        self.db.execute_batch("BEGIN")?;
        Ok(())
    }

    /// Commit a batch opened with [`Self::begin_batch`].
    pub fn commit_batch(&self) -> Result<(), VaultError> {
        self.db.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Roll back a batch opened with [`Self::begin_batch`]. Best
    /// effort: an error here usually means SQLite already rolled the
    /// transaction back on its own, so it is logged and swallowed.
    pub fn rollback_batch(&self) {
        if let Err(e) = self.db.execute_batch("ROLLBACK") {
            tracing::warn!("rollback_batch: {e}");
        }
    }

}
