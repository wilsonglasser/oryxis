//! File-based IPC between Oryxis processes for the multi-window
//! tray story. Cross-process named pipes on Windows would be the
//! "correct" answer, but for our needs (small payloads, ~100 ms
//! latency tolerance, primary already polling on a timer) a flat
//! `~/.oryxis/runtime/{instances,commands}/` directory does the
//! job in a fraction of the code:
//!
//! - Each running Oryxis writes its current state (window title,
//!   tab labels, hidden / visible) to `instances/<pid>.json`.
//! - The primary (first to grab the single-instance mutex) scans
//!   that directory on every TrayPoll tick, filters out the rows
//!   whose PID is no longer an Oryxis process (`is_live_oryxis`:
//!   a freed PID is handed to the next program that starts), and
//!   uses the survivors to populate the
//!   "Hidden windows" section of its tray menu.
//! - When the user clicks one of those entries, primary writes
//!   `commands/<pid>.json` and that child's TrayPoll picks it up
//!   on its next tick and reacts (e.g. show the window).
//!
//! Failure modes:
//!
//! - **Hard kill**: child process gone, file stays. Primary sweeps
//!   stale files on each scan, by what the PID is now, not by
//!   whether something answers to it.
//! - **Primary dies**: children's state files persist. A future
//!   release will add primary handoff (first surviving child picks
//!   up the mutex), v0.7 just orphans the tray; user has to relaunch.
//! - **Disk write fails**: best-effort, never panic; the worst case
//!   is the tray under-reports state for one tick.
//!
//! All file I/O is synchronous to keep the module side-effect-free
//! from iced's perspective; calls happen from the iced UI thread on
//! TrayPoll / dispatch handlers, and the payloads are small enough
//! that the latency is invisible (<1 ms in practice).

// Dead-code allow because the Primary side of the API is wired in
// the next PR in this series (9d). Without the allow the
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Snapshot a child process writes to advertise its current state.
/// Versioned with `#[serde(default)]` on every field so older
/// primaries reading newer payloads (or vice versa) don't crash on
/// added keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    pub pid: u32,
    #[serde(default)]
    pub title: String,
    /// Tab labels for the "Active sessions" submenu the primary
    /// renders. Empty for a window with no open SSH tabs (lock
    /// screen, dashboard idle, etc.).
    #[serde(default)]
    pub tabs: Vec<String>,
    /// True when this instance has hidden its window to the tray
    /// (via close-to-tray or minimize-to-tray). False means the
    /// window is visible somewhere on the desktop. The primary
    /// uses this to decide which entries to surface in the menu
    /// and whether the tray icon itself should be visible.
    #[serde(default)]
    pub is_hidden: bool,
}

/// Commands the primary writes for a specific child to consume on
/// its next TrayPoll tick. Single-shot: child reads + deletes the
/// file so a second click queues a fresh command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum Command {
    /// Surface this child's window: hop through Win32 ShowWindow
    /// (same path as TrayShow Message) so the window comes back
    /// from a hidden state and grabs focus.
    Show,
    /// Quit the child process cleanly. Mirrors what TrayQuit does
    /// inside a single process but reachable from the primary's
    /// tray when the user wants to close a backgrounded window
    /// without surfacing it first.
    Quit,
}

fn runtime_root() -> Option<PathBuf> {
    let mut p = oryxis_core::paths::oryxis_dir()?;
    p.push("runtime");
    Some(p)
}

fn instances_dir() -> Option<PathBuf> {
    runtime_root().map(|p| p.join("instances"))
}

fn commands_dir() -> Option<PathBuf> {
    runtime_root().map(|p| p.join("commands"))
}

fn deeplink_dir() -> Option<PathBuf> {
    runtime_root().map(|p| p.join("deeplink"))
}

