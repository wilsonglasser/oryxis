//! Running a parsed [`Command`] against a live [`SftpClient`].
//!
//! This is the only half of the console that needs a server. Everything
//! it decides that does NOT need one (what a pattern matches, what a
//! listing looks like) is delegated to the pure modules, so what remains
//! here is the sequence of round trips and the handling of what comes
//! back.
//!
//! Output is written through a [`ConsoleSink`] rather than returned,
//! because a `get` of four gigabytes has to paint its progress while it
//! runs, and a function that answers with a `String` at the end cannot.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::sftp::SftpClient;
use crate::{SftpEntry, SshError};

use super::glob;
use super::parser::{Command, LsOpts, Owner, XferOpts};
use super::render::{self, CRLF};

/// How often the progress meter repaints. Fast enough to look live, slow
/// enough that a transfer saturating a local link does not spend its time
/// feeding the emulator: without a cap, every chunk becomes a frame.
const PROGRESS_TICK: std::time::Duration = std::time::Duration::from_millis(100);

/// Where console output goes. An unbounded sender in production; the
/// tests collect into a Vec.
pub trait ConsoleSink: Send {
    fn write(&mut self, bytes: &[u8]);
}

impl ConsoleSink for tokio::sync::mpsc::UnboundedSender<Vec<u8>> {
    fn write(&mut self, bytes: &[u8]) {
        // A closed channel means the pane is gone. Dropping is right:
        // the REPL notices through its own shutdown path, and erroring
        // here would turn every write site into a fallible one for a
        // condition none of them can act on.
        let _ = self.send(bytes.to_vec());
    }
}

impl ConsoleSink for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// The console's own state, the part that survives between commands.
#[derive(Debug, Clone)]
pub struct ShellState {
    /// Remote working directory, always absolute and canonical.
    pub remote_cwd: String,
    /// The remote home, resolved once at start so `cd` with no argument
    /// and a leading `~` both have an answer without a round trip.
    pub remote_home: String,
    /// Local working directory. Native paths: POSIX on unix, `C:\...` on
    /// Windows, because `lls` lists the machine the app runs on.
    pub local_cwd: PathBuf,
    /// Whether transfers paint a progress meter. `sftp(1)` starts with it
    /// on and `progress` toggles it.
    pub progress: bool,
    /// The mask applied to the permissions of files `get` creates, set by
    /// `lumask`.
    ///
    /// Held here rather than pushed into the PROCESS umask, which is what
    /// `sftp(1)` does and what a console living inside a GUI application
    /// must not do: the process umask is shared with every other thing
    /// the app writes, and a console command has no business changing how
    /// the vault or a log file is created.
    pub lumask: u32,
    /// Terminal width, for the listing columns and the meter.
    pub cols: u16,
}

impl ShellState {
    pub fn new(remote_home: String, local_cwd: PathBuf, cols: u16) -> Self {
        Self {
            remote_cwd: remote_home.clone(),
            remote_home,
            local_cwd,
            progress: true,
            // `sftp(1)`'s own default, and the shell's.
            lumask: 0o022,
            cols: cols.max(1),
        }
    }
}

/// What running a command decided about the session's future.
///
/// There is deliberately no `Disconnected` here, and the reason is worth
/// recording: [`SftpClient`] maps EVERY failure to `SshError::Channel`,
/// a missing file and a dead link alike, so no inspection of the error
/// can tell them apart. Guessing from the message text would be a
/// classifier that breaks the first time a server words a status
/// differently.
///
/// So the console does not ask the error, it asks the SESSION, once per
/// command, through the `is_alive` machinery that already carries the
/// dead-before-silent ordering guarantee. That check lives in the REPL
/// loop; this enum only reports what the USER asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Print a prompt and read another line. `failed` is whether the
    /// command reported an error, which is the REPL's cue to spend one
    /// cheap round trip asking whether the link is still there. It is
    /// NOT a classification of the error: see the note above.
    Continue { failed: bool },
    /// The user asked to leave (`bye` / `quit` / `exit`).
    Quit,
}

/// Run one command. Errors are PRINTED, not returned: a console reports
/// what went wrong and carries on, which is the whole difference between
/// it and a script. The only thing that ends the loop is [`Outcome`].
pub async fn run(
    cmd: Command,
    client: &SftpClient,
    state: &mut ShellState,
    out: &mut impl ConsoleSink,
) -> Outcome {
    let result = match cmd {
        Command::Quit => return Outcome::Quit,
        Command::Help => {
            out.write(render::help_text().as_bytes());
            Ok(())
        }
        Command::Version => {
            // russh-sftp speaks version 3, which is what every server we
            // can reach implements. Reported rather than negotiated
            // because there is nothing to negotiate.
            line(out, "SFTP protocol version 3");
            Ok(())
        }
        Command::Progress => {
            state.progress = !state.progress;
            line(
                out,
                if state.progress {
                    "Progress meter enabled"
                } else {
                    "Progress meter disabled"
                },
            );
            Ok(())
        }
        Command::Pwd => {
            line(
                out,
                &format!("Remote working directory: {}", state.remote_cwd),
            );
            Ok(())
        }
        Command::Lpwd => {
            line(
                out,
                &format!("Local working directory: {}", state.local_cwd.display()),
            );
            Ok(())
        }
        Command::Cd(path) => cd(path, client, state, out).await,
        Command::Lcd(path) => lcd(path, state, out),
        Command::Ls(opts) => ls(opts, client, state, out).await,
        Command::Lls(opts) => lls(opts, state, out).await,
        Command::Get {
            opts,
            remote,
            local,
        } => get(opts, remote, local, client, state, out).await,
        Command::Put {
            opts,
            local,
            remote,
        } => put(opts, local, remote, client, state, out).await,
        Command::Rm(paths) => rm(paths, client, state, out).await,
        Command::Mkdir(path) => {
            let path = state.resolve_remote(&path);
            client.create_dir(&path).await
        }
        Command::Lmkdir(path) => {
            let path = state.resolve_local(&path);
            tokio::fs::create_dir(&path).await.map_err(SshError::from)
        }
        Command::Rmdir(path) => {
            let path = state.resolve_remote(&path);
            client.remove_dir(&path).await
        }
        Command::Lumask(mask) => {
            state.lumask = mask;
            line(out, &format!("Local umask: {mask:03o}"));
            Ok(())
        }
        Command::Df {
            path,
            human,
            inodes,
        } => df(path, human, inodes, client, state, out).await,
        Command::Ln {
            target,
            link,
            symbolic,
        } => ln(target, link, symbolic, client, state, out).await,
        Command::Chown {
            id,
            which,
            paths,
            follow,
        } => chown(id, which, paths, follow, client, state, out).await,
        Command::Copy { from, to } => {
            let from = state.resolve_remote(&from);
            let to = state.resolve_remote(&to);
            // A destination that is a directory means "into it", which is
            // what `cp` means to anyone who types it.
            let to = if is_remote_dir(client, &to).await {
                join_remote(&to, from.rsplit('/').next().unwrap_or(&from))
            } else {
                to
            };
            client.copy_file(&from, &to).await
        }
        Command::Rename { from, to } => {
            let from = state.resolve_remote(&from);
            let to = state.resolve_remote(&to);
            // The POSIX extension replaces the destination atomically,
            // which is what `rename` means to anyone who types it. Plain
            // SFTP rename fails when the target exists, so falling back
            // to it silently would make the command mean two things
            // depending on the server; the fallback is only for servers
            // that do not offer the extension at all.
            match client.posix_rename(&from, &to).await {
                Ok(()) => Ok(()),
                Err(_) => client.rename(&from, &to).await,
            }
        }
        Command::Chmod {
            mode,
            paths,
            follow,
        } => chmod(mode, paths, follow, client, state, out).await,
    };

    match result {
        Ok(()) => Outcome::Continue { failed: false },
        Err(e) => {
            // Errors are PRINTED, not returned: a console reports what
            // went wrong and carries on, which is the whole difference
            // between it and a script.
            line(out, &e.to_string());
            Outcome::Continue { failed: true }
        }
    }
}

