//! A one-line marker another process bumps after it writes the vault,
//! so a running app knows to re-read its in-memory lists.
//!
//! The app caches every entity list in memory and only reloads on its
//! own writes (and after a snapshot sync round). `oryxis-mcp` opens the
//! same file from another process, so a host it creates or a key it
//! pins would stay invisible until the next unlock. This marker is the
//! shout across the gap: the writer bumps it, every app instance polls
//! it (there can be several, one per window, so the deep-link inbox's
//! rename-to-claim would starve all but one) and reloads when its
//! content changed since the last look. A stale marker from a previous
//! day fires nothing: a watch seeds itself with whatever is there when
//! it starts.
//!
//! The file lives in the app's runtime folder (`~/.oryxis/runtime/`),
//! beside the deep-link inbox, and is written atomically (temp +
//! rename) so a reader never sees a torn line.

use std::path::{Path, PathBuf};

/// File name under the runtime folder.
pub const MARKER_FILE: &str = "vault-changed";

/// Where the marker lives, `None` when the home directory is unknown.
pub fn marker_path() -> Option<PathBuf> {
    oryxis_core::paths::oryxis_dir().map(|d| d.join("runtime").join(MARKER_FILE))
}

/// What a bump says: who wrote the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub writer: String,
}

/// Bump the marker on behalf of `writer` (a short process name, no
/// spaces). Creates the runtime folder when absent.
pub fn announce(writer: &str) -> std::io::Result<()> {
    let Some(path) = marker_path() else {
        return Err(std::io::Error::other("home directory unknown"));
    };
    announce_at(&path, writer)
}

/// [`announce`] against an explicit path (tests, and callers that
/// resolved the runtime folder themselves).
pub fn announce_at(path: &Path, writer: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Nanoseconds since the epoch plus the pid: distinct for every bump
    // a reader can tell apart, monotonic enough that two writers never
    // collide on the same content.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let line = format!("{nanos} {} {}\n", std::process::id(), writer.trim());
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, line.as_bytes())?;
    std::fs::rename(&tmp, path)
}

/// A reader's position: the marker content it last saw.
#[derive(Debug, Default)]
pub struct ChangeWatch {
    path: Option<PathBuf>,
    last: Option<String>,
}

impl ChangeWatch {
    /// Start watching the app's marker, seeded with its current content
    /// so a bump from before this process started is not reported.
    pub fn new() -> Self {
        Self::at(marker_path())
    }

    /// [`ChangeWatch::new`] against an explicit path (`None` watches
    /// nothing and never fires).
    pub fn at(path: Option<PathBuf>) -> Self {
        let last = path.as_deref().and_then(read_marker);
        Self { path, last }
    }

    /// The bump since the last poll, if any. Cheap: one small read.
    pub fn poll(&mut self) -> Option<Notice> {
        let path = self.path.as_deref()?;
        let current = read_marker(path)?;
        if self.last.as_deref() == Some(current.as_str()) {
            return None;
        }
        self.last = Some(current.clone());
        let writer = current
            .split_whitespace()
            .nth(2)
            .unwrap_or("unknown")
            .to_string();
        Some(Notice { writer })
    }
}

fn read_marker(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_started_after_a_bump_stays_quiet_until_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime").join(MARKER_FILE);
        announce_at(&path, "earlier").unwrap();
        let mut watch = ChangeWatch::at(Some(path.clone()));
        assert_eq!(watch.poll(), None, "the bump predates the watch");
        announce_at(&path, "oryxis-mcp").unwrap();
        assert_eq!(
            watch.poll(),
            Some(Notice { writer: "oryxis-mcp".into() })
        );
        assert_eq!(watch.poll(), None, "reported once");
    }

    #[test]
    fn a_missing_marker_is_not_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime").join(MARKER_FILE);
        let mut watch = ChangeWatch::at(Some(path.clone()));
        assert_eq!(watch.poll(), None);
        announce_at(&path, "x").unwrap();
        assert!(watch.poll().is_some());
        assert!(!path.with_extension(format!("tmp-{}", std::process::id())).exists());
    }

    #[test]
    fn nothing_to_watch_never_fires() {
        let mut watch = ChangeWatch::at(None);
        assert_eq!(watch.poll(), None);
    }
}
