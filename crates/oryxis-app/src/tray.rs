// Windows system tray integration. Gated entirely on Windows:
// macOS expects different lifecycle conventions (LSUIElement plist,
// Cmd+Q never quits, dock icon owns the "show" verb) and Linux tray
// support is fragmented across DEs (GNOME deprecated it without an
// AppIndicator extension, Wayland-pure setups don't have one at all).
// Shipping Windows-first matches the actual user demand from issue
// #18 (koobs on Win11 Pro) and avoids platform-specific bug surface
// we can't reasonably test from CI today.
//
// Architecture: a singleton TrayHandle holds the underlying
// `tray_icon::TrayIcon` and its `MenuItem` references. The iced
// `Subscription` in `subscription.rs` polls the global tray event
// receivers (menu + icon) and converts them into `Message`s the
// dispatcher already understands. The HWND dance for true
// hide-to-tray lives in `dispatch.rs` (it uses `iced::window::run`
// to grab the raw window handle on demand).

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::OnceLock;

    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    /// Menu item ids the dispatcher maps back to `Message`s. The
    /// crate identifies items by their `MenuId` (a string), we keep
    /// the values short and stable so the matching is cheap.
    pub const MENU_ID_SHOW: &str = "oryxis-tray-show";
    pub const MENU_ID_HIDE: &str = "oryxis-tray-hide";
    pub const MENU_ID_QUIT: &str = "oryxis-tray-quit";

    /// Held for the lifetime of the process. Dropping the `TrayIcon`
    /// removes the icon from the notification area immediately, so
    /// we stash it in a `OnceLock` and never release it. iced owns
    /// the message loop the icon's event channels feed into.
    static TRAY: OnceLock<TrayIcon> = OnceLock::new();

    /// Create the tray icon at app startup. Safe to call once; later
    /// calls are no-ops (idempotent via `OnceLock::set`). Returns
    /// `Ok(())` on success or `Err(...)` if the OS refused to
    /// register the icon (rare on Windows, can happen on locked-down
    /// kiosks).
    pub fn install() -> Result<(), Box<dyn std::error::Error>> {
        if TRAY.get().is_some() {
            return Ok(());
        }

        let menu = Menu::new();
        menu.append(&MenuItem::with_id(MENU_ID_SHOW, "Show Oryxis", true, None))?;
        menu.append(&MenuItem::with_id(MENU_ID_HIDE, "Hide to tray", true, None))?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&MenuItem::with_id(MENU_ID_QUIT, "Quit", true, None))?;

        let icon = load_icon();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Oryxis")
            .with_icon(icon)
            .build()?;

        // OnceLock::set returns Err with the value we tried to set
        // if another thread won the race; the icon dropping there
        // would un-register from the tray, so we deliberately leak
        // it via mem::forget if that happens to keep the visible
        // icon alive.
        if let Err(losing) = TRAY.set(tray) {
            std::mem::forget(losing);
        }
        Ok(())
    }

    fn load_icon() -> Icon {
        let bytes = include_bytes!("../../../resources/logo_64.png");
        let img = image::load_from_memory(bytes)
            .expect("bundled tray icon decodes")
            .into_rgba8();
        let (w, h) = img.dimensions();
        Icon::from_rgba(img.into_raw(), w, h).expect("rgba dimensions match")
    }

    /// Drain any pending menu click event without blocking. Called
    /// from the iced subscription poll. Returns the clicked menu
    /// item's id, or `None` when the queue is empty.
    pub fn poll_menu_event() -> Option<String> {
        MenuEvent::receiver().try_recv().ok().map(|e| e.id.0)
    }

    /// Drain any pending icon click event (left click, right click,
    /// double click). Returned variant lets the dispatcher decide
    /// the verb. Empty queue -> `None`.
    pub fn poll_icon_event() -> Option<TrayIconEvent> {
        TrayIconEvent::receiver().try_recv().ok()
    }
}

#[cfg(target_os = "windows")]
pub use imp::*;

/// Cross-platform stubs so call sites compile uniformly. On non-
/// Windows targets the tray module is a no-op: `install` succeeds
/// silently, polls return `None`. The settings UI also hides the
/// tray-related toggles outside Windows, but the runtime hooks stay
/// callable to keep dispatch.rs free of cfg branches.
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
mod stub {
    pub const MENU_ID_SHOW: &str = "oryxis-tray-show";
    pub const MENU_ID_HIDE: &str = "oryxis-tray-hide";
    pub const MENU_ID_QUIT: &str = "oryxis-tray-quit";

    pub fn install() -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub fn poll_menu_event() -> Option<String> {
        None
    }

    /// Placeholder type so subscription.rs can match a single shape
    /// regardless of platform. The Windows path returns the real
    /// `tray_icon::TrayIconEvent`; here we never produce one.
    pub enum TrayIconEvent {}

    pub fn poll_icon_event() -> Option<TrayIconEvent> {
        None
    }
}

#[cfg(not(target_os = "windows"))]
pub use stub::*;
