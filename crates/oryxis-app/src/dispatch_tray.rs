//! `Oryxis::handle_tray`: settings-panel-independent dispatch arms for the
//! tray area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{Message, Oryxis};

impl Oryxis {
    pub(crate) fn handle_tray(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- System tray --
            Message::TrayPoll => {
                // Rebuild the dynamic submenu (Active sessions +
                // Recent hosts) when the state behind it changed.
                // Signature is a hash of the tab count + connection
                // last_used times. The hash itself is cheap, but the
                // IPC registry scan behind it stats files, so the
                // signature pass runs every 5th tick (500 ms) while
                // the event drain below keeps the 100 ms cadence for
                // click responsiveness.
                static TRAY_SIG_TICK: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
                let run_signature_pass = TRAY_SIG_TICK
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .is_multiple_of(5);
                if run_signature_pass {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};

                    let mut h = DefaultHasher::new();
                    self.tabs.len().hash(&mut h);
                    for t in &self.tabs {
                        t.label.hash(&mut h);
                    }
                    self.connections.len().hash(&mut h);
                    for c in &self.connections {
                        c.id.hash(&mut h);
                        c.last_used.map(|d| d.timestamp_millis()).hash(&mut h);
                    }
                    // Fold the IPC registry into the signature so a
                    // child going hidden / changing title triggers
                    // a primary menu rebuild on the next tick. Cheap:
                    // list_instances does one dir scan + PID liveness
                    // check per entry, which on a typical setup means
                    // <5 file reads.
                    let ipc_instances = crate::tray_ipc::Primary::list_instances();
                    ipc_instances.len().hash(&mut h);
                    for inst in &ipc_instances {
                        inst.pid.hash(&mut h);
                        inst.is_hidden.hash(&mut h);
                        inst.title.hash(&mut h);
                    }
                    let sig = h.finish();
                    if sig != self.tray_menu_signature {
                        self.tray_menu_signature = sig;
                        // `&` is the Windows menu accelerator prefix:
                        // a host named "R&D" would render as "RD" with
                        // D underlined. Doubling the `&` escapes it.
                        // Capped at 20: a user with 50+ open tabs gets
                        // an unwieldy submenu otherwise; recent-hosts
                        // submenu already had a `.take(10)` for the
                        // same reason.
                        let active: Vec<(String, String)> = self
                            .tabs
                            .iter()
                            .enumerate()
                            .take(20)
                            .map(|(i, t)| (t.label.replace('&', "&&"), i.to_string()))
                            .collect();
                        // Recent hosts: top 10 by last_used desc.
                        // Hosts that were never connected drop to
                        // the bottom and get sliced off, so the
                        // menu only lists hosts the user actually
                        // touched.
                        let mut recent_pairs: Vec<&oryxis_core::models::connection::Connection> =
                            self.connections.iter().filter(|c| c.last_used.is_some()).collect();
                        recent_pairs.sort_by_key(|c| std::cmp::Reverse(c.last_used));
                        let recent: Vec<(String, String)> = recent_pairs
                            .iter()
                            .take(10)
                            .map(|c| (c.label.replace('&', "&&"), c.id.to_string()))
                            .collect();
                        // Unified "Windows" list: every window the
                        // user owns that's currently hidden, primary
                        // first (when the primary itself is hidden)
                        // then each hidden child via the IPC registry.
                        // The id-suffix is the owning process's PID;
                        // the menu click dispatcher checks self_pid
                        // to decide between local TrayShow and an
                        // IPC send_command.
                        let mut hidden: Vec<(String, String)> = Vec::new();
                        if self.is_window_hidden {
                            let primary_label = self
                                .active_tab
                                .and_then(|i| self.tabs.get(i))
                                .map(|t| t.label.clone())
                                .unwrap_or_else(|| crate::i18n::t("tray_main_window").to_string());
                            hidden.push((
                                primary_label.replace('&', "&&"),
                                std::process::id().to_string(),
                            ));
                        }
                        for inst in crate::tray_ipc::Primary::list_instances() {
                            if !inst.is_hidden {
                                continue;
                            }
                            let label = if inst.title.is_empty() || inst.title == "Oryxis" {
                                format!("{} (PID {})", crate::i18n::t("tray_main_window"), inst.pid)
                            } else {
                                inst.title.clone()
                            };
                            hidden.push((label.replace('&', "&&"), inst.pid.to_string()));
                        }
                        if let Err(e) = crate::tray::rebuild_menu(&active, &recent, &hidden) {
                            tracing::warn!("tray menu rebuild failed: {e}");
                        }
                        // Tray icon is only visible when at least
                        // one window (primary's own or any child's)
                        // is currently hidden. The "1 tray to rule
                        // them all" UX the user asked for: when
                        // everything's visible on screen there's no
                        // reason to clutter the notification area
                        // with a redundant icon.
                        let any_hidden = self.is_window_hidden || !hidden.is_empty();
                        crate::tray::set_visible(any_hidden);
                    }
                }
                // Drain whatever the tray-icon crate's event threads
                // queued since the last poll. Each menu id resolves
                // to a real Message via Task::batch so we can emit
                // more than one event per tick if the user spam-
                // clicked. On non-Windows targets both polls return
                // None immediately, so this is harmless overhead.
                let mut follow_ups: Vec<Task<Message>> = Vec::new();

                // Push our state into the tray_ipc registry so the
                // primary's "Hidden windows" menu reflects any tab
                // label edits / new sessions / etc. between explicit
                // hide/show events. No-op for the primary itself.
                self.broadcast_ipc_state_if_child();

                // Drain whatever command the primary queued for us
                // (a Show or Quit from a click in its tray menu).
                // No-op for the primary process (it never has its
                // own command file because we skip self_pid in
                // Primary::list_instances).
                let is_primary = crate::app::APP_IS_PRIMARY
                    .load(std::sync::atomic::Ordering::Relaxed);
                if !is_primary {
                    while let Some(cmd) = crate::tray_ipc::Child::poll_command() {
                        match cmd {
                            crate::tray_ipc::Command::Show => {
                                follow_ups.push(Task::done(Message::TrayShow));
                            }
                            crate::tray_ipc::Command::Quit => {
                                follow_ups.push(Task::done(Message::TrayQuit));
                            }
                        }
                    }

                    // Promotion check: if the primary process
                    // exited (mutex released) one of the surviving
                    // children needs to take over so the user
                    // doesn't end up with orphaned hidden windows
                    // and no tray to surface them. try_acquire_mutex
                    // succeeds when nobody else owns the mutex; the
                    // first child to win the race becomes the new
                    // primary, installs the tray, and unregisters
                    // its own IPC row.
                    if crate::tray::try_acquire_mutex() {
                        tracing::info!("tray IPC: promoting to primary (old primary gone)");
                        crate::app::APP_IS_PRIMARY
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        if let Err(e) = crate::tray::install() {
                            tracing::warn!("tray install on promotion: {e}");
                        }
                        crate::tray_ipc::Child::unregister();
                    }
                }

                while let Some(id) = crate::tray::poll_menu_event() {
                    let msg = match id.as_str() {
                        crate::tray::MENU_ID_SHOW => Some(Message::TrayShow),
                        crate::tray::MENU_ID_HIDE => Some(Message::TrayHide),
                        crate::tray::MENU_ID_QUIT => Some(Message::TrayQuit),
                        s if s.starts_with(crate::tray::MENU_PREFIX_SESSION) => {
                            // "oryxis-tray-session:<idx>" -> activate
                            // that open tab. The dispatcher already
                            // has TabSelect plumbed through every code
                            // path that switches the active terminal.
                            let suffix = &s[crate::tray::MENU_PREFIX_SESSION.len()..];
                            suffix.parse::<usize>().ok().and_then(|idx| {
                                if idx < self.tabs.len() {
                                    Some(Message::TrayActivateSession(idx))
                                } else {
                                    None
                                }
                            })
                        }
                        s if s.starts_with(crate::tray::MENU_PREFIX_HOST) => {
                            // "oryxis-tray-host:<uuid>" -> open a new
                            // tab against that saved connection.
                            let suffix = &s[crate::tray::MENU_PREFIX_HOST.len()..];
                            uuid::Uuid::parse_str(suffix)
                                .ok()
                                .map(Message::TrayOpenHost)
                        }
                        s if s.starts_with(crate::tray::MENU_PREFIX_HIDDEN) => {
                            // "oryxis-tray-hidden:<pid>". If pid is
                            // our own, the menu item refers to the
                            // primary's own hidden window: fire
                            // TrayShow locally. Otherwise queue an
                            // IPC Show command for the child whose
                            // TrayPoll routes it back into TrayShow
                            // on its side.
                            let suffix = &s[crate::tray::MENU_PREFIX_HIDDEN.len()..];
                            if let Ok(pid) = suffix.parse::<u32>() {
                                if pid == std::process::id() {
                                    Some(Message::TrayShow)
                                } else {
                                    crate::tray_ipc::Primary::send_command(
                                        pid,
                                        crate::tray_ipc::Command::Show,
                                    );
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(m) = msg {
                        follow_ups.push(Task::done(m));
                    }
                }
                // Left-click on the tray icon body (not the menu)
                // counts as "Show". We drain but ignore the right-
                // click event; Windows already pops the menu on its
                // own for right-clicks via the registered Menu.
                #[cfg(target_os = "windows")]
                while let Some(ev) = crate::tray::poll_icon_event() {
                    if matches!(
                        ev,
                        tray_icon::TrayIconEvent::DoubleClick { .. }
                    ) {
                        follow_ups.push(Task::done(Message::TrayShow));
                    }
                }
                #[cfg(not(target_os = "windows"))]
                while crate::tray::poll_icon_event().is_some() {}

                if !follow_ups.is_empty() {
                    return Ok(Task::batch(follow_ups));
                }
            }
            Message::TrayShow => {
                // Hop through iced::window::oldest -> window::run so
                // we get the raw window handle on the UI thread. The
                // tray hide/show helpers swallow non-Windows targets
                // (stubs return false), so this is a no-op outside
                // Windows even though the code compiles everywhere.
                // `.discard()` drops the `()` return so the chain
                // matches the dispatcher's `Task<Message>` shape.
                self.is_window_hidden = false;
                self.broadcast_ipc_state_if_child();
                return Ok(iced::window::oldest()
                    .and_then(|id| {
                        iced::window::run(id, |window| {
                            crate::tray::show_window(window);
                        })
                    })
                    .discard());
            }
            Message::TrayHide => {
                self.is_window_hidden = true;
                self.broadcast_ipc_state_if_child();
                return Ok(iced::window::oldest()
                    .and_then(|id| {
                        iced::window::run(id, |window| {
                            crate::tray::hide_window(window);
                        })
                    })
                    .discard());
            }
            Message::TrayQuit => {
                tracing::info!("tray: quit requested");
                return Ok(iced::exit());
            }
            Message::TrayActivateSession(idx) => {
                // Show first (window may be hidden) then re-emit
                // SelectTab via Task::done. Bundled together so the
                // user sees the tab swap and the window pop in the
                // same frame.
                if idx < self.tabs.len() {
                    return Ok(Task::batch(vec![
                        Task::done(Message::TrayShow),
                        Task::done(Message::SelectTab(idx)),
                    ]));
                }
            }
            Message::TrayOpenHost(uuid) => {
                if let Some(idx) =
                    self.connections.iter().position(|c| c.id == uuid)
                {
                    return Ok(Task::batch(vec![
                        Task::done(Message::TrayShow),
                        Task::done(Message::ConnectSsh(idx)),
                    ]));
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
