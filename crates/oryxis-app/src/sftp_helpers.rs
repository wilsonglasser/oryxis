//! Pure helpers for the SFTP transfer pipeline, pulled out of `app.rs`
//! to keep that file from growing past 7k lines. Everything here is a
//! free function (no `&self`) operating on owned data, so the move is
//! mechanical.
//!
//! Most callers are inside `app.rs::update`; the test module re-exports
//! these names through `pub(crate) use` so existing `app_tests.rs`
//! references stay valid.
//!
//! `pub(crate)` everywhere keeps the API internal, these aren't
//! intended for any consumer outside the app crate.

/// Outcome of stepping through one queue item, in either direction:
/// either it completed (file written or dir created), or the destination
/// already exists and the user has to pick what to do next via the
/// overwrite modal.
pub(crate) enum TransferStepOutcome {
    Done,
    Conflict {
        prompt: crate::state::OverwritePrompt,
        /// The item that was popped, kept around so the resolve
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

/// Every upload the app performs, so the scratch-name setting cannot be
/// honoured by some paths and forgotten by others.
///
/// There is no second way to send a file from this crate: `upload_from`
/// and friends are the engine's API, and reaching for them directly here
/// is how a global setting quietly stops being global. Six call sites
/// already disagreed before this existed (the queue runner, single-file
/// upload, conflict resolution's replace and duplicate arms, archive
/// extraction and OS drops), which is five more than it takes for a user
/// to notice that a checkbox does nothing on the path they use.
pub(crate) async fn upload_one(
    client: &oryxis_ssh::SftpClient,
    local: &std::path::Path,
    remote: &str,
    temp_name: bool,
    progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
) -> Result<(), String> {
    client
        .upload_from_options(
            local,
            remote,
            oryxis_ssh::UploadOptions {
                progress,
                temp_name,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.to_string())
}

/// True when a directory-listing entry name from the SFTP server is a
/// single plain path component. Anything else (separators, `..`,
/// absolute paths, drive prefixes) would let a hostile server steer
/// the recursive walks outside the destination the user picked, so
/// such entries are skipped rather than joined onto a local path.
///
/// The rule itself lives in `oryxis_ssh`, next to the `list_dir` that
/// mints the names, so the SFTP console guards with the same predicate
/// these call sites do instead of a second copy of it.
pub(crate) fn is_safe_remote_entry_name(name: &str) -> bool {
    oryxis_ssh::sftp::is_safe_entry_name(name)
}

/// Slack left underneath a download so the volume never lands at
/// exactly zero. The free-space number is a snapshot and other
/// processes keep writing during a long transfer, so a check with no
/// margin still ends in ENOSPC (with a part file to clean up) often
/// enough to be worth the margin.
const DOWNLOAD_HEADROOM: u64 = 64 * 1024 * 1024;

/// Refuse a download that does not fit on the destination volume.
///
/// The remote peer decides both the size and how many files there are,
/// so this is the one place the sizes are known BEFORE anything is
/// written: a listing carries them, and the walk sums them. Failing
/// here costs the user a message; failing at 90% costs them the disk,
/// a stranded `.oryxis-part`, and no explanation.
///
/// Permissive when the platform will not answer (see
/// `oryxis_core::disk::fits`): an unavailable probe is not evidence of
/// a full disk.
pub(crate) fn ensure_local_space(dir: &std::path::Path, need: u64) -> Result<(), String> {
    if oryxis_core::disk::fits(dir, need, DOWNLOAD_HEADROOM) {
        return Ok(());
    }
    let free = oryxis_core::disk::available_space(dir).unwrap_or(0);
    Err(format!(
        "{} ({} / {})",
        crate::i18n::t("sftp_not_enough_space"),
        crate::views::sftp::format_size(need),
        crate::views::sftp::format_size(free),
    ))
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

/// Short label for a transfer queue item, just the basename for files,
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
                size: None,
            });
            walk_local_for_upload(&child_src, &child_dst, queue)?;
        } else {
            queue.push_back(crate::state::TransferItem {
                src: child_src.to_string_lossy().into_owned(),
                dst: child_dst,
                is_dir: false,
                // Carry the byte size so the transfer's total is known up
                // front and the progress bar can advance by bytes.
                size: Some(metadata.len()),
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
                size: None,
            });
            walk_local_for_duplicate(&child_src, &child_dst, queue)?;
        } else {
            queue.push_back(crate::state::TransferItem {
                src: child_src.to_string_lossy().into_owned(),
                dst: child_dst.to_string_lossy().into_owned(),
                is_dir: false,
                size: None,
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
            if !is_safe_remote_entry_name(&entry.name) {
                tracing::warn!("sftp download: skipping unsafe entry name {:?} in {src}", entry.name);
                continue;
            }
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
                    size: None,
                });
                walk_remote_for_download(client, &child_src, &child_dst, queue).await?;
            } else {
                queue.push_back(crate::state::TransferItem {
                    src: child_src,
                    dst: child_dst.to_string_lossy().into_owned(),
                    is_dir: false,
                    size: Some(entry.size),
                });
            }
        }
        Ok(())
    })
}

