#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Prevent NVIDIA/AMD GPU drivers from treating this app as a game
// (disables automatic overlay activation on Windows)
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub static NvOptimusEnablement: u32 = 0;
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub static AmdPowerXpressRequestHighPerformance: u32 = 0;

mod ai;
mod app;
mod biometric;
mod boot;
mod jumplist;
mod color_picker;
mod chat_persist;
mod command_capture;
mod connect_methods;
mod deep_link;
mod dialog_warmup;
mod dispatch;
mod dispatch_ai;
mod dispatch_editor;
mod dispatch_keynav;
mod dispatch_keynav_modal;
mod dispatch_keynav_panel;
mod dispatch_keynav_sidebar;
mod dispatch_local;
mod dispatch_login_script;
mod dispatch_password_suggest;
mod dispatch_keys;
mod dispatch_known_hosts;
mod dispatch_tray;
mod dispatch_vault;
mod dispatch_snippets;
mod dispatch_navigation;
mod dispatch_onboarding;
mod dispatch_folder_sync;
mod dispatch_git_sync;
mod dispatch_global;
mod dispatch_history;
mod dispatch_player;
mod gif_export;
mod dispatch_command_history;
mod dispatch_monitor;
mod dispatch_monitor_dash;
mod dispatch_net_tools;
mod dispatch_tmux;
mod dispatch_mcp;
mod dispatch_sync;
mod dispatch_proxy_identity;
mod dispatch_cloud;
mod dispatch_plugins;
mod dispatch_port_forwards;
mod dispatch_session_group;
mod dispatch_settings;
mod dispatch_sftp;
mod dispatch_sftp_console;
mod dispatch_sidebar_files;
mod dispatch_sftp_archive;
mod dispatch_sftp_files;
mod dispatch_sftp_sync;
mod dispatch_sftp_transfers;
mod dispatch_share;
mod dispatch_remote_desktop;
mod dispatch_serial;
mod dispatch_ssh;
mod dispatch_tabs;
mod dispatch_telnet;
mod dispatch_terminal;
mod dispatch_update;
mod dispatch_webdav_sync;
mod dispatch_zmodem;
mod font_family;
mod fonts;
#[cfg(feature = "harness")]
mod harness;
mod highlight_rules;
mod i18n;
mod key_encode;
mod keynav;
mod logging;
mod stall_watchdog;
mod agent_server;
mod dispatch_agent;
mod net_mirror;
// The network tools panel's probes (DNS, ping, port, HTTP/TLS, WHOIS,
// DNSBL). Hidden behind the `network_tools_enabled` setting, off by
// default, like every other optional surface.
mod net_tools;
mod mcp;
mod mcp_install;
mod messages;
mod mime_types;
mod os_icon;
// MSIX / Microsoft Store container probe. Gates the self-updater and
// the explicit AppUserModelID, both of which are wrong inside a package.
mod packaged;
// Split-anchor geometry for dropping a dragged tab into a pane grid.
mod pane_drop;
// Answering the engine's command-proxy approval question on dials with
// no user behind them.
mod proxy_consent;
// Cloud-provider plugin subsystem. Inert until the cloud dispatch
// path is rewired onto it in a later PR, the `allow` keeps the
// clippy `-D warnings` gate green while the infra (and its public
// re-exports) sit unused.
#[allow(dead_code, unused_imports)]
mod plugins;
mod renderer_probe;
mod root_view;
// Locates the AWS `session-manager-plugin` system binary. Pure
// path-finding, no SDK, relocated here from `oryxis-cloud-aws` when
// the AWS provider moved into its plugin subprocess.
mod session_manager_plugin;
mod hotkeys;
mod ansi_render;
mod session_group_helpers;
mod palette;
mod paste_guard;
mod settings_index;
mod remote_desktop;
mod session_redact;
mod sftp_helpers;
mod sftp_methods;
mod drag_out;
mod install_presets;
mod local_files;
mod shell_integration;
mod shortcuts;
mod sidebar_regions;
mod smart_tabs;
mod importers;
mod ssh_config;
mod ssh_reuse;
mod state;
mod subscription;
mod sync_runtime;
mod tab_conn_state;
mod tab_cycle;
mod terminal_appearance;
mod theme;
mod monitor;
mod theme_export;
mod theme_import;
mod tmux;
mod tray;
mod tray_ipc;
mod update;
mod util;
mod views;
mod widgets;
mod wol;

use iced::{window, Size};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 750.0;
const MIN_WIDTH: f32 = 800.0;
const MIN_HEIGHT: f32 = 500.0;

