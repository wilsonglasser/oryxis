//! Where a ZMODEM download lands when the user is asked, and when.
//!
//! "Ask where to save downloads" used to run the folder dialog BEFORE
//! the driver spawned, so the remote `sz` sat on its opening header
//! waiting for the receiver to answer, and stock lrzsz gives that up
//! after about 30 s (`sz -t` is its own knob, tenths of a second, and
//! nothing on this side can move it). The reporter of issue #230 asked
//! for that timeout to be configurable; the honest answer is to stop
//! racing it. So the driver now starts at once, receiving into a
//! STAGING folder of the app's own, the dialog runs meanwhile, and each
//! finished file is moved to the chosen folder once both are known. The
//! dialog can stay open as long as the user likes.
//!
//! The rules, each one a test below:
//!
//! - Answered before a file completes: that file moves on its
//!   `FileDone`, and the ones already finished move right away.
//! - A file is delivered under the name the SENDER announced
//!   (sanitized), never under the name it happened to get in staging:
//!   a same-named file already parked there would otherwise hand the
//!   picked folder a " (1)" it has no collision to explain.
//! - Answered after the whole transfer completed: the parked files
//!   move, and the app says where they went, because the completion
//!   toast has already fired with no location to name.
//! - Declined while the transfer runs: the driver is asked to abort
//!   (flag plus wake-up, the driver sends the CANCEL sequence itself,
//!   never this module: after the driver has ended the divert is gone
//!   and those bytes would land in the shell), and once it has ended
//!   the part file of the file in flight is deleted with its owner
//!   record. A declined download is not something to resume, and in a
//!   folder every host shares a part nobody wants is only a trap for
//!   the next file of the same name.
//! - Declined after the last file finished but before the sender's
//!   sign-off: the session still ends `Completed` on the wire, and the
//!   relay reports it as [`DeliveryEvent::Declined`] so the app does
//!   not announce a download it has just thrown away.
//! - Declined after it completed: the finished files of THIS transfer
//!   are deleted, by the paths their `FileDone` reported; the folder
//!   is never swept.
//! - A move that fails is reported by name with where the file stayed,
//!   never redirected somewhere the user did not pick.
//!
//! Staging is ONE fixed folder (`~/.oryxis/incoming`, 0700 on unix),
//! not one per transfer: the driver's `<name>.oryxis-part` is what a
//! later `sz` of the same file resumes from, and a folder minted per
//! transfer would lose that. Because every host shares it, the driver
//! is given a resume OWNER (the saved host, or the pane for a host that
//! was never saved) and a part resumes only for the owner it records.
//!
//! Several windows (and several transfers) share the folder, so a
//! staged transfer holds a SHARED lock on `.lock` in it for as long as
//! its relay lives ([`StagingClaim`]), and the boot scan takes an
//! EXCLUSIVE one without waiting: another process mid-dialog makes the
//! scan stand down whole, so it can never touch a live transfer's parked
//! files, parts or name reservations. An OS file lock dies with its
//! process, which is what makes it a liveness test with no pid
//! bookkeeping to go stale.
//!
//! What a closed window left behind is NOT delivered on its own. Those
//! files arrived under "ask" and nobody answered; putting them in the
//! default folder would answer for the user, and the remote host
//! decided their names and contents. So [`scan_staging`] only reports
//! them, the app offers to save them to a folder the user picks, and
//! until then they stay where they are (the offer repeats at the next
//! launch). Parts and owner records older than [`PART_MAX_AGE`] are
//! expired by the same scan: nothing resumes a transfer that old.
//!
//! Uploads are untouched: there is nothing to send before a file is
//! picked, so their picker keeps running first.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use oryxis_zmodem::{PART_SUFFIX, Progress, place_file, resume_owner_path_for_part};
use tokio::sync::mpsc;

/// Parts (and their owner records) older than this are expired by the
/// boot scan. A week covers a weekend away from a half-finished `sz`.
pub(crate) const PART_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The lock file inside the staging folder. A leading dot, like the
/// owner records: the driver strips leading dots from every announced
/// name, so no download can be called this.
const CLAIM_FILE: &str = ".lock";

/// The folder the driver receives into while the user is being asked.
/// `None` without a home directory: there is no app folder to stage in,
/// and a folder relative to wherever the process was started from would
/// scatter downloads the scan could never find again.
pub(crate) fn staging_dir() -> Option<PathBuf> {
    oryxis_core::paths::oryxis_dir().map(|d| d.join("incoming"))
}

/// Create the staging folder, private to the user on unix: it holds
/// files a remote host named and filled before anyone looked at them.
pub(crate) async fn prepare_staging(dir: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(())
}

