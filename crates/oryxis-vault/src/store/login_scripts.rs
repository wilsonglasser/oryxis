use super::*;

use oryxis_core::models::LoginScript;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Login scripts CRUD
    // -----------------------------------------------------------------------

    /// Save a login script. Unlike every other entity in this family
    /// there is no password argument: a script can only ever reference
    /// a secret (`SecretRef`), never carry one, which is what lets the
    /// whole row live in plaintext columns.
    pub fn save_login_script(&self, script: &LoginScript) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO login_scripts
             (id, name, steps, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                script.id.to_string(),
                script.name,
                serde_json::to_string(&script.steps).unwrap_or_else(|_| "[]".into()),
                script.created_at.to_rfc3339(),
                script.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_login_scripts(&self) -> Result<Vec<LoginScript>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, steps, created_at, updated_at
             FROM login_scripts ORDER BY name",
        )?;
        let scripts = stmt
            .query_map([], |row| {
                Ok(LoginScript {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    name: row.get(1)?,
                    // Malformed steps read as an empty script rather
                    // than failing the listing: an unusable automation
                    // is better than a settings screen that cannot open.
                    steps: row
                        .get::<_, String>(2)
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
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
        Ok(scripts)
    }

    /// Delete a script and detach it from every host that used it.
    ///
    /// The detach is hygiene, not correctness: resolution treats a
    /// dangling id as "no automation" already, the same rule
    /// `resolve_proxy` follows for a deleted proxy identity. Clearing
    /// the column keeps the host editor from showing a blank picker
    /// entry that a later script could silently inherit by id reuse.
    pub fn delete_login_script(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "UPDATE connections SET login_script = NULL
             WHERE json_extract(login_script, '$.id') = ?1",
            params![id.to_string()],
        )?;
        self.db.execute(
            "DELETE FROM login_scripts WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    /// How many hosts reference each script, for the management list.
    /// One query rather than a per-script count so the settings row can
    /// render without an N+1 walk.
    pub fn login_script_usage(&self) -> Result<std::collections::HashMap<Uuid, usize>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT json_extract(login_script, '$.id') AS sid, COUNT(*)
             FROM connections WHERE sid IS NOT NULL GROUP BY sid",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
            .filter_map(|r| r.ok())
            .filter_map(|(id, n)| {
                Uuid::parse_str(&id)
                    .ok()
                    .map(|id| (id, usize::try_from(n).unwrap_or(0)))
            })
            .collect();
        Ok(rows)
    }
}
