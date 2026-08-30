//! `Oryxis::handle_sidebar_files`, the terminal sidebar's Files tab.
//!
//! Was a 706-line match. The groups are the parts of the surface: the
//! list and how you move through it (`navigate`), the path header
//! (`path_bar`), the context menus (`menus`), the entry editors
//! (`entries`), the transfers (`transfer`), and the async listings
//! that feed all of it (`listing`).
//!
//! The helpers the arms share (path math, the pane lookups, the
//! `op_then_list` operation wrapper) stay here, since every group
//! reaches for them.

// The `Err(message)` pass-through of the try_handler! chain carries the full
// Message enum by design; same allowance as the sibling dispatch modules.
#![allow(clippy::result_large_err)]

// Dispatch sub-handlers, one file per arm family.
mod navigate;
mod path_bar;
mod menus;
mod entries;
mod transfer;
mod listing;

use iced::Task;
use uuid::Uuid;

use crate::app::Oryxis;
use crate::messages::{Message, SidebarFilesMessage, TabsMessage, SftpMessage};
use crate::state::TerminalSidebarTab;

/// Dirs first, then case-insensitive by name, the sidebar's fixed sort
/// (the full SFTP pane has sortable columns; this browser does not).
fn sort_entries(entries: &mut [oryxis_ssh::SftpEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Parent of an absolute POSIX path, `None` at the root.
pub(crate) fn files_parent_dir(path: &str) -> Option<String> {
    // Windows-style local path (issue #145): `C:\Users\x` parents to
    // `C:\Users`, and a drive root has no parent. Detected per path,
    // because the browser only ever sees a `\` path when it is showing
    // a Windows filesystem.
    if is_windows_path(path) {
        let trimmed = path.trim_end_matches(['\\', '/']);
        // "C:" (a bare drive) has nothing above it.
        if trimmed.len() <= 2 {
            return None;
        }
        let idx = trimmed.rfind(['\\', '/'])?;
        return Some(if idx <= 2 {
            // Direct child of the drive root: keep the trailing
            // separator (`C:\`), which is what listing expects.
            format!("{}\\", &trimmed[..2])
        } else {
            trimmed[..idx].to_string()
        });
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let idx = trimmed.rfind('/')?;
    Some(if idx == 0 { "/".to_string() } else { trimmed[..idx].to_string() })
}

/// Join an entry name onto the browser's current directory.
pub(crate) fn files_join(path: &str, name: &str) -> String {
    if is_windows_path(path) {
        return format!("{}\\{name}", path.trim_end_matches(['\\', '/']));
    }
    if path == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", path.trim_end_matches('/'))
    }
}

/// The final path component, whichever separator family the path uses.
pub(crate) fn files_basename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Size of `path` according to `entries`, the listing of `dir`, or
/// `None` when no file there IS that path.
///
/// The match rejoins each listed name onto the directory it was listed
/// in, because a name is only meaningful next to its directory: matching
/// `path.ends_with(&entry.name)` instead also accepted any sibling whose
/// name is a suffix of the target's (`content.zip` for `wp-content.zip`),
/// and the first such row in the listing won. Only a progress total
/// depends on it, so being wrong was quiet.
pub(crate) fn listed_size(
    entries: &[oryxis_ssh::SftpEntry],
    dir: &str,
    path: &str,
) -> Option<u64> {
    entries
        .iter()
        .find(|e| !e.is_dir && files_join(dir, &e.name) == path)
        .map(|e| e.size)
}

/// A `C:\...` / `\\server\share` shape, i.e. a Windows filesystem path
/// the local browser can show. Remote SFTP paths are always POSIX, so
/// this never misfires on them.
fn is_windows_path(path: &str) -> bool {
    let b = path.as_bytes();
    (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()) || path.starts_with("\\\\")
}

/// Working-directory fallback for shells without OSC 7 integration:
/// the stock Debian/Ubuntu/Fedora PS1 titles the window `\u@\h: \w`,
/// so an OSC 0/2 title like `root@web: /var/www` carries the cwd.
/// Extracts the trailing path (absolute or `~`-relative); anything
/// else (`vim main.rs`, plain program names) yields `None`.
pub(crate) fn title_cwd(title: &str) -> Option<&str> {
    // Preferred: the "\u@\h: \w" form (Debian/Ubuntu default, colon +
    // space). Fallback: the no-space "\u@\h:\w" some PS1s use, taken
    // only when the head looks like `user@host` so a stray "note:foo"
    // title can't masquerade as a cwd.
    let tail = title
        .rsplit_once(": ")
        .map(|(_, t)| t)
        .or_else(|| {
            let (head, t) = title.rsplit_once(':')?;
            head.contains('@').then_some(t)
        })
        .unwrap_or(title);
    let tail = tail.trim();
    (tail.starts_with('/') || tail == "~" || tail.starts_with("~/")).then_some(tail)
}

/// Expand a possibly `~`-relative cwd (the title fallback) against the
/// session's home directory. Absolute paths pass through; `~` without
/// a known home resolves to `None` (can't follow yet, the mount's
/// canonicalize will supply the home).
fn expand_cwd(cwd: &str, home: Option<&str>) -> Option<String> {
    if cwd.starts_with('/') {
        return Some(cwd.to_string());
    }
    let home = home?.trim_end_matches('/');
    if cwd == "~" {
        return Some(home.to_string());
    }
    cwd.strip_prefix("~/").map(|rest| format!("{home}/{rest}"))
}

/// Cap per host, matching the per-pane dropdown's own cap: the list is
/// there to be scanned, not archived.
const FILES_RECENT_CAP: usize = 20;

impl Oryxis {
    /// Record a visited folder in the persistent, host-keyed history and
    /// write it back. No-op for panes with no saved host (quick-connect,
    /// local), which have no stable key to file it under.
    fn record_files_recent(&mut self, pane_id: uuid::Uuid, path: &str) {
        if path.is_empty() {
            return;
        }
        let host = self
            .tabs
            .iter()
            .flat_map(|t| t.pane_grid.panes.values())
            .find(|p| p.id == pane_id)
            .and_then(|p| match p.origin {
                crate::state::PaneOrigin::Host(id) => Some(id),
                _ => None,
            });
        let Some(host) = host else {
            return;
        };
        let list = self.files_recent_folders.entry(host).or_default();
        if list.first().is_some_and(|p| p == path) {
            // Already on top: the optimistic navigate records the same
            // path twice (mount then listing), so this is the common case
            // and it must not cost a vault write.
            return;
        }
        list.retain(|p| p != path);
        list.insert(0, path.to_string());
        list.truncate(FILES_RECENT_CAP);
        // Encrypted, not a plain setting: the settings table is readable
        // without unlocking, and this is the user's directory trail on
        // every host. A write while the vault is soft-locked has no key,
        // so it just doesn't persist; the in-memory list still works for
        // the session.
        if let Ok(json) = serde_json::to_string(&self.files_recent_folders)
            && let Some(vault) = &self.vault
            && let Err(e) = vault.set_files_recent_folders(&json)
        {
            tracing::warn!("failed to persist the Files folder history: {e}");
        }
    }

    /// Refill a pane's dropdown from the stored history for its host.
    /// Called when the Files sidebar mounts, which is what makes the
    /// disconnect-time wipe harmless.
    fn hydrate_files_recent(&mut self, pane_id: uuid::Uuid) {
        let host = self
            .tabs
            .iter()
            .flat_map(|t| t.pane_grid.panes.values())
            .find(|p| p.id == pane_id)
            .and_then(|p| match p.origin {
                crate::state::PaneOrigin::Host(id) => Some(id),
                _ => None,
            });
        let Some(stored) = host.and_then(|h| self.files_recent_folders.get(&h)).cloned() else {
            return;
        };
        if let Some(pane) = self.pane_by_id_any_tab(pane_id)
            && pane.files.path_history.is_empty()
        {
            pane.files.path_history = stored;
        }
    }

    pub(crate) fn handle_sidebar_files(
        &mut self,
        message: SidebarFilesMessage,
    ) -> Task<Message> {
        match message {
            m @ (
                SidebarFilesMessage::SidebarFilesRowHovered(..)
                | SidebarFilesMessage::SidebarFilesRowUnhovered(..)
                | SidebarFilesMessage::SidebarFilesSelectRow(..)
                | SidebarFilesMessage::SidebarFilesToggleFollow
                | SidebarFilesMessage::SidebarFilesToggleHidden
                | SidebarFilesMessage::SidebarFilesRefresh
                | SidebarFilesMessage::SidebarFilesNavigate(..)
                | SidebarFilesMessage::SidebarFilesExpand
                | SidebarFilesMessage::SidebarFilesOpenSftpAt(..)
            ) => self.handle_sidebar_files_navigate(m),
            m @ (
                SidebarFilesMessage::SidebarFilesStartEditPath
                | SidebarFilesMessage::SidebarFilesEditPath(..)
                | SidebarFilesMessage::SidebarFilesEditBlur
                | SidebarFilesMessage::SidebarFilesPathHistoryToggle
                | SidebarFilesMessage::SidebarFilesPathHistoryClose
                | SidebarFilesMessage::SidebarFilesPathHistoryPick(..)
                | SidebarFilesMessage::SidebarFilesCommitPath
            ) => self.handle_sidebar_files_path_bar(m),
            m @ (
                SidebarFilesMessage::ShowSidebarFilesRowMenu(..)
                | SidebarFilesMessage::ShowSidebarFilesBackgroundMenu
                | SidebarFilesMessage::SidebarFilesShowProperties(..)
            ) => self.handle_sidebar_files_menus(m),
            m @ (
                SidebarFilesMessage::SidebarFilesStartRename(..)
                | SidebarFilesMessage::SidebarFilesRenameInput(..)
                | SidebarFilesMessage::SidebarFilesRenameCommit
                | SidebarFilesMessage::SidebarFilesStartNewEntry(..)
                | SidebarFilesMessage::SidebarFilesNewEntryInput(..)
                | SidebarFilesMessage::SidebarFilesNewEntryCommit
                | SidebarFilesMessage::SidebarFilesDelete(..)
                | SidebarFilesMessage::SidebarFilesDeleteConfirmed(..)
            ) => self.handle_sidebar_files_entries(m),
            m @ (
                SidebarFilesMessage::SidebarFilesDownload(..)
                | SidebarFilesMessage::SidebarFilesDownloadPicked(..)
                | SidebarFilesMessage::SidebarFilesUploadInto(..)
                | SidebarFilesMessage::SidebarFilesUploadPicked(..)
                | SidebarFilesMessage::SidebarFilesEdit(..)
                | SidebarFilesMessage::SidebarFilesOpToast(..)
            ) => self.handle_sidebar_files_transfer(m),
            m @ (
                SidebarFilesMessage::SidebarFilesMounted(..)
                | SidebarFilesMessage::SidebarFilesListed(..)
                | SidebarFilesMessage::SidebarFilesError(..)
            ) => self.handle_sidebar_files_listing(m),
        }
    }

    /// The active tab's focused pane, mutably. `None` outside a
    /// terminal tab.
    /// Whether the ACTIVE pane's Files browser shows the local
    /// filesystem (issue #145): its mounted backend says so, or (pre-
    /// mount) the pane is a local shell. Read by the menu builders to
    /// swap the transfer-shaped items for OS ones.
    pub(crate) fn sidebar_files_is_local(&self) -> bool {
        self.active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.active())
            .is_some_and(|p| {
                p.files.client.as_ref().is_some_and(|c| c.is_local())
                    || (p.session.is_none()
                        && matches!(p.origin, crate::state::PaneOrigin::Local(_)))
            })
    }

    pub(crate) fn active_pane_mut(&mut self) -> Option<&mut crate::state::Pane> {
        let idx = self.active_tab?;
        Some(self.tabs.get_mut(idx)?.active_mut())
    }

    /// Find a pane by its stable id across every tab (async results
    /// arrive after the user may have switched tabs / panes).
    pub(crate) fn pane_by_id_any_tab(&mut self, pane_id: Uuid) -> Option<&mut crate::state::Pane> {
        self.tabs
            .iter_mut()
            .flat_map(|t| t.pane_grid.panes.values_mut())
            .find(|p| p.id == pane_id)
    }

    /// Bring the visible Files browser in line with its pane: mount the
    /// SFTP channel if the tab just opened, or chase the shell's OSC 7
    /// cwd when follow is on. Idempotent and cheap when nothing needs
    /// doing, so every entry point (tab select, sidebar open, pane
    /// focus, cwd change) just calls it.
    pub(crate) fn sidebar_files_sync(&mut self) -> Task<Message> {
        // Only the visible browser drives SFTP traffic; a background
        // pane's cwd changes are picked up when its tab shows again.
        if !self.sidebar_tab_shown(TerminalSidebarTab::Files) {
            return Task::none();
        }
        let Some(pane) = self.active_pane_mut() else {
            return Task::none();
        };
        // A local shell browses the app's own filesystem (issue #145);
        // everything else needs the live SSH transport.
        let is_local = pane.session.is_none()
            && matches!(pane.origin, crate::state::PaneOrigin::Local(_));
        let ssh = pane.session.as_ref().and_then(|s| s.ssh()).cloned();
        if !is_local {
            let Some(ssh) = ssh.as_ref() else {
                return Task::none();
            };
            if !ssh.is_alive() {
                return Task::none();
            }
        }
        let pane_id = pane.id;

        // Not mounted yet: open the channel (or, locally, resolve the
        // home) and land on the shell's cwd (when following) or the
        // home directory.
        if pane.files.client.is_none() {
            if pane.files.mounting {
                return Task::none();
            }
            pane.files.mounting = true;
            pane.files.error = None;
            // The pre-mount hint can only use an absolute cwd (a
            // `~`-relative title fallback has no home to expand against
            // yet; the mount lands on the home anyway and the post-mount
            // chase in SidebarFilesMounted finishes the job). A local
            // shell's cwd is a native path, absolute as reported.
            let hint = if pane.files.follow() {
                if is_local {
                    pane.cwd
                        .clone()
                        .filter(|c| std::path::Path::new(c).is_absolute())
                } else {
                    pane.cwd.as_deref().and_then(|c| expand_cwd(c, None))
                }
            } else {
                None
            };
            let seq = pane.files.next_req();
            if is_local {
                return Task::perform(
                    async move {
                        let fs = crate::local_files::LocalFs;
                        let home = dirs::home_dir()
                            .map(|h| h.to_string_lossy().into_owned());
                        // Land on the hinted cwd when it lists, home
                        // otherwise; a machine with neither answers
                        // with the real error.
                        let mut start = hint
                            .or_else(|| home.clone())
                            .unwrap_or_else(|| "/".to_string());
                        start = fs.canonicalize(&start).await.unwrap_or(start);
                        let entries = match fs.list_dir(&start).await {
                            Ok(e) => e,
                            Err(first_err) => match home
                                .as_deref()
                                .filter(|h| **h != *start)
                            {
                                Some(h) => {
                                    start = h.to_string();
                                    fs.list_dir(h).await.map_err(|e| e.to_string())?
                                }
                                None => return Err(first_err.to_string()),
                            },
                        };
                        Ok::<_, String>((
                            crate::local_files::FilesClient::Local(fs),
                            home,
                            start,
                            entries,
                        ))
                    },
                    move |result| match result {
                        Ok((client, home, path, entries)) => {
                            Message::SidebarFiles(SidebarFilesMessage::SidebarFilesMounted(pane_id, seq, client, home, path, entries))
                        }
                        Err(e) => Message::SidebarFiles(SidebarFilesMessage::SidebarFilesError(pane_id, seq, e)),
                    },
                );
            }
            let ssh = ssh.expect("checked above");
            return Task::perform(
                async move {
                    let client = ssh.open_sftp().await.map_err(|e| e.to_string())?;
                    // Session home, resolved once: expands `~`-relative
                    // cwds from the title fallback.
                    let home = client.canonicalize(".").await.ok();
                    let (path, entries) =
                        crate::dispatch_sftp::initial_remote_listing(&client, hint).await?;
                    Ok::<_, String>((
                        crate::local_files::FilesClient::Sftp(client),
                        home,
                        path,
                        entries,
                    ))
                },
                move |result| match result {
                    Ok((client, home, path, entries)) => {
                        Message::SidebarFiles(SidebarFilesMessage::SidebarFilesMounted(pane_id, seq, client, home, path, entries))
                    }
                    Err(e) => Message::SidebarFiles(SidebarFilesMessage::SidebarFilesError(pane_id, seq, e)),
                },
            );
        }

        // Mounted: follow the shell if it moved. A local shell's cwd is
        // a native absolute path (`expand_cwd` only speaks POSIX, which
        // would refuse a `C:\` path on a Windows local shell).
        if pane.files.follow()
            && !pane.files.loading
            && let Some(cwd) = pane.cwd.as_deref().and_then(|c| {
                if is_local {
                    std::path::Path::new(c)
                        .is_absolute()
                        .then(|| c.to_string())
                        .or_else(|| expand_cwd(c, pane.files.home.as_deref()))
                } else {
                    expand_cwd(c, pane.files.home.as_deref())
                }
            })
            && cwd != pane.files.path
        {
            let client = pane.files.client.clone().expect("checked above");
            pane.files.loading = true;
            let seq = pane.files.next_req();
            return list_dir_task(client, cwd, pane_id, seq);
        }
        Task::none()
    }
}

/// Run a mutation (rename / create / delete) then re-list the current
/// directory, all on the sidebar browser's channel; the completion
/// carries the request stamp like any listing.
fn op_then_list(
    client: crate::local_files::FilesClient,
    list_path: String,
    pane_id: Uuid,
    seq: u64,
    op: impl std::future::Future<Output = Result<(), oryxis_ssh::SshError>> + Send + 'static,
) -> Task<Message> {
    Task::perform(
        async move {
            op.await.map_err(|e| e.to_string())?;
            let entries = client
                .list_dir(&list_path)
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((list_path, entries))
        },
        move |result| match result {
            Ok((path, entries)) => Message::SidebarFiles(SidebarFilesMessage::SidebarFilesListed(pane_id, seq, path, entries)),
            Err(e) => Message::SidebarFiles(SidebarFilesMessage::SidebarFilesError(pane_id, seq, e)),
        },
    )
}

/// One directory listing on the sidebar browser's channel. `seq` is the
/// request stamp compared on completion (latest request wins).
/// `pub(crate)`: the OS-drop upload refreshes the visible listing on
/// completion through this (`drop.rs`), pinned to its pane id rather
/// than riding `SidebarFilesRefresh`, whose "active pane" can have
/// changed during a long upload.
pub(crate) fn list_dir_task(
    client: crate::local_files::FilesClient,
    path: String,
    pane_id: Uuid,
    seq: u64,
) -> Task<Message> {
    Task::perform(
        async move {
            let entries = client.list_dir(&path).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((path, entries))
        },
        move |result| match result {
            Ok((path, entries)) => Message::SidebarFiles(SidebarFilesMessage::SidebarFilesListed(pane_id, seq, path, entries)),
            Err(e) => Message::SidebarFiles(SidebarFilesMessage::SidebarFilesError(pane_id, seq, e)),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, size: u64) -> oryxis_ssh::SftpEntry {
        oryxis_ssh::SftpEntry {
            name: name.to_string(),
            is_dir: false,
            is_symlink: false,
            size,
            mtime: None,
            permissions: None,
            uid: None,
            gid: None,
        }
    }

    #[test]
    fn listed_size_matches_the_whole_name_not_a_suffix() {
        let entries = vec![entry("content.zip", 10), entry("wp-content.zip", 999)];
        // The suffix sibling comes FIRST in the listing, which is what
        // made the old `ends_with` match report 10 bytes here.
        assert_eq!(
            listed_size(&entries, "/srv", "/srv/wp-content.zip"),
            Some(999)
        );
        assert_eq!(listed_size(&entries, "/srv", "/srv/content.zip"), Some(10));
        // A name that only LOOKS listed (another directory, a directory
        // row, an unknown file) has no size to report.
        assert_eq!(listed_size(&entries, "/srv", "/other/content.zip"), None);
        assert_eq!(listed_size(&entries, "/srv", "/srv/missing.zip"), None);
        let dirs = vec![oryxis_ssh::SftpEntry {
            is_dir: true,
            ..entry("backups", 4096)
        }];
        assert_eq!(listed_size(&dirs, "/srv", "/srv/backups"), None);
    }

    #[test]
    fn title_cwd_extracts_stock_ps1_titles() {
        // Stock Debian/Ubuntu PS1: \u@\h: \w
        assert_eq!(title_cwd("root@web-01: /var/www"), Some("/var/www"));
        assert_eq!(title_cwd("root@web-01: ~"), Some("~"));
        assert_eq!(title_cwd("u@h: ~/projects/api"), Some("~/projects/api"));
        // Colons inside the path segment: the LAST ": " wins.
        assert_eq!(title_cwd("u@h: /data/a: b"), None); // "b" is not a path
        assert_eq!(title_cwd("note: see: /etc"), Some("/etc"));
        // No-space "\u@\h:\w" form (the head has an '@' so it's trusted).
        assert_eq!(title_cwd("root@web-01:/var/www"), Some("/var/www"));
        assert_eq!(title_cwd("root@web-01:~"), Some("~"));
        // A bare absolute path as the whole title.
        assert_eq!(title_cwd("/srv/app"), Some("/srv/app"));
    }

    #[test]
    fn title_cwd_rejects_non_path_titles() {
        assert_eq!(title_cwd("vim main.rs"), None);
        assert_eq!(title_cwd("htop"), None);
        assert_eq!(title_cwd(""), None);
        assert_eq!(title_cwd("root@web-01"), None);
        // Windows-style path in a title is not a POSIX cwd.
        assert_eq!(title_cwd(r"cmd: C:\Users\x"), None);
        // A bare "~x" user-home form is ambiguous; declined.
        assert_eq!(title_cwd("u@h: ~other"), None);
    }

    #[test]
    fn expand_cwd_handles_absolute_and_home_relative() {
        assert_eq!(expand_cwd("/var/www", None).as_deref(), Some("/var/www"));
        assert_eq!(expand_cwd("~", Some("/root")).as_deref(), Some("/root"));
        assert_eq!(
            expand_cwd("~/a/b", Some("/home/u/")).as_deref(),
            Some("/home/u/a/b")
        );
        // Home unknown: `~` can't expand yet.
        assert_eq!(expand_cwd("~", None), None);
        assert_eq!(expand_cwd("~/x", None), None);
    }

    #[test]
    fn files_join_and_parent_are_inverse_at_the_root() {
        assert_eq!(files_join("/", "etc"), "/etc");
        assert_eq!(files_join("/var/www", "html"), "/var/www/html");
        assert_eq!(files_parent_dir("/etc").as_deref(), Some("/"));
        assert_eq!(files_parent_dir("/var/www/html").as_deref(), Some("/var/www"));
        assert_eq!(files_parent_dir("/"), None);
    }

    #[test]
    fn files_helpers_speak_windows_for_the_local_browser() {
        // Issue #145: a local shell on Windows browses `C:\` paths with
        // the same helpers the POSIX paths use.
        assert_eq!(files_join(r"C:\Users", "wilson"), r"C:\Users\wilson");
        assert_eq!(files_join(r"C:\", "Users"), r"C:\Users");
        assert_eq!(
            files_parent_dir(r"C:\Users\wilson").as_deref(),
            Some(r"C:\Users")
        );
        // A drive root's child parents to `C:\` (separator kept, which
        // is what a listing call expects), and the root itself to
        // nothing.
        assert_eq!(files_parent_dir(r"C:\Users").as_deref(), Some(r"C:\"));
        assert_eq!(files_parent_dir(r"C:\"), None);
        assert_eq!(files_basename(r"C:\Users\wilson"), "wilson");
        assert_eq!(files_basename("/var/www/html"), "html");
        // Remote POSIX paths never trip the detection.
        assert!(!is_windows_path("/var/c:d"));
        assert!(is_windows_path(r"C:\Users"));
    }
}
