//! SFTP client built on top of an existing SSH session.
//!
//! `SshSession::open_sftp()` opens a fresh channel on the underlying SSH
//! connection, requests the `sftp` subsystem, and hands back an
//! [`SftpClient`] that wraps the high-level operations exposed by the
//! `russh-sftp` crate.

use russh::ChannelMsg;
use russh_sftp::client::{RawSftpSession, SftpSession};
use russh_sftp::protocol::{FileAttributes, OpenFlags};
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

    /// Thin wrapper that snapshots the per-op timeout once and hands the
    /// copy off to [`pump_bytes`]. Reading the deadline a single time at
    /// entry matches the `set_op_timeout` contract (in-flight ops keep
    /// their existing deadline) and keeps the loop itself free-standing
    /// and unit-testable.
    async fn pump<R, W>(&self, op_name: &str, reader: R, writer: W) -> Result<(), SshError>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        pump_bytes(reader, writer, self.current_op_timeout(), op_name).await
    }

    /// Stream a remote file down to a local path without buffering the
    /// whole thing in RAM. Small files take the single-handle sequential
    /// pump; large ones (>= `STREAM_THRESHOLD`) carry a sliding window of
    /// concurrent reads on one handle (see `windowed_download_copy`). The
    /// remote handle is opened under the session lock, then the lock is
    /// released for the copy, so other ops on this client stay responsive.
    /// On error the partial local file is removed (it would otherwise be a
    /// deceptive correct-size, zero-filled file).
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
        let label = format!("download({remote})");
        let size = match size_hint {
            Some(s) => s,
            None => self.stat(remote).await?.size,
        };

        if size < STREAM_THRESHOLD {
            let remote_file = self
                .with_op_timeout(&label, async {
                    let s = self.inner.lock().await;
                    s.open_with_flags(remote, OpenFlags::READ)
                        .await
                        .map_err(|e| SshError::Channel(format!("sftp open({remote}): {e}")))
                })
                .await?;
            let local_file = tokio::fs::File::create(local)
                .await
                .map_err(|e| SshError::Channel(format!("create {}: {e}", local.display())))?;
            let r = self.pump(&label, remote_file, local_file).await;
            if r.is_err() {
                // Don't leave a partial download behind.
                let _ = tokio::fs::remove_file(local).await;
            }
            return r;
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
        let f = tokio::fs::File::create(local)
            .await
            .map_err(|e| SshError::Channel(format!("create {}: {e}", local.display())))?;
        f.set_len(actual)
            .await
            .map_err(|e| SshError::Channel(format!("alloc {}: {e}", local.display())))?;
        drop(f);
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .open(local)
            .map_err(|e| SshError::Channel(format!("open {}: {e}", local.display())))?;

        let timeout = self.current_op_timeout();
        let raw_read = raw.clone();
        let handle_read = handle.clone();
        let result = windowed_download_copy(
            actual,
            STREAM_CHUNK as u64,
            STREAM_WINDOW,
            timeout,
            &label,
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
                    .map_err(|e| SshError::Channel(format!("seek {}: {e}", local.display())))?;
                out.write_all(&data)
                    .map_err(|e| SshError::Channel(format!("write {}: {e}", local.display())))
            },
        )
        .await;
        // Best-effort close; the transfer result is what matters.
        let _ = raw.close(handle).await;
        if result.is_err() {
            // A failed windowed download leaves a correct-size but
            // zero-filled (corrupt) file behind, set_len preallocated it.
            // Remove it so the size doesn't masquerade as a good download.
            let _ = tokio::fs::remove_file(local).await;
        }
        result
    }

    /// Stream a local file up to a remote path. Mirror of
    /// [`download_to`](Self::download_to): small files go through the
    /// single-handle pump (which truncates-or-creates), large ones
    /// (>= `STREAM_THRESHOLD`) truncate the destination once up front,
    /// then carry a sliding window of concurrent writes on one handle (see
    /// `windowed_upload_copy`).
    pub async fn upload_from(&self, local: &std::path::Path, remote: &str) -> Result<(), SshError> {
        let label = format!("upload({remote})");
        let size = tokio::fs::metadata(local)
            .await
            .map_err(|e| SshError::Channel(format!("stat {}: {e}", local.display())))?
            .len();

        if size < STREAM_THRESHOLD {
            let local_file = tokio::fs::File::open(local)
                .await
                .map_err(|e| SshError::Channel(format!("open {}: {e}", local.display())))?;
            let remote_file = self
                .with_op_timeout(&label, async {
                    let s = self.inner.lock().await;
                    s.open_with_flags(
                        remote,
                        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                    )
                    .await
                    .map_err(|e| SshError::Channel(format!("sftp open(W,{remote}): {e}")))
                })
                .await?;
            return self.pump(&label, local_file, remote_file).await;
        }

        // Large file: one streaming handle carrying a sliding window of
        // concurrent writes. The single open with TRUNCATE clears any
        // prior contents once; positioned writes lay down the new bytes,
        // so a smaller new file can't leave a stale tail.
        let raw = self.open_raw_streaming().await?;
        let handle = raw
            .open(
                remote,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                FileAttributes::empty(),
            )
            .await
            .map_err(|e| SshError::Channel(format!("sftp open(W,{remote}): {e}")))?
            .handle;
        let mut input = std::fs::File::open(local)
            .map_err(|e| SshError::Channel(format!("open {}: {e}", local.display())))?;
        let local_disp = local.display().to_string();

        let timeout = self.current_op_timeout();
        let raw_write = raw.clone();
        let handle_write = handle.clone();
        let result = windowed_upload_copy(
            size,
            STREAM_CHUNK as u64,
            STREAM_WINDOW,
            timeout,
            &label,
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
                async move {
                    raw.write(handle, off, data)
                        .await
                        .map(|_| ())
                        .map_err(|e| SshError::Channel(format!("sftp write({off}): {e}")))
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
            .map_err(|e| SshError::Channel(format!("sftp close({remote}): {e}")));
        result.and(close)
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
        let label = format!("relay({src_remote} -> {dst_remote})");
        let size = match size_hint {
            Some(s) => s,
            None => self.stat(src_remote).await?.size,
        };
        let result = if size < STREAM_THRESHOLD {
            self.relay_small(&label, src_remote, dst, dst_remote).await
        } else {
            self.relay_windowed(&label, src_remote, size, dst, dst_remote)
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

    async fn relay_small(
        &self,
        label: &str,
        src_remote: &str,
        dst: &SftpClient,
        dst_remote: &str,
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
        self.pump(label, src_file, dst_file).await
    }

    async fn relay_windowed(
        &self,
        label: &str,
        src_remote: &str,
        size: u64,
        dst: &SftpClient,
        dst_remote: &str,
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
                async move {
                    raw.write(h, off, data)
                        .await
                        .map(|_| ())
                        .map_err(|e| SshError::Channel(format!("sftp relay write({off}): {e}")))
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
            raw.set_timeout(op_secs).await;
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
async fn windowed_download_copy<RDR, FRDR, WRT>(
    size: u64,
    chunk: u64,
    window: usize,
    timeout: std::time::Duration,
    op_name: &str,
    read_at: RDR,
    mut write_at: WRT,
) -> Result<(), SshError>
where
    RDR: Fn(u64, u32) -> FRDR,
    FRDR: std::future::Future<Output = Result<Vec<u8>, SshError>> + Send + 'static,
    WRT: FnMut(u64, Vec<u8>) -> Result<(), SshError>,
{
    let mut pending: std::collections::VecDeque<(u64, u32)> = std::collections::VecDeque::new();
    let mut start = 0u64;
    while start < size {
        let len = chunk.min(size - start) as u32;
        pending.push_back((start, len));
        start += len as u64;
    }
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
        let got = data.len() as u32;
        write_at(off, data)?;
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
async fn windowed_upload_copy<RDR, WRT, FWRT>(
    size: u64,
    chunk: u64,
    window: usize,
    timeout: std::time::Duration,
    op_name: &str,
    mut read_at: RDR,
    write_at: WRT,
) -> Result<(), SshError>
where
    RDR: FnMut(u64, u32) -> Result<Vec<u8>, SshError>,
    WRT: Fn(u64, Vec<u8>) -> FWRT,
    FWRT: std::future::Future<Output = Result<(), SshError>> + Send + 'static,
{
    let mut inflight: tokio::task::JoinSet<Result<(), SshError>> = tokio::task::JoinSet::new();
    let mut off = 0u64;
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
        joined.map_err(|e| SshError::Channel(format!("sftp {op_name} write task: {e}")))??;
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
    use super::{pump_bytes, windowed_download_copy, windowed_relay_copy, windowed_upload_copy};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Run the pump over an in-memory source and assert the sink received
    /// the exact bytes. Exercises the real chunk loop with no SSH server.
    async fn round_trip(len: usize) {
        let src: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let mut dst: Vec<u8> = Vec::new();
        // `&[u8]` is AsyncRead, `&mut Vec<u8>` is AsyncWrite (shutdown is
        // a no-op for Vec), so this drives the loop end to end.
        pump_bytes(src.as_slice(), &mut dst, Duration::from_secs(5), "test")
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
        let src = pattern(size);
        let mut dst = vec![0u8; size];
        let src_read = src.clone();
        windowed_download_copy(
            size as u64,
            chunk as u64,
            window,
            Duration::from_secs(5),
            "test",
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
        assert_eq!(dst, src, "download reassembly mismatch size={size}");
    }

    /// Same, but the fake remote returns at most 1000 bytes per read
    /// regardless of the requested length, forcing the short-read
    /// re-queue path so its offset math is exercised.
    async fn download_short_reads(size: usize, chunk: usize, window: usize) {
        let src = pattern(size);
        let mut dst = vec![0u8; size];
        let src_read = src.clone();
        windowed_download_copy(
            size as u64,
            chunk as u64,
            window,
            Duration::from_secs(5),
            "test",
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

    /// Drive `windowed_upload_copy`: serial source reads + concurrent
    /// positioned writes into a shared buffer. Asserts byte-identical.
    async fn upload_round_trip(size: usize, chunk: usize, window: usize) {
        let src = pattern(size);
        let dst = Arc::new(Mutex::new(vec![0u8; size]));
        let src_read = src.clone();
        let dst_write = dst.clone();
        windowed_upload_copy(
            size as u64,
            chunk as u64,
            window,
            Duration::from_secs(5),
            "test",
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
        assert_eq!(*dst.lock().unwrap(), src, "upload reassembly mismatch size={size}");
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
