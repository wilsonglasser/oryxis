//! SFTP client built on top of an existing SSH session.
//!
//! `SshSession::open_sftp()` opens a fresh channel on the underlying SSH
//! connection, requests the `sftp` subsystem, and hands back an
//! [`SftpClient`] that wraps the high-level operations exposed by the
//! `russh-sftp` crate.

use russh::ChannelMsg;
use russh_sftp::client::{RawSftpSession, SftpSession};
use russh_sftp::protocol::{FileAttributes, OpenFlags, Packet, StatusCode};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::engine::{SharedHandle, SshError};

/// File metadata snapshot returned by [`SftpClient::list_dir`]. Times are
/// kept as raw u32 unix seconds (what the SFTP protocol exposes); the UI
/// converts to human strings.
#[derive(Debug, Clone)]
pub struct SftpEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mtime: Option<u32>,
    pub permissions: Option<u32>,
    /// Owning uid/gid as reported by the directory listing. Populated
    /// from the SFTP attributes when the server includes them; `None`
    /// otherwise. Drives the optional Owner column in the UI.
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

/// True when a name that came from the SFTP SERVER is a single plain
/// path component, and so is safe to join onto a local path.
///
/// A directory listing is remote input. The protocol carries a name as
/// an opaque string, so a hostile or compromised server can answer with
/// `../../.ssh/authorized_keys`, `..\..\evil.exe` or `C:evil`, and every
/// one of those steers a later join outside the directory the user
/// picked. Taking the last `/`-separated component is NOT enough: it
/// leaves both Windows forms intact, and on Windows `\` is a separator
/// while a drive prefix re-roots the whole path.
///
/// This is the single authority for that question. `oryxis-app`'s
/// `is_safe_remote_entry_name` delegates here rather than keeping a
/// second copy, because two predicates guarding one threat drift, and
/// the one that drifts is the one nobody is looking at.
pub fn is_safe_entry_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return false;
    }
    // Windows absolute/drive-relative forms ("C:foo") survive the
    // separator check above but still re-root PathBuf::join there.
    if name.as_bytes().get(1) == Some(&b':') {
        return false;
    }
    true
}

/// Destination-side policy for [`SftpClient::upload_from_options`].
///
/// The defaults are the historical behaviour, so the plain
/// [`upload_from`](SftpClient::upload_from) family keeps meaning exactly
/// what it always did: truncate the destination and write under its real
/// name.
#[derive(Debug, Clone, Default)]
pub struct UploadOptions {
    /// Shared counter the UI polls for a live progress bar.
    pub progress: Option<Arc<std::sync::atomic::AtomicU64>>,
    /// Continue from the destination's current length instead of
    /// truncating it. Only ever set from a caller that ASKED the user:
    /// the check that the existing bytes belong to this file is a tail
    /// comparison, not a proof (see `RESUME_VERIFY_BYTES`).
    pub resume: bool,
    /// Write to a scratch name and rename into place on success, so the
    /// real name is only ever a finished file. Costs a rename the server
    /// may forbid, which is why it is a choice and not the default.
    pub temp_name: bool,
}

/// Per-path stat snapshot used by the Properties dialog.
#[derive(Debug, Clone)]
pub struct RemoteStat {
    pub size: u64,
    pub permissions: Option<u32>,
    pub mtime: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

/// High-level SFTP client tied to a single subsystem channel. Cheap to
/// clone (it's an `Arc<Mutex<...>>`) so multiple UI components can share
/// the same session.
///
/// Holds a clone of the underlying SSH handle so it can open exec channels
/// on the same connection, needed for ops like recursive delete where
/// shelling out to `rm -rf` is dramatically faster than walking the tree
/// over SFTP.
#[derive(Clone)]
pub struct SftpClient {
    inner: Arc<Mutex<SftpSession>>,
    handle: SharedHandle,
    /// Timeout used by `open_sibling`, propagated from the parent
    /// `SshSession` so siblings honour the same configured limit.
    open_timeout: std::time::Duration,
    /// Per-operation timeout in seconds, shared across clones via an
    /// atomic so the user's settings panel can update it live without
    /// reconnecting. Caps how long the UI can stay in a "Loading…"
    /// state when the remote stops responding mid-request.
    op_timeout_secs: Arc<std::sync::atomic::AtomicU64>,
}

impl std::fmt::Debug for SftpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpClient").finish_non_exhaustive()
    }
}

