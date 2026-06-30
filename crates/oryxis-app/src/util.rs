//! Pure utility functions, no UI, no state.

use iced::keyboard;

/// Format byte size for display (e.g. "12.3 KB").
pub(crate) fn format_data_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Open the OS screen-capture tool. On Windows this launches the
/// modern Snipping Tool region overlay via the `ms-screenclip:` URI,
/// matching the default PrintScreen behavior. winit hands PrintScreen
/// to the focused window without forwarding it to `DefWindowProc`, so
/// Windows' own PrintScreen handler never fires while Oryxis has focus;
/// we trigger the snip explicitly. No-op elsewhere (on Linux/macOS the
/// desktop environment owns the key and it reaches the OS normally).
#[cfg(target_os = "windows")]
pub(crate) fn open_screenshot_tool() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // UTF-16, NUL-terminated. The "open" verb on the ms-screenclip:
    // scheme launches the Snip & Sketch region picker (same as the
    // Win+Shift+S shortcut and the Win11 default PrintScreen action).
    let verb: Vec<u16> = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target: Vec<u16> = std::ffi::OsStr::new("ms-screenclip:")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// How the terminal reacts to the bell (BEL / `\a`). Persisted as its `code()`
/// string in the `terminal_bell_mode` setting. Default `Beep`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum BellMode {
    /// Ignore the bell entirely.
    Off,
    /// Briefly flash the terminal pane (visual bell).
    Flash,
    /// Audible system beep (best-effort native).
    #[default]
    Beep,
}

impl BellMode {
    pub(crate) const ALL: [BellMode; 3] = [BellMode::Off, BellMode::Flash, BellMode::Beep];

    pub(crate) fn code(self) -> &'static str {
        match self {
            BellMode::Off => "off",
            BellMode::Flash => "flash",
            BellMode::Beep => "beep",
        }
    }

    pub(crate) fn from_code(code: &str) -> Self {
        match code {
            "off" => BellMode::Off,
            "flash" => BellMode::Flash,
            _ => BellMode::Beep,
        }
    }

    /// i18n key for the localized label shown in the settings pick-list.
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            BellMode::Off => "bell_off",
            BellMode::Flash => "bell_flash",
            BellMode::Beep => "bell_beep",
        }
    }
}

/// OSC 52 clipboard access policy. Persisted as `code()` in the
/// `terminal_clipboard_access` setting. Default `WriteOnly`: apps may set the
/// system clipboard (tmux/vim yank) but not read it (read is a privacy risk).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum ClipboardAccess {
    /// Ignore OSC 52 entirely.
    Off,
    /// Apps may set the clipboard, not read it.
    #[default]
    WriteOnly,
    /// Apps may both set and read the clipboard.
    ReadWrite,
}

impl ClipboardAccess {
    pub(crate) const ALL: [ClipboardAccess; 3] = [
        ClipboardAccess::Off,
        ClipboardAccess::WriteOnly,
        ClipboardAccess::ReadWrite,
    ];

    pub(crate) fn code(self) -> &'static str {
        match self {
            ClipboardAccess::Off => "off",
            ClipboardAccess::WriteOnly => "write",
            ClipboardAccess::ReadWrite => "readwrite",
        }
    }

    pub(crate) fn from_code(code: &str) -> Self {
        match code {
            "off" => ClipboardAccess::Off,
            "readwrite" => ClipboardAccess::ReadWrite,
            _ => ClipboardAccess::WriteOnly,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            ClipboardAccess::Off => "clipboard_off",
            ClipboardAccess::WriteOnly => "clipboard_write",
            ClipboardAccess::ReadWrite => "clipboard_readwrite",
        }
    }

    /// `(write, read)` flags for `oryxis_terminal::set_clipboard_access`.
    pub(crate) fn flags(self) -> (bool, bool) {
        match self {
            ClipboardAccess::Off => (false, false),
            ClipboardAccess::WriteOnly => (true, false),
            ClipboardAccess::ReadWrite => (true, true),
        }
    }
}