/// Inbox for `oryxis user@host` CLI launches, deliberately SEPARATE
/// from the deep-link one. The two payloads look identical on the wire
/// (both are just a target string) but route differently: a CLI target
/// dials, a `ssh://` link only prefills a confirm surface. Keeping them
/// in different directories makes provenance structural, so a claiming
/// window never has to guess which launcher wrote a file.
fn connect_dir() -> Option<PathBuf> {
    runtime_root().map(|p| p.join("connect"))
}

/// Create the runtime subdirectories if missing. Idempotent; the
/// `create_dir_all` call no-ops on existing paths. Failures are
/// logged and swallowed because there's nothing useful the caller
/// can do at the call site (the tray feature degrades gracefully).
pub fn init_runtime_dirs() {
    for dir in [instances_dir(), commands_dir(), deeplink_dir(), connect_dir()]
        .into_iter()
        .flatten()
    {
        if let Err(e) = fs::create_dir_all(&dir) {
            tracing::warn!("tray_ipc: failed to create {:?}: {e}", dir);
        }
    }
}

fn current_pid() -> u32 {
    std::process::id()
}

fn write_atomic(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    // Same dir + rename so the read side never observes a partial
    // file. tempfile crate would be overkill for one .tmp suffix.
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Child-side API. The current process announces itself, updates
/// its state on window events, polls for commands, and unregisters
/// on shutdown.
pub struct Child;

impl Child {
    fn instance_path() -> Option<PathBuf> {
        instances_dir().map(|d| d.join(format!("{}.json", current_pid())))
    }

    fn command_path() -> Option<PathBuf> {
        commands_dir().map(|d| d.join(format!("{}.json", current_pid())))
    }

    /// Write the initial instance file with the given title (and
    /// an empty tabs list, `is_hidden = false`). Called once at
    /// child boot after we detect we're not the primary.
    pub fn register(title: impl Into<String>) {
        Self::write_state(InstanceState {
            pid: current_pid(),
            title: title.into(),
            tabs: Vec::new(),
            is_hidden: false,
        });
    }

    /// Replace the on-disk state. The primary picks up the change
    /// on its next TrayPoll scan (~100 ms later).
    pub fn write_state(state: InstanceState) {
        let Some(path) = Self::instance_path() else { return };
        let bytes = match serde_json::to_vec(&state) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("tray_ipc: serialize instance: {e}");
                return;
            }
        };
        if let Err(e) = write_atomic(&path, &bytes) {
            tracing::warn!("tray_ipc: write {:?}: {e}", path);
        }
    }

    /// Remove the instance file. Best-effort: the primary's sweep
    /// will eventually clean it up via the PID-liveness check, but
    /// calling this on graceful shutdown shaves a tick of staleness
    /// off the user-facing menu.
    pub fn unregister() {
        if let Some(path) = Self::instance_path() {
            let _ = fs::remove_file(path);
        }
    }

    /// Read + consume any pending command from the primary. Returns
    /// `None` when no command is queued. The file is deleted after
    /// a successful read so the same command doesn't fire twice.
    pub fn poll_command() -> Option<Command> {
        let path = Self::command_path()?;
        let bytes = fs::read(&path).ok()?;
        // Delete BEFORE handing the command back: if the caller
        // panics processing it, we don't want the same command
        // re-firing forever on the next tick.
        let _ = fs::remove_file(&path);
        match serde_json::from_slice::<Command>(&bytes) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("tray_ipc: parse command: {e}");
                None
            }
        }
    }
}

/// Primary-side API. The single process that holds the
/// single-instance mutex reads the registry, sends commands.
pub struct Primary;

/// Cache keyed by registry file path. Each entry remembers the mtime
/// of the underlying file the last time we successfully parsed it
/// plus the parsed `InstanceState`, so the 100 ms `TrayPoll` tick can
/// skip the JSON parse for unchanged rows (the typical case). A
/// missing entry, an mtime change, or a vanished file all fall back
/// to the slow path (`fs::read` + parse).
static INSTANCE_CACHE: std::sync::Mutex<
    Option<std::collections::HashMap<std::path::PathBuf, CachedInstance>>,
