/// Bracketed-paste start marker (`ESC [ 200 ~`).
const PASTE_START: &[u8] = b"\x1b[200~";
/// Bracketed-paste end marker (`ESC [ 201 ~`).
const PASTE_END: &[u8] = b"\x1b[201~";

/// Prepare clipboard text for writing to a terminal session.
///
/// When `bracketed` is true (the focused app enabled DECSET 2004), wrap the
/// payload in `ESC [ 200 ~` ... `ESC [ 201 ~` so readline / TUI programs
/// (bash, zsh, Codex CLI, ...) treat the whole block as one paste and only
/// submit when the user presses Enter, instead of one submit per embedded
/// newline. Any marker already present in the clipboard is stripped first so
/// the payload can't prematurely close (or reopen) the bracket.
///
/// When `bracketed` is false the text is returned unchanged, so plain shells
/// that never requested the mode are unaffected.
pub fn wrap_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }
    let sanitized = text.replace("\x1b[200~", "").replace("\x1b[201~", "");
    let mut out = Vec::with_capacity(sanitized.len() + PASTE_START.len() + PASTE_END.len());
    out.extend_from_slice(PASTE_START);
    out.extend_from_slice(sanitized.as_bytes());
    out.extend_from_slice(PASTE_END);
    out
}

/// Write `text` to the system clipboard, best-effort. Errors are swallowed
/// (a backend may be unavailable on a headless box or under a compositor
/// without the data-control protocol); a failed copy should never panic
/// the UI. Shared by the copy-on-select, right-click-copy and Ctrl+Shift+C
/// paths so the three sites stay in sync.
pub(crate) fn set_clipboard_text(text: &str) {
    if let Ok(mut clip) = arboard::Clipboard::new() {
        let _ = clip.set_text(text);
    }
}

/// Best-effort spawn of the OS default handler for a URL. Runs detached; the
/// terminal widget never blocks on it and errors are swallowed, a failed
/// launch just means nothing happens visibly, same as any other click miss.
pub(crate) fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW so the `cmd /C start` shim doesn't flash a
        // console window on the GUI-subsystem app.
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod paste_tests {
    use super::wrap_paste;

    #[test]
    fn raw_when_mode_disabled() {
        let text = "line one\nline two\n";
        assert_eq!(wrap_paste(text, false), text.as_bytes());
    }

    #[test]
    fn wraps_when_mode_enabled() {
        let out = wrap_paste("hello\nworld", true);
        assert_eq!(out, b"\x1b[200~hello\nworld\x1b[201~");
    }

    #[test]
    fn strips_embedded_markers_so_payload_cannot_break_out() {
        // A clipboard carrying its own bracket markers must not be able to
        // close the bracket early or open a nested one.
        let out = wrap_paste("a\x1b[201~b\x1b[200~c", true);
        assert_eq!(out, b"\x1b[200~abc\x1b[201~");
    }
}
