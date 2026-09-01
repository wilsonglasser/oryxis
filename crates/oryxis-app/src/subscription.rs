//! `Oryxis::subscription`, the iced event/timer multiplexer. Pulled
//! out of `app.rs` so the message-loop module is more browsable.

use std::sync::atomic::{AtomicI32, Ordering};

use iced::Subscription;

use crate::app::{SftpMessage, SettingsMessage, TabsMessage, TerminalMessage, CloudMessage, PortForwardMessage, AiMessage, SyncMessage, PlayerMessage, Message, Oryxis};
#[cfg(target_os = "windows")]
use crate::app::TrayMessage;

// Coarse-grained record of the last cursor position forwarded to the
// message loop. The subscription closure quantises to a 4 px grid and
// drops events that resolve to the same cell as the previous forward,
// so iced's bounded subscription channel can't be drowned by 100 Hz
// mouse-move bursts on dense pages (keychain grid, SFTP listing).
// Using i32 lets us store the snapped coords with one atomic each
// rather than reaching for a Mutex<Point>.
static LAST_MOUSE_X: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_MOUSE_Y: AtomicI32 = AtomicI32::new(i32::MIN);

// Interest gate for cursor-move forwarding. In iced, every forwarded
// message goes through `update()` and forces a full view() rebuild +
// layout + redraw, so streaming CursorMoved into the app re-renders the
// whole UI at mouse-move frequency (60-125 Hz) even when nothing the
// view draws depends on the position. Only a handful of app states
// genuinely consume continuous positions (active drags, the fullscreen
// top-zone reveal, the post-keyboard-nav hover restore), so the end of
// every `update()` recomputes this flag from that state
// (`Oryxis::mouse_interest`) and the listener below drops CursorMoved
// before it ever becomes a message while the flag is off. Widget-level
// hover (buttons, tooltips, the terminal canvas) rides iced's internal
// event path and keeps working regardless; this gate only affects the
// app-message lane.
static MOUSE_INTEREST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

// The live (raw, unsnapped) cursor position, updated on every
// CursorMoved even while `MOUSE_INTEREST` is off, stored as f32 bits.
// `Oryxis::update` syncs `self.mouse_position` from here at the top of
// every message, so click-time readers (drag press anchors, the kebab
// menu position) always see a fresh position without the app paying a
// re-render per mouse move. The same sync doubles as the activity
// signal: a position change since the previous message counts as user
// input for the vault auto-lock idle clock.
static LIVE_MOUSE_X: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static LIVE_MOUSE_Y: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Publish whether the app currently needs continuous cursor positions.
/// Called at the end of every `update()` pass.
pub(crate) fn set_mouse_interest(on: bool) {
    MOUSE_INTEREST.store(on, Ordering::Relaxed);
}

/// The most recent cursor position seen by the event listener, whether
/// or not it was forwarded as a message.
pub(crate) fn live_mouse_position() -> iced::Point {
    iced::Point {
        x: f32::from_bits(LIVE_MOUSE_X.load(Ordering::Relaxed)),
        y: f32::from_bits(LIVE_MOUSE_Y.load(Ordering::Relaxed)),
    }
}