fn main() -> iced::Result {
    // Headless E2E harness argument pickup (feature `harness`). Must
    // run before anything else touches the vault or the environment:
    // when a harness mode is requested it redirects $HOME to a sandbox
    // directory, so a harness run can never read or write the real
    // ~/.oryxis. Still single-threaded here, so the env mutation is
    // sound under the Rust 2024 contract.
    #[cfg(feature = "harness")]
    let harness_options = harness::options_from_args();
    #[cfg(feature = "harness")]
    let harness_active = harness_options.is_some();
    #[cfg(not(feature = "harness"))]
    let harness_active = false;
    // Published for the parts of boot that must not reach the network
    // under an emulated run (see `app::HARNESS_ACTIVE`). Set here, next
    // to the $HOME redirect, because both answer the same question and
    // a second place to ask it would eventually disagree.
    let _ = app::HARNESS_ACTIVE.set(harness_active);
    // A binary built WITHOUT the harness feature must refuse `--harness-*`
    // instead of silently falling through to the windowed app: that
    // fallthrough boots against the caller's REAL $HOME (an agent driving
    // QA right after a featureless rebuild would spawn stray windowed
    // instances on the real vault, which actually happened).
    #[cfg(not(feature = "harness"))]
    if std::env::args().any(|a| a.starts_with("--harness")) {
        eprintln!(
            "error: this binary was built without the `harness` feature; \
             rebuild with `cargo build -p oryxis-app --features harness`"
        );
        std::process::exit(2);
    }

    // rustls 0.23 requires a crypto provider to be installed before any
    // TLS connection, without it the AWS SDK's HTTPS client fails with a
    // generic "dispatch failure" and reqwest panics with "No provider
    // set". See the helper for why the tree carries exactly one backend.
    util::ensure_crypto_provider();

    // Sweep the `.old.exe` left behind by a Windows nightly self-update
    // (no-op elsewhere). Done before anything else touches the binary.
    update::sweep_stale_binary();

    // Renderer escape hatch. Some GPU/driver stacks (seen on GNOME +
    // Mesa) corrupt the wgpu surface, bleeding other windows' pixels
    // into our chrome while a terminal session forces frequent redraws.
    // The corruption lives below iced (swapchain/present in the driver),
    // so we can't repaint our way out; instead we let the user pick a
    // different render path. Both knobs are read once while iced builds
    // its compositor, so they must be set before `iced::application(..)
    // .run()`. The setting lives in the vault's `settings` table, which
    // reads without the master password, so we resolve it here at
    // process start.
    //   - "opengl"   -> force wgpu's GL backend instead of Vulkan,
    //                   still hardware-accelerated; fixes most Vulkan-
    //                   on-Mesa corruption without the software cost.
    //   - "software" -> force iced's tiny-skia (CPU) renderer; the
    //                   terminal is a plain `canvas` widget so it renders
    //                   identically off the GPU.
    //   - "auto" / missing -> on Windows, probe the Vulkan/DX12 adapters
    //                   first and redirect to the software renderer when
    //                   the default pick has no healthy hardware path
    //                   (WARP-only setups, Haswell iGPUs whose EOL
    //                   drivers present undecorated windows offset on
    //                   every GPU backend). See renderer_probe.
    let mut renderer_mode: Option<String> = None;
    // Window geometry persisted by `persist_window_geometry` on every
    // exit path. Same unlocked settings read as the renderer knob: the
    // values must be known before `iced::application(..).run()` builds
    // the window, so they're resolved here rather than in boot().
    let mut window_size = Size::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    let mut window_position = window::Position::Default;
    let mut window_maximized = false;
    let mut window_fullscreen = false;
    // Read straight off the argv here rather than in the flag loop far
    // below: the debug sink has to be armed before the vault read, so
    // the whole boot is captured.
    let force_debug_log = std::env::args().skip(1).any(|a| a == "--debug-log");
    if let Ok(vault) = oryxis_vault::VaultStore::open_default() {
        renderer_mode = vault.get_setting("renderer_backend").ok().flatten();
        if let (Some(w), Some(h)) = (
            vault.get_setting("window_width").ok().flatten(),
            vault.get_setting("window_height").ok().flatten(),
        ) && let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>())
            && w.is_finite()
            && h.is_finite()
        {
            // Clamp so a hand-edited vault (or a corrupt row) can't
            // restore an unusably small window; oversized values are
            // capped by the window manager against the actual desktop.
            window_size = Size::new(
                w.clamp(MIN_WIDTH, 16384.0),
                h.clamp(MIN_HEIGHT, 16384.0),
            );
        }
        // Outer position in logical desktop coordinates. Legitimately
        // negative on monitors left of / above the primary, which is
        // exactly how the "same monitor" part round-trips; a stale
        // position (that monitor unplugged since) is rescued after boot
        // by `WindowEnsureOnScreen`. Absent rows (fresh install, or a
        // Wayland session where positions don't exist) fall through to
        // the platform default placement.
        if let (Some(x), Some(y)) = (
            vault.get_setting("window_pos_x").ok().flatten(),
            vault.get_setting("window_pos_y").ok().flatten(),
        ) && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
            && x.is_finite()
            && y.is_finite()
        {
            window_position =
                window::Position::Specific(iced::Point::new(x, y));
        }
        window_maximized = vault
            .get_setting("window_maximized")
            .ok()
            .flatten()
            .as_deref()
            == Some("true");
        window_fullscreen = vault
            .get_setting("window_fullscreen")
            .ok()
            .flatten()
            .as_deref()
            == Some("true");
        // Terminal background opacity. Read here, with the geometry and
        // the renderer knob, because a transparent surface is decided
        // when the window is created and can't be turned on afterwards
        // (winit exposes `set_transparent`, but it is a no-op on most
        // platforms and the fork is not ours to change). A window born
        // opaque therefore stays opaque for the whole run, which is what
        // the restart prompt in Settings covers. `boot` hydrates the
        // same row into `prefs` for the UI.
        if let Some(v) = vault.get_setting("terminal_opacity").ok().flatten()
            && let Ok(percent) = v.parse::<u8>()
        {
            theme::set_terminal_opacity(percent);
        }
        // Debug logging (Settings > Advanced, or `--debug-log`). Armed
        // before the tracing subscriber below is built so the earliest
        // boot lines land in the file too; same unlocked settings read
        // as the renderer knob. The flag wins over the stored setting
        // and pins the sink on for the whole process (see
        // `logging::force_enable`), which is what makes a diagnostic
        // session survive the user toggling Settings mid-run.
        let armed = if force_debug_log {
            logging::force_enable().map(|_| ())
        } else if vault.get_setting("debug_logging").ok().flatten().as_deref() == Some("true") {
            logging::enable().map(|_| ())
        } else {
            Ok(())
        };
        if let Err(e) = armed {
            eprintln!("oryxis: failed to enable debug logging: {e}");
        }
    }
    // SAFETY (both set_var sites): still single-threaded here (tracing
    // not yet initialized, no threads spawned), so mutating the process
    // environment is sound under the Rust 2024 contract.
    let mut renderer_probe_note: Option<String> = None;
    match renderer_mode.as_deref() {
        Some("opengl") => {
            // "OpenGL (GPU)" promises hardware acceleration. When the
            // GL stack can only offer a software rasterizer (llvmpipe
            // after a WSLg vGPU death, SwiftShader), wgpu-GL-on-software
            // is the one combination that MISRENDERS (missing background
            // quads, clip leaks), so honor the option's intent instead
            // of its letter and fall back to the correct software
            // renderer. Cross-platform: this stack class lives on
            // Linux/WSL, not just Windows.
            if let Some(redirect) = renderer_probe::opengl_backend_override() {
                unsafe { std::env::set_var(redirect.env_key, redirect.env_value) };
                renderer_probe::mark_redirected();
                renderer_probe_note = Some(redirect.reason);
            } else {
                unsafe { std::env::set_var("WGPU_BACKEND", "gl") }
            }
        }
        Some("software") => unsafe { std::env::set_var("ICED_BACKEND", "tiny-skia") },
        _ => {
            // The probe is Windows-only: that's where the broken-DX12
            // class of hardware lives, and where GL (WGL) is a reliable
            // hardware fallback. It respects pre-set env overrides.
            if cfg!(windows)
                && let Some(redirect) = renderer_probe::auto_backend_override()
            {
                unsafe { std::env::set_var(redirect.env_key, redirect.env_value) };
                renderer_probe::mark_redirected();
                renderer_probe_note = Some(redirect.reason);
            }
        }
    }

    // Game overlays and capture tools (MangoHud, vkBasalt, OBS's
    // vkcapture) ship as implicit Vulkan layers: the loader injects them
    // into EVERY Vulkan process on the machine, an SSH client included,
    // where they hook swapchain presentation for a HUD nobody asked for
    // and become a real stall/crash surface (#104's environment is a
    // gaming distro). Opt this process out, by name, on two levels:
    // VK_LOADER_LAYERS_DISABLE (loader >= 1.3.234) blocks them at the
    // loader, and the layers' own documented disable_environment
    // switches cover older loaders (MangoHud and vkBasalt have stable
    // ones; OBS relies on the loader glob). Deliberately NOT a blanket
    // `~implicit~`: VK_LAYER_MESA_device_select and VK_LAYER_NV_optimus
    // are load-bearing for GPU selection on hybrid-graphics machines.
    // A user who set VK_LOADER_LAYERS_DISABLE themselves keeps their
    // policy untouched, and the loader's VK_LOADER_LAYERS_ENABLE still
    // force-enables a layer over our list, so both escape hatches
    // survive. Same single-threaded SAFETY contract as the renderer
    // set_var block above (nothing has spawned a thread yet).
    let vulkan_layer_note: Option<&str> =
        if std::env::var_os("VK_LOADER_LAYERS_DISABLE").is_some() {
            Some("VK_LOADER_LAYERS_DISABLE preset by the user; overlay-layer opt-out left alone")
        } else {
            unsafe {
                std::env::set_var(
                    "VK_LOADER_LAYERS_DISABLE",
                    "VK_LAYER_MANGOHUD_overlay*,VK_LAYER_VKBASALT*,VK_LAYER_OBS_vkcapture*",
                );
            }
            for var in ["DISABLE_MANGOHUD", "DISABLE_VKBASALT"] {
                if std::env::var_os(var).is_none() {
                    unsafe { std::env::set_var(var, "1") };
                }
            }
            Some("game-overlay Vulkan layers opted out for this process (MangoHud, vkBasalt, OBS vkcapture)")
        };

    // Self-heal a renderer crash on GPU/driver stacks that can't satisfy
    // iced_wgpu's shader capabilities (VMs, old drivers, software Vulkan):
    // catch the wgpu panic and relaunch with a safer backend. Installed after
    // the vault-driven backend env above so it escalates from whatever backend
    // is currently active. The debug-log hook goes in first so every panic is
    // stamped into the log file before any self-heal relaunch happens.
    install_panic_log_hook();
    install_renderer_fallback_hook();

    // CLI arg pickup, flags set when another Oryxis instance spawned
    // us via "Duplicate in New Window". Unknown flags are silently
    // ignored so future flags / OS double-click args don't crash boot.
    //   --connect <uuid>     : auto-open this saved connection
    //   --inherit-vault      : read the master password from stdin and
    //                          use it to unlock the vault on boot
    //   --relaunch           : this process replaces a prior one (e.g. a
    //                          renderer-change restart); wait briefly for
    //                          the old single-instance mutex to release so
    //                          we boot as primary, not as a tray-less child
    //   --debug-log          : force the debug log file on for this whole
    //                          run, overriding (and un-overridable by) the
    //                          Settings toggle. Read far above, before the
    //                          vault, so boot lines are captured too.
    let mut args = std::env::args().skip(1);
    let mut inherit_vault = false;
    let mut relaunching = false;
    let mut deep_link_url: Option<String> = None;
    let mut connect_target: Option<String> = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            // OS-registered `oryxis://` and `ssh://` scheme launches.
            // Captured raw here; validated and possibly forwarded to a
            // running instance below.
            url if url.starts_with("oryxis://") || url.starts_with("ssh://") => {
                deep_link_url = Some(flag);
            }
            "--connect" => {
                if let Some(value) = args.next()
                    && let Ok(uuid) = uuid::Uuid::parse_str(&value)
                {
                    let _ = app::AUTO_CONNECT.set(uuid);
                }
            }
            "--inherit-vault" => {
                inherit_vault = true;
            }
            "--relaunch" => {
                relaunching = true;
            }
            // `oryxis user@host[:port]`: the CLI quick-connect form.
            // The `@` is REQUIRED, which is both the documented shape
            // and what keeps a file manager's `%u` (the desktop entry
            // passes one) from turning a double-clicked file name into
            // a connect attempt. Anything starting with `-` is a flag
            // (or a flag's value, already consumed by its own arm), and
            // only the first positional counts.
            positional
                if connect_target.is_none()
                    && !positional.starts_with('-')
                    && positional.contains('@')
                    && oryxis_core::ssh_target::SshTarget::parse(positional).is_some() =>
            {
                connect_target = Some(flag);
            }
            _ => {}
        }
    }
    // Deep-link launch. When another Oryxis is already running, this
    // process is only a courier: drop the URL in the cross-process
    // inbox (tray_ipc), wait for a window to claim it, and exit
    // without ever booting iced. If nobody claims it (the instance
    // died mid-race, or its build predates the inbox), reclaim the
    // file and boot a window ourselves with the link stashed, so a
    // click never lands nowhere. The harness skips only the FORWARD:
    // its $HOME is sandboxed but the runtime dirs are not, so a test
    // must never hand its URL to the developer's real window, while
    // still exercising the cold-start route headlessly.
    if let Some(url) = deep_link_url {
        if deep_link::parse(&url).is_none() {
            // Not ours / malformed: boot normally, exactly like a
            // plain double-click. Tracing isn't up yet.
            eprintln!("oryxis: ignoring malformed deep link");
        } else {
            let mut forwarded = false;
            if !harness_active {
                tray_ipc::init_runtime_dirs();
            }
            if !harness_active
                && tray_ipc::any_live_instance()
                && let Some(pending) = tray_ipc::write_deeplink(&url)
            {
                let mut waited = 0;
                while pending.exists() && waited < 2000 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    waited += 100;
                }
                if pending.exists() {
                    let _ = std::fs::remove_file(&pending);
                } else {
                    forwarded = true;
                }
            }
            if forwarded {
                return Ok(());
            }
            let _ = app::PENDING_DEEP_LINK.set(url);
        }
    }
    // Same courier dance for a CLI target, on its own inbox so the
    // claiming window knows this one came from the user's shell (and
    // therefore dials) rather than from a clicked link.
    if let Some(target) = connect_target {
        let mut forwarded = false;
        // The harness skips only the FORWARD, not the stash: its $HOME
        // is sandboxed but the runtime dirs are not, so a test must
        // never hand its target to the developer's real window, while
        // still exercising the cold-start route headlessly.
        if !harness_active {
            tray_ipc::init_runtime_dirs();
        }
        if !harness_active
            && tray_ipc::any_live_instance()
            && let Some(pending) = tray_ipc::write_connect(&target)
        {
            let mut waited = 0;
            while pending.exists() && waited < 2000 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                waited += 100;
            }
            if pending.exists() {
                let _ = std::fs::remove_file(&pending);
            } else {
                forwarded = true;
            }
        }
        if forwarded {
            return Ok(());
        }
        let _ = app::PENDING_CONNECT_TARGET.set(target);
    }
    if inherit_vault {
        // Parent writes a single line to our stdin and closes the pipe;
        // anything past that line is ignored.
        use std::io::BufRead as _;
        let stdin = std::io::stdin();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_ok() {
            let pw = line.trim_end_matches(['\n', '\r']).to_string();
            if !pw.is_empty() {
                let _ = app::AUTO_PASSWORD.set(pw);
            }
        }
    }

    // Stdout is a PTY when the app is launched from a terminal, and a
    // synchronous write to a PTY that stopped draining (Ctrl+S, a
    // paused scrollback, a suspended terminal) BLOCKS the writing
    // thread once the kernel buffer fills. tracing events are written
    // on the emitting thread, which for most of our events is the UI
    // thread, so a wedged stdout would freeze the whole app. Route the
    // stdout layer through tracing-appender's worker thread instead: a
    // blocked stdout at worst drops log lines (lossy by default),
    // never the UI. The guard flushes buffered lines when main returns.
    let (stdout_writer, _stdout_log_guard) =
        tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::registry()
        // `arboard` logs a WARN on every clipboard op when the Wayland
        // data-control protocol is unavailable (common under WSL / some
        // compositors) and it falls back to X11, which works fine. Quiet
        // that one target so copy-on-select doesn't spam the log on every
        // click; everything else stays at info.
        .with(tracing_subscriber::EnvFilter::new("oryxis=debug,info,arboard=error"))
        // In harness mode stdout belongs to the driving protocol (the
        // REPL's `== ` lines, MCP's JSON-RPC messages), so logs go to
        // stderr there; the normal app keeps stdout.
        .with(
            (!harness_active)
                .then(|| tracing_subscriber::fmt::layer().with_writer(stdout_writer)),
        )
        .with(harness_active.then(|| {
            tracing_subscriber::fmt::layer().with_writer(std::io::stderr)
        }))
        // Second sink for the Settings > Advanced debug-log file. Always
        // installed; the writer discards everything while the feature is
        // off, so the toggle works at runtime without rebuilding the
        // subscriber. No ANSI, the file is read in editors, not a tty.
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(logging::DebugFileWriter),
        )
        .init();

    tracing::info!("Starting Oryxis");
    if let Some(note) = renderer_probe_note {
        // Deferred from the probe above, which runs before the
        // subscriber exists.
        tracing::info!("renderer auto-probe: {note}");
    }
    if let Some(note) = vulkan_layer_note {
        // Deferred like the probe note; lands in the debug log next to
        // the environment header, so a report tells whether an overlay
        // could have been injected at all.
        tracing::info!("vulkan layers: {note}");
    }

    // Single-instance + multi-window IPC roles. The first process to
    // boot grabs the mutex and owns the tray icon ("primary"); every
    // subsequent process becomes a "child" that registers with the
    // primary via the filesystem-based tray_ipc registry and skips
    // tray installation entirely. The primary's tray menu aggregates
    // all known windows into a single "Hidden windows" section so
    // the user sees one tray ruling them all instead of N duplicates.
    // On a relaunch the old process may not have released the
    // single-instance mutex yet, which would demote us to a tray-less
    // child. Retry for up to ~2 s so the intended in-place restart
    // comes back as primary. No-op where there is no mutex (Linux/macOS
    // report not-running immediately).
    let is_primary = if harness_active {
        // Headless harness: no tray, no IPC registry, no single-instance
        // coordination. The emulator process is self-contained and must
        // never demote a real running Oryxis to a child (or vice versa).
        true
    } else if relaunching {
        let mut primary = !tray::another_instance_running();
        let mut waited = 0;
        while !primary && waited < 2000 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            waited += 100;
            primary = !tray::another_instance_running();
        }
        primary
    } else {
        !tray::another_instance_running()
    };
    app::APP_IS_PRIMARY.store(is_primary, std::sync::atomic::Ordering::Relaxed);

    if !harness_active {
        tray_ipc::init_runtime_dirs();

        // Every window (primary included, every platform) registers a
        // PID file: it's how a later `oryxis://` launcher process
        // knows a live instance exists to forward to. Before deep
        // links only Windows children registered (the tray menu was
        // the file's only reader).
        tray_ipc::Child::register("Oryxis");

        if is_primary {
            // Install the Windows system tray icon. No-op on macOS/Linux
            // until those platforms get their own backends. Failure here
            // is logged but non-fatal: the app still runs without a tray.
            if let Err(e) = tray::install() {
                tracing::warn!("tray icon registration failed: {e}");
            }
        } else {
            // Child: announce ourselves to the primary's registry.
            // Title is the default app title; per-window state updates
            // refine it later via tray_ipc::Child::write_state.
            tray_ipc::Child::register("Oryxis");
            tracing::info!("running as tray IPC child (primary already up)");
        }
    }

    // Warm the OS file dialog up now, so the first Download / Upload /
    // Import does not also pay for loading the shell's COM server. Off
    // the main thread and after the `set_var` blocks above, whose
    // soundness argument is that the process is still single-threaded.
    // Skipped under the harness: it opens no dialogs, and a headless run
    // has no business touching the shell.
    if !harness_active {
        dialog_warmup::spawn();
    }

    // Load window icon from PNG
    let icon = load_icon();

    // A translucent terminal needs a surface that composites with the
    // desktop, and the alpha has to reach it: the clear colour is the
    // bottom-most layer of every frame, so an opaque one would sit under
    // the terminal and turn the effect into "the app's own background
    // showing through". Only claimed when the user asked for it, so the
    // default install keeps the exact surface it has today.
    let transparent_window = theme::terminal_opacity() < 100;
    theme::set_window_transparent(transparent_window);
    let mut application =
        iced::application(app::Oryxis::boot, app::Oryxis::update, app::Oryxis::view)
            .title(app::Oryxis::title)
            .theme(app::Oryxis::theme)
            .style(|_state, theme| {
                let mut style = iced::theme::default(theme);
                if theme::window_transparent() {
                    style.background_color = iced::Color::TRANSPARENT;
                }
                style
            })
            .subscription(app::Oryxis::subscription);
    // Every bundled font, from the Lucide icon glyphs to the Nerd Font
    // terminal faces. The list (and the rationale for each entry) lives
    // in `fonts::BUNDLED_FONTS` so the headless harness can load the
    // exact same set into its windowless renderer.
    for font in fonts::BUNDLED_FONTS {
        application = application.font(*font);
    }
    let application = application
        // Default UI font is the bundled Noto Sans on every platform, so
        // the UI looks identical everywhere and never depends on a system
        // font being installed.
        .default_font(theme::SYSTEM_UI)
        .window(window::Settings {
            size: window_size,
            // The saved outer position also selects the monitor: winit
            // maximizes / fullscreens onto the monitor containing the
            // window's position, so a maximized restore lands on the
            // same display it was closed on.
            position: window_position,
            // Reopen maximized / fullscreen when the user left it that
            // way; `size` still carries the floating size underneath so
            // un-maximizing lands where the user last had the window.
            maximized: window_maximized,
            fullscreen: window_fullscreen,
            min_size: Some(Size::new(MIN_WIDTH, MIN_HEIGHT)),
            icon,
            decorations: false, // native title bar off, our own chrome in the tab bar
            // Only when a translucent terminal was asked for: a surface
            // that composites with the desktop is not free everywhere
            // (X11 needs a running compositor; DX12 usually offers no
            // pre-multiplied alpha mode at all, where this degrades to a
            // plain opaque window rather than to a broken one).
            transparent: transparent_window,
            // Take ownership of every close verb. With iced's default
            // (`true`) the winit shell closes the window itself on
            // `CloseRequested` and never forwards the event, so the
            // `window::close_requests()` subscription never fires and
            // Alt+F4 / taskbar Close bypass `handle_window_close`
            // entirely: close-to-tray is skipped, and so are the
            // session-log flush and the window-geometry save. Set to
            // `false` so every close path (our chrome X, Alt+F4, the
            // taskbar) lands on the same `Message::Tabs(TabsMessage::WindowClose)`
            // handler, which decides between hide-to-tray and a real
            // `window::close`. Not Windows-gated: the flush + geometry
            // recovery matter on every platform.
            exit_on_close_request: false,
            #[cfg(target_os = "windows")]
            platform_specific: window::settings::platform::PlatformSpecific {
                // Win11+ rounds corners only when DWM has a frame to
                // composite. Undecorated windows lose that by default,
                // so opt back in via the DWM corner-preference API and
                // re-enable the drop shadow that brings the rounded
                // mask along.
                corner_preference:
                    window::settings::platform::CornerPreference::Round,
                undecorated_shadow: true,
                // ICON_BIG. The generic `icon` above only ever becomes
                // ICON_SMALL (the title-bar size), and the classic
                // Alt+Tab switcher asks the window for ICON_BIG, so
                // without this it draws the default executable glyph
                // (issue #182). The modern switcher and the taskbar
                // resolve the icon through the shortcut instead, which
                // is why they never showed the gap.
                taskbar_icon: load_taskbar_icon(),
                ..Default::default()
            },
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                // Sets the X11 WM_CLASS and the Wayland app_id. GNOME
                // (and other desktops) match a running window to its
                // installed `.desktop` entry by this id to resolve the
                // taskbar / dock icon. The id must equal the .desktop
                // basename. For the .deb / AppImage that is "oryxis"; inside
                // a Flatpak the runtime exports FLATPAK_ID (the app id, e.g.
                // "app.oryxis.Oryxis"), which is also the installed .desktop
                // basename there, so honor it when present.
                application_id: std::env::var("FLATPAK_ID")
                    .unwrap_or_else(|_| "oryxis".to_string()),
                ..Default::default()
            },
            ..Default::default()
        })
        .antialiasing(true);

    // Headless E2E harness (feature `harness`): hand the fully
    // configured application to the emulator-backed driver instead of
    // opening a window. See `harness.rs` / docs/HARNESS.md.
    #[cfg(feature = "harness")]
    if let Some(options) = harness_options {
        return harness::run(application, options);
    }

    application.run()
}

