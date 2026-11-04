//! `Oryxis::handle_global`: cross-cutting message arms handled outside
//! any single domain (clipboard, URL open, toasts, error dialog, privacy
//! reveal toggle, no-op). Routed here explicitly from `dispatch_message`.
#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]

use iced::Task;

use crate::app::{Message, Oryxis};

/// Read the system clipboard as text, best-effort, as a `Task`.
///
/// The single read entry point for the whole app. Reads MUST go through the
/// iced runtime: it serves them one at a time on its own worker thread, and a
/// second concurrent clipboard open in the same process is fatal on Windows
/// (`STATUS_HEAP_CORRUPTION` inside `user32!GetClipboardData`, killing the
/// process with no panic and no log). Never call `arboard` directly.
/// See `oryxis_terminal::host_clipboard` for the field crash.
pub(crate) fn read_clipboard_text(
    to_message: impl Fn(Option<String>) -> Message + Send + 'static,
) -> Task<Message> {
    iced::clipboard::read_text().map(move |result| {
        to_message(result.ok().map(|text| text.to_string()))
    })
}

/// Write `text` to the system clipboard as a `Task`. Same rule as
/// [`read_clipboard_text`]: the runtime owns the clipboard, we only ask.
pub(crate) fn write_clipboard_text(text: String) -> Task<Message> {
    iced::clipboard::write(text).discard()
}

/// Read the PRIMARY selection as text, best-effort, as a `Task`.
///
/// Only meaningful where [`oryxis_terminal::has_primary_selection`] holds:
/// elsewhere the runtime serves PRIMARY from the ordinary clipboard, so a
/// caller that skipped the gate would silently read (and, on the write side,
/// overwrite) the user's Ctrl+C.
pub(crate) fn read_primary_text(
    to_message: impl Fn(Option<String>) -> Message + Send + 'static,
) -> Task<Message> {
    iced::clipboard::read_primary(iced::clipboard::Kind::Text).map(move |result| {
        to_message(match result.as_deref() {
            Ok(iced::clipboard::Content::Text(text)) => Some(text.clone()),
            _ => None,
        })
    })
}

/// Perform the clipboard work `oryxis-terminal` queued for us (copy-on-select,
/// the copy chord, right-click copy, OSC 52 store / load, the widget's paste
/// fallbacks). Drained once per `update()` so the widget layer never needs a
/// clipboard of its own.
///
/// The requests are `Task::chain`ed, not batched: a batch runs its streams
/// concurrently, which would let a queued read answer before a queued write
/// has landed. Order matters because a remote can emit an OSC 52 store and an
/// OSC 52 query inside the same output batch (tmux / vim clipboard
/// integration does), and the query must see the text the store just set.
pub(crate) fn serve_terminal_clipboard_requests() -> Option<Task<Message>> {
    use oryxis_terminal::ClipboardRequest;

    oryxis_terminal::take_clipboard_requests()
        .into_iter()
        .map(|request| match request {
            ClipboardRequest::Write(text) => write_clipboard_text(text),
            // Queued only where a PRIMARY selection exists (the terminal
            // crate gates it), so this never lands on the clipboard.
            ClipboardRequest::WritePrimary(text) => {
                iced::clipboard::write_primary(text).discard()
            }
            // The sink is the terminal crate's own closure (an OSC 52 reply,
            // a PTY paste); it only formats text and writes bytes to a
            // channel, so running it in the task chain is safe.
            ClipboardRequest::Read(sink) => iced::clipboard::read_text().then(move |result| {
                let text = result.map(|t| t.to_string()).unwrap_or_default();
                sink.deliver(&text);
                Task::none()
            }),
        })
        .reduce(Task::chain)
}

/// A native OS notification waiting for the end of the current `update`,
/// where it is handed to a blocking task off the UI thread.
///
/// `fallback` is the in-app toast to raise if the OS refuses to carry it
/// (no notification daemon on Linux, no AppUserModelID on a
/// non-installed Windows build); `None` when the caller keeps a
/// persistent in-app record of its own and needs no stand-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OsNotice {
    pub title: String,
    pub body: String,
    pub fallback: Option<String>,
}

impl Oryxis {
    /// Hand the OS a notice about something that finished while the
    /// user was away from the window, and say whether it was handed over.
    ///
    /// A file transfer is the long operation the user is EXPECTED to
    /// walk away from, which is what makes it worth a native popup:
    /// SFTP queues (both directions, both browsers) and ZMODEM alike.
    /// In front of the window it is noise, because every one of those
    /// surfaces already shows its own progress, so this refuses there
    /// and the caller keeps whatever in-app feedback it had.
    ///
    /// `false` means the window is focused and nothing was queued, so a
    /// caller with an in-app fallback shows it now. `true` means the
    /// notice is queued for the OS; whether the OS carries it is only
    /// known later, off the UI thread, and a refusal raises `fallback`
    /// as a toast then (see [`Oryxis::take_os_notice_tasks`]).
    ///
    /// No setting gates this: `terminal_notification` governs notices
    /// a remote SHELL asks for, which is a different question from
    /// whether the user's own transfer is done.
    pub(crate) fn notify_away(
        &mut self,
        title: &str,
        body: &str,
        fallback: Option<String>,
    ) -> bool {
        if self.cur_focused() {
            return false;
        }
        // One line, and never the full path of a deep tree: the OS
        // decides how much of a body it shows, and a truncated middle
        // reads worse than a short line.
        self.push_os_notice(title, &crate::util::truncate_middle(body, 120), fallback);
        true
    }

