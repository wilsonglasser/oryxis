use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Snippets CRUD
    // -----------------------------------------------------------------------

    pub fn save_snippet(&self, snippet: &Snippet) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO snippets (id, label, command, description, tags, group_name, hotkey, install, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                snippet.id.to_string(),
                snippet.label,
                snippet.command,
                snippet.description,
                serde_json::to_string(&snippet.tags).unwrap_or_default(),
                snippet.group,
                snippet.hotkey,
                i32::from(snippet.install),
                snippet.created_at.to_rfc3339(),
                snippet.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_snippets(&self) -> Result<Vec<Snippet>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, command, description, tags, group_name, hotkey, install, created_at, updated_at FROM snippets ORDER BY label",
        )?;
        let snippets = stmt
            .query_map([], |row| {
                Ok(Snippet {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    command: row.get(2)?,
                    description: row.get(3)?,
                    tags: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    group: row
                        .get::<_, Option<String>>(5)?
                        .filter(|g| !g.trim().is_empty()),
                    hotkey: row
                        .get::<_, Option<String>>(6)?
                        .filter(|h| !h.trim().is_empty()),
                    install: row
                        .get::<_, Option<i64>>(7)?
                        .unwrap_or(0)
                        != 0,
                    created_at: row
                        .get::<_, String>(8)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(snippets)
    }

    pub fn delete_snippet(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM snippets WHERE id = ?1",
            params![id.to_string()],
        )?;
        // Its install-run rows say "THIS snippet ran here"; without the
        // snippet they answer nothing (and would resurrect as a wrong
        // hint if its id were ever reused by an import).
        self.db.execute(
            "DELETE FROM install_runs WHERE snippet_id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // -- Install runs (issue #147) --
    // Which install script ran on which host, and when. Local
    // bookkeeping only: not synced, not exported (the table states a
    // fact about this vault's own history with a host, and a hint that
    // under-reports is fine, one that over-reports is not).

    /// Record (or refresh) "this install snippet ran on this host now".
    pub fn record_install_run(
        &self,
        host_id: &Uuid,
        snippet_id: &Uuid,
    ) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO install_runs (host_id, snippet_id, ran_at)
             VALUES (?1,?2,?3)",
            params![
                host_id.to_string(),
                snippet_id.to_string(),
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Every recorded run, for the boot load: ((host, snippet), when).
    pub fn list_install_runs(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, chrono::DateTime<chrono::Utc>)>, VaultError> {
        let mut stmt = self
            .db
            .prepare("SELECT host_id, snippet_id, ran_at FROM install_runs")?;
        let runs = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .filter_map(|(h, s, at)| {
                Some((
                    Uuid::parse_str(&h).ok()?,
                    Uuid::parse_str(&s).ok()?,
                    chrono::DateTime::parse_from_rfc3339(&at)
                        .ok()?
                        .with_timezone(&chrono::Utc),
                ))
            })
            .collect();
        Ok(runs)
    }

    // -- Custom terminal themes --
    // Plain config rows (no secrets, so no per-field encryption; not in the
    // sync set yet, so no tombstones). Colors are `"#RRGGBB"` hex; the 16
    // ANSI entries are stored as a JSON array.

    pub fn save_custom_terminal_theme(
        &self,
        theme: &CustomTerminalTheme,
    ) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO custom_terminal_themes
                (id, name, foreground, background, cursor, ansi, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                theme.id.to_string(),
                theme.name,
                theme.foreground,
                theme.background,
                theme.cursor,
                serde_json::to_string(&theme.ansi).unwrap_or_default(),
                theme.created_at.to_rfc3339(),
                theme.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_custom_terminal_themes(
        &self,
    ) -> Result<Vec<CustomTerminalTheme>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, foreground, background, cursor, ansi, created_at, updated_at
             FROM custom_terminal_themes ORDER BY name",
        )?;
        let themes = stmt
            .query_map([], |row| {
                let ansi: [String; 16] = row
                    .get::<_, String>(5)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_else(|| std::array::from_fn(|_| "#000000".to_string()));
                Ok(CustomTerminalTheme {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    name: row.get(1)?,
                    foreground: row.get(2)?,
                    background: row.get(3)?,
                    cursor: row.get(4)?,
                    ansi,
                    created_at: row
                        .get::<_, String>(6)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, String>(7)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(themes)
    }

    pub fn delete_custom_terminal_theme(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM custom_terminal_themes WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // -- Custom UI (chrome) themes --
    // Plain config rows; the 21 chrome colors are stored as a JSON array.

    pub fn save_custom_ui_theme(&self, theme: &CustomUiTheme) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO custom_ui_themes
                (id, name, colors, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                theme.id.to_string(),
                theme.name,
                serde_json::to_string(&theme.colors).unwrap_or_default(),
                theme.created_at.to_rfc3339(),
                theme.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_custom_ui_themes(&self) -> Result<Vec<CustomUiTheme>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, colors, created_at, updated_at
             FROM custom_ui_themes ORDER BY name",
        )?;
        let themes = stmt
            .query_map([], |row| {
                let colors: [String; 21] = row
                    .get::<_, String>(2)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_else(|| std::array::from_fn(|_| "#000000".to_string()));
                Ok(CustomUiTheme {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    name: row.get(1)?,
                    colors,
                    created_at: row
                        .get::<_, String>(3)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, String>(4)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(themes)
    }

    pub fn delete_custom_ui_theme(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM custom_ui_themes WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

}