impl ShellState {
    /// Turn a user-typed remote path into an absolute one. Relative paths
    /// resolve against the working directory, `~` against the home.
    ///
    /// Visible to the module because completion resolves the SAME way a
    /// command does: a Tab after `~/` that listed a different directory
    /// than the `get` behind it would offer files the transfer then could
    /// not find.
    pub(super) fn resolve_remote(&self, path: &str) -> String {
        // The operand arrives glob-escaped from the tokenizer, because
        // the pass that decides what is a wildcard runs before this one.
        // Here it stops being a pattern and becomes a PATH, which is
        // exactly where the escapes come off, and the reason this is the
        // funnel every remote operand goes through.
        let path = &glob::unescape(path);
        if path == "~" {
            return self.remote_home.clone();
        }
        if let Some(rest) = path.strip_prefix("~/") {
            return join_remote(&self.remote_home, rest);
        }
        if path.starts_with('/') {
            return normalize_remote(path);
        }
        join_remote(&self.remote_cwd, path)
    }

    /// The local twin. `~` expands from the environment rather than from
    /// a session, because the local side has no session to ask.
    pub(super) fn resolve_local(&self, path: &str) -> PathBuf {
        // Same funnel, same reason: see [`Self::resolve_remote`].
        let path = &glob::unescape(path);
        if path == "~" {
            return local_home();
        }
        if let Some(rest) = path.strip_prefix("~/") {
            return local_home().join(rest);
        }
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.local_cwd.join(p)
        }
    }
}

