//! `Oryxis::handle_sync`: settings-panel-independent dispatch arms for the
//! sync area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;
use zeroize::Zeroizing;

use crate::app::{Message, Oryxis, SyncMessage};

/// What started a snapshot round, which is what decides whether the
/// typed passphrase buffer may be used as the round's key.
///
/// Only a MANUAL round consumes the buffer. The auto cadence fires on a
/// timer, so it lands on whatever the user happens to have half-typed:
/// against an existing snapshot that round fails with "Decryption
/// failed" out of nowhere, and with NO snapshot yet (a first setup, or
/// the state the recovery hint asks the user to create) it SEALS one
/// with the partial value and stores it as the group key. That is the
/// same silent key swap the empty-field rule exists to prevent, so the
/// tick reads storage and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncRoundTrigger {
    /// The user pressed "Sync now".
    Manual,
    /// The 5-minute auto-cadence tick (`SnapshotTick`).
    Auto,
}

/// The key one snapshot round seals its blob with, plus where it came
/// from. Resolved ONCE at the top of the round
/// ([`Oryxis::sync_round_passphrase`]) and carried by value from there:
/// nothing downstream re-reads the field, which is the whole point,
/// since the field stays editable for the entire round.
pub(crate) struct RoundKey {
    /// The passphrase the round derives its secret from.
    value: Zeroizing<String>,
    /// True when it came from the TYPED buffer, so it is a candidate
    /// for the stored group key and gets committed if the round
    /// succeeds. A key read from storage is already the group key.
    candidate: bool,
}

impl Oryxis {
    /// One edit of the shared snapshot-transport group passphrase (SFTP /
    /// folder / Git / WebDAV all derive their key from the same stored
    /// row). Two rules keep the group key from silently changing under
    /// the existing snapshot:
    ///
    /// - The field starts EMPTY (the stored value never pre-fills it: a
    ///   masked pre-filled passphrase turns every later keystroke into an
    ///   append that silently swaps the key).
    /// - Typing NEVER writes through to storage. The typed value is the
    ///   round's key candidate, committed only when a MANUAL round
    ///   succeeds with it ([`Self::settle_sync_passphrase`]), so an
    ///   accidental keystroke can't destroy the stored key.
    ///
    /// The per-keystroke comparison against the stored value powers the
    /// "matches / differs" hint under the field. The field is masked, so
    /// the hint is the one thing that says BEFORE a round whether the
    /// typed value is the saved key; the reveal eye beside the field is
    /// how a stray character the mask hides is then found.
    fn set_sync_passphrase(&mut self, value: String) {
        self.sync.passphrase_input.set(value);
        self.refresh_passphrase_match();
    }

    /// Recompute the live "matches / differs" hint from the current
    /// buffer. Called per keystroke, and again after a round commits a
    /// new stored key, because the same buffer can flip its answer when
    /// the value it is compared against changes under it.
    fn refresh_passphrase_match(&mut self) {
        // An empty field says nothing (the user just cleared it, and may
        // keep typing); only a non-empty typed value is compared against
        // the stored passphrase for the live hint.
        if self.sync.passphrase_input.as_str().is_empty() {
            self.sync.passphrase_matches = None;
            return;
        }
        // The stored value is read ONCE per edit and kept for the edit's
        // lifetime (`passphrase_stored`): the comparison runs per
        // keystroke, and a SELECT plus a decrypt per keystroke is not a
        // price a hint should cost. The cache is dropped with the edit
        // and on lock, and refreshed by the round that commits a new key.
        if self.sync.passphrase_stored.is_none() && self.sync.passphrase_known {
            self.sync.passphrase_stored = self
                .vault
                .as_ref()
                .and_then(|v| v.get_sync_sftp_passphrase().ok().flatten())
                .map(Zeroizing::new);
        }
        let typed = self.sync.passphrase_input.as_str();
        self.sync.passphrase_matches = self
            .sync
            .passphrase_stored
            .as_ref()
            .map(|stored| stored.as_str() == typed);
    }

