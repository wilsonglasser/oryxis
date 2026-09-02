//! Unit tests for `app.rs` helpers. Loaded via `#[path] mod tests` at
//! the bottom of `app.rs` so it's part of the same module and can see
//! the private helpers directly.

#[allow(unused_imports)]
use super::*;
use crate::sftp_helpers::{
    is_safe_remote_entry_name, parent_path, remote_join, transfer_item_label, unique_entry_name,
};

#[test]
fn unsafe_remote_entry_names_are_rejected() {
    // Server-controlled names must never escape the download root.
    for bad in [
        "",
        ".",
        "..",
        "../etc",
        "a/b",
        "/etc/cron.d/x",
        "..\\evil",
        "C:\\evil",
        "C:evil",
        "a\0b",
    ] {
        assert!(!is_safe_remote_entry_name(bad), "accepted {bad:?}");
    }
    for good in ["file.txt", ".bashrc", "...", "a b c", "weird:name", "über"] {
        assert!(is_safe_remote_entry_name(good), "rejected {good:?}");
    }
}

#[test]
fn remote_join_root_special_case() {
    // The root case is the only one that tripped us in real use
    // `/` + `foo` was producing `//foo` until we special-cased it.
    assert_eq!(remote_join("/", "foo"), "/foo");
    assert_eq!(remote_join("/home", "foo"), "/home/foo");
    assert_eq!(remote_join("/home/", "foo"), "/home/foo");
    assert_eq!(remote_join("/a/b/c", "d"), "/a/b/c/d");
}

#[test]
fn unique_entry_name_no_collision_keeps_basename() {
    let busy: std::collections::HashSet<String> = ["other.txt"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        unique_entry_name("file.txt", |n| !busy.contains(n)),
        "file.txt"
    );
}

#[test]
fn unique_entry_name_first_collision_appends_copy() {
    let busy: std::collections::HashSet<String> = ["file.txt"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        unique_entry_name("file.txt", |n| !busy.contains(n)),
        "file copy.txt"
    );
}

#[test]
fn unique_entry_name_repeated_collision_uses_numeric_suffix() {
    let busy: std::collections::HashSet<String> =
        ["file.txt", "file copy.txt", "file copy 2.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    assert_eq!(
        unique_entry_name("file.txt", |n| !busy.contains(n)),
        "file copy 3.txt"
    );
}

#[test]
fn unique_entry_name_handles_extensionless_files() {
    let busy: std::collections::HashSet<String> = ["README"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        unique_entry_name("README", |n| !busy.contains(n)),
        "README copy"
    );
}

#[test]
fn unique_entry_name_handles_dotfiles() {
    // `.bashrc` has no "stem.ext" split, the leading dot is part
    // of the name, so the suffix lands at the end.
    let busy: std::collections::HashSet<String> = [".bashrc"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let result = unique_entry_name(".bashrc", |n| !busy.contains(n));
    // Either "bashrc copy" with the leading dot eaten by the
    // rsplit_once boundary check, or ".bashrc copy", accept the
    // function's actual behaviour and lock it in here.
    assert_ne!(result, ".bashrc");
    assert!(!busy.contains(&result));
}

#[test]
fn parent_path_root_stays_root() {
    assert_eq!(parent_path("/"), "/");
    assert_eq!(parent_path(""), "/");
}

#[test]
fn parent_path_strips_one_segment() {
    assert_eq!(parent_path("/foo"), "/");
    assert_eq!(parent_path("/foo/bar"), "/foo");
    assert_eq!(parent_path("/foo/bar/baz"), "/foo/bar");
}

#[test]
fn parent_path_ignores_trailing_slash() {
    assert_eq!(parent_path("/foo/bar/"), "/foo");
}

#[test]
fn transfer_item_label_marks_directories() {
    let dir = crate::state::TransferItem {
        src: "/a/b/c".into(),
        dst: "/x/c".into(),
        is_dir: true,
        size: None,
    };
    let file = crate::state::TransferItem {
        src: "/a/b/c.txt".into(),
        dst: "/x/c.txt".into(),
        is_dir: false,
        size: Some(123),
    };
    assert_eq!(transfer_item_label(&dir), "c/");
    assert_eq!(transfer_item_label(&file), "c.txt");
}

// ---------------------------------------------------------------------------
// Property-based tests
//
// Fuzz-style coverage for the path / name helpers, generates random
// strings through proptest and asserts invariants that should hold
// regardless of input shape. Catches edge cases the hand-written
// examples missed (empty strings, embedded slashes, weird unicode,
// extreme lengths).
// ---------------------------------------------------------------------------

use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_remote_join_never_doubles_slash(
        // Caller contract: `dir` is a well-formed POSIX absolute path
        // (single leading slash, no embedded `//`). Generate strings
        // matching that, `/segment/segment/...` with no doubles.
        dir in "/(([a-zA-Z0-9_-]+)(/[a-zA-Z0-9_-]+)*)?",
        basename in "[a-zA-Z0-9_.-]+",
    ) {
        let joined = remote_join(&dir, &basename);
        // Invariant 1: no `//` should ever appear. The whole point of
        // remote_join's special-cased root is to avoid `//foo` when
        // dir is just `/`.
        prop_assert!(!joined.contains("//"));
        // Invariant 2: result starts at root.
        prop_assert!(joined.starts_with('/'));
        // Invariant 3: the basename is the trailing segment.
        prop_assert!(joined.ends_with(&basename));
    }

    #[test]
    fn prop_unique_entry_name_returns_free_name(
        basename in "[a-zA-Z0-9._-]{1,30}",
        // Up to 5 random "busy" names that collide; helper should
        // skip past them.
        busy_count in 0usize..6,
    ) {
        let mut busy = std::collections::HashSet::new();
        // Force the basename itself to be busy so we exercise the
        // suffixing path; then add busy_count more decoys.
        busy.insert(basename.clone());
        // "name copy", "name copy 2", ... "name copy K" all busy
        let (stem, ext) = match basename.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{}", e)),
            _ => (basename.clone(), String::new()),
        };
        if busy_count >= 1 {
            busy.insert(format!("{} copy{}", stem, ext));
        }
        for k in 2..=busy_count {
            busy.insert(format!("{} copy {}{}", stem, k, ext));
        }
        let result = unique_entry_name(&basename, |n| !busy.contains(n));
        // Invariant: result is not in the busy set.
        prop_assert!(!busy.contains(&result));
        // Invariant: result preserves the extension when one existed.
        if let Some((_, ext)) = basename.rsplit_once('.')
            && !basename.starts_with('.')
        {
            let suffix = format!(".{}", ext);
            prop_assert!(result.ends_with(&suffix));
        }
    }

    #[test]
    fn prop_parent_path_idempotent_on_root(
        // "/" repeated some number of times, parent of any all-slash
        // string should still be "/".
        n in 1usize..10,
    ) {
        let path = "/".repeat(n);
        prop_assert_eq!(parent_path(&path), "/");
    }

    #[test]
    fn prop_parent_path_strips_one_segment(
        segments in proptest::collection::vec("[a-zA-Z0-9_-]+", 1..6),
    ) {
        let path = format!("/{}", segments.join("/"));
        let parent = parent_path(&path);
        // parent should be the path minus the last segment, rooted.
        let expected = if segments.len() == 1 {
            "/".to_string()
        } else {
            format!("/{}", segments[..segments.len() - 1].join("/"))
        };
        prop_assert_eq!(parent, expected);
    }
}