impl Oryxis {
    pub fn subscription(&self) -> Subscription<Message> {
        let events = iced::event::listen_with(|event, _status, _window| {
            match event {
                iced::event::Event::Keyboard(ke) => Some(Message::Terminal(TerminalMessage::KeyboardEvent(ke))),
                // Text committed by the OS IME (composed CJK characters,
                // etc.). Routed to the active PTY in dispatch_terminal,
                // behind the same focus guards as KeyboardEvent. Preedit /
                // open / close phases are handled by the OS overlay; only
                // the final commit needs forwarding.
                iced::event::Event::InputMethod(
                    iced::advanced::input_method::Event::Commit(text),
                ) => {
                    // IME tracing (debug log, opt-in): whether the OS ever
                    // delivered composition events is exactly what an "IME
                    // types nothing" report (issue #176) needs answered.
                    // Lengths only, never content: commits are what the
                    // user typed.
                    if crate::logging::is_enabled() {
                        tracing::debug!(len = text.chars().count(), "ime-commit delivered");
                    }
                    Some(Message::Terminal(TerminalMessage::TerminalImeCommit(text)))
                }
                // Composition (preedit) updates: pinyin syllables, kana,
                // etc. while an IME is composing. Stored on the focused
                // pane so the `ime_host` overlay draws them at the caret;
                // an empty string (or the IME closing) clears it. The
                // cursor-range hint is ignored: the terminal's caret is
                // where the composed text belongs.
                iced::event::Event::InputMethod(
                    iced::advanced::input_method::Event::Preedit(text, _),
                ) => {
                    if crate::logging::is_enabled() {
                        tracing::debug!(len = text.chars().count(), "ime-preedit delivered");
                    }
                    Some(Message::Terminal(TerminalMessage::TerminalImePreedit(
                        text,
                    )))
                }
                iced::event::Event::InputMethod(iced::advanced::input_method::Event::Opened) => {
                    // No message: the app has nothing to do on open. Traced
                    // because "opened but no preedit ever followed" and "never
                    // opened at all" are different IME failures.
                    if crate::logging::is_enabled() {
                        tracing::debug!("ime-opened");
                    }
                    None
                }
                iced::event::Event::InputMethod(iced::advanced::input_method::Event::Closed) => {
                    if crate::logging::is_enabled() {
                        tracing::debug!("ime-closed");
                    }
                    Some(Message::Terminal(TerminalMessage::TerminalImePreedit(
                        String::new(),
                    )))
                }
                iced::event::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    // Always record the raw position (cheap, no message):
                    // `update()` syncs `self.mouse_position` from these on
                    // the next message, so click-time consumers stay fresh
                    // even while forwarding is gated off.
                    LIVE_MOUSE_X.store(position.x.to_bits(), Ordering::Relaxed);
                    LIVE_MOUSE_Y.store(position.y.to_bits(), Ordering::Relaxed);
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
                    // Nothing in the app consumes continuous positions
                    // right now: drop the event before it becomes a
                    // message (and a full view rebuild).
                    if !MOUSE_INTEREST.load(Ordering::Relaxed) {
                        return None;
                    }
                    Some(Message::Tabs(TabsMessage::MouseMoved(position)))
                }
                // Global Left press, used to start a potential SFTP
                // internal drag. Doesn't capture the event, so widget-
                // level handlers (button click, etc.) still fire.
                iced::event::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Left,
                )) => Some(Message::Sftp(SftpMessage::SftpMouseLeftPressed)),
                // Global mouse-up so the sidebar resize stops even when the
                // cursor leaves the resize handle while the user is dragging.
                // Same handler also closes any active SFTP internal drag.
                iced::event::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::Ai(AiMessage::ChatSidebarResizeStop)),
                // Global Right press, purely to guarantee an `update()` runs
                // after it. The terminal widget's right-click copy (xterm
                // extend-and-copy, and right-click-over-selection under the
                // Paste scheme) CAPTURES the press and publishes nothing, and
                // the queued copy is only performed by the clipboard drain at
                // the end of `update()` (see
                // `dispatch_global::serve_terminal_clipboard_requests`).
                // Without this the copy would sit in the queue until some
                // unrelated message came along.
                iced::event::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Right,
                )) => Some(Message::NoOp),
                // Bindable mouse buttons (middle / back / forward / any
                // extra): the Shortcuts editor RECORDS one here, and a
                // bound SIDE button FIRES from here, which is what makes
                // it work window-wide instead of only over the canvas.
                // Emitted unconditionally rather than gated on app state:
                // this closure is built once and outlives every update,
                // so any captured flag would go stale.
                iced::event::Event::Mouse(iced::mouse::Event::ButtonPressed(button))
                    if crate::hotkeys::MouseButton::from_iced(button).is_some() =>
                {
                    Some(Message::Settings(SettingsMessage::MouseButtonPressed(button)))
                }
                iced::event::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::Tabs(TabsMessage::WindowResized(size)))
                }
                // Outer position in logical desktop coordinates; feeds
                // the persisted geometry so the next launch reopens on
                // the same monitor. Wayland never emits this (positions
                // aren't a thing there), which is fine: the handler just
                // never records one.
                iced::event::Event::Window(iced::window::Event::Moved(pos)) => {
                    Some(Message::Tabs(TabsMessage::WindowMoved(pos)))
                }
                iced::event::Event::Window(iced::window::Event::Focused) => {
                    Some(Message::Tabs(TabsMessage::WindowFocusChanged(true)))
                }
                iced::event::Event::Window(iced::window::Event::Unfocused) => {
                    Some(Message::Tabs(TabsMessage::WindowFocusChanged(false)))
                }
                // OS-level file drag-and-drop. iced fires one event per
                // file, so multi-file drops produce a sequence of
                // `FileDropped` messages, they're just queued through
                // the SFTP upload handler.
                iced::event::Event::Window(iced::window::Event::FileHovered(_)) => {
                    Some(Message::Sftp(SftpMessage::SftpFileHovered))
                }
                iced::event::Event::Window(iced::window::Event::FilesHoveredLeft) => {
                    Some(Message::Sftp(SftpMessage::SftpFilesHoveredLeft))
                }
                iced::event::Event::Window(iced::window::Event::FileDropped(path)) => {
                    Some(Message::Sftp(SftpMessage::SftpFileDropped(path)))
                }
                _ => None,
            }
        });
        let mut subs = vec![events];

        // While a passphrase edit is open, any left click is a candidate
        // for "focus left the field": probe the click position against the
        // field's last drawn bounds, and fall back to a focus query when
        // the geometry is inconclusive or the buffer is stale (a
        // select-all delete clears the display without firing on_input).
        if self.sync.passphrase_editing {
            subs.push(iced::event::listen_with(|event, _status, _window| {
                match event {
                    iced::event::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )) => Some(Message::Sync(SyncMessage::PassphraseBlurCheck)),
                    _ => None,
                }
            }));
        }

        // Stall-watchdog pacemaker (#104): while debug logging is on, a
        // 500 ms NoOp keeps the update heartbeat beating on an idle app,
        // which is what lets the watchdog thread tell "idle" from "the
        // event loop died". Costs nothing with the toggle off.
        if crate::logging::is_enabled() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(500))
                    .map(|_| Message::NoOp),
            );
        }

        // 30-second poll for silent auto-reconnect of disconnected SSH
        // tabs. Unmounted while the vault is locked (soft auto-lock keeps
        // sessions alive): a reconnect needs credentials from the sealed
        // vault and would only burn retry attempts.
        if self.vault_ui.state == crate::state::VaultState::Unlocked {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(30))
                    .map(|_| Message::Settings(SettingsMessage::AutoReconnectTick)),
            );
        }

        // 100 ms tick that drives the pulsing "loading" ring on the active
        // connection step. Only runs while a connection is in progress and
        // hasn't failed, no perpetual re-renders on idle.
        let is_connecting = self
            .connecting
            .as_ref()
            .map(|p| !p.failed)
            .unwrap_or(false);
        if is_connecting {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(100))
                    .map(|_| Message::Settings(SettingsMessage::ConnectAnimTick)),
            );
        }
        // Running-command indicator on the tab strip (issue #146), only
        // while smart tabs can even time a command. Fast while some
        // command already runs past the long-command threshold (drives
        // the marching dots); slow while one runs below it (only has to
        // catch the crossing so the dots appear); idle otherwise.
        if self.prefs.smart_tabs && self.prefs.smart_long_secs > 0 {
            let threshold =
                std::time::Duration::from_secs(u64::from(self.prefs.smart_long_secs));
            let mut any_running = false;
            let mut any_busy = false;
            for pane in self.tabs.iter().flat_map(|t| t.pane_grid.panes.values()) {
                if let Some(run) = &pane.running_cmd {
                    any_running = true;
                    if run.started.elapsed() >= threshold {
                        any_busy = true;
                        break;
                    }
                }
            }
            if any_running {
                let period = if any_busy { 250 } else { 1000 };
                subs.push(
                    iced::time::every(std::time::Duration::from_millis(period))
                        .map(|_| Message::Tabs(crate::app::TabsMessage::BusyAnimTick)),
                );
            }
        }
        // Auto-dismiss the transient toast chip. Only ticks while a toast
        // is shown, otherwise idle; the handler clears it once its
        // deadline passes.
        if self.toast.is_some() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(200))
                    .map(|_| Message::ToastClear),
            );
        }
        // 2s mtime poll on the edit-in-place temp file, only ticks
        // while a session is actually active, otherwise idle. Scans every
        // SFTP surface: the live buffer, parked standalone tabs AND parked
        // hybrid tabs' `files_state` (a hoisted hybrid state lives in
        // `self.sftp` and its slot holds a taken default, so there is no
        // double count), so a backgrounded edit-session keeps watching for
        // external saves no matter which surface owns it.
        // Host monitor: only while its sidebar tab is the visible one and
        // the focused pane's host actually opted in, so an idle screen
        // (or a host that never enabled monitoring) never probes.
        // Unlocked-gated like every other periodic subscription: a soft
        // lock keeps live sessions but must stop reading the host, or
        // the locked screen would still be gathering (and discarding)
        // telemetry behind the lock screen.
        if self.prefs.host_monitoring
            && self.vault_ui.state == crate::state::VaultState::Unlocked
            && (self.monitor_tab_visible()
                || (self.prefs.monitor_status_bar && self.prefs.show_status_bar))
            && self.monitor_target().is_some()
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(
                    self.monitor_interval_secs(),
                ))
                .map(|_| Message::Monitor(crate::app::MonitorMessage::PollHosts)),
            );
        }

        // Multi-host dashboard heartbeat (issue #95): a 1 s tick that
        // staggers the per-host probes, mounted only while the view is
        // actually up. Same Unlocked gate as the sidebar monitor: a
        // soft lock must stop reading the fleet behind the lock screen.
        if self.prefs.host_monitoring
            && self.vault_ui.state == crate::state::VaultState::Unlocked
            && self.active_view == crate::state::View::Monitoring
            && self.active_tab.is_none()
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Monitor(crate::app::MonitorMessage::DashTick)),
            );
        }

        // The latency segment reads the RTT that the in-session probe
        // updates WITHOUT emitting a Message, so on an idle terminal the
        // bar would freeze on the last value forever. A light tick
        // matching the probe cadence re-renders it while visible.
        if self.prefs.show_status_bar
            && self.prefs.status_show_latency
            && self.vault_ui.state == crate::state::VaultState::Unlocked
            && self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .and_then(|t| t.active().session.as_ref().and_then(|s| s.ssh()))
                .is_some_and(|s| s.is_alive())
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::NoOp),
            );
        }

        // The same freeze, for the transport where it matters most. A
        // mosh session that has gone quiet emits NOTHING by definition,
        // so without a tick the one reading that is growing is the one
        // reading that never repaints, and the interface would sit on
        // "no contact 6s" for the rest of the outage. One second rather
        // than three because both surfaces print whole seconds, and a
        // clock that skips two out of every three is a clock that looks
        // broken.
        //
        // Not gated on the status bar the way the SSH tick above is:
        // this state also tints the tab strip's dot, which is on with
        // the bar hidden. Gating it there would leave the dot amber for
        // an outage that ended. Unmounted the moment the focused pane is
        // not on mosh, like every other periodic subscription here.
        if self.vault_ui.state == crate::state::VaultState::Unlocked
            && self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .and_then(|t| t.active().session.as_ref().and_then(|s| s.mosh()))
                .is_some_and(|m| m.is_alive())
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::NoOp),
            );
        }

        // Unlocked-gated like the monitor above: the soft-lock sweep
        // clears every watch, but the gate makes the invariant structural
        // (no save can upload behind the lock screen even if a watch
        // slipped past a future sweep edit).
        if self.vault_ui.state == crate::state::VaultState::Unlocked
            && (!self.sftp.edit_watches.is_empty()
                || self
                    .sftp_tabs
                    .iter()
                    .any(|t| !t.state.edit_watches.is_empty())
                || self
                    .tabs
                    .iter()
                    .any(|t| !t.files_state.edit_watches.is_empty()))
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(2))
                    .map(|_| Message::Sftp(SftpMessage::SftpEditWatchTick)),
            );
        }
        // Live transfer bar: poll the shared byte counter a few times a
        // second while a transfer runs, so the progress bar advances
        // smoothly. Idle otherwise. Scans every SFTP surface (live buffer,
        // parked standalone tabs, parked hybrid tabs' `files_state`) so a
        // backgrounded transfer keeps the bar live when refocused.
        // The sidebar browsers are scanned too, and PER PANE: a split tab
        // can have two of them transferring, and their slots are on the
        // panes, not on the tab. Missing them here would leave the strip's
        // border and the sidebar's own bar both frozen at whatever value
        // the last repaint happened to catch.
        if self.sftp.transfer.state.is_some()
            || self.sftp_tabs.iter().any(|t| t.state.transfer.state.is_some())
            || self.tabs.iter().any(|t| {
                t.files_state.transfer.state.is_some()
                    || t.pane_grid
                        .panes
                        .values()
                        .any(|p| p.files.transfer.state.is_some())
            })
        {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(120))
                    .map(|_| Message::Sftp(SftpMessage::SftpTransferTick)),
            );
        }
        // Intercept the user's close verb (Alt+F4, OS taskbar Close,
        // any path that produces a winit CloseRequested) and route it
        // through the existing WindowClose dispatcher so the close-
        // to-tray check lives in one place. This is the ONLY path that
        // closes the window on those verbs: `window::Settings` sets
        // `exit_on_close_request: false` (see main.rs) so the shell
        // hands us the event instead of acting on it. Keep this
        // subscription unconditional, a gated one would make the
        // window unclosable from the OS.
        subs.push(iced::window::close_requests().map(|_| Message::Tabs(TabsMessage::WindowClose)));

        // Tray icon event drain. On Windows the tray-icon crate runs
        // its own thread that pushes menu / icon events into a pair
        // of crossbeam channels; the dispatcher's `TrayPoll` handler
        // calls `tray::poll_*` to drain them. 100 ms is the same
        // cadence Tauri uses internally for the same job. Windows only.
        //
        // Split into two: a slow heartbeat and an event-driven click
        // path. The old design was a single 100 ms timer, but every tick
        // is a Message through `update()`, which forces a full
        // view()+layout+redraw of the entire app, 10x/s, forever, even
        // idle. On weak GPUs / slow CPUs that constant churn makes the
        // whole UI feel sluggish (scrolling especially).
        //
        // - Heartbeat (500 ms): the multi-window IPC housekeeping that
        //   genuinely needs a timer (rebuild the dynamic submenu when
        //   state changed, poll the primary's IPC commands from a child,
        //   promotion when the primary dies). 500 ms is plenty for those
        //   and cuts the idle re-render rate 5x.
        // - Clicks (event-driven): `tray_event_stream` polls the
        //   tray-icon crate's channels inside its own async task and only
        //   yields a Message when a real click arrives, so a menu / icon
        //   click still wakes the UI instantly while an idle tray never
        //   re-renders anything.
        #[cfg(target_os = "windows")]
        {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(500))
                    .map(|_| Message::Tray(TrayMessage::Poll)),
            );
            subs.push(Subscription::run(tray_event_stream));
        }

        // Cross-process deep-link inbox (every platform). Same
        // yield-only-on-event shape as `tray_event_stream`: the fs
        // poll runs inside the stream's own task, so an empty inbox
        // (the permanent steady state) never wakes `update()` or
        // forces a re-render.
        subs.push(Subscription::run(deep_link_stream));

        // Port forward liveness sweep. Only mounts while at least one
        // forward is active; a 5 s tick is enough to flip a row's toggle
        // back to off shortly after its connection drops, without polling
        // when nothing is forwarding.
        if !self.active_forwards.is_empty() {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(5))
                    .map(|_| Message::PortForward(PortForwardMessage::PortForwardLivenessTick)),
            );
        }

        // Port forward auto-start retry. Only mounts while at least one
        // auto_start rule is down and pending a re-attempt, and only while
        // unlocked (a retry needs vault credentials, and would burn attempts
        // against a sealed vault; a soft auto-lock keeps live forwards but
        // pauses the healer until unlock). The 15 s heartbeat just wakes the
        // handler; the per-rule exponential backoff (15 s → 120 s ceiling)
        // decides which rules are actually due. 15 s matches the shortest
        // backoff, so a permanently-down forward keeps this at a gentle
        // 4 wakes/min rather than churning the view faster than any rule
        // could possibly be due.
        if self.vault_ui.state == crate::state::VaultState::Unlocked
            && !self.port_forward_retry.is_empty()
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(15))
                    .map(|_| Message::PortForward(PortForwardMessage::PortForwardRetryTick)),
            );
        }

        // Session-log flush ticker. Only mounts while at least one pane
        // is recording; drains the per-pane output buffers into the vault
        // every 2 s so an idle-but-trickling session still persists
        // promptly without a write per SSH chunk. Also unmounted while
        // the vault is locked (the log key is zeroized, a drain would
        // discard data): buffers accumulate and flush after unlock.
        if self.vault_ui.state == crate::state::VaultState::Unlocked
            && self
                .tabs
                .iter()
                .any(|t| t.pane_grid.panes.values().any(|p| p.session_log_id.is_some()))
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(2))
                    .map(|_| Message::Terminal(TerminalMessage::SessionLogFlushTick)),
            );
        }

        // Cloud auto-refresh ticker. Only mounts the subscription when
        // the user enabled the toggle in Settings; otherwise zero
        // background API calls. Interval reads the persisted setting
        // and falls back to 30 min on any parse failure so a malformed
        // value doesn't pin the ticker at 1 ms.
        if self.prefs.cloud_auto_refresh_enabled && !self.cloud_profiles.is_empty() {
            let minutes = self
                .prefs.cloud_auto_refresh_interval_minutes
                .parse::<u64>()
                .ok()
                .filter(|m| *m > 0)
                .unwrap_or(30);
            subs.push(
                iced::time::every(std::time::Duration::from_secs(minutes * 60))
                    .map(|_| Message::Cloud(CloudMessage::CloudAutoRefreshTick)),
            );
        }
        // Cloud SSM/ECS idle keepalive. The SSM websocket drops the
        // session after ~20 min of inactivity, which bites when the user
        // alt-tabs away and comes back much later. We only mount the
        // ticker while the window is unfocused (an in-focus session has
        // the user's own input resetting the idle timer, and resizing a
        // visible terminal would be jarring) and only when at least one
        // SSM/ECS tab is open. 4 min comfortably beats the 20 min
        // default even allowing for a missed tick; users who lowered the
        // SSM idle timeout below ~5 min would need the server-side
        // setting raised instead.
        if !self.window_focused
            && self.tabs.iter().any(|t| t.ssm_keepalive)
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(240))
                    .map(|_| Message::Tabs(TabsMessage::SsmKeepaliveTick)),
            );
        }
        // Snapshot-transport auto cadence. The P2P transport runs its
        // own timer inside the engine; the snapshot transports (SFTP,
        // folder, Git, WebDAV) have none, so the cadence lives here.
        // Mounts for any of them in enabled + auto; the tick is a no-op
        // while a round is already in flight. 5 min matches the P2P
        // `auto_interval_secs` default.
        // Unlocked is a REAL condition here, not belt and braces: a soft
        // auto-lock zeroizes the master key and drops
        // `master_password` while the app keeps running, and the round
        // would then reach a server with stored credentials on behalf
        // of a vault the user believes is closed (and fail, since it
        // cannot decrypt anything).
        if self.sync.enabled
            && !self.sync_uses_p2p()
            && self.sync.mode == "auto"
            && self.vault_ui.state == crate::state::VaultState::Unlocked
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(300))
                    .map(|_| Message::Sync(SyncMessage::SnapshotTick)),
            );
        }

        // Session-player playback clock (issue #71). Only mounts while
        // the player is actually playing on the History view; paused,
        // closed or backgrounded players tick nothing. Leaving the view
        // suspends the clock (the handler clamps the resume delta, so
        // the absence never counts as playback time). ~30 fps is the
        // live-terminal ballpark and keeps scrubbing responsive.
        //
        // Watching is not user activity (only real input resets the idle
        // clock), so a soft auto-lock can fire mid-playback. Gate on the
        // unlocked state like the other subscriptions do, or the clock
        // keeps advancing behind the lock screen: CPU burned, position
        // lost on return, and the documented "subscriptions unmount
        // while locked" contract broken.
        if self.active_view == crate::state::View::History
            && self.vault_ui.state == crate::state::VaultState::Unlocked
            && self.session_player.as_ref().is_some_and(|p| p.playing)
        {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(33))
                    .map(|_| Message::Player(PlayerMessage::ClockTick)),
            );
        }

        // Vault auto-lock idle check. Only mounts while unlocked with a
        // non-zero threshold configured, so the common case (feature off)
        // costs nothing. The 30 s cadence bounds the overshoot past the
        // configured idle threshold; the handler does the actual
        // elapsed-time comparison against `last_user_activity`.
        if self.vault_ui.state == crate::state::VaultState::Unlocked
            && self
                .prefs.auto_lock_minutes
                .parse::<u64>()
                .ok()
                .filter(|m| *m > 0)
                .is_some()
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(30))
                    .map(|_| Message::Settings(SettingsMessage::AutoLockTick)),
            );
        }

        Subscription::batch(subs)
    }
}