impl SftpClient {
    pub(crate) fn new(
        session: SftpSession,
        handle: SharedHandle,
        open_timeout: std::time::Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(session)),
            handle,
            open_timeout,
            // Default 30s until `set_op_timeout` is called by the
            // caller. Seconds-grained because that's what the settings
            // panel exposes.
            op_timeout_secs: Arc::new(std::sync::atomic::AtomicU64::new(30)),
        }
    }

    /// Override the per-operation timeout. Takes effect on the next
    /// op, already-in-flight calls keep their existing deadline.
    /// Cheap (atomic store) so the settings panel can call this on
    /// every input change without throttling.
    pub fn set_op_timeout(&self, t: std::time::Duration) {
        self.op_timeout_secs
            .store(t.as_secs().max(1), std::sync::atomic::Ordering::Relaxed);
    }

    fn current_op_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.op_timeout_secs
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Wrap an async op in the configured op timeout. Centralises the
    /// "X timed out after Ys" error message so the UI can surface a
    /// consistent retry hint.
    async fn with_op_timeout<T, F>(&self, op_name: &str, fut: F) -> Result<T, SshError>
    where
        F: std::future::Future<Output = Result<T, SshError>>,
    {
        let timeout = self.current_op_timeout();
        match tokio::time::timeout(timeout, fut).await {
            Ok(r) => r,
            Err(_) => Err(SshError::Channel(format!(
                "sftp {} timed out after {}s",
                op_name,
                timeout.as_secs()
            ))),
        }
    }

    /// List directory contents. Filters out the synthetic `.` / `..`
    /// entries, the UI provides its own breadcrumb / "go up" affordance.
    pub async fn list_dir(&self, path: &str) -> Result<Vec<SftpEntry>, SshError> {
        let label = format!("read_dir({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            let entries = s
                .read_dir(path)
                .await
                .map_err(|e| SshError::Channel(format!("sftp read_dir({path}): {e}")))?;
            let mut out = Vec::new();
            for entry in entries {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let metadata = entry.metadata();
                out.push(SftpEntry {
                    name,
                    is_dir: metadata.is_dir(),
                    is_symlink: metadata.is_symlink(),
                    size: metadata.size.unwrap_or(0),
                    mtime: metadata.mtime,
                    permissions: metadata.permissions,
                    uid: metadata.uid,
                    gid: metadata.gid,
                });
            }
            Ok(out)
        })
        .await
    }

    /// Resolve a possibly-relative path to its canonical absolute form.
    /// Used at session open to anchor the user's first directory.
    pub async fn canonicalize(&self, path: &str) -> Result<String, SshError> {
        let label = format!("canonicalize({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            s.canonicalize(path)
                .await
                .map_err(|e| SshError::Channel(format!("sftp canonicalize({path}): {e}")))
        })
        .await
    }

    /// True when both clients ride the same SSH connection, and therefore
    /// see the same filesystem as the same user.
    ///
    /// Every `SftpClient` opened from one `SshSession` is handed a clone
    /// of that session's own handle `Arc`, so pointer identity is an
    /// exact answer rather than a heuristic. The converse does NOT hold:
    /// two separate sessions may still reach the same machine (two vault
    /// entries for one host, or two users on one server), so a `false`
    /// here means "cannot prove same host", never "different host".
    pub fn shares_session_with(&self, other: &SftpClient) -> bool {
        Arc::ptr_eq(&self.handle, &other.handle)
    }

    /// Canonical identity of `path` for same-file comparisons, resolving
    /// symlinks and relative segments.
    ///
    /// Falls back to canonicalizing the PARENT and re-joining the base
    /// name when the path itself doesn't resolve, which is the normal
    /// case for a transfer destination that doesn't exist yet: servers
    /// answer `SSH_FXP_REALPATH` for a missing leaf inconsistently, but
    /// its directory always resolves.
    async fn path_identity(&self, path: &str) -> Option<String> {
        if let Ok(real) = self.canonicalize(path).await {
            return Some(real);
        }
        let trimmed = path.trim_end_matches('/');
        let (parent, base) = match trimmed.rsplit_once('/') {
            Some((p, b)) if !b.is_empty() => (if p.is_empty() { "/" } else { p }, b),
            _ => return None,
        };
        let real_parent = self.canonicalize(parent).await.ok()?;
        Some(format!("{}/{}", real_parent.trim_end_matches('/'), base))
    }

    /// Read a remote file fully into memory. Fine for small files the UI
    /// touches whole (edit-in-place, config snippets). Bulk transfers go
    /// through [`download_to`](Self::download_to) instead, which streams
    /// in bounded chunks rather than buffering the whole file.
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>, SshError> {
        use tokio::io::AsyncReadExt as _;
        let label = format!("read({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            let mut file = s
                .open_with_flags(path, OpenFlags::READ)
                .await
                .map_err(|e| SshError::Channel(format!("sftp open({path}): {e}")))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .await
                .map_err(|e| SshError::Channel(format!("sftp read({path}): {e}")))?;
            Ok(buf)
        })
        .await
    }

    /// Replace the contents of a remote file. Truncates if it exists,
    /// creates if it doesn't.
    pub async fn write_file(&self, path: &str, contents: &[u8]) -> Result<(), SshError> {
        use tokio::io::AsyncWriteExt as _;
        let label = format!("write({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            let mut file = s
                .open_with_flags(
                    path,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                )
                .await
                .map_err(|e| SshError::Channel(format!("sftp open(W,{path}): {e}")))?;
            file.write_all(contents)
                .await
                .map_err(|e| SshError::Channel(format!("sftp write({path}): {e}")))?;
            file.shutdown()
                .await
                .map_err(|e| SshError::Channel(format!("sftp close({path}): {e}")))?;
            Ok(())
        })
        .await
    }

    /// Create an empty remote file, failing if the path already exists
    /// (SSH_FXF_EXCL). The "touch" primitive for new-file UI: unlike
    /// [`write_file`](Self::write_file), a name collision surfaces as an
    /// error instead of silently truncating the existing file. Note that
    /// protocol-v3 servers report the collision as a generic failure
    /// status; callers wanting a readable message should stat first.
    pub async fn create_file_exclusive(&self, path: &str) -> Result<(), SshError> {
        use tokio::io::AsyncWriteExt as _;
        let label = format!("create({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            let mut file = s
                .open_with_flags(
                    path,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                )
                .await
                .map_err(|e| SshError::Channel(format!("sftp create({path}): {e}")))?;
            file.shutdown()
                .await
                .map_err(|e| SshError::Channel(format!("sftp close({path}): {e}")))?;
            Ok(())
        })
        .await
    }

    /// Thin wrapper that snapshots the per-op timeout once and hands the
    /// copy off to [`pump_bytes`]. Reading the deadline a single time at
    /// entry matches the `set_op_timeout` contract (in-flight ops keep
    /// their existing deadline) and keeps the loop itself free-standing
    /// and unit-testable.
    async fn pump<R, W>(
        &self,
        op_name: &str,
        reader: R,
        writer: W,
        progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    ) -> Result<(), SshError>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        pump_bytes(
            reader,
            writer,
            self.current_op_timeout(),
            op_name,
            progress.as_deref(),
        )
        .await
    }

    /// Throw away the bytes an interrupted download left behind.
    ///
    /// For CANCEL, which is not the same event as a failure: a failure
    /// keeps its scratch file because the user still wants the file and a
    /// later attempt continues from it, while a cancel is the user saying
    /// they do not, so leaving debris behind would be the surprise.
    pub async fn discard_download_scratch(local: &std::path::Path) {
        let _ = tokio::fs::remove_file(part_path(local)).await;
    }

    /// The upload mirror of [`discard_download_scratch`], for a cancel that
    /// used [`UploadOptions::temp_name`]. A cancelled upload writing under
    /// the real name is not swept here: it has already destroyed whatever
    /// was there, and removing it too would turn a cancel into a delete.
    pub async fn discard_upload_scratch(&self, remote: &str) {
        let _ = self.remove_file(&remote_part_path(remote)).await;
    }

    /// Does `local` hold the same bytes as `remote` over the last
    /// [`RESUME_VERIFY_BYTES`] before `at`? The question a resume has to
    /// answer, in both directions: on download `remote` is the source and
    /// `local` the scratch file we already filled, on upload `remote` is
    /// the partial sitting on the server and `local` the file being sent.
    ///
    /// `false` (rather than an error) for every honest reason not to
    /// resume: nothing transferred yet, or the remote is shorter than what
    /// we think we already have, so it cannot contain that range at all.
    /// Read what [`RESUME_VERIFY_BYTES`] documents before trusting a
    /// `true`: it rules out a different file, not a different version of
    /// this one.
    async fn tail_matches(
        &self,
        remote: &str,
        local: &std::path::Path,
        at: u64,
    ) -> Result<bool, SshError> {
        if at == 0 {
            return Ok(false);
        }
        let span = RESUME_VERIFY_BYTES.min(at);
        let from = at - span;
        let f = self.open_ranged(remote).await?;
        if f.len() < at {
            f.close().await;
            return Ok(false);
        }
        let remote_tail = f.read_at(from, span as usize).await;
        f.close().await;
        let remote_tail = remote_tail?;
        if remote_tail.len() as u64 != span {
            return Ok(false);
        }
        Ok(read_local_range(local, from, span as usize)? == remote_tail)
    }

    /// Stream a remote file down to a local path without buffering the
    /// whole thing in RAM. Small files take the single-handle sequential
    /// pump; large ones (>= `STREAM_THRESHOLD`) carry a sliding window of
    /// concurrent reads on one handle (see `windowed_download_copy`). The
    /// remote handle is opened under the session lock, then the lock is
    /// released for the copy, so other ops on this client stay responsive.
    ///
    /// Bytes land in a scratch file next to the target ([`PART_SUFFIX`])
    /// and are renamed into place only once the copy completes, so `local`
    /// is either absent or a finished file, never a plausible-looking
    /// truncation. A failed transfer KEEPS its scratch file: those bytes
    /// are what a later call resumes from, and throwing them away is the
    /// whole reason a dropped 3 GB download used to cost the user the 3 GB
    /// (a partial at the target name is the thing to avoid, not partial
    /// bytes as such). Resume is automatic and only ever continues a
    /// scratch file whose tail still matches the server's, see
    /// [`RESUME_VERIFY_BYTES`] for exactly how much that proves.
    ///
    /// `size_hint` lets a caller that already knows the remote size (e.g.
    /// from the directory listing it walked) skip the `stat` round trip.
    /// This matters for bulk downloads of many small files, where an extra
    /// stat per file is pure latency. `None` falls back to `stat`.
    pub async fn download_to(
        &self,
        remote: &str,
        local: &std::path::Path,
        size_hint: Option<u64>,
    ) -> Result<(), SshError> {
        self.download_to_progress(remote, local, size_hint, None)
            .await
    }

    /// Like [`download_to`](Self::download_to) but reports bytes transferred
    /// into `progress` (a shared counter the UI polls for a live bar).
    pub async fn download_to_progress(
        &self,
        remote: &str,
        local: &std::path::Path,
        size_hint: Option<u64>,
        progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    ) -> Result<(), SshError> {
        let label = format!("download({remote})");
        let size = match size_hint {
            Some(s) => s,
            None => self.stat(remote).await?.size,
        };
        let part = part_path(local);

        // Resume is attempted only on the windowed path. Below the
        // threshold the verification round trip costs more than re-sending
        // the bytes it would save, and this decision is automatic, so it
        // should never spend a round trip it cannot repay.
        let resume_from = if size >= STREAM_THRESHOLD {
            match tokio::fs::metadata(&part).await {
                Ok(m) if m.len() > 0 => {
                    let have = resume_offset(size, m.len());
                    let matches = have > 0 && self.tail_matches(remote, &part, have).await?;
                    tracing::info!(
                        remote,
                        scratch_bytes = m.len(),
                        resume_from = if matches { have } else { 0 },
                        tail_matched = matches,
                        "sftp download: resume decision"
                    );
                    if matches { have } else { 0 }
                }
                _ => 0,
            }
        } else {
            0
        };
        // The counter is shared across every file of a batch, so a resume
        // ADDS what is already on disk rather than storing it: the bar has
        // to account for bytes this call will never transfer.
        // Adding zero is a no-op, so the non-resuming case needs no
        // special path here.
        if let Some(p) = &progress {
            p.fetch_add(resume_from, std::sync::atomic::Ordering::Relaxed);
        }

        if size < STREAM_THRESHOLD {
            let remote_file = self
                .with_op_timeout(&label, async {
                    let s = self.inner.lock().await;
                    s.open_with_flags(remote, OpenFlags::READ)
                        .await
                        .map_err(|e| SshError::Channel(format!("sftp open({remote}): {e}")))
                })
                .await?;
            // `create` truncates, which is what a from-zero transfer wants
            // of a scratch file an earlier attempt may have left behind.
            let local_file = tokio::fs::File::create(&part)
                .await
                .map_err(|e| SshError::Channel(format!("create {}: {e}", part.display())))?;
            self.pump(&label, remote_file, local_file, progress).await?;
            return finish_part(&part, local).await;
        }

        // Large file: one streaming handle carrying a sliding window of
        // concurrent reads. Open the handle on a dedicated raw session,
        // preallocate the local file, then positioned-write each completed
        // read into its region.
        let raw = self.open_raw_streaming().await?;
        let handle = raw
            .open(remote, OpenFlags::READ, FileAttributes::empty())
            .await
            .map_err(|e| SshError::Channel(format!("sftp open({remote}): {e}")))?
            .handle;
        // The hint only chose this branch; trusting it for the transfer
        // extent would silently truncate if it is stale (the file grew
        // since the directory walk). Re-stat the open handle for the
        // authoritative size, falling back to the hint if fstat can't.
        let actual = match raw.fstat(handle.clone()).await {
            Ok(a) => a.attrs.size.unwrap_or(size),
            Err(_) => size,
        };
        // `actual` is authoritative and `resume_from` was decided against
        // the possibly-stale hint, so a file that shrank under us must not
        // leave the window starting past its end.
        let resume_from = if resume_from >= actual { 0 } else { resume_from };
        // A resume must not truncate: keep the scratch file's bytes and
        // write past them; only a from-zero run creates-or-truncates.
        //
        // The full extent is deliberately NOT preallocated. `set_len` here
        // would make the file's length the size of the DOWNLOAD rather than
        // of what has been written, and after a crash that length is the
        // only thing a later attempt has to go on (see `RESUME_REWIND`).
        // Positioned writes past the end extend the file on their own, so
        // the reservation bought nothing except an unreadable length: on
        // Linux `set_len` does not reserve blocks either way.
        let mut out = if resume_from > 0 {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&part)
                .map_err(|e| SshError::Channel(format!("open {}: {e}", part.display())))?
        } else {
            std::fs::File::create(&part)
                .map_err(|e| SshError::Channel(format!("create {}: {e}", part.display())))?
        };

        let timeout = self.current_op_timeout();
        let raw_read = raw.clone();
        let handle_read = handle.clone();
        let done = std::sync::atomic::AtomicU64::new(resume_from);
        let result = windowed_download_copy(
            resume_from,
            actual,
            STREAM_CHUNK as u64,
            STREAM_WINDOW,
            timeout,
            &label,
            &done,
            move |off, len| {
                let raw = raw_read.clone();
                let handle = handle_read.clone();
                async move {
                    raw.read(handle, off, len)
                        .await
                        .map(|d| d.data)
                        .map_err(|e| SshError::Channel(format!("sftp read({off}): {e}")))
                }
            },
            |off, data| {
                use std::io::{Seek, SeekFrom, Write};
                out.seek(SeekFrom::Start(off))
                    .map_err(|e| SshError::Channel(format!("seek {}: {e}", part.display())))?;
                let n = data.len() as u64;
                out.write_all(&data)
                    .map_err(|e| SshError::Channel(format!("write {}: {e}", part.display())))?;
                if let Some(p) = &progress {
                    p.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                }
                Ok(())
            },
        )
        .await;
        // Release the write handle before anything else touches the file:
        // Windows refuses a second writable open while it lives.
        drop(out);
        // Best-effort close; the transfer result is what matters.
        let _ = raw.close(handle).await;
        if let Err(e) = result {
            // The target name is only ever claimed by `finish_part`, so a
            // failure leaves no deceptive file there. The scratch file
            // stays, trimmed to the contiguous prefix so the holes the
            // window may have left above it are gone. This is the tidy
            // path; `RESUME_REWIND` is what covers the untidy one, where
            // the process never got here at all.
            if let Ok(f) = tokio::fs::OpenOptions::new().write(true).open(&part).await {
                let _ = f
                    .set_len(done.load(std::sync::atomic::Ordering::Relaxed))
                    .await;
            }
            return Err(e);
        }
        // Success, but the resume may have started from a scratch that
        // was LONGER than the remote is now (the file shrank between
        // attempts, with an identical prefix, so `resume_from < actual`
        // still held): the window wrote `[resume_from, actual)` and left
        // the stale tail above `actual` in place, which `finish_part`
        // would rename into the final file as garbage bytes. Trim to the
        // authoritative size before promoting. Only ever shortens, so
        // the crash-recovery length invariant on the FAILURE path above
        // is untouched: this runs after the copy fully succeeded.
        if let Ok(f) = tokio::fs::OpenOptions::new().write(true).open(&part).await {
            let _ = f.set_len(actual).await;
        }
        finish_part(&part, local).await
    }

    /// Stream a local file up to a remote path. Mirror of
    /// [`download_to`](Self::download_to): small files go through the
    /// single-handle pump (which truncates-or-creates), large ones
    /// (>= `STREAM_THRESHOLD`) truncate the destination once up front,
    /// then carry a sliding window of concurrent writes on one handle (see
    /// `windowed_upload_copy`).
    pub async fn upload_from(&self, local: &std::path::Path, remote: &str) -> Result<(), SshError> {
        self.upload_from_progress(local, remote, None).await
    }

    /// Like [`upload_from`](Self::upload_from) but reports bytes transferred
    /// into `progress` (a shared counter the UI polls for a live bar).
    pub async fn upload_from_progress(
        &self,
        local: &std::path::Path,
        remote: &str,
        progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    ) -> Result<(), SshError> {
        self.upload_from_options(
            local,
            remote,
            UploadOptions {
                progress,
                ..UploadOptions::default()
            },
        )
        .await
    }

    /// [`upload_from`](Self::upload_from) with the destination-side policy
    /// spelled out, see [`UploadOptions`].
    ///
    /// Unlike the download direction, NOTHING here is decided on the user's
    /// behalf. A download owns its destination and can prove what it is
    /// resuming; an upload is writing into someone else's namespace, where
    /// a shorter file with a matching tail is only probably the same file
    /// (see [`RESUME_VERIFY_BYTES`]). So resume is off unless a caller that
    /// asked the user turns it on, and a resume whose verification fails is
    /// an ERROR rather than a silent restart: the destination is not ours
    /// to truncate on a guess.
    pub async fn upload_from_options(
        &self,
        local: &std::path::Path,
        remote: &str,
        opts: UploadOptions,
    ) -> Result<(), SshError> {
        let UploadOptions {
            progress,
            resume,
            temp_name,
        } = opts;
        let label = format!("upload({remote})");
        let size = tokio::fs::metadata(local)
            .await
            .map_err(|e| SshError::Channel(format!("stat {}: {e}", local.display())))?
            .len();
        // Where the bytes actually go. With `temp_name` the final path is
        // only claimed by the rename at the end, so an interrupted upload
        // never leaves a plausible-looking file under the real name and a
        // watcher on the server sees the file appear whole.
        let target = if temp_name {
            remote_part_path(remote)
        } else {
            remote.to_string()
        };

        let resume_from = if resume {
            match self.stat(&target).await {
                // No destination yet: a resume request on a fresh path is
                // just an upload, not an error.
                Err(_) => 0,
                Ok(st) if st.size == 0 => 0,
                Ok(st) if st.size >= size => {
                    return Err(SshError::Channel(format!(
                        "sftp resume({remote}): the file already there is {} bytes, this one is {size}, so it cannot be a partial copy of it",
                        st.size
                    )));
                }
                Ok(st) => {
                    // Same rule as the download side, for the same reason:
                    // the destination's SIZE is the highest offset an
                    // interrupted window reached, and a crash left no
                    // chance to trim the holes below it.
                    let have = resume_offset(size, st.size);
                    tracing::info!(
                        remote,
                        target,
                        destination_bytes = st.size,
                        source_bytes = size,
                        resume_from = have,
                        "sftp upload: resume decision"
                    );
                    if have == 0 {
                        // Nothing worth continuing, so this is an ordinary
                        // upload.
                        0
                    } else if self.tail_matches(&target, local, have).await? {
                        tracing::info!(remote, resume_from = have, "sftp upload: resuming");
                        have
                    } else {
                        return Err(SshError::Channel(format!(
                            "sftp resume({remote}): the {} bytes already there do not match this file, so resuming would corrupt it",
                            st.size
                        )));
                    }
                }
            }
        } else {
            0
        };
        // Shared across a batch, so ADD what this call will not transfer
        // rather than storing it (see the download side).
        // Adding zero is a no-op, so the non-resuming case needs no
        // special path here.
        if let Some(p) = &progress {
            p.fetch_add(resume_from, std::sync::atomic::Ordering::Relaxed);
        }

        // A resume has to place its bytes at an offset, which is what the
        // windowed path does, so it takes that path at any size.
        if size < STREAM_THRESHOLD && resume_from == 0 {
            let local_file = tokio::fs::File::open(local)
                .await
                .map_err(|e| SshError::Channel(format!("open {}: {e}", local.display())))?;
            let remote_file = self
                .with_op_timeout(&label, async {
                    let s = self.inner.lock().await;
                    s.open_with_flags(
                        &target,
                        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                    )
                    .await
                    .map_err(|e| SshError::Channel(format!("sftp open(W,{target}): {e}")))
                })
                .await?;
            let result = self.pump(&label, local_file, remote_file, progress).await;
            if result.is_err() {
                self.discard_upload_partial(&target, temp_name, 0).await;
                return result;
            }
            return self.claim_upload_target(&target, remote, temp_name).await;
        }

        // Large file: one streaming handle carrying a sliding window of
        // concurrent writes. TRUNCATE clears any prior contents once so a
        // smaller new file can't leave a stale tail; a resume must NOT ask
        // for it, since the bytes it is continuing from are the point.
        let raw = self.open_raw_streaming().await?;
        let mut flags = OpenFlags::WRITE | OpenFlags::CREATE;
        if resume_from == 0 {
            flags |= OpenFlags::TRUNCATE;
        }
        let handle = raw
            .open(&target, flags, FileAttributes::empty())
            .await
            .map_err(|e| SshError::Channel(format!("sftp open(W,{target}): {e}")))?
            .handle;
        let mut input = std::fs::File::open(local)
            .map_err(|e| SshError::Channel(format!("open {}: {e}", local.display())))?;
        let local_disp = local.display().to_string();

        let timeout = self.current_op_timeout();
        let raw_write = raw.clone();
        let handle_write = handle.clone();
        let done = std::sync::atomic::AtomicU64::new(resume_from);
        let result = windowed_upload_copy(
            resume_from,
            size,
            STREAM_CHUNK as u64,
            STREAM_WINDOW,
            timeout,
            &label,
            &done,
            |off, len| {
                use std::io::{Read, Seek, SeekFrom};
                input
                    .seek(SeekFrom::Start(off))
                    .map_err(|e| SshError::Channel(format!("seek {local_disp}: {e}")))?;
                let mut buf = vec![0u8; len as usize];
                input
                    .read_exact(&mut buf)
                    .map_err(|e| SshError::Channel(format!("read {local_disp}: {e}")))?;
                Ok(buf)
            },
            move |off, data| {
                let raw = raw_write.clone();
                let handle = handle_write.clone();
                let prog = progress.clone();
                async move {
                    let n = data.len() as u64;
                    raw.write(handle, off, data)
                        .await
                        .map(|_| ())
                        .map_err(|e| SshError::Channel(format!("sftp write({off}): {e}")))?;
                    if let Some(p) = &prog {
                        p.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(())
                }
            },
        )
        .await;
        // Close flushes the handle server-side; fold its error in so a
        // failed close after a clean copy still surfaces.
        let close = raw
            .close(handle)
            .await
            .map(|_| ())
            .map_err(|e| SshError::Channel(format!("sftp close({target}): {e}")));
        let result = result.and(close);
        if let Err(e) = result {
            self.discard_upload_partial(
                &target,
                temp_name || resume_from > 0,
                done.load(std::sync::atomic::Ordering::Relaxed),
            )
            .await;
            return Err(e);
        }
        self.claim_upload_target(&target, remote, temp_name).await
    }

    /// What to do with the bytes an interrupted upload left behind.
    ///
    /// `keepable` says whether they are somewhere a later resume could use:
    /// a scratch name, or a destination the caller was already resuming
    /// into. Then the file is TRIMMED to the contiguous prefix instead of
    /// removed, because the window can leave holes past it and a hole is
    /// invisible to the tail check that guards the next resume.
    ///
    /// Otherwise the partial sits under the real name, where TRUNCATE
    /// already destroyed whatever was there and its size would masquerade
    /// as a complete upload, so it goes.
    async fn discard_upload_partial(&self, target: &str, keepable: bool, keep: u64) {
        if !keepable {
            let _ = self.remove_file(target).await;
            return;
        }
        // No ftruncate in SFTP: SETSTAT with just a size is the truncate.
        // If the server refuses it the prefix cannot be trusted, and a
        // partial nobody can resume is only a trap, so drop it.
        let attrs = FileAttributes {
            size: Some(keep),
            ..FileAttributes::default()
        };
        let trimmed = match self.open_raw_streaming().await {
            Ok(raw) => raw.setstat(target, attrs).await.is_ok(),
            Err(_) => false,
        };
        if !trimmed {
            let _ = self.remove_file(target).await;
        }
    }

    /// Move a finished scratch upload onto its real name.
    ///
    /// `posix-rename@openssh.com` replaces an existing destination in one
    /// step; plain SFTP v3 `rename` is specified to fail when the target
    /// exists, so the fallback has to clear it first and accepts the window
    /// that opens. Whether the target should be replaced at all was settled
    /// by conflict resolution before any byte moved.
    async fn claim_upload_target(
        &self,
        target: &str,
        remote: &str,
        temp_name: bool,
    ) -> Result<(), SshError> {
        if !temp_name {
            return Ok(());
        }
        if self.posix_rename(target, remote).await.is_ok() {
            return Ok(());
        }
        let _ = self.remove_file(remote).await;
        self.rename(target, remote).await
    }

    /// Relay a file directly between two remote servers, `self` (source)
    /// to `dst` (destination), streaming the bytes through this process
    /// without a local temp file. This is the SFTP-native way to do
    /// "server to server": the protocol has no FXP equivalent (one SSH
    /// channel, nothing to redirect), so the client relays.
    ///
    /// Both handles are opened under their own client's lock and the lock
    /// is released before the pipe runs, so nothing is held across the
    /// transfer and the two sides can't deadlock against each other. On
    /// error the (truncated) destination is best-effort removed.
    ///
    /// Small files stream sequentially over the already-open sessions;
    /// large ones (>= `STREAM_THRESHOLD`) use a concurrent window over
    /// raw sessions on both ends. `size_hint` (e.g. from the source
    /// listing) skips a `stat`; it only gates the path, the windowed
    /// branch re-stats the source handle for the authoritative extent.
    pub async fn relay_to(
        &self,
        src_remote: &str,
        dst: &SftpClient,
        dst_remote: &str,
        size_hint: Option<u64>,
    ) -> Result<(), SshError> {
        self.relay_to_progress(src_remote, dst, dst_remote, size_hint, None)
            .await
    }

    /// Like [`relay_to`](Self::relay_to) but reports bytes transferred into
    /// `progress` (a shared counter the UI polls for a live bar).
    pub async fn relay_to_progress(
        &self,
        src_remote: &str,
        dst: &SftpClient,
        dst_remote: &str,
        size_hint: Option<u64>,
        progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    ) -> Result<(), SshError> {
        let label = format!("relay({src_remote} -> {dst_remote})");
        self.refuse_self_relay(src_remote, dst, dst_remote).await?;
        let size = match size_hint {
            Some(s) => s,
            None => self.stat(src_remote).await?.size,
        };
        let result = if size < STREAM_THRESHOLD {
            self.relay_small(&label, src_remote, dst, dst_remote, progress)
                .await
        } else {
            self.relay_windowed(&label, src_remote, size, dst, dst_remote, progress)
                .await
        };
        if result.is_err() {
            // Don't leave a partial/holed file on the destination server.
            // (TRUNCATE already clobbered any prior file there; preserving
            // it on failure would need a .part + rename, a later refinement.)
            let _ = dst.remove_file(dst_remote).await;
        }
        result
    }

    /// Refuse a relay whose two sides are the same file.
    ///
    /// Both relay branches open the destination `WRITE | CREATE |
    /// TRUNCATE` before reading a single source byte, so a self-relay
    /// would empty the file and the failure cleanup would then remove
    /// it. The app's queue builder happens to prevent this upstream (it
    /// picks a destination name that is free in the destination
    /// directory, and naming the same file requires naming a name that
    /// is taken), but `relay_to` is public API: the MCP handlers and any
    /// future caller get no such protection, so the invariant is
    /// enforced here where the truncate actually happens.
    ///
    /// Only checked when both sides ride the same SSH connection. Two
    /// separate sessions may still reach one machine, but proving that
    /// needs a server identity the client doesn't retain today, and the
    /// error asymmetry says not to guess: a missed detection leaves the
    /// current behaviour, a false positive refuses a legitimate copy.
    ///
    /// Costs nothing on the normal cross-host relay (a pointer
    /// comparison). The two `realpath` round trips are only paid when
    /// the two sides genuinely share a connection.
    async fn refuse_self_relay(
        &self,
        src_remote: &str,
        dst: &SftpClient,
        dst_remote: &str,
    ) -> Result<(), SshError> {
        if !self.shares_session_with(dst) {
            return Ok(());
        }
        let same = if src_remote == dst_remote {
            // Identical strings on one connection need no round trip.
            true
        } else {
            match (
                self.path_identity(src_remote).await,
                dst.path_identity(dst_remote).await,
            ) {
                (Some(a), Some(b)) => a == b,
                // Unresolvable on either side: fall through and let the
                // open report the real error, rather than refusing a
                // transfer on a guess.
                _ => false,
            }
        };
        if same {
            return Err(SshError::Channel(format!(
                "refusing to relay {src_remote} onto itself: source and destination are the same file"
            )));
        }
        Ok(())
    }

    async fn relay_small(
        &self,
        label: &str,
        src_remote: &str,
        dst: &SftpClient,
        dst_remote: &str,
        progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    ) -> Result<(), SshError> {
        let src_file = self
            .with_op_timeout(label, async {
                let s = self.inner.lock().await;
                s.open_with_flags(src_remote, OpenFlags::READ)
                    .await
                    .map_err(|e| SshError::Channel(format!("sftp open src({src_remote}): {e}")))
            })
            .await?;
        let dst_file = dst
            .with_op_timeout(label, async {
                let s = dst.inner.lock().await;
                s.open_with_flags(
                    dst_remote,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                )
                .await
                .map_err(|e| SshError::Channel(format!("sftp open dst({dst_remote}): {e}")))
            })
            .await?;
        // pump_bytes shuts the writer down (closing the dst handle); the
        // src handle closes on drop.
        self.pump(label, src_file, dst_file, progress).await
    }

    async fn relay_windowed(
        &self,
        label: &str,
        src_remote: &str,
        size: u64,
        dst: &SftpClient,
        dst_remote: &str,
        progress: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    ) -> Result<(), SshError> {
        let src_raw = self.open_raw_streaming().await?;
        let src_handle = src_raw
            .open(src_remote, OpenFlags::READ, FileAttributes::empty())
            .await
            .map_err(|e| SshError::Channel(format!("sftp open src({src_remote}): {e}")))?
            .handle;
        // Authoritative size from the source handle; the hint may be stale.
        let actual = match src_raw.fstat(src_handle.clone()).await {
            Ok(a) => a.attrs.size.unwrap_or(size),
            Err(_) => size,
        };
        let dst_raw = dst.open_raw_streaming().await?;
        let dst_handle = dst_raw
            .open(
                dst_remote,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                FileAttributes::empty(),
            )
            .await
            .map_err(|e| SshError::Channel(format!("sftp open dst({dst_remote}): {e}")))?
            .handle;

        let timeout = self.current_op_timeout();
        let src_raw_r = src_raw.clone();
        let src_h = src_handle.clone();
        let dst_raw_w = dst_raw.clone();
        let dst_h = dst_handle.clone();
        let r = windowed_relay_copy(
            actual,
            STREAM_CHUNK as u64,
            STREAM_WINDOW,
            timeout,
            label,
            move |off, len| {
                let raw = src_raw_r.clone();
                let h = src_h.clone();
                async move {
                    raw.read(h, off, len)
                        .await
                        .map(|d| d.data)
                        .map_err(|e| SshError::Channel(format!("sftp relay read({off}): {e}")))
                }
            },
            move |off, data| {
                let raw = dst_raw_w.clone();
                let h = dst_h.clone();
                let prog = progress.clone();
                async move {
                    let n = data.len() as u64;
                    raw.write(h, off, data)
                        .await
                        .map(|_| ())
                        .map_err(|e| SshError::Channel(format!("sftp relay write({off}): {e}")))?;
                    if let Some(p) = &prog {
                        p.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(())
                }
            },
        )
        .await;
        let _ = src_raw.close(src_handle).await;
        let close = dst_raw
            .close(dst_handle)
            .await
            .map(|_| ())
            .map_err(|e| SshError::Channel(format!("sftp close dst({dst_remote}): {e}")));
        r.and(close)
    }

    pub async fn create_dir(&self, path: &str) -> Result<(), SshError> {
        let label = format!("mkdir({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            s.create_dir(path)
                .await
                .map_err(|e| SshError::Channel(format!("sftp mkdir({path}): {e}")))
        })
        .await
    }

    pub async fn remove_file(&self, path: &str) -> Result<(), SshError> {
        let label = format!("rm({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            s.remove_file(path)
                .await
                .map_err(|e| SshError::Channel(format!("sftp rm({path}): {e}")))
        })
        .await
    }

    pub async fn remove_dir(&self, path: &str) -> Result<(), SshError> {
        let label = format!("rmdir({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            s.remove_dir(path)
                .await
                .map_err(|e| SshError::Channel(format!("sftp rmdir({path}): {e}")))
        })
        .await
    }

    /// Apply a new permission bitmask to a remote path. Sends an SFTP
    /// SETSTAT with only the `permissions` field populated; everything
    /// else is `None` (the protocol's flag-driven serialization skips
    /// unset fields, so owner/group/times stay intact). `Default` is
    /// the wrong base, it pre-fills size/uid/permissions and would
    /// nuke other attrs.
    pub async fn chmod(&self, path: &str, mode: u32) -> Result<(), SshError> {
        let label = format!("chmod({path}, {:o})", mode);
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            let mut attrs = russh_sftp::protocol::FileAttributes::empty();
            attrs.permissions = Some(mode);
            s.set_metadata(path.to_string(), attrs)
                .await
                .map_err(|e| SshError::Channel(format!("sftp chmod({path}): {e}")))
        })
        .await
    }

    /// Stat a remote path. Returns just the data the Properties dialog
    /// needs (size, permissions, mtime, owner uid/gid). Symlinks are
    /// followed, we want the target's metadata, not the link itself.
    pub async fn stat(&self, path: &str) -> Result<RemoteStat, SshError> {
        let label = format!("stat({path})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            let meta = s
                .metadata(path.to_string())
                .await
                .map_err(|e| SshError::Channel(format!("sftp stat({path}): {e}")))?;
            Ok(RemoteStat {
                size: meta.size.unwrap_or(0),
                permissions: meta.permissions,
                mtime: meta.mtime,
                uid: meta.uid,
                gid: meta.gid,
            })
        })
        .await
    }

    pub async fn rename(&self, from: &str, to: &str) -> Result<(), SshError> {
        let label = format!("rename({from} → {to})");
        self.with_op_timeout(&label, async {
            let s = self.inner.lock().await;
            s.rename(from, to)
                .await
                .map_err(|e| SshError::Channel(format!("sftp rename({from} → {to}): {e}")))
        })
        .await
    }

    /// Atomically rename `from` over `to` via the
    /// `posix-rename@openssh.com` extension, which replaces an existing
    /// destination in one step. Plain SFTP v3 `rename` (above) is
    /// specified to FAIL when the target exists, and many servers honour
    /// that, so a write-temp-then-replace flow needs this extension to
    /// stay atomic. The high-level `SftpSession` doesn't surface extended
    /// requests, so this runs on a dedicated raw channel. Returns an error
    /// (typically `OpUnsupported`) when the server lacks the extension;
    /// callers that need portability fall back to remove + `rename`.
    pub async fn posix_rename(&self, from: &str, to: &str) -> Result<(), SshError> {
        let raw = self.open_raw_streaming().await?;
        // posix-rename@openssh.com payload: `string oldpath; string
        // newpath`, each an SSH string (u32 big-endian length + bytes).
        let mut data = Vec::with_capacity(8 + from.len() + to.len());
        data.extend_from_slice(&(from.len() as u32).to_be_bytes());
        data.extend_from_slice(from.as_bytes());
        data.extend_from_slice(&(to.len() as u32).to_be_bytes());
        data.extend_from_slice(to.as_bytes());
        let label = format!("posix_rename({from} → {to})");
        self.with_op_timeout(&label, async {
            match raw.extended("posix-rename@openssh.com", data).await {
                Ok(Packet::Status(s)) if s.status_code == StatusCode::Ok => Ok(()),
                Ok(Packet::Status(s)) => Err(SshError::Channel(format!(
                    "sftp posix-rename({from} → {to}): {:?}",
                    s.status_code
                ))),
                Ok(_) => Err(SshError::Channel(
                    "sftp posix-rename: unexpected reply".into(),
                )),
                Err(e) => Err(SshError::Channel(format!(
                    "sftp posix-rename({from} → {to}): {e}"
                ))),
            }
        })
        .await
    }

    /// Open another independent SFTP subsystem channel on the same SSH
    /// connection. The returned client has its own protocol session and
    /// own internal mutex, concurrent calls on the original and the
    /// sibling don't serialize on each other. Used by the parallel
    /// transfer worker pool to actually move bytes in parallel instead
    /// of just queuing on a single channel's mutex.
    pub async fn open_sibling(&self) -> Result<SftpClient, SshError> {
        let timeout = self.open_timeout;
        let handle_for_new = self.handle.clone();
        let inner = async {
            let handle = self.handle.lock().await;
            let channel = handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Channel(format!("sftp sibling channel: {e}")))?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|e| SshError::Channel(format!("sftp sibling subsystem: {e}")))?;
            let session = russh_sftp::client::SftpSession::new(channel.into_stream())
                .await
                .map_err(|e| SshError::Channel(format!("sftp sibling init: {e}")))?;
            Ok::<_, SshError>(session)
        };
        let session = tokio::time::timeout(timeout, inner)
            .await
            .map_err(|_| {
                SshError::Channel(format!(
                    "sftp sibling open timed out after {}s",
                    timeout.as_secs()
                ))
            })??;
        Ok(SftpClient::new(session, handle_for_new, timeout))
    }

    /// Open a low-level `RawSftpSession` on its own fresh channel, used
    /// only for streaming. The high-level `SftpSession` issues one
    /// read/write request per poll (no pipelining); the raw session
    /// exposes offset-addressable `read`/`write` with `&self`, so a single
    /// file handle can carry a sliding window of concurrent requests (the
    /// OpenSSH/WinSCP model). All metadata ops stay on the high-level
    /// session, this is additive. The raw `Limits` default leaves
    /// read/write length uncapped, so the 255 KiB per-request chunk is
    /// safe without negotiating the `limits@openssh.com` extension.
    async fn open_raw_streaming(&self) -> Result<Arc<RawSftpSession>, SshError> {
        let timeout = self.open_timeout;
        let op_secs = self.current_op_timeout().as_secs().max(10);
        let inner = async {
            let handle = self.handle.lock().await;
            let channel = handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Channel(format!("sftp raw channel: {e}")))?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|e| SshError::Channel(format!("sftp raw subsystem: {e}")))?;
            let raw = RawSftpSession::new(channel.into_stream());
            // Align the session's per-request deadline with the client's
            // op timeout so a single 255 KiB request on a slow link isn't
            // killed by the library's 10s default.
            raw.set_timeout(op_secs);
            raw.init()
                .await
                .map_err(|e| SshError::Channel(format!("sftp raw init: {e}")))?;
            Ok::<_, SshError>(raw)
        };
        let raw = tokio::time::timeout(timeout, inner).await.map_err(|_| {
            SshError::Channel(format!(
                "sftp raw open timed out after {}s",
                timeout.as_secs()
            ))
        })??;
        Ok(Arc::new(raw))
    }

    /// Run a one-shot command on a fresh exec channel. Multiplexed onto
    /// the same SSH connection that hosts SFTP, so no extra auth round
    /// trip. Returns `(exit_code, stdout, stderr)`.
    pub async fn exec(&self, command: &str) -> Result<(u32, String, String), SshError> {
        let handle = self.handle.lock().await;
        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Channel(format!("exec channel open: {e}")))?;
        channel
            .exec(true, command)
            .await
            .map_err(|e| SshError::Channel(format!("exec({command}): {e}")))?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<u32> = None;
        // Read until the channel itself closes (`None`). Some servers
        // deliver `ExitStatus` *after* `Eof`, and breaking on Eof leaves
        // `exit_code` defaulted to 255, which is exactly the symptom we
        // hit on `cp -r` ("exit 255") even though the copy succeeded.
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => stdout.extend_from_slice(&data),
                Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                    stderr.extend_from_slice(&data)
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                None => break,
                _ => {}
            }
        }
        Ok((
            exit_code.unwrap_or(255),
            String::from_utf8_lossy(&stdout).into_owned(),
            String::from_utf8_lossy(&stderr).into_owned(),
        ))
    }

    /// Recursive directory delete. SFTP's `remove_dir` only handles empty
    /// dirs and walking the tree from the client side is slow over a
    /// high-latency link, so we shell out to `rm -rf` on the remote, same
    /// connection, separate channel. Path is single-quoted with the POSIX
    /// `'\''` escape so embedded quotes don't break out of the literal.
    pub async fn remove_dir_recursive(&self, path: &str) -> Result<(), SshError> {
        let escaped = path.replace('\'', "'\\''");
        let cmd = format!("rm -rf -- '{}'", escaped);
        let (code, _stdout, stderr) = self.exec(&cmd).await?;
        if code == 0 {
            Ok(())
        } else {
            let err = stderr.trim();
            let detail = if err.is_empty() {
                format!("rm -rf exited with code {}", code)
            } else {
                err.to_string()
            };
            Err(SshError::Channel(format!("rm -rf {path}: {detail}")))
        }
    }

    /// Open `path` for positioned reads. Powers the zip virtual-browse
    /// path: the archive's central directory and individual entry
    /// ranges are fetched with ranged reads instead of downloading the
    /// file. Rides its own raw session (like the streaming transfers)
    /// so metadata ops on the main session never queue behind range
    /// fetches.
    pub async fn open_ranged(&self, path: &str) -> Result<RemoteRangedFile, SshError> {
        let raw = self.open_raw_streaming().await?;
        let handle = raw
            .open(path, OpenFlags::READ, FileAttributes::empty())
            .await
            .map_err(|e| SshError::Channel(format!("sftp open({path}): {e}")))?
            .handle;
        let attrs = raw
            .fstat(handle.clone())
            .await
            .map_err(|e| SshError::Channel(format!("sftp fstat({path}): {e}")))?;
        let len = attrs.attrs.size.unwrap_or(0);
        Ok(RemoteRangedFile {
            raw,
            handle,
            len,
            timeout: self.current_op_timeout(),
        })
    }
}