/// A hold on the staging folder: shared while a transfer uses it,
/// exclusive while the boot scan reads it. Released on drop, and by the
/// OS when the process dies.
#[derive(Debug)]
pub(crate) struct StagingClaim {
    _lock: std::fs::File,
}

fn open_claim_file(staging: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(staging.join(CLAIM_FILE))
}

impl StagingClaim {
    /// Take the shared hold a staged transfer keeps for its whole life.
    /// Waits out a boot scan that holds the exclusive one (it lasts a
    /// directory listing).
    pub(crate) async fn shared(staging: PathBuf) -> std::io::Result<Self> {
        tokio::task::spawn_blocking(move || {
            let file = open_claim_file(&staging)?;
            file.lock_shared()?;
            Ok(Self { _lock: file })
        })
        .await
        .map_err(std::io::Error::other)?
    }

    /// Try for the exclusive hold without waiting: `Ok(None)` when any
    /// transfer, in this process or another, still holds the folder.
    pub(crate) fn try_exclusive(staging: &Path) -> std::io::Result<Option<Self>> {
        let file = open_claim_file(staging)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _lock: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e),
        }
    }
}

/// The answer to "where do these files go", as the relay sees it.
pub(crate) type DestinationAnswer = Pin<Box<dyn Future<Output = Option<PathBuf>> + Send>>;

/// How a transfer's files reach their destination.
pub(crate) enum Placement {
    /// The driver writes straight into the destination (the toggle is
    /// off, or this is an upload): every `FileDone` path is final.
    Direct,
    /// The driver writes into `staging`; `answer` resolves to the folder
    /// the user picked, or `None` when they declined. `claim` is held
    /// until the relay ends, so no scan runs over the files meanwhile.
    Staged {
        staging: PathBuf,
        answer: DestinationAnswer,
        claim: StagingClaim,
    },
}

/// What the relay reports upward, one message each in the app.
#[derive(Debug)]
pub(crate) enum DeliveryEvent {
    /// A driver event, forwarded; a `FileDone` carries the path the
    /// file ended up at, which is the moved one once a folder is known.
    Progress(Progress),
    /// The folder was picked AFTER the transfer had completed, and the
    /// parked files were just moved there.
    Delivered { dir: PathBuf, files: Vec<String> },
    /// One file could not be moved into the picked folder; it is still
    /// at `staying`.
    MoveFailed {
        name: String,
        dir: PathBuf,
        staying: PathBuf,
        err: String,
    },
    /// The session completed on the wire after the user had declined:
    /// the terminal half of `Completed` (the divert ends, `trailing`
    /// goes back to the terminal), reported as the cancel it was.
    Declined { trailing: Vec<u8> },
}

