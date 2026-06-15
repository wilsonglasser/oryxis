use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Logs CRUD
    // -----------------------------------------------------------------------

    pub fn add_log(&self, entry: &oryxis_core::models::log_entry::LogEntry) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT INTO logs (id, connection_label, hostname, event, message, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                entry.id.to_string(), entry.connection_label, entry.hostname,
                entry.event.to_string(), entry.message, entry.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_logs(&self, limit: usize) -> Result<Vec<oryxis_core::models::log_entry::LogEntry>, VaultError> {
        self.list_logs_page(0, limit)
    }

    /// Paginated variant: skip `offset` rows and return up to `limit` rows
    /// (still ordered by timestamp desc).
    pub fn list_logs_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<oryxis_core::models::log_entry::LogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_label, hostname, event, message, timestamp
             FROM logs ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2",
        )?;
        let logs = stmt.query_map(params![limit as i64, offset as i64], |row| {
            let event_str: String = row.get(3)?;
            let event = match event_str.as_str() {
                "Connected" => oryxis_core::models::log_entry::LogEvent::Connected,
                "Disconnected" => oryxis_core::models::log_entry::LogEvent::Disconnected,
                "Auth Failed" => oryxis_core::models::log_entry::LogEvent::AuthFailed,
                _ => oryxis_core::models::log_entry::LogEvent::Error,
            };
            Ok(oryxis_core::models::log_entry::LogEntry {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                connection_label: row.get(1)?,
                hostname: row.get(2)?,
                event,
                message: row.get(4)?,
                timestamp: row.get::<_, String>(5).ok()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now),
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    pub fn clear_logs(&self) -> Result<(), VaultError> {
        self.db.execute("DELETE FROM logs", [])?;
        Ok(())
    }

    /// Total number of log rows, used to drive pagination controls.
    pub fn count_logs(&self) -> Result<usize, VaultError> {
        let n: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    // -----------------------------------------------------------------------
    // Session Logs CRUD (terminal recording)
    // -----------------------------------------------------------------------

    /// Create a new session log entry with started_at = now.
    pub fn create_session_log(
        &self,
        id: &Uuid,
        connection_id: &Uuid,
        label: &str,
    ) -> Result<(), VaultError> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT INTO session_logs (id, connection_id, label, started_at, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.to_string(),
                connection_id.to_string(),
                label,
                now,
                Vec::<u8>::new(),
            ],
        )?;
        Ok(())
    }

    /// Content key for session recordings: a random 256-bit key wrapped
    /// with the master key in `vault_meta` (`session_log_key`). Chunks
    /// are sealed with this key directly (no per-chunk KDF), so appends
    /// stay cheap; only the first use after unlock pays the unwrap.
    /// Generated lazily on the first recording of a vault's lifetime.
    fn session_log_key(&self) -> Result<[u8; KEY_LEN], VaultError> {
        if let Some(k) = *self.session_log_key.lock().unwrap() {
            return Ok(k);
        }
        let master = self.require_unlocked()?;
        let wrapped: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'session_log_key'",
                [],
                |row| row.get(0),
            )
            .ok();
        let key: [u8; KEY_LEN] = match wrapped {
            Some(w) => decrypt_with_key(&w, master)?
                .try_into()
                .map_err(|_| VaultError::Crypto("malformed session log key".into()))?,
            None => {
                let mut k = [0u8; KEY_LEN];
                OsRng.fill_bytes(&mut k);
                let w = encrypt_with_key(&k, master)?;
                self.db.execute(
                    "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('session_log_key', ?1)",
                    params![w],
                )?;
                k
            }
        };
        *self.session_log_key.lock().unwrap() = Some(key);
        Ok(key)
    }

    /// Seal a chunk with the session content key: random nonce(12) +
    /// ciphertext(+16 tag).
    fn seal_chunk(&self, data: &[u8]) -> Result<Vec<u8>, VaultError> {
        let key = self.session_log_key()?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|e| VaultError::Crypto(e.to_string()))?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), data)
            .map_err(|e| VaultError::Crypto(e.to_string()))?;
        let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ct);
        Ok(blob)
    }

    /// Inverse of `seal_chunk`. `None` when the blob isn't a sealed
    /// chunk under `key` (i.e. a chunk recorded by an older version),
    /// in which case the caller uses the raw bytes as-is.
    fn unseal_chunk(key: &[u8; KEY_LEN], blob: &[u8]) -> Option<Vec<u8>> {
        if blob.len() < NONCE_LEN + 16 {
            return None;
        }
        let cipher = ChaCha20Poly1305::new_from_slice(key).ok()?;
        cipher
            .decrypt(Nonce::from_slice(&blob[..NONCE_LEN]), &blob[NONCE_LEN..])
            .ok()
    }

    /// Append recorded terminal bytes to a session log. One INSERT of just
    /// the new bytes, no read-modify-write of the growing stream. Callers
    /// should batch (see the app's per-pane buffer) so this fires at a
    /// human cadence rather than once per SSH chunk.
    pub fn append_session_data(&self, id: &Uuid, data: &[u8]) -> Result<(), VaultError> {
        if data.is_empty() {
            return Ok(());
        }
        let sealed = self.seal_chunk(data)?;
        self.db.execute(
            "INSERT INTO session_log_chunks (log_id, data) VALUES (?1, ?2)",
            params![id.to_string(), sealed],
        )?;
        Ok(())
    }

    /// Set ended_at = now on a session log.
    pub fn end_session_log(&self, id: &Uuid) -> Result<(), VaultError> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "UPDATE session_logs SET ended_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    /// List all session logs (metadata only, no data blob).
    pub fn list_session_logs(&self) -> Result<Vec<SessionLogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_id, label, started_at, ended_at,
                    LENGTH(COALESCE(data, X'')) + COALESCE(
                        (SELECT SUM(LENGTH(c.data)) FROM session_log_chunks c
                         WHERE c.log_id = session_logs.id), 0)
             FROM session_logs ORDER BY started_at DESC",
        )?;
        let logs = stmt
            .query_map([], |row| {
                Ok(SessionLogEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    connection_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                    label: row.get(2)?,
                    started_at: row
                        .get::<_, String>(3)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                    ended_at: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc)),
                    data_size: row.get::<_, i64>(5).unwrap_or(0) as usize,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    /// Paginated variant of `list_session_logs`. Same column projection
    /// (no data blob), ordered by started_at desc, sliced by SQL LIMIT/OFFSET.
    pub fn list_session_logs_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<SessionLogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_id, label, started_at, ended_at,
                    LENGTH(COALESCE(data, X'')) + COALESCE(
                        (SELECT SUM(LENGTH(c.data)) FROM session_log_chunks c
                         WHERE c.log_id = session_logs.id), 0)
             FROM session_logs ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let logs = stmt
            .query_map(params![limit as i64, offset as i64], |row| {
                Ok(SessionLogEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    connection_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                    label: row.get(2)?,
                    started_at: row
                        .get::<_, String>(3)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                    ended_at: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc)),
                    data_size: row.get::<_, i64>(5).unwrap_or(0) as usize,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    /// Total number of session log rows.
    pub fn count_session_logs(&self) -> Result<usize, VaultError> {
        let n: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM session_logs",
            [],
            |row| row.get(0),
        )?;
        Ok(n as usize)
    }

    /// Delete connection events and *finished* session recordings
    /// older than `cutoff` (retention setting). In-progress sessions
    /// are never pruned: their rows are still being appended to.
    /// Returns how many rows (events + sessions) were removed.
    pub fn prune_logs_older_than(
        &self,
        cutoff: chrono::DateTime<Utc>,
    ) -> Result<usize, VaultError> {
        let cutoff = cutoff.to_rfc3339();
        let events = self.db.execute(
            "DELETE FROM logs WHERE timestamp < ?1",
            params![cutoff],
        )?;
        self.db.execute(
            "DELETE FROM session_log_chunks WHERE log_id IN
                 (SELECT id FROM session_logs
                  WHERE ended_at IS NOT NULL AND started_at < ?1)",
            params![cutoff],
        )?;
        let sessions = self.db.execute(
            "DELETE FROM session_logs WHERE ended_at IS NOT NULL AND started_at < ?1",
            params![cutoff],
        )?;
        Ok(events + sessions)
    }

    /// Drop every session log row (and its recorded chunks).
    pub fn clear_session_logs(&self) -> Result<(), VaultError> {
        self.db.execute("DELETE FROM session_log_chunks", [])?;
        self.db.execute("DELETE FROM session_logs", [])?;
        Ok(())
    }

    /// Get the raw recorded bytes for a session log: the legacy inline
    /// blob (empty for sessions recorded after the chunk migration)
    /// followed by every appended chunk in append order. The row lookup
    /// doubles as the existence check (NotFound when the log is gone).
    /// Sealed chunks are opened with the session content key; chunks
    /// recorded by older versions pass through as-is.
    pub fn get_session_data(&self, id: &Uuid) -> Result<Option<Vec<u8>>, VaultError> {
        let id_str = id.to_string();
        let legacy: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT data FROM session_logs WHERE id = ?1",
                params![id_str],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Session log {}", id)))?;
        let key = self.session_log_key().ok();
        let mut buf = legacy.unwrap_or_default();
        let mut stmt = self.db.prepare(
            "SELECT data FROM session_log_chunks WHERE log_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id_str], |row| row.get::<_, Vec<u8>>(0))?;
        for chunk in rows {
            let chunk = chunk?;
            match key.as_ref().and_then(|k| Self::unseal_chunk(k, &chunk)) {
                Some(plain) => buf.extend_from_slice(&plain),
                None => buf.extend_from_slice(&chunk),
            }
        }
        Ok(Some(buf))
    }

    /// Delete a session log and its recorded chunks.
    pub fn delete_session_log(&self, id: &Uuid) -> Result<(), VaultError> {
        let id_str = id.to_string();
        self.db.execute(
            "DELETE FROM session_log_chunks WHERE log_id = ?1",
            params![id_str],
        )?;
        self.db.execute(
            "DELETE FROM session_logs WHERE id = ?1",
            params![id_str],
        )?;
        Ok(())
    }

}
