//! Pure helpers for the SFTP transfer pipeline — pulled out of `app.rs`
//! to keep that file from growing past 7k lines. Everything here is a
//! free function (no `&self`) operating on owned data, so the move is
//! mechanical.
//!
//! Most callers are inside `app.rs::update`; the test module re-exports
//! these names through `pub(crate) use` so existing `app_tests.rs`
//! references stay valid.
//!
//! `pub(crate)` everywhere keeps the API internal — these aren't
//! intended for any consumer outside the app crate.

/// Resolution returned by the upload pre-flight task — either upload
/// completed silently, or we need to bounce back to the UI to ask the
/// user how to handle a name collision.
pub(crate) enum UploadOutcome {
    Done(String),
    Conflict(crate::state::OverwritePrompt),
}

/// Outcome of stepping through one upload-queue item: either it
/// completed (file written or dir created), or the destination already
/// exists and the user has to pick what to do next via the overwrite
/// modal.
pub(crate) enum UploadStepOutcome {
    Done,
    Conflict {
        prompt: crate::state::OverwritePrompt,
        /// The item that was popped — kept around so the resolve
        /// handler can re-apply the action to the right destination.
        item: crate::state::TransferItem,
    },
}

/// Spin up `concurrency-1` extra SFTP subsystem channels alongside the
/// caller's client. Slot 0 reuses the original client (cheap), slots
/// 1..N are fresh siblings on the same SSH connection. Used by every
/// transfer that wants to pump bytes in parallel.
pub(crate) async fn build_client_pool(
    primary: oryxis_ssh::SftpClient,
    concurrency: u8,
) -> Result<Vec<oryxis_ssh::SftpClient>, String> {
    let mut pool = Vec::with_capacity(concurrency as usize);
    pool.push(primary);
    for _ in 1..concurrency {
        let sibling = pool[0]
            .open_sibling()
            .await
            .map_err(|e| e.to_string())?;
        pool.push(sibling);
    }
    Ok(pool)
}

/// Join a basename onto a POSIX directory path, handling the root case
/// (which would otherwise produce `//foo`).
pub(crate) fn remote_join(dir: &str, basename: &str) -> String {
    if dir == "/" {
        format!("/{}", basename)
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), basename)
    }
}

/// Short label for a transfer queue item — just the basename for files,
/// trailing slash for dirs so the user can tell them apart in the
/// progress strip.
pub(crate) fn transfer_item_label(item: &crate::state::TransferItem) -> String {
    let raw = item
        .src
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(&item.src);
    if item.is_dir {
        format!("{}/", raw)
    } else {
        raw.to_string()
    }
}

/// Walk a local directory tree depth-first and append entries to `queue`
/// as `(local_src, remote_dst, is_dir)` triples. Caller is expected to
/// have already pushed the root directory itself; this only enumerates
/// children.
pub(crate) fn walk_local_for_upload(
    src: &std::path::Path,
    dst: &str,
    queue: &mut std::collections::VecDeque<crate::state::TransferItem>,
) -> Result<(), String> {
    let read = std::fs::read_dir(src).map_err(|e| format!("read_dir {}: {e}", src.display()))?;
    for entry in read.flatten() {
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_src = src.join(&name);
        let child_dst = format!("{}/{}", dst, name);
        if metadata.is_dir() {
            queue.push_back(crate::state::TransferItem {
                src: child_src.to_string_lossy().into_owned(),
                dst: child_dst.clone(),
                is_dir: true,
            });
            walk_local_for_upload(&child_src, &child_dst, queue)?;
        } else {
            queue.push_back(crate::state::TransferItem {
                src: child_src.to_string_lossy().into_owned(),
                dst: child_dst,
                is_dir: false,
            });
        }
    }
    Ok(())
}

/// Walk a local directory tree for a same-side copy. The `dst` is a
/// local path here, not a remote one.
pub(crate) fn walk_local_for_duplicate(
    src: &std::path::Path,
    dst: &std::path::Path,
    queue: &mut std::collections::VecDeque<crate::state::TransferItem>,
) -> Result<(), String> {
    let read = std::fs::read_dir(src).map_err(|e| format!("read_dir {}: {e}", src.display()))?;
    for entry in read.flatten() {
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let child_src = src.join(&name);
        let child_dst = dst.join(&name);
        if metadata.is_dir() {
            queue.push_back(crate::state::TransferItem {
                src: child_src.to_string_lossy().into_owned(),
                dst: child_dst.to_string_lossy().into_owned(),
                is_dir: true,
            });
            walk_local_for_duplicate(&child_src, &child_dst, queue)?;
        } else {
            queue.push_back(crate::state::TransferItem {
                src: child_src.to_string_lossy().into_owned(),
                dst: child_dst.to_string_lossy().into_owned(),
                is_dir: false,
            });
        }
    }
    Ok(())
}

