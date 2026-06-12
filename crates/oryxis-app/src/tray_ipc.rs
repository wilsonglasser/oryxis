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
//!   that directory on every TrayPoll tick, filters out dead PIDs
//!   (via OpenProcess), and uses the survivors to populate the
//!   "Hidden windows" section of its tray menu.
//! - When the user clicks one of those entries, primary writes
//!   `commands/<pid>.json` and that child's TrayPoll picks it up
//!   on its next tick and reacts (e.g. show the window).
//!
//! Failure modes:
//!
//! - **Hard kill**: child process gone, file stays. Primary sweeps
//!   stale files based on PID liveness on each scan.
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
    let mut p = dirs::home_dir()?;
    p.push(".oryxis");
    p.push("runtime");
    Some(p)
}

fn instances_dir() -> Option<PathBuf> {
    runtime_root().map(|p| p.join("instances"))
}

fn commands_dir() -> Option<PathBuf> {
    runtime_root().map(|p| p.join("commands"))
}

/// Create the runtime subdirectories if missing. Idempotent; the
/// `create_dir_all` call no-ops on existing paths. Failures are
/// logged and swallowed because there's nothing useful the caller
/// can do at the call site (the tray feature degrades gracefully).
pub fn init_runtime_dirs() {
    if let Some(dir) = instances_dir()
        && let Err(e) = fs::create_dir_all(&dir)
    {
        tracing::warn!("tray_ipc: failed to create {:?}: {e}", dir);
    }
    if let Some(dir) = commands_dir()
        && let Err(e) = fs::create_dir_all(&dir)
    {
        tracing::warn!("tray_ipc: failed to create {:?}: {e}", dir);
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
            if !is_process_alive(state.pid) {
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

/// PID liveness check. Windows-only path opens the process for
/// query access; success means it's still running. Non-Windows
/// targets always return false (the whole multi-window tray story
/// is Windows-only today).
#[cfg(target_os = "windows")]
fn is_process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: pid is a primitive integer, OpenProcess is documented
    // safe to call from any thread, return value is a handle or
    // null; we close on success.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        CloseHandle(h);
        true
    }
}

#[cfg(not(target_os = "windows"))]
fn is_process_alive(_pid: u32) -> bool {
    false
}