> = std::sync::Mutex::new(None);

#[derive(Clone)]
struct CachedInstance {
    mtime: std::time::SystemTime,
    state: InstanceState,
}

impl Primary {
    /// Scan `instances/` and return one `InstanceState` per file.
    /// Files whose PID no longer maps to a live process are deleted
    /// during the scan so the registry self-heals from hard kills.
    /// Returns an empty vec on any I/O error (degrade gracefully).
    ///
    /// Skips the JSON parse for entries whose mtime hasn't changed
    /// since the last poll. The steady state for an idle fleet of
    /// children is "nothing changed", so re-parsing every row every
    /// 100 ms is wasted work. Cache is keyed by path; stale entries
    /// for removed files fall out implicitly because we rebuild the
    /// map on every scan from the current `read_dir` listing.
    pub fn list_instances() -> Vec<InstanceState> {
        let Some(dir) = instances_dir() else { return Vec::new() };
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<InstanceState> = Vec::new();
        let self_pid = current_pid();
        // Lock the cache once for the whole scan. Poisoned mutexes
        // recover the inner value: a stale cache is harmless.
        let mut cache_guard = match INSTANCE_CACHE.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let cache = cache_guard.get_or_insert_with(std::collections::HashMap::new);
        let mut next: std::collections::HashMap<std::path::PathBuf, CachedInstance> =
            std::collections::HashMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            // Cheap stat first: if mtime matches the cached entry,
            // reuse the parsed state without re-reading the file.
            let mtime = entry.metadata().and_then(|m| m.modified()).ok();
            let state: InstanceState = match (mtime, cache.get(&path)) {
                (Some(now), Some(cached)) if cached.mtime == now => cached.state.clone(),
                _ => {
                    let bytes = match fs::read(&path) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    match serde_json::from_slice(&bytes) {
                        Ok(s) => s,
                        Err(_) => {
                            let _ = fs::remove_file(&path);
                            continue;
                        }
                    }
                }
            };
            // Skip our own row, the primary tracks its own state
            // via the in-process Oryxis struct, not the registry.
            if state.pid == self_pid {
                continue;
            }
            if !is_live_oryxis(state.pid) {
                let _ = fs::remove_file(&path);
                continue;
            }
            if let Some(m) = mtime {
                next.insert(
                    path.clone(),
                    CachedInstance {
                        mtime: m,
                        state: state.clone(),
                    },
                );
            }
            out.push(state);
        }
        // Swap in the new map. Entries for files that vanished
        // between polls drop here.
        *cache = next;
        out
    }

    /// Queue a command for the given child PID. Idempotent: writing
    /// over an existing pending command just replaces it; the next
    /// child poll consumes the latest.
    pub fn send_command(pid: u32, command: Command) {
        let Some(dir) = commands_dir() else { return };
        let path = dir.join(format!("{pid}.json"));
        let bytes = match serde_json::to_vec(&command) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("tray_ipc: serialize command: {e}");
                return;
            }
        };
        if let Err(e) = write_atomic(&path, &bytes) {
            tracing::warn!("tray_ipc: write {:?}: {e}", path);
        }
    }
}

/// Drop a deep-link URL into the cross-process inbox for a running
/// instance to claim. Returns the file's path so the (short-lived)
/// launcher process can watch for the claim and fall back to booting
/// a window itself if nobody picks the link up.
pub fn write_deeplink(url: &str) -> Option<PathBuf> {
    let dir = deeplink_dir()?;
    // pid + counter: unique even if one launcher ever wrote twice.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{}-{seq}.link", current_pid()));
    match write_atomic(&path, url.as_bytes()) {
        Ok(()) => Some(path),
        Err(e) => {
            tracing::warn!("tray_ipc: write deep link {:?}: {e}", path);
            None
        }
    }
}