fn load_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("../../../resources/logo_64.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

/// The Windows ICON_BIG. Bigger than the window icon on purpose: the
/// consumers of ICON_BIG (classic Alt+Tab, third-party switchers) draw
/// at SM_CXICON and beyond, and 256 is the documented ceiling for the
/// field, so the OS always scales DOWN from here.
#[cfg(target_os = "windows")]
fn load_taskbar_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("../../../resources/logo_256.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Decide how to recover from a panic, given the retry count, the panic text
/// (message + source file), and which backend is currently forced. Returns the
/// `(renderer_backend setting, env key, env value)` to relaunch with, or `None`
/// to let the process crash. Pure so it can be unit-tested without a GPU.
///
/// Ladder: auto -> GL -> software. `retry >= 2` and "already on software" both
/// stop, so a genuinely unrenderable setup can't loop forever.
fn renderer_fallback_action(
    retry: u32,
    panic_text: &str,
    on_gl: bool,
    on_software: bool,
) -> Option<(&'static str, &'static str, &'static str)> {
    // Hard loop guard.
    if retry >= 2 {
        return None;
    }
    // Only act on renderer-related panics.
    const RENDERER_MARKERS: &[&str] = &[
        "wgpu",
        "naga",
        "create_shader",
        "Validation Error",
        "surface",
        "Surface",
        "adapter",
        "Backends",
        "iced_wgpu",
    ];
    if !RENDERER_MARKERS.iter().any(|k| panic_text.contains(k)) {
        return None;
    }
    if on_software {
        None // already on the software renderer, nothing safer to try
    } else if on_gl {
        Some(("software", "ICED_BACKEND", "tiny-skia"))
    } else {
        Some(("opengl", "WGPU_BACKEND", "gl"))
    }
}

/// Install a panic hook that self-heals a renderer crash by relaunching with a
/// safer wgpu backend. Some GPU/driver stacks (VMs, old drivers, software
/// Vulkan) expose an adapter whose shader capabilities don't match what
/// iced_wgpu requires (e.g. `SHADER_FLOAT16_IN_FLOAT32`), so wgpu panics during
/// shader validation *after* the device is created, which is past the point
/// where iced would fall back to its tiny-skia software renderer. We catch that
/// panic, escalate the renderer (see [`renderer_fallback_action`]), persist the
/// choice in the `renderer_backend` setting so the next cold launch skips the
/// crash, and re-exec.
/// Chain a hook that stamps every panic into the debug-log file (when
/// the Settings > Advanced toggle is on). See [`logging::log_panic`]
/// for why stderr alone isn't enough on Windows GUI builds. Both this
/// and the renderer fallback hook call the previous hook, so they
/// compose in installation order.
fn install_panic_log_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logging::log_panic(info);
        prev(info);
    }));
}

