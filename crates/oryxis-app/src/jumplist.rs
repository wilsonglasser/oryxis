// Windows JumpList integration: populates the taskbar / Start menu
// right-click menu with a "Recent Hosts" category whose entries
// launch the app with `--connect <uuid>`. Mirrors the PhpStorm
// "Recent Projects" pattern from issue #18.
//
// The full COM dance lives behind a single `rebuild` entry point
// the dispatcher calls when the recent-hosts list changes. Failure
// is logged and swallowed: the JumpList missing or being stale is
// not worth crashing the app over.
//
// Limitation today: each JumpList item launches a new oryxis
// process. When the single-instance mutex (PR 8f) detects a
// running instance, the duplicate exits silently and the launch
// is dropped. Routing the args into the existing instance needs
// an IPC channel which lands in v0.7.1.

#[cfg(target_os = "windows")]
mod imp {
    use std::path::PathBuf;

    use windows::core::{Interface, PCWSTR, Result as WResult};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
        CoInitializeEx,
    };
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList,
        IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink,
    };

    /// Stable AppUserModelID for Oryxis. Has to match what the OS
    /// associates with our shortcuts for the JumpList to render on
    /// the right pinned/start-menu item. Reverse-DNS-style format
    /// is the Microsoft recommendation.
    const APP_USER_MODEL_ID: &str = "io.oryxis.Oryxis";

    /// Register the AppUserModelID with the OS, called once at
    /// process start before the first window opens (so taskbar
    /// grouping picks it up). Failure here is logged + ignored,
    /// the app still runs without a tagged taskbar entry.
    pub fn set_app_id() {
        let wide: Vec<u16> = APP_USER_MODEL_ID
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: wide is null-terminated, function is documented
        // safe to call from any thread before the first taskbar
        // interaction.
        unsafe {
            let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide.as_ptr()));
        }
    }

    /// Replace the JumpList's "Recent Hosts" category with one
    /// shell-link entry per item in `recent`. Each tuple is
    /// `(label, cli_args)` where `cli_args` is appended to the
    /// current exe path (e.g. `"--connect <uuid>"`).
    ///
    /// Safe to call repeatedly; the Shell deduplicates by AppID +
    /// command line internally. No-op when called with an empty
    /// list (clears the category but keeps the JumpList structure).
    pub fn rebuild(recent: &[(String, String)]) {
        if let Err(e) = inner_rebuild(recent) {
            tracing::warn!("JumpList rebuild failed: {e:?}");
        }
    }

    fn inner_rebuild(recent: &[(String, String)]) -> WResult<()> {
        // CoInitializeEx is idempotent per-thread under apartment
        // threading; calling it from each rebuild is safe and lets
        // us not care about who called us first.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() };

        let exe: PathBuf = std::env::current_exe()
            .map_err(|e| windows::core::Error::new(
                windows::core::HRESULT(-1),
                format!("current_exe: {e}"),
            ))?;
        let exe_wide: Vec<u16> = exe
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // ICustomDestinationList is the entry point. BeginList
        // tells the Shell we're rebuilding; AppendCategory adds
        // our named section; CommitList finalises.
        let cdl: ICustomDestinationList = unsafe {
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?
        };
        // Tag this rebuild with our AppUserModelID so the JumpList
        // shows up under our taskbar entry and not whatever the
        // user last interacted with.
        let id_wide: Vec<u16> = APP_USER_MODEL_ID
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe { cdl.SetAppID(PCWSTR(id_wide.as_ptr()))? };

        let mut removed_count: u32 = 0;
        let _removed: IObjectArray =
            unsafe { cdl.BeginList(&mut removed_count)? };

        if !recent.is_empty() {
            let collection: IObjectCollection = unsafe {
                CoCreateInstance(
                    &EnumerableObjectCollection,
                    None,
                    CLSCTX_INPROC_SERVER,
                )?
            };

            for (label, args) in recent {
                if let Ok(link) = build_shell_link(&exe_wide, args, label) {
                    unsafe { collection.AddObject(&link)? };
                }
            }

            let category_wide: Vec<u16> = crate::i18n::t("tray_recent_hosts")
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let array: IObjectArray = collection.cast()?;
            unsafe {
                cdl.AppendCategory(PCWSTR(category_wide.as_ptr()), &array)?;
            }
        }

        unsafe { cdl.CommitList()? };
        Ok(())
    }

    /// Construct one IShellLink with target = oryxis.exe, args =
    /// `--connect <uuid>` (or whatever the caller passed), label =
    /// host label. We use `SetDescription` rather than the more
    /// conventional `PKEY_Title` via IPropertyStore because the
    /// PROPVARIANT struct in windows-rs 0.61 has no `From<PCWSTR>`
    /// impl and constructing the tagged union by hand is more code
    /// for the same visible result: the JumpList displays the
    /// description text on each entry.
    fn build_shell_link(
        exe_wide: &[u16],
        args: &str,
        label: &str,
    ) -> WResult<IShellLinkW> {
        let link: IShellLinkW = unsafe {
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?
        };
        let args_wide: Vec<u16> = args
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let label_wide: Vec<u16> = label
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            link.SetPath(PCWSTR(exe_wide.as_ptr()))?;
            link.SetArguments(PCWSTR(args_wide.as_ptr()))?;
            link.SetDescription(PCWSTR(label_wide.as_ptr()))?;
        }

        Ok(link)
    }

    // OsStrExt::encode_wide is platform-gated to Windows in std;
    // re-import the trait here so the methods chain above resolve.
    use std::os::windows::ffi::OsStrExt;
}

#[cfg(target_os = "windows")]
pub use imp::{rebuild, set_app_id};

#[cfg(not(target_os = "windows"))]
mod stub {
    pub fn set_app_id() {}
    pub fn rebuild(_recent: &[(String, String)]) {}
}

#[cfg(not(target_os = "windows"))]
pub use stub::*;