/// Claim + consume every pending deep link. Exactly-once across
/// instances: each file is renamed (atomic on every platform we ship)
/// to a per-claimant name first, so when several windows poll the same
/// inbox only the rename winner reads and handles a given link. Files
/// that fail the rename belong to another instance and are skipped.
pub fn take_deeplinks() -> Vec<String> {
    let Some(dir) = deeplink_dir() else { return Vec::new() };
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("link") {
            continue;
        }
        let claimed = path.with_extension(format!("claim-{}", current_pid()));
        if fs::rename(&path, &claimed).is_err() {
            continue; // another instance won this link
        }
        if let Ok(bytes) = fs::read(&claimed)
            && let Ok(url) = String::from_utf8(bytes)
        {
            out.push(url);
        }
        let _ = fs::remove_file(&claimed);
    }
    out
}

/// Hand a CLI connect target to a running instance. Same courier
/// protocol as [`write_deeplink`] (caller waits for the file to vanish,
/// reclaims it on timeout), on the separate inbox that carries the
/// "the user typed this locally" provenance.
pub fn write_connect(target: &str) -> Option<PathBuf> {
    let dir = connect_dir()?;
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{}-{seq}.connect", current_pid()));
    match write_atomic(&path, target.as_bytes()) {
        Ok(()) => Some(path),
        Err(e) => {
            tracing::warn!("tray_ipc: write connect target {:?}: {e}", path);
            None
        }
    }
}

/// Claim + consume every pending CLI connect target, with the same
/// rename-to-claim exactly-once rule as [`take_deeplinks`].
pub fn take_connects() -> Vec<String> {
    let Some(dir) = connect_dir() else { return Vec::new() };
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("connect") {
            continue;
        }
        let claimed = path.with_extension(format!("claim-{}", current_pid()));
        if fs::rename(&path, &claimed).is_err() {
            continue; // another instance won this target
        }
        if let Ok(bytes) = fs::read(&claimed)
            && let Ok(target) = String::from_utf8(bytes)
        {
            out.push(target);
        }
        let _ = fs::remove_file(&claimed);
    }
    out
}

/// Whether any OTHER live Oryxis instance is registered. Used by a
/// deep-link launcher process to decide between forwarding the URL and
/// booting a window itself. PID files whose process died (hard kill)
/// are swept here, which is also what keeps the registry clean on
/// Linux/macOS where the Windows primary's tray sweep never runs.
pub fn any_live_instance() -> bool {
    let Some(dir) = instances_dir() else { return false };
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let self_pid = current_pid();
    let mut alive = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        // The file name IS the pid (`<pid>.json`), no parse needed.
        let Some(pid) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        if is_live_oryxis(pid) {
            alive = true;
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    alive
}

/// Whether `pid` is a live Oryxis process. The number alone does not
/// say: only a graceful exit removes a registry row, so a window
/// killed from Task Manager (or taken down by a crash) leaves its file
/// behind, and Windows hands a freed PID to the next process that
/// starts, within seconds. Probing the number then reports whatever
/// got it (a browser helper, an updater) as an Oryxis window, which
/// refused the in-place update ("close the other windows") and handed
/// a deep link to nobody until a reboot. So the probe asks what the
/// process IS: its image must carry this executable's file name, see
/// [`same_image`].
///
/// Windows opens the process for limited query access and reads its
/// image path; Linux reads `/proc/<pid>/exe`, which also answers "no"
/// for a zombie, where `kill(pid, 0)` would still say yes. Both treat
/// an unreadable image as "not ours": the registry lives under this
/// user's home, so a sibling is always a process this user can query.
#[cfg(target_os = "windows")]
fn is_live_oryxis(pid: u32) -> bool {
    use std::os::windows::ffi::OsStringExt as _;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // The longest path the kernel hands back, in UTF-16 units: an
    // install directory past MAX_PATH must not read as "not ours".
    let mut image = vec![0u16; 32_768];
    let mut len = image.len() as u32;
    // SAFETY: pid is a primitive integer and OpenProcess is documented
    // safe to call from any thread; it returns a handle or null. The
    // buffer outlives the query, `len` carries its capacity in and the
    // written length out, and the handle is closed on every path.
    let ok = unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, image.as_mut_ptr(), &mut len);
        CloseHandle(h);
        ok != 0
    };
    if !ok {
        return false;
    }
    let image = std::ffi::OsString::from_wide(&image[..len as usize]);
    same_image(std::path::Path::new(&image), &own_image_stem())
}