/// How an OSC 9 notification from the shell is surfaced. Persisted as `code()`
/// in `terminal_notification`. Default `Os`: a notification's whole point is to
/// reach you when the window isn't visible, so the native OS notification is
/// the useful one; the in-app toast only helps when the app is already on
/// screen (where you'd have seen the output anyway).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum NotificationMode {
    /// Ignore OSC 9 notifications.
    Off,
    /// Show an in-app toast.
    Toast,
    /// Show a native OS notification (falls back to a toast if it fails).
    #[default]
    Os,
}

impl NotificationMode {
    pub(crate) const ALL: [NotificationMode; 3] = [
        NotificationMode::Off,
        NotificationMode::Toast,
        NotificationMode::Os,
    ];

    pub(crate) fn code(self) -> &'static str {
        match self {
            NotificationMode::Off => "off",
            NotificationMode::Toast => "toast",
            NotificationMode::Os => "os",
        }
    }

    pub(crate) fn from_code(code: &str) -> Self {
        match code {
            "off" => NotificationMode::Off,
            "toast" => NotificationMode::Toast,
            _ => NotificationMode::Os,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            NotificationMode::Off => "notify_off",
            NotificationMode::Toast => "notify_toast",
            NotificationMode::Os => "notify_os",
        }
    }
}

/// How terminal teaching hints (the "hold Shift to select" mouse-capture
/// toast, the "Ctrl + Click to open" link tooltip) are surfaced. Default
/// `Once`: each hint shows a single time per terminal pane, enough to teach
/// without nagging, and returns on a fresh pane (new tab / host). `Always`
/// shows it on every trigger; `Never` silences them. Replaces the old
/// persisted "shown once forever" flag + "Reset hints" button.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum HintMode {
    /// Show the hint every time its trigger fires.
    Always,
    /// Show the hint once per pane, then retire it for that pane.
    #[default]
    Once,
    /// Never show terminal hints.
    Never,
}

impl HintMode {
    pub(crate) const ALL: [HintMode; 3] =
        [HintMode::Always, HintMode::Once, HintMode::Never];

    pub(crate) fn code(self) -> &'static str {
        match self {
            HintMode::Always => "always",
            HintMode::Once => "once",
            HintMode::Never => "never",
        }
    }

    pub(crate) fn from_code(code: &str) -> Self {
        match code {
            "always" => HintMode::Always,
            "never" => HintMode::Never,
            _ => HintMode::Once,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            HintMode::Always => "hint_mode_always",
            HintMode::Once => "hint_mode_once",
            HintMode::Never => "hint_mode_never",
        }
    }

    /// Whether a hint should render now, given whether it has already been
    /// shown for this pane. `Always` ignores the flag; `Once` honours it;
    /// `Never` is always silent.
    pub(crate) fn should_show(self, already_shown: bool) -> bool {
        match self {
            HintMode::Always => true,
            HintMode::Once => !already_shown,
            HintMode::Never => false,
        }
    }
}

/// Show a native OS notification (OSC 9). Returns whether it was dispatched;
/// the caller falls back to an in-app toast on `false` (no notification daemon
/// on Linux, or no registered AppUserModelID on a non-installed Windows build).
pub(crate) fn show_os_notification(summary: &str, body: &str) -> bool {
    notify_rust::Notification::new()
        .summary(summary)
        .body(body)
        .appname("Oryxis")
        .show()
        .is_ok()
}

/// Best-effort native system beep, no audio dependency. Windows uses
/// `MessageBeep`; macOS shells out to `osascript -e beep`; Linux tries the
/// freedesktop bell through whichever player is present. Silent if none is
/// available, which is exactly why the visual `Flash` mode exists as the
/// reliable alternative. Never blocks the UI thread.
pub(crate) fn play_system_beep() {
    #[cfg(target_os = "windows")]
    {
        // 0xFFFFFFFF = a simple speaker beep, independent of the sound scheme.
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(0xFFFF_FFFF);
        }
    }
    #[cfg(target_os = "macos")]
    {
        spawn_and_reap(std::process::Command::new("osascript").args(["-e", "beep"]));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // First player that launches wins; each is a no-op if not installed.
        const BELL_OGA: &str = "/usr/share/sounds/freedesktop/stereo/bell.oga";
        let mut canberra = std::process::Command::new("canberra-gtk-play");
        canberra.args(["-i", "bell"]);
        let mut paplay = std::process::Command::new("paplay");
        paplay.arg(BELL_OGA);
        let mut pw_play = std::process::Command::new("pw-play");
        pw_play.arg(BELL_OGA);
        for cmd in [&mut canberra, &mut paplay, &mut pw_play] {
            if spawn_and_reap(cmd) {
                break;
            }
        }
    }
}

