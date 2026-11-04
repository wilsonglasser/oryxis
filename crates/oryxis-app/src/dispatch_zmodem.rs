//! In-terminal ZMODEM transfer wiring (the app half of `oryxis-zmodem`).
//!
//! The detector lives in the `PtyOutput` path (`dispatch_terminal.rs`);
//! this module owns starting a transfer once detected, streaming its
//! progress back as messages, and tearing the divert down when it ends.
//!
//! Divert model: while `pane.zmodem` is `Some`, `PtyOutput` for the pane
//! is routed into the driver's wire channel instead of the emulator, and
//! keyboard input is suppressed. The driver writes protocol replies
//! straight to the pane transport's input sender (where a keystroke
//! would go). Exactly one terminal `Progress` (Completed / Aborted /
//! Error) is guaranteed, and it clears `pane.zmodem`, resuming the
//! terminal, so a transfer can never strand the pane as a dead sink.

#![allow(clippy::result_large_err)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::Task;
use iced::futures::SinkExt;
use uuid::Uuid;

use oryxis_zmodem::{Direction, Progress, TransferIo, TransferSpec};

/// Free space this never spends, so a download the user did ask for
/// cannot be the thing that fills the volume to the last byte. The
/// figure is a snapshot taken once, before the first ZFILE, and the
/// session runs for as long as the sender keeps sending, so the margin
/// also absorbs whatever else the machine writes meanwhile.
const ZMODEM_DISK_HEADROOM: u64 = 256 * 1024 * 1024;

use crate::app::{TerminalMessage, ZmodemMessage, Message, Oryxis};
use crate::state::{TerminalTransport, ZmodemPane};

impl Oryxis {
    /// Open / close the Telnet inbound raw window for a ZMODEM transfer
    /// on `pane_id`. No-op for SSH (8-bit clean) and serial (already the
    /// raw wire); Telnet needs both halves of the raw contract, or a
    /// non-UTF-8 host's charset decoder corrupts every inbound frame.
    pub(crate) fn set_zmodem_binary_inbound(&self, pane_id: Uuid, on: bool) {
        if let Some(TerminalTransport::Telnet(t)) = self
            .pane_by_id(pane_id)
            .and_then(|p| p.session.as_ref())
        {
            t.set_binary_inbound(on);
        }
    }