fn install_renderer_fallback_hook() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Surface the panic first (stderr / logs), then try to heal.
        prev(info);

        let retry: u32 = std::env::var("ORYXIS_RENDER_RETRY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let file = info
            .location()
            .map(|l| l.file().to_string())
            .unwrap_or_default();
        let panic_text = format!("{msg} {file}");
        let on_software = std::env::var("ICED_BACKEND").ok().as_deref() == Some("tiny-skia");
        let on_gl = std::env::var("WGPU_BACKEND").ok().as_deref() == Some("gl");

        let Some((setting, env_key, env_val)) =
            renderer_fallback_action(retry, &panic_text, on_gl, on_software)
        else {
            return;
        };

        // Persist so future cold launches skip the crash, then relaunch with
        // the safer backend forced directly (works even if the write failed).
        if let Ok(vault) = oryxis_vault::VaultStore::open_default() {
            let _ = vault.set_setting("renderer_backend", setting);
        }
        let _ = std::process::Command::new(&exe)
            .args(&args)
            .env(env_key, env_val)
            .env("ORYXIS_RENDER_RETRY", (retry + 1).to_string())
            .spawn();
    }));
}

#[cfg(test)]
mod renderer_fallback_tests {
    use super::renderer_fallback_action;

    #[test]
    fn ignores_non_renderer_panics() {
        // A normal panic (no renderer marker) must not relaunch anything.
        assert_eq!(
            renderer_fallback_action(0, "index out of bounds: the len is 3 at app.rs", false, false),
            None
        );
    }