/// Spawn a fire-and-forget child and reap it on a detached thread so it never
/// becomes a zombie. Returns whether the spawn itself succeeded.
#[cfg(unix)]
fn spawn_and_reap(cmd: &mut std::process::Command) -> bool {
    use std::process::Stdio;
    match cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// xterm modifier parameter: `1 + Shift(1) + Alt(2) + Ctrl(4)`. Returns 1 when
/// no modifier is held (the "unmodified" sentinel xterm uses in `CSI 1 ; N …`).
fn xterm_modifier_param(m: &keyboard::Modifiers) -> u8 {
    1 + (m.shift() as u8) + 2 * (m.alt() as u8) + 4 * (m.control() as u8)
}

/// Translate a named iced key (Enter, Tab, ArrowUp, …) into the PTY byte
/// sequence, honoring modifiers in the xterm scheme.
///
/// Cursor / Home / End keys take the SS3 form (`ESC O A`) under application-
/// cursor-keys mode (DECCKM) and the CSI form (`ESC [ A`) otherwise, but a
/// *modified* press is always CSI with a parameter (`ESC [ 1 ; 5 A` for
/// Ctrl+Up), the form vim / readline / editors bind word-jump and selection
/// to. The `~`-terminated keys (PageUp/Down, Insert, Delete, F5-F12) carry the
/// modifier as `ESC [ N ; M ~`. F1-F4 go from `ESC O P` to `ESC [ 1 ; M P`
/// when modified. Shift+Tab is the back-tab `ESC [ Z`.
pub(crate) fn key_to_named_bytes(
    key: &keyboard::Key,
    modifiers: &keyboard::Modifiers,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let keyboard::Key::Named(named) = key else {
        return None;
    };
    let param = xterm_modifier_param(modifiers);
    let modified = param > 1;

    // Arrows + Home/End: `letter` is the CSI/SS3 final byte.
    let csi_letter = |letter: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{}{}", param, letter as char).into_bytes()
        } else if app_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    // `~`-terminated keys (PageUp/Down, Insert, Delete, F5-F12).
    let tilde = |num: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{num};{param}~").into_bytes()
        } else {
            format!("\x1b[{num}~").into_bytes()
        }
    };
    // F1-F4: SS3 final byte unmodified, CSI with parameter when modified.
    let ss3_fn = |letter: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{}{}", param, letter as char).into_bytes()
        } else {
            vec![0x1b, b'O', letter]
        }
    };

    use keyboard::key::Named;
    let bytes: Vec<u8> = match named {
        Named::Enter => b"\r".to_vec(),
        Named::Backspace => b"\x7f".to_vec(),
        // Shift+Tab is back-tab (CBT); plain Tab stays HT.
        Named::Tab if modifiers.shift() => b"\x1b[Z".to_vec(),
        Named::Tab => b"\t".to_vec(),
        Named::Escape => b"\x1b".to_vec(),
        Named::Space => b" ".to_vec(),
        Named::ArrowUp => csi_letter(b'A'),
        Named::ArrowDown => csi_letter(b'B'),
        Named::ArrowRight => csi_letter(b'C'),
        Named::ArrowLeft => csi_letter(b'D'),
        Named::Home => csi_letter(b'H'),
        Named::End => csi_letter(b'F'),
        Named::PageUp => tilde(5),
        Named::PageDown => tilde(6),
        Named::Insert => tilde(2),
        Named::Delete => tilde(3),
        Named::F1 => ss3_fn(b'P'),
        Named::F2 => ss3_fn(b'Q'),
        Named::F3 => ss3_fn(b'R'),
        Named::F4 => ss3_fn(b'S'),
        Named::F5 => tilde(15),
        Named::F6 => tilde(17),
        Named::F7 => tilde(18),
        Named::F8 => tilde(19),
        Named::F9 => tilde(20),
        Named::F10 => tilde(21),
        Named::F11 => tilde(23),
        Named::F12 => tilde(24),
        _ => return None,
    };
    Some(bytes)
}