    /// Begin a ZMODEM transfer on `pane_id` after the detector fired.
    /// Sets up the divert (so subsequent `PtyOutput` for the pane feeds
    /// the driver) and returns a task that runs the transfer and streams
    /// its progress. `first_wire` is the detector's initial bytes.
    pub(crate) fn begin_zmodem_transfer(
        &mut self,
        pane_id: Uuid,
        direction: Direction,
        first_wire: Vec<u8>,
    ) -> Task<Message> {
        // The transport channel that carries the protocol replies.
        // ZMODEM frames are raw bytes, not keystrokes: Telnet's generic
        // input path charset-transcodes and maps line endings (which
        // corrupts binary frames), so it gets the IAC-doubling-only raw
        // sender; serial's `write_sender` is already the raw, echo-free
        // wire path; an SSH PTY channel is 8-bit clean as-is.
        let Some((wire_out, is_serial)) = self
            .pane_by_id(pane_id)
            .and_then(|p| p.session.as_ref())
            .map(|s| {
                // Serial is byte-rate-limited: it needs a small streaming
                // window so the upload watchdog never mistakes a slow drain
                // for a dead peer (see the driver's window constants).
                let is_serial = matches!(s, TerminalTransport::Serial(_));
                let wire_out = match s {
                    TerminalTransport::Telnet(t) => t.raw_write_sender(),
                    other => other.write_sender(),
                };
                (wire_out, is_serial)
            })
        else {
            // No transport (local shell): nothing to run the protocol on.
            return Task::none();
        };
        let streaming_window = if is_serial {
            oryxis_zmodem::SERIAL_STREAMING_WINDOW
        } else {
            oryxis_zmodem::DEFAULT_STREAMING_WINDOW
        };

        let (wire_tx, wire_in) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (progress_tx, progress_rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
        let abort = Arc::new(AtomicBool::new(false));
        // The relay's own handle on the divert: a declined folder dialog
        // wakes the driver through it (`zmodem_delivery`).
        let wake_tx = wire_tx.clone();

        // An OS drop staged its sources and typed `rz -y` (drop.rs): the
        // detection firing IS the proof the receiver started, so consume
        // the stash and skip the picker. Taken regardless of direction so
        // a stale stash can never leak into an unrelated later `sz`.
        let preset_sources: Option<Vec<std::path::PathBuf>> =
            self.pane_by_id_mut(pane_id).and_then(|p| {
                if p.pending_drop_sources.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut p.pending_drop_sources))
                }
            })
            .filter(|_| direction == Direction::Upload);

        // Telnet: open the inbound raw window before any protocol frame
        // can arrive; closed again wherever the divert is torn down.
        self.set_zmodem_binary_inbound(pane_id, true);
        // Seed the divert with the detector's first wire bytes, then flip
        // the pane into transfer mode so every later batch follows.
        let _ = wire_tx.send(first_wire);
        if let Some(pane) = self.pane_by_id_mut(pane_id) {
            pane.zmodem = Some(ZmodemPane {
                direction,
                wire_tx,
                abort: abort.clone(),
                file_name: None,
                batch: None,
                transferred: 0,
                total: None,
                late: Vec::new(),
            });
        } else {
            return Task::none();
        }

        let dest_dir = self.default_download_dir();
        // Whose part files these are, for a download staged in the
        // folder every host shares: the saved host, or this pane for a
        // host that was never saved. A part resumes only for its owner.
        let resume_owner = self
            .pane_by_id(pane_id)
            .and_then(|p| p.saved_conn_id())
            .map(|id| format!("host:{id}"))
            .unwrap_or_else(|| format!("pane:{pane_id}"));
        // "Ask where to save ZMODEM downloads" (Settings > Terminal)
        // governs the terminal half only: an `sz` in a terminal has no
        // destination pane to point at, unlike a dual-pane SFTP download,
        // so it keeps its own toggle (issue #230, split out on request).
        // Snapshotted here because the stream outlives `self`.
        let ask_dest = self.vaults.cur().prefs.zmodem_ask_download_dir;
        let cancelled = abort.clone();
        let io = TransferIo {
            wire_in,
            wire_out: wire_out.clone(),
            progress: progress_tx,
            abort,
        };

        // The stream owns the driver. A download starts receiving AT
        // ONCE: when the setting says to ask, it receives into the
        // staging folder while the dialog is up, and `zmodem_delivery`
        // moves each finished file once the answer is in, so the wait
        // is the user's, never the remote's (stock lrzsz `sz` gives up
        // after ~30 s of silence, which is what the dialog used to
        // race). An upload still asks first, because there is nothing
        // to send before a file is picked; lrzsz keeps retransmitting
        // its opening header meanwhile (the divert is already live, so
        // those retransmits queue on `wire_in` for the driver to drain).
        let stream = iced::stream::channel::<Message>(
            64,
            move |mut out: iced::futures::channel::mpsc::Sender<Message>| async move {
                let spec = match direction {
                    Direction::Download => {
                        let (driver_dir, placement, resume_owner) = if ask_dest {
                            let staging = match crate::zmodem_delivery::staging_dir() {
                                Some(dir) => dir,
                                None => {
                                    let _ = out
                                        .send(Message::Zmodem(ZmodemMessage::ZmodemProgress(
                                            pane_id,
                                            Progress::Error(
                                                crate::i18n::t("zmodem_no_staging").to_string(),
                                            ),
                                        )))
                                        .await;
                                    return;
                                }
                            };
                            // Private, and held for the whole transfer
                            // so the boot scan of another window never
                            // reads it mid-dialog.
                            let claim = match crate::zmodem_delivery::prepare_staging(&staging).await {
                                Ok(()) => {
                                    crate::zmodem_delivery::StagingClaim::shared(staging.clone()).await
                                }
                                Err(e) => Err(e),
                            };
                            let claim = match claim {
                                Ok(claim) => claim,
                                Err(e) => {
                                    let _ = out
                                        .send(Message::Zmodem(ZmodemMessage::ZmodemProgress(
                                            pane_id,
                                            Progress::Error(format!(
                                                "{}: {e}",
                                                staging.display()
                                            )),
                                        )))
                                        .await;
                                    return;
                                }
                            };
                            let start_in = dest_dir.clone();
                            let answer: crate::zmodem_delivery::DestinationAnswer =
                                Box::pin(async move {
                                    rfd::AsyncFileDialog::new()
                                        .set_title(crate::i18n::t("sftp_download_to"))
                                        .set_directory(&start_in)
                                        .pick_folder()
                                        .await
                                        .map(|handle| handle.path().to_path_buf())
                                });
                            (
                                staging.clone(),
                                crate::zmodem_delivery::Placement::Staged {
                                    staging,
                                    answer,
                                    claim,
                                },
                                Some(resume_owner),
                            )
                        } else {
                            // A folder the user chose for downloads
                            // keeps the plain `rz -r` resume rule.
                            (dest_dir.clone(), crate::zmodem_delivery::Placement::Direct, None)
                        };
                        // The folder the driver writes into (the staging
                        // folder, or the configured / default one) may
                        // not exist yet; its `File::create` would fail
                        // without the parent. The budget probe below
                        // reads the same volume, which is the one the
                        // bytes land on first.
                        if let Err(e) = tokio::fs::create_dir_all(&driver_dir).await {
                            let _ = out
                                .send(Message::Zmodem(ZmodemMessage::ZmodemProgress(
                                    pane_id,
                                    Progress::Error(format!(
                                        "download folder {}: {e}",
                                        driver_dir.display()
                                    )),
                                )))
                                .await;
                            return;
                        }
                        // What the whole session may write. A ZMODEM
                        // download starts from six bytes the SERVER
                        // printed, and the sender picks both the sizes
                        // and how many files follow, so the only thing
                        // standing between a hostile peer and a full
                        // disk is a number computed here. Measured after
                        // `create_dir_all` so the probe sees the real
                        // volume. It is a guard, not a promise about the
                        // folder picked later: a move into a fuller
                        // volume reports itself as a failed move.
                        let budget = oryxis_core::disk::available_space(&driver_dir)
                            .map(|free| free.saturating_sub(ZMODEM_DISK_HEADROOM));
                        Some((
                            TransferSpec::Download {
                                dest_dir: driver_dir,
                                budget,
                                resume_owner,
                            },
                            placement,
                        ))
                    }
                    Direction::Upload if preset_sources.is_some() => {
                        // OS drop: the sources were chosen by the drop
                        // gesture itself, no picker. `is_some` checked in
                        // the guard, so the expect can never fire.
                        Some((
                            TransferSpec::Upload {
                                sources: preset_sources.expect("guarded by the match arm"),
                                streaming_window,
                            },
                            crate::zmodem_delivery::Placement::Direct,
                        ))
                    }
                    Direction::Upload => {
                        // Multi-select: every picked file goes out in
                        // one ZMODEM session, in order.
                        match rfd::AsyncFileDialog::new().pick_files().await {
                            // A Cancel clicked on the overlay while the
                            // picker was up raised the flag with no
                            // driver to honour it; one spawned now would
                            // answer a pane the cancel already tore down.
                            Some(handles)
                                if !handles.is_empty()
                                    && !cancelled.load(Ordering::Relaxed) =>
                            {
                                Some((
                                    TransferSpec::Upload {
                                        sources: handles
                                            .iter()
                                            .map(|h| h.path().to_path_buf())
                                            .collect(),
                                        streaming_window,
                                    },
                                    crate::zmodem_delivery::Placement::Direct,
                                ))
                            }
                            _ => {
                                // Declined: cancel the waiting remote `rz`
                                // so it doesn't hang, and end the transfer.
                                let _ = wire_out.send(oryxis_zmodem::CANCEL.to_vec());
                                None
                            }
                        }
                    }
                };
                match spec {
                    Some((spec, placement)) => {
                        // Run the driver; it drops `progress` (via `io`)
                        // when done, which is what ends the relay's
                        // driver half. The relay closes `events` when
                        // the answer has been consumed too.
                        tokio::spawn(oryxis_zmodem::run(direction, spec, Vec::new(), io));
                        let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
                        tokio::spawn(crate::zmodem_delivery::relay(
                            progress_rx,
                            placement,
                            cancelled,
                            wake_tx,
                            events_tx,
                        ));
                        while let Some(ev) = events_rx.recv().await {
                            let msg = match ev {
                                crate::zmodem_delivery::DeliveryEvent::Progress(p) => {
                                    ZmodemMessage::ZmodemProgress(pane_id, p)
                                }
                                crate::zmodem_delivery::DeliveryEvent::Delivered { dir, files } => {
                                    ZmodemMessage::ZmodemDelivered { dir, files }
                                }
                                crate::zmodem_delivery::DeliveryEvent::MoveFailed {
                                    name,
                                    dir,
                                    staying,
                                    err,
                                } => ZmodemMessage::ZmodemMoveFailed {
                                    name,
                                    dir,
                                    staying,
                                    err,
                                },
                                crate::zmodem_delivery::DeliveryEvent::Declined { trailing } => {
                                    ZmodemMessage::ZmodemDeclined(pane_id, trailing)
                                }
                            };
                            if out.send(Message::Zmodem(msg)).await.is_err() {
                                break;
                            }
                        }
                    }
                    None => {
                        let _ = out.send(Message::Zmodem(ZmodemMessage::ZmodemProgress(pane_id, Progress::Aborted))).await;
                    }
                }
            },
        );

        // Every event is about `pane_id`: the progress overlay and the
        // "Save to folder" ask land in the window that shows the pane's
        // tab when they arrive, not the one the transfer began in.
        Task::stream(stream).map(move |m| Message::ForPane(pane_id, Box::new(m)))
    }

    /// Scan the ZMODEM staging folder once per process
    /// (`zmodem_delivery::scan_staging`): stale parts are expired, and
    /// files a previous process received under "ask" but never saved are
    /// OFFERED, never moved on their own. The scan stands down while any
    /// transfer (this window's or another's) holds the folder, so it can
    /// never touch a live dialog's files. Called at boot for an open
    /// vault and at the unlock otherwise, which is where the offer can
    /// be shown.
    pub(crate) fn zmodem_sweep_task(&mut self) -> Option<Task<Message>> {
        if std::mem::replace(&mut self.zmodem_swept, true) {
            return None;
        }
        let staging = crate::zmodem_delivery::staging_dir()?;
        Some(Task::perform(
            async move {
                let scan = crate::zmodem_delivery::scan_staging(
                    &staging,
                    crate::zmodem_delivery::PART_MAX_AGE,
                )
                .await;
                (staging, scan)
            },
            |(staging, scan)| {
                let orphans = match scan {
                    crate::zmodem_delivery::StagingScan::Busy => Vec::new(),
                    crate::zmodem_delivery::StagingScan::Scanned { orphans, expired } => {
                        if expired > 0 {
                            tracing::info!(
                                "expired {expired} stale ZMODEM part file(s) in {}",
                                staging.display()
                            );
                        }
                        orphans
                    }
                };
                Message::Zmodem(ZmodemMessage::ZmodemStagingScanned { staging, orphans })
            },
        ))
    }

    /// A transfer event for `pane_id`. `declined` marks a session that
    /// completed on the wire after the user declined the folder dialog:
    /// the divert ends exactly as on a completion, but the outcome is
    /// the cancel it was, since the files are gone.
    fn handle_zmodem_progress(
        &mut self,
        pane_id: Uuid,
        progress: Progress,
        declined: bool,
    ) -> Task<Message> {
        // Terminal events tear the divert down and replay any
        // output the transfer no longer owns: the driver's
        // `trailing` (bytes past the peer's "OO" sign-off),
        // then whatever landed on the dead wire channel while
        // this message was in flight (`late`), in arrival
        // order. Replaying synchronously through the normal
        // `PtyOutput` path keeps rendering, logging and
        // detection identical to live output, and nothing else
        // can interleave (the divert is cleared right here).
        let mut toast: Option<String> = None;
        let mut replay: Vec<u8> = Vec::new();
        let mut divert_closed = false;
        // What the OS notice names, taken while the overlay is
        // still here: the terminal arms below consume it, and
        // the transfer's own file name is the only thing that
        // tells two of them apart in a notification list.
        let mut finished: Option<(String, String)> = None;
        {
            let Some(pane) = self.pane_by_id_mut(pane_id) else {
                return Task::none();
            };
            let file_name = pane
                .zmodem
                .as_ref()
                .and_then(|zm| zm.file_name.clone())
                .unwrap_or_default();
            match progress {
                Progress::Started { name, size, batch, .. } => {
                    if let Some(zm) = pane.zmodem.as_mut() {
                        zm.file_name = Some(name);
                        zm.total = size;
                        zm.transferred = 0;
                        zm.batch = batch;
                    }
                }
                Progress::Advanced { transferred, total } => {
                    if let Some(zm) = pane.zmodem.as_mut() {
                        zm.transferred = transferred;
                        zm.total = total;
                    }
                }
                Progress::FileDone { .. } => {}
                Progress::Completed { trailing } => {
                    replay = trailing;
                    // Which way this ran, read before the state goes away.
                    let download = pane
                        .zmodem
                        .as_ref()
                        .is_some_and(|zm| zm.direction == Direction::Download);
                    if let Some(zm) = pane.zmodem.take() {
                        replay.extend(zm.late);
                    }
                    let title = if declined {
                        crate::i18n::t("transfer_notify_cancelled")
                    } else {
                        crate::i18n::t("transfer_notify_done")
                    };
                    // A finished download says where its files went, in the
                    // `ZmodemDelivered` that follows this event, so this
                    // bare "done" chip would only be a flash. A cancel
                    // still says so (nothing follows it), and so does `rz`,
                    // whose files go to the peer where no folder is ours to
                    // name. The OS notice is unaffected: `finished` feeds it
                    // either way.
                    if declined || !download {
                        toast = Some(title.to_string());
                    }
                    finished = Some((title.to_string(), file_name));
                    divert_closed = true;
                }
                Progress::Aborted => {
                    if let Some(zm) = pane.zmodem.take() {
                        replay = zm.late;
                    }
                    let title = crate::i18n::t("transfer_notify_cancelled");
                    toast = Some(title.to_string());
                    finished = Some((title.to_string(), file_name));
                    divert_closed = true;
                }
                Progress::Error(e) => {
                    if let Some(zm) = pane.zmodem.take() {
                        replay = zm.late;
                    }
                    toast = Some(format!("{}: {e}", crate::i18n::t("transfer_notify_failed")));
                    finished =
                        Some((crate::i18n::t("transfer_notify_failed").to_string(), e.to_string()));
                    divert_closed = true;
                }
            }
        }
        if divert_closed {
            // Close the Telnet inbound raw window with the divert.
            // The replayed trailing bytes skip the charset decode
            // (they were captured raw); a non-ASCII prompt tail may
            // render mojibake once, which beats corrupting the
            // whole transfer.
            self.set_zmodem_binary_inbound(pane_id, false);
        }
        // A ZMODEM transfer runs in the terminal the user
        // started it from, so away from the window it gets the
        // same OS notice an SFTP queue does; the toast is the
        // in-app half, and stays whenever that notice wasn't
        // shown. Both directions land here: `rz` and `sz` are
        // one driver, and every one of its ends is one of the
        // three arms above.
        //
        // A cancel notifies too, which is not the redundancy it
        // looks like: cancelling from the card means being in
        // front of the window, where the notice is suppressed
        // anyway. What is left is a cancel from the OTHER end,
        // and from a room away that is a transfer that stopped
        // without finishing, same as a failure.
        let notified = finished
            .as_ref()
            .is_some_and(|(title, body)| self.notify_away(title, body, toast.clone()));
        if let Some(text) = toast
            && !notified
        {
            self.set_toast(text);
        }
        if replay.is_empty() {
            Task::none()
        } else {
            self.update(Message::Terminal(TerminalMessage::PtyOutput(pane_id, replay)))
        }
    }

    /// Handle a streamed transfer event: update the overlay state and,
    /// on a terminal event, tear the divert down (resuming the terminal)
    /// and toast the outcome.
    pub(crate) fn handle_zmodem(&mut self, message: ZmodemMessage) -> Task<Message> {
        match message {
            ZmodemMessage::ZmodemProgress(pane_id, progress) => {
                self.handle_zmodem_progress(pane_id, progress, false)
            }
            ZmodemMessage::ZmodemDeclined(pane_id, trailing) => {
                self.handle_zmodem_progress(pane_id, Progress::Completed { trailing }, true)
            }
            ZmodemMessage::PickZmodemDownloadDir => Task::perform(
                tokio::task::spawn_blocking(|| {
                    rfd::FileDialog::new()
                        .set_title(crate::i18n::t("default_download_dir"))
                        .pick_folder()
                        .map(|p| p.display().to_string())
                }),
                |res| Message::Zmodem(ZmodemMessage::ZmodemDownloadDirPicked(res.ok().flatten())),
            ),
            ZmodemMessage::ZmodemDownloadDirPicked(dir) => {
                if let Some(dir) = dir {
                    self.persist_setting("zmodem_download_dir", &dir);
                    self.device_prefs.zmodem_download_dir = dir;
                }
                Task::none()
            }
            ZmodemMessage::ClearZmodemDownloadDir => {
                self.persist_setting("zmodem_download_dir", "");
                self.device_prefs.zmodem_download_dir = String::new();
                Task::none()
            }
            ZmodemMessage::ZmodemDelivered { dir, files } => {
                // The completion toast said "done" without naming a
                // folder (it did not know one at that moment); this is
                // the moment the user learns where the files went, and
                // the chip offers to open that folder.
                let text = match files.as_slice() {
                    [one] => crate::i18n::t("zmodem_saved_one")
                        .replacen("{name}", one, 1)
                        .replacen("{dir}", &dir.display().to_string(), 1),
                    many => crate::i18n::t("zmodem_saved_many")
                        .replacen("{n}", &many.len().to_string(), 1)
                        .replacen("{dir}", &dir.display().to_string(), 1),
                };
                self.set_toast_action(
                    text,
                    6,
                    crate::i18n::t("toast_show_in_folder").to_string(),
                    Message::Zmodem(ZmodemMessage::ZmodemRevealInFolder(dir)),
                );
                Task::none()
            }
            ZmodemMessage::ZmodemRevealInFolder(dir) => {
                // Acting on the chip closes it, like any other click.
                self.toast = None;
                if let Err(e) = crate::util::reveal_in_file_manager(&dir, true) {
                    tracing::warn!("reveal {} failed: {e}", dir.display());
                }
                Task::none()
            }
            ZmodemMessage::ZmodemMoveFailed { name, dir, staying, err } => {
                let text = crate::i18n::t("zmodem_move_failed")
                    .replacen("{name}", &name, 1)
                    .replacen("{dir}", &dir.display().to_string(), 1)
                    .replacen("{err}", &err, 1)
                    .replacen("{staying}", &staying.display().to_string(), 1);
                self.show_toast_secs(text, 8)
            }
            ZmodemMessage::ZmodemStagingScanned { staging, orphans } => {
                if orphans.is_empty() {
                    return Task::none();
                }
                // The dialog slot is single. Something already on it (an
                // error raised at boot or at unlock in the same instant)
                // is not ours to replace: the files stay staged and the
                // scan is re-armed, so the next unlock offers them again
                // (and a new launch would anyway).
                if self.error_dialog.is_some() {
                    self.zmodem_swept = false;
                    return Task::none();
                }
                // Received under "ask" and never answered: the choice is
                // still the user's, so it is offered again rather than
                // made for them (`zmodem_delivery` module docs). Close
                // leaves the files where they are, and the offer comes
                // back at the next launch.
                let body = match orphans.as_slice() {
                    [one] => crate::i18n::t("zmodem_orphans_body_one").replacen(
                        "{name}",
                        &one.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        1,
                    ),
                    many => crate::i18n::t("zmodem_orphans_body_many")
                        .replacen("{n}", &many.len().to_string(), 1),
                }
                .replacen("{dir}", &staging.display().to_string(), 1);
                self.error_dialog = Some(crate::state::ErrorDialog {
                    title: crate::i18n::t("zmodem_orphans_title").to_string(),
                    body,
                    link: None,
                    action: Some(crate::state::ErrorDialogAction {
                        label: crate::i18n::t("zmodem_orphans_save").to_string(),
                        message: Box::new(Message::Zmodem(ZmodemMessage::ZmodemOrphansPick(
                            orphans,
                        ))),
                        danger: false,
                    }),
                });
                Task::none()
            }
            ZmodemMessage::ZmodemOrphansPick(files) => {
                let start_in = self.default_download_dir();
                Task::perform(
                    async move {
                        let dir = rfd::AsyncFileDialog::new()
                            .set_title(crate::i18n::t("sftp_download_to"))
                            .set_directory(&start_in)
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf());
                        (files, dir)
                    },
                    |(files, dir)| Message::Zmodem(ZmodemMessage::ZmodemOrphansPicked(files, dir)),
                )
            }
            ZmodemMessage::ZmodemOrphansPicked(files, dir) => {
                // Cancelled: the files stay staged and are offered again
                // at the next launch.
                let Some(dir) = dir else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        crate::zmodem_delivery::deliver_orphans(&files, &dir)
                            .await
                            .map(|(moved, failed)| (dir, moved, failed))
                    },
                    |placed| match placed {
                        Some((dir, moved, failed)) => {
                            Message::Zmodem(ZmodemMessage::ZmodemOrphansPlaced { dir, moved, failed })
                        }
                        // A transfer is receiving into the same folder:
                        // nothing moved, and the files are offered again
                        // at the next launch.
                        None => Message::ToastShow(
                            crate::i18n::t("zmodem_orphans_busy").to_string(),
                        ),
                    },
                )
            }
            ZmodemMessage::ZmodemOrphansPlaced { dir, moved, failed } => {
                for (staying, err) in &failed {
                    tracing::warn!("saving {} to {} failed: {err}", staying.display(), dir.display());
                }
                // A failure outranks the success line: the file that did
                // not move is the one the user has to go and find.
                if let Some((staying, err)) = failed.first() {
                    let name = staying
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let text = crate::i18n::t("zmodem_move_failed")
                        .replacen("{name}", &name, 1)
                        .replacen("{dir}", &dir.display().to_string(), 1)
                        .replacen("{err}", err, 1)
                        .replacen("{staying}", &staying.display().to_string(), 1);
                    return self.show_toast_secs(text, 8);
                }
                if moved.is_empty() {
                    return Task::none();
                }
                self.update(Message::Zmodem(ZmodemMessage::ZmodemDelivered { dir, files: moved }))
            }
            ZmodemMessage::ZmodemCancel(pane_id) => {
                if let Some(pane) = self.pane_by_id_mut(pane_id)
                    && let Some(zm) = pane.zmodem.as_ref()
                {
                    // Cooperative cancel: raise the flag, then wake the
                    // driver with an empty wire chunk in case it is
                    // parked on a silent peer's recv (an empty chunk is
                    // the driver's documented wake-up). It sends the
                    // CANCEL sequence and ends with `Aborted`, which
                    // clears the divert.
                    zm.abort.store(true, Ordering::Relaxed);
                    let _ = zm.wire_tx.send(Vec::new());
                }
                Task::none()
            }
            ZmodemMessage::ZmodemDropRzTimeout(pane_id) => {
                // Sources still staged = the detector never saw the
                // remote receiver start within the window: no lrzsz, or
                // the `rz -y` landed inside a full-screen program. Clear
                // and explain. When the transfer did start, the stash was
                // consumed by `begin_zmodem_transfer` and this is a no-op,
                // so the timeout can never touch a running transfer.
                if let Some(pane) = self.pane_by_id_mut(pane_id)
                    && !pane.pending_drop_sources.is_empty()
                {
                    pane.pending_drop_sources.clear();
                    self.set_toast(crate::i18n::t("terminal_drop_no_rz").to_string());
                }
                Task::none()
            }
        }
    }
}
