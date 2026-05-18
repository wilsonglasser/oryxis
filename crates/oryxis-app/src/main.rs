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
mod connect_methods;
mod dispatch;
mod dispatch_ai;
mod dispatch_editor;
mod dispatch_keys;
mod dispatch_proxy_identity;
mod dispatch_cloud;
mod dispatch_plugins;
mod dispatch_settings;
mod dispatch_sftp;
mod dispatch_sftp_files;
mod dispatch_sftp_transfers;
mod dispatch_share;
mod dispatch_ssh;
mod dispatch_tabs;
mod dispatch_terminal;
mod i18n;
mod jumplist;
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
mod sftp_helpers;
mod sftp_methods;
mod shortcuts;
mod ssh_config;
mod state;
mod subscription;
mod sync_runtime;
mod theme;
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
    app::APP_IS_PRIMARY.set(is_primary).ok();
    tray_ipc::init_runtime_dirs();

    // Register our AppUserModelID with the OS so taskbar grouping
    // tags our window with the right identity and any JumpList we
    // build later attaches to the right taskbar entry. Must happen
    // before the first window opens. No-op on non-Windows. Both
    // primary and children call it so their windows group under
    // the same taskbar entry.
    jumplist::set_app_id();

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
        // Inter (default UI font, matches Termius' UI aesthetic).
        .font(include_bytes!("../../../resources/fonts/Inter-Regular.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/Inter-SemiBold.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/Inter-Bold.ttf").as_slice())
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
        // Default UI font is the system font (Segoe UI on Windows, SF Pro
        // on macOS, bundled Inter on Linux), matches how Electron apps
        // like Termius render and keeps the UI feeling native per-OS.
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
