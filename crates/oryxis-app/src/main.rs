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
mod boot;
mod color_picker;
mod connect_methods;
mod dispatch;
mod dispatch_ai;
mod dispatch_editor;
mod dispatch_keys;
mod dispatch_proxy_identity;
mod dispatch_cloud;
mod dispatch_plugins;
mod dispatch_port_forwards;
mod dispatch_session_group;
mod dispatch_settings;
mod dispatch_sftp;
mod dispatch_sftp_files;
mod dispatch_sftp_transfers;
mod dispatch_share;
mod dispatch_ssh;
mod dispatch_tabs;
mod dispatch_terminal;
mod fonts;
mod i18n;
mod mcp;
mod mcp_install;
mod messages;
mod os_icon;
// Cloud-provider plugin subsystem. Inert until the cloud dispatch
// path is rewired onto it in a later PR, the `allow` keeps the
// clippy `-D warnings` gate green while the infra (and its public
// re-exports) sit unused.
#[allow(dead_code, unused_imports)]
mod plugins;
mod root_view;
// Locates the AWS `session-manager-plugin` system binary. Pure
// path-finding, no SDK, relocated here from `oryxis-cloud-aws` when
// the AWS provider moved into its plugin subprocess.
mod session_manager_plugin;
mod hotkeys;
mod session_group_helpers;
mod sftp_helpers;
mod sftp_methods;
mod shortcuts;
mod ssh_config;
mod state;
mod subscription;
mod sync_runtime;
mod theme;
mod theme_import;
mod tray;
mod tray_ipc;
mod update;
mod util;
mod views;
mod widgets;

use iced::{window, Size};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 750.0;
const MIN_WIDTH: f32 = 800.0;
const MIN_HEIGHT: f32 = 500.0;

