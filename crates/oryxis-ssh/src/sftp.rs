//! SFTP client built on top of an existing SSH session.
//!
//! `SshSession::open_sftp()` opens a fresh channel on the underlying SSH
//! connection, requests the `sftp` subsystem, and hands back an
//! [`SftpClient`] that wraps the high-level operations exposed by the
//! `russh-sftp` crate.

use russh::ChannelMsg;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
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
/// on the same connection — needed for ops like recursive delete where
/// shelling out to `rm -rf` is dramatically faster than walking the tree
/// over SFTP.
#[derive(Clone)]
pub struct SftpClient {
    inner: Arc<Mutex<SftpSession>>,
    handle: SharedHandle,
    /// Timeout used by `open_sibling` — propagated from the parent
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
    /// op — already-in-flight calls keep their existing deadline.
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
    /// entries — the UI provides its own breadcrumb / "go up" affordance.
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

    /// Read a remote file fully into memory. Caller is responsible for
    /// not asking for files that don't fit — large transfers should use
    /// streamed `download_to` once we wire that up.
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
    /// the wrong base — it pre-fills size/uid/permissions and would
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
    /// followed — we want the target's metadata, not the link itself.
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
    /// own internal mutex — concurrent calls on the original and the
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
        // `exit_code` defaulted to 255 — which is exactly the symptom we
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
    /// high-latency link, so we shell out to `rm -rf` on the remote — same
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