    /// Queue a native OS notification for the end of this `update`.
    ///
    /// Nothing is shown here. `util::show_os_notification` blocks the
    /// thread it runs on (macOS spins the main run loop while it waits
    /// for the delivery callback, Linux makes a D-Bus round trip), and
    /// inside `update` that thread is the one drawing the window and
    /// hosting winit's event handler, so the call is left to the
    /// blocking task [`Oryxis::take_os_notice_tasks`] spawns.
    pub(crate) fn push_os_notice(&mut self, title: &str, body: &str, fallback: Option<String>) {
        self.os_notices.push(OsNotice {
            title: title.to_string(),
            body: body.to_string(),
            fallback,
        });
    }

    /// Drain the notices queued this cycle into one blocking task each.
    ///
    /// Called from `update` after every message, next to the terminal
    /// clipboard requests: one funnel, so no caller can hand the OS a
    /// notification from the UI thread. A task reports back only when
    /// the OS refused, as a `ToastShow` of the notice's fallback.
    pub(crate) fn take_os_notice_tasks(&mut self) -> Vec<Task<Message>> {
        std::mem::take(&mut self.os_notices)
            .into_iter()
            .map(|notice| {
                Task::perform(
                    async move {
                        let OsNotice { title, body, fallback } = notice;
                        let shown = tokio::task::spawn_blocking(move || {
                            crate::util::show_os_notification(&title, &body)
                        })
                        .await
                        .unwrap_or(false);
                        if shown { None } else { fallback }
                    },
                    |fallback| match fallback {
                        Some(text) => Message::ToastShow(text),
                        None => Message::NoOp,
                    },
                )
            })
            .collect()
    }

    pub(crate) fn handle_global(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TogglePrivacyReveal => {
                self.vaults.cur_mut().privacy.revealed = !self.vaults.cur_mut().privacy.revealed;
            }

            Message::OpenUrl(url) => {
                if let Err(e) = crate::util::open_in_browser(&url) {
                    tracing::warn!("open_in_browser({url}) failed: {e}");
                }
            }
            Message::CopyToClipboard(content) => {
                // The write is served by the runtime (one clipboard access per
                // process at a time), so its result arrives as
                // `ClipboardWritten` and the toast waits for it: a failed copy
                // must not claim success.
                return iced::clipboard::write(content)
                    .map(|result| Message::ClipboardWritten(result.is_ok()));
            }
            Message::ClipboardWritten(ok) => {
                if !ok {
                    tracing::warn!("clipboard write failed");
                    return Task::none();
                }
                self.set_toast(crate::i18n::t("copied_to_clipboard").to_string());
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                    },
                    |_| Message::ToastClear,
                );
            }

            Message::ToastClear => {
                // Deadline-guarded auto-dismiss (subscription tick or a
                // legacy scheduled sleep-timer). Only the current toast's
                // own elapsed deadline clears it, so a superseded timer can
                // never wipe a newer toast.
                // Never while the window is unfocused: a toast raised then
                // (the fallback for an OS notification the desktop refused
                // is the usual one) would be gone before anyone looked.
                // The focus-return handler re-stamps the deadline.
                if self.cur_focused()
                    && self
                        .toast
                        .as_ref()
                        .and_then(|toast| toast.deadline)
                        .is_some_and(|d| std::time::Instant::now() >= d)
                {
                    self.toast = None;
                }
            }

            Message::ToastDismiss => {
                // Explicit click on the chip: clear immediately.
                self.toast = None;
            }

            Message::ToastShow(text) => {
                // The in-app stand-in for an OS notification the OS
                // refused, reported by the blocking task that asked it.
                self.set_toast(text);
            }

            Message::ErrorDialogRunAction => {
                if let Some(dialog) = self.error_dialog.take()
                    && let Some(action) = dialog.action
                {
                    return self.update(*action.message);
                }
            }

            Message::ErrorDialogDismiss => {
                self.error_dialog = None;
            }
            Message::NoOp => {}

            // `update` lists the cross-cutting globals explicitly, so
            // anything else here is a routing mistake, not a runtime
            // case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    /// `util::show_os_notification` blocks the thread it runs on, and on
    /// the UI thread that thread hosts winit's event handler, so the only
    /// place allowed to call it is the blocking task `take_os_notice_tasks`
    /// spawns. A new call site anywhere else is the regression this pins.
    #[test]
    fn show_os_notification_is_called_only_from_the_blocking_task() {
        // Split so this test's own source does not match itself.
        let needle = ["show_os_", "notification("].concat();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root.clone()];
        let mut call_sites: Vec<String> = Vec::new();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("read source");
                for (idx, line) in src.lines().enumerate() {
                    // The two `fn` definitions in `util.rs` (one per OS
                    // family) are not calls.
                    if line.contains(&needle) && !line.contains(&format!("fn {needle}")) {
                        let rel = path.strip_prefix(&root).expect("under src");
                        call_sites.push(format!("{}:{}", rel.display(), idx + 1));
                    }
                }
            }
        }
        call_sites.sort();
        assert_eq!(
            call_sites.len(),
            1,
            "show_os_notification is called from {call_sites:?}; queue through \
             Oryxis::push_os_notice / notify_away instead"
        );
        assert!(
            call_sites[0].starts_with("dispatch_global.rs:"),
            "the one call site must be take_os_notice_tasks, found {call_sites:?}"
        );
    }
}