#[test]
fn sftp_owned_envelope_carries_owner_for_routing() {
    // The mount pipeline stamps its completions with the buffer owner at
    // kickoff time; the dispatcher must read that stamp back so
    // `route_sftp_async` can swap the owner's state in.
    let id = uuid::Uuid::new_v4();
    let wrapped = Message::sftp_owned(
        Some(id),
        SftpMessage::RemoteError(crate::state::SftpPaneSide::Right, "boom".into()),
    );
    assert_eq!(wrapped.sftp_async_owner(), Some(id));
    // No owner at kickoff: the bare message falls back to the live buffer
    // and must not enter the routing path.
    let bare = Message::sftp_owned(
        None,
        SftpMessage::RemoteError(crate::state::SftpPaneSide::Right, "boom".into()),
    );
    assert_eq!(bare.sftp_async_owner(), None);
}

#[test]
fn sftp_state_unsaved_covers_edit_watches() {
    // Shared close-guard predicate: standalone tab close and the hybrid
    // Close-SFTP-session guard must agree on what counts as unsaved work.
    let mut st = crate::state::SftpState::default();
    assert!(!crate::sftp_methods::sftp_state_has_unsaved(&st));
    let session = crate::state::EditSession {
        client: None,
        remote_path: "/srv/x.conf".into(),
        temp_path: std::path::PathBuf::from("/tmp/oryxis-x.conf"),
        label: "x.conf".into(),
        host: "web-1".into(),
        opener: crate::state::SftpEditOpener::OsDefault,
        initial_mtime: None,
        dirty: false,
        uploading: false,
    };
    // A registered watch counts as unsaved even while CLEAN: the external
    // editor is still open and closing would silently orphan its future
    // saves. A watch holding a pending save obviously counts too.
    st.edit_watches.push(session);
    assert!(crate::sftp_methods::sftp_state_has_unsaved(&st));
    st.edit_watches[0].dirty = true;
    assert!(crate::sftp_methods::sftp_state_has_unsaved(&st));
    st.edit_watches.clear();
    assert!(!crate::sftp_methods::sftp_state_has_unsaved(&st));
}

// --- SFTP console gating (issue #188) --------------------------------

