use super::*;

impl VaultStore {
    pub(super) fn create_tables(&mut self) -> Result<(), VaultError> {
        self.db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS vault_meta (
                key   TEXT PRIMARY KEY,
                value BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS groups (
                id         TEXT PRIMARY KEY,
                label      TEXT NOT NULL,
                parent_id  TEXT,
                color      TEXT,
                icon       TEXT,
                sort_order INTEGER DEFAULT 0,
                is_shared  INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS session_groups (
                id         TEXT PRIMARY KEY,
                label      TEXT NOT NULL,
                group_id   TEXT,
                color      TEXT,
                icon       TEXT,
                layout     TEXT NOT NULL,
                last_used  TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connections (
                id          TEXT PRIMARY KEY,
                label       TEXT NOT NULL,
                hostname    TEXT NOT NULL,
                port        INTEGER DEFAULT 22,
                username    TEXT,
                auth_method TEXT NOT NULL DEFAULT 'password',
                key_id      TEXT,
                group_id    TEXT REFERENCES groups(id),
                jump_chain  TEXT,
                proxy       TEXT,
                tags        TEXT,
                notes       TEXT,
                color       TEXT,
                password    BLOB,
                last_used   TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS keys (
                id            TEXT PRIMARY KEY,
                label         TEXT NOT NULL,
                fingerprint   TEXT,
                algorithm     TEXT NOT NULL,
                public_key    TEXT,
                private_key   BLOB,
                has_passphrase INTEGER DEFAULT 0,
                created_at    TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS snippets (
                id          TEXT PRIMARY KEY,
                label       TEXT NOT NULL,
                command     TEXT NOT NULL,
                description TEXT,
                tags        TEXT,
                created_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS custom_terminal_themes (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                foreground  TEXT NOT NULL,
                background  TEXT NOT NULL,
                cursor      TEXT NOT NULL,
                ansi        TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS custom_ui_themes (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                colors      TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS port_forward_rules (
                id          TEXT PRIMARY KEY,
                label       TEXT NOT NULL,
                kind        TEXT NOT NULL,
                host_id     TEXT NOT NULL,
                listen_host TEXT NOT NULL,
                listen_port INTEGER NOT NULL,
                target_host TEXT NOT NULL,
                target_port INTEGER NOT NULL,
                auto_start  INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS identities (
                id         TEXT PRIMARY KEY,
                label      TEXT NOT NULL,
                username   TEXT,
                password   BLOB,
                key_id     TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            -- Reusable proxy configurations linked from `connections`
            -- via `proxy_identity_id`. Password is stored encrypted in
            -- the same column-level scheme as `identities.password`.
            CREATE TABLE IF NOT EXISTS proxy_identities (
                id         TEXT PRIMARY KEY,
                label      TEXT NOT NULL,
                proxy_type TEXT NOT NULL,
                host       TEXT NOT NULL DEFAULT '',
                port       INTEGER NOT NULL DEFAULT 0,
                username   TEXT,
                password   BLOB,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS known_hosts (
                id          TEXT PRIMARY KEY,
                hostname    TEXT NOT NULL,
                port        INTEGER DEFAULT 22,
                key_type    TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                first_seen  TEXT NOT NULL,
                last_seen   TEXT NOT NULL,
                UNIQUE(hostname, port)
            );

            CREATE TABLE IF NOT EXISTS logs (
                id               TEXT PRIMARY KEY,
                connection_label TEXT NOT NULL,
                hostname         TEXT NOT NULL,
                event            TEXT NOT NULL,
                message          TEXT NOT NULL,
                timestamp        TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_logs (
                id            TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                label         TEXT NOT NULL,
                started_at    TEXT NOT NULL,
                ended_at      TEXT,
                data          BLOB
            );

            -- Append-only recorded terminal output. The original design
            -- stored the whole stream in `session_logs.data` and rewrote
            -- that growing BLOB on every chunk (O(n^2) writes, disk-bound
            -- on verbose sessions). Each append is now one INSERT of just
            -- the new bytes; `get_session_data` concatenates by rowid. The
            -- monotonic `id` (plain rowid, no AUTOINCREMENT needed since we
            -- only ever delete whole logs) preserves append order. Legacy
            -- rows keep their inline `session_logs.data` and are read back
            -- as a prefix.
            CREATE TABLE IF NOT EXISTS session_log_chunks (
                id     INTEGER PRIMARY KEY,
                log_id TEXT NOT NULL,
                data   BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session_log_chunks_log
                ON session_log_chunks(log_id);

            -- Cloud account credentials (AWS profile / SSO / access key,
            -- K8s kubeconfig path, ...). `config` carries the non-secret
            -- JSON payload owned by each provider crate. `secret` is the
            -- per-field encrypted blob hydrated only when the provider
            -- actually needs it (mirrors `identities.password`).
            CREATE TABLE IF NOT EXISTS cloud_profiles (
                id              TEXT PRIMARY KEY,
                label           TEXT NOT NULL,
                provider        TEXT NOT NULL,
                auth_kind       TEXT NOT NULL,
                config          TEXT NOT NULL DEFAULT '{}',
                secret          BLOB,
                last_discovered TEXT,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            );
            ",
        )?;

        // Migrations: add columns to existing tables (ignore errors if already present)
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN identity_id TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN mcp_enabled INTEGER DEFAULT 1;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN port_forwards TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN detected_os TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN custom_icon TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN custom_color TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN agent_forwarding INTEGER DEFAULT 0;");
        // Proxy password is stored encrypted in its own BLOB column so it
        // never leaks via the plaintext `proxy` JSON column.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN proxy_password BLOB;");
        // Reference to a `proxy_identities` row when the host uses a
        // saved proxy config instead of an inline one. NULL on cascade
        // when the referenced identity is deleted.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN proxy_identity_id TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN terminal_theme TEXT;");
        // Cloud-managed handle for hosts imported from a `cloud_profiles`
        // row (EC2 in v0.6). JSON-encoded `CloudRef`. NULL for manual hosts.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN cloud_ref TEXT;");
        // Per-host initial command sent right after the shell opens.
        // Independent of cloud, used by ECS / K8s entries that drop into
        // `/bin/sh` and want `exec bash`.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN initial_command TEXT;");
        // Optional reference to a snippet whose body is the startup command
        // (resolved live at connect). Stored as the UUID text.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN startup_snippet_id TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN keepalive_interval INTEGER;");
        // Per-host auto-title (OSC 0/2) override: NULL inherits global, 0/1 force.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN auto_title INTEGER;");
        // Per-host TERM name (NULL = xterm-256color).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN terminal_type TEXT;");
        // Per-host SSH algorithm overrides (legacy-cipher support). JSON
        // arrays of wire names; NULL = Auto (russh safe defaults).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN ciphers TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN kex TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN macs TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN host_key_algorithms TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN icon_style TEXT;");
        // JSON array of field names the user has explicitly overridden
        // on a cloud-imported host. Reimport leaves listed fields
        // alone. NULL / empty for manual hosts and untouched imports.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN customized_fields TEXT;");
        // JSON array of per-host environment variables sent via SSH setenv.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN env_vars TEXT;");
        // Per-host character encoding label (NULL = UTF-8).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN encoding TEXT;");
        // Per-host session-recording override. NULL = inherit the global
        // `session_logging` setting, 0 = never record, 1 = always record.
        // Existing rows stay NULL, so behavior is unchanged on upgrade.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN session_logging INTEGER;");
        // Backing query for dynamic groups (ECS services / K8s workloads).
        // JSON-encoded `CloudQuery`. NULL for manual groups.
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN cloud_query TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE keys ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN created_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE snippets ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE known_hosts ADD COLUMN updated_at TEXT;");

        // Populate new timestamp columns with sensible defaults
        let _ = self.db.execute_batch("UPDATE keys SET updated_at = created_at WHERE updated_at IS NULL;");
        let _ = self.db.execute_batch("UPDATE groups SET created_at = datetime('now'), updated_at = datetime('now') WHERE created_at IS NULL;");
        let _ = self.db.execute_batch("UPDATE snippets SET updated_at = created_at WHERE updated_at IS NULL;");
        let _ = self.db.execute_batch("UPDATE known_hosts SET updated_at = last_seen WHERE updated_at IS NULL;");

        // Sync tables
        let _ = self.db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sync_peers (
                peer_id         TEXT PRIMARY KEY,
                device_name     TEXT NOT NULL,
                public_key      BLOB NOT NULL,
                shared_secret   BLOB,
                last_known_ip   TEXT,
                last_known_port INTEGER,
                last_synced_at  TEXT,
                paired_at       TEXT NOT NULL,
                is_active       INTEGER DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS sync_metadata (
                entity_type TEXT NOT NULL,
                entity_id   TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                is_deleted  INTEGER DEFAULT 0,
                deleted_at  TEXT,
                PRIMARY KEY (entity_type, entity_id)
            );
            ",
        );

        Ok(())
    }
}