    #[test]
    fn auto_escalates_to_gl() {
        // The real-world float16 crash text, on the default (auto) backend.
        let text = "wgpu error: Validation Error in Device::create_shader_module wgpu_core.rs";
        assert_eq!(
            renderer_fallback_action(0, text, false, false),
            Some(("opengl", "WGPU_BACKEND", "gl"))
        );
    }

    #[test]
    fn gl_escalates_to_software() {
        assert_eq!(
            renderer_fallback_action(1, "wgpu_core surface error", true, false),
            Some(("software", "ICED_BACKEND", "tiny-skia"))
        );
    }

    #[test]
    fn software_is_the_end_of_the_ladder() {
        assert_eq!(
            renderer_fallback_action(1, "wgpu Validation Error", false, true),
            None
        );
    }

    #[test]
    fn retry_cap_stops_the_loop() {
        assert_eq!(
            renderer_fallback_action(2, "wgpu Validation Error", false, false),
            None
        );
    }

    #[test]
    fn detects_each_renderer_marker() {
        for marker in [
            "wgpu",
            "naga",
            "create_shader",
            "Validation Error",
            "iced_wgpu",
            "Surface",
            "adapter",
            "Backends",
        ] {
            let text = format!("thread 'main' panicked: {marker}");
            assert!(
                renderer_fallback_action(0, &text, false, false).is_some(),
                "marker {marker:?} should be detected as a renderer panic"
            );
        }
    }
}
