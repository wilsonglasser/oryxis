//! Pure utility functions, no UI, no state.

/// File-name-safe version of a label (ASCII alphanumerics, `-`, `_`;
/// everything else collapses to `_`, capped at 48 chars). Used by the
/// command-history and session-recording exports.
pub(crate) fn sanitize_file_stem(label: &str) -> String {
    let mut s: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    s.truncate(48);
    if s.is_empty() {
        s.push('_');
    }
    s
}

/// Display label + unique local temp path for an edit-in-place download
/// (SFTP "Edit" / "Open with"). The label is the remote basename,
/// verbatim. The temp file name swaps out characters that are path
/// separators or reserved on any supported OS: a remote Unix file name
/// may legally contain `\`, `:` or `"`, which on Windows would split the
/// path or fail the write, and `\` would even escape the temp dir via
/// `Path::join`. Non-ASCII stays intact so the name the editor shows
/// remains recognizable. A UUID tag keeps concurrent edits of same-named
/// files apart; long names truncate keeping the extension so the OS
/// name-length limit (255) is never hit and file-type association works.
pub(crate) fn edit_temp_file(remote_path: &str) -> (String, std::path::PathBuf) {
    let label = remote_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(remote_path)
        .to_string();
    let mut safe: String = label
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    const MAX_CHARS: usize = 120;
    if safe.chars().count() > MAX_CHARS {
        // Keep a short extension on the truncated name: the earliest dot
        // whose suffix fits 16 bytes, so ".tar.gz" survives whole.
        let ext: String = safe
            .char_indices()
            .filter(|&(_, c)| c == '.')
            .map(|(i, _)| i)
            .find(|&i| safe.len() - i <= 16)
            .map(|i| safe[i..].to_string())
            .unwrap_or_default();
        safe = safe
            .chars()
            .take(MAX_CHARS - ext.chars().count())
            .collect::<String>()
            + &ext;
    }
    if safe.is_empty() {
        safe.push('_');
    }
    let temp_path = std::env::temp_dir().join(format!("oryxis-{}-{}", uuid::Uuid::new_v4(), safe));
    (label, temp_path)
}

/// Parse a comma-separated tag field: trim, drop empties, dedup
/// case-insensitively while keeping the first spelling and the typed
/// order. Shared by the snippet and host editors.
pub(crate) fn parse_tags(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in input.split(',') {
        let tag = raw.trim();
        if tag.is_empty() {
            continue;
        }
        if !out.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            out.push(tag.to_string());
        }
    }
    out
}

/// Snippet variable placeholders: `{name}` / `{name:default}` where
/// `name` starts with a letter or `_` and continues with `[\w-]`.
/// Deliberately narrow so shell text never trips it: `${VAR}` (the
/// brace is preceded by `$`), `{}` (find/awk, empty name), `{print $1}`
/// (spaces / `$` in the name) all pass through untouched. Returns the
/// distinct placeholders in first-appearance order, first default wins.
pub(crate) fn snippet_placeholders(body: &str) -> Vec<(String, String)> {
    let bytes = body.as_bytes();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' || (i > 0 && bytes[i - 1] == b'$') {
            i += 1;
            continue;
        }
        let Some(close_rel) = body[i + 1..].find('}') else { break };
        let inner = &body[i + 1..i + 1 + close_rel];
        let (name, default) = match inner.split_once(':') {
            Some((n, d)) => (n, d),
            None => (inner, ""),
        };
        let valid = !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if valid {
            if !out.iter().any(|(n, _)| n == name) {
                out.push((name.to_string(), default.to_string()));
            }
            i += close_rel + 2;
        } else {
            i += 1;
        }
    }
    out
}