/// Random-access read handle over one remote file (see
/// [`SftpClient::open_ranged`]). Cheap to keep around: one raw SFTP
/// channel + one open server-side handle.
pub struct RemoteRangedFile {
    raw: Arc<RawSftpSession>,
    handle: String,
    len: u64,
    timeout: std::time::Duration,
}

impl std::fmt::Debug for RemoteRangedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteRangedFile")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl RemoteRangedFile {
    /// File size as reported by `fstat` at open time.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Read `[offset, offset + want)`, clamped to EOF, issuing up to
    /// `STREAM_WINDOW` concurrent SFTP requests (each capped at the
    /// protocol's 255 KiB). Returns the bytes; empty only at/after EOF.
    pub async fn read_at(&self, offset: u64, want: usize) -> Result<Vec<u8>, SshError> {
        if offset >= self.len || want == 0 {
            return Ok(Vec::new());
        }
        let span = (self.len - offset).min(want as u64);
        let mut buf = vec![0u8; span as usize];
        let raw = self.raw.clone();
        let handle = self.handle.clone();
        // The range lands in memory, so there is nothing to resume from and
        // nothing to trim: this reads `[0, span)` of a fresh buffer and
        // discards the contiguous-prefix counter.
        let done = std::sync::atomic::AtomicU64::new(0);
        windowed_download_copy(
            0,
            span,
            STREAM_CHUNK as u64,
            STREAM_WINDOW,
            self.timeout,
            "ranged read",
            &done,
            move |off, len| {
                let raw = raw.clone();
                let handle = handle.clone();
                let abs = offset + off;
                async move {
                    raw.read(handle, abs, len)
                        .await
                        .map(|d| d.data)
                        .map_err(|e| SshError::Channel(format!("sftp read({abs}): {e}")))
                }
            },
            |off, data: Vec<u8>| {
                buf[off as usize..off as usize + data.len()].copy_from_slice(&data);
                Ok(())
            },
        )
        .await?;
        Ok(buf)
    }

    /// Close the remote handle. Best-effort: dropping the struct also
    /// tears the raw session's channel down, this just returns the
    /// server-side handle promptly.
    pub async fn close(self) {
        let _ = self.raw.close(self.handle).await;
    }
}