/// Relay the driver's progress to `events`, placing each finished file
/// once the destination is known. Runs until the driver's channel has
/// closed AND the answer (if any) has been consumed: a native dialog
/// cannot be closed from here, and a dropped future would leave it on
/// screen with its answer thrown away.
pub(crate) async fn relay(
    mut progress: mpsc::UnboundedReceiver<Progress>,
    placement: Placement,
    abort: Arc<AtomicBool>,
    wire_tx: mpsc::UnboundedSender<Vec<u8>>,
    events: mpsc::UnboundedSender<DeliveryEvent>,
) {
    let (staging, mut answer, claim) = match placement {
        Placement::Direct => (None, None, None),
        Placement::Staged {
            staging,
            answer,
            claim,
        } => (Some(staging), Some(answer), Some(claim)),
    };
    // `None` until the user answers; then the folder, or `None` again
    // inside for a decline.
    let mut destination: Option<Option<PathBuf>> = None;
    // Finished files waiting for the answer: where each sits, and the
    // name the sender announced for it.
    let mut parked: Vec<(PathBuf, String)> = Vec::new();
    // The (sanitized) name of the file in flight, which is what it is
    // delivered under.
    let mut announced: Option<String> = None;
    // Where the driver is writing the file in flight. Reported by the
    // driver rather than derived from the name: a part another transfer
    // holds is never shared, so this one may carry a number.
    let mut in_flight_part: Option<PathBuf> = None;
    let mut driver_done = false;
    loop {
        tokio::select! {
            picked = async { answer.as_mut().expect("guarded").await }, if answer.is_some() => {
                answer = None;
                match picked {
                    Some(dir) => {
                        let moved = place_parked(&mut parked, &dir, &events).await;
                        if driver_done && !moved.is_empty() {
                            let _ = events.send(DeliveryEvent::Delivered { dir: dir.clone(), files: moved });
                        }
                        destination = Some(Some(dir));
                    }
                    None => {
                        destination = Some(None);
                        for (path, _) in parked.drain(..) {
                            let _ = tokio::fs::remove_file(&path).await;
                        }
                        if !driver_done {
                            // The driver owns the wire: the flag plus
                            // an empty chunk is its documented wake-up,
                            // and it answers with the CANCEL sequence.
                            abort.store(true, Ordering::Relaxed);
                            let _ = wire_tx.send(Vec::new());
                        }
                    }
                }
            }
            event = progress.recv(), if !driver_done => {
                match event {
                    None => driver_done = true,
                    Some(Progress::Started { name, size, batch, part }) if staging.is_some() => {
                        announced = Some(name.clone());
                        in_flight_part = part.clone();
                        let _ = events.send(DeliveryEvent::Progress(Progress::Started {
                            name,
                            size,
                            batch,
                            part,
                        }));
                    }
                    Some(Progress::FileDone { name, path: Some(src) }) if staging.is_some() => {
                        // What the sender called it; the staged name
                        // may carry a collision suffix of its own.
                        let wanted = announced.take().unwrap_or_else(|| name.clone());
                        // Its part became this file.
                        in_flight_part = None;
                        let (name, path) = match &destination {
                            Some(Some(dir)) => {
                                match place_file(&src, dir, &wanted).await {
                                    Ok(moved) => (file_name(&moved), Some(moved)),
                                    Err(err) => {
                                        let _ = events.send(DeliveryEvent::MoveFailed {
                                            name: wanted.clone(),
                                            dir: dir.clone(),
                                            staying: src.clone(),
                                            err,
                                        });
                                        (name, Some(src))
                                    }
                                }
                            }
                            // Declined: a file that completed between
                            // the decline and the abort is not wanted.
                            Some(None) => {
                                let _ = tokio::fs::remove_file(&src).await;
                                (name, None)
                            }
                            None => {
                                parked.push((src.clone(), wanted));
                                (name, Some(src))
                            }
                        };
                        let _ = events.send(DeliveryEvent::Progress(Progress::FileDone { name, path }));
                    }
                    Some(Progress::Completed { trailing }) if matches!(destination, Some(None)) => {
                        let _ = events.send(DeliveryEvent::Declined { trailing });
                    }
                    Some(p) => {
                        let _ = events.send(DeliveryEvent::Progress(p));
                    }
                }
            }
        }
        if driver_done && answer.is_none() {
            break;
        }
    }
    // A declined transfer leaves nothing behind in staging: the file it
    // was in the middle of goes with its owner record. Only now, once
    // the driver has ended, so the removal cannot race its last write.
    if let (Some(_), Some(None), Some(part)) = (&staging, &destination, &in_flight_part) {
        let _ = tokio::fs::remove_file(part).await;
        if let Some(record) = resume_owner_path_for_part(part) {
            let _ = tokio::fs::remove_file(record).await;
        }
    }
    drop(claim);
}

/// Move every parked file into `dir` under its announced name,
/// reporting each failure; returns the names the files landed under. A
/// folder the user typed into the dialog may not exist yet.
async fn place_parked(
    parked: &mut Vec<(PathBuf, String)>,
    dir: &Path,
    events: &mpsc::UnboundedSender<DeliveryEvent>,
) -> Vec<String> {
    let mut moved = Vec::new();
    let ready = match tokio::fs::create_dir_all(dir).await {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("create {}: {e}", dir.display())),
    };
    for (src, wanted) in parked.drain(..) {
        let outcome = match &ready {
            Ok(()) => place_file(&src, dir, &wanted).await,
            Err(e) => Err(e.clone()),
        };
        match outcome {
            Ok(landed) => moved.push(file_name(&landed)),
            Err(err) => {
                let _ = events.send(DeliveryEvent::MoveFailed {
                    name: wanted,
                    dir: dir.to_path_buf(),
                    staying: src,
                    err,
                });
            }
        }
    }
    moved
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "received.bin".to_string())
}

/// Whether a staging entry is the app's own bookkeeping rather than a
/// download, recognised by its exact name: the claim file. Parts, owner
/// records (`.<stem>.oryxis-part`) and the cross-volume copy's temporary
/// all end in the part suffix and are handled as parts before this is
/// asked. A leading dot is deliberately NOT the test: anything else in
/// the folder is a file somebody may want, and hiding it would keep it
/// there, unoffered, for good.
fn is_internal(name: &str) -> bool {
    name == CLAIM_FILE
}

/// What the boot scan of the staging folder found.
#[derive(Debug, PartialEq)]
pub(crate) enum StagingScan {
    /// Another transfer holds the folder: nothing was read or touched.
    Busy,
    /// Finished files nobody picked a folder for, and how many stale
    /// parts / owner records were expired.
    Scanned { orphans: Vec<PathBuf>, expired: usize },
}