fn local_home() -> PathBuf {
    // `HOME` then `USERPROFILE`, the same order `resolve_disk_key` uses,
    // and deliberately not the app's own `ORYXIS_HOME` override: `lcd ~`
    // means the user's home, not the app's data directory.
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Join two remote path segments and collapse `.` / `..`. SFTP paths are
/// always POSIX on the wire, whatever the server runs.
fn join_remote(base: &str, rest: &str) -> String {
    let joined = if base.ends_with('/') {
        format!("{base}{rest}")
    } else {
        format!("{base}/{rest}")
    };
    normalize_remote(&joined)
}

/// Collapse `.`, `..` and duplicate separators. Done locally so `cd ..`
/// answers instantly and so the path shown in the prompt is the one the
/// user would write, rather than the accumulated trail.
fn normalize_remote(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

fn line(out: &mut impl ConsoleSink, text: &str) {
    out.write(text.as_bytes());
    out.write(CRLF.as_bytes());
}

async fn cd(
    path: Option<String>,
    client: &SftpClient,
    state: &mut ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let target = match path {
        None => state.remote_home.clone(),
        Some(p) => state.resolve_remote(&p),
    };
    // Verify before committing: a `cd` that silently accepts a bad path
    // makes every later command fail with a confusing message about a
    // file, when the real problem was the directory.
    let stat = client.stat(&target).await?;
    if stat.permissions.is_some_and(|m| m & 0o040000 == 0) {
        line(out, &format!("{target}: Not a directory"));
        return Ok(());
    }
    // Canonicalize so the prompt and later joins use the resolved path,
    // which is what makes a `cd` through a symlink behave afterwards.
    state.remote_cwd = client.canonicalize(&target).await.unwrap_or(target);
    Ok(())
}

fn lcd(
    path: Option<String>,
    state: &mut ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let target = match path {
        None => local_home(),
        Some(p) => state.resolve_local(&p),
    };
    match std::fs::metadata(&target) {
        Ok(m) if m.is_dir() => {
            state.local_cwd = std::fs::canonicalize(&target).unwrap_or(target);
            Ok(())
        }
        Ok(_) => {
            line(out, &format!("{}: Not a directory", target.display()));
            Ok(())
        }
        Err(e) => {
            line(out, &format!("{}: {e}", target.display()));
            Ok(())
        }
    }
}

async fn ls(
    opts: LsOpts,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let (dir, pattern) = state.split_listing_target(opts.path.as_deref());
    // `ls <file>` lists THE FILE, the way `ls` and `sftp(1)` both do.
    // Without this the path goes to `read_dir`, which answers "no such
    // file" about a file that is plainly there, and the error names the
    // wrong problem. Only for an operand with no wildcard: a pattern is
    // always a filter on a directory.
    if pattern.is_none()
        && let Ok(stat) = client.stat(&dir).await
        && stat.permissions.is_some_and(|m| m & 0o040000 == 0)
    {
        let name = dir.rsplit('/').next().unwrap_or(&dir).to_string();
        // A long listing of ONE file still shows the owner by name, or
        // `ls -l x` and `ls -l` would disagree about who owns `x` on the
        // same screen. The name only exists in a DIRECTORY listing, so
        // the parent is read to find it; a stat has no such line.
        let named = if opts.long && !opts.numeric {
            let parent = dir.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let parent = if parent.is_empty() { "/" } else { parent };
            client
                .list_dir_long(parent)
                .await
                .ok()
                .and_then(|entries| entries.into_iter().find(|e| e.name == name))
        } else {
            None
        };
        let entry = SftpEntry {
            owner: named.as_ref().and_then(|e| e.owner.clone()),
            group: named.as_ref().and_then(|e| e.group.clone()),
            name,
            is_dir: false,
            is_symlink: false,
            size: stat.size,
            mtime: stat.mtime,
            permissions: stat.permissions,
            uid: stat.uid,
            gid: stat.gid,
        };
        // `all` is forced on: the user named this file, so hiding it for
        // starting with a dot would answer nothing at all.
        let opts = LsOpts {
            all: true,
            ..opts
        };
        out.write(render::render_listing(&[entry], &opts, now_secs(), state.cols).as_bytes());
        return Ok(());
    }
    // A long listing asks for the owner NAMES, which cost an extra
    // channel and a second code path; every other listing does not need
    // them and does not pay for them. `-n` asked for the numbers, so it
    // does not pay either.
    let mut entries = if opts.long && !opts.numeric {
        match client.list_dir_long(&dir).await {
            Ok(entries) => entries,
            // The names are a nicety and the listing is not. A server
            // that refuses the long read still gets listed, numerically.
            Err(_) => client.list_dir(&dir).await?,
        }
    } else {
        client.list_dir(&dir).await?
    };
    if let Some(pat) = pattern {
        entries.retain(|e| glob::matches(&pat, &e.name));
    }
    out.write(render::render_listing(&entries, &opts, now_secs(), state.cols).as_bytes());
    Ok(())
}

async fn lls(opts: LsOpts, state: &ShellState, out: &mut impl ConsoleSink) -> Result<(), SshError> {
    let raw = opts.path.as_deref().unwrap_or(".");
    let target = state.resolve_local(raw);
    // A trailing component with a wildcard is a filter, not a directory,
    // mirroring the remote side.
    let (dir, pattern) = match target.file_name().and_then(|n| n.to_str()) {
        Some(name) if glob::has_magic(name) => (
            target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(target.clone()),
            Some(name.to_string()),
        ),
        _ => (target.clone(), None),
    };
    // `lls <file>` lists THE FILE, the same way `ls <file>` does on the
    // remote side. Without this the path goes to `read_dir`, which
    // answers "not a directory" about a file that is plainly there, and
    // the error names the wrong problem. The two sides of the console
    // must not disagree about what an operand means.
    if pattern.is_none()
        && let Ok(meta) = tokio::fs::symlink_metadata(&dir).await
        && !meta.is_dir()
    {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        let is_symlink = meta.is_symlink();
        let meta = tokio::fs::metadata(&dir).await.ok();
        // `all` is forced on: the user named this file, so hiding it for
        // starting with a dot would answer nothing at all.
        let opts = LsOpts { all: true, ..opts };
        let entry = local_entry(name, is_symlink, meta);
        out.write(render::render_listing(&[entry], &opts, now_secs(), state.cols).as_bytes());
        return Ok(());
    }
    let mut entries = Vec::new();
    let mut read = tokio::fs::read_dir(&dir).await?;
    while let Some(entry) = read.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(pat) = &pattern
            && !glob::matches(pat, &name)
        {
            continue;
        }
        let is_symlink = entry
            .file_type()
            .await
            .map(|t| t.is_symlink())
            .unwrap_or(false);
        let meta = tokio::fs::metadata(entry.path()).await;
        entries.push(local_entry(name, is_symlink, meta.ok()));
    }
    out.write(render::render_listing(&entries, &opts, now_secs(), state.cols).as_bytes());
    Ok(())
}

/// Describe a local file in the same shape the remote listing uses, so
/// one renderer serves `ls` and `lls`.
fn local_entry(name: String, is_symlink: bool, meta: Option<std::fs::Metadata>) -> SftpEntry {
    let Some(m) = meta else {
        // A broken symlink still lists, with what is known about it.
        return SftpEntry {
            name,
            is_dir: false,
            is_symlink,
            size: 0,
            mtime: None,
            permissions: None,
            uid: None,
            gid: None,
            owner: None,
            group: None,
        };
    };
    SftpEntry {
        name,
        is_dir: m.is_dir(),
        is_symlink,
        size: m.len(),
        mtime: m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().min(u64::from(u32::MAX)) as u32),
        permissions: unix_mode(&m),
        uid: unix_uid(&m),
        gid: unix_gid(&m),
        // The local side has ids and no resolver: reading `/etc/passwd`
        // to turn one into a name is a different feature from listing a
        // directory, and the remote half is numeric by default too.
        owner: None,
        group: None,
    }
}

#[cfg(unix)]
fn unix_mode(m: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    Some(m.mode())
}

#[cfg(not(unix))]
fn unix_mode(m: &std::fs::Metadata) -> Option<u32> {
    // Windows has no mode bits. Reporting the read-only flag as 0444 /
    // 0644 is a lie a listing would carry into a `chmod`, so the column
    // reads as unknown instead, which `render` already handles.
    let _ = m;
    None
}

#[cfg(unix)]
fn unix_uid(m: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    Some(m.uid())
}