/// Per-request payload size: the SFTP protocol's 255 KiB ceiling,
/// matching `MAX_READ_LENGTH` / `MAX_WRITE_LENGTH` in russh-sftp. Larger
/// requests would be capped (or rejected) per request.
const STREAM_CHUNK: usize = 255 * 1024;

/// Files at or above this size use the windowed concurrent path; smaller
/// ones stay on the single-handle sequential pump (less setup, and the
/// extra channel + window machinery would not pay for itself).
const STREAM_THRESHOLD: u64 = 8 * 1024 * 1024;

/// Number of requests kept in flight on the one streaming handle. The
/// `russh_sftp` session multiplexes responses by request id, so a single
/// handle can carry many concurrent reads/writes (the OpenSSH/WinSCP
/// model). 16 is a deep-enough window for high-latency links without
/// flooding the server.
const STREAM_WINDOW: usize = 16;

/// Suffix of the scratch file a transfer writes into before it is renamed
/// onto the caller's target. Deliberately NOT the conventional `.part`:
/// one of these can outlive a crash in a user's download folder or on
/// their server, so it should say who left it, and a file the user
/// legitimately named `x.part` must never be mistaken for our scratch.
const PART_SUFFIX: &str = ".oryxis-part";

/// How much of the already-transferred tail is compared before a resume is
/// allowed. One SFTP read, so the check costs a single round trip.
///
/// This is a heuristic and the code must not pretend otherwise: no SFTP
/// server we can rely on computes checksums (OpenSSH 9.6's `sftp-server`
/// advertises `posix-rename`, `statvfs`, `fstatvfs`, `hardlink`, `fsync`,
/// `limits`, `expand-path`, `copy-data`, `home-directory` and
/// `users-groups-by-id`, and no hash extension at all), so a full proof
/// would mean re-reading every byte already transferred. What this catches
/// is the honest mistake, a partial belonging to a completely different
/// file. What it cannot catch: large runs of zeroes (VM images, sparse
/// files, preallocated databases, tar padding) match on both sides, and a
/// versioned prefix (the destination holds v1, the source is now v2
/// sharing its first N bytes) matches because it genuinely IS a prefix,
/// of the wrong file. Callers that resume on the user's behalf must say
/// so; see `UploadOptions::resume`.
const RESUME_VERIFY_BYTES: u64 = 64 * 1024;

