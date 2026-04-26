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
mod dispatch_settings;
mod dispatch_sftp;
mod dispatch_sftp_files;
mod dispatch_sftp_transfers;
mod dispatch_share;
mod dispatch_ssh;
mod dispatch_tabs;
mod dispatch_terminal;
mod i18n;
mod mcp;
mod messages;
mod os_icon;
mod root_view;
mod sftp_helpers;
mod sftp_methods;
mod ssh_config;
mod state;
mod subscription;
mod theme;
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
    // CLI arg pickup — flags set when another Oryxis instance spawned
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

    // Held for the duration of main() — drop flushes pending events.
    let _sentry_guard = init_sentry();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("oryxis=debug,info"))
        .with(tracing_subscriber::fmt::layer())
        .with(sentry_tracing::layer())
        .init();

    tracing::info!("Starting Oryxis");

    // Load window icon from PNG
    let icon = load_icon();

    iced::application(app::Oryxis::boot, app::Oryxis::update, app::Oryxis::view)
        .title(app::Oryxis::title)
        .theme(app::Oryxis::theme)
        .subscription(app::Oryxis::subscription)
        .font(iced_fonts::LUCIDE_FONT_BYTES)
        // Codicon — used for window chrome glyphs (chrome-minimize/maximize/
        // restore/close) which match the native Windows title bar look that
        // VS Code uses.
        .font(iced_fonts::CODICON_FONT_BYTES)
        // Simple Icons — official distro/OS brand marks (Ubuntu, Debian,
        // Arch, Fedora, CentOS, Red Hat, Alpine, Apple, FreeBSD, Rocky,
        // Alma, openSUSE, NixOS…) rendered via codepoint lookup. Replaces
        // the Nerd-Fonts-patched Devicon which had broken glyph mappings.
        .font(include_bytes!("../../../resources/fonts/SimpleIcons.ttf").as_slice())
        // Inter — default UI font (matches Termius' UI aesthetic).
        .font(include_bytes!("../../../resources/fonts/Inter-Regular.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/Inter-SemiBold.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/Inter-Bold.ttf").as_slice())
        // Source Code Pro — default terminal font; additional mono fonts are
        // resolved by name from the system when the user picks them.
        .font(include_bytes!("../../../resources/fonts/SourceCodePro-Regular.ttf").as_slice())
        .font(include_bytes!("../../../resources/fonts/SourceCodePro-Medium.ttf").as_slice())
        // Default UI font is the system font (Segoe UI on Windows, SF Pro
        // on macOS, bundled Inter on Linux) — matches how Electron apps
        // like Termius render and keeps the UI feeling native per-OS.
        .default_font(theme::SYSTEM_UI)
        .window(window::Settings {
            size: Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            min_size: Some(Size::new(MIN_WIDTH, MIN_HEIGHT)),
            icon,
            decorations: false, // native title bar off — our own chrome in the tab bar
            ..Default::default()
        })
        .antialiasing(true)
        .run()
}

fn init_sentry() -> Option<sentry::ClientInitGuard> {
    // DSN baked in at build time via the SENTRY_DSN env var. Missing/empty =
    // sentry stays off (local dev builds, contributors without the secret).
    let dsn = option_env!("SENTRY_DSN").unwrap_or("");
    if dsn.is_empty() {
        return None;
    }
    let environment = if cfg!(debug_assertions) {
        "development"
    } else {
        "production"
    };
    Some(sentry::init((
        dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            environment: Some(environment.into()),
            attach_stacktrace: true,
            send_default_pii: false,
            ..Default::default()
        },
    )))
}

fn load_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("../../../resources/logo_64.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}