/// Event-driven tray click source (Windows only). Polls the tray-icon
/// crate's menu / icon channels inside this async task and yields a
/// `Message` only when a real event arrives, so an idle tray never
/// forces a UI re-render. The internal `try_recv` poll is cheap and,
/// crucially, does NOT go through `update()`, so it costs nothing on
/// the render side; only a yielded event wakes the app. Replaces the
/// old 100 ms `TrayPoll` timer that re-rendered the whole app 10x/s.
/// Claim deep links dropped in `~/.oryxis/runtime/deeplink/` by a
/// `oryxis://` launcher process (see the deep-link block in
/// `main.rs`). One yielded Message per claimed URL; the 500 ms sleep
/// bounds the courier's wait loop, which gives up (and boots its own
/// window) after 2 s without a claim.
fn deep_link_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::futures::stream::unfold(Vec::<Message>::new(), |mut queue| async {
        loop {
            if let Some(msg) = queue.pop() {
                return Some((msg, queue));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            // Two inboxes, two messages: a `ssh://` link prefills a
            // confirm surface, a CLI target dials. They are drained here
            // together only because they share a poll interval; the
            // payloads never mix (see `tray_ipc::connect_dir`).
            queue = crate::tray_ipc::take_deeplinks()
                .into_iter()
                .map(|url| Message::Tray(crate::messages::TrayMessage::DeepLink(url)))
                .chain(
                    crate::tray_ipc::take_connects().into_iter().map(|target| {
                        Message::Tray(crate::messages::TrayMessage::ConnectTarget(target))
                    }),
                )
                .collect();
        }
    })
}

#[cfg(target_os = "windows")]
fn tray_event_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::futures::stream::unfold((), |()| async {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if let Some(id) = crate::tray::poll_menu_event() {
                return Some((Message::Tray(TrayMessage::MenuEvent(id)), ()));
            }
            // Left-click / double-click on the icon body restores the
            // window; other icon events (move, right-click, which
            // Windows handles by popping the menu itself) are ignored
            // and the loop keeps waiting.
            if let Some(ev) = crate::tray::poll_icon_event()
                && matches!(ev, tray_icon::TrayIconEvent::DoubleClick { .. })
            {
                return Some((Message::Tray(TrayMessage::IconDoubleClick), ()));
            }
        }
    })
}
