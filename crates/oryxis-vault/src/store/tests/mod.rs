use super::*;
use oryxis_core::models::connection::Connection;
use oryxis_core::models::group::Group;
use oryxis_core::models::key::{KeyAlgorithm, SshKey};
use oryxis_core::models::known_host::KnownHost;
use oryxis_core::models::log_entry::{LogEntry, LogEvent};
use oryxis_core::models::session_group::{
    PaneLayout, PaneMember, PaneSource, SessionGroup, SplitAxis,
};
use oryxis_core::models::snippet::Snippet;
use tempfile::NamedTempFile;

fn temp_vault() -> VaultStore {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    // Keep the file alive by leaking it (tests are short-lived)
    std::mem::forget(tmp);
    VaultStore::open(&path).unwrap()
}

fn unlocked_vault() -> VaultStore {
    let mut vault = temp_vault();
    vault.set_master_password("testpass123").unwrap();
    vault
}

mod cloud;
mod connections;
mod core_crypto;
mod forwarding;
mod groups;
mod identities;
mod keys;
mod logs;
mod portable;
mod settings;
mod snippets;
mod sync;