/// Walk a remote directory via SFTP and enqueue each entry. Recursive
/// async fns require boxed pinning since the future can't reference its
/// own type at compile time without indirection.
pub(crate) fn walk_remote_for_download<'a>(
    client: &'a oryxis_ssh::SftpClient,
    src: &'a str,
    dst: &'a std::path::Path,
    queue: &'a mut std::collections::VecDeque<crate::state::TransferItem>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        let entries = client.list_dir(src).await.map_err(|e| e.to_string())?;
        for entry in entries {
            let child_src = if src == "/" {
                format!("/{}", entry.name)
            } else {
                format!("{}/{}", src.trim_end_matches('/'), entry.name)
            };
            let child_dst = dst.join(&entry.name);
            if entry.is_dir {
                queue.push_back(crate::state::TransferItem {
                    src: child_src.clone(),
                    dst: child_dst.to_string_lossy().into_owned(),
                    is_dir: true,
                });
                walk_remote_for_download(client, &child_src, &child_dst, queue).await?;
            } else {
                queue.push_back(crate::state::TransferItem {
                    src: child_src,
                    dst: child_dst.to_string_lossy().into_owned(),
                    is_dir: false,
                });
            }
        }
        Ok(())
    })
}

/// Apply a single upload-queue item with conflict awareness. Files
/// existence-check the destination; if a conflict comes up and there's
/// a sticky default action, apply it; otherwise return a Conflict outcome
/// for the caller to surface in the modal.
pub(crate) async fn do_upload_item(
    client: oryxis_ssh::SftpClient,
    item: crate::state::TransferItem,
    overwrite_default: Option<crate::state::OverwriteAction>,
    multi: bool,
) -> Result<UploadStepOutcome, String> {
    if item.is_dir {
        // `create_dir` errors when the dir already exists; harmless for
        // recursive uploads since later child writes need it to be
        // present. Real "no such parent" failures surface via the
        // child write_file calls.
        let _ = client.create_dir(&item.dst).await;
        return Ok(UploadStepOutcome::Done);
    }
    let parent = parent_path(&item.dst);
    let basename = item
        .dst
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(&item.dst)
        .to_string();
    let entries = client
        .list_dir(&parent)
        .await
        .map_err(|e| e.to_string())?;
    let conflict = entries.iter().find(|e| e.name == basename);
    if let Some(existing) = conflict {
        if let Some(action) = overwrite_default {
            apply_overwrite_for_item(client.clone(), item.clone(), action).await?;
            return Ok(UploadStepOutcome::Done);
        }
        let src_size = tokio::fs::metadata(&item.src)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let prompt = crate::state::OverwritePrompt {
            src: std::path::PathBuf::from(&item.src),
            dst_dir: parent,
            basename,
            src_size,
            dst_size: existing.size,
            multi,
            apply_to_all: false,
        };
        return Ok(UploadStepOutcome::Conflict { prompt, item });
    }
    let bytes = tokio::fs::read(&item.src)
        .await
        .map_err(|e| format!("read {}: {e}", item.src))?;
    client
        .write_file(&item.dst, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(UploadStepOutcome::Done)
}

/// Apply a chosen overwrite action to a single transfer item. Callable
/// both inside the queue runner (when a sticky default is set) and from
/// the resolve handler (when the user picked an action manually).
pub(crate) async fn apply_overwrite_for_item(
    client: oryxis_ssh::SftpClient,
    item: crate::state::TransferItem,
    action: crate::state::OverwriteAction,
) -> Result<(), String> {
    match action {
        crate::state::OverwriteAction::Cancel => Ok(()),
        crate::state::OverwriteAction::Replace => {
            let bytes = tokio::fs::read(&item.src)
                .await
                .map_err(|e| format!("read {}: {e}", item.src))?;
            client
                .write_file(&item.dst, &bytes)
                .await
                .map_err(|e| e.to_string())
        }
        crate::state::OverwriteAction::ReplaceIfDifferent => {
            let local_size = tokio::fs::metadata(&item.src)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            let parent = parent_path(&item.dst);
            let basename = item
                .dst
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(&item.dst)
                .to_string();
            let entries = client
                .list_dir(&parent)
                .await
                .map_err(|e| e.to_string())?;
            let remote_size = entries
                .iter()
                .find(|e| e.name == basename)
                .map(|e| e.size)
                .unwrap_or(0);
            if local_size == remote_size {
                return Ok(());
            }
            let bytes = tokio::fs::read(&item.src)
                .await
                .map_err(|e| format!("read {}: {e}", item.src))?;
            client
                .write_file(&item.dst, &bytes)
                .await
                .map_err(|e| e.to_string())
        }
        crate::state::OverwriteAction::Duplicate => {
            let parent = parent_path(&item.dst);
            let basename = item
                .dst
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(&item.dst)
                .to_string();
            let entries = client
                .list_dir(&parent)
                .await
                .map_err(|e| e.to_string())?;
            let names: std::collections::HashSet<String> =
                entries.into_iter().map(|e| e.name).collect();
            let unique = unique_entry_name(&basename, |n| !names.contains(n));
            let target = remote_join(&parent, &unique);
            let bytes = tokio::fs::read(&item.src)
                .await
                .map_err(|e| format!("read {}: {e}", item.src))?;
            client
                .write_file(&target, &bytes)
                .await
                .map_err(|e| e.to_string())
        }
    }
}

pub(crate) async fn do_download_item(
    client: oryxis_ssh::SftpClient,
    item: crate::state::TransferItem,
) -> Result<(), String> {
    if item.is_dir {
        tokio::fs::create_dir_all(&item.dst)
            .await
            .map_err(|e| format!("mkdir {}: {e}", item.dst))
    } else {
        let bytes = client
            .read_file(&item.src)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::write(&item.dst, &bytes)
            .await
            .map_err(|e| format!("write {}: {e}", item.dst))
    }
}

pub(crate) fn do_local_duplicate_item(
    item: &crate::state::TransferItem,
) -> Result<(), String> {
    if item.is_dir {
        std::fs::create_dir_all(&item.dst).map_err(|e| format!("mkdir {}: {e}", item.dst))
    } else {
        std::fs::copy(&item.src, &item.dst)
            .map(|_| ())
            .map_err(|e| format!("copy {} → {}: {e}", item.src, item.dst))
    }
}

/// Pick a name that doesn't collide with any existing entry in the same
/// directory — `name.ext` → `name copy.ext`, then `name copy 2.ext`,
/// `name copy 3.ext`, … if those are taken too. Mirrors macOS Finder.
/// Caller supplies the membership predicate so the helper works for both
/// local listings and remote SFTP listings.
pub(crate) fn unique_entry_name(basename: &str, is_free: impl Fn(&str) -> bool) -> String {
    if is_free(basename) {
        return basename.to_string();
    }
    let (stem, ext) = match basename.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{}", e)),
        _ => (basename.to_string(), String::new()),
    };
    let first = format!("{} copy{}", stem, ext);
    if is_free(&first) {
        return first;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{} copy {}{}", stem, n, ext);
        if is_free(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Final path component, suitable for prefilling a rename dialog. Pane
/// side disambiguates the separator convention (POSIX `/` for remote vs.
/// platform-native for local).
pub(crate) fn file_basename(
    path: &str,
    side: crate::state::SftpPaneSide,
) -> String {
    match side {
        crate::state::SftpPaneSide::Local => std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        crate::state::SftpPaneSide::Remote => path
            .rsplit_once('/')
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| path.to_string()),
    }
}

/// Strip the last path segment from a POSIX-style path (used by the SFTP
/// pane). Returns "/" when the input is the root.
pub(crate) fn parent_path(path: &str) -> String {
    if path == "/" || path.is_empty() {
        return "/".to_string();
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_string(),
        Some(idx) => trimmed[..idx].to_string(),
        None => "/".to_string(),
    }
}

/// Sort SFTP entries the way every file manager does: directories
/// (and dir-symlinks) first, then plain files, each group sorted by
/// the user-selected column / direction. Symlinks are bucketed with
/// directories since the user can't tell from a listing alone whether
/// they point to a file or a dir, and treating them as nav-able feels
/// less surprising.
pub(crate) fn sort_remote_entries(
    entries: &mut [oryxis_ssh::SftpEntry],
    sort: crate::state::SftpSort,
) {
    use crate::state::SftpSortColumn;
    entries.sort_by(|a, b| {
        let a_dir = a.is_dir || a.is_symlink;
        let b_dir = b.is_dir || b.is_symlink;
        let group_cmp = b_dir.cmp(&a_dir);
        if group_cmp != std::cmp::Ordering::Equal {
            return group_cmp;
        }
        let cmp = match sort.column {
            SftpSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SftpSortColumn::Size => a.size.cmp(&b.size),
            SftpSortColumn::Modified => a.mtime.unwrap_or(0).cmp(&b.mtime.unwrap_or(0)),
        };
        if sort.ascending { cmp } else { cmp.reverse() }
    });
}

pub(crate) fn sort_local_entries(
    entries: &mut [crate::state::LocalEntry],
    sort: crate::state::SftpSort,
) {
    use crate::state::SftpSortColumn;
    entries.sort_by(|a, b| {
        let group_cmp = b.is_dir.cmp(&a.is_dir);
        if group_cmp != std::cmp::Ordering::Equal {
            return group_cmp;
        }
        let cmp = match sort.column {
            SftpSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SftpSortColumn::Size => a.size.cmp(&b.size),
            SftpSortColumn::Modified => a
                .modified
                .unwrap_or(std::time::UNIX_EPOCH)
                .cmp(&b.modified.unwrap_or(std::time::UNIX_EPOCH)),
        };
        if sort.ascending { cmp } else { cmp.reverse() }
    });
}