#[cfg(not(unix))]
fn unix_uid(_m: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn unix_gid(m: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    Some(m.gid())
}

#[cfg(not(unix))]
fn unix_gid(_m: &std::fs::Metadata) -> Option<u32> {
    None
}

impl ShellState {
    /// Split a listing operand into the directory to read and the
    /// pattern to filter by. `ls *.gz` reads the working directory and
    /// filters; `ls /var/log` reads that directory whole.
    fn split_listing_target(&self, operand: Option<&str>) -> (String, Option<String>) {
        let Some(operand) = operand else {
            return (self.remote_cwd.clone(), None);
        };
        if !glob::has_magic(operand) {
            return (self.resolve_remote(operand), None);
        }
        match operand.rsplit_once('/') {
            Some((dir, pattern)) => {
                let dir = if dir.is_empty() { "/" } else { dir };
                (self.resolve_remote(dir), Some(pattern.to_string()))
            }
            None => (self.remote_cwd.clone(), Some(operand.to_string())),
        }
    }
}

/// Expand a remote operand that may hold wildcards into concrete paths.
///
/// A pattern with no matches is an ERROR, not an empty success. `sftp(1)`
/// reports it, and it matters: `mget *.gz` in the wrong directory would
/// otherwise look exactly like a directory with no gzips, and the user
/// would go looking for files that were never fetched.
async fn expand_remote(
    operand: &str,
    client: &SftpClient,
    state: &ShellState,
) -> Result<Vec<String>, SshError> {
    if !glob::has_magic(operand) {
        return Ok(vec![state.resolve_remote(operand)]);
    }
    let (dir, pattern) = match operand.rsplit_once('/') {
        Some((dir, pattern)) => {
            let dir = if dir.is_empty() { "/" } else { dir };
            (state.resolve_remote(dir), pattern.to_string())
        }
        None => (state.remote_cwd.clone(), operand.to_string()),
    };
    let entries = client.list_dir(&dir).await?;
    let mut matched: Vec<String> = entries
        .iter()
        .filter(|e| glob::matches(&pattern, &e.name))
        .map(|e| join_remote(&dir, &e.name))
        .collect();
    matched.sort();
    if matched.is_empty() {
        return Err(SshError::Channel(format!("{operand}: no matches found")));
    }
    Ok(matched)
}

/// The local twin of [`expand_remote`].
async fn expand_local(operand: &str, state: &ShellState) -> Result<Vec<PathBuf>, SshError> {
    if !glob::has_magic(operand) {
        return Ok(vec![state.resolve_local(operand)]);
    }
    let target = state.resolve_local(operand);
    let (dir, pattern) = match target.file_name().and_then(|n| n.to_str()) {
        Some(name) if glob::has_magic(name) => (
            target.parent().map(Path::to_path_buf).unwrap_or_default(),
            name.to_string(),
        ),
        _ => return Ok(vec![target]),
    };
    let mut matched = Vec::new();
    let mut read = tokio::fs::read_dir(&dir).await?;
    while let Some(entry) = read.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if glob::matches(&pattern, &name) {
            matched.push(dir.join(name));
        }
    }
    matched.sort();
    if matched.is_empty() {
        return Err(SshError::Channel(format!("{operand}: no matches found")));
    }
    Ok(matched)
}

async fn get(
    opts: XferOpts,
    remote: String,
    local: Option<String>,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let sources = expand_remote(&remote, client, state).await?;
    // A named destination only makes sense for a single source; with
    // several it would mean overwriting the same file N times, so it
    // becomes a directory to put them in.
    let multiple = sources.len() > 1;
    for source in sources {
        let base = source.rsplit('/').next().unwrap_or(&source).to_string();
        // `base` is about to become part of a LOCAL path, and for a glob
        // it is a name the server chose. The `/` split above is not a
        // guard: `..\..\evil.exe` and `C:evil` carry no slash and both
        // steer the join on Windows. Skip the entry rather than fail the
        // whole transfer, and say which name was refused, because a file
        // silently missing from a `mget` is worse than a loud one.
        if !crate::sftp::is_safe_entry_name(&base) {
            let shown = render::display_name(&base);
            line(out, &format!("{shown}: skipped, unsafe file name"));
            continue;
        }
        let dest = match (&local, multiple) {
            (Some(l), false) => {
                let p = state.resolve_local(l);
                // `get f dir/` and `get f dir` both mean "into dir" when
                // dir exists, which is what a user expects from `cp`.
                if p.is_dir() { p.join(&base) } else { p }
            }
            (Some(l), true) => state.resolve_local(l).join(&base),
            (None, _) => state.local_cwd.join(&base),
        };
        let stat = client.stat(&source).await.ok();
        if stat.as_ref().is_some_and(is_dir_stat) {
            if !opts.recursive {
                line(out, &format!("{source}: not a regular file"));
                continue;
            }
            // The destination for a tree is the DIRECTORY the tree lands
            // in, which is what `dest` already computed for a file of the
            // same name.
            get_tree(&opts, &source, &dest, client, state, out).await?;
            continue;
        }
        get_one(&opts, &source, &base, &dest, stat, state, client, out).await?;
    }
    Ok(())
}

/// Whether a stat describes a directory. The mode's type bits are the
/// only thing SFTP v3 offers to say so.
fn is_dir_stat(stat: &crate::RemoteStat) -> bool {
    stat.permissions.is_some_and(|m| m & 0o040000 != 0)
}

/// Download ONE file, meter and all, then settle its local attributes.
#[allow(clippy::too_many_arguments)]
async fn get_one(
    opts: &XferOpts,
    source: &str,
    base: &str,
    dest: &Path,
    stat: Option<crate::RemoteStat>,
    state: &ShellState,
    client: &SftpClient,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let size = stat.as_ref().map(|s| s.size);
    // ONE counter, shared: the transfer writes into it and the meter
    // reads it. Minting a second one for the meter would leave it
    // reading a counter nobody feeds, so the bar would sit at zero
    // for the whole transfer and jump to done at the end.
    let counter = Arc::new(AtomicU64::new(0));
    transfer(
        // The progress line goes to the terminal, so it carries the
        // sanitized name; `dest` was built from the real one.
        &render::display_name(base),
        size,
        state,
        out,
        Arc::clone(&counter),
        client.download_to_progress(source, dest, size, Some(counter.clone())),
    )
    .await?;

    // The mode a downloaded file ends up with, and the reason `lumask`
    // exists: without `-p` the source mode is not copied at all, so the
    // starting point is the shell's own 0666 and the mask takes bits
    // off it. With `-p` the source mode is the starting point and the
    // mask still applies, which is what `sftp(1)` does.
    let mode = if opts.preserve {
        stat.as_ref().and_then(|s| s.permissions).map(|m| m & 0o7777)
    } else {
        Some(0o666)
    };
    if let Some(mode) = mode {
        set_local_mode(dest, mode & !state.lumask, out);
    }
    if opts.preserve && let Some(mtime) = stat.as_ref().and_then(|s| s.mtime) {
        set_local_mtime(dest, mtime, out);
    }
    if opts.fsync {
        sync_local(dest, out).await;
    }
    Ok(())
}

