use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Groups CRUD
    // -----------------------------------------------------------------------

    pub fn save_group(&self, group: &Group) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO groups (id, label, parent_id, color, icon, sort_order, is_shared, created_at, updated_at, cloud_query)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                group.id.to_string(),
                group.label,
                group.parent_id.map(|u| u.to_string()),
                group.color,
                group.icon,
                group.sort_order,
                group.is_shared as i32,
                group.created_at.to_rfc3339(),
                group.updated_at.to_rfc3339(),
                group.cloud_query.as_ref().map(|q| serde_json::to_string(q).unwrap_or_default()),
            ],
        )?;
        // Re-creating an entity clears any stale tombstone for it
        // (resurrection by a peer pushing a newer version after a
        // local delete). The free GC of stale tombstones happens via
        // `vacuum_tombstones` in the engine.
        self.clear_tombstone("group", &group.id)?;
        Ok(())
    }

    pub fn list_groups(&self) -> Result<Vec<Group>, VaultError> {
        let mut stmt = self
            .db
            .prepare("SELECT id, label, parent_id, color, icon, sort_order, is_shared, created_at, updated_at, cloud_query FROM groups ORDER BY sort_order")?;
        let groups = stmt
            .query_map([], |row| {
                Ok(Group {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    parent_id: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    color: row.get(3)?,
                    icon: row.get(4)?,
                    sort_order: row.get(5)?,
                    is_shared: row.get::<_, i32>(6)? != 0,
                    created_at: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    cloud_query: row
                        .get::<_, Option<String>>(9)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str::<CloudQuery>(&s).ok()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(groups)
    }

    pub fn delete_group(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db
            .execute("DELETE FROM groups WHERE id = ?1", params![id.to_string()])?;
        self.record_tombstone("group", id)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Session groups CRUD
    //
    // A session group is a saved split-panel arrangement. It carries no
    // credentials (every leaf references a host by id or is a local shell),
    // so there is no encrypted column and no password getter/setter. The
    // split tree lives in the `layout` column as JSON.
    // -----------------------------------------------------------------------

    pub fn save_session_group(&self, group: &SessionGroup) -> Result<(), VaultError> {
        let layout = serde_json::to_string(&group.layout)
            .map_err(|e| VaultError::Database(format!("session group layout: {e}")))?;
        self.db.execute(
            "INSERT OR REPLACE INTO session_groups (id, label, group_id, color, icon, layout, last_used, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                group.id.to_string(),
                group.label,
                group.group_id.map(|u| u.to_string()),
                group.color,
                group.icon_style,
                layout,
                group.last_used.map(|d| d.to_rfc3339()),
                group.created_at.to_rfc3339(),
                group.updated_at.to_rfc3339(),
            ],
        )?;
        self.clear_tombstone("session_group", &group.id)?;
        Ok(())
    }

    pub fn list_session_groups(&self) -> Result<Vec<SessionGroup>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, group_id, color, icon, layout, last_used, created_at, updated_at FROM session_groups ORDER BY label",
        )?;
        let groups = stmt
            .query_map([], |row| {
                let layout_json: String = row.get(5)?;
                let layout = serde_json::from_str(&layout_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                Ok(SessionGroup {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    group_id: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    color: row.get(3)?,
                    icon_style: row.get(4)?,
                    layout,
                    last_used: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc)),
                    created_at: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(groups)
    }

    pub fn delete_session_group(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM session_groups WHERE id = ?1",
            params![id.to_string()],
        )?;
        self.record_tombstone("session_group", id)?;
        Ok(())
    }

}
