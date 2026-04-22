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
mod i18n;
mod mcp;
mod os_icon;
mod state;
mod theme;
mod update;
mod util;
mod views;
mod widgets;

use iced::{window, Size};

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 750.0;
const MIN_WIDTH: f32 = 800.0;
const MIN_HEIGHT: f32 = 500.0;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter("oryxis=debug,info")
        .init();

    tracing::info!("Starting Oryxis");

    // Load window icon from PNG
    let icon = load_icon();

    iced::application(app::Oryxis::boot, app::Oryxis::update, app::Oryxis::view)
        .title(app::Oryxis::title)
        .theme(app::Oryxis::theme)
        .subscription(app::Oryxis::subscription)
        .font(iced_fonts::LUCIDE_FONT_BYTES)
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
        .default_font(iced::Font::with_name("Inter"))
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

fn load_icon() -> Option<window::Icon> {
    let bytes = include_bytes!("../../../resources/logo_64.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    window::icon::from_rgba(img.into_raw(), w, h).ok()
}