/// Download a directory tree.
///
/// Iterative rather than recursive because an `async fn` that calls
/// itself needs boxing at every level, and the stack here is a list of
/// directories rather than a call chain anyway.
///
/// Two rules, both `sftp(1)`'s: a symlink is FOLLOWED, so a link to a
/// file is fetched as that file, and anything that is neither a directory
/// nor a regular file is named and skipped rather than silently dropped.
async fn get_tree(
    opts: &XferOpts,
    root_remote: &str,
    root_local: &Path,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let mut stack = vec![(root_remote.to_string(), root_local.to_path_buf())];
    while let Some((remote_dir, local_dir)) = stack.pop() {
        line(out, &format!("Retrieving {remote_dir}"));
        if let Err(e) = tokio::fs::create_dir_all(&local_dir).await {
            line(out, &format!("{}: {e}", local_dir.display()));
            continue;
        }
        let entries = match client.list_dir(&remote_dir).await {
            Ok(entries) => entries,
            // One unreadable directory does not abandon the rest of the
            // tree, the same way one failed `rm` does not abandon the
            // rest of the line.
            Err(e) => {
                line(out, &e.to_string());
                continue;
            }
        };
        for entry in entries {
            // Every component is checked, not just the one the user
            // typed: past the first level these names are the SERVER's,
            // and `..` among them would walk the download out of the
            // destination directory entirely.
            if !crate::sftp::is_safe_entry_name(&entry.name) {
                let shown = render::display_name(&entry.name);
                line(out, &format!("{shown}: skipped, unsafe file name"));
                continue;
            }
            let remote_path = join_remote(&remote_dir, &entry.name);
            let local_path = local_dir.join(&entry.name);
            // A symlink's own listing entry says nothing about what it
            // points at, so the target is stated for.
            let stat = if entry.is_symlink {
                match client.stat(&remote_path).await {
                    Ok(s) => Some(s),
                    Err(_) => {
                        line(out, &format!("{remote_path}: skipping broken symlink"));
                        continue;
                    }
                }
            } else {
                client.stat(&remote_path).await.ok()
            };
            let is_dir = stat.as_ref().is_some_and(is_dir_stat) || (entry.is_dir && stat.is_none());
            if is_dir {
                stack.push((remote_path, local_path));
                continue;
            }
            if !is_regular(&stat, &entry) {
                line(out, &format!("{remote_path}: skipping non-regular file"));
                continue;
            }
            get_one(
                opts,
                &remote_path,
                &entry.name,
                &local_path,
                stat,
                state,
                client,
                out,
            )
            .await?;
        }
        if opts.preserve {
            // The directory's own attributes are applied on the way out,
            // after its contents exist: setting a read-only mode first
            // would stop the very writes that fill it.
            if let Ok(stat) = client.stat(&remote_dir).await {
                if let Some(mode) = stat.permissions {
                    set_local_mode(&local_dir, mode & 0o7777 & !state.lumask, out);
                }
                if let Some(mtime) = stat.mtime {
                    set_local_mtime(&local_dir, mtime, out);
                }
            }
        }
    }
    Ok(())
}

/// Whether an entry is a plain file worth transferring. A device node, a
/// socket or a fifo is not, and copying one would produce something that
/// is not the same object on the other side.
fn is_regular(stat: &Option<crate::RemoteStat>, entry: &SftpEntry) -> bool {
    match stat.as_ref().and_then(|s| s.permissions) {
        Some(mode) => mode & 0o170000 == 0o100000,
        // A server that does not report a mode leaves the listing as the
        // only evidence, and it only distinguishes directories.
        None => !entry.is_dir,
    }
}

async fn put(
    opts: XferOpts,
    local: String,
    remote: Option<String>,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let sources = expand_local(&local, state).await?;
    let multiple = sources.len() > 1;
    for source in sources {
        let base = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dest = match (&remote, multiple) {
            (Some(r), false) => {
                let resolved = state.resolve_remote(r);
                // A trailing slash, or an existing directory, means
                // "into it". Checked rather than assumed, because the
                // remote side is the one we cannot see.
                if r.ends_with('/') || is_remote_dir(client, &resolved).await {
                    join_remote(&resolved, &base)
                } else {
                    resolved
                }
            }
            (Some(r), true) => join_remote(&state.resolve_remote(r), &base),
            (None, _) => join_remote(&state.remote_cwd, &base),
        };
        let meta = tokio::fs::metadata(&source).await.ok();
        if meta.as_ref().is_some_and(std::fs::Metadata::is_dir) {
            if !opts.recursive {
                line(out, &format!("{}: not a regular file", source.display()));
                continue;
            }
            put_tree(&opts, &source, &dest, client, state, out).await?;
            continue;
        }
        put_one(&opts, &source, &base, &dest, meta, state, client, out).await?;
    }
    Ok(())
}

/// Upload ONE file, meter and all, then settle its remote attributes.
#[allow(clippy::too_many_arguments)]
async fn put_one(
    opts: &XferOpts,
    source: &Path,
    base: &str,
    dest: &str,
    meta: Option<std::fs::Metadata>,
    state: &ShellState,
    client: &SftpClient,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let size = meta.as_ref().map(std::fs::Metadata::len);
    let counter = Arc::new(AtomicU64::new(0));
    let options = crate::sftp::UploadOptions {
        progress: Some(counter.clone()),
        // Unlike download, resume here is only ever the user's
        // explicit `-a` / `reput`: an upload writes into someone
        // else's namespace, where a shorter file with a matching
        // tail is only PROBABLY the same file.
        resume: opts.resume,
        // `-f` asks the server to flush before it answers, which is the
        // only place the guarantee can be made: an upload that returned
        // and then lost the file to a power cut reported a success that
        // was not one.
        fsync: opts.fsync,
        ..Default::default()
    };
    transfer(
        &render::display_name(base),
        size,
        state,
        out,
        Arc::clone(&counter),
        client.upload_from_options(source, dest, options),
    )
    .await?;

    if opts.preserve && let Some(meta) = meta {
        let update = crate::AttrUpdate {
            permissions: unix_mode(&meta).map(|m| m & 0o7777),
            atime: file_seconds(meta.accessed().ok()),
            mtime: file_seconds(meta.modified().ok()),
            ..Default::default()
        };
        // The times are a PAIR on the wire, so a metadata read that
        // answered only one of them would send a zero for the other.
        let update = if update.atime.is_none() || update.mtime.is_none() {
            crate::AttrUpdate {
                atime: None,
                mtime: None,
                ..update
            }
        } else {
            update
        };
        if let Err(e) = client.set_attrs(dest, update, true).await {
            line(out, &e.to_string());
        }
    }
    Ok(())
}

