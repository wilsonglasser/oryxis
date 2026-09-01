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

            -- Install scripts (issue #147): which install snippet ran on
            -- which host, and when. Local bookkeeping, deliberately not
            -- synced or exported: it states a fact about THIS vault's
            -- view of a host, and a hint is allowed to be conservative.
            CREATE TABLE IF NOT EXISTS install_runs (
                host_id    TEXT NOT NULL,
                snippet_id TEXT NOT NULL,
                ran_at     TEXT NOT NULL,
                PRIMARY KEY (host_id, snippet_id)
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

            -- Reusable expect/send login automations linked from
            -- `connections` via `login_script_id`. `steps` is plain JSON
            -- on purpose: a step can only reference a secret, never
            -- carry one (see `oryxis_core::login_script::SecretRef`).
            CREATE TABLE IF NOT EXISTS login_scripts (
                id         TEXT PRIMARY KEY,
                name       TEXT NOT NULL,
                steps      TEXT NOT NULL DEFAULT '[]',
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

            -- Command proxies this DEVICE accepts spawning. Deliberately
            -- outside sync and portable export (no EntityType, no export
            -- category): the approval answers whether the local human
            -- accepts running the line on THIS machine, and a replicated
            -- answer would be a decision made for a person who never saw
            -- the command. See store/proxy_trust.rs for the rationale.
            CREATE TABLE IF NOT EXISTS trusted_proxy_commands (
                fingerprint TEXT PRIMARY KEY,
                label       TEXT NOT NULL,
                trusted_at  TEXT NOT NULL
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

            -- Per-host command history (terminal sidebar History tab). One
            -- row per distinct command per host, frequency-counted in place;
            -- local-only like session logs (no sync, no portable export).
            -- The command text is sealed with the content key in
            -- `command_enc` (a command line can carry echoed inline
            -- secrets the capture gates cannot see, so it is treated
            -- like session-recording data, not like snippets.command).
            -- `command` holds a keyed dedup hash (HMAC-SHA256 under the
            -- content key), which keeps the unique index and the
            -- bump-in-place UPDATE working without deterministic
            -- encryption. Rows written before this scheme carry the
            -- plaintext in `command` with a NULL `command_enc` and are
            -- sealed by a one-shot migration on first unlocked use.
            CREATE TABLE IF NOT EXISTS command_history (
                id            TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                command       TEXT NOT NULL,
                command_enc   BLOB,
                use_count     INTEGER NOT NULL DEFAULT 1,
                last_used_at  TEXT NOT NULL,
                created_at    TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_command_history_conn_cmd
                ON command_history(connection_id, command);

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

            -- Saved AI chat conversations, one row per tab conversation.
            -- Local-only like session logs and command history: never
            -- synced, never in the portable export.
            --
            -- `session_log_id` is an OPTIONAL correlation with the
            -- recording of the same session, deliberately not a
            -- dependency: session logging is opt-in per host, and a chat
            -- must not silently go unsaved because recording happened to
            -- be off. It also keeps the chat out of the `.cast` and
            -- transcript exports, which carry terminal output only.
            --
            -- `connection_id` is NULL for a local shell (no saved host).
            CREATE TABLE IF NOT EXISTS chat_conversations (
                id             TEXT PRIMARY KEY,
                connection_id  TEXT,
                session_log_id TEXT,
                label          TEXT NOT NULL,
                provider       TEXT NOT NULL,
                model          TEXT NOT NULL,
                started_at     TEXT NOT NULL,
                updated_at     TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_conversations_updated
                ON chat_conversations(updated_at);

            -- Turns of a saved conversation, append-only, ordered by the
            -- monotonic rowid the way `session_log_chunks` is.
            --
            -- Both payloads are sealed with the session-log content key,
            -- not stored in plaintext: a chat turn quotes terminal output
            -- and command lines, which is exactly the material the session
            -- recording treats as secret-bearing. The reasoning field is
            -- deliberately NOT persisted: it is provider bookkeeping for a
            -- live conversation, and saved chats are read-only.
            CREATE TABLE IF NOT EXISTS chat_messages (
                id              INTEGER PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                role            TEXT NOT NULL,
                content_enc     BLOB NOT NULL,
                tool_enc        BLOB,
                created_at      TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_messages_conv
                ON chat_messages(conversation_id);
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
        let _ = self.db.execute_batch("ALTER TABLE keys ADD COLUMN expose_via_agent INTEGER NOT NULL DEFAULT 1;");
        // B2: an OpenSSH user certificate for the key (public material, so
        // a plaintext column like `public_key`). NULL = no certificate.
        let _ = self.db.execute_batch("ALTER TABLE keys ADD COLUMN certificate TEXT;");
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
        // Per-host Privacy Mode override. NULL = inherit the global
        // `privacy_mode` setting, 0 = never hide, 1 = always hide.
        // Existing rows stay NULL, so behavior is unchanged on upgrade.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN privacy_mode INTEGER;");
        // TOTP secret for keyboard-interactive 2FA autofill. Encrypted in
        // its own BLOB column like `proxy_password`; stores the user's raw
        // input (bare Base32 or a full otpauth:// URI), parsed at code
        // generation time.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN totp_secret BLOB;");
        // Wire protocol selector ("ssh" / "telnet" / "serial"). NULL = ssh,
        // so every pre-existing row keeps meaning what it meant.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN protocol TEXT;");
        // Serial-line parameters as JSON (SerialParams). NULL on every
        // non-serial host.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN serial_config TEXT;");
        // Telnet-only options as JSON (TelnetOptions: TLS + its per-host
        // verification escape). NULL on every other host AND on a Telnet
        // host that never turned TLS on, which is what makes an upgraded
        // vault read back as plain Telnet with verification intact.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN telnet_config TEXT;");
        // mosh options as JSON. NULL on every host that is not carried
        // over mosh, which is every host written before this existed.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN mosh_config TEXT;");
        // Local-shell settings as JSON (LocalConfig: which curated local
        // terminal to spawn, and where it starts). NULL on every
        // non-local host.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN local_config TEXT;");
        // Remote-desktop connection fields (protocol = RemoteDesktop):
        // `rd_kind` = 'rdp' | 'vnc'; `rd_gateway_id` = the SSH host to
        // tunnel through (NULL = direct). The desktop endpoint + login
        // reuse hostname/port/username/password. The earlier
        // `remote_desktop` JSON column (unreleased 0.9 bolt-on) is left
        // dead, superseded by these.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN remote_desktop TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN rd_kind TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN rd_gateway_id TEXT;");
        // Outbound address-family preference ('auto' | 'v4' | 'v6');
        // NULL on older rows reads as 'auto'.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN address_family TEXT;");
        // Backing query for dynamic groups (ECS services / K8s workloads).
        // JSON-encoded `CloudQuery`. NULL for manual groups.
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN cloud_query TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE keys ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN created_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN updated_at TEXT;");
        // Per-parameter defaults the group hands down to its hosts
        // (D4). JSON-encoded `GroupDefaults`, NULL when the group sets
        // nothing, which is every group from before the feature. No
        // secret lives here: credentials are an identity REFERENCE.
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN defaults TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE snippets ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE known_hosts ADD COLUMN updated_at TEXT;");
        // Snippet group ("folder") name; free-form, NULL = ungrouped.
        // "group" is an SQL keyword, hence group_name.
        let _ = self.db.execute_batch("ALTER TABLE snippets ADD COLUMN group_name TEXT;");
        // Per-snippet custom hotkey (serialized binding, e.g. "ctrl+shift+k").
        let _ = self.db.execute_batch("ALTER TABLE snippets ADD COLUMN hotkey TEXT;");
        // Install-script category (issue #147): 1 = one-time host setup
        // with the confirm + per-host memory affordances. NULL/0 =
        // ordinary snippet.
        let _ = self.db.execute_batch("ALTER TABLE snippets ADD COLUMN install INTEGER;");
        // Session-recording timing (asciicast export): milliseconds since
        // the log's started_at, stamped at capture time. NULL on chunks
        // recorded before this column existed (exported with a fixed
        // small delta so old logs still replay, just without real
        // timing). `kind` distinguishes output ('o', the default) from
        // terminal resizes ('r', whose data is "<cols>x<rows>").
        // A recording that was cut short (free space ran out, or the
        // size cap was reached) must SAY so: the player and the
        // `.cast` / transcript exports otherwise hand back a partial
        // stream that presents itself as the whole session, which is a
        // worse failure for an audit feature than stopping is.
        let _ = self.db.execute_batch("ALTER TABLE session_logs ADD COLUMN truncated INTEGER NOT NULL DEFAULT 0;");
        let _ = self.db.execute_batch("ALTER TABLE session_log_chunks ADD COLUMN offset_ms INTEGER;");
        let _ = self.db.execute_batch("ALTER TABLE session_log_chunks ADD COLUMN kind TEXT NOT NULL DEFAULT 'o';");
        // Per-chunk compression flag: 0 = raw, 1 = deflate applied to
        // the plaintext before sealing (ciphertext doesn't compress).
        // Pre-existing rows are raw by the DEFAULT.
        let _ = self.db.execute_batch("ALTER TABLE session_log_chunks ADD COLUMN comp INTEGER NOT NULL DEFAULT 0;");
        // Encrypted command text (content-key sealed). Rows written
        // before this column existed keep their plaintext in `command`
        // until the one-shot migration on first unlocked use replaces
        // it with the keyed dedup hash (see the table comment above).
        let _ = self.db.execute_batch("ALTER TABLE command_history ADD COLUMN command_enc BLOB;");

        // C5: per-host legacy keyboard modes + terminal feature toggles
        // (JSON blob, NULL = all-xterm defaults) and the SSH rekey
        // threshold in MB (NULL = russh default).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN quirks TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN terminal_appearance TEXT;");
        // C6: this host's own highlight rules plus the append /
        // replace choice (JSON blob, NULL = follow the global list).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN highlight_rules TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN rekey_limit_mb INTEGER;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN monitor_enabled INTEGER DEFAULT 0;");
        // Which mounts the monitor reports for this host (issue #135):
        // NULL = Auto (the probe's own rules), a JSON list = Custom.
        // The empty list is a real value there ("no disks on this
        // host"), which is why it is a nullable JSON column rather than
        // a comma-separated string.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN monitor_disks TEXT;");

        // Offer a key read from `~/.ssh` when this host has no vault
        // key, and which file to read (OpenSSH's `IdentityFile`, NULL =
        // scan the default names). `identity_file` holds a PATH, never
        // key material, so it stays a plaintext column: the file is
        // read at connect time and nothing about it is stored here.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN use_disk_key INTEGER DEFAULT 0;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN identity_file TEXT;");

        // Per-host tri-state override for auto-opening the terminal
        // sidebar on connect (NULL = follow the global setting).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN sidebar_auto_open INTEGER;");

        // Directory a fresh SFTP mount of the host lands in (NULL / empty
        // = the login directory, the previous behaviour).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN sftp_initial_path TEXT;");

        // Drag-and-drop uploads to this host ride ZMODEM (`rz`) instead
        // of SFTP, for shells that run inside a container (0 = standard
        // drop routing).
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN zmodem_drops INTEGER DEFAULT 0;");

        // Per-host X11 forwarding (OpenSSH `ForwardX11`), off by default.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN x11_forwarding INTEGER DEFAULT 0;");

        // Wake-on-LAN MAC address (NULL / empty = no MAC, card action
        // hidden). Plain text: a MAC is a locator, not a credential.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN mac_address TEXT;");

        // Login automation for hosts behind an interactive bastion:
        // `login_script` is the JSON `{ id, vars }` pair (plaintext, it
        // holds only a reference and placeholder values), while
        // `target_password` is the credential the script types at the
        // asset's own prompt, encrypted like `totp_secret`. The
        // connection's existing `password` column is spent on the
        // bastion login, which is why the second secret needs a column
        // of its own.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN login_script TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN target_password BLOB;");

        // East Asian ambiguous width ('auto' | 'narrow' | 'wide'); NULL
        // on older rows reads as 'auto', which resolves narrow unless the
        // host carries a legacy CJK encoding.
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN ambiguous_width TEXT;");

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