/// How far back from a partial's length a resume starts.
///
/// A graceful failure trims the partial to its contiguous prefix, so its
/// length is exact. A CRASH (kill, power loss, OOM) trims nothing, and
/// what is left is a file whose length is the highest offset the window
/// reached, possibly with holes below it. The tail check cannot see such a
/// hole, because the tail is one of the chunks that DID land.
///
/// The window itself bounds the damage. Chunks are dispatched in
/// increasing offset order and at most `STREAM_WINDOW` are ever in flight,
/// so at the moment of a crash the unwritten gaps all sit within
/// `STREAM_WINDOW * STREAM_CHUNK` bytes of the end. Everything below that
/// is solid, whatever happened. Rewinding by that much unconditionally is
/// what makes a crash and a clean failure the same case: it costs a few
/// megabytes of re-transfer on a path that already trimmed itself, and it
/// removes an entire class of silent corruption on the path that could
/// not. Do not "optimise" it away by trusting the length.
const RESUME_REWIND: u64 = STREAM_WINDOW as u64 * STREAM_CHUNK as u64;

/// How much of a `have`-byte partial can be continued, for a source of
/// `size` bytes. Zero means "there is nothing worth continuing", which
/// callers treat as an ordinary from-scratch transfer.
///
/// The rewind exists ONLY because the sliding window can leave holes,
/// and the window only ever runs at or above [`STREAM_THRESHOLD`].
/// Below it the sequential pump writes strictly in order, so a partial
/// of a small file has no holes to protect against and its whole length
/// is real. Rewinding there anyway made resume a no-op for every file
/// smaller than the rewind itself: a 1.2 MB upload could never have a
/// 4 MB partial, so "continue" silently became "start over".
///
/// This is the ONE authority on the question. The UI asks it before
/// offering to continue, so it cannot offer something the engine will
/// then decline to do.
pub fn resume_offset(size: u64, have: u64) -> u64 {
    if have == 0 || have >= size {
        return 0;
    }
    if size < STREAM_THRESHOLD {
        return have;
    }
    have.saturating_sub(RESUME_REWIND)
}

/// Longest single path component almost every filesystem accepts, in
/// bytes (`NAME_MAX` on Linux, and the same number on NTFS and APFS).
const NAME_MAX: usize = 255;

/// Shorten `name` so that appending [`PART_SUFFIX`] still fits a path
/// component. Trimming from the end can make two very long names share a
/// scratch file, which is strictly better than failing to transfer at all,
/// and the tail check is what stops a collision from being spliced into
/// the wrong file.
fn fit_part_name(name: &str) -> String {
    let mut base = name.to_string();
    let budget = NAME_MAX.saturating_sub(PART_SUFFIX.len());
    while base.len() > budget {
        // Pops a whole char, so the result stays valid UTF-8.
        base.pop();
    }
    format!("{base}{PART_SUFFIX}")
}

/// Scratch path a download writes into, next to its target.
///
/// The suffix has to FIT: a name already near the limit would otherwise
/// fail to create, turning a long filename into a failed download.
pub(crate) fn part_path(local: &std::path::Path) -> std::path::PathBuf {
    let len = local.file_name().map(|n| n.len()).unwrap_or(0);
    if len + PART_SUFFIX.len() <= NAME_MAX {
        // The common case, byte-exact: a name that is not valid UTF-8
        // (legal on Unix) survives untouched.
        let mut raw = local.as_os_str().to_os_string();
        raw.push(PART_SUFFIX);
        return std::path::PathBuf::from(raw);
    }
    let name = local.file_name().unwrap_or_default().to_string_lossy();
    local.with_file_name(fit_part_name(&name))
}

/// Scratch path an upload writes into, beside its target on the server.
fn remote_part_path(remote: &str) -> String {
    let (dir, name) = match remote.rsplit_once('/') {
        Some((d, n)) => (d, n),
        None => ("", remote),
    };
    if name.len() + PART_SUFFIX.len() <= NAME_MAX {
        return format!("{remote}{PART_SUFFIX}");
    }
    format!("{dir}/{}", fit_part_name(name))
}