#[cfg(target_os = "linux")]
fn is_live_oryxis(pid: u32) -> bool {
    match std::fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(exe) => same_image(&exe, &own_image_stem()),
        Err(_) => false,
    }
}

/// macOS has no image lookup short of libproc, and its PIDs are not
/// recycled the way Windows' are: the plain existence probe stays.
/// Signal 0 performs the permission / existence checks without
/// delivering anything; EPERM means the process exists but is not
/// ours, which still counts as alive.
#[cfg(all(unix, not(target_os = "linux")))]
fn is_live_oryxis(pid: u32) -> bool {
    // SAFETY: kill with signal 0 only validates, it never signals.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    rc == 0
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(any(target_os = "windows", unix)))]
fn is_live_oryxis(_pid: u32) -> bool {
    false
}

/// This executable's file stem, lower-cased: the name every sibling
/// shares (`oryxis`, whatever directory or scope it was installed to).
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn own_image_stem() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| image_stem(&exe))
        .unwrap_or_else(|| "oryxis".to_string())
}

/// Whether `image` names the same program as `own_stem` (the value of
/// [`own_image_stem`]): same file stem, case-insensitively, so
/// `ORYXIS.EXE` under Program Files and `oryxis.exe` in a portable
/// folder are siblings. Linux suffixes the link of a binary that was
/// replaced under a running process with ` (deleted)`, which is still
/// the old Oryxis for as long as it runs.
#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn same_image(image: &std::path::Path, own_stem: &str) -> bool {
    image_stem(image).as_deref() == Some(own_stem)
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn image_stem(image: &std::path::Path) -> Option<String> {
    let name = image.file_name()?.to_string_lossy().into_owned();
    let name = name.strip_suffix(" (deleted)").unwrap_or(&name);
    let stem = std::path::Path::new(name).file_stem()?.to_string_lossy().to_lowercase();
    Some(stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paths are built with `join` so the file-name split follows the
    /// platform the test runs on (a backslash is a plain character on
    /// Linux).
    #[test]
    fn same_image_compares_the_file_stem_case_insensitively() {
        let own = "oryxis";
        let dir = std::path::PathBuf::from("install").join("Oryxis");
        assert!(same_image(&dir.join("ORYXIS.EXE"), own));
        assert!(same_image(&dir.join("oryxis"), own));
        assert!(same_image(&dir.join("oryxis (deleted)"), own));
        assert!(!same_image(&dir.join("oryxis-mcp.exe"), own));
        assert!(!same_image(&dir.join("sleep"), own));
        assert!(!same_image(&dir.join("svchost.exe"), own));
    }

    /// The probe compares against THIS binary's name, so the running
    /// test process is its own sibling.
    #[test]
    fn the_current_process_is_a_live_sibling() {
        assert!(is_live_oryxis(std::process::id()));
    }

    /// A live process that is not Oryxis is not a sibling: a stale
    /// registry row whose PID was handed to another program must sweep,
    /// not refuse the update.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    fn another_program_on_a_live_pid_is_not_a_sibling() {
        let (program, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
            ("ping", &["-n", "30", "127.0.0.1"])
        } else {
            ("sleep", &["30"])
        };
        let mut child = std::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn a long-lived child");
        let verdict = is_live_oryxis(child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(!verdict);
    }
}
