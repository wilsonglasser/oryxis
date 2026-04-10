use std::cmp::Ordering;

use crate::protocol::ManifestEntry;

/// What to do with a record after comparing local vs remote manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    /// Remote is newer — accept it.
    AcceptRemote,
    /// Local is newer — push to remote.
    PushLocal,
    /// Same timestamp — skip.
    Skip,
}

/// Last-writer-wins conflict resolution by comparing `updated_at`.
pub fn resolve(local: &ManifestEntry, remote: &ManifestEntry) -> SyncAction {
    match local.updated_at.cmp(&remote.updated_at) {
        Ordering::Less => SyncAction::AcceptRemote,
        Ordering::Greater => SyncAction::PushLocal,
        Ordering::Equal => SyncAction::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::EntityType;
    use chrono::Utc;
    use uuid::Uuid;

    fn entry(secs_offset: i64, deleted: bool) -> ManifestEntry {
        ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: Uuid::new_v4(),
            updated_at: Utc::now() + chrono::Duration::seconds(secs_offset),
            is_deleted: deleted,
        }
    }

    #[test]
    fn remote_newer_wins() {
        let local = entry(0, false);
        let remote = entry(10, false);
        assert_eq!(resolve(&local, &remote), SyncAction::AcceptRemote);
    }

    #[test]
    fn local_newer_wins() {
        let local = entry(10, false);
        let remote = entry(0, false);
        assert_eq!(resolve(&local, &remote), SyncAction::PushLocal);
    }

    #[test]
    fn same_timestamp_skips() {
        let ts = Utc::now();
        let id = Uuid::new_v4();
        let local = ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: id,
            updated_at: ts,
            is_deleted: false,
        };
        let remote = ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: id,
            updated_at: ts,
            is_deleted: false,
        };
        assert_eq!(resolve(&local, &remote), SyncAction::Skip);
    }

    #[test]
    fn deletion_participates_in_lww() {
        let local = entry(0, false);
        let remote = entry(10, true); // deleted more recently
        assert_eq!(resolve(&local, &remote), SyncAction::AcceptRemote);
    }
}