/// Read `[from, from + span)` out of a local file. Blocking, like the rest
/// of the local half of the streaming paths (a 64 KiB read next to a
/// multi-gigabyte transfer is not worth a `spawn_blocking`).
fn read_local_range(
    local: &std::path::Path,
    from: u64,
    span: usize,
) -> Result<Vec<u8>, SshError> {
    use std::io::{Read, Seek, SeekFrom};
    let disp = local.display();
    let mut f = std::fs::File::open(local).map_err(|e| SshError::Channel(format!("open {disp}: {e}")))?;
    f.seek(SeekFrom::Start(from))
        .map_err(|e| SshError::Channel(format!("seek {disp}: {e}")))?;
    let mut buf = vec![0u8; span];
    f.read_exact(&mut buf)
        .map_err(|e| SshError::Channel(format!("read {disp}: {e}")))?;
    Ok(buf)
}

/// Move a finished scratch file onto its target.
///
/// The rename is tried FIRST so the Unix path keeps its atomicity (rename
/// over an existing file is one step there); only when that fails does the
/// target get cleared, which is what Windows needs since its rename
/// refuses an existing destination. Whether the target should be replaced
/// at all was already decided by conflict resolution, before any byte
/// moved.
async fn finish_part(part: &std::path::Path, target: &std::path::Path) -> Result<(), SshError> {
    if tokio::fs::rename(part, target).await.is_ok() {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(target).await;
    tokio::fs::rename(part, target).await.map_err(|e| {
        SshError::Channel(format!(
            "rename {} -> {}: {e}",
            part.display(),
            target.display()
        ))
    })
}

/// Copy bytes from `reader` to `writer` in bounded 255 KiB chunks (the
/// SFTP per-request ceiling, matching `MAX_READ_LENGTH` /
/// `MAX_WRITE_LENGTH` in russh-sftp). `russh_sftp::File` issues exactly
/// one read/write request per poll and awaits it before the next (no
/// pipelining), so the chunk size IS the throughput knob and memory
/// stays flat regardless of file size.
///
/// Awaits `writer.shutdown()` at the end so the close round-trip and any
/// late error surface here rather than in `File`'s background `Drop`.
/// Each step rides `timeout`: a stalled link trips it, a healthy long
/// transfer resets it per chunk.
///
/// Free function (not a method) so it's unit-testable with in-memory
/// streams and reusable as the server-to-server relay primitive: piping
/// one remote `File` straight into another needs exactly this, no local
/// detour.
async fn pump_bytes<R, W>(
    mut reader: R,
    mut writer: W,
    timeout: std::time::Duration,
    op_name: &str,
    progress: Option<&std::sync::atomic::AtomicU64>,
) -> Result<(), SshError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let timed = |label: &str| {
        SshError::Channel(format!(
            "sftp {op_name} {label} timed out after {}s",
            timeout.as_secs()
        ))
    };
    let mut buf = vec![0u8; STREAM_CHUNK];
    loop {
        let n = match tokio::time::timeout(timeout, reader.read(&mut buf)).await {
            Ok(r) => r.map_err(|e| SshError::Channel(format!("sftp {op_name} read: {e}")))?,
            Err(_) => return Err(timed("read")),
        };
        if n == 0 {
            break;
        }
        match tokio::time::timeout(timeout, writer.write_all(&buf[..n])).await {
            Ok(r) => r.map_err(|e| SshError::Channel(format!("sftp {op_name} write: {e}")))?,
            Err(_) => return Err(timed("write")),
        }
        if let Some(p) = progress {
            p.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        }
    }
    match tokio::time::timeout(timeout, writer.shutdown()).await {
        Ok(r) => r.map_err(|e| SshError::Channel(format!("sftp {op_name} close: {e}")))?,
        Err(_) => return Err(timed("close")),
    }
    Ok(())
}

/// One completed windowed read: `(offset, requested_len, bytes)`. The
/// requested length rides along so a short read can re-queue its
/// remainder.
type ReadChunk = Result<(u64, u32, Vec<u8>), SshError>;

/// Download `[0, size)` with a sliding window of `window` concurrent
/// reads. `read_at(offset, len)` issues one remote read (these run on
/// spawned tasks, so they pipeline); `write_at(offset, data)` positions
/// the bytes into the destination and is called serially as completions
/// arrive (so it needs no locking). Short reads (server returns fewer
/// bytes than asked) re-queue the remainder. Generic over both sides so
/// the byte-level logic is unit-tested with in-memory fakes.
#[allow(clippy::too_many_arguments)]
async fn windowed_download_copy<RDR, FRDR, WRT>(
    from: u64,
    size: u64,
    chunk: u64,
    window: usize,
    timeout: std::time::Duration,
    op_name: &str,
    done: &std::sync::atomic::AtomicU64,
    read_at: RDR,
    mut write_at: WRT,
) -> Result<(), SshError>
where
    RDR: Fn(u64, u32) -> FRDR,
    FRDR: std::future::Future<Output = Result<Vec<u8>, SshError>> + Send + 'static,
    WRT: FnMut(u64, Vec<u8>) -> Result<(), SshError>,
{
    let mut pending: std::collections::VecDeque<(u64, u32)> = std::collections::VecDeque::new();
    let mut at = from;
    while at < size {
        let len = chunk.min(size - at) as u32;
        pending.push_back((at, len));
        at += len as u64;
    }
    // Chunks finish out of order, so the only honest measure of "how much
    // of this destination is real" is the CONTIGUOUS prefix, never the
    // file's length: a chunk that landed past a hole would otherwise make
    // a later resume start beyond bytes that were never written, and the
    // tail check cannot see a hole in the middle. `ahead` holds the
    // completed ranges waiting for that prefix to reach them.
    let mut ahead: std::collections::BTreeMap<u64, u64> = std::collections::BTreeMap::new();
    let mut contiguous = from;
    done.store(contiguous, std::sync::atomic::Ordering::Relaxed);
    let mut inflight: tokio::task::JoinSet<ReadChunk> = tokio::task::JoinSet::new();
    loop {
        while inflight.len() < window {
            let Some((off, len)) = pending.pop_front() else {
                break;
            };
            // Build the read future here (it's the `Send + 'static` part);
            // the task holds only that, not the `read_at` closure itself.
            let fut = read_at(off, len);
            let op = op_name.to_string();
            inflight.spawn(async move {
                let data = tokio::time::timeout(timeout, fut).await.map_err(|_| {
                    SshError::Channel(format!(
                        "sftp {op} read timed out after {}s",
                        timeout.as_secs()
                    ))
                })??;
                Ok((off, len, data))
            });
        }
        let Some(joined) = inflight.join_next().await else {
            break;
        };
        let (off, reqlen, data) =
            joined.map_err(|e| SshError::Channel(format!("sftp {op_name} read task: {e}")))??;
        if data.is_empty() {
            return Err(SshError::Channel(format!(
                "sftp {op_name} unexpected empty read at offset {off}"
            )));
        }
        // A reply larger than the request is a protocol violation
        // (russh-sftp does not cap reply payloads); positioning sinks
        // slice fixed buffers by `off + data.len()`, so an over-long
        // reply from a hostile server must error here, not panic there.
        if data.len() as u64 > reqlen as u64 {
            return Err(SshError::Channel(format!(
                "sftp {op_name} read at offset {off} returned {} bytes for a {reqlen}-byte request",
                data.len()
            )));
        }
        let got = data.len() as u32;
        write_at(off, data)?;
        ahead.insert(off, off + got as u64);
        while let Some(end) = ahead.remove(&contiguous) {
            contiguous = end;
        }
        done.store(contiguous, std::sync::atomic::Ordering::Relaxed);
        if got < reqlen {
            pending.push_front((off + got as u64, reqlen - got));
        }
    }
    Ok(())
}