/// Substitute every `{name}` / `{name:*}` occurrence with its value
/// (same validity rules as [`snippet_placeholders`]; unknown names and
/// shell-shaped braces stay literal).
pub(crate) fn substitute_snippet_vars(body: &str, vars: &[(String, String)]) -> String {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{'
            && !(i > 0 && bytes[i - 1] == b'$')
            && let Some(close_rel) = body[i + 1..].find('}')
        {
            let inner = &body[i + 1..i + 1 + close_rel];
            let name = inner.split_once(':').map(|(n, _)| n).unwrap_or(inner);
            if let Some((_, value)) = vars.iter().find(|(n, _)| n == name) {
                out.push_str(value);
                i += close_rel + 2;
                continue;
            }
        }
        let ch = body[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

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

/// What a terminal right-click does (PuTTY's three schemes). Persisted
/// as `code()`; maps to `oryxis_terminal::RightClickAction` at build.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum RightClickMode {
    /// Open a context menu (Copy All / Paste / Clear Scrollback).
    Menu,
    /// Paste the clipboard (current default; also the only mode where
    /// the copy-on-select "copy on right-click" sub-option applies).
    #[default]
    Paste,
    /// Extend the current selection to the click point, then copy (xterm).
    Extend,
}

impl RightClickMode {
    pub(crate) const ALL: [RightClickMode; 3] = [
        RightClickMode::Menu,
        RightClickMode::Paste,
        RightClickMode::Extend,
    ];

    pub(crate) fn code(self) -> &'static str {
        match self {
            RightClickMode::Menu => "menu",
            RightClickMode::Paste => "paste",
            RightClickMode::Extend => "extend",
        }
    }

    pub(crate) fn from_code(code: &str) -> Self {
        match code {
            "menu" => RightClickMode::Menu,
            "extend" => RightClickMode::Extend,
            _ => RightClickMode::Paste,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            RightClickMode::Menu => "right_click_menu",
            RightClickMode::Paste => "right_click_paste",
            RightClickMode::Extend => "right_click_extend",
        }
    }

    pub(crate) fn to_widget(self) -> oryxis_terminal::RightClickAction {
        match self {
            RightClickMode::Menu => oryxis_terminal::RightClickAction::Menu,
            RightClickMode::Paste => oryxis_terminal::RightClickAction::Paste,
            RightClickMode::Extend => oryxis_terminal::RightClickAction::Extend,
        }
    }
}

/// How terminal teaching hints (the "hold Shift to select" mouse-capture
/// toast, the "hold Ctrl and click" link toast) are surfaced. Default
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

/// The connection's address as the UI shows it: `user@host[:port]` for
/// SSH / Telnet (each protocol's default port omitted, so the common case
/// stays short), and `port @ baud` for a serial line, where a TCP port and
/// a username are meaningless.
///
/// Shared by the dashboard card subtitle (`show_host_address`) and the
/// tab's second line (`show_tab_host_address`) so the two can never
/// disagree about what a host's address looks like. Privacy masking is
/// the caller's, because the two surfaces reveal on different gestures
/// (card hover vs tab hover).
pub(crate) fn host_address_label(conn: &oryxis_core::models::Connection) -> String {
    use oryxis_core::models::connection::ConnectionProtocol;
    match conn.protocol {
        ConnectionProtocol::Serial => {
            let baud = conn.serial.map(|s| s.baud).unwrap_or(9600);
            format!("{} @ {}", conn.hostname, baud)
        }
        _ => {
            let default_port = conn.protocol.default_port().unwrap_or(22);
            let port_part = if conn.port == default_port {
                String::new()
            } else {
                format!(":{}", conn.port)
            };
            format!(
                "{}@{}{}",
                conn.username.as_deref().unwrap_or("root"),
                conn.hostname,
                port_part
            )
        }
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
        AuthMethod::PasswordPrompt => t("auth_password_prompt"),
        AuthMethod::Certificate => t("auth_certificate"),
    }
    .to_string()
}

// ── C5 quirk pick-list labels (mirror `auth_method_label`: the enums
//    live in oryxis-core with English `Display`; the picker uses these
//    localized labels and the dispatch handlers map them back). ──

pub(crate) fn quirk_backspace_label(
    m: oryxis_core::models::terminal_quirks::BackspaceMode,
) -> String {
    use crate::i18n::t;
    use oryxis_core::models::terminal_quirks::BackspaceMode;
    match m {
        BackspaceMode::Del127 => t("quirks_backspace_del"),
        BackspaceMode::CtrlH => t("quirks_backspace_ctrl_h"),
    }
    .to_string()
}

pub(crate) fn quirk_home_end_label(
    m: oryxis_core::models::terminal_quirks::HomeEndMode,
) -> String {
    use crate::i18n::t;
    use oryxis_core::models::terminal_quirks::HomeEndMode;
    match m {
        HomeEndMode::Standard => t("quirks_home_end_standard"),
        HomeEndMode::Rxvt => t("quirks_home_end_rxvt"),
    }
    .to_string()
}

pub(crate) fn quirk_fn_keys_label(
    m: oryxis_core::models::terminal_quirks::FunctionKeyMode,
) -> String {
    use crate::i18n::t;
    use oryxis_core::models::terminal_quirks::FunctionKeyMode;
    match m {
        FunctionKeyMode::Xterm => t("quirks_fn_xterm"),
        FunctionKeyMode::LinuxConsole => t("quirks_fn_linux"),
        FunctionKeyMode::Vt400 => t("quirks_fn_vt400"),
        FunctionKeyMode::Rxvt => t("quirks_fn_rxvt"),
    }
    .to_string()
}

pub(crate) fn quirk_option_as_meta_label(
    m: oryxis_core::models::terminal_quirks::OptionAsMeta,
) -> String {
    use crate::i18n::t;
    use oryxis_core::models::terminal_quirks::OptionAsMeta;
    match m {
        OptionAsMeta::None => t("quirks_option_meta_off"),
        OptionAsMeta::OnlyLeft => t("quirks_option_meta_left"),
        OptionAsMeta::OnlyRight => t("quirks_option_meta_right"),
        OptionAsMeta::Both => t("quirks_option_meta_both"),
    }
    .to_string()
}

pub(crate) fn quirk_option_as_meta_from_label(
    v: &str,
) -> oryxis_core::models::terminal_quirks::OptionAsMeta {
    use oryxis_core::models::terminal_quirks::OptionAsMeta;
    [
        OptionAsMeta::None,
        OptionAsMeta::OnlyLeft,
        OptionAsMeta::OnlyRight,
        OptionAsMeta::Both,
    ]
    .into_iter()
    .find(|m| v == quirk_option_as_meta_label(*m))
    .unwrap_or(OptionAsMeta::None)
}

pub(crate) fn quirk_backspace_from_label(
    v: &str,
) -> oryxis_core::models::terminal_quirks::BackspaceMode {
    use oryxis_core::models::terminal_quirks::BackspaceMode;
    if v == quirk_backspace_label(BackspaceMode::CtrlH) {
        BackspaceMode::CtrlH
    } else {
        BackspaceMode::Del127
    }
}

pub(crate) fn quirk_home_end_from_label(
    v: &str,
) -> oryxis_core::models::terminal_quirks::HomeEndMode {
    use oryxis_core::models::terminal_quirks::HomeEndMode;
    if v == quirk_home_end_label(HomeEndMode::Rxvt) {
        HomeEndMode::Rxvt
    } else {
        HomeEndMode::Standard
    }
}

pub(crate) fn quirk_fn_keys_from_label(
    v: &str,
) -> oryxis_core::models::terminal_quirks::FunctionKeyMode {
    use oryxis_core::models::terminal_quirks::FunctionKeyMode;
    [
        FunctionKeyMode::Xterm,
        FunctionKeyMode::LinuxConsole,
        FunctionKeyMode::Vt400,
        FunctionKeyMode::Rxvt,
    ]
    .into_iter()
    .find(|m| v == quirk_fn_keys_label(*m))
    .unwrap_or(FunctionKeyMode::Xterm)
}

pub(crate) fn quirk_osc52_label(
    m: Option<oryxis_core::models::terminal_quirks::Osc52Override>,
) -> String {
    use crate::i18n::t;
    use oryxis_core::models::terminal_quirks::Osc52Override;
    match m {
        Some(Osc52Override::On) => t("quirks_osc52_on"),
        Some(Osc52Override::Off) => t("quirks_osc52_off"),
        None => t("quirks_osc52_default"),
    }
    .to_string()
}

pub(crate) fn quirk_osc52_from_label(
    v: &str,
) -> Option<oryxis_core::models::terminal_quirks::Osc52Override> {
    use oryxis_core::models::terminal_quirks::Osc52Override;
    [Some(Osc52Override::On), Some(Osc52Override::Off)]
        .into_iter()
        .find(|m| v == quirk_osc52_label(*m))
        // Unknown labels (including the localized "Default") inherit the
        // global policy, mirroring the tri-state pick semantics.
        .unwrap_or(None)
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
    } else if v == t("auth_password_prompt") || v == "PasswordPrompt" {
        AuthMethod::PasswordPrompt
    } else if v == t("auth_certificate") || v == "Certificate" {
        AuthMethod::Certificate
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
        AuthMethod::PasswordPrompt => "PasswordPrompt",
        AuthMethod::Certificate => "Certificate",
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
        "PasswordPrompt" => AuthMethod::PasswordPrompt,
        "Certificate" => AuthMethod::Certificate,
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
    use std::time::Duration;

    #[test]
    fn snippet_placeholders_match_only_variable_shapes() {
        let ph = snippet_placeholders(
            "deploy {env:prod} --tag {tag} on ${HOME} with {} and {print $1} again {env}",
        );
        assert_eq!(
            ph,
            vec![
                ("env".to_string(), "prod".to_string()),
                ("tag".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn snippet_substitution_replaces_every_occurrence() {
        let vars = vec![
            ("env".to_string(), "staging".to_string()),
            ("tag".to_string(), "v2".to_string()),
        ];
        assert_eq!(
            substitute_snippet_vars("do {env} {tag} ${HOME} {env:prod}", &vars),
            "do staging v2 ${HOME} staging"
        );
    }

    #[test]
    fn parse_tags_trims_dedups_and_keeps_order() {
        assert_eq!(
            parse_tags(" prod, web ,PROD,, db "),
            vec!["prod", "web", "db"]
        );
        assert!(parse_tags("  ,, ").is_empty());
    }

    #[test]
    fn quirk_label_round_trips_for_every_variant() {
        use oryxis_core::models::terminal_quirks::{
            BackspaceMode, FunctionKeyMode, HomeEndMode, OptionAsMeta, Osc52Override,
        };
        // label(m) -> from_label -> m must be the identity for every
        // variant, so the host-editor pick can't silently map a mode to
        // the wrong enum (the reverse of the label helpers).
        for m in [BackspaceMode::Del127, BackspaceMode::CtrlH] {
            assert_eq!(quirk_backspace_from_label(&quirk_backspace_label(m)), m);
        }
        for m in [HomeEndMode::Standard, HomeEndMode::Rxvt] {
            assert_eq!(quirk_home_end_from_label(&quirk_home_end_label(m)), m);
        }
        for m in [
            FunctionKeyMode::Xterm,
            FunctionKeyMode::LinuxConsole,
            FunctionKeyMode::Vt400,
            FunctionKeyMode::Rxvt,
        ] {
            assert_eq!(quirk_fn_keys_from_label(&quirk_fn_keys_label(m)), m);
        }
        for m in [
            OptionAsMeta::None,
            OptionAsMeta::OnlyLeft,
            OptionAsMeta::OnlyRight,
            OptionAsMeta::Both,
        ] {
            assert_eq!(quirk_option_as_meta_from_label(&quirk_option_as_meta_label(m)), m);
        }
        for m in [Some(Osc52Override::On), Some(Osc52Override::Off), None] {
            assert_eq!(quirk_osc52_from_label(&quirk_osc52_label(m)), m);
        }
        // An unknown label falls back to the default (never panics).
        assert_eq!(quirk_backspace_from_label("garbage"), BackspaceMode::Del127);
        assert_eq!(quirk_fn_keys_from_label("garbage"), FunctionKeyMode::Xterm);
        assert_eq!(quirk_option_as_meta_from_label("garbage"), OptionAsMeta::None);
        assert_eq!(quirk_osc52_from_label("garbage"), None);
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
            AuthMethod::PasswordPrompt,
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

    #[test]
    fn edit_temp_file_sanitizes_hostile_basenames() {
        // The label keeps the remote name verbatim; the temp file name
        // must not let separators or reserved characters through.
        let (label, path) = edit_temp_file("/etc/we\"ird\\name:file.txt");
        assert_eq!(label, "we\"ird\\name:file.txt");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("we_ird_name_file.txt"), "{name}");
        // A backslash in the name must never escape the temp dir.
        assert_eq!(path.parent().unwrap(), std::env::temp_dir());

        let (_, path) = edit_temp_file("/tmp/../../x");
        assert_eq!(path.parent().unwrap(), std::env::temp_dir());
    }

    #[test]
    fn edit_temp_file_truncates_keeping_extension() {
        let long = format!("/d/{}.tar.gz", "a".repeat(300));
        let (label, path) = edit_temp_file(&long);
        assert_eq!(label.chars().count(), 307);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with(".tar.gz"));
        // uuid prefix (43 chars incl. separators) + capped stem.
        assert!(name.chars().count() <= 43 + 120 + 1);

        // Unicode names survive intact.
        let (_, path) = edit_temp_file("/srv/配置ファイル.yaml");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("配置ファイル.yaml"));
    }
}