/// Read what a previous process left in `staging`, under the exclusive
/// claim so no live transfer can be in it. Expires parts and owner
/// records older than `max_age`; REPORTS finished files and moves
/// nothing (see the module docs for why).
pub(crate) async fn scan_staging(staging: &Path, max_age: Duration) -> StagingScan {
    if !staging.is_dir() {
        return StagingScan::Scanned {
            orphans: Vec::new(),
            expired: 0,
        };
    }
    let claim = match StagingClaim::try_exclusive(staging) {
        Ok(Some(claim)) => claim,
        Ok(None) => return StagingScan::Busy,
        Err(e) => {
            tracing::warn!("ZMODEM staging folder {} not scanned: {e}", staging.display());
            return StagingScan::Busy;
        }
    };
    let mut orphans = Vec::new();
    let mut expired = 0usize;
    if let Ok(mut entries) = tokio::fs::read_dir(staging).await {
        let now = std::time::SystemTime::now();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
                continue;
            };
            if !meta.file_type().is_file() {
                continue;
            }
            let name = file_name(&path);
            if name.ends_with(PART_SUFFIX) {
                let stale = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .is_some_and(|age| age > max_age);
                if stale && tokio::fs::remove_file(&path).await.is_ok() {
                    expired += 1;
                }
                continue;
            }
            if is_internal(&name) {
                continue;
            }
            orphans.push(path);
        }
    }
    orphans.sort();
    drop(claim);
    StagingScan::Scanned { orphans, expired }
}