/// Walk a remote directory via SFTP for a server-to-server relay. Like
/// `walk_remote_for_download`, but both ends are remote: `dst` is a
/// destination-host POSIX path (not a local path), and each enqueued
/// item's `dst` is built by joining onto it.
pub(crate) fn walk_remote_for_relay<'a>(
    client: &'a oryxis_ssh::SftpClient,
    src: &'a str,
    dst: &'a str,
    queue: &'a mut std::collections::VecDeque<crate::state::TransferItem>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        let entries = client.list_dir(src).await.map_err(|e| e.to_string())?;
        for entry in entries {
            if !is_safe_remote_entry_name(&entry.name) {
                tracing::warn!("sftp relay: skipping unsafe entry name {:?} in {src}", entry.name);
                continue;
            }
            let child_src = if src == "/" {
                format!("/{}", entry.name)
            } else {
                format!("{}/{}", src.trim_end_matches('/'), entry.name)
            };
            let child_dst = remote_join(dst, &entry.name);
            if entry.is_dir {
                queue.push_back(crate::state::TransferItem {
                    src: child_src.clone(),
                    dst: child_dst.clone(),
                    is_dir: true,
                    size: None,
                });
                walk_remote_for_relay(client, &child_src, &child_dst, queue).await?;
            } else {
                queue.push_back(crate::state::TransferItem {
                    src: child_src,
                    dst: child_dst,
                    is_dir: false,
                    size: Some(entry.size),
                });
            }
        }
        Ok(())
    })
}

/// Apply a single relay-queue item: create the destination directory on
/// the dest host, or stream a single file from the source host to the
/// dest host via `relay_to`. `relay_to` opens the destination with
/// TRUNCATE, so an existing same-named file is overwritten silently;
/// the queue-building step picks a non-colliding root name to avoid the
/// common case, but nested collisions are not prompted (a known v1 gap).
pub(crate) async fn do_relay_item(
    src_client: oryxis_ssh::SftpClient,
    dst_client: oryxis_ssh::SftpClient,
    item: crate::state::TransferItem,
    progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    verify: bool,
) -> Result<(), String> {
    if item.is_dir {
        // A pre-existing directory is fine (the relay merges into it);
        // anything else (permission denied, path taken by a file) poisons
        // every child below it, so surface the mkdir error itself instead
        // of the cascade of child failures it would cause.
        if let Err(e) = dst_client.create_dir(&item.dst).await
            && dst_client.list_dir(&item.dst).await.is_err()
        {
            return Err(e.to_string());
        }
        Ok(())
    } else {
        src_client
            .relay_to_progress(&item.src, &dst_client, &item.dst, item.size, progress)
            .await
            .map_err(|e| e.to_string())?;
        if verify {
            verify_relayed_size(&src_client, &dst_client, &item).await?;
        }
        Ok(())
    }
}

/// Confirm a relayed file actually landed whole, by asking the
/// destination host how big it now is and comparing with the source.
///
/// Only a MOVE pays for this round trip, and it pays because the next
/// thing a move does is delete the original: `Ok(())` from the transfer
/// means "no error was reported", which is not the same claim as "the
/// bytes are on the other host". Nothing is ever removed on the weaker
/// claim (issue #115).
async fn verify_relayed_size(
    src_client: &oryxis_ssh::SftpClient,
    dst_client: &oryxis_ssh::SftpClient,
    item: &crate::state::TransferItem,
) -> Result<(), String> {
    let expected = match item.size {
        Some(s) => s,
        // No hint from the listing: ask the source. A source that can no
        // longer be stat'ed is itself a reason to keep it.
        None => src_client
            .stat(&item.src)
            .await
            .map_err(|e| format!("{}: {e}", crate::i18n::t("sftp_move_verify_failed")))?
            .size,
    };
    let landed = dst_client
        .stat(&item.dst)
        .await
        .map_err(|e| format!("{}: {e}", crate::i18n::t("sftp_move_verify_failed")))?
        .size;
    if landed != expected {
        return Err(format!(
            "{} ({}: {expected}, {}: {landed})",
            crate::i18n::t("sftp_move_size_mismatch"),
            item.src,
            item.dst,
        ));
    }
    Ok(())
}

