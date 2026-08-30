//! Optional debug logging to a file (Settings > Advanced) plus the
//! environment report copied into GitHub issues.
//!
//! A second `tracing_subscriber::fmt` layer (installed in `main.rs`)
//! writes through [`DebugFileWriter`], which forwards every formatted
//! event to a process-global file sink while the feature is on and
//! discards the bytes otherwise. The sink flips at runtime from the
//! Settings toggle without rebuilding the subscriber, and `main.rs`
//! arms it before the subscriber is built (the `debug_logging` setting
//! reads without the master password) so boot lines are captured too.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Fast path checked on every formatted event so the layer costs one
/// relaxed load while the feature is off (the common case).
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Set by the `--debug-log` command-line flag: logging stays on for the
/// whole process and the Settings toggle cannot switch it off.
///
/// The reason is a real diagnostic session (issue #104): the reporter
/// armed the toggle, used the app for 49 seconds, switched it off, and
/// only then hit the freezes we were instrumenting for, so the log came
/// back empty. A flag that outlives the toggle takes that failure mode
/// out of the loop. The persisted `debug_logging` setting is untouched,
/// so the next launch without the flag behaves as the user left it.
static FORCED: AtomicBool = AtomicBool::new(false);

/// The open log file while enabled. A single guarded handle (instead of
/// reopening per write) so enable/disable/clear and the tracing layer
/// never race on a half-open file.
static SINK: Mutex<Option<File>> = Mutex::new(None);

/// Rotation threshold: a debug session left on for weeks must not eat
/// the disk. 5 MB of plain text is plenty of history for an issue.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Lock the sink, shrugging off poisoning: a panic while holding the
/// lock leaves at worst a partially written log line, and diagnostics
/// must never take the app down with them.
fn sink() -> MutexGuard<'static, Option<File>> {
    SINK.lock().unwrap_or_else(|e| e.into_inner())
}

/// `~/.oryxis/oryxis-debug.log`, next to the vault. Self-describing
/// name so the file still identifies itself when attached to an issue.
pub(crate) fn log_path() -> Option<PathBuf> {
    oryxis_core::paths::oryxis_dir().map(|dir| dir.join("oryxis-debug.log"))
}

/// Sibling the oversized log rotates aside to on enable.
fn rotated_path(path: &Path) -> PathBuf {
    path.with_extension("log.old")
}

pub(crate) fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Whether `--debug-log` pinned logging on for this process.
pub(crate) fn is_forced() -> bool {
    FORCED.load(Ordering::Relaxed)
}

/// [`enable`] plus the pin that makes [`disable`] a no-op for the rest
/// of the process. Called from `main.rs` when `--debug-log` is present.
pub(crate) fn force_enable() -> io::Result<PathBuf> {
    let path = enable()?;
    FORCED.store(true, Ordering::Relaxed);
    Ok(path)
}

/// Open (or create) the log file, write a session header and start
/// forwarding tracing events to it. Idempotent while already enabled.
pub(crate) fn enable() -> io::Result<PathBuf> {
    let path = log_path().ok_or_else(|| io::Error::other("no home directory"))?;
    enable_at(&path)?;
    Ok(path)
}

fn enable_at(path: &Path) -> io::Result<()> {
    let mut guard = sink();
    if guard.is_some() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    rotate_if_oversized(path, MAX_LOG_BYTES);
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    write_session_header(&mut file)?;
    *guard = Some(file);
    ENABLED.store(true, Ordering::Relaxed);
    Ok(())
}

/// Move a grown log aside instead of truncating it, so one long session
/// doesn't destroy the history that made it interesting. Best-effort:
/// a failed rename just means the file keeps growing for now.
fn rotate_if_oversized(path: &Path, max_bytes: u64) {
    let oversized = std::fs::metadata(path).map(|m| m.len() > max_bytes).unwrap_or(false);
    if oversized {
        let _ = std::fs::rename(path, rotated_path(path));
    }
}