    /// Abandon an open passphrase edit without committing: the typed
    /// buffer is dropped and the read-only masked display returns.
    /// Shared by the blur probe (a click outside the empty field), the
    /// keynav Escape path and the settings Tab walk, all of which are
    /// "the user left the field" signals.
    pub(crate) fn exit_passphrase_edit(&mut self) {
        self.sync.passphrase_input.clear();
        self.sync.passphrase_matches = None;
        self.sync.passphrase_stored = None;
        self.sync.passphrase_editing = false;
        self.sync.passphrase_field_id = None;
        // A revealed passphrase lives only as long as the edit that
        // showed it: the next edit opens masked again. The stored value
        // is never revealed, so there is nothing else to sweep here (a
        // locked vault clears the whole set).
        self.revealed_secrets
            .remove(&crate::state::SecretField::SyncPassphrase);
    }

    /// Hand the round its key and remember what it seals with, so the
    /// commit at the end writes THAT value and not whatever the buffer
    /// holds by then ([`Self::settle_sync_passphrase`]). Nothing gates
    /// the field while a round is in flight (`in_progress` only disables
    /// the button), and a round costs an Argon2id derivation plus a
    /// network trip, so the buffer is freely mutable across the whole
    /// window: a user still correcting the field would otherwise store a
    /// key the snapshot was never sealed with, and every later round
    /// would fail with no visible cause.
    ///
    /// Only a key that came from the TYPED buffer is armed; a round
    /// keyed from storage has nothing to commit. `pub(crate)` because
    /// the four transports call it; its other half
    /// ([`Self::settle_sync_passphrase`]) is only ever reached from the
    /// round-finished arms in this file.
    pub(crate) fn arm_round_passphrase(&mut self, key: RoundKey) -> Zeroizing<String> {
        self.sync.passphrase_sealed = key.candidate.then(|| key.value.clone());
        key.value
    }

    /// End of a snapshot round: the key that SEALED it becomes the
    /// stored group key when the round succeeded, and is dropped when it
    /// failed. Writing it here and nowhere else is what makes the
    /// post-restart key equal to the key the fresh snapshot was sealed
    /// with (typing alone never touches storage).
    ///
    /// The edit closes only when the buffer still holds exactly what was
    /// committed. A user who kept typing during the round keeps their
    /// edit (and gets the hint recomputed against the new stored value):
    /// dropping those keystrokes would look like the field eating input.
    fn settle_sync_passphrase(&mut self, succeeded: bool) {
        let Some(sealed) = self.sync.passphrase_sealed.take() else {
            return;
        };
        if !succeeded {
            return;
        }
        if let Some(vault) = &self.vault {
            let _ = vault.set_sync_sftp_passphrase(&sealed);
        }
        self.sync.passphrase_known = !sealed.is_empty();
        // The hint compares against the cache, so the cache follows the
        // commit: what was just written is what the field is now judged
        // against.
        self.sync.passphrase_stored = Some(sealed.clone());
        if self.sync.passphrase_input.as_str() == sealed.as_str() {
            self.exit_passphrase_edit();
        } else {
            // On a first setup the field was open because nothing was
            // stored, not because an edit was: `passphrase_known` just
            // flipped, so without this the field would turn into the
            // read-only display under the user's cursor.
            self.sync.passphrase_editing = true;
            self.refresh_passphrase_match();
        }
    }

    /// The group key for one snapshot round: the TYPED buffer for a
    /// MANUAL round that has one (committed to storage only if that
    /// round succeeds, see [`Self::settle_sync_passphrase`]), else the
    /// stored value. Typing never writes through, and an auto round
    /// never reads the buffer at all ([`SyncRoundTrigger`]), so neither
    /// an accidental keystroke nor a timer can swap the key under the
    /// existing snapshot. `None` = no usable passphrase (nothing typed,
    /// nothing stored); the caller shows the "set a passphrase" status
    /// and aborts.
    ///
    /// An EMPTY buffer falls through to the stored value rather than
    /// aborting: clearing the field is how the user starts over
    /// mid-correction, never an instruction to sync with no key.
    pub(crate) fn sync_round_passphrase(&self, trigger: SyncRoundTrigger) -> Option<RoundKey> {
        if trigger == SyncRoundTrigger::Manual && self.sync.passphrase_input.touched() {
            let typed = self.sync.passphrase_input.as_str();
            if !typed.is_empty() {
                return Some(RoundKey {
                    value: Zeroizing::new(typed.to_owned()),
                    candidate: true,
                });
            }
        }
        self.vault
            .as_ref()
            .and_then(|v| v.get_sync_sftp_passphrase().ok().flatten())
            .filter(|p| !p.is_empty())
            .map(|stored| RoundKey {
                value: Zeroizing::new(stored),
                candidate: false,
            })
    }