/// Snap the chat sidebar's scrollable to its bottom, used after the
/// user sends a message and after the assistant response arrives, so
/// the conversation stays anchored at the latest exchange.
pub(crate) fn chat_scroll_to_end() -> iced::Task<crate::app::Message> {
    iced::widget::operation::snap_to_end(iced::widget::Id::new("chat-scroll"))
}

/// Strip non-digit characters and clamp the result against `max`.
/// Empty / fully-invalid input collapses to `"0"`. Used to keep numeric
/// setting fields from accepting garbage like "abc" or
/// "999999999999999".
pub(crate) fn sanitize_uint(input: &str, max: u64) -> String {
    let digits: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return "0".to_string();
    }
    let value: u64 = digits.parse().unwrap_or(max);
    value.min(max).to_string()
}

/// Open an external URL in the user's default browser. Best-effort
/// the UI falls back to copying the URL to the clipboard if this fails,
/// so the io::Error here is something the caller can swallow.
pub(crate) fn open_in_browser(url: &str) -> Result<(), std::io::Error> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW so the `cmd /C start` shim doesn't flash a
        // console window on the GUI-subsystem app.
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(0x0800_0000)
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

/// Reveal a local path in the OS file manager. A directory opens in
/// place; a file opens its containing folder with the file selected
/// where the platform supports it (Windows `explorer /select`, macOS
/// `open -R`, Linux freedesktop `FileManager1.ShowItems` with a
/// fall back to opening the parent folder). Best-effort: the io::Error
/// is surfaced so the caller can show it.
pub(crate) fn reveal_in_file_manager(
    path: &std::path::Path,
    is_dir: bool,
) -> Result<(), std::io::Error> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW so the launch doesn't flash a console window.
        let mut cmd = std::process::Command::new("explorer");
        if is_dir {
            cmd.arg(path);
        } else {
            // "/select," must be glued to the path in a single argument;
            // explorer parses the comma-separated form itself.
            cmd.arg(format!("/select,{}", path.display()));
        }
        cmd.creation_flags(0x0800_0000).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if is_dir {
            cmd.arg(path);
        } else {
            // -R reveals (selects) the item in Finder.
            cmd.arg("-R").arg(path);
        }
        cmd.spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if is_dir {
            std::process::Command::new("xdg-open").arg(path).spawn()?;
        } else {
            // Ask the freedesktop file manager to show + select the item.
            // Most managers (Nautilus, Dolphin, Nemo, ...) implement this;
            // if D-Bus or the service is missing, fall back to opening the
            // containing directory so the action never silently no-ops.
            let uri = format!("file://{}", path.display());
            let shown = std::process::Command::new("dbus-send")
                .args([
                    "--session",
                    "--dest=org.freedesktop.FileManager1",
                    "--type=method_call",
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1.ShowItems",
                    &format!("array:string:{uri}"),
                    "string:",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !shown {
                let parent = path.parent().unwrap_or(path);
                std::process::Command::new("xdg-open").arg(parent).spawn()?;
            }
        }
    }
    Ok(())
}