/// Stop forwarding events and close the file. The file itself stays on
/// disk so it can still be attached to an issue after switching off.
///
/// No-op under `--debug-log`: the flag is the stronger statement of
/// intent, and a diagnostic session that can be switched off by an
/// errant click is the exact failure this flag exists to prevent.
pub(crate) fn disable() {
    if is_forced() {
        return;
    }
    ENABLED.store(false, Ordering::Relaxed);
    if let Some(mut file) = sink().take() {
        let _ = file.flush();
    }
}

/// Wipe the log (a live sink's file is truncated through a fresh write
/// handle and re-stamped, otherwise the file and any rotated leftover
/// are deleted). Returns `false` when there was nothing to clear.
pub(crate) fn clear() -> io::Result<bool> {
    let Some(path) = log_path() else {
        return Ok(false);
    };
    clear_at(&path)
}

/// [`clear`] against an explicit path, split out (like [`enable_at`])
/// so the lifecycle test can exercise the real branches against its
/// sandboxed file instead of duplicating them.
fn clear_at(path: &Path) -> io::Result<bool> {
    let removed_old = std::fs::remove_file(rotated_path(path)).is_ok();
    let guard = sink();
    if guard.is_some() {
        // The sink handle is append-only, and Rust deliberately strips
        // FILE_WRITE_DATA from append handles on Windows, so a truncate
        // through the live handle is denied there ("Clear debug log"
        // errored whenever the log was on). Truncate through a second
        // write handle instead: append-mode writes always land at the
        // current end of file, so the sink keeps appending afterwards
        // and stays untouched when the wipe itself fails. The guard is
        // held across the wipe so the tracing layer cannot interleave.
        let mut fresh = OpenOptions::new().create(true).write(true).truncate(true).open(path)?;
        write_session_header(&mut fresh)?;
        return Ok(true);
    }
    drop(guard);
    if path.exists() {
        std::fs::remove_file(path)?;
        return Ok(true);
    }
    Ok(removed_old)
}

/// Each enable (and each boot while the setting is on) opens with a
/// timestamped banner plus the environment block, so a log spanning
/// several sessions stays legible and always carries the system info
/// the issue template asks for.
fn write_session_header(file: &mut File) -> io::Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z");
    writeln!(file, "==== Oryxis debug session started {now} ====")?;
    for line in environment_report(None).lines() {
        writeln!(file, "  {line}")?;
    }
    file.flush()
}

/// Stamp a panic into the debug-log file (no-op while the toggle is
/// off). Wired into the process panic hook in `main.rs`: the default
/// hook prints to stderr, which a `windows_subsystem = "windows"` GUI
/// build silently drops, so without this a user-reported crash leaves
/// nothing to attach to an issue. Best-effort by design, diagnostics
/// must never take the app down with them.
pub(crate) fn log_panic(info: &std::panic::PanicHookInfo<'_>) {
    if !is_enabled() {
        return;
    }
    let msg = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string());
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown location>".to_string());
    // Forced capture: the hook must not depend on RUST_BACKTRACE being
    // set in the crashing user's environment.
    let backtrace = std::backtrace::Backtrace::force_capture();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    if let Some(file) = sink().as_mut() {
        let _ = writeln!(file, "{now} PANIC at {location}: {msg}");
        let _ = writeln!(file, "{backtrace}");
        let _ = file.flush();
    }
}

/// `MakeWriter` handed to the file `fmt` layer in `main.rs`. Zero-sized;
/// all state lives in the module statics so the writer created per event
/// is free.
#[derive(Clone, Copy)]
pub(crate) struct DebugFileWriter;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for DebugFileWriter {
    type Writer = DebugFileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        *self
    }
}

