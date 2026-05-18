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

    /// Hide the window passed in, going through the raw HWND
    /// instead of `winit::Window::set_visible` (iced 0.14 doesn't
    /// expose it). Called from the iced dispatcher inside a
    /// `iced::window::run` callback, which guarantees we're on the
    /// UI thread with a valid handle. Returns `false` if the handle
    /// wasn't the expected `Win32WindowHandle` variant; that
    /// shouldn't happen in practice but we'd rather log + skip than
    /// panic.
    pub fn hide_window(handle: &dyn raw_window_handle::HasWindowHandle) -> bool {
        use raw_window_handle::RawWindowHandle;
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

        let Ok(wh) = handle.window_handle() else {
            return false;
        };
        let RawWindowHandle::Win32(win32) = wh.as_raw() else {
            return false;
        };
        // SAFETY: HWND is valid for the lifetime of the &dyn handle
        // reference; SW_HIDE is a constant integer argument with no
        // pointer semantics. The call is documented as thread-safe
        // for the owning thread, which is where iced::window::run
        // dispatches us.
        unsafe {
            let _ = ShowWindow(win32.hwnd.get() as _, SW_HIDE);
        }
        true
    }

    /// Restore a hidden window: show it, then pull to foreground.
    /// `SW_SHOW` alone leaves it in the previous z-order, so an
    /// `SetForegroundWindow` chases it to land on top. If the
    /// window was minimized when hidden, `SW_RESTORE` instead of
    /// `SW_SHOW` un-minimizes too.
    pub fn show_window(handle: &dyn raw_window_handle::HasWindowHandle) -> bool {
        use raw_window_handle::RawWindowHandle;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_RESTORE,
        };

        let Ok(wh) = handle.window_handle() else {
            return false;
        };
        let RawWindowHandle::Win32(win32) = wh.as_raw() else {
            return false;
        };
        // SAFETY: same rationale as hide_window. SetForegroundWindow
        // can fail silently (Windows focus-stealing prevention) but
        // doesn't unsafe-misuse the HWND on failure.
        unsafe {
            let hwnd = win32.hwnd.get() as _;
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        true
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

    /// Stub: never actually hides anything on non-Windows targets.
    /// Same signature as the Windows impl so dispatch.rs stays cfg-
    /// free. Returns false so the caller knows nothing happened.
    pub fn hide_window<H: ?Sized>(_handle: &H) -> bool {
        false
    }

    /// Stub: never actually shows anything on non-Windows targets.
    pub fn show_window<H: ?Sized>(_handle: &H) -> bool {
        false
    }
}

#[cfg(not(target_os = "windows"))]
pub use stub::*;