/// True when two directory paths on the SAME host resolve to one
/// directory, so a move between them would have nothing to move.
///
/// Cheap first: identical strings need no round trip. Otherwise both
/// sides are canonicalized, which is what catches `/home/me`,
/// `/home/me/`, and a symlinked spelling of the same folder. Answers
/// `false` when either side fails to resolve, because a guess that
/// refuses is worse than one that lets the transfer report its own
/// error (issue #115).
pub(crate) async fn destinations_are_one_directory(
    a_client: &oryxis_ssh::SftpClient,
    a_dir: &str,
    b_client: &oryxis_ssh::SftpClient,
    b_dir: &str,
) -> bool {
    if a_dir.trim_end_matches('/') == b_dir.trim_end_matches('/') {
        return true;
    }
    match (
        a_client.canonicalize(a_dir).await,
        b_client.canonicalize(b_dir).await,
    ) {
        (Ok(a), Ok(b)) => a.trim_end_matches('/') == b.trim_end_matches('/'),
        _ => false,
    }
}

/// True when a folder relay's destination root would land inside its own
/// source tree, which is only ever a mistake: the copy nests the folder
/// into itself (`/srv/data` becomes `/srv/data/data`).
///
/// Not destructive today, because the tree is walked into a queue before
/// anything is written and the walk therefore cannot see the copy it is
/// producing, so there is no runaway recursion. It is still nonsense,
/// and it is the one same-file-family mistake the queue builder's
/// unique-name step does not already prevent (issue #115).
///
/// Both paths must already be resolved on the SAME host: this is pure
/// string containment, so a symlinked spelling of the same directory
/// would walk straight past it. The caller feeds it
/// [`resolved_path`] output for exactly that reason, which is also what
/// the sibling guard `destinations_are_one_directory` does.
pub(crate) fn relay_target_is_inside_source(src_root: &str, dst_root: &str) -> bool {
    let src = src_root.trim_end_matches('/');
    let dst = dst_root.trim_end_matches('/');
    if src.is_empty() {
        // Source is the filesystem root: everything is inside it.
        return true;
    }
    dst == src || dst.starts_with(&format!("{src}/"))
}

/// A path with every symlink resolved, or the path itself when the server
/// will not say.
///
/// The containment guard below compares strings, so `/srv/link/x` and
/// `/srv/data/x` are different paths to it even when `link` IS `data`.
/// Resolving first closes that, and falling back to the original on
/// failure keeps the guard's error asymmetry intact: a missed detection
/// leaves today's behaviour, while refusing a transfer we could not
/// prove anything about would break a legitimate one.
pub(crate) async fn resolved_path(client: &oryxis_ssh::SftpClient, path: &str) -> String {
    client
        .canonicalize(path)
        .await
        .unwrap_or_else(|_| path.to_string())
}

/// Remove the source side of a completed MOVE.
///
/// Runs only from the finalize arm, which is unreachable unless every
/// queue item copied and verified. Files go first, then directories
/// deepest first, and directories are removed with `remove_dir`, NOT
/// recursively: if something appeared in a source folder while the copy
/// was running, that folder is not empty, the removal fails loudly and
/// the new data survives. A recursive delete here would silently take it
/// (issue #115).
pub(crate) async fn remove_moved_sources(
    client: oryxis_ssh::SftpClient,
    sources: Vec<crate::state::TransferItem>,
) -> Result<usize, String> {
    let mut removed = 0usize;
    for (path, is_dir) in moved_source_removal_order(&sources) {
        let outcome = if is_dir {
            client.remove_dir(&path).await
        } else {
            client.remove_file(&path).await
        };
        outcome.map_err(|e| {
            format!(
                "{} ({path}): {e}",
                crate::i18n::t("sftp_move_cleanup_failed")
            )
        })?;
        removed += 1;
    }
    Ok(removed)
}