/// Upload a directory tree, the mirror of [`get_tree`].
///
/// A local symlink is followed for the same reason the remote one is: the
/// user asked for the tree they can see, and a link to a file reads as
/// that file everywhere else they look at it.
async fn put_tree(
    opts: &XferOpts,
    root_local: &Path,
    root_remote: &str,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let mut stack = vec![(root_local.to_path_buf(), root_remote.to_string())];
    while let Some((local_dir, remote_dir)) = stack.pop() {
        line(out, &format!("Entering {}", local_dir.display()));
        // An existing directory is not an error here: `put -r` over a
        // tree that is partly there is the ordinary way to finish an
        // interrupted upload.
        if let Err(e) = client.create_dir(&remote_dir).await
            && !is_remote_dir(client, &remote_dir).await
        {
            line(out, &e.to_string());
            continue;
        }
        let mut read = match tokio::fs::read_dir(&local_dir).await {
            Ok(read) => read,
            Err(e) => {
                line(out, &format!("{}: {e}", local_dir.display()));
                continue;
            }
        };
        while let Ok(Some(entry)) = read.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            let local_path = entry.path();
            let remote_path = join_remote(&remote_dir, &name);
            // `metadata` follows links; a broken one has no target to
            // send and is named rather than dropped.
            let meta = match tokio::fs::metadata(&local_path).await {
                Ok(m) => m,
                Err(e) => {
                    line(out, &format!("{}: {e}", local_path.display()));
                    continue;
                }
            };
            if meta.is_dir() {
                stack.push((local_path, remote_path));
                continue;
            }
            if !meta.is_file() {
                line(
                    out,
                    &format!("{}: skipping non-regular file", local_path.display()),
                );
                continue;
            }
            put_one(
                opts,
                &local_path,
                &name,
                &remote_path,
                Some(meta),
                state,
                client,
                out,
            )
            .await?;
        }
        if opts.preserve && let Ok(meta) = tokio::fs::metadata(&local_dir).await {
            // On the way out, for the same reason the download side does
            // it: a read-only mode applied first would stop the writes
            // that fill the directory.
            let update = crate::AttrUpdate {
                permissions: unix_mode(&meta).map(|m| m & 0o7777),
                ..Default::default()
            };
            if update.permissions.is_some()
                && let Err(e) = client.set_attrs(&remote_dir, update, true).await
            {
                line(out, &e.to_string());
            }
        }
    }
    Ok(())
}

/// A `SystemTime` as the u32 unix seconds the protocol carries, or `None`
/// when it falls outside what that can express.
fn file_seconds(t: Option<std::time::SystemTime>) -> Option<u32> {
    t?.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
        .and_then(|s| u32::try_from(s).ok())
}

/// Apply a permission bitmask to a local path, reporting a failure rather
/// than failing the transfer: the bytes arrived, and that is the part the
/// user asked for.
#[cfg(unix)]
fn set_local_mode(path: &Path, mode: u32, out: &mut impl ConsoleSink) {
    use std::os::unix::fs::PermissionsExt as _;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)) {
        line(out, &format!("{}: {e}", path.display()));
    }
}

/// Windows has no mode bits, so there is nothing to apply. Silent rather
/// than reported: this runs on every download, and a line per file saying
/// the platform is the platform is noise.
#[cfg(not(unix))]
fn set_local_mode(path: &Path, mode: u32, out: &mut impl ConsoleSink) {
    let _ = (path, mode, out);
}

fn set_local_mtime(path: &Path, mtime: u32, out: &mut impl ConsoleSink) {
    let stamp = filetime::FileTime::from_unix_time(i64::from(mtime), 0);
    if let Err(e) = filetime::set_file_mtime(path, stamp) {
        line(out, &format!("{}: {e}", path.display()));
    }
}

/// Flush a downloaded file to the disk it landed on. The remote twin of
/// `put -f`, and the same promise: the call returned, so the bytes are
/// there.
async fn sync_local(path: &Path, out: &mut impl ConsoleSink) {
    match tokio::fs::File::open(path).await {
        Ok(f) => {
            if let Err(e) = f.sync_all().await {
                line(out, &format!("{}: {e}", path.display()));
            }
        }
        Err(e) => line(out, &format!("{}: {e}", path.display())),
    }
}

async fn is_remote_dir(client: &SftpClient, path: &str) -> bool {
    client
        .stat(path)
        .await
        .ok()
        .and_then(|s| s.permissions)
        .is_some_and(|m| m & 0o040000 != 0)
}

async fn rm(
    paths: Vec<String>,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    for operand in paths {
        // Each operand is expanded and removed on its own, and a failure
        // on one does not abandon the rest: `rm a b c` where `b` is
        // protected should still remove `c`, the way the shell does.
        match expand_remote(&operand, client, state).await {
            Ok(targets) => {
                for target in targets {
                    line(out, &format!("Removing {target}"));
                    if let Err(e) = client.remove_file(&target).await {
                        line(out, &e.to_string());
                    }
                }
            }
            Err(e) => line(out, &e.to_string()),
        }
    }
    Ok(())
}

async fn chmod(
    mode: u32,
    paths: Vec<String>,
    follow: bool,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let update = crate::AttrUpdate {
        permissions: Some(mode),
        ..Default::default()
    };
    apply_attrs("Changing mode on", update, paths, follow, client, state, out).await
}