/// Upload `[0, size)` with a sliding window of `window` concurrent writes.
/// `read_at(offset, len)` reads one chunk from the source and is called
/// serially (cheap local reads); `write_at(offset, data)` issues one
/// remote write (spawned, so they pipeline). SFTP writes are all-or-error,
/// so there is no short-write case to re-queue. Generic over both sides
/// for the same in-memory unit testing as the download path.
#[allow(clippy::too_many_arguments)]
async fn windowed_upload_copy<RDR, WRT, FWRT>(
    from: u64,
    size: u64,
    chunk: u64,
    window: usize,
    timeout: std::time::Duration,
    op_name: &str,
    done: &std::sync::atomic::AtomicU64,
    mut read_at: RDR,
    write_at: WRT,
) -> Result<(), SshError>
where
    RDR: FnMut(u64, u32) -> Result<Vec<u8>, SshError>,
    WRT: Fn(u64, Vec<u8>) -> FWRT,
    FWRT: std::future::Future<Output = Result<(), SshError>> + Send + 'static,
{
    let mut inflight: tokio::task::JoinSet<Result<(u64, u64), SshError>> =
        tokio::task::JoinSet::new();
    let mut off = from;
    // Same contiguous-prefix bookkeeping as the download side, and for the
    // same reason: the destination's SIZE after a failure is the highest
    // offset the window happened to reach, which can sit past a hole.
    let mut ahead: std::collections::BTreeMap<u64, u64> = std::collections::BTreeMap::new();
    let mut contiguous = from;
    done.store(contiguous, std::sync::atomic::Ordering::Relaxed);
    loop {
        while inflight.len() < window && off < size {
            let len = chunk.min(size - off) as u32;
            let data = read_at(off, len)?;
            // Build the write future here (the `Send + 'static` part); the
            // task holds only that, not the `write_at` closure.
            let fut = write_at(off, data);
            let op = op_name.to_string();
            inflight.spawn(async move {
                tokio::time::timeout(timeout, fut).await.map_err(|_| {
                    SshError::Channel(format!(
                        "sftp {op} write timed out after {}s",
                        timeout.as_secs()
                    ))
                })??;
                Ok((off, len as u64))
            });
            off += len as u64;
        }
        let Some(joined) = inflight.join_next().await else {
            if off >= size {
                break;
            }
            continue;
        };
        let (wrote_at, wrote_len) =
            joined.map_err(|e| SshError::Channel(format!("sftp {op_name} write task: {e}")))??;
        ahead.insert(wrote_at, wrote_at + wrote_len);
        while let Some(end) = ahead.remove(&contiguous) {
            contiguous = end;
        }
        done.store(contiguous, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

/// Relay `[0, size)` from a remote source to a remote destination with a
/// sliding window of `window` concurrent chunk tasks. Each task reads its
/// whole 255 KiB region from the source (looping over short reads), then
/// writes it to the destination at the same offset. Positioned writes make
/// completion order irrelevant, so coupling read+write per task pipelines
/// fine without a reorder buffer. Both `read_at` and `write_at` are remote
/// async ops; memory is bounded at ~`window` x chunk. Generic over both
/// for in-memory unit testing.
async fn windowed_relay_copy<RDR, FRDR, WRT, FWRT>(
    size: u64,
    chunk: u64,
    window: usize,
    timeout: std::time::Duration,
    op_name: &str,
    read_at: RDR,
    write_at: WRT,
) -> Result<(), SshError>
where
    RDR: Fn(u64, u32) -> FRDR + Clone + Send + 'static,
    FRDR: std::future::Future<Output = Result<Vec<u8>, SshError>> + Send + 'static,
    WRT: Fn(u64, Vec<u8>) -> FWRT + Clone + Send + 'static,
    FWRT: std::future::Future<Output = Result<(), SshError>> + Send + 'static,
{
    let mut inflight: tokio::task::JoinSet<Result<(), SshError>> = tokio::task::JoinSet::new();
    let mut off = 0u64;
    loop {
        while inflight.len() < window && off < size {
            let len = chunk.min(size - off) as u32;
            let this_off = off;
            let rd = read_at.clone();
            let wr = write_at.clone();
            let op = op_name.to_string();
            inflight.spawn(async move {
                // Read the whole chunk from the source, looping over short
                // reads. An empty read before the chunk is full means the
                // source shrank, fail loudly rather than write a short hole.
                let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
                while (buf.len() as u32) < len {
                    let want = len - buf.len() as u32;
                    let part = tokio::time::timeout(timeout, rd(this_off + buf.len() as u64, want))
                        .await
                        .map_err(|_| {
                            SshError::Channel(format!(
                                "sftp {op} relay read timed out after {}s",
                                timeout.as_secs()
                            ))
                        })??;
                    if part.is_empty() {
                        return Err(SshError::Channel(format!(
                            "sftp {op} relay: source shrank at offset {}",
                            this_off + buf.len() as u64
                        )));
                    }
                    // Same protocol-violation guard as the download path:
                    // an over-long reply would spill past this chunk and
                    // silently corrupt the destination layout.
                    if part.len() as u64 > want as u64 {
                        return Err(SshError::Channel(format!(
                            "sftp {op} relay read at offset {} returned {} bytes for a {want}-byte request",
                            this_off + buf.len() as u64,
                            part.len()
                        )));
                    }
                    buf.extend_from_slice(&part);
                }
                tokio::time::timeout(timeout, wr(this_off, buf))
                    .await
                    .map_err(|_| {
                        SshError::Channel(format!(
                            "sftp {op} relay write timed out after {}s",
                            timeout.as_secs()
                        ))
                    })??;
                Ok(())
            });
            off += len as u64;
        }
        let Some(joined) = inflight.join_next().await else {
            if off >= size {
                break;
            }
            continue;
        };
        joined.map_err(|e| SshError::Channel(format!("sftp {op_name} relay task: {e}")))??;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        pump_bytes, windowed_download_copy, windowed_relay_copy, windowed_upload_copy, SshError,
    };
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Run the pump over an in-memory source and assert the sink received
    /// the exact bytes. Exercises the real chunk loop with no SSH server.
    async fn round_trip(len: usize) {
        let src: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let mut dst: Vec<u8> = Vec::new();
        // `&[u8]` is AsyncRead, `&mut Vec<u8>` is AsyncWrite (shutdown is
        // a no-op for Vec), so this drives the loop end to end.
        pump_bytes(src.as_slice(), &mut dst, Duration::from_secs(5), "test", None)
            .await
            .expect("pump_bytes");
        assert_eq!(dst, src, "round trip mismatch at len {len}");
    }

    #[tokio::test]
    async fn pump_empty_file() {
        round_trip(0).await;
    }

    #[tokio::test]
    async fn pump_sub_chunk() {
        round_trip(1024).await;
    }

    #[tokio::test]
    async fn pump_exact_one_chunk() {
        round_trip(255 * 1024).await;
    }

    #[tokio::test]
    async fn pump_exact_two_chunks() {
        round_trip(510 * 1024).await;
    }

    #[tokio::test]
    async fn pump_multi_chunk_with_remainder() {
        round_trip(600 * 1024).await;
    }

    fn pattern(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    /// Drive `windowed_download_copy` over in-memory fakes: a remote
    /// "read" that returns the requested slice, and a positioned write
    /// into a destination buffer. Asserts byte-identical reassembly.
    async fn download_round_trip(size: usize, chunk: usize, window: usize) {
        download_round_trip_from(0, size, chunk, window).await;
    }

    /// As above but starting at `from`, the resume case: everything below
    /// the offset must be left exactly as the destination already had it,
    /// and the contiguous counter must end at the full size.
    async fn download_round_trip_from(from: usize, size: usize, chunk: usize, window: usize) {
        let src = pattern(size);
        // Sentinel below `from` stands in for bytes a previous attempt
        // already wrote: the copy must not touch them.
        let mut dst = vec![0u8; size];
        dst[..from].fill(0xEE);
        let src_read = src.clone();
        let done = std::sync::atomic::AtomicU64::new(0);
        windowed_download_copy(
            from as u64,
            size as u64,
            chunk as u64,
            window,
            Duration::from_secs(5),
            "test",
            &done,
            move |off, len| {
                let o = off as usize;
                let end = (o + len as usize).min(src_read.len());
                let data = src_read[o..end].to_vec();
                async move { Ok(data) }
            },
            |off, data| {
                let o = off as usize;
                dst[o..o + data.len()].copy_from_slice(&data);
                Ok(())
            },
        )
        .await
        .expect("download");
        assert_eq!(
            done.load(std::sync::atomic::Ordering::Relaxed),
            size as u64,
            "contiguous prefix must reach the end (from={from} size={size})"
        );
        assert!(
            dst[..from].iter().all(|b| *b == 0xEE),
            "resume overwrote bytes below the start offset (from={from})"
        );
        assert_eq!(
            dst[from..],
            src[from..],
            "download reassembly mismatch from={from} size={size}"
        );
    }

    /// Same, but the fake remote returns at most 1000 bytes per read
    /// regardless of the requested length, forcing the short-read
    /// re-queue path so its offset math is exercised.
    async fn download_short_reads(size: usize, chunk: usize, window: usize) {
        let src = pattern(size);
        let mut dst = vec![0u8; size];
        let src_read = src.clone();
        let done = std::sync::atomic::AtomicU64::new(0);
        windowed_download_copy(
            0,
            size as u64,
            chunk as u64,
            window,
            Duration::from_secs(5),
            "test",
            &done,
            move |off, len| {
                let o = off as usize;
                let cap = (len as usize).min(1000);
                let end = (o + cap).min(src_read.len());
                let data = src_read[o..end].to_vec();
                async move { Ok(data) }
            },
            |off, data| {
                let o = off as usize;
                dst[o..o + data.len()].copy_from_slice(&data);
                Ok(())
            },
        )
        .await
        .expect("download short");
        assert_eq!(dst, src, "short-read reassembly mismatch size={size}");
    }

    /// A reply longer than the request is a protocol violation from a
    /// hostile / non-conforming server: the pump must surface an error
    /// instead of letting a positioning sink slice out of bounds.
    #[tokio::test]
    async fn download_overlong_reply_errors() {
        let mut dst = vec![0u8; 1024];
        let done = std::sync::atomic::AtomicU64::new(0);
        let result = windowed_download_copy(
            0,
            1024,
            1024,
            2,
            Duration::from_secs(5),
            "test",
            &done,
            move |_off, len| {
                // Return double the requested bytes.
                let data = vec![0xAAu8; len as usize * 2];
                async move { Ok(data) }
            },
            |off, data| {
                let o = off as usize;
                dst[o..o + data.len()].copy_from_slice(&data);
                Ok(())
            },
        )
        .await;
        let err = result.expect_err("over-long reply must error");
        assert!(
            err.to_string().contains("bytes for a"),
            "unexpected error: {err}"
        );
    }

    /// Drive `windowed_upload_copy`: serial source reads + concurrent
    /// positioned writes into a shared buffer. Asserts byte-identical.
    async fn upload_round_trip(size: usize, chunk: usize, window: usize) {
        upload_round_trip_from(0, size, chunk, window).await;
    }

    /// As above but starting at `from`, the resume case, with the same two
    /// extra assertions as the download side.
    async fn upload_round_trip_from(from: usize, size: usize, chunk: usize, window: usize) {
        let src = pattern(size);
        let mut initial = vec![0u8; size];
        initial[..from].fill(0xEE);
        let dst = Arc::new(Mutex::new(initial));
        let src_read = src.clone();
        let dst_write = dst.clone();
        let done = std::sync::atomic::AtomicU64::new(0);
        windowed_upload_copy(
            from as u64,
            size as u64,
            chunk as u64,
            window,
            Duration::from_secs(5),
            "test",
            &done,
            move |off, len| {
                let o = off as usize;
                Ok(src_read[o..o + len as usize].to_vec())
            },
            move |off, data| {
                let dst = dst_write.clone();
                async move {
                    let o = off as usize;
                    dst.lock().unwrap()[o..o + data.len()].copy_from_slice(&data);
                    Ok(())
                }
            },
        )
        .await
        .expect("upload");
        let out = dst.lock().unwrap();
        assert_eq!(
            done.load(std::sync::atomic::Ordering::Relaxed),
            size as u64,
            "contiguous prefix must reach the end (from={from} size={size})"
        );
        assert!(
            out[..from].iter().all(|b| *b == 0xEE),
            "resume overwrote bytes below the start offset (from={from})"
        );
        assert_eq!(
            out[from..],
            src[from..],
            "upload reassembly mismatch from={from} size={size}"
        );
    }

    #[tokio::test]
    async fn windowed_download_even() {
        download_round_trip(600 * 1024, 100 * 1024, 4).await;
    }

    #[tokio::test]
    async fn windowed_download_ragged_and_prime() {
        download_round_trip(1_000_003, 99_991, 8).await;
    }

    #[tokio::test]
    async fn windowed_download_smaller_than_window() {
        // Fewer chunks than the window: priming must not over-pop.
        download_round_trip(50 * 1024, 100 * 1024, 16).await;
    }

    #[tokio::test]
    async fn windowed_download_short_reads_requeue() {
        download_short_reads(600 * 1024, 100 * 1024, 4).await;
    }

    #[tokio::test]
    async fn windowed_upload_even() {
        upload_round_trip(600 * 1024, 100 * 1024, 4).await;
    }

    #[tokio::test]
    async fn windowed_upload_ragged_and_prime() {
        upload_round_trip(1_000_003, 99_991, 8).await;
    }

    #[tokio::test]
    async fn windowed_upload_smaller_than_window() {
        upload_round_trip(50 * 1024, 100 * 1024, 16).await;
    }

    /// Resume from a chunk boundary and from the middle of one: the
    /// window has to re-derive its chunk grid from the start offset, so an
    /// offset that is not a multiple of the chunk size is the interesting
    /// case (the first chunk of the resumed run is a short one).
    #[tokio::test]
    async fn windowed_download_resume_chunk_aligned() {
        download_round_trip_from(200 * 1024, 600 * 1024, 100 * 1024, 4).await;
    }

    #[tokio::test]
    async fn windowed_download_resume_mid_chunk() {
        download_round_trip_from(250 * 1024 + 7, 600 * 1024, 100 * 1024, 4).await;
    }

    #[tokio::test]
    async fn windowed_download_resume_last_chunk() {
        download_round_trip_from(599 * 1024, 600 * 1024, 100 * 1024, 8).await;
    }

    #[tokio::test]
    async fn windowed_upload_resume_chunk_aligned() {
        upload_round_trip_from(200 * 1024, 600 * 1024, 100 * 1024, 4).await;
    }

    #[tokio::test]
    async fn windowed_upload_resume_mid_chunk() {
        upload_round_trip_from(250 * 1024 + 7, 600 * 1024, 100 * 1024, 4).await;
    }

    #[tokio::test]
    async fn windowed_upload_resume_last_chunk() {
        upload_round_trip_from(599 * 1024, 600 * 1024, 100 * 1024, 8).await;
    }

    /// THE invariant behind resume. The first chunk fails, slowly, so
    /// every later chunk in the window lands before the failure surfaces.
    /// The destination then holds real bytes ABOVE a hole, and its LENGTH
    /// would claim the transfer got far. Resuming from that length would
    /// skip the hole forever, and the tail check cannot see it (the tail
    /// is one of the chunks that did land). The contiguous prefix is the
    /// only honest answer, and here it is zero.
    #[tokio::test]
    async fn windowed_download_hole_does_not_advance_prefix() {
        let size = 600 * 1024usize;
        let mut dst = vec![0u8; size];
        let src = Arc::new(pattern(size));
        let done = std::sync::atomic::AtomicU64::new(0);
        let result = windowed_download_copy(
            0,
            size as u64,
            100 * 1024,
            8,
            Duration::from_secs(5),
            "test",
            &done,
            move |off, len| {
                let src = src.clone();
                async move {
                    if off == 0 {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        return Err(SshError::Channel("first chunk fails".into()));
                    }
                    let o = off as usize;
                    Ok(src[o..(o + len as usize).min(src.len())].to_vec())
                }
            },
            |off, data| {
                let o = off as usize;
                dst[o..o + data.len()].copy_from_slice(&data);
                Ok(())
            },
        )
        .await;
        result.expect_err("the failing chunk must fail the copy");
        assert_eq!(
            done.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "bytes written past a hole must not count as resumable progress"
        );
    }

    /// The upload mirror: same hole, same reason. Here the consequence is
    /// worse if it were wrong, since the destination is the user's server
    /// and the hole would be zeroes inside their file.
    #[tokio::test]
    async fn windowed_upload_hole_does_not_advance_prefix() {
        let size = 600 * 1024usize;
        let src = pattern(size);
        let dst = Arc::new(Mutex::new(vec![0u8; size]));
        let src_read = src.clone();
        let dst_write = dst.clone();
        let done = std::sync::atomic::AtomicU64::new(0);
        let result = windowed_upload_copy(
            0,
            size as u64,
            100 * 1024,
            8,
            Duration::from_secs(5),
            "test",
            &done,
            move |off, len| {
                let o = off as usize;
                Ok(src_read[o..o + len as usize].to_vec())
            },
            move |off, data| {
                let dst = dst_write.clone();
                async move {
                    if off == 0 {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        return Err(SshError::Channel("first chunk fails".into()));
                    }
                    let o = off as usize;
                    dst.lock().unwrap()[o..o + data.len()].copy_from_slice(&data);
                    Ok(())
                }
            },
        )
        .await;
        result.expect_err("the failing chunk must fail the copy");
        assert_eq!(
            done.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "bytes written past a hole must not count as resumable progress"
        );
    }

    /// A resumed run reassembles byte-identically to a from-zero one. The
    /// point is not that both are correct in isolation, it is that the
    /// SEAM at the resume offset carries no gap and no overlap.
    #[tokio::test]
    async fn resumed_download_matches_from_zero() {
        let size = 1_000_003usize;
        let chunk = 99_991usize;
        let src = pattern(size);
        let cut = 400_000usize;

        let mut fresh = vec![0u8; size];
        let src_a = src.clone();
        let done_a = std::sync::atomic::AtomicU64::new(0);
        windowed_download_copy(
            0,
            size as u64,
            chunk as u64,
            8,
            Duration::from_secs(5),
            "test",
            &done_a,
            move |off, len| {
                let o = off as usize;
                let data = src_a[o..(o + len as usize).min(src_a.len())].to_vec();
                async move { Ok(data) }
            },
            |off, data| {
                let o = off as usize;
                fresh[o..o + data.len()].copy_from_slice(&data);
                Ok(())
            },
        )
        .await
        .expect("fresh download");

        // Second run: the first `cut` bytes are already there (as a real
        // previous attempt would have left them) and the copy continues.
        let mut resumed = vec![0u8; size];
        resumed[..cut].copy_from_slice(&src[..cut]);
        let src_b = src.clone();
        let done_b = std::sync::atomic::AtomicU64::new(0);
        windowed_download_copy(
            cut as u64,
            size as u64,
            chunk as u64,
            8,
            Duration::from_secs(5),
            "test",
            &done_b,
            move |off, len| {
                let o = off as usize;
                let data = src_b[o..(o + len as usize).min(src_b.len())].to_vec();
                async move { Ok(data) }
            },
            |off, data| {
                let o = off as usize;
                resumed[o..o + data.len()].copy_from_slice(&data);
                Ok(())
            },
        )
        .await
        .expect("resumed download");

        assert_eq!(fresh, resumed, "resumed result differs from a fresh one");
        assert_eq!(fresh, src, "fresh download is not the source");
    }

    /// Drive `windowed_relay_copy` over in-memory fakes: an async source
    /// read and an async positioned destination write. `read_cap` bounds
    /// how many bytes each read returns (to exercise the short-read loop
    /// inside each chunk task); 0 means "return the full requested slice".
    async fn relay_round_trip(size: usize, chunk: usize, window: usize, read_cap: usize) {
        let src = pattern(size);
        let dst = Arc::new(Mutex::new(vec![0u8; size]));
        let src_read = Arc::new(src.clone());
        let dst_write = dst.clone();
        windowed_relay_copy(
            size as u64,
            chunk as u64,
            window,
            Duration::from_secs(5),
            "test",
            move |off, len| {
                let src = src_read.clone();
                async move {
                    let o = off as usize;
                    let mut take = len as usize;
                    if read_cap > 0 {
                        take = take.min(read_cap);
                    }
                    let end = (o + take).min(src.len());
                    Ok(src[o..end].to_vec())
                }
            },
            move |off, data| {
                let dst = dst_write.clone();
                async move {
                    let o = off as usize;
                    dst.lock().unwrap()[o..o + data.len()].copy_from_slice(&data);
                    Ok(())
                }
            },
        )
        .await
        .expect("relay");
        assert_eq!(*dst.lock().unwrap(), src, "relay reassembly mismatch size={size}");
    }

    #[tokio::test]
    async fn windowed_relay_even() {
        relay_round_trip(600 * 1024, 100 * 1024, 4, 0).await;
    }

    #[tokio::test]
    async fn windowed_relay_ragged_and_prime() {
        relay_round_trip(1_000_003, 99_991, 8, 0).await;
    }

    #[tokio::test]
    async fn windowed_relay_smaller_than_window() {
        relay_round_trip(50 * 1024, 100 * 1024, 16, 0).await;
    }

    #[tokio::test]
    async fn windowed_relay_short_reads() {
        // Source returns <= 1000 bytes per read, forcing each chunk task's
        // read loop to iterate before the write.
        relay_round_trip(600 * 1024, 100 * 1024, 4, 1000).await;
    }
}

#[cfg(test)]
mod scratch_name_tests {
    use super::{fit_part_name, part_path, remote_part_path, NAME_MAX, PART_SUFFIX};

    #[test]
    fn ordinary_names_just_get_the_suffix() {
        assert_eq!(
            part_path(std::path::Path::new("/tmp/backup.tar.gz")),
            std::path::PathBuf::from("/tmp/backup.tar.gz.oryxis-part")
        );
        assert_eq!(remote_part_path("/srv/backup.tar.gz"), "/srv/backup.tar.gz.oryxis-part");
        assert_eq!(remote_part_path("relative.bin"), "relative.bin.oryxis-part");
    }

    /// A name already at the filesystem's component limit must still
    /// produce a creatable scratch name, or a long filename would become a
    /// download that cannot start.
    #[test]
    fn names_at_the_limit_are_shortened_to_fit() {
        let long = "x".repeat(NAME_MAX);
        let fitted = fit_part_name(&long);
        assert!(fitted.len() <= NAME_MAX, "scratch name still too long");
        assert!(fitted.ends_with(PART_SUFFIX));

        let path = part_path(&std::path::PathBuf::from(format!("/tmp/{long}")));
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.len() <= NAME_MAX, "scratch component still too long");
        assert_eq!(path.parent().unwrap(), std::path::Path::new("/tmp"));

        let remote = remote_part_path(&format!("/srv/data/{long}"));
        let component = remote.rsplit('/').next().unwrap();
        assert!(component.len() <= NAME_MAX, "remote scratch component too long");
        assert!(remote.starts_with("/srv/data/"), "shortening moved the file");
    }

    /// Shortening must not split a multi-byte character, which would make
    /// the name invalid UTF-8 on the way out.
    #[test]
    fn shortening_respects_char_boundaries() {
        let long = "é".repeat(NAME_MAX);
        let fitted = fit_part_name(&long);
        assert!(fitted.len() <= NAME_MAX);
        assert!(fitted.ends_with(PART_SUFFIX));
        assert!(fitted.trim_end_matches(PART_SUFFIX).chars().all(|c| c == 'é'));
    }
}

#[cfg(test)]
mod resume_offset_tests {
    use super::{resume_offset, RESUME_REWIND, STREAM_THRESHOLD};

    /// A partial of a SMALL file has no holes to protect against: only
    /// the sliding window can leave them, and it never runs below the
    /// threshold. Rewinding there made every resume of a file smaller
    /// than the rewind a silent start-over, which is what a 1.2 MB
    /// upload hit.
    #[test]
    fn small_files_resume_from_the_whole_partial() {
        let size = 1_200_000;
        assert!(size < STREAM_THRESHOLD);
        assert_eq!(resume_offset(size, 400_000), 400_000);
        assert_eq!(resume_offset(size, 1), 1);
    }

    /// At or above the threshold the window could have been writing, so
    /// the rewind applies.
    #[test]
    fn large_files_rewind_past_the_window() {
        let size = 40 * 1024 * 1024;
        let have = 30 * 1024 * 1024;
        assert_eq!(resume_offset(size, have), have - RESUME_REWIND);
        // A partial smaller than the rewind leaves nothing provable.
        assert_eq!(resume_offset(size, RESUME_REWIND - 1), 0);
    }

    /// Nothing to continue is zero, in both directions: an empty
    /// destination, and one that is already at least as long as the
    /// source (which cannot be a partial copy of it).
    #[test]
    fn nothing_to_continue_is_zero() {
        assert_eq!(resume_offset(1_000, 0), 0);
        assert_eq!(resume_offset(1_000, 1_000), 0);
        assert_eq!(resume_offset(1_000, 2_000), 0);
    }
}

#[cfg(test)]
mod entry_name_tests {
    use super::is_safe_entry_name;

    /// Every shape a hostile listing can use to escape the directory the
    /// user picked. The two Windows forms are the ones a `/`-only split
    /// leaves intact, which is exactly how the SFTP console's `get` was
    /// reachable before it started asking this question.
    #[test]
    fn an_entry_name_that_could_escape_a_join_is_refused() {
        for bad in [
            "",
            ".",
            "..",
            "../evil",
            "/etc/passwd",
            "..\\..\\evil.exe",
            "dir\\evil",
            "C:evil",
            "C:\\evil",
            "nul\0byte",
        ] {
            assert!(!is_safe_entry_name(bad), "accepted {bad:?}");
        }
        // `a:b` is refused with the drive letters: one byte before a
        // colon is indistinguishable from `C:foo` at this layer, and a
        // remote file named that way is not worth the ambiguity.
        for good in ["file.txt", "..leading-dots", "école", "with space", "a.b:c"] {
            assert!(is_safe_entry_name(good), "rejected {good:?}");
        }
    }
}