/// Move the files a scan reported into `dir`, the folder the user
/// picked for them. Each is re-checked first: something may have
/// changed in the folder since the scan (the user tidied it by hand).
/// Returns the names that landed and, per failure, where the file
/// stayed and why.
///
/// `None` when a transfer holds the staging folder: the pick can come
/// minutes after the scan, and a live transfer may by then have written
/// a file under one of the scanned names, so nothing moves without the
/// same exclusive claim the scan took. The files stay staged and are
/// offered again at the next launch.
pub(crate) async fn deliver_orphans(
    files: &[PathBuf],
    dir: &Path,
) -> Option<(Vec<String>, Vec<(PathBuf, String)>)> {
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    let Some(staging) = files.first().and_then(|f| f.parent()) else {
        return Some((moved, failed));
    };
    let _claim = match StagingClaim::try_exclusive(staging) {
        Ok(Some(claim)) => claim,
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!("ZMODEM staging folder {} not claimed: {e}", staging.display());
            return None;
        }
    };
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        let err = format!("create {}: {e}", dir.display());
        for f in files {
            failed.push((f.clone(), err.clone()));
        }
        return Some((moved, failed));
    }
    for src in files {
        let name = file_name(src);
        match tokio::fs::symlink_metadata(src).await {
            Ok(m) if m.file_type().is_file() => {}
            // Gone or replaced since the scan: nothing of ours to move.
            _ => continue,
        }
        match place_file(src, dir, &name).await {
            Ok(landed) => moved.push(file_name(&landed)),
            Err(err) => failed.push((src.clone(), err)),
        }
    }
    Some((moved, failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oryxis_zmodem::resume_owner_path;
    use tokio::sync::oneshot;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oryxis-zm-delivery-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct Harness {
        staging: PathBuf,
        dest: PathBuf,
        progress: mpsc::UnboundedSender<Progress>,
        answer: Option<oneshot::Sender<Option<PathBuf>>>,
        abort: Arc<AtomicBool>,
        wire: mpsc::UnboundedReceiver<Vec<u8>>,
        events: mpsc::UnboundedReceiver<DeliveryEvent>,
        task: tokio::task::JoinHandle<()>,
    }

    async fn start(tag: &str) -> Harness {
        let root = scratch(tag);
        let staging = root.join("incoming");
        let dest = root.join("chosen");
        prepare_staging(&staging).await.unwrap();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        let (answer_tx, answer_rx) = oneshot::channel::<Option<PathBuf>>();
        let (wire_tx, wire_rx) = mpsc::unbounded_channel();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let abort = Arc::new(AtomicBool::new(false));
        let placement = Placement::Staged {
            staging: staging.clone(),
            answer: Box::pin(async move { answer_rx.await.unwrap_or(None) }),
            claim: StagingClaim::shared(staging.clone()).await.unwrap(),
        };
        let task = tokio::spawn(relay(
            progress_rx,
            placement,
            abort.clone(),
            wire_tx,
            events_tx,
        ));
        Harness {
            staging,
            dest,
            progress: progress_tx,
            answer: Some(answer_tx),
            abort,
            wire: wire_rx,
            events: events_rx,
            task,
        }
    }

    impl Harness {
        /// The driver announcing a file (the name is the sanitized one),
        /// written to the part of the same name.
        async fn started(&mut self, name: &str) {
            let part = self.staging.join(format!("{name}{PART_SUFFIX}"));
            self.started_at(name, part).await;
        }
        /// The driver announcing a file written to `part`: a numbered one
        /// when another transfer holds the part of that name.
        async fn started_at(&mut self, name: &str, part: PathBuf) {
            self.progress
                .send(Progress::Started {
                    name: name.to_string(),
                    size: None,
                    batch: None,
                    part: Some(part),
                })
                .unwrap();
            assert!(matches!(
                self.next().await,
                DeliveryEvent::Progress(Progress::Started { .. })
            ));
        }
        /// The driver finishing a file: it exists in staging under its
        /// final name, and `FileDone` names it.
        fn finish(&self, name: &str, body: &[u8]) -> PathBuf {
            let path = self.staging.join(name);
            std::fs::write(&path, body).unwrap();
            self.progress
                .send(Progress::FileDone {
                    name: name.to_string(),
                    path: Some(path.clone()),
                })
                .unwrap();
            path
        }
        fn complete(&mut self) {
            self.progress
                .send(Progress::Completed {
                    trailing: Vec::new(),
                })
                .unwrap();
        }
        fn answer(&mut self, dir: Option<PathBuf>) {
            self.answer.take().unwrap().send(dir).unwrap();
        }
        async fn next(&mut self) -> DeliveryEvent {
            tokio::time::timeout(std::time::Duration::from_secs(5), self.events.recv())
                .await
                .expect("an event within 5 s")
                .expect("relay still running")
        }
        /// Wait until the relay has acted on the answer, observed on
        /// disk: with the dialog and a `FileDone` both ready, `select!`
        /// picks either first, so a test that needs the answer applied
        /// before the next file waits for its footprint.
        async fn wait_until(&self, cond: impl Fn() -> bool) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !cond() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "condition not met within 5 s"
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        /// The driver has ended while the dialog may still be up.
        async fn driver_ended(&mut self) {
            drop(std::mem::replace(
                &mut self.progress,
                mpsc::unbounded_channel().0,
            ));
            tokio::task::yield_now().await;
        }
        async fn finish_relay(self) {
            drop(self.progress);
            tokio::time::timeout(std::time::Duration::from_secs(5), self.task)
                .await
                .expect("relay ends once the driver and the answer are both done")
                .unwrap();
        }
    }

    fn done_path(ev: DeliveryEvent) -> PathBuf {
        match ev {
            DeliveryEvent::Progress(Progress::FileDone { path: Some(p), .. }) => p,
            other => panic!("expected FileDone, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_folder_picked_first_moves_every_file_as_it_finishes() {
        let mut h = start("early").await;
        h.answer(Some(h.dest.clone()));
        // The answer is applied once the picked folder exists.
        let dest = h.dest.clone();
        h.wait_until(|| dest.is_dir()).await;
        h.finish("a.txt", b"aaa");
        let a = done_path(h.next().await);
        assert_eq!(a, h.dest.join("a.txt"));
        assert_eq!(std::fs::read(&a).unwrap(), b"aaa");
        assert!(!h.staging.join("a.txt").exists());
        h.finish("b.txt", b"bbb");
        assert_eq!(done_path(h.next().await), h.dest.join("b.txt"));
        h.complete();
        assert!(matches!(
            h.next().await,
            DeliveryEvent::Progress(Progress::Completed { .. })
        ));
        h.finish_relay().await;
    }

    #[tokio::test]
    async fn a_folder_picked_mid_transfer_moves_the_parked_files_and_the_rest_on_arrival() {
        let mut h = start("mid").await;
        h.finish("first.bin", b"1");
        // Not yet known where: the file stays in staging, reported as is.
        assert_eq!(done_path(h.next().await), h.staging.join("first.bin"));
        h.answer(Some(h.dest.clone()));
        let first = h.dest.join("first.bin");
        h.wait_until(|| first.exists()).await;
        // No Delivered toast mid-transfer: the completion toast is coming.
        h.finish("second.bin", b"2");
        assert_eq!(done_path(h.next().await), h.dest.join("second.bin"));
        assert!(
            h.dest.join("first.bin").exists(),
            "parked file moved on the answer"
        );
        assert!(!h.staging.join("first.bin").exists());
        h.complete();
        assert!(matches!(
            h.next().await,
            DeliveryEvent::Progress(Progress::Completed { .. })
        ));
        h.finish_relay().await;
    }

    #[tokio::test]
    async fn a_file_is_delivered_under_the_name_the_sender_announced() {
        let mut h = start("announced").await;
        // The driver met a same-named file in staging and finalized
        // this one as "report (1).pdf" there.
        h.started("report.pdf").await;
        h.finish("report (1).pdf", b"new");
        assert_eq!(done_path(h.next().await), h.staging.join("report (1).pdf"));
        h.started("notes.txt").await;
        h.answer(Some(h.dest.clone()));
        let parked = h.dest.join("report.pdf");
        // `place_file` reserves the destination with an empty marker before
        // replacing it, so existence alone can observe the marker mid-move.
        h.wait_until(|| std::fs::read(&parked).is_ok_and(|bytes| bytes == b"new"))
            .await;
        assert_eq!(std::fs::read(&parked).unwrap(), b"new");
        // Once the folder is known, the announced name travels too.
        h.finish("notes (1).txt", b"n");
        match h.next().await {
            DeliveryEvent::Progress(Progress::FileDone { name, path: Some(p) }) => {
                assert_eq!(name, "notes.txt");
                assert_eq!(p, h.dest.join("notes.txt"));
            }
            other => panic!("expected FileDone, got {other:?}"),
        }
        h.complete();
        let _ = h.next().await;
        h.finish_relay().await;
    }

    #[tokio::test]
    async fn a_folder_picked_after_completion_moves_the_files_and_says_where() {
        let mut h = start("late").await;
        h.finish("report.pdf", b"pdf");
        assert_eq!(done_path(h.next().await), h.staging.join("report.pdf"));
        h.complete();
        assert!(matches!(
            h.next().await,
            DeliveryEvent::Progress(Progress::Completed { .. })
        ));
        // The driver is gone; the dialog is still up.
        h.driver_ended().await;
        // A folder that does not exist yet is created.
        let picked = h.dest.join("deeper");
        h.answer(Some(picked.clone()));
        match h.next().await {
            DeliveryEvent::Delivered { dir, files } => {
                assert_eq!(dir, picked);
                assert_eq!(files, vec!["report.pdf".to_string()]);
            }
            other => panic!("expected Delivered, got {other:?}"),
        }
        assert_eq!(std::fs::read(picked.join("report.pdf")).unwrap(), b"pdf");
        assert!(!h.staging.join("report.pdf").exists());
        h.finish_relay().await;
    }

    #[tokio::test]
    async fn declining_while_running_aborts_the_driver_and_leaves_nothing_in_staging() {
        let mut h = start("decline-running").await;
        h.finish("done.bin", b"x");
        assert_eq!(done_path(h.next().await), h.staging.join("done.bin"));
        // The file in flight: its part and its owner record.
        h.started("inflight.bin").await;
        let part = h.staging.join(format!("inflight.bin{PART_SUFFIX}"));
        let record = resume_owner_path(&h.staging, "inflight.bin");
        std::fs::write(&part, b"half").unwrap();
        std::fs::write(&record, b"host\n9\n").unwrap();
        // Another transfer's part of another name is none of its business.
        let other = h.staging.join(format!("other.bin{PART_SUFFIX}"));
        std::fs::write(&other, b"o").unwrap();
        h.answer(None);
        // Cooperative cancel: the flag and the empty wake-up chunk, the
        // CANCEL bytes themselves are the driver's to send.
        let wake = tokio::time::timeout(std::time::Duration::from_secs(5), h.wire.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(wake.is_empty());
        assert!(h.abort.load(Ordering::Relaxed));
        assert!(
            !h.staging.join("done.bin").exists(),
            "finished file discarded"
        );
        // The driver honours the flag and ends.
        h.progress.send(Progress::Aborted).unwrap();
        assert!(matches!(
            h.next().await,
            DeliveryEvent::Progress(Progress::Aborted)
        ));
        h.finish_relay().await;
        assert!(!part.exists(), "a declined part is not a resume anchor");
        assert!(!record.exists());
        assert!(other.exists());
    }

    /// A decline removes the part THIS transfer was writing, which the
    /// driver names: with another live transfer of the same name holding
    /// `inflight.bin.oryxis-part`, ours is a numbered part, and the other
    /// one's part and owner record are left alone.
    #[tokio::test]
    async fn declining_removes_its_own_numbered_part_and_nothing_else() {
        let mut h = start("decline-numbered").await;
        let theirs = h.staging.join(format!("inflight.bin{PART_SUFFIX}"));
        let their_record = resume_owner_path(&h.staging, "inflight.bin");
        std::fs::write(&theirs, b"theirs").unwrap();
        std::fs::write(&their_record, b"host-a\n9\n").unwrap();
        let ours = h.staging.join(format!("inflight (1).bin{PART_SUFFIX}"));
        let our_record = resume_owner_path(&h.staging, "inflight (1).bin");
        h.started_at("inflight.bin", ours.clone()).await;
        std::fs::write(&ours, b"half").unwrap();
        std::fs::write(&our_record, b"host-b\n9\n").unwrap();
        h.answer(None);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), h.wire.recv()).await;
        h.progress.send(Progress::Aborted).unwrap();
        let _ = h.next().await;
        h.finish_relay().await;
        assert!(!ours.exists(), "our part outlived the decline");
        assert!(!our_record.exists(), "our owner record outlived the decline");
        assert_eq!(std::fs::read(&theirs).unwrap(), b"theirs");
        assert!(their_record.exists());
    }

    #[tokio::test]
    async fn declining_before_the_sign_off_is_reported_as_declined_not_completed() {
        let mut h = start("decline-signoff").await;
        h.finish("last.bin", b"l");
        assert_eq!(done_path(h.next().await), h.staging.join("last.bin"));
        h.answer(None);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), h.wire.recv())
            .await
            .unwrap();
        // Too late for the cancel: the sender had already finished.
        h.progress
            .send(Progress::Completed {
                trailing: b"$ ".to_vec(),
            })
            .unwrap();
        match h.next().await {
            DeliveryEvent::Declined { trailing } => assert_eq!(trailing, b"$ "),
            other => panic!("expected Declined, got {other:?}"),
        }
        let staging = h.staging.clone();
        h.finish_relay().await;
        assert!(!staging.join("last.bin").exists());
    }

    #[tokio::test]
    async fn declining_after_completion_deletes_only_this_transfers_files() {
        let mut h = start("decline-done").await;
        // Something else's finished file in the same staging folder.
        std::fs::write(h.staging.join("someone-elses.bin"), b"keep").unwrap();
        h.finish("mine.bin", b"drop");
        assert_eq!(done_path(h.next().await), h.staging.join("mine.bin"));
        h.complete();
        assert!(matches!(
            h.next().await,
            DeliveryEvent::Progress(Progress::Completed { .. })
        ));
        h.driver_ended().await;
        h.answer(None);
        let staging = h.staging.clone();
        let abort = h.abort.clone();
        h.finish_relay().await;
        assert!(!staging.join("mine.bin").exists());
        assert!(staging.join("someone-elses.bin").exists());
        // Nothing was aborted: the driver had already ended.
        assert!(!abort.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn a_move_that_fails_is_reported_and_the_file_stays_put() {
        let mut h = start("move-fails").await;
        h.finish("stuck.bin", b"s");
        assert_eq!(done_path(h.next().await), h.staging.join("stuck.bin"));
        h.complete();
        assert!(matches!(
            h.next().await,
            DeliveryEvent::Progress(Progress::Completed { .. })
        ));
        h.driver_ended().await;
        // A destination that cannot be a folder: a regular file.
        let blocked = h.dest.parent().unwrap().join("not-a-folder");
        std::fs::write(&blocked, b"file").unwrap();
        h.answer(Some(blocked.clone()));
        match h.next().await {
            DeliveryEvent::MoveFailed {
                name, dir, staying, ..
            } => {
                assert_eq!(name, "stuck.bin");
                assert_eq!(dir, blocked);
                assert_eq!(staying, h.staging.join("stuck.bin"));
            }
            other => panic!("expected MoveFailed, got {other:?}"),
        }
        assert!(h.staging.join("stuck.bin").exists());
        h.finish_relay().await;
    }

    #[tokio::test]
    async fn direct_placement_forwards_everything_untouched() {
        let root = scratch("direct");
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        let (wire_tx, _wire_rx) = mpsc::unbounded_channel();
        let (events_tx, mut events_rx) = mpsc::unbounded_channel();
        let abort = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn(relay(
            progress_rx,
            Placement::Direct,
            abort,
            wire_tx,
            events_tx,
        ));
        let path = root.join("direct.bin");
        std::fs::write(&path, b"d").unwrap();
        progress_tx
            .send(Progress::FileDone {
                name: "direct.bin".into(),
                path: Some(path.clone()),
            })
            .unwrap();
        progress_tx
            .send(Progress::Completed {
                trailing: b"$ ".to_vec(),
            })
            .unwrap();
        drop(progress_tx);
        assert_eq!(done_path(events_rx.recv().await.unwrap()), path);
        assert!(matches!(
            events_rx.recv().await.unwrap(),
            DeliveryEvent::Progress(Progress::Completed { trailing }) if trailing == b"$ "
        ));
        assert!(events_rx.recv().await.is_none());
        task.await.unwrap();
        assert!(path.exists());
    }

    fn age(path: &Path, by: Duration) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(std::time::SystemTime::now() - by).unwrap();
    }

    #[tokio::test]
    async fn the_scan_reports_orphans_moves_nothing_and_expires_old_parts() {
        let root = scratch("scan");
        let staging = root.join("incoming");
        prepare_staging(&staging).await.unwrap();
        std::fs::write(staging.join("finished.txt"), b"f").unwrap();
        let fresh_part = staging.join(format!("half.bin{PART_SUFFIX}"));
        let old_part = staging.join(format!("ancient.bin{PART_SUFFIX}"));
        let old_record = resume_owner_path(&staging, "ancient.bin");
        std::fs::write(&fresh_part, b"h").unwrap();
        std::fs::write(&old_part, b"a").unwrap();
        std::fs::write(&old_record, b"host\n1\n").unwrap();
        age(&old_part, PART_MAX_AGE * 2);
        age(&old_record, PART_MAX_AGE * 2);
        std::fs::create_dir_all(staging.join("a-folder")).unwrap();
        let scan = scan_staging(&staging, PART_MAX_AGE).await;
        assert_eq!(
            scan,
            StagingScan::Scanned {
                orphans: vec![staging.join("finished.txt")],
                expired: 2,
            }
        );
        // Reported, not delivered: the file is still where it was.
        assert!(staging.join("finished.txt").exists());
        assert!(fresh_part.exists());
        assert!(!old_part.exists());
        assert!(!old_record.exists());
        // The claim file is bookkeeping, never an orphan.
        assert!(staging.join(CLAIM_FILE).exists());
    }

    /// Only the claim file is bookkeeping: any other dot file in the
    /// folder is offered like every finished file, never hidden there.
    #[tokio::test]
    async fn a_dot_file_is_offered_and_only_the_claim_is_internal() {
        let root = scratch("scan-dot");
        let staging = root.join("incoming");
        prepare_staging(&staging).await.unwrap();
        std::fs::write(staging.join(".env"), b"e").unwrap();
        let scan = scan_staging(&staging, PART_MAX_AGE).await;
        assert_eq!(
            scan,
            StagingScan::Scanned {
                orphans: vec![staging.join(".env")],
                expired: 0,
            }
        );
    }

    /// Saving the orphans waits for no one and moves nothing while a
    /// transfer holds the staging folder.
    #[tokio::test]
    async fn orphans_are_not_moved_while_a_transfer_holds_the_folder() {
        let root = scratch("deliver-busy");
        let staging = root.join("incoming");
        let dest = root.join("picked");
        prepare_staging(&staging).await.unwrap();
        std::fs::write(staging.join("waiting.txt"), b"w").unwrap();
        let files = vec![staging.join("waiting.txt")];
        let held = StagingClaim::shared(staging.clone()).await.unwrap();
        assert!(deliver_orphans(&files, &dest).await.is_none());
        assert!(staging.join("waiting.txt").exists());
        drop(held);
        let (moved, failed) = deliver_orphans(&files, &dest).await.unwrap();
        assert_eq!(moved, vec!["waiting.txt".to_string()]);
        assert!(failed.is_empty());
    }

    #[tokio::test]
    async fn the_scan_stands_down_while_any_transfer_holds_the_folder() {
        let root = scratch("scan-busy");
        let staging = root.join("incoming");
        prepare_staging(&staging).await.unwrap();
        std::fs::write(staging.join("parked.bin"), b"p").unwrap();
        let held = StagingClaim::shared(staging.clone()).await.unwrap();
        assert_eq!(scan_staging(&staging, PART_MAX_AGE).await, StagingScan::Busy);
        assert!(staging.join("parked.bin").exists());
        drop(held);
        assert!(matches!(
            scan_staging(&staging, PART_MAX_AGE).await,
            StagingScan::Scanned { .. }
        ));
        // Two transfers share the folder at once.
        let a = StagingClaim::shared(staging.clone()).await.unwrap();
        let b = StagingClaim::shared(staging.clone()).await.unwrap();
        assert_eq!(scan_staging(&staging, PART_MAX_AGE).await, StagingScan::Busy);
        drop((a, b));
    }

    #[tokio::test]
    async fn orphans_go_where_the_user_picked_without_clobbering() {
        let root = scratch("deliver");
        let staging = root.join("incoming");
        let dest = root.join("picked");
        prepare_staging(&staging).await.unwrap();
        std::fs::write(staging.join("finished.txt"), b"f").unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("finished.txt"), b"original").unwrap();
        let files = vec![staging.join("finished.txt"), staging.join("gone.txt")];
        let (moved, failed) = deliver_orphans(&files, &dest).await.unwrap();
        assert_eq!(moved, vec!["finished (1).txt".to_string()]);
        assert!(failed.is_empty(), "a file gone since the scan is skipped: {failed:?}");
        assert_eq!(std::fs::read(dest.join("finished.txt")).unwrap(), b"original");
        assert_eq!(std::fs::read(dest.join("finished (1).txt")).unwrap(), b"f");
        assert!(!staging.join("finished.txt").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_staging_folder_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("private");
        let staging = root.join("incoming");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).unwrap();
        prepare_staging(&staging).await.unwrap();
        let mode = std::fs::metadata(&staging).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
