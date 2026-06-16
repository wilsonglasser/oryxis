use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Port forward rules
    // -----------------------------------------------------------------------

    pub fn save_port_forward_rule(&self, rule: &PortForwardRule) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO port_forward_rules
                (id, label, kind, host_id, listen_host, listen_port, target_host, target_port, auto_start, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                rule.id.to_string(),
                rule.label,
                rule.kind.as_token(),
                rule.host_id.to_string(),
                rule.listen_host,
                rule.listen_port as i64,
                rule.target_host,
                rule.target_port as i64,
                rule.auto_start as i64,
                rule.created_at.to_rfc3339(),
                rule.updated_at.to_rfc3339(),
            ],
        )?;
        self.clear_tombstone("port_forward_rule", &rule.id)?;
        Ok(())
    }

    pub fn list_port_forward_rules(&self) -> Result<Vec<PortForwardRule>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, kind, host_id, listen_host, listen_port, target_host, target_port, auto_start, created_at, updated_at
             FROM port_forward_rules ORDER BY label",
        )?;
        let rules = stmt
            .query_map([], |row| {
                Ok(PortForwardRule {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    kind: ForwardKind::from_token(&row.get::<_, String>(2)?),
                    host_id: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                    listen_host: row.get(4)?,
                    listen_port: row.get::<_, i64>(5)? as u16,
                    target_host: row.get(6)?,
                    target_port: row.get::<_, i64>(7)? as u16,
                    auto_start: row.get::<_, i64>(8)? != 0,
                    created_at: row
                        .get::<_, String>(9)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, String>(10)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rules)
    }

    pub fn delete_port_forward_rule(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM port_forward_rules WHERE id = ?1",
            params![id.to_string()],
        )?;
        self.record_tombstone("port_forward_rule", id)?;
        Ok(())
    }

}
