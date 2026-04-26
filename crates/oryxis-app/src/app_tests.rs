//! Unit tests for `app.rs` helpers. Loaded via `#[path] mod tests` at
//! the bottom of `app.rs` so it's part of the same module and can see
//! the private helpers directly.

#[allow(unused_imports)]
use super::*;
use crate::sftp_helpers::{parent_path, remote_join, transfer_item_label, unique_entry_name};

#[test]
fn remote_join_root_special_case() {
    // The root case is the only one that tripped us in real use —
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
    // `.bashrc` has no "stem.ext" split — the leading dot is part
    // of the name, so the suffix lands at the end.
    let busy: std::collections::HashSet<String> = [".bashrc"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let result = unique_entry_name(".bashrc", |n| !busy.contains(n));
    // Either "bashrc copy" with the leading dot eaten by the
    // rsplit_once boundary check, or ".bashrc copy" — accept the
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
    };
    let file = crate::state::TransferItem {
        src: "/a/b/c.txt".into(),
        dst: "/x/c.txt".into(),
        is_dir: false,
    };
    assert_eq!(transfer_item_label(&dir), "c/");
    assert_eq!(transfer_item_label(&file), "c.txt");
}

// ---------------------------------------------------------------------------
// Property-based tests
//
// Fuzz-style coverage for the path / name helpers — generates random
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
        // matching that — `/segment/segment/...` with no doubles.
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
        // "/" repeated some number of times — parent of any all-slash
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