fn main() -> iced::Result {
    // rustls 0.23 requires a crypto provider to be installed before
    // any TLS connection, without it, the AWS SDK's HTTPS client
    // fails with a generic "dispatch failure". The workspace pins
    // `ring` as the rustls crypto, so install ring's default provider
    // here at process start. `install_default` returns Err if a
    // provider was already set (cheap re-entry from tests / repeated
    // calls), which we deliberately ignore.
    let _ = rustls::crypto::ring::default_provider().install_default();

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
    // process start. Default ("auto" / missing) sets nothing.
    //   - "opengl"   -> force wgpu's GL backend instead of Vulkan,
    //                   still hardware-accelerated; fixes most Vulkan-
    //                   on-Mesa corruption without the software cost.
    //   - "software" -> force iced's tiny-skia (CPU) renderer; the
    //                   terminal is a plain `canvas` widget so it renders
    //                   identically off the GPU.
    if let Ok(vault) = oryxis_vault::VaultStore::open_default()
        && let Ok(Some(mode)) = vault.get_setting("renderer_backend")
    {
        // SAFETY: still single-threaded here (tracing not yet
        // initialized, no threads spawned), so mutating the process
        // environment is sound under the Rust 2024 contract.
        match mode.as_str() {
            "opengl" => unsafe { std::env::set_var("WGPU_BACKEND", "gl") },
            "software" => unsafe { std::env::set_var("ICED_BACKEND", "tiny-skia") },
            _ => {}
        }
    }

    // CLI arg pickup, flags set when another Oryxis instance spawned
    // us via "Duplicate in New Window". Unknown flags are silently
    // ignored so future flags / OS double-click args don't crash boot.
    //   --connect <uuid>     : auto-open this saved connection
    //   --inherit-vault      : read the master password from stdin and
    //                          use it to unlock the vault on boot
    let mut args = std::env::args().skip(1);
    let mut inherit_vault = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
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
            _ => {}
        }
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

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("oryxis=debug,info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Oryxis");

    // Single-instance + multi-window IPC roles. The first process to
    // boot grabs the mutex and owns the tray icon ("primary"); every
    // subsequent process becomes a "child" that registers with the
    // primary via the filesystem-based tray_ipc registry and skips
    // tray installation entirely. The primary's tray menu aggregates
    // all known windows into a single "Hidden windows" section so
    // the user sees one tray ruling them all instead of N duplicates.
    let is_primary = !tray::another_instance_running();
    app::APP_IS_PRIMARY.store(is_primary, std::sync::atomic::Ordering::Relaxed);
    tray_ipc::init_runtime_dirs();

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

    // Load window icon from PNG
    let icon = load_icon();

    iced::application(app::Oryxis::boot, app::Oryxis::update, app::Oryxis::view)
        .title(app::Oryxis::title)
        .theme(app::Oryxis::theme)
        .subscription(app::Oryxis::subscription)
        .font(iced_fonts::LUCIDE_FONT_BYTES)
        // Codicon, used for window chrome glyphs (chrome-minimize/maximize/
        // restore/close) which match the native Windows title bar look that
        // VS Code uses.
        .font(iced_fonts::CODICON_FONT_BYTES)
        // Brand glyphs are bundled per-brand as SVGs in
        // `resources/icons/brand/`, no additional font needed. See
        // `os_icon::BRAND_ICONS`.
        // Noto Sans, the single bundled UI font across every platform (one
        // standard look instead of per-OS system fonts). Covers Latin,
        // Latin Extended, Cyrillic, Greek and Vietnamese in one family, so
        // English, Portuguese, Spanish, French, German, Italian, Russian,
        // Polish, Turkish, Indonesian, Vietnamese and Ukrainian all render
        // from the bundle with no system font dependency. Regular (400),
        // SemiBold (600) and Bold (700) share the "Noto Sans" typographic
        // family (name ID 16), so weight selection resolves to the right
        // file. Licensed under SIL OFL 1.1 (see resources/fonts/OFL.txt).
        .font(include_bytes!("../../../resources/fonts/NotoSans-Regular.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/NotoSans-SemiBold.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/NotoSans-Bold.ttf").as_slice())
        // Noto Sans Arabic, bundled so the already-shipped Arabic and
        // Persian languages render offline. cosmic-text falls back to it
        // per-codepoint for Arabic-script glyphs the Latin Noto lacks.
        // CJK (Chinese / Japanese / Korean) is the genuinely large script
        // set and is downloaded on demand instead (see mcp_install-style
        // font cache), not bundled here.
        .font(include_bytes!("../../../resources/fonts/NotoSansArabic-Regular.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/NotoSansArabic-SemiBold.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/NotoSansArabic-Bold.ttf").as_slice())
        // Tiny (~4 KB) CJK subset holding only the glyphs for the
        // language-picker names (한국어 / 中文 / 日本語). Bundling it means
        // those entries always render, even on a fresh install before the
        // full CJK font has been downloaded on demand, so the user can
        // always read and pick a CJK language. Distinct family
        // ("Oryxis Menu CJK") so it is a pure per-codepoint fallback and
        // never shadows the full Noto Sans / downloaded CJK faces.
        .font(include_bytes!("../../../resources/fonts/MenuCJK.ttf").as_slice())
        // SauceCodePro Nerd Font, default terminal font (Source Code
        // Pro patched with the full Nerd Font glyph set: Powerline,
        // Font Awesome, Devicons, Octicons, Codicons, Material). One
        // bundled mono font covers both regular text and the Private
        // Use Area symbol ranges, so prompts using Starship / Powerline
        // segments render correctly out of the box without a system
        // install. Additional mono fonts the user picks are resolved
        // by name from the system; for any system font that lacks the
        // PUA glyphs, the terminal widget's symbol_map falls back to
        // this family per-codepoint.
        .font(include_bytes!("../../../resources/fonts/SauceCodeProNerdFont-Regular.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/SauceCodeProNerdFont-Medium.ttf").as_slice())
        // Symbols Nerd Font: same PUA glyph set as SauceCodePro Nerd
        // but with no Latin coverage, purpose-built as a fallback-only
        // font. Loaded into the iced fontdb so cosmic-text picks it up
        // automatically for nerd glyph codepoints in proportional text
        // (Noto Sans / system fonts have no PUA coverage). Keeps prose
        // proportional while still rendering Powerline/Devicon/etc.
        // characters in chat messages, host labels, snippets, etc.
        .font(include_bytes!("../../../resources/fonts/SymbolsNerdFont-Regular.ttf").as_slice())
        // Default UI font is the bundled Noto Sans on every platform, so
        // the UI looks identical everywhere and never depends on a system
        // font being installed.
        .default_font(theme::SYSTEM_UI)
        .window(window::Settings {
            size: Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            min_size: Some(Size::new(MIN_WIDTH, MIN_HEIGHT)),
            icon,
            decorations: false, // native title bar off, our own chrome in the tab bar
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
                ..Default::default()
            },
            #[cfg(target_os = "linux")]
            platform_specific: window::settings::PlatformSpecific {
                // Sets the X11 WM_CLASS and the Wayland app_id. GNOME
                // (and other desktops) match a running window to its
                // installed `oryxis.desktop` entry by this id to resolve
                // the taskbar / dock icon. The id must equal the
                // .desktop basename ("oryxis"). Without it the id stays
                // empty and the window falls back to a generic icon.
                application_id: "oryxis".to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
        .antialiasing(true)
        .run()
}

fn load_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("../../../resources/logo_64.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}
