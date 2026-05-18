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
    use std::sync::Mutex;

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
    /// Prefix for "active session" submenu entries. The dispatcher
    /// strips the prefix and parses the remainder as a tab index.
    pub const MENU_PREFIX_SESSION: &str = "oryxis-tray-session:";
    /// Prefix for "recent host" entries. Suffix is the connection
    /// UUID (parsed back in dispatch_tabs to open a new tab).
    pub const MENU_PREFIX_HOST: &str = "oryxis-tray-host:";
    /// Prefix for "hidden window" entries (child processes whose
    /// window is currently hidden to the tray). Suffix is the child
    /// PID; the dispatcher forwards a Show command via tray_ipc.
    pub const MENU_PREFIX_HIDDEN: &str = "oryxis-tray-hidden:";

    /// Wrapper that asserts Send + Sync on a value the compiler
    /// thinks is neither. `tray_icon::TrayIcon` contains an Rc /
    /// RefCell which marks it !Send, but we only ever access it
    /// from the main thread (iced's message loop), so the
    /// guarantee holds in practice. The OnceLock storage below
    /// needs the assertion to compile.
    struct ThreadBound<T>(T);
    // SAFETY: see TRAY comment, every read/write happens on the
    // main thread. The unsafe here is a contract with the caller,
    // not a guarantee from the type system.
    unsafe impl<T> Send for ThreadBound<T> {}
    unsafe impl<T> Sync for ThreadBound<T> {}

    /// Held for the lifetime of the process when set. Mutex (not
    /// OnceLock) because the child-promotion path installs the tray
    /// after boot when the original primary dies, and a OnceLock
    /// would refuse the second `set`. iced owns the message loop
    /// the icon's event channels feed into; every interaction
    /// happens from there, hence the ThreadBound safety claim.
    static TRAY: Mutex<Option<ThreadBound<TrayIcon>>> = Mutex::new(None);

    /// `ThreadId` of the thread that called `install()`. Every
    /// subsequent `set_visible` / `rebuild_menu` asserts it's on
    /// the same thread, so a future refactor that moves a tray call
    /// onto a `Task::perform` worker fails loud in debug builds
    /// rather than silently corrupting the `Rc` refcount inside
    /// `tray_icon::TrayIcon`. Release builds skip the assert; the
    /// invariant is documented in the `ThreadBound` SAFETY comment.
    static TRAY_THREAD: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

    /// Panic in debug builds if called from a thread other than the
    /// one that installed the tray. No-op once the tray hasn't been
    /// installed yet (still in setup), and no-op in release builds.
    fn assert_tray_thread(op: &'static str) {
        if cfg!(debug_assertions)
            && let Some(expected) = TRAY_THREAD.get()
            && std::thread::current().id() != *expected
        {
            panic!(
                "tray::{op} called from {:?}, expected {:?} (the install thread). \
                 TrayIcon holds non-Send state.",
                std::thread::current().id(),
                expected
            );
        }
    }

    /// Create the tray icon at app startup. Safe to call once; later
    /// calls are no-ops (idempotent via `OnceLock::set`). Returns
    /// `Ok(())` on success or `Err(...)` if the OS refused to
    /// register the icon (rare on Windows, can happen on locked-down
    /// kiosks).
    pub fn install() -> Result<(), Box<dyn std::error::Error>> {
        // Already installed (we're called twice on the same process
        // somehow). Bail without rebuilding.
        if let Ok(guard) = TRAY.lock()
            && guard.is_some()
        {
            return Ok(());
        }
        // Pin the thread that owns the tray. First call wins; later
        // installs after a promotion still happen on the same iced
        // main thread, so the OnceLock pin holds.
        let _ = TRAY_THREAD.set(std::thread::current().id());

        let menu = Menu::new();
        // Labels go through the i18n table so the tray respects the
        // user's language pick (set in Settings -> Interface).
        // Rebuilding the menu on language change is not yet wired:
        // the user has to restart for new labels to land. Same
        // limitation Termius / Tabby ship with on Windows.
        // Bootstrap menu, replaced by rebuild_menu on the first
        // TrayPoll tick after boot. The unified Windows / Active
        // sessions / Recent hosts sections come in via rebuild;
        // here we only carry the always-present Quit entry so the
        // menu has something to render at boot before the first
        // poll. There is no "Show" or "Hide to tray" static item:
        // the user's UX vision (D-lite) routes Show through the
        // Windows section (one row per hidden window) and treats
        // the title-bar minimize / close buttons as the canonical
        // hide path.
        menu.append(&MenuItem::with_id(
            MENU_ID_QUIT,
            crate::i18n::t("tray_quit"),
            true,
            None,
        ))?;

        let icon = load_icon();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Oryxis")
            .with_icon(icon)
            .build()?;
        // Hide the icon at boot; the dispatcher's visibility rule
        // mounts it as soon as the primary's own window OR any child
        // reports hidden state. tray-icon's builder doesn't expose a
        // with_visible(false), so we toggle right after build.
        let _ = tray.set_visible(false);

        // Promotion path may have raced us; if a TRAY is already
        // here when we try to install, drop ours (the existing one
        // wins) so we never end up with two icons in the tray.
        if let Ok(mut guard) = TRAY.lock()
            && guard.is_none()
        {
            *guard = Some(ThreadBound(tray));
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

    /// Toggle the tray icon's visibility in the notification area.
    /// The user-visible rule lives in the dispatcher (primary's own
    /// window hidden OR any child reports hidden -> show icon, else
    /// hide). This helper just forwards to tray-icon's set_visible.
    /// Failure is logged + swallowed; the worst case is a stale tray
    /// icon hanging around for a tick longer than ideal.
    pub fn set_visible(visible: bool) {
        assert_tray_thread("set_visible");
        let Ok(guard) = TRAY.lock() else { return };
        let Some(ThreadBound(tray)) = guard.as_ref() else { return };
        if let Err(e) = tray.set_visible(visible) {
            tracing::warn!("tray set_visible({visible}): {e}");
        }
    }

    /// Replace the tray icon's menu with a freshly built one that
    /// reflects the current `Active sessions` and `Recent hosts`
    /// lists. Idempotent: the tray-icon crate swaps the underlying
    /// HMENU in place, the OS picks up the new menu on the next
    /// right-click (open menus aren't disrupted because Windows
    /// uses a snapshot).
    ///
    /// The two parameters are pre-formatted (label, id-suffix)
    /// pairs so this module doesn't need to know about TerminalTab
    /// / Connection internals. The caller assembles them from app
    /// state and decides the cap (top N).
    pub fn rebuild_menu(
        active_sessions: &[(String, String)],
        recent_hosts: &[(String, String)],
        hidden_windows: &[(String, String)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert_tray_thread("rebuild_menu");
        let guard = match TRAY.lock() {
            Ok(g) => g,
            Err(_) => return Ok(()),
        };
        let Some(ThreadBound(tray)) = guard.as_ref() else {
            // No tray installed (install() failed or platform stub),
            // nothing to rebuild. Caller doesn't care.
            return Ok(());
        };

        let menu = Menu::new();
        // Unified "Windows" section: every hidden window the user
        // owns lives here (primary's own as the first row when
        // primary is hidden, then every child via the IPC registry).
        // Clicking any row surfaces THAT window. Per the user's
        // UX vision the menu doesn't carry a redundant "Hide to
        // tray" entry; the window's own title-bar minimize / close
        // are the canonical hide path.
        //
        // The caller passes `hidden_windows` already merged (primary
        // first if it belongs there, then children). The id-suffix
        // is the owning process's PID; the dispatcher checks for
        // self_pid to decide between a local TrayShow and an IPC
        // send to a child.
        if !hidden_windows.is_empty() {
            menu.append(&MenuItem::new(
                crate::i18n::t("tray_windows"),
                false,
                None,
            ))?;
            for (label, id_suffix) in hidden_windows {
                let id = format!("{MENU_PREFIX_HIDDEN}{id_suffix}");
                menu.append(&MenuItem::with_id(id, label, true, None))?;
            }
        }

        if !active_sessions.is_empty() {
            if !hidden_windows.is_empty() {
                menu.append(&PredefinedMenuItem::separator())?;
            }
            // Header item, disabled so it reads as a section label.
            menu.append(&MenuItem::new(
                crate::i18n::t("tray_active_sessions"),
                false,
                None,
            ))?;
            for (label, id_suffix) in active_sessions {
                let id = format!("{MENU_PREFIX_SESSION}{id_suffix}");
                menu.append(&MenuItem::with_id(id, label, true, None))?;
            }
        }

        if !recent_hosts.is_empty() {
            menu.append(&PredefinedMenuItem::separator())?;
            menu.append(&MenuItem::new(
                crate::i18n::t("tray_recent_hosts"),
                false,
                None,
            ))?;
            for (label, id_suffix) in recent_hosts {
                let id = format!("{MENU_PREFIX_HOST}{id_suffix}");
                menu.append(&MenuItem::with_id(id, label, true, None))?;
            }
        }

        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&MenuItem::with_id(
            MENU_ID_QUIT,
            crate::i18n::t("tray_quit"),
            true,
            None,
        ))?;

        tray.set_menu(Some(Box::new(menu)));
        Ok(())
    }

    /// Drain any pending icon click event (left click, right click,
    /// double click). Returned variant lets the dispatcher decide
    /// the verb. Empty queue -> `None`.
    pub fn poll_icon_event() -> Option<TrayIconEvent> {
        TrayIconEvent::receiver().try_recv().ok()
    }

    /// Try to acquire the single-instance mutex. Returns true if
    /// we won (we ARE primary), false if another instance owns it.
    /// Side effect: when we win, the handle stays alive for the
    /// rest of the process via a deliberate leak so we hold the
    /// mutex until exit.
    ///
    /// Called twice in the lifecycle:
    /// 1. At boot, to decide primary vs child role.
    /// 2. Periodically from children's TrayPoll to detect a dead
    ///    primary and promote (the OS releases mutexes when the
    ///    owning process exits, so a fresh CreateMutexW succeeds
    ///    again once primary is gone).
    pub fn try_acquire_mutex() -> bool {
        use windows_sys::Win32::Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError,
        };
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let name: Vec<u16> = "Local\\oryxis-single-instance\0"
            .encode_utf16()
            .collect();
        let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already {
            unsafe {
                CloseHandle(h);
            }
            return false;
        }
        // Leak the handle so the mutex object stays owned for the
        // lifetime of this process. The OS reclaims it on exit.
        true
    }

    /// Back-compat wrapper kept for `main.rs`. Returns the inverse
    /// of `try_acquire_mutex`: true when another instance is
    /// already running. The original API was the negation; this
    /// shim avoids churning the call site for what is essentially
    /// the same call.
    pub fn another_instance_running() -> bool {
        !try_acquire_mutex()
    }

    /// Hide the window passed in, going through the raw HWND
    /// instead of `winit::Window::set_visible` (iced 0.14 doesn't
    /// expose it). Called from the iced dispatcher inside a
    /// `iced::window::run` callback, which guarantees we're on the
    /// UI thread with a valid handle. Returns `false` if the handle
    /// wasn't the expected `Win32WindowHandle` variant; that
    /// shouldn't happen in practice but we'd rather log + skip than
    /// panic.
    ///
    /// Takes `&dyn iced::Window` so the dispatcher
    /// can pass the exact closure argument from `window::run`
    /// without an extra crate import. `Window` is `HasWindowHandle
    /// + HasDisplayHandle`, which is the trait method we need.
    pub fn hide_window(handle: &dyn iced::Window) -> bool {
        use iced::window::raw_window_handle::RawWindowHandle;
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
    pub fn show_window(handle: &dyn iced::Window) -> bool {
        use iced::window::raw_window_handle::RawWindowHandle;
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
    pub const MENU_PREFIX_SESSION: &str = "oryxis-tray-session:";
    pub const MENU_PREFIX_HOST: &str = "oryxis-tray-host:";
    pub const MENU_PREFIX_HIDDEN: &str = "oryxis-tray-hidden:";

    pub fn rebuild_menu(
        _active_sessions: &[(String, String)],
        _recent_hosts: &[(String, String)],
        _hidden_windows: &[(String, String)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub fn install() -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub fn set_visible(_visible: bool) {}

    /// Stub: never reports a duplicate instance on non-Windows.
    /// macOS / Linux apps can still be launched twice; the limitation
    /// matches the platform-only scope of the tray feature itself.
    pub fn another_instance_running() -> bool {
        false
    }

    /// Stub: always reports success on non-Windows, so the
    /// promotion path treats every process as already-primary
    /// (matches the no-tray scope).
    pub fn try_acquire_mutex() -> bool {
        true
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
    pub fn hide_window(_handle: &dyn iced::Window) -> bool {
        false
    }

    /// Stub: never actually shows anything on non-Windows targets.
    pub fn show_window(_handle: &dyn iced::Window) -> bool {
        false
    }
}

#[cfg(not(target_os = "windows"))]
pub use stub::*;
