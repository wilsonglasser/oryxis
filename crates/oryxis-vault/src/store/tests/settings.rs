use super::*;

#[test]
fn settings_round_trip_string_value() {
    let vault = temp_vault();
    vault.set_setting("scrollback_rows", "10000").unwrap();
    assert_eq!(
        vault.get_setting("scrollback_rows").unwrap().as_deref(),
        Some("10000"),
    );
}


#[test]
fn settings_get_unset_returns_none() {
    let vault = temp_vault();
    // No tests should pollute global state, but defensively
    // assert that a fresh vault has nothing under our key.
    assert_eq!(
        vault.get_setting("never_set_anywhere").unwrap(),
        None,
    );
}


#[test]
fn settings_overwrite_replaces_value() {
    let vault = temp_vault();
    vault.set_setting("ai_provider", "anthropic").unwrap();
    vault.set_setting("ai_provider", "openai").unwrap();
    assert_eq!(
        vault.get_setting("ai_provider").unwrap().as_deref(),
        Some("openai"),
    );
}


#[test]
fn settings_persist_across_reopen() {
    // Opening the vault file twice (different `VaultStore`
    // instances backed by the same SQLite file) must yield the
    // same setting value, the bug we're guarding against is
    // someone adding a transient cache that doesn't write through.
    use tempfile::NamedTempFile;
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    {
        let v = VaultStore::open(&path).unwrap();
        v.set_setting("sftp_concurrency", "4").unwrap();
    }
    let v2 = VaultStore::open(&path).unwrap();
    assert_eq!(
        v2.get_setting("sftp_concurrency").unwrap().as_deref(),
        Some("4"),
    );
}


#[test]
fn settings_handle_unicode_values() {
    // The settings panel exposes `ai_system_prompt` as a free-form
    // string the user can fill with anything. Make sure we round-
    // trip non-ASCII without mangling.
    let vault = temp_vault();
    let value = "Olá 🚀, system prompt with emojis";
    vault.set_setting("ai_system_prompt", value).unwrap();
    assert_eq!(
        vault.get_setting("ai_system_prompt").unwrap().as_deref(),
        Some(value),
    );
}


#[test]
fn settings_independent_keys_dont_collide() {
    let vault = temp_vault();
    vault.set_setting("a", "1").unwrap();
    vault.set_setting("b", "2").unwrap();
    vault.set_setting("c", "3").unwrap();
    assert_eq!(vault.get_setting("a").unwrap().as_deref(), Some("1"));
    assert_eq!(vault.get_setting("b").unwrap().as_deref(), Some("2"));
    assert_eq!(vault.get_setting("c").unwrap().as_deref(), Some("3"));
}

// ── Cloud Profiles ──