/// Order the source paths of a completed move for removal: every file
/// first, then directories deepest first.
///
/// The order is the whole safety property. A directory can only be
/// removed once it is empty, so its own children have to go before it,
/// and a deeper directory is always a child of a shallower one within
/// one moved tree. Depth by separator count is exact here because the
/// walk produces absolute POSIX paths under a single root.
///
/// Pure and separate from the I/O so the ordering can be asserted
/// without a server (issue #115).
fn moved_source_removal_order(sources: &[crate::state::TransferItem]) -> Vec<(String, bool)> {
    let mut files: Vec<&crate::state::TransferItem> =
        sources.iter().filter(|i| !i.is_dir).collect();
    files.sort_by(|a, b| a.src.cmp(&b.src));
    let mut dirs: Vec<&crate::state::TransferItem> = sources.iter().filter(|i| i.is_dir).collect();
    dirs.sort_by_key(|i| {
        (
            std::cmp::Reverse(i.src.trim_end_matches('/').matches('/').count()),
            i.src.clone(),
        )
    });
    files
        .into_iter()
        .chain(dirs)
        .map(|i| (i.src.clone(), i.is_dir))
        .collect()
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
    progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    temp_name: bool,
) -> Result<TransferStepOutcome, String> {
    if item.is_dir {
        // A pre-existing directory is fine (batch uploads merge into it,
        // confirmed by the list_dir probe); anything else (permission
        // denied, path taken by a file) poisons every child below it, so
        // surface the mkdir error itself instead of the cascade of child
        // failures it would cause. The queue runner's dir barrier
        // guarantees the parent already exists at this point.
        if let Err(e) = client.create_dir(&item.dst).await
            && client.list_dir(&item.dst).await.is_err()
        {
            return Err(e.to_string());
        }
        return Ok(TransferStepOutcome::Done);
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
            apply_overwrite_for_item(client.clone(), item.clone(), action, temp_name, progress.clone())
                .await?;
            return Ok(TransferStepOutcome::Done);
        }
        let src_size = tokio::fs::metadata(&item.src)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let prompt = crate::state::OverwritePrompt {
            dst_dir: parent,
            basename,
            src_size,
            dst_size: existing.size,
            direction: crate::state::OverwriteDirection::Upload,
            multi,
            apply_to_all: false,
                    owner: None,
                    drop_upload_pane: None,
        };
        return Ok(TransferStepOutcome::Conflict { prompt, item });
    }
    upload_one(&client, std::path::Path::new(&item.src), &item.dst, temp_name, progress)
        .await?;
    Ok(TransferStepOutcome::Done)
}

/// Apply a chosen overwrite action to a single transfer item. Callable
/// both inside the queue runner (when a sticky default is set) and from
/// the resolve handler (when the user picked an action manually).
pub(crate) async fn apply_overwrite_for_item(
    client: oryxis_ssh::SftpClient,
    item: crate::state::TransferItem,
    action: crate::state::OverwriteAction,
    temp_name: bool,
    progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
) -> Result<(), String> {
    match action {
        crate::state::OverwriteAction::Cancel => Ok(()),
        crate::state::OverwriteAction::Replace => {
            upload_one(&client, std::path::Path::new(&item.src), &item.dst, temp_name, progress).await
        }
        // The engine re-checks everything the modal used to decide this
        // was offerable, and REFUSES rather than restarting if the check
        // fails: the destination is not ours to truncate on a guess, and
        // the user asked to continue a file, not to replace one.
        crate::state::OverwriteAction::Resume => client
            .upload_from_options(
                std::path::Path::new(&item.src),
                &item.dst,
                oryxis_ssh::UploadOptions {
                    progress,
                    resume: true,
                    // Deliberately no scratch name: the user pointed at
                    // the file already there, so continuing it means
                    // writing into THAT file, not beside it.
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string()),
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
            upload_one(&client, std::path::Path::new(&item.src), &item.dst, temp_name, progress).await
        }
        crate::state::OverwriteAction::Duplicate => {
            let parent = parent_path(&item.dst);
            let basename = item
                .dst
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(&item.dst)
                .to_string();
            let unique = unique_name_in_remote_dir(&client, &parent, &basename).await?;
            let target = remote_join(&parent, &unique);
            upload_one(&client, std::path::Path::new(&item.src), &target, temp_name, progress).await
        }
    }
}