    pub(crate) fn handle_sync(
        &mut self,
        message: SyncMessage,
    ) -> Task<Message> {
        match message {
            // ── Sync ──
            SyncMessage::ToggleEnabled => {
                self.sync.enabled = !self.sync.enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_enabled", if self.sync.enabled { "true" } else { "false" });
                }
                // SFTP transport has no background engine: enabling just
                // persists the flag (the cadence subscription picks it up);
                // disabling clears any stale status.
                if !self.sync_uses_p2p() {
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
            SyncMessage::TogglePasswords => {
                self.sync.passwords = !self.sync.passwords;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting(
                        "sync_passwords",
                        if self.sync.passwords { "true" } else { "false" },
                    );
                }
            }
            SyncMessage::ModeChanged(v) => {
                self.sync.mode = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_mode", &v);
                }
            }
            SyncMessage::DeviceNameChanged(v) => {
                // The same cap a peer applies when this name arrives,
                // so what is typed here is what the other side shows.
                let v: String = v
                    .chars()
                    .take(oryxis_sync::engine::DEVICE_NAME_MAX_CHARS)
                    .collect();
                self.sync.device_name = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_device_name", &v);
                }
            }
            SyncMessage::SignalingUrlChanged(v) => {
                self.sync.signaling_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_url", &v);
                }
            }
            SyncMessage::SignalingTokenChanged(v) => {
                let v = v.into_inner();
                self.sync.signaling_token = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_signaling_token", &v);
                }
            }
            SyncMessage::RelayUrlChanged(v) => {
                self.sync.relay_url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_relay_url", &v);
                }
            }
            SyncMessage::ListenPortChanged(v) => {
                self.sync.listen_port = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_listen_port", &v);
                }
            }
            SyncMessage::WizardToggle => {
                let w = &mut self.sync.relay_wizard;
                w.open = !w.open;
                if w.open && w.token.is_empty() {
                    w.token = fresh_relay_token();
                }
                if !w.open {
                    // The deploy block lives inside this card: folding it
                    // away hides the deploy, and a session (or a run)
                    // nobody can see must not outlive its surface, the
                    // same rule as closing the deploy section itself.
                    self.sync.relay_deploy.reset_probe();
                }
            }
            // The wizard's domain, public port and token are baked into
            // a deploy plan; while a deploy probe or run is in flight
            // they are frozen (the inputs are disabled, and refused here
            // for the keyboard's sake), so nothing can move under a run.
            SyncMessage::WizardDomainChanged(v) => {
                if !self.sync.relay_deploy.busy {
                    self.sync.relay_wizard.domain = v;
                    self.sync.relay_wizard.result = None;
                    // Editing the endpoint invalidates any in-flight probe:
                    // with the snapshot gone, its result is discarded on
                    // arrival instead of persisting a never-tested value.
                    self.sync.relay_wizard.testing_snapshot = None;
                    // The deploy plan bakes the domain into the Caddy site
                    // and the adopted URL, so it goes the same way.
                    self.sync.relay_deploy.reset_probe();
                }
            }
            SyncMessage::WizardPortChanged(v) => {
                if !self.sync.relay_deploy.busy {
                    self.sync.relay_wizard.port = v;
                    self.sync.relay_wizard.result = None;
                    self.sync.relay_wizard.testing_snapshot = None;
                    self.sync.relay_deploy.reset_probe();
                }
            }
            SyncMessage::WizardFormatChanged(f) => {
                self.sync.relay_wizard.format = f;
            }
            SyncMessage::WizardRegenToken => {
                if !self.sync.relay_deploy.busy {
                    self.sync.relay_wizard.token = fresh_relay_token();
                    self.sync.relay_wizard.result = None;
                    self.sync.relay_wizard.testing_snapshot = None;
                    // The plan carries the token it would write into the
                    // service; a fresh one needs a fresh plan.
                    self.sync.relay_deploy.reset_probe();
                }
            }
            SyncMessage::WizardTest => {
                let Some(base) = self.sync.relay_wizard.base_url() else {
                    return Task::none();
                };
                if self.sync.relay_wizard.testing {
                    return Task::none();
                }
                self.sync.relay_wizard.testing = true;
                self.sync.relay_wizard.result = None;
                // Snapshot what this probe actually tests. The result
                // handler adopts these values, never the live form, so
                // whatever the user types during the 8s probe window is
                // never persisted untested.
                self.sync.relay_wizard.testing_snapshot =
                    Some((base.clone(), self.sync.relay_wizard.token.clone()));
                return Task::perform(
                    probe_relay_health(base),
                    |v| Message::Sync(SyncMessage::WizardTestResult(v)),
                );
            }
            SyncMessage::WizardTestResult(r) => {
                self.sync.relay_wizard.testing = false;
                // The probe tested the SNAPSHOT taken when it started,
                // not whatever the form says now. A domain / port /
                // token edit during the probe clears the snapshot; in
                // that case the result describes a stale endpoint, so
                // drop it entirely (no success or failure line) and
                // let the user re-test the edited values.
                let Some((base, token)) =
                    self.sync.relay_wizard.testing_snapshot.take()
                else {
                    self.sync.relay_wizard.result = None;
                    return Task::none();
                };
                let ok = r.is_ok();
                self.sync.relay_wizard.result = Some(r);
                if ok {
                    // Reachable: adopt the relay as this device's
                    // signaling endpoint, exactly as if the user had
                    // filled the Advanced fields by hand. Adopt the
                    // probed snapshot, never the live form.
                    self.sync.signaling_url = base.clone();
                    self.sync.signaling_token = token.clone();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("sync_signaling_url", &base);
                        let _ =
                            vault.set_setting("sync_signaling_token", &token);
                    }
                    // A running engine keeps its old config; bounce
                    // it so the new endpoint takes effect now.
                    if self.sync.engine_running {
                        self.stop_sync_engine();
                        return self.start_sync_engine();
                    }
                }
            }
            // "Install on one of your hosts" (E3): its own file.
            m @ (SyncMessage::DeployToggle
            | SyncMessage::DeployHostPickerOpen
            | SyncMessage::DeployHostPickerClose
            | SyncMessage::DeployHostPickerSearch(..)
            | SyncMessage::DeployHostChanged(..)
            | SyncMessage::DeployPortChanged(..)
            | SyncMessage::DeployCaddyToggled
            | SyncMessage::DeployProbe
            | SyncMessage::DeployProbed(..)
            | SyncMessage::DeployReview
            | SyncMessage::DeployConfirmCancel
            | SyncMessage::DeployRun
            | SyncMessage::DeployProgress(..)
            | SyncMessage::DeployFinished(..)) => {
                return self
                    .handle_relay_deploy(m)
                    .unwrap_or_else(crate::dispatch::unrouted);
            }
            SyncMessage::StartPairing => {
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
            SyncMessage::CancelHostingPairing => {
                if let Some(runtime) = &self.sync.runtime {
                    runtime.handle().cancel_hosting_pairing();
                }
                self.sync.pairing.code = None;
                self.sync.pairing.link = None;
                self.sync.pairing.state = crate::state::SyncPairingState::Idle;
            }
            SyncMessage::JoinPairingRequested => {
                self.sync.pairing.state = crate::state::SyncPairingState::Joining;
                self.sync.pairing.join_code_input.clear();
                self.sync.pairing.join_target_input.clear();
                self.sync.pairing.join_link_input.clear();
            }
            SyncMessage::JoinCodeChanged(v) => {
                self.sync.pairing.join_code_input = v;
            }
            SyncMessage::JoinTargetChanged(v) => {
                self.sync.pairing.join_target_input = v;
            }
            SyncMessage::JoinLinkChanged(v) => {
                self.sync.pairing.join_link_input = v;
            }
            SyncMessage::JoinPairingCancel => {
                self.sync.pairing.state = crate::state::SyncPairingState::Idle;
            }
            SyncMessage::PairWithDiscovered(device_id) => {
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
            SyncMessage::JoinPairingByLink => {
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
            SyncMessage::JoinPairingConnect => {
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
            SyncMessage::UnpairDevice(peer_id) => {
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_sync_peer(&peer_id);
                    self.sync.peers = vault.list_sync_peers().unwrap_or_default();
                }
            }
            SyncMessage::Now => {
                // SFTP transport: a manual round goes through the
                // snapshot path, not the P2P engine.
                if self.sync.transport == "sftp" {
                    return self.run_sftp_sync_round(SyncRoundTrigger::Manual);
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
                        |v| Message::Sync(SyncMessage::NowFinished(v)),
                    );
                }
                self.sync.status =
                    Some(crate::i18n::t("sync_status_disabled").to_string());
            }
            SyncMessage::CancelInProgress => {
                if let Some(tx) = self.sync.abort_tx.take() {
                    let _ = tx.send(());
                }
                // Don't clear `sync_in_progress` here: the Task lands
                // back as `SyncNowFinished(Err("__cancelled__"))` and
                // clears it there, so the Cancel button stays visible
                // until the cancellation actually settles.
            }
            SyncMessage::EngineSpawned => {
                let Some(mut rx) = self.sync.pending_engine.take() else {
                    // `stop_sync_engine` abandoned the spawn: its oneshot
                    // receiver disappeared, the task's `send` failed and
                    // the fresh runtime dropped (stopping the engine's
                    // background tasks). Nothing to adopt.
                    return Task::none();
                };
                match rx.try_recv() {
                    Ok(Ok((runtime, event_rx))) => {
                        self.sync.runtime = Some(runtime);
                        self.sync.engine_running = true;
                        self.sync.status = Some(crate::i18n::t("sync_status_running").to_string());
                        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(event_rx);
                        return Task::stream(stream)
                            .map(|v| Message::Sync(SyncMessage::EngineEvent(v)));
                    }
                    Ok(Err(e)) => {
                        self.sync.engine_running = false;
                        self.sync.status =
                            Some(format!("{}: {e}", crate::i18n::t("sync_status_failed"),));
                        tracing::warn!("sync engine failed to start: {e}");
                        return Task::none();
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        // A STALE notification. A spawn always sends its
                        // result BEFORE emitting its own `EngineSpawned`,
                        // so an empty channel means this message belongs
                        // to an OLDER spawn (one `stop_sync_engine`
                        // abandoned during a fast off/on toggle) while
                        // the receiver just taken belongs to a newer
                        // spawn still inside its ~1s unlock. Put it
                        // back: dropping it here would fail the new
                        // spawn's send and silently drop its engine,
                        // leaving sync enabled in the UI with nothing
                        // running.
                        self.sync.pending_engine = Some(rx);
                        return Task::none();
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                        // The spawn task panicked before sending; there
                        // is nothing to adopt and `engine_running` was
                        // never set.
                        return Task::none();
                    }
                }
            }
            SyncMessage::TransportChanged(v) => {
                let mut tasks = Vec::new();
                if v != self.sync.transport {
                    // Leaving P2P: tear the engine down so QUIC/mDNS stop.
                    // Entering P2P (and enabled): bring it up.
                    self.sync.transport = v.clone();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("sync_transport", &v);
                    }
                    // Every transport's status, not just SFTP's: the
                    // old line was written when there were two, so
                    // switching away and back showed the previous
                    // transport's "Synced, N records" as if it were
                    // this one's.
                    self.sync.status = None;
                    self.sync.sftp.status = None;
                    self.sync.folder.status = None;
                    self.sync.git.status = None;
                    self.sync.webdav.status = None;
                    if !self.sync_uses_p2p() {
                        self.stop_sync_engine();
                    } else if self.sync.enabled {
                        tasks.push(self.start_sync_engine());
                    }
                }
                // The git card's availability probe is a subprocess
                // spawn: run it off the UI thread (see
                // `dispatch_git_sync::git_availability_task`), whenever
                // the git card can be on screen.
                if self.sync.transport == "git" {
                    tasks.push(crate::dispatch_git_sync::git_availability_task());
                }
                return if tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(tasks)
                };
            }
            SyncMessage::GitAvailabilityChecked(available) => {
                self.sync.git.git_available = Some(available);
            }
            SyncMessage::SftpHostChanged(id) => {
                self.sync.sftp.host_id = Some(id);
                self.sync.sftp.picker_open = false;
                self.sync.sftp.picker_search.clear();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_sftp_host_id", &id.to_string());
                }
            }
            SyncMessage::SftpHostPickerOpen => {
                self.sync.sftp.picker_open = true;
                self.sync.sftp.picker_search.clear();
            }
            SyncMessage::SftpHostPickerClose => {
                self.sync.sftp.picker_open = false;
                self.sync.sftp.picker_search.clear();
            }
            SyncMessage::SftpHostPickerSearch(v) => {
                self.sync.sftp.picker_search = v;
            }
            SyncMessage::WebdavUrlChanged(v) => {
                self.sync.webdav.url = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_webdav_url", &v);
                }
            }
            SyncMessage::WebdavUserChanged(v) => {
                self.sync.webdav.user = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_webdav_user", &v);
                }
            }
            SyncMessage::WebdavPasswordChanged(v) => {
                let v = v.into_inner();
                self.sync.webdav.password = v.clone();
                if let Some(vault) = &self.vault {
                    // Its own encrypted setting, not the group
                    // passphrase: this one is an account credential on
                    // one server, and every device has a different one.
                    let _ = vault.set_sync_webdav_password(&v);
                }
            }
            SyncMessage::PassphraseChanged(v) => {
                self.set_sync_passphrase(v.into_inner());
            }
            SyncMessage::PassphraseChangeRequested(id) => {
                // The read-only display hides the input until the user
                // explicitly asks to change the group passphrase; edit
                // mode starts with an empty buffer (the stored value
                // never pre-fills it). Focus the fresh field so the
                // first keystroke lands without a second click.
                self.sync.passphrase_editing = true;
                self.sync.passphrase_field_id = Some(id);
                return crate::widgets::focus_input(iced::widget::Id::new(id));
            }
            SyncMessage::PassphraseBlurCheck => {
                // A click landed while a passphrase edit was open. Two
                // probes, both gated on an EMPTY buffer: a non-empty edit
                // is mid-correction and survives any click elsewhere.
                // 1. Geometry: the click against the field's last drawn
                //    bounds (iced does not blur a text input on a
                //    button/blank click, so position is the signal).
                // 2. Focus: when the geometry is inconclusive, ask where
                //    iced focus actually went.
                if !self.sync.passphrase_input.as_str().is_empty() {
                    return Task::none();
                }
                let bounds = self.sync.passphrase_field_bounds.get();
                let pos = crate::subscription::live_mouse().1;
                if bounds.width > 0.0 && !bounds.contains(pos) {
                    self.exit_passphrase_edit();
                    return Task::none();
                }
                let Some(field_id) = self.sync.passphrase_field_id else {
                    return Task::none();
                };
                return iced::widget::operation::find_focused().map(move |focused| {
                    Message::Sync(SyncMessage::PassphraseBlurChecked(
                        focused.is_some_and(|id| id == iced::widget::Id::new(field_id)),
                    ))
                });
            }
            SyncMessage::PassphraseBlurChecked(still_on_field) => {
                // Focus-probe fallback: an empty edit whose field lost
                // iced focus is abandoned. Buffer content is consulted
                // here (not in the probe) so a non-empty edit survives.
                if !still_on_field && self.sync.passphrase_input.as_str().is_empty() {
                    self.exit_passphrase_edit();
                }
            }
            SyncMessage::WebdavSyncNow => {
                return self.run_webdav_sync_round(SyncRoundTrigger::Manual);
            }
            SyncMessage::WebdavRoundFinished(result) => {
                self.sync.webdav.in_progress = false;
                // Store the key that sealed this round's snapshot, or
                // drop it: either way the armed key is spent here.
                self.settle_sync_passphrase(result.is_ok());
                if result.is_ok() {
                    // The round merged on its own `VaultStore` handle,
                    // so the in-memory lists are stale until this.
                    self.load_data_from_vault();
                }
                self.sync.webdav.status = Some(match result {
                    Ok(pulled) => Ok(crate::i18n::t("snapshot_sync_ok")
                        .replace("{n}", &pulled.to_string())),
                    Err(e) => Err(e),
                });
            }
            SyncMessage::GitRemoteChanged(v) => {
                self.sync.git.remote = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_git_remote", &v);
                }
            }
            SyncMessage::GitSyncNow => return self.run_git_sync_round(SyncRoundTrigger::Manual),
            SyncMessage::GitRoundFinished(result) => {
                self.sync.git.in_progress = false;
                // Store the key that sealed this round's snapshot, or
                // drop it: either way the armed key is spent here.
                self.settle_sync_passphrase(result.is_ok());
                if result.is_ok() {
                    // A completed round proves `git` is on PATH; keep
                    // the card's cached probe current without re-spawning
                    // on every frame.
                    self.sync.git.git_available = Some(true);
                    // The round merged on its OWN `VaultStore` handle,
                    // so every in-memory list is stale: whatever it
                    // pulled is on disk and invisible until this
                    // reload. Same reason `SftpDone` does it.
                    self.load_data_from_vault();
                }
                self.sync.git.status = Some(match result {
                    Ok(pulled) => Ok(crate::i18n::t("snapshot_sync_ok")
                        .replace("{n}", &pulled.to_string())),
                    Err(e) => Err(e),
                });
            }
            SyncMessage::FolderPathChanged(v) => {
                self.sync.folder.path = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_folder_path", &v);
                }
            }
            SyncMessage::FolderPickDirectory => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_string_lossy().into_owned())
                    },
                    |picked| Message::Sync(SyncMessage::FolderDirectoryPicked(picked)),
                );
            }
            SyncMessage::FolderDirectoryPicked(picked) => {
                // A cancel is silent: the user closing a file dialog is
                // not an event worth reporting.
                if let Some(path) = picked {
                    self.sync.folder.path = path.clone();
                    if let Some(vault) = &self.vault {
                        let _ = vault.set_setting("sync_folder_path", &path);
                    }
                }
            }
            SyncMessage::FolderSyncNow => {
                return self.run_folder_sync_round(SyncRoundTrigger::Manual);
            }
            SyncMessage::FolderRoundFinished(result) => {
                self.sync.folder.in_progress = false;
                // Store the key that sealed this round's snapshot, or
                // drop it: either way the armed key is spent here.
                self.settle_sync_passphrase(result.is_ok());
                if result.is_ok() {
                    // The round merged on its OWN `VaultStore` handle,
                    // so every in-memory list is stale: whatever it
                    // pulled is on disk and invisible until this
                    // reload. Same reason `SftpDone` does it.
                    self.load_data_from_vault();
                }
                self.sync.folder.status = Some(match result {
                    Ok(pulled) => Ok(crate::i18n::t("snapshot_sync_ok")
                        .replace("{n}", &pulled.to_string())),
                    Err(e) => Err(e),
                });
            }
            SyncMessage::SftpPathChanged(v) => {
                self.sync.sftp.remote_path = v.clone();
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("sync_sftp_remote_path", &v);
                }
            }
            SyncMessage::SnapshotTick => {
                // Auto-cadence tick. Re-checks the conditions the
                // subscription mounted on, because the subscription is
                // rebuilt a frame LATE: a user who switches to manual
                // (or to P2P) mid-interval would otherwise get one more
                // round after the change. The per-transport
                // `in_progress` guard keeps a slow round from stacking.
                if !self.sync.enabled
                    || self.sync.mode != "auto"
                    || self.vault_ui.state != crate::state::VaultState::Unlocked
                {
                    return Task::none();
                }
                // `Auto`: a timed round never reads the typed buffer, so
                // it cannot seal a snapshot with a half-typed key.
                match self.sync.transport.as_str() {
                    "sftp" if !self.sync.sftp.in_progress => {
                        return self.run_sftp_sync_round(SyncRoundTrigger::Auto);
                    }
                    "folder" if !self.sync.folder.in_progress => {
                        return self.run_folder_sync_round(SyncRoundTrigger::Auto);
                    }
                    "git" if !self.sync.git.in_progress => {
                        return self.run_git_sync_round(SyncRoundTrigger::Auto);
                    }
                    "webdav" if !self.sync.webdav.in_progress => {
                        return self.run_webdav_sync_round(SyncRoundTrigger::Auto);
                    }
                    _ => {}
                }
            }
            SyncMessage::SftpDone(result) => {
                self.sync.sftp.in_progress = false;
                // Store the key that sealed this round's snapshot, or
                // drop it: either way the armed key is spent here.
                self.settle_sync_passphrase(result.is_ok());
                if result.is_ok() {
                    // The merge ran on a separate vault handle, so the
                    // in-memory lists are stale: reload to reflect it.
                    self.load_data_from_vault();
                }
                self.sync.sftp.status = Some(result);
            }
            SyncMessage::NowFinished(result) => {
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
            SyncMessage::EngineEvent(event) => {
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
                        // The engine wrote the pulled records through its
                        // own vault handle: the in-memory lists are re-read
                        // the way an external write is (entities, pins,
                        // forward rules, the open editor following its
                        // row), never through `load_data_from_vault`,
                        // which also restores tabs and runs migrations.
                        // A pulled tombstone may also have removed a cloud
                        // account whose managed kubeconfig is still on
                        // disk, so the sweep follows.
                        if pulled > 0 {
                            self.reload_after_external_write();
                            self.sweep_orphan_kubeconfigs();
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
                        self.sync.signaling_last = Some(Ok(format!("{ip}:{port}")));
                        self.sync.status = Some(format!(
                            "{} ({}): {ip}:{port}",
                            crate::i18n::t("sync_status_signaling_registered"),
                            self.sync.signaling_tick,
                        ));
                    }
                    SyncEvent::SignalingFailed { reason } => {
                        self.sync.signaling_last = Some(Err(reason.clone()));
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
        }
        Task::none()
    }
}