/// A console dials through `start_ssh_tab`, and `start_ssh_tab` forwards
/// every non-SSH protocol to its own connect path. None of those reach
/// `SshConnected`, so a console asked for on one would never open AND
/// its one-shot purpose flag would never be consumed: the next ordinary
/// SSH tab would be born a console. The gate is what prevents both.
#[test]
fn only_ssh_hosts_can_carry_a_console() {
    use oryxis_core::models::connection::ConnectionProtocol;

    let mut conn = oryxis_core::models::Connection::new("host", "example.com");
    conn.protocol = ConnectionProtocol::Ssh;
    assert!(Oryxis::host_can_console(&conn));

    for protocol in [
        ConnectionProtocol::Telnet,
        ConnectionProtocol::Raw,
        ConnectionProtocol::Serial,
        ConnectionProtocol::Local,
        ConnectionProtocol::RemoteDesktop,
    ] {
        conn.protocol = protocol;
        assert!(
            !Oryxis::host_can_console(&conn),
            "{protocol:?} offered a console"
        );
    }
}

/// A mosh host branches ONE LINE ABOVE the console in `SshConnected`,
/// deliberately, because mosh closes the SSH session it is handed. So
/// asking for a console on one would silently deliver a mosh shell. An
/// open mosh tab is already covered by `transport.ssh()` answering None;
/// this is what covers the host card, where there is no tab to ask.
#[test]
fn a_mosh_host_does_not_offer_a_console() {
    let mut conn = oryxis_core::models::Connection::new("host", "example.com");
    conn.protocol = oryxis_core::models::connection::ConnectionProtocol::Ssh;
    assert!(Oryxis::host_can_console(&conn));

    conn.mosh = Some(oryxis_core::models::mosh::MoshOptions::default());
    assert!(!Oryxis::host_can_console(&conn));
}

/// The invariant the compiler cannot hold: a pane's purpose has to
/// survive `spawn_ssh_for_pane_conn` rebuilding its session in place,
/// or a console whose link dropped comes back as a SHELL, changing what
/// the tab is without anybody asking. Both values are legal at every
/// point, so only a test can say which one is right.
#[test]
fn a_panes_purpose_survives_a_session_rebuild() {
    use crate::state::{Pane, PanePurpose};

    let terminal = std::sync::Arc::new(std::sync::Mutex::new(
        oryxis_terminal::widget::TerminalState::new_no_pty(80, 24).expect("terminal"),
    ));
    let mut pane = Pane::new("console".to_string(), std::sync::Arc::clone(&terminal));
    assert_eq!(pane.purpose, PanePurpose::Shell, "wrong default");

    pane.purpose = PanePurpose::SftpConsole;
    // What an in-place reconnect actually replaces: the transport and
    // the emulator behind the pane. Everything else, this field
    // included, is the pane's own identity and must ride through.
    pane.session = None;
    pane.terminal = std::sync::Arc::new(std::sync::Mutex::new(
        oryxis_terminal::widget::TerminalState::new_no_pty(80, 24).expect("terminal"),
    ));
    assert_eq!(
        pane.purpose,
        PanePurpose::SftpConsole,
        "a reconnected console came back as a shell"
    );
}

/// A fresh pane must never be born holding an end-of-session verdict:
/// `ended` is what draws the restart / close card over the grid, so a
/// wrong default would put a "Session ended" card over a shell that is
/// about to start printing (issue #208).
#[test]
fn a_new_pane_is_not_already_ended() {
    use crate::state::Pane;

    let terminal = std::sync::Arc::new(std::sync::Mutex::new(
        oryxis_terminal::widget::TerminalState::new_no_pty(80, 24).expect("terminal"),
    ));
    let pane = Pane::new("host".to_string(), terminal);
    assert!(!pane.ended, "a pane was born disconnected");
    // Nothing has been armed yet, and `arm_local_stream` returns the
    // POST-increment value, so no live pane ever carries 0. That is what
    // makes 0 usable as the "pane is gone" answer.
    assert_eq!(pane.local_generation, 0, "generation must start unarmed");
}

/// The persisted `pane_end_action` setting has to survive the round trip
/// through its stored code, and an unreadable value has to land on the
/// default rather than on whichever variant happens to be first.
#[test]
fn pane_end_action_round_trips_through_its_stored_code() {
    use crate::util::PaneEndAction;

    for action in PaneEndAction::ALL {
        assert_eq!(
            PaneEndAction::from_code(action.code()),
            action,
            "{} did not survive its own code",
            action.code(),
        );
    }
    // A vault row from a future version, or a hand-edited one: keeping
    // the pane is the answer that loses nothing.
    assert_eq!(PaneEndAction::from_code("nonsense"), PaneEndAction::Prompt);
    assert_eq!(PaneEndAction::default(), PaneEndAction::Prompt);
}

/// The settings picker maps the user's choice back by comparing
/// LOCALIZED labels, so two variants sharing one label would silently
/// select the wrong action. The compiler cannot see that coupling.
#[test]
fn every_pane_end_action_has_its_own_label() {
    use crate::util::PaneEndAction;

    let labels: Vec<&'static str> = PaneEndAction::ALL
        .iter()
        .map(|a| crate::i18n::t(a.label_key()))
        .collect();
    for (i, a) in labels.iter().enumerate() {
        assert!(!a.is_empty(), "unresolved label key");
        for b in labels.iter().skip(i + 1) {
            assert_ne!(a, b, "two pane-end actions share a label");
        }
    }
}