/// Apply a single download-queue item with conflict awareness, the mirror
/// of `do_upload_item`. Files existence-check the local destination; if a
/// conflict comes up and there's a sticky default action, apply it,
/// otherwise return a Conflict outcome for the caller to surface in the
/// modal. The check is a local `metadata` call, so unlike the upload side
/// it costs no SFTP round trip.
pub(crate) async fn do_download_item(
    client: oryxis_ssh::SftpClient,
    item: crate::state::TransferItem,
    overwrite_default: Option<crate::state::OverwriteAction>,
    multi: bool,
    progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
) -> Result<TransferStepOutcome, String> {
    if item.is_dir {
        // An existing directory is not a conflict: a batch download merges
        // into it, exactly like the upload side treats a pre-existing
        // remote directory.
        tokio::fs::create_dir_all(&item.dst)
            .await
            .map_err(|e| format!("mkdir {}: {e}", item.dst))?;
        return Ok(TransferStepOutcome::Done);
    }
    let dst = std::path::PathBuf::from(&item.dst);
    if let Ok(existing) = tokio::fs::metadata(&dst).await {
        if let Some(action) = overwrite_default {
            apply_overwrite_for_download_item(client.clone(), item.clone(), action, progress.clone())
                .await?;
            return Ok(TransferStepOutcome::Done);
        }
        let parent = dst
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let basename = dst
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| item.dst.clone());
        let prompt = crate::state::OverwritePrompt {
            dst_dir: parent,
            basename,
            // The walk already carries the remote size; only fall back to
            // a `stat` round trip when it doesn't (a single-file item).
            src_size: match item.size {
                Some(size) => size,
                None => client.stat(&item.src).await.map(|s| s.size).unwrap_or(0),
            },
            dst_size: existing.len(),
            direction: crate::state::OverwriteDirection::Download,
            multi,
            apply_to_all: false,
                    owner: None,
                    drop_upload_pane: None,
        };
        return Ok(TransferStepOutcome::Conflict { prompt, item });
    }
    client
        .download_to_progress(&item.src, &dst, item.size, progress)
        .await
        .map_err(|e| e.to_string())?;
    Ok(TransferStepOutcome::Done)
}

/// Apply a chosen overwrite action to a single DOWNLOAD item, the mirror
/// of `apply_overwrite_for_item`. Callable both inside the queue runner
/// (sticky default) and from the resolve handler (manual answer).
pub(crate) async fn apply_overwrite_for_download_item(
    client: oryxis_ssh::SftpClient,
    item: crate::state::TransferItem,
    action: crate::state::OverwriteAction,
    progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
) -> Result<(), String> {
    let dst = std::path::PathBuf::from(&item.dst);
    match action {
        crate::state::OverwriteAction::Cancel => Ok(()),
        // Never offered on this side (see `OverwriteAction::Resume`): a
        // finished file at a download's destination is not a partial of
        // anything, and the real partial lives in its own scratch file
        // and continues without asking. Loud rather than quietly doing
        // something else, since reaching here means the modal changed.
        crate::state::OverwriteAction::Resume => Err(crate::i18n::t(
            "resume_not_for_downloads",
        )
        .to_string()),
        crate::state::OverwriteAction::Replace => client
            .download_to_progress(&item.src, &dst, item.size, progress.clone())
            .await
            .map_err(|e| e.to_string()),
        crate::state::OverwriteAction::ReplaceIfDifferent => {
            let local_size = tokio::fs::metadata(&dst).await.map(|m| m.len()).unwrap_or(0);
            let remote_size = match item.size {
                Some(size) => size,
                None => client.stat(&item.src).await.map_err(|e| e.to_string())?.size,
            };
            if local_size == remote_size {
                return Ok(());
            }
            client
                .download_to_progress(&item.src, &dst, item.size, progress.clone())
                .await
                .map_err(|e| e.to_string())
        }
        crate::state::OverwriteAction::Duplicate => {
            let parent = dst
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let basename = dst
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| item.dst.clone());
            let target = parent.join(unique_name_in_local_dir(&parent, &basename));
            client
                .download_to_progress(&item.src, &target, item.size, progress.clone())
                .await
                .map_err(|e| e.to_string())
        }
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

/// List a LOCAL directory and pick a non-colliding name for `basename`
/// via `unique_entry_name`. A read error is treated as an empty
/// directory (no collisions). Non-UTF8 entry names are skipped, matching
/// the historical inline behavior. The caller does its own `dir.join(..)`
/// afterward; this only returns the chosen name.
pub(crate) fn unique_name_in_local_dir(dir: &std::path::Path, basename: &str) -> String {
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            if let Some(n) = entry.file_name().to_str() {
                names.insert(n.to_string());
            }
        }
    }
    unique_entry_name(basename, |n| !names.contains(n))
}