impl Write for DebugFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if ENABLED.load(Ordering::Relaxed)
            && let Some(file) = sink().as_mut()
        {
            // Swallow write errors: a full disk must never crash the
            // app through its own diagnostics.
            let _ = file.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = sink().as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

/// The plain-text environment block: shown in Settings > Advanced,
/// copied to the clipboard for GitHub issues, and stamped into every
/// debug-log session header. `renderer` is the lazily loaded
/// `(backend, adapter)` pair from app state; the line is omitted while
/// it hasn't resolved (e.g. the log header written during boot).
pub(crate) fn environment_report(renderer: Option<&(String, String)>) -> String {
    let channel = if env!("ORYXIS_CHANNEL") == "nightly" { "nightly" } else { "stable" };
    let sha: String = env!("ORYXIS_GIT_SHA").chars().take(7).collect();
    let mut lines = vec![
        format!("Oryxis: v{} ({channel}, {sha})", env!("CARGO_PKG_VERSION")),
        format!("OS: {}", os_summary()),
    ];
    #[cfg(target_os = "linux")]
    {
        // Wayland-vs-X11 and the desktop in play decide which renderer
        // and clipboard quirks apply, worth one line on Linux.
        let session =
            std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_string());
        match std::env::var("XDG_CURRENT_DESKTOP") {
            Ok(desktop) if !desktop.is_empty() => {
                lines.push(format!("Display: {session}, {desktop}"));
            }
            _ => lines.push(format!("Display: {session}")),
        }
        // Implicit Vulkan layers (MangoHud, vkBasalt, vendor overlays)
        // inject into every Vulkan process, this one included, and are a
        // documented source of presentation stalls that read as app
        // freezes (#104). Naming them here turns "do you run an
        // overlay?" from a support question into a fact already on file.
        if let Some(layers) = vulkan_layers_line() {
            lines.push(format!("Vulkan implicit layers: {layers}"));
        }
    }
    if let Some((backend, adapter)) = renderer {
        lines.push(format!("Renderer: {backend}, {adapter}"));
    }
    lines.push(format!("Language: {}", crate::i18n::Language::active().code()));
    lines.join("\n")
}

/// The installed Vulkan implicit layers as one comma-joined line,
/// resolved once per process (the report renders on every frame of the
/// Settings view, and the loader only reads manifests at instance
/// creation anyway, so live changes would not affect this process).
/// `None` when no manifests exist, the common case, so the report
/// omits the line entirely rather than printing "none".
#[cfg(target_os = "linux")]
fn vulkan_layers_line() -> Option<&'static str> {
    static LINE: OnceLock<Option<String>> = OnceLock::new();
    LINE.get_or_init(|| {
        // The Vulkan-Loader implicit-layer search path (data dirs +
        // sysconf dirs + the per-user XDG homes). Missing dirs skip.
        let mut roots: Vec<PathBuf> = vec![
            PathBuf::from("/usr/share/vulkan/implicit_layer.d"),
            PathBuf::from("/usr/local/share/vulkan/implicit_layer.d"),
            PathBuf::from("/etc/vulkan/implicit_layer.d"),
        ];
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".local/share/vulkan/implicit_layer.d"));
            roots.push(home.join(".config/vulkan/implicit_layer.d"));
        }
        let mut layers: Vec<String> = Vec::new();
        for root in roots {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for layer in
                    parse_layer_manifest(&text, |var| std::env::var_os(var).is_some())
                {
                    // The same manifest often exists in more than one
                    // root (distro package + local install).
                    if !layers.contains(&layer) {
                        layers.push(layer);
                    }
                }
            }
        }
        // Explicit layers forced onto every instance from the
        // environment are just as relevant as the implicit set.
        if let Ok(forced) = std::env::var("VK_INSTANCE_LAYERS")
            && !forced.is_empty()
        {
            layers.push(format!("VK_INSTANCE_LAYERS={forced}"));
        }
        (!layers.is_empty()).then(|| layers.join(", "))
    })
    .as_deref()
}

