//! In-app session player messages (issue #71), wrapped by
//! [`crate::messages::Message::Player`]. Handled by `Oryxis::handle_player`;
//! the player surface's keyboard layer lives in `handle_player_key`.

/// Drives the read-only session playback surface on the History view.
#[derive(Debug, Clone)]
pub enum PlayerMessage {
    /// Open the player for a recording (its session-log id) and show
    /// the first frame.
    Open(uuid::Uuid),
    /// Close the player, back to the History view.
    Close,
    /// Play/pause toggle; playing again after the end restarts.
    TogglePlay,
    Restart,
    /// Cycle the playback speed through the preset steps.
    SpeedCycle,
    /// Jump to a playback position (milliseconds); applies immediately.
    Seek(f64),
    /// Scrubber drag in progress: record the target so the knob/label
    /// follow live, deferring the emulator rebuild to release.
    Scrub(f64),
    /// Scrubber released: apply the pending scrub target once.
    ScrubCommit,
    /// Playback clock tick (subscription mounted while playing).
    ClockTick,
}