/// List a REMOTE directory over SFTP and pick a non-colliding name for
/// `basename` via `unique_entry_name`. The caller does its own
/// `remote_join(dir, ..)` afterward; this only returns the chosen name.
pub(crate) async fn unique_name_in_remote_dir(
    client: &oryxis_ssh::SftpClient,
    dir: &str,
    basename: &str,
) -> Result<String, String> {
    let entries = client.list_dir(dir).await.map_err(|e| e.to_string())?;
    let names: std::collections::HashSet<String> = entries.into_iter().map(|e| e.name).collect();
    Ok(unique_entry_name(basename, |n| !names.contains(n)))
}

/// How [`exec_checked`] maps the remote exit code to success.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecTolerance {
    /// Only a clean exit (code 0) is success.
    Strict,
    /// Exit code 1 is also accepted. `unzip` exits 1 for benign
    /// warnings (e.g. trailing garbage) while still extracting, so the
    /// archive extract path opts in when the synthesized command is an
    /// unzip run.
    AcceptWarning,
}

/// Run `cmd` on the live handle's exec channel and map the exit status
/// to `Ok(stdout)` / `Err(message)`. A failing exit prefers the
/// command's own (trimmed) stderr text; when stderr is empty the
/// caller-supplied `fallback` renders a message from the exit code, so
/// each call site keeps its own wording.
pub(crate) async fn exec_checked(
    client: &oryxis_ssh::SftpClient,
    cmd: &str,
    tolerance: ExecTolerance,
    fallback: impl FnOnce(u32) -> String,
) -> Result<String, String> {
    let (code, out, err) = client.exec(cmd).await.map_err(|e| e.to_string())?;
    let ok = code == 0 || (code == 1 && tolerance == ExecTolerance::AcceptWarning);
    if ok {
        Ok(out)
    } else {
        let err = err.trim();
        if err.is_empty() {
            Err(fallback(code))
        } else {
            Err(err.to_string())
        }
    }
}

/// Run a remote `cp`/`cp -r` over the exec channel, mapping the exit
/// code to `Ok(())` / `Err(message)`. Paths are quoted with the archive
/// crate's [`oryxis_archive::quote::sh_quote`], the same hostile-name
/// boundary the remote archive commands go through (it also rejects
/// line breaks outright instead of trusting the login shell to keep
/// them literal). `--` prevents dashes in names from being parsed as
/// flags. `recursive` selects `cp -r --` vs `cp --` and the matching
/// exit-code label.
pub(crate) async fn remote_cp(
    client: &oryxis_ssh::SftpClient,
    src: &str,
    dst: &str,
    recursive: bool,
) -> Result<(), String> {
    let quoted_src = oryxis_archive::quote::sh_quote(src).map_err(|e| e.to_string())?;
    let quoted_dst = oryxis_archive::quote::sh_quote(dst).map_err(|e| e.to_string())?;
    let cmd = if recursive {
        format!("cp -r -- {quoted_src} {quoted_dst}")
    } else {
        format!("cp -- {quoted_src} {quoted_dst}")
    };
    exec_checked(client, &cmd, ExecTolerance::Strict, |code| {
        if recursive {
            format!("cp -r exited {code}")
        } else {
            format!("cp exited {code}")
        }
    })
    .await
    .map(|_| ())
}

/// Pick a name that doesn't collide with any existing entry in the same
/// directory, `name.ext` → `name copy.ext`, then `name copy 2.ext`,
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