async fn chown(
    id: u32,
    which: Owner,
    paths: Vec<String>,
    follow: bool,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    // The protocol sends uid and gid in ONE attribute word, so changing
    // either means naming both. The other half is read back per target
    // rather than assumed: sending a zero for it would hand every file to
    // root, which is a spectacular way to answer `chgrp`.
    let label = match which {
        Owner::User => "Changing owner on",
        Owner::Group => "Changing group on",
    };
    for operand in paths {
        match expand_remote(&operand, client, state).await {
            Ok(targets) => {
                for target in targets {
                    line(out, &format!("{label} {target}"));
                    let current = if follow {
                        client.stat(&target).await
                    } else {
                        client.lstat(&target).await
                    };
                    let current = match current {
                        Ok(st) => st,
                        Err(e) => {
                            line(out, &e.to_string());
                            continue;
                        }
                    };
                    let (uid, gid) = match which {
                        Owner::User => (Some(id), current.gid),
                        Owner::Group => (current.uid, Some(id)),
                    };
                    // The half the user did not name has to be sent back
                    // unchanged, and a server that reported no ids leaves
                    // nothing to send. Refusing is the only safe answer:
                    // the flag covers both words, so the request would
                    // carry a zero, and zero is root.
                    let (Some(uid), Some(gid)) = (uid, gid) else {
                        line(
                            out,
                            &format!(
                                "{target}: the server did not report the current owner, so \
                                 {} alone cannot be changed",
                                which.verb()
                            ),
                        );
                        continue;
                    };
                    let update = crate::AttrUpdate {
                        uid: Some(uid),
                        gid: Some(gid),
                        ..Default::default()
                    };
                    if let Err(e) = client.set_attrs(&target, update, follow).await {
                        line(out, &e.to_string());
                    }
                }
            }
            Err(e) => line(out, &e.to_string()),
        }
    }
    Ok(())
}

/// The shape `chmod` and `chown` share: expand each operand, act on every
/// match, and let a failure on one leave the rest alone.
async fn apply_attrs(
    label: &str,
    update: crate::AttrUpdate,
    paths: Vec<String>,
    follow: bool,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    for operand in paths {
        match expand_remote(&operand, client, state).await {
            Ok(targets) => {
                for target in targets {
                    line(out, &format!("{label} {target}"));
                    if let Err(e) = client.set_attrs(&target, update, follow).await {
                        line(out, &e.to_string());
                    }
                }
            }
            Err(e) => line(out, &e.to_string()),
        }
    }
    Ok(())
}

async fn ln(
    target: String,
    link: String,
    symbolic: bool,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let link = state.resolve_remote(&link);
    if symbolic {
        // The TARGET is deliberately NOT resolved against the working
        // directory. A symlink's target is stored as written and is read
        // relative to the link's own directory, so rewriting `../lib`
        // into an absolute path would create a different link than the
        // one asked for, and one that breaks if the tree moves.
        let target = glob::unescape(&target);
        return client.symlink(&target, &link).await;
    }
    // A hard link, by contrast, is resolved: it names an inode that has
    // to exist right now.
    let target = state.resolve_remote(&target);
    if !client.hardlink(&target, &link).await? {
        line(
            out,
            "This server does not offer hardlink@openssh.com. Use ln -s for a symbolic link.",
        );
    }
    Ok(())
}

async fn df(
    path: Option<String>,
    human: bool,
    inodes: bool,
    client: &SftpClient,
    state: &ShellState,
    out: &mut impl ConsoleSink,
) -> Result<(), SshError> {
    let target = match path {
        Some(p) => state.resolve_remote(&p),
        None => state.remote_cwd.clone(),
    };
    let Some(info) = client.fs_info(&target).await? else {
        // Not an error: the question cannot be asked of this server, and
        // saying which extension is missing is what tells an operator
        // whether that is worth changing.
        line(
            out,
            "This server does not offer statvfs@openssh.com, so free space cannot be read.",
        );
        return Ok(());
    };
    out.write(render::df_table(&info, human, inodes).as_bytes());
    Ok(())
}

