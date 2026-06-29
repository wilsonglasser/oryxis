//! `Oryxis::update`, the master message-dispatch table. ~5k lines of
//! match arms; pulled out of `app.rs` so the wiring file stays trim.
//! All `pub(crate)` helpers it relies on live in sibling modules
//! (`sftp_helpers`, `sftp_methods`, `connect_methods`, `util`,
//! `boot`, `mcp`, `state`).

#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;


use crate::app::{Message, Oryxis};

/// How long a dynamic group's resolved host list stays "fresh" before
/// re-opening the group triggers a background re-resolve. Cloud
/// resources (ECS tasks especially) recycle, so a list older than this
/// is likely to contain dead rows that fail on click. 60s balances
/// freshness against hammering the cloud API on every navigation.
pub(crate) const DYNAMIC_GROUP_CACHE_TTL_SECS: i64 = 60;

/// Chain `message` through a domain handler. If the handler claims it
/// (returns `Ok`), short-circuit and return the resulting task.
/// Otherwise, the message is handed back unchanged for the next link.
macro_rules! try_handler {
    ($self:ident, $msg:ident, $handler:ident) => {
        match $self.$handler($msg) {
            Ok(task) => return task,
            Err(m) => m,
        }
    };
}

impl Oryxis {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // SFTP async-continuation messages target a specific tab that may no
        // longer be focused. Swap the owning tab's state into `self.sftp` for
        // the duration so the (unchanged) handlers route to the right tab,
        // then swap back. See `route_sftp_async`.
        let task = if let Some(id) = message.sftp_async_owner() {
            self.route_sftp_async(id, message)
        } else {
            self.dispatch_message(message)
        };
        // Keep the unified strip order (terminal + SFTP) in sync with the live
        // tabs after every message: new tabs appended, closed ones dropped,
        // drag-reordered order preserved.
        self.reconcile_tab_order();
        task
    }

    /// Show a generic "remove this?" confirmation. Confirming dispatches
    /// `action` (the real `Delete*` message). Routes destructive removals
    /// (host, key, identity, snippet, session group) through an explicit
    /// confirm, mirroring the known-hosts / SFTP delete guards so a stray
    /// click can't silently drop an entry. Closes any open card menu first
    /// so it doesn't linger behind the dialog scrim.
    pub(crate) fn confirm_remove(&mut self, name: String, action: Message) {
        self.card_context_menu = None;
        self.snippet_context_menu = None;
        self.key_context_menu = None;
        self.identity_context_menu = None;
        self.overlay = None;
        self.error_dialog = Some(crate::state::ErrorDialog {
            title: crate::i18n::t("remove_confirm_title").to_string(),
            body: format!("\"{name}\""),
            link: None,
            action: Some(crate::state::ErrorDialogAction {
                label: crate::i18n::t("remove").to_string(),
                message: Box::new(action),
                danger: true,
            }),
        });
    }

    pub(crate) fn dispatch_message(&mut self, message: Message) -> Task<Message> {
        // Domain-specific handlers each claim a slice of `Message`
        // variants and return `Err(message)` for everything else, so
        // the chain naturally falls through to the inline match below.
        let message = try_handler!(self, message, handle_sftp_transfers);
        let message = try_handler!(self, message, handle_sftp_files);
        let message = try_handler!(self, message, handle_sftp);
        let message = try_handler!(self, message, handle_ssh);
        let message = try_handler!(self, message, handle_port_forwards);
        let message = try_handler!(self, message, handle_settings);
        let message = try_handler!(self, message, handle_keys);
        let message = try_handler!(self, message, handle_proxy_identity);
        let message = try_handler!(self, message, handle_plugins);
        let message = try_handler!(self, message, handle_cloud);
        let message = try_handler!(self, message, handle_ai);
        let message = try_handler!(self, message, handle_editor);
        let message = try_handler!(self, message, handle_session_group);
        let message = try_handler!(self, message, handle_tabs);
        let message = try_handler!(self, message, handle_terminal);
        let message = try_handler!(self, message, handle_share);
        let message = try_handler!(self, message, handle_known_hosts);
        let message = try_handler!(self, message, handle_tray);
        let message = try_handler!(self, message, handle_vault);
        let message = try_handler!(self, message, handle_snippets);
        let message = try_handler!(self, message, handle_navigation);
        let message = try_handler!(self, message, handle_history);
        let message = try_handler!(self, message, handle_mcp);

        match message {
            // ── Sync ──
            Message::SyncToggleEnabled => {
                self.sync.enabled = !self.sync.enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_enabled", if self.sync.enabled { "true" } else { "false" });
                }
                // SFTP transport has no background engine: enabling just
                // persists the flag (the cadence subscription picks it up);
                // disabling clears any stale status.
                if self.sync.transport == "sftp" {
                    self.sync.status = Some(
                        crate::i18n::t(if self.sync.enabled {
                            "sync_status_enabled"
                        } else {
                            "sync_status_stopped"
                        })
                        .to_string(),
                    );
                } else if self.sync.enabled {
                    return self.start_sync_engine();
                } else {
                    self.stop_sync_engine();
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_stopped").to_string());
                }
            }
            Message::SyncTogglePasswords => {
                self.sync.passwords = !self.sync.passwords;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting(
                        "sync_passwords",
                        if self.sync.passwords { "true" } else { "false" },
                    );
                }
            }
            Message::SyncModeChanged(v) => {
                self.sync.mode = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_mode", &v);
                }
            }
            Message::SyncDeviceNameChanged(v) => {
                self.sync.device_name = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_device_name", &v);
                }
            }
            Message::SyncSignalingUrlChanged(v) => {
                self.sync.signaling_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_url", &v);
                }
            }
            Message::SyncSignalingTokenChanged(v) => {
                self.sync.signaling_token = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_token", &v);
                }
            }
            Message::SyncRelayUrlChanged(v) => {
                self.sync.relay_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_relay_url", &v);
                }
            }
            Message::SyncListenPortChanged(v) => {
                self.sync.listen_port = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_listen_port", &v);
                }
            }
            Message::SyncStartPairing => {
                // Host a real pairing code on the engine. The engine
                // also emits `PairingCodeGenerated`, but we set the
                // code + state here directly so the UI flips instantly.
                if let Some(runtime) = &self.sync.runtime {
                    let handle = runtime.handle();
                    let code = handle.start_hosting_pairing();
                    let link = handle.pairing_link(&code);
                    self.sync.pairing.link = Some(link);
                    self.sync.pairing.code = Some(code);
                    self.sync.pairing.state = crate::state::SyncPairingState::Hosting;
                } else {
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                }
            }
            Message::SyncCancelHostingPairing => {
                if let Some(runtime) = &self.sync.runtime {
                    runtime.handle().cancel_hosting_pairing();
                }
                self.sync.pairing.code = None;
                self.sync.pairing.link = None;
                self.sync.pairing.state = crate::state::SyncPairingState::Idle;
            }
            Message::SyncJoinPairingRequested => {
                self.sync.pairing.state = crate::state::SyncPairingState::Joining;
                self.sync.pairing.join_code_input.clear();
                self.sync.pairing.join_target_input.clear();
                self.sync.pairing.join_link_input.clear();
            }
            Message::SyncJoinCodeChanged(v) => {
                self.sync.pairing.join_code_input = v;
            }
            Message::SyncJoinTargetChanged(v) => {
                self.sync.pairing.join_target_input = v;
            }
            Message::SyncJoinLinkChanged(v) => {
                self.sync.pairing.join_link_input = v;
            }
            Message::SyncJoinPairingCancel => {
                self.sync.pairing.state = crate::state::SyncPairingState::Idle;
            }
            Message::SyncPairWithDiscovered(device_id) => {
                if let Some(peer) = self
                    .sync.discovered
                    .iter()
                    .find(|p| p.device_id == device_id)
                {
                    self.sync.pairing.state = crate::state::SyncPairingState::Joining;
                    self.sync.pairing.join_code_input.clear();
                    self.sync.pairing.join_link_input.clear();
                    self.sync.pairing.join_target_input = peer.addr.to_string();
                }
            }
            Message::SyncJoinPairingByLink => {
                let Some(runtime) = &self.sync.runtime else {
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                    return Task::none();
                };
                let link = self.sync.pairing.join_link_input.trim().to_string();
                if oryxis_sync::parse_pairing_link(&link).is_none() {
                    self.sync.status = Some(
                        crate::i18n::t("sync_pairing_bad_link").to_string(),
                    );
                    return Task::none();
                }
                let handle = runtime.handle();
                // Keep at Joining so the inline status + form stay
                // visible; the PairingCompleted / PairingFailed event
                // handler decides whether to drop back to Idle.
                self.sync.status =
                    Some(crate::i18n::t("sync_pairing_connecting").to_string());
                return Task::perform(
                    async move {
                        let _ = handle.join_pairing_remote(&link).await;
                    },
                    |()| Message::NoOp,
                );
            }
            Message::SyncJoinPairingConnect => {
                let Some(runtime) = &self.sync.runtime else {
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_disabled").to_string());
                    return Task::none();
                };
                let code = self.sync.pairing.join_code_input.trim().to_string();
                if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
                    self.sync.status =
                        Some(crate::i18n::t("sync_pairing_invalid_code").to_string());
                    return Task::none();
                }
                let addr: std::net::SocketAddr =
                    match self.sync.pairing.join_target_input.trim().parse() {
                        Ok(a) => a,
                        Err(_) => {
                            self.sync.status = Some(
                                crate::i18n::t("sync_pairing_bad_address").to_string(),
                            );
                            return Task::none();
                        }
                    };
                let handle = runtime.handle();
                // Keep at Joining so the inline status + form stay
                // visible while the handshake runs; the PairingCompleted
                // event flips back to Idle, PairingFailed stays put so
                // the user can fix the code/addr and retry.
                self.sync.status =
                    Some(crate::i18n::t("sync_pairing_connecting").to_string());
                // join_pairing emits PairingCompleted / PairingFailed,
                // which the SyncEngineEvent arm turns into UI state.
                return Task::perform(
                    async move {
                        let _ = handle.join_pairing(addr, code).await;
                    },
                    |()| Message::NoOp,
                );
            }
            Message::SyncUnpairDevice(peer_id) => {
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_sync_peer(&peer_id);
                    self.sync.peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            Message::SyncNow => {
                // SFTP transport: a manual round goes through the
                // snapshot path, not the P2P engine.
                if self.sync.transport == "sftp" {
                    return self.run_sftp_sync_round();
                }
                if self.sync.in_progress {
                    // Defensive: shouldn't fire because the UI swaps
                    // Sync Now for Cancel while a sync is running,
                    // but if a stray click does land, ignore it.
                    return Task::none();
                }
                if let Some(runtime) = &self.sync.runtime {
                    let handle = runtime.handle();
                    let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
                    self.sync.abort_tx = Some(abort_tx);
                    self.sync.in_progress = true;
                    self.sync.status =
                        Some(crate::i18n::t("sync_status_syncing").to_string());
                    // Race the sync against a 90s timeout AND the
                    // abort channel. Whichever fires first wins; the
                    // sync future is dropped, which closes the QUIC
                    // connection mid-handshake (quinn cleans up).
                    return Task::perform(
                        async move {
                            tokio::select! {
                                r = tokio::time::timeout(
                                    std::time::Duration::from_secs(90),
                                    handle.sync_now(),
                                ) => match r {
                                    Ok(Ok(())) => Ok(()),
                                    Ok(Err(e)) => Err(format!("{e}")),
                                    Err(_) => Err("__timeout__".into()),
                                },
                                _ = abort_rx => Err("__cancelled__".into()),
                            }
                        },
                        Message::SyncNowFinished,
                    );
                }
                self.sync.status =
                    Some(crate::i18n::t("sync_status_disabled").to_string());
            }
            Message::SyncCancelInProgress => {
                if let Some(tx) = self.sync.abort_tx.take() {
                    let _ = tx.send(());
                }
                // Don't clear `sync_in_progress` here: the Task lands
                // back as `SyncNowFinished(Err("__cancelled__"))` and
                // clears it there, so the Cancel button stays visible
                // until the cancellation actually settles.
            }
            Message::SyncTransportChanged(v) => {
                if v != self.sync.transport {
                    // Leaving P2P: tear the engine down so QUIC/mDNS stop.
                    // Entering P2P (and enabled): bring it up.
                    self.sync.transport = v.clone();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("sync_transport", &v);
                    }
                    self.sync.status = None;
                    self.sync.sftp.status = None;
                    if v == "sftp" {
                        self.stop_sync_engine();
                    } else if self.sync.enabled {
                        return self.start_sync_engine();
                    }
                }
            }
            Message::SyncSftpHostChanged(id) => {
                self.sync.sftp.host_id = Some(id);
                self.sync.sftp.picker_open = false;
                self.sync.sftp.picker_search.clear();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_sftp_host_id", &id.to_string());
                }
            }
            Message::SyncSftpOpenPicker => {
                self.sync.sftp.picker_open = true;
                self.sync.sftp.picker_search.clear();
            }
            Message::SyncSftpClosePicker => {
                self.sync.sftp.picker_open = false;
                self.sync.sftp.picker_search.clear();
            }
            Message::SyncSftpPickerSearch(v) => {
                self.sync.sftp.picker_search = v;
            }
            Message::SyncSftpPathChanged(v) => {
                self.sync.sftp.remote_path = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_sftp_remote_path", &v);
                }
            }
            Message::SyncSftpPassphraseChanged(v) => {
                self.sync.sftp.passphrase = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_sync_sftp_passphrase(&v);
                }
            }
            Message::SftpSyncTick => {
                // Auto-cadence tick. Only act in SFTP+enabled+auto and
                // when no round is already running; otherwise the tick
                // is a no-op (the subscription keeps firing regardless).
                if self.sync.transport == "sftp"
                    && self.sync.enabled
                    && self.sync.mode == "auto"
                    && !self.sync.sftp.in_progress
                {
                    return self.run_sftp_sync_round();
                }
            }
            Message::SftpSyncDone(result) => {
                self.sync.sftp.in_progress = false;
                if result.is_ok() {
                    // The merge ran on a separate vault handle, so the
                    // in-memory lists are stale: reload to reflect it.
                    self.load_data_from_vault();
                }
                self.sync.sftp.status = Some(result);
            }
            Message::SyncNowFinished(result) => {
                self.sync.in_progress = false;
                self.sync.abort_tx = None;
                match result {
                    Ok(()) => {}
                    Err(e) if e == "__cancelled__" => {
                        self.sync.status = Some(
                            crate::i18n::t("sync_status_cancelled").to_string(),
                        );
                    }
                    Err(e) if e == "__timeout__" => {
                        self.sync.status = Some(
                            crate::i18n::t("sync_status_timeout").to_string(),
                        );
                    }
                    Err(e) => {
                        self.sync.status = Some(format!(
                            "{}: {e}",
                            crate::i18n::t("sync_status_failed"),
                        ));
                    }
                }
                // Per-peer outcomes already arrived as SyncEngineEvent;
                // refresh the peer list so last_synced_at is current.
                if let Some(vault) = &self.vault {
                    self.sync.peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            Message::SyncEngineEvent(event) => {
                use oryxis_sync::SyncEvent;
                match event {
                    SyncEvent::PeerDiscovered { device_id, device_name, addr, .. } => {
                        // Dedup by device_id: an mDNS browse can
                        // republish the same peer (different network
                        // interface, restart, etc.). Last writer wins
                        // on the address so a roaming peer's entry
                        // tracks its new ip:port.
                        let info = crate::state::DiscoveredPeerInfo {
                            device_id,
                            device_name,
                            addr,
                        };
                        if let Some(existing) = self
                            .sync.discovered
                            .iter_mut()
                            .find(|p| p.device_id == device_id)
                        {
                            *existing = info;
                        } else {
                            self.sync.discovered.push(info);
                        }
                    }
                    SyncEvent::PairingCodeGenerated { code } => {
                        self.sync.pairing.code = Some(code);
                    }
                    SyncEvent::PairingCompleted { device_name, .. } => {
                        self.sync.status = Some(format!(
                            "{} {device_name}",
                            crate::i18n::t("sync_paired_with"),
                        ));
                        // Pairing done on either side: close the modal
                        // sub-view, drop the hosted code / link / QR,
                        // and refresh the peer list.
                        self.sync.pairing.state =
                            crate::state::SyncPairingState::Idle;
                        self.sync.pairing.code = None;
                        self.sync.pairing.link = None;
                        if let Some(vault) = &self.vault {
                            self.sync.peers =
                                vault.list_sync_peers().unwrap_or_default();
                        }
                    }
                    SyncEvent::PairingFailed { reason } => {
                        self.sync.status = Some(format!(
                            "{}: {reason}",
                            crate::i18n::t("sync_pairing_failed"),
                        ));
                        // Stay in whichever sub-view triggered the
                        // pairing so the user sees the error in
                        // context and can fix + retry without
                        // re-entering everything. Host-side: clear
                        // the code/link since the single-shot was
                        // consumed even on failure.
                        if self.sync.pairing.state
                            == crate::state::SyncPairingState::Hosting
                        {
                            self.sync.pairing.code = None;
                            self.sync.pairing.link = None;
                            self.sync.pairing.state =
                                crate::state::SyncPairingState::Idle;
                        }
                    }
                    SyncEvent::SyncStarted { .. } => {
                        self.sync.status =
                            Some(crate::i18n::t("sync_status_syncing").to_string());
                    }
                    SyncEvent::SyncCompleted { pushed, pulled, .. } => {
                        self.sync.status = Some(format!(
                            "{} (+{pushed} / -{pulled})",
                            crate::i18n::t("sync_status_done"),
                        ));
                        if let Some(vault) = &self.vault {
                            self.sync.peers =
                                vault.list_sync_peers().unwrap_or_default();
                        }
                    }
                    SyncEvent::SyncFailed { error, .. } => {
                        self.sync.status = Some(format!(
                            "{}: {error}",
                            crate::i18n::t("sync_status_failed"),
                        ));
                    }
                    SyncEvent::PeerOnline { .. } | SyncEvent::PeerOffline { .. } => {}
                    SyncEvent::SignalingRegistered { ip, port } => {
                        // Confirms cross-network pairing is reachable
                        // at this address. Until this fires the host is
                        // LAN-only (or signaling failed silently). The
                        // `(n)` counter bumps on every refresh so the
                        // user sees heartbeats land even when the IP
                        // is stable.
                        self.sync.signaling_tick =
                            self.sync.signaling_tick.saturating_add(1);
                        self.sync.status = Some(format!(
                            "{} ({}): {ip}:{port}",
                            crate::i18n::t("sync_status_signaling_registered"),
                            self.sync.signaling_tick,
                        ));
                    }
                    SyncEvent::SignalingFailed { reason } => {
                        self.sync.status = Some(format!(
                            "{}: {reason}",
                            crate::i18n::t("sync_status_signaling_failed"),
                        ));
                    }
                    SyncEvent::VersionMismatch {
                        peer_version,
                        local_version,
                        ..
                    } => {
                        self.sync.status = Some(format!(
                            "{}: peer v{peer_version}, local v{local_version}",
                            crate::i18n::t("sync_status_version_mismatch"),
                        ));
                    }
                    SyncEvent::PeerStaleWarning { days_since_sync, .. } => {
                        self.sync.status = Some(format!(
                            "{} ({}d)",
                            crate::i18n::t("sync_status_peer_stale"),
                            days_since_sync,
                        ));
                    }
                }
            }


            // Anything not handled above was claimed by one of the
            // domain handlers in the `try_handler!` chain above. Any
            // variant reaching here means we forgot to claim it; treat
            // as a no-op so we don't crash on an unclaimed message.
            _ => {}
        }
        Task::none()
    }

    /// Push the current window state (hidden + tab labels) into the
    /// tray_ipc registry so the primary's tray menu picks it up on
    /// its next scan. No-op for the primary itself (its tray rebuild
    /// reads from in-process Oryxis state directly, not via the
    /// filesystem registry).
    ///
    /// Signature-gated so 100 ms TrayPoll ticks don't churn the
    /// filesystem when nothing changed; explicit hide/show handlers
    /// also call this so the registry refreshes within one tick of
    /// the user action instead of waiting for the polling tick.
    pub(crate) fn broadcast_ipc_state_if_child(&mut self) {
        if crate::app::APP_IS_PRIMARY.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.is_window_hidden.hash(&mut h);
        self.tabs.len().hash(&mut h);
        for t in &self.tabs {
            t.label.hash(&mut h);
        }
        let sig = h.finish();
        if sig == self.ipc_state_signature {
            return;
        }
        self.ipc_state_signature = sig;
        let tabs: Vec<String> = self.tabs.iter().map(|t| t.label.clone()).collect();
        // Title: when the user has an active tab the label is what
        // they're staring at, otherwise fall back to a generic
        // "Oryxis" so the primary's submenu still has something to
        // show.
        let title = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.label.clone())
            .unwrap_or_else(|| "Oryxis".to_string());
        crate::tray_ipc::Child::write_state(crate::tray_ipc::InstanceState {
            pid: std::process::id(),
            title,
            tabs,
            is_hidden: self.is_window_hidden,
        });
    }
}