/// Final path component, suitable for prefilling a rename dialog. The
/// pane nature disambiguates the separator convention (POSIX `/` for
/// remote vs. platform-native for local).
pub(crate) fn file_basename(path: &str, is_remote: bool) -> String {
    if is_remote {
        path.rsplit_once('/')
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| path.to_string())
    } else {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
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
            // Kind sorts by the SAME label the cell shows (issue #143),
            // so the order matches what the eye reads, localized names
            // included. Permissions by the numeric mode bits (unknown
            // first), Owner by uid then gid.
            SftpSortColumn::Kind => {
                crate::views::sftp::format_kind(&a.name, a.is_dir, a.is_symlink)
                    .cmp(&crate::views::sftp::format_kind(&b.name, b.is_dir, b.is_symlink))
            }
            SftpSortColumn::Permissions => (a.permissions.unwrap_or(0) & 0o7777)
                .cmp(&(b.permissions.unwrap_or(0) & 0o7777)),
            SftpSortColumn::Owner => a
                .uid
                .cmp(&b.uid)
                .then_with(|| a.gid.cmp(&b.gid)),
        }
        // Ties (every folder shares one Kind, one owner owns most of a
        // home directory) resolve by name so the order stays stable and
        // scannable instead of listing-order arbitrary.
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
            // Same trio as the remote side (issue #143); the local
            // listing has no symlink flag, matching its Kind cell.
            SftpSortColumn::Kind => crate::views::sftp::format_kind(&a.name, a.is_dir, false)
                .cmp(&crate::views::sftp::format_kind(&b.name, b.is_dir, false)),
            SftpSortColumn::Permissions => (a.mode.unwrap_or(0) & 0o7777)
                .cmp(&(b.mode.unwrap_or(0) & 0o7777)),
            SftpSortColumn::Owner => a
                .uid
                .cmp(&b.uid)
                .then_with(|| a.gid.cmp(&b.gid)),
        }
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        if sort.ascending { cmp } else { cmp.reverse() }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The optional columns sort too (issue #143): Kind by the shown
    /// label, Permissions by mode bits, Owner by uid/gid, all with the
    /// name as the tiebreak so equal keys stay scannable.
    #[test]
    fn extra_columns_sort_with_name_tiebreak() {
        use crate::state::{SftpSort, SftpSortColumn};
        let entry = |name: &str, perm: u32, uid: u32| oryxis_ssh::SftpEntry {
            name: name.to_string(),
            is_dir: false,
            is_symlink: false,
            size: 0,
            mtime: None,
            permissions: Some(perm),
            uid: Some(uid),
            gid: Some(0),
        };
        // Permissions: numeric mode order, names break the tie.
        let mut rows = vec![entry("b.txt", 0o755, 1), entry("a.txt", 0o755, 2), entry("c.txt", 0o600, 3)];
        sort_remote_entries(&mut rows, SftpSort { column: SftpSortColumn::Permissions, ascending: true });
        let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["c.txt", "a.txt", "b.txt"]);
        // Owner: uid order.
        sort_remote_entries(&mut rows, SftpSort { column: SftpSortColumn::Owner, ascending: false });
        let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["c.txt", "a.txt", "b.txt"]);
        // Kind: the shown label (text/plain groups together), name tiebreak.
        let mut rows = vec![entry("z.txt", 0, 0), entry("a.png", 0, 0), entry("m.txt", 0, 0)];
        sort_remote_entries(&mut rows, SftpSort { column: SftpSortColumn::Kind, ascending: true });
        let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.png", "m.txt", "z.txt"]);
    }

    /// Relaying a folder into its own subtree nests it into itself. The
    /// guard has to catch containment without catching the legitimate
    /// same-host cases around it, which is the whole difficulty: a
    /// sibling whose name merely starts with the source's name is NOT
    /// inside it (issue #115).
    #[test]
    fn containment_guard_catches_self_nesting_only() {
        // The reported shape: destination pane sitting in the folder
        // being relayed.
        assert!(relay_target_is_inside_source("/srv/data", "/srv/data/data"));
        // Deeper still, and the same directory named twice.
        assert!(relay_target_is_inside_source("/srv/data", "/srv/data/a/b"));
        assert!(relay_target_is_inside_source("/srv/data", "/srv/data"));
        // Trailing separators must not change the answer.
        assert!(relay_target_is_inside_source("/srv/data/", "/srv/data/x"));
        assert!(relay_target_is_inside_source("/srv/data", "/srv/data/x/"));
        // Root as the source contains everything.
        assert!(relay_target_is_inside_source("/", "/anywhere"));

        // A SIBLING sharing a name prefix is not inside the source. This
        // is the case a naive `starts_with` gets wrong, and getting it
        // wrong refuses a perfectly good transfer.
        assert!(!relay_target_is_inside_source("/srv/data", "/srv/data2"));
        assert!(!relay_target_is_inside_source("/srv/data", "/srv/database/x"));
        // The parent is not inside its own child.
        assert!(!relay_target_is_inside_source("/srv/data", "/srv"));
        // Ordinary unrelated destinations.
        assert!(!relay_target_is_inside_source("/srv/data", "/backup/data"));
        assert!(!relay_target_is_inside_source("/home/a/x", "/home/b/x"));
    }

    /// A move deletes its source last, and the ORDER is what makes the
    /// deletion safe: `remove_dir` refuses a non-empty directory, so
    /// every file has to be gone before its folder is attempted, and a
    /// child folder before its parent. Get this backwards and a move
    /// leaves the whole tree behind with a confusing error (issue #115).
    #[test]
    fn moved_sources_are_removed_children_first() {
        let item = |src: &str, is_dir: bool| crate::state::TransferItem {
            src: src.to_string(),
            dst: format!("/dst{src}"),
            is_dir,
            size: None,
        };
        // The queue order a folder walk produces: parent dirs before
        // their contents, which is exactly the reverse of what removal
        // needs.
        let sources = vec![
            item("/srv/data", true),
            item("/srv/data/a.txt", false),
            item("/srv/data/sub", true),
            item("/srv/data/sub/b.txt", false),
            item("/srv/data/sub/deep", true),
            item("/srv/data/sub/deep/c.txt", false),
        ];

        let order = moved_source_removal_order(&sources);
        let paths: Vec<&str> = order.iter().map(|(p, _)| p.as_str()).collect();

        // Every file precedes every directory.
        let first_dir = order.iter().position(|(_, d)| *d).expect("a directory");
        assert!(
            order[..first_dir].iter().all(|(_, d)| !*d),
            "a directory was scheduled before a file: {paths:?}"
        );
        // Directories run deepest first.
        let dir_pos = |p: &str| paths.iter().position(|q| *q == p).unwrap();
        assert!(
            dir_pos("/srv/data/sub/deep") < dir_pos("/srv/data/sub"),
            "a parent was scheduled before its child: {paths:?}"
        );
        assert!(
            dir_pos("/srv/data/sub") < dir_pos("/srv/data"),
            "a parent was scheduled before its child: {paths:?}"
        );
        // Nothing invented, nothing dropped: a move removes exactly what
        // it copied.
        assert_eq!(order.len(), sources.len(), "removal list changed size");
    }

    /// The transfer queue's dir barrier (`TransferState::dir_slot`)
    /// relies on the walk enqueueing every directory before anything
    /// inside it. If this ordering ever breaks, parallel slots race
    /// child uploads against their parent's mkdir again (issue #63).
    #[test]
    fn upload_walk_enqueues_dirs_before_their_children() {
        let root = std::env::temp_dir()
            .join(format!("oryxis-walk-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("sub/b.txt"), b"b").unwrap();
        std::fs::write(root.join("sub/deep/c.txt"), b"c").unwrap();

        let mut queue = std::collections::VecDeque::new();
        queue.push_back(crate::state::TransferItem {
            src: root.to_string_lossy().into_owned(),
            dst: "/dst/root".to_string(),
            is_dir: true,
            size: None,
        });
        walk_local_for_upload(&root, "/dst/root", &mut queue).unwrap();
        let items: Vec<_> = queue.into_iter().collect();

        assert_eq!(items.len(), 6, "root + sub + deep + 3 files");
        for (i, item) in items.iter().enumerate() {
            if !item.is_dir {
                continue;
            }
            let prefix = format!("{}/", item.dst);
            for earlier in &items[..i] {
                assert!(
                    !earlier.dst.starts_with(&prefix),
                    "{} enqueued before its parent dir {}",
                    earlier.dst,
                    item.dst
                );
            }
        }
        // Every file's parent dir is present as an earlier dir item.
        for (i, item) in items.iter().enumerate() {
            if item.is_dir {
                continue;
            }
            let parent = parent_path(&item.dst);
            assert!(
                items[..i].iter().any(|d| d.is_dir && d.dst == parent),
                "file {} has no earlier dir item for {}",
                item.dst,
                parent
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The download side's Duplicate answer ("rename") leans entirely on
    /// this: it must never hand back a name that is already taken, or
    /// the "don't overwrite" answer would overwrite (issue #114).
    #[test]
    fn local_unique_name_never_collides() {
        let root = std::env::temp_dir().join(format!(
            "oryxis-unique-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).unwrap();

        // Empty directory: the name is free, so it comes back verbatim.
        assert_eq!(unique_name_in_local_dir(&root, "notes.txt"), "notes.txt");

        // Taken once, then the alternative is taken too: each round has
        // to move past everything already on disk.
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..3 {
            let name = unique_name_in_local_dir(&root, "notes.txt");
            assert!(
                !root.join(&name).exists(),
                "{name} already exists in the destination"
            );
            assert!(!seen.contains(&name), "{name} handed out twice");
            std::fs::write(root.join(&name), b"x").unwrap();
            seen.push(name);
        }
        // Extension preserved, so the renamed copy still opens.
        for name in &seen[1..] {
            assert!(name.ends_with(".txt"), "{name} lost its extension");
        }

        // A dotfile has no extension to preserve; the suffix goes at the
        // end and must still avoid the original.
        std::fs::write(root.join(".bashrc"), b"x").unwrap();
        let dot = unique_name_in_local_dir(&root, ".bashrc");
        assert_ne!(dot, ".bashrc");
        assert!(!root.join(&dot).exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
