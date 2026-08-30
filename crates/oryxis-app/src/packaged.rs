//! Whether this process is running from an MSIX package.
//!
//! The Microsoft Store build ships the same binary as the NSIS
//! installers; what changes is the container around it. Two behaviors
//! must bend when we run inside a package, and both are correctness
//! issues rather than policy niceties:
//!
//! * **Self-update.** `%ProgramFiles%\WindowsApps` is read-only and the
//!   package is serviced by the Store. Running our NSIS installer over
//!   it would not upgrade anything, it would lay down a SECOND,
//!   unpackaged copy next to the packaged one.
//! * **AppUserModelID.** Windows assigns a packaged process the
//!   `<PackageFamilyName>!<App Id>` AUMID. Overwriting it with our own
//!   string (`jumplist::AUMID`) detaches the taskbar button from the
//!   package identity: the JumpList is filed under an id no button
//!   carries, so it silently never shows.
//!
//! Detection is a runtime probe, not a cargo feature, so one build
//! serves both channels: the MSIX workflow packages the very artifact
//! the GitHub release already produces.

/// Whether the current process runs from an MSIX package.
///
/// `GetCurrentPackageFullName` is the documented probe: an unpackaged
/// process answers `APPMODEL_ERROR_NO_PACKAGE`, a packaged one answers
/// `ERROR_INSUFFICIENT_BUFFER` for the deliberately-too-small buffer we
/// pass (we want the verdict, not the name). Any other error is treated
/// as "packaged" only if it is not the no-package sentinel, which keeps
/// an unexpected failure from silently re-enabling the self-updater on a
/// Store install.
#[cfg(target_os = "windows")]
pub(crate) fn is_packaged() -> bool {
    use std::sync::OnceLock;

    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        use windows_sys::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
        use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

        let mut len: u32 = 0;
        let rc = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
        rc != APPMODEL_ERROR_NO_PACKAGE
    })
}

/// The `<PackageFamilyName>!<App Id>` AUMID Windows assigned this
/// packaged process. `None` outside a package (or on an unexpected
/// failure); callers use it to file toasts under the package identity,
/// which is the only id the shell attributes to a Store install.
#[cfg(target_os = "windows")]
pub(crate) fn application_user_model_id() -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentApplicationUserModelId;

    // First call sizes the buffer (ERROR_INSUFFICIENT_BUFFER + length
    // incl. the trailing NUL), second call fills it.
    let mut len: u32 = 0;
    unsafe { GetCurrentApplicationUserModelId(&mut len, std::ptr::null_mut()) };
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    let rc = unsafe { GetCurrentApplicationUserModelId(&mut len, buf.as_mut_ptr()) };
    if rc != ERROR_SUCCESS {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len.saturating_sub(1) as usize]))
}

/// Non-Windows builds are never packaged; MSIX is a Windows container.
/// Only Windows call sites exist, so other targets see it as dead code.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[cfg(not(target_os = "windows"))]
pub(crate) fn is_packaged() -> bool {
    false
}
