//! ZMODEM in-terminal transfer progress and download-dir settings, wrapped by [`crate::messages::Message::Zmodem`]. Handled by `Oryxis::handle_zmodem`.

use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum ZmodemMessage {
    /// A ZMODEM transfer streamed a progress / outcome event for a pane.
    /// Terminal states (Completed / Aborted / Error) clear the pane's
    /// transfer and resume the terminal.
    ZmodemProgress(Uuid, oryxis_zmodem::Progress),  // (pane_id, progress)
    /// User asked to cancel the pane's in-flight ZMODEM transfer.
    ZmodemCancel(Uuid),  // (pane_id)
    /// The detect window for an OS-drop `rz -y` elapsed. If the pane
    /// still holds pending drop sources, the detector never saw the
    /// remote receiver start (no lrzsz, or the line went into a
    /// full-screen program): clear them and explain. A no-op when the
    /// transfer already started, so this can never abort one, unlike
    /// the mid-transfer watchdog it replaces from #106.
    ZmodemDropRzTimeout(Uuid),  // (pane_id)
    /// Pick the folder ZMODEM downloads are saved into.
    PickZmodemDownloadDir,
    /// ZMODEM download folder chosen (or dialog dismissed with `None`).
    ZmodemDownloadDirPicked(Option<String>),
    /// Reset the ZMODEM download folder to the OS default.
    ClearZmodemDownloadDir,
    /// A finished download landed in `dir`, reported once so the app can
    /// say where and offer to reveal it. Fires for every download that
    /// ended with its files in place: a direct one, a folder picked
    /// mid-transfer, and one picked after completion.
    ZmodemDelivered { dir: std::path::PathBuf, files: Vec<String> },
    /// Reveal a finished download's folder in the OS file manager (the
    /// completion toast's "Show in folder" action).
    ZmodemRevealInFolder(std::path::PathBuf),
    /// A finished download could not be moved into the picked folder
    /// and is still at `staying`.
    ZmodemMoveFailed {
        name: String,
        dir: std::path::PathBuf,
        staying: std::path::PathBuf,
        err: String,
    },
    /// The session completed on the wire after the user had declined
    /// the folder dialog: tears the divert down like a completion
    /// (`trailing` goes back to the terminal) but reports a cancel,
    /// because the files are gone.
    ZmodemDeclined(Uuid, Vec<u8>),  // (pane_id, trailing)
    /// The boot scan of the staging folder (`zmodem_delivery`): files a
    /// previous process received under "ask" and nobody saved, which
    /// are offered, never delivered on their own.
    ZmodemStagingScanned { staging: std::path::PathBuf, orphans: Vec<std::path::PathBuf> },
    /// The user asked to save the scan's files: open the folder picker.
    ZmodemOrphansPick(Vec<std::path::PathBuf>),
    /// The folder picker for those files answered (`None` = cancelled,
    /// the files stay in staging).
    ZmodemOrphansPicked(Vec<std::path::PathBuf>, Option<std::path::PathBuf>),
    /// The scan's files were moved into `dir`: the names that landed,
    /// and per failure where the file stayed and why.
    ZmodemOrphansPlaced {
        dir: std::path::PathBuf,
        moved: Vec<String>,
        failed: Vec<(std::path::PathBuf, String)>,
    },
}