/// Resolve the effective SSH keepalive duration for a connection.
/// `per_host` mirrors `Connection.keepalive_interval`: `None` falls
/// back to the parsed `global` string; `Some(0)` explicitly disables
/// keepalive on this host even when the global default is non-zero;
/// `Some(n)` overrides with `n` seconds. `global` is the raw value
/// from the settings text input, so non-numeric or empty input
/// degrades gracefully to disabled.
pub(crate) fn resolve_keepalive(
    per_host: Option<u32>,
    global: &str,
) -> Option<std::time::Duration> {
    let secs = match per_host {
        Some(n) => u64::from(n),
        None => global.parse().unwrap_or(0),
    };
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

/// Translate a Ctrl+<char> combination into the control byte sequence.
pub(crate) fn ctrl_key_bytes(key: &keyboard::Key) -> Option<Vec<u8>> {
    if let keyboard::Key::Character(c) = key {
        let ch = c.as_str().bytes().next()?;
        let ctrl = match ch {
            b'a'..=b'z' => ch - b'a' + 1,
            b'A'..=b'Z' => ch - b'A' + 1,
            b'[' => 27,
            b'\\' => 28,
            b']' => 29,
            b'^' => 30,
            b'_' => 31,
            _ => return None,
        };
        Some(vec![ctrl])
    } else {
        None
    }
}

// ── New-connection default helpers ──
//
// These translate the typed "default host profile" settings to / from
// their settings-table string form and the localized picker labels.

/// Localized picker label for an auth method (mirrors the host editor's
/// auth picker).
pub(crate) fn auth_method_label(m: &oryxis_core::models::connection::AuthMethod) -> String {
    use crate::i18n::t;
    use oryxis_core::models::connection::AuthMethod;
    match m {
        AuthMethod::Auto => t("auth_auto"),
        AuthMethod::Password => t("auth_password"),
        AuthMethod::Key => t("auth_key"),
        AuthMethod::Agent => t("auth_agent"),
        AuthMethod::Interactive => t("auth_interactive"),
    }
    .to_string()
}

/// Resolve a localized (or English) auth-picker label back to the enum.
/// Mirrors `EditorAuthMethodChanged`: English fallback keeps a label
/// persisted in another locale resolvable. Unknown values are `Auto`.
pub(crate) fn auth_method_from_label(v: &str) -> oryxis_core::models::connection::AuthMethod {
    use crate::i18n::t;
    use oryxis_core::models::connection::AuthMethod;
    if v == t("auth_password") || v == "Password" {
        AuthMethod::Password
    } else if v == t("auth_key") || v == "Key" {
        AuthMethod::Key
    } else if v == t("auth_agent") || v == "Agent" {
        AuthMethod::Agent
    } else if v == t("auth_interactive") || v == "Interactive" {
        AuthMethod::Interactive
    } else {
        AuthMethod::Auto
    }
}

/// Stable settings-table string for an auth method (the variant name,
/// locale-independent so the persisted value survives a language switch).
pub(crate) fn auth_method_to_setting(m: &oryxis_core::models::connection::AuthMethod) -> String {
    use oryxis_core::models::connection::AuthMethod;
    match m {
        AuthMethod::Auto => "Auto",
        AuthMethod::Password => "Password",
        AuthMethod::Key => "Key",
        AuthMethod::Agent => "Agent",
        AuthMethod::Interactive => "Interactive",
    }
    .to_string()
}

/// Parse the settings-table auth-method string back to the enum; unknown
/// / legacy values fall back to `Auto`.
pub(crate) fn auth_method_from_setting(v: &str) -> oryxis_core::models::connection::AuthMethod {
    use oryxis_core::models::connection::AuthMethod;
    match v {
        "Password" => AuthMethod::Password,
        "Key" => AuthMethod::Key,
        "Agent" => AuthMethod::Agent,
        "Interactive" => AuthMethod::Interactive,
        _ => AuthMethod::Auto,
    }
}

/// Serialize the default env-var rows to the JSON array stored in the
/// settings table. Rows with a blank key are dropped (key trimmed) so a
/// half-typed row never persists; values may contain `=`, hence JSON
/// rather than `KEY=VALUE` lines.
pub(crate) fn env_vars_to_setting(rows: &[crate::state::EnvVarForm]) -> String {
    let kept: Vec<oryxis_core::models::connection::EnvVar> = rows
        .iter()
        .filter(|e| !e.key.trim().is_empty())
        .map(|e| oryxis_core::models::connection::EnvVar {
            key: e.key.trim().to_string(),
            value: e.value.clone(),
        })
        .collect();
    serde_json::to_string(&kept).unwrap_or_else(|_| "[]".to_string())
}

/// Parse the settings-table env-vars JSON into editable form rows. A
/// malformed / legacy value yields an empty list rather than an error.
pub(crate) fn env_vars_from_setting(v: &str) -> Vec<crate::state::EnvVarForm> {
    serde_json::from_str::<Vec<oryxis_core::models::connection::EnvVar>>(v)
        .unwrap_or_default()
        .into_iter()
        .map(|e| crate::state::EnvVarForm { key: e.key, value: e.value })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};
    use std::time::Duration;

    fn nb(named: Named, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
        key_to_named_bytes(&Key::Named(named), &mods, app_cursor).unwrap()
    }

    #[test]
    fn arrows_plain_are_csi_and_app_cursor_is_ss3() {
        // Default (normal) cursor mode: CSI form.
        assert_eq!(nb(Named::ArrowUp, Modifiers::empty(), false), b"\x1b[A");
        assert_eq!(nb(Named::ArrowLeft, Modifiers::empty(), false), b"\x1b[D");
        // Application-cursor-keys mode (DECCKM): SS3 form, what mc/vim bind to.
        assert_eq!(nb(Named::ArrowUp, Modifiers::empty(), true), b"\x1bOA");
        assert_eq!(nb(Named::End, Modifiers::empty(), true), b"\x1bOF");
    }

    #[test]
    fn modified_arrows_use_xterm_parameter_and_stay_csi() {
        // Ctrl = param 5, Shift = 2, Alt = 3.
        assert_eq!(nb(Named::ArrowRight, Modifiers::CTRL, false), b"\x1b[1;5C");
        assert_eq!(nb(Named::ArrowUp, Modifiers::SHIFT, false), b"\x1b[1;2A");
        assert_eq!(nb(Named::ArrowDown, Modifiers::ALT, false), b"\x1b[1;3B");
        assert_eq!(
            nb(Named::ArrowLeft, Modifiers::CTRL | Modifiers::SHIFT, false),
            b"\x1b[1;6D"
        );
        // A modified press is CSI even under application-cursor-keys mode.
        assert_eq!(nb(Named::ArrowUp, Modifiers::CTRL, true), b"\x1b[1;5A");
    }

    #[test]
    fn tilde_keys_carry_modifier() {
        assert_eq!(nb(Named::PageUp, Modifiers::empty(), false), b"\x1b[5~");
        assert_eq!(nb(Named::PageUp, Modifiers::CTRL, false), b"\x1b[5;5~");
        assert_eq!(nb(Named::Delete, Modifiers::SHIFT, false), b"\x1b[3;2~");
    }

    #[test]
    fn function_keys_promote_to_csi_when_modified() {
        assert_eq!(nb(Named::F1, Modifiers::empty(), false), b"\x1bOP");
        assert_eq!(nb(Named::F1, Modifiers::CTRL, false), b"\x1b[1;5P");
        assert_eq!(nb(Named::F5, Modifiers::empty(), false), b"\x1b[15~");
        assert_eq!(nb(Named::F5, Modifiers::CTRL, false), b"\x1b[15;5~");
    }

    #[test]
    fn shift_tab_is_back_tab() {
        assert_eq!(nb(Named::Tab, Modifiers::empty(), false), b"\t");
        assert_eq!(nb(Named::Tab, Modifiers::SHIFT, false), b"\x1b[Z");
    }

    #[test]
    fn bell_mode_code_round_trips_and_defaults_to_beep() {
        for m in BellMode::ALL {
            assert_eq!(BellMode::from_code(m.code()), m);
        }
        // Unknown / legacy values fall back to the default (beep).
        assert_eq!(BellMode::from_code("garbage"), BellMode::Beep);
        assert_eq!(BellMode::default(), BellMode::Beep);
    }

    #[test]
    fn notification_mode_round_trips_and_defaults_to_os() {
        for m in NotificationMode::ALL {
            assert_eq!(NotificationMode::from_code(m.code()), m);
        }
        assert_eq!(NotificationMode::from_code("garbage"), NotificationMode::Os);
        assert_eq!(NotificationMode::default(), NotificationMode::Os);
    }

    #[test]
    fn clipboard_access_round_trips_and_defaults_to_write_only() {
        for m in ClipboardAccess::ALL {
            assert_eq!(ClipboardAccess::from_code(m.code()), m);
        }
        assert_eq!(ClipboardAccess::from_code("garbage"), ClipboardAccess::WriteOnly);
        assert_eq!(ClipboardAccess::default(), ClipboardAccess::WriteOnly);
        // Flag mapping: write-only allows write, blocks read; off blocks both.
        assert_eq!(ClipboardAccess::Off.flags(), (false, false));
        assert_eq!(ClipboardAccess::WriteOnly.flags(), (true, false));
        assert_eq!(ClipboardAccess::ReadWrite.flags(), (true, true));
    }

    #[test]
    fn keepalive_inherits_global_when_per_host_is_none() {
        assert_eq!(resolve_keepalive(None, "30"), Some(Duration::from_secs(30)));
        assert_eq!(resolve_keepalive(None, "60"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn keepalive_global_zero_means_disabled() {
        assert_eq!(resolve_keepalive(None, "0"), None);
    }

    #[test]
    fn keepalive_per_host_zero_disables_even_when_global_is_set() {
        // Per-host explicit "0" must beat a non-zero global. This is
        // the escape hatch for users who want keepalive globally but
        // need it off on a specific host (rare, but it must work).
        assert_eq!(resolve_keepalive(Some(0), "30"), None);
        assert_eq!(resolve_keepalive(Some(0), "120"), None);
    }

    #[test]
    fn keepalive_per_host_overrides_global() {
        assert_eq!(resolve_keepalive(Some(60), "30"), Some(Duration::from_secs(60)));
        assert_eq!(resolve_keepalive(Some(15), "0"), Some(Duration::from_secs(15)));
    }

    #[test]
    fn keepalive_invalid_global_degrades_to_disabled() {
        // The settings field accepts arbitrary text; non-numeric values
        // must not panic. They collapse to disabled (parse() -> 0).
        assert_eq!(resolve_keepalive(None, "abc"), None);
        assert_eq!(resolve_keepalive(None, ""), None);
        assert_eq!(resolve_keepalive(None, "  "), None);
    }

    #[test]
    fn keepalive_per_host_wins_over_invalid_global() {
        // Even if the global setting is malformed, an explicit per-host
        // value must still apply.
        assert_eq!(resolve_keepalive(Some(45), "garbage"), Some(Duration::from_secs(45)));
    }

    #[test]
    fn default_auth_method_setting_round_trips_and_defaults_to_auto() {
        use oryxis_core::models::connection::AuthMethod;
        for m in [
            AuthMethod::Auto,
            AuthMethod::Password,
            AuthMethod::Key,
            AuthMethod::Agent,
            AuthMethod::Interactive,
        ] {
            let s = auth_method_to_setting(&m);
            assert_eq!(auth_method_from_setting(&s), m);
        }
        // Unknown / legacy values fall back to Auto, never panic.
        assert_eq!(auth_method_from_setting("garbage"), AuthMethod::Auto);
        assert_eq!(auth_method_from_setting(""), AuthMethod::Auto);
    }

    #[test]
    fn default_env_vars_setting_round_trips_and_drops_blank_keys() {
        let rows = vec![
            crate::state::EnvVarForm { key: "LANG".into(), value: "en_US.UTF-8".into() },
            // Value carrying an '=' must survive (JSON, not KEY=VALUE lines).
            crate::state::EnvVarForm { key: "FLAGS".into(), value: "a=b=c".into() },
            // Blank / whitespace key is dropped; key is trimmed.
            crate::state::EnvVarForm { key: "  ".into(), value: "ignored".into() },
            crate::state::EnvVarForm { key: " LC_ALL ".into(), value: "C".into() },
        ];
        let serialized = env_vars_to_setting(&rows);
        let back = env_vars_from_setting(&serialized);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].key, "LANG");
        assert_eq!(back[0].value, "en_US.UTF-8");
        assert_eq!(back[1].value, "a=b=c");
        assert_eq!(back[2].key, "LC_ALL");
        // A malformed / legacy value yields an empty list, never an error.
        assert!(env_vars_from_setting("not json").is_empty());
        assert!(env_vars_from_setting("").is_empty());
    }
}