/// Layer names out of one implicit-layer manifest, each marked
/// "(inactive)" when its environment gate keeps the loader from
/// injecting it. Loader semantics, best-effort: an implicit layer is
/// on by default, `enable_environment` restricts it to sessions where
/// that variable is set, and `disable_environment` wins over both.
/// The env lookup comes in as a closure so tests control it.
#[cfg(any(target_os = "linux", test))]
fn parse_layer_manifest(text: &str, env_is_set: impl Fn(&str) -> bool) -> Vec<String> {
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    // Manifests carry either one `layer` object or a `layers` array.
    let single = manifest.get("layer").into_iter();
    let multi = manifest
        .get("layers")
        .and_then(|l| l.as_array())
        .into_iter()
        .flatten();
    let mut out = Vec::new();
    for layer in single.chain(multi) {
        let Some(name) = layer.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let env_key = |key: &str| {
            layer
                .get(key)
                .and_then(|m| m.as_object())
                .and_then(|m| m.keys().next().cloned())
        };
        let enabled = env_key("enable_environment").is_none_or(|var| env_is_set(&var));
        let disabled = env_key("disable_environment").is_some_and(|var| env_is_set(&var));
        if enabled && !disabled {
            out.push(name.to_string());
        } else {
            out.push(format!("{name} (inactive)"));
        }
    }
    out
}