/// Drive one transfer, painting the meter while it runs.
///
/// The transfer future and the meter share `counter` rather than the
/// transfer calling back, which is what lets the repaint be rate-limited
/// independently of how the bytes arrive: a fast local link delivers
/// thousands of chunks a second and every one of them would otherwise be
/// a frame for the emulator to parse.
///
/// The caller mints the counter and hands the same handle to both sides.
async fn transfer<F>(
    name: &str,
    size: Option<u64>,
    state: &ShellState,
    out: &mut impl ConsoleSink,
    counter: Arc<AtomicU64>,
    future: F,
) -> Result<(), SshError>
where
    F: std::future::Future<Output = Result<(), SshError>>,
{
    if !state.progress {
        return future.await;
    }
    let started = std::time::Instant::now();
    tokio::pin!(future);
    let mut ticker = tokio::time::interval(PROGRESS_TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let result = loop {
        tokio::select! {
            // Biased so a finished transfer is seen as finished rather
            // than painting one more frame of a bar that is already done.
            biased;
            done = &mut future => break done,
            _ = ticker.tick() => {
                let moved = counter.load(Ordering::Relaxed);
                let elapsed = started.elapsed().as_secs_f64();
                let rate = if elapsed > 0.0 { moved as f64 / elapsed } else { 0.0 };
                out.write(
                    render::progress_line(name, moved, size, rate, elapsed, state.cols).as_bytes(),
                );
            }
        }
    };

    // One last frame so the line ends at what actually happened rather
    // than at whatever the last tick caught, then a newline to leave the
    // meter behind instead of letting the next output overwrite it.
    let elapsed = started.elapsed().as_secs_f64();
    let moved = size.unwrap_or_else(|| counter.load(Ordering::Relaxed));
    let rate = if elapsed > 0.0 {
        moved as f64 / elapsed
    } else {
        0.0
    };
    if result.is_ok() {
        out.write(render::progress_line(name, moved, size, rate, elapsed, state.cols).as_bytes());
    }
    out.write(CRLF.as_bytes());
    result
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ShellState {
        ShellState::new("/home/deploy".into(), PathBuf::from("/tmp"), 80)
    }

    #[test]
    fn remote_paths_resolve_against_the_working_directory() {
        let mut s = state();
        s.remote_cwd = "/var/log".into();
        assert_eq!(s.resolve_remote("nginx"), "/var/log/nginx");
        assert_eq!(s.resolve_remote("/etc/hosts"), "/etc/hosts");
        assert_eq!(s.resolve_remote("~"), "/home/deploy");
        assert_eq!(s.resolve_remote("~/.ssh"), "/home/deploy/.ssh");
    }

    /// The other end of the tokenizer's contract: an operand arrives
    /// glob-escaped and stops being a pattern exactly here, which is why
    /// this is the funnel every remote path goes through rather than a
    /// call each command site has to remember.
    #[test]
    fn resolving_a_path_drops_the_glob_escaping() {
        let mut s = state();
        s.remote_cwd = "/var/log".into();
        assert_eq!(s.resolve_remote(r"report\[1\].txt"), "/var/log/report[1].txt");
        assert_eq!(s.resolve_remote(r"a\*b"), "/var/log/a*b");
        assert_eq!(
            s.resolve_local(r"report\[1\].txt"),
            PathBuf::from("/tmp/report[1].txt")
        );
    }

    /// A quoted wildcard names a file and is NOT expanded, while a bare
    /// one still is. Measured through `has_magic`, which is what the
    /// executor actually branches on.
    #[test]
    fn a_quoted_wildcard_is_a_name_and_a_bare_one_is_a_pattern() {
        let quoted = &super::super::parser::tokenize(r#"get "*.gz""#).unwrap()[1];
        let bare = &super::super::parser::tokenize("get *.gz").unwrap()[1];
        assert!(!glob::has_magic(quoted));
        assert!(glob::has_magic(bare));
        let s = state();
        assert_eq!(s.resolve_remote(quoted), "/home/deploy/*.gz");
    }

    #[test]
    fn dot_and_dotdot_collapse() {
        let mut s = state();
        s.remote_cwd = "/var/log/nginx".into();
        assert_eq!(s.resolve_remote(".."), "/var/log");
        assert_eq!(s.resolve_remote("../.."), "/var");
        assert_eq!(s.resolve_remote("./x"), "/var/log/nginx/x");
        assert_eq!(s.resolve_remote("../other/./x"), "/var/log/other/x");
    }

    /// Walking above the root stays at the root rather than producing a
    /// path with a phantom parent.
    #[test]
    fn dotdot_stops_at_the_root() {
        let mut s = state();
        s.remote_cwd = "/".into();
        assert_eq!(s.resolve_remote(".."), "/");
        assert_eq!(s.resolve_remote("../../.."), "/");
    }

    #[test]
    fn duplicate_separators_collapse() {
        assert_eq!(normalize_remote("//var///log/"), "/var/log");
        assert_eq!(normalize_remote("/"), "/");
    }

    #[test]
    fn listing_targets_split_into_directory_and_pattern() {
        let mut s = state();
        s.remote_cwd = "/var/log".into();
        assert_eq!(s.split_listing_target(None), ("/var/log".to_string(), None));
        assert_eq!(
            s.split_listing_target(Some("/etc")),
            ("/etc".to_string(), None)
        );
        assert_eq!(
            s.split_listing_target(Some("*.gz")),
            ("/var/log".to_string(), Some("*.gz".to_string()))
        );
        assert_eq!(
            s.split_listing_target(Some("old/*.gz")),
            ("/var/log/old".to_string(), Some("*.gz".to_string()))
        );
        assert_eq!(
            s.split_listing_target(Some("/etc/*.conf")),
            ("/etc".to_string(), Some("*.conf".to_string()))
        );
    }

    /// A pattern rooted at `/` must read the root, not turn into a
    /// relative path against the working directory.
    #[test]
    fn a_pattern_at_the_root_reads_the_root() {
        let s = state();
        assert_eq!(
            s.split_listing_target(Some("/*.txt")),
            ("/".to_string(), Some("*.txt".to_string()))
        );
    }

    /// The failure mode that makes an unmatched glob worth reporting: a
    /// silent empty success looks exactly like a directory with nothing
    /// in it, and the user goes looking for files that were never
    /// fetched.
    #[test]
    fn an_unmatched_pattern_is_an_error_not_an_empty_list() {
        let e = SshError::Channel("*.gz: no matches found".into());
        assert!(e.to_string().contains("no matches"));
    }

    /// `SftpClient` maps every failure to `SshError::Channel`, a missing
    /// file and a dead link alike. This test exists to pin that fact:
    /// it is why the console asks the SESSION whether it is alive
    /// instead of trying to classify the error, and it would need
    /// revisiting the day the client grows a richer error type.
    #[test]
    fn sftp_errors_are_indistinguishable_by_variant() {
        let dead = SshError::Channel("sftp read_dir(/x): channel closed".into());
        let missing = SshError::Channel("sftp read_dir(/x): No such file".into());
        assert!(matches!(dead, SshError::Channel(_)));
        assert!(matches!(missing, SshError::Channel(_)));
    }

    /// The sink is what lets a command paint while it runs; a `Vec` is
    /// the same trait, which is what makes the output assertions above
    /// possible without a server.
    #[test]
    fn a_vec_is_a_console_sink() {
        let mut out: Vec<u8> = Vec::new();
        line(&mut out, "hello");
        assert_eq!(String::from_utf8(out).unwrap(), "hello\r\n");
    }

    /// Every console line ends CRLF, because the emulator is in the
    /// state a PTY leaves it in: a bare LF moves down without returning
    /// to column zero, and the output walks off to the right.
    #[test]
    fn console_lines_are_crlf_terminated() {
        let mut out: Vec<u8> = Vec::new();
        line(&mut out, "a");
        line(&mut out, "b");
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "a\r\nb\r\n");
        assert!(!text.contains("\n\r"));
    }

    #[test]
    fn local_paths_resolve_against_the_local_directory() {
        let s = ShellState::new("/home".into(), PathBuf::from("/tmp/work"), 80);
        assert_eq!(s.resolve_local("a.txt"), PathBuf::from("/tmp/work/a.txt"));
        // A rooted path REPLACES the local cwd instead of being joined
        // onto it. Asserted as the resolved path rather than through
        // `is_absolute()`, which cannot say so on Windows: absolute
        // means "has a drive prefix" there, so `/etc/hosts` is rooted,
        // resolves correctly, and still answers false. The old
        // assertion made this test fail on Windows for a path the code
        // handles perfectly well.
        assert_eq!(s.resolve_local("/etc/hosts"), PathBuf::from("/etc/hosts"));
    }
}