/// GET `<base>/healthz` and require the answer to come from an Oryxis
/// relay. Shared by the level-1 Test button and the level-2 deploy's
/// out-of-band check, so both adopt on the same evidence.
///
/// The wizard sets up a self-hosted `oryxis-relay` binary, whose
/// `/healthz` is unauthenticated (the Worker backend authenticates
/// every route, so it is not a wizard target). Probe reachability + TLS
/// without leaking the token, but require the relay to identify itself:
/// a parked domain or CDN will answer 2xx on any path, and adopting it
/// would silently overwrite a working endpoint. The `x-oryxis-relay`
/// header is the positive signal; an exact `ok` body is the fallback
/// for relays that predate the header.
pub(crate) async fn probe_relay_health(base: String) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client
        .get(format!("{base}/healthz"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    // The header alone confirms an Oryxis relay; short circuit so a
    // confirmed relay's body is never read.
    if resp.headers().contains_key("x-oryxis-relay") {
        return Ok(());
    }
    // Fallback for relays predating the header: the body must be
    // exactly "ok". Cap the read at 4 KiB so a mistyped or hostile URL
    // streaming a huge body can't OOM the app (the 8s timeout bounds
    // time, not bytes).
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > 4096 {
            return Err(crate::i18n::t("sync_relay_not_recognized").to_string());
        }
        body.extend_from_slice(&chunk);
    }
    if String::from_utf8_lossy(&body).trim() == "ok" {
        Ok(())
    } else {
        Err(crate::i18n::t("sync_relay_not_recognized").to_string())
    }
}

/// Random bearer token for a wizard-generated relay: two UUIDs' worth
/// of hex (256 bits), long enough that the token is never the weak
/// link (the relay token only gates "can talk to the relay"; payloads
/// stay end-to-end encrypted above it).
pub(crate) fn fresh_relay_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