/// OS name + version + arch, resolved once per process (`os_info::get`
/// reads platform sources, not something to repeat on every redraw of
/// the settings view).
fn os_summary() -> &'static str {
    static SUMMARY: OnceLock<String> = OnceLock::new();
    SUMMARY.get_or_init(|| {
        let info = os_info::get();
        // Only the Linux arm below appends to it.
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut summary =
            format!("{} {} ({})", info.os_type(), info.version(), std::env::consts::ARCH);
        #[cfg(target_os = "linux")]
        {
            // The kernel string tells the WSL / distro-kernel stories
            // the os-release name doesn't.
            if let Ok(out) = std::process::Command::new("uname").arg("-r").output()
                && out.status.success()
            {
                let kernel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !kernel.is_empty() {
                    summary.push_str(&format!(", kernel {kernel}"));
                }
            }
        }
        summary
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sequential test for the whole lifecycle: the sink statics are
    /// process-global, so parallel test fns would trample each other.
    #[test]
    fn debug_log_lifecycle() {
        let dir = std::env::temp_dir().join(format!("oryxis-logging-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("oryxis-debug.log");

        // Disabled: the writer discards without erroring.
        assert!(!is_enabled());
        assert_eq!(DebugFileWriter.write(b"dropped\n").unwrap(), 8);

        // Enable creates the dir + file and stamps the session header
        // with the environment block.
        enable_at(&path).unwrap();
        assert!(is_enabled());
        let header = std::fs::read_to_string(&path).unwrap();
        assert!(header.contains("Oryxis debug session started"));
        assert!(header.contains(concat!("Oryxis: v", env!("CARGO_PKG_VERSION"))));

        // Enabled: writes land in the file. Re-enabling is a no-op.
        DebugFileWriter.write_all(b"line-a\n").unwrap();
        enable_at(&path).unwrap();
        DebugFileWriter.write_all(b"line-b\n").unwrap();
        DebugFileWriter.flush().unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("line-a"));
        assert!(body.contains("line-b"));

        // Clear wipes the file and re-stamps the header while the sink
        // stays armed. (clear() resolves the real home-dir path, so go
        // through clear_at against the test file.) On Windows the
        // append sink cannot truncate itself (Rust strips
        // FILE_WRITE_DATA from append handles), so the wipe goes
        // through a second write handle.
        assert!(clear_at(&path).unwrap());
        let cleared = std::fs::read_to_string(&path).unwrap();
        assert!(cleared.contains("Oryxis debug session started"));
        assert!(!cleared.contains("line-a"));
        // The untouched sink keeps appending at the new end of file.
        DebugFileWriter.write_all(b"line-c\n").unwrap();
        DebugFileWriter.flush().unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("line-c"));

        // Disable closes the sink and the writer goes back to discarding.
        disable();
        assert!(!is_enabled());
        let before = std::fs::read_to_string(&path).unwrap();
        DebugFileWriter.write_all(b"after-disable\n").unwrap();
        assert_eq!(before, std::fs::read_to_string(&path).unwrap());

        // Under --debug-log the sink is pinned: the Settings toggle
        // routes through disable(), which must not be able to end a
        // diagnostic session (issue #104, where the reporter switched
        // the log off before the freezes we were instrumenting for).
        assert!(!is_forced());
        enable_at(&path).unwrap();
        FORCED.store(true, Ordering::Relaxed);
        disable();
        assert!(is_enabled(), "--debug-log must survive disable()");
        DebugFileWriter.write_all(b"after-forced-disable\n").unwrap();
        DebugFileWriter.flush().unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("after-forced-disable"));

        // Unpinned again, disable() behaves normally. Restored before
        // the test ends so the process-global statics don't leak into
        // whatever runs next.
        FORCED.store(false, Ordering::Relaxed);
        disable();
        assert!(!is_enabled());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_moves_oversized_log_aside() {
        let dir =
            std::env::temp_dir().join(format!("oryxis-rotate-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("oryxis-debug.log");

        std::fs::write(&path, b"0123456789").unwrap();
        rotate_if_oversized(&path, 4);
        assert!(!path.exists());
        assert_eq!(std::fs::read(rotated_path(&path)).unwrap(), b"0123456789");

        // Under the threshold nothing moves.
        std::fs::write(&path, b"abc").unwrap();
        rotate_if_oversized(&path, 4);
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layer_manifest_names_and_env_gates() {
        // MangoHud-style manifest: enable_environment gates injection.
        let mangohud = r#"{
            "file_format_version": "1.0.0",
            "layer": {
                "name": "VK_LAYER_MANGOHUD_overlay",
                "enable_environment": {"MANGOHUD": "1"},
                "disable_environment": {"DISABLE_MANGOHUD": "1"}
            }
        }"#;
        // Gate variable unset: present but not injected.
        assert_eq!(
            parse_layer_manifest(mangohud, |_| false),
            vec!["VK_LAYER_MANGOHUD_overlay (inactive)"]
        );
        // Gate set: injected into every Vulkan process.
        assert_eq!(
            parse_layer_manifest(mangohud, |var| var == "MANGOHUD"),
            vec!["VK_LAYER_MANGOHUD_overlay"]
        );
        // Disable wins over enable.
        assert_eq!(
            parse_layer_manifest(mangohud, |_| true),
            vec!["VK_LAYER_MANGOHUD_overlay (inactive)"]
        );

        // No env gates at all (vkBasalt-style): active by default, and
        // the `layers` array form parses the same as `layer`.
        let plain = r#"{"layers": [{"name": "VK_LAYER_VKBASALT_post_processing"}]}"#;
        assert_eq!(
            parse_layer_manifest(plain, |_| false),
            vec!["VK_LAYER_VKBASALT_post_processing"]
        );

        // Garbage input degrades to nothing, never a panic.
        assert!(parse_layer_manifest("not json", |_| false).is_empty());
        assert!(parse_layer_manifest("{}", |_| false).is_empty());
    }

    #[test]
    fn environment_report_shape() {
        let report = environment_report(Some(&("Vulkan".to_string(), "Test GPU".to_string())));
        assert!(report.contains(concat!("Oryxis: v", env!("CARGO_PKG_VERSION"))));
        assert!(report.contains("OS: "));
        assert!(report.contains("Renderer: Vulkan, Test GPU"));
        assert!(report.contains("Language: "));
        // The renderer line is omitted, not left dangling, while unknown.
        assert!(!environment_report(None).contains("Renderer:"));
    }
}
