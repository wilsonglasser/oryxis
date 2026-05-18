//! `Oryxis::subscription`, the iced event/timer multiplexer. Pulled
//! out of `app.rs` so the message-loop module is more browsable.

use std::sync::atomic::{AtomicI32, Ordering};

use iced::Subscription;

use crate::app::{Message, Oryxis};

// Coarse-grained record of the last cursor position forwarded to the
// message loop. The subscription closure quantises to a 4 px grid and
// drops events that resolve to the same cell as the previous forward,
// so iced's bounded subscription channel can't be drowned by 100 Hz
// mouse-move bursts on dense pages (keychain grid, SFTP listing).
// Using i32 lets us store the snapped coords with one atomic each
// rather than reaching for a Mutex<Point>.
static LAST_MOUSE_X: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_MOUSE_Y: AtomicI32 = AtomicI32::new(i32::MIN);

impl Oryxis {
    pub fn subscription(&self) -> Subscription<Message> {
        let events = iced::event::listen_with(|event, _status, _window| {
            match event {
                iced::event::Event::Keyboard(ke) => Some(Message::KeyboardEvent(ke)),
                iced::event::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    // Quantise to a 4 px grid. Same cell as last forward
                    // → drop the event before it hits the subscription
                    // channel. Drag handlers that need pixel precision
                    // recover the exact cursor coord from iced's own
                    // event state on the next non-debounced sample.
                    const SNAP: f32 = 4.0;
                    let sx = (position.x / SNAP).round() as i32;
                    let sy = (position.y / SNAP).round() as i32;
                    let prev_x = LAST_MOUSE_X.swap(sx, Ordering::Relaxed);
                    let prev_y = LAST_MOUSE_Y.swap(sy, Ordering::Relaxed);
                    if sx == prev_x && sy == prev_y {
                        return None;
                    }
                    Some(Message::MouseMoved(position))
                }
                // Global Left press, used to start a potential SFTP
                // internal drag. Doesn't capture the event, so widget-
                // level handlers (button click, etc.) still fire.
                iced::event::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Left,
                )) => Some(Message::SftpMouseLeftPressed),
                // Global mouse-up so the sidebar resize stops even when the
                // cursor leaves the resize handle while the user is dragging.
                // Same handler also closes any active SFTP internal drag.
                iced::event::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::ChatSidebarResizeStop),
                iced::event::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size))
                }
                // OS-level file drag-and-drop. iced fires one event per
                // file, so multi-file drops produce a sequence of
                // `FileDropped` messages, they're just queued through
                // the SFTP upload handler.
                iced::event::Event::Window(iced::window::Event::FileHovered(_)) => {
                    Some(Message::SftpFileHovered)
                }
                iced::event::Event::Window(iced::window::Event::FilesHoveredLeft) => {
                    Some(Message::SftpFilesHoveredLeft)
                }
                iced::event::Event::Window(iced::window::Event::FileDropped(path)) => {
                    Some(Message::SftpFileDropped(path))
                }
                _ => None,
            }
        });
        // 30-second poll for silent auto-reconnect of disconnected SSH tabs.
        let auto_reconnect = iced::time::every(std::time::Duration::from_secs(30))
            .map(|_| Message::AutoReconnectTick);

        // 100 ms tick that drives the pulsing "loading" ring on the active
        // connection step. Only runs while a connection is in progress and
        // hasn't failed, no perpetual re-renders on idle.
        let mut subs = vec![events, auto_reconnect];
        let is_connecting = self
            .connecting
            .as_ref()
            .map(|p| !p.failed)
            .unwrap_or(false);
        if is_connecting {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(100))
                    .map(|_| Message::ConnectAnimTick),
            );
        }
        // 2s mtime poll on the edit-in-place temp file, only ticks
        // while a session is actually active, otherwise idle.
        if self.sftp.edit_session.is_some() {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(2))
                    .map(|_| Message::SftpEditWatchTick),
            );
        }
        // Tray icon event drain. On Windows the tray-icon crate runs
        // its own thread that pushes menu / icon events into a pair
        // of crossbeam channels; the dispatcher's `TrayPoll` handler
        // calls `tray::poll_*` to drain them. 100 ms is the same
        // cadence Tauri uses internally for the same job. On non-
        // Windows targets the polls are no-ops, so mounting the
        // subscription unconditionally costs only the timer thread,
        // which iced shares across all `time::every` ticks anyway.
        subs.push(
            iced::time::every(std::time::Duration::from_millis(100))
                .map(|_| Message::TrayPoll),
        );

        // Cloud auto-refresh ticker. Only mounts the subscription when
        // the user enabled the toggle in Settings; otherwise zero
        // background API calls. Interval reads the persisted setting
        // and falls back to 30 min on any parse failure so a malformed
        // value doesn't pin the ticker at 1 ms.
        if self.setting_cloud_auto_refresh_enabled && !self.cloud_profiles.is_empty() {
            let minutes = self
                .setting_cloud_auto_refresh_interval_minutes
                .parse::<u64>()
                .ok()
                .filter(|m| *m > 0)
                .unwrap_or(30);
            subs.push(
                iced::time::every(std::time::Duration::from_secs(minutes * 60))
                    .map(|_| Message::CloudAutoRefreshTick),
            );
        }
        Subscription::batch(subs)
    }
}
