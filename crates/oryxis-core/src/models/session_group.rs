use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A saved split-panel arrangement. Unlike a `Connection`, it carries no
/// connection data of its own: every leaf references a host by id (a live
/// reference, pruned with a warning if the host is later deleted) or is a
/// local shell. Opening it rebuilds a single tab with the exact split tree
/// (axes + ratios) and runs each pane's per-pane initial script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGroup {
    pub id: Uuid,
    pub label: String,
    /// Folder (Group) this session group lives under, same as Connection.group_id.
    pub group_id: Option<Uuid>,
    pub color: Option<String>,
    pub icon_style: Option<String>,
    /// Serialized split tree mirroring iced's `pane_grid::Node`.
    pub layout: PaneLayout,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Binary split tree mirroring `iced::widget::pane_grid::Node` /
/// `Configuration`, so it can be snapshotted from a live grid and restored
/// via `State::with_configuration`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaneLayout {
    Split {
        axis: SplitAxis,
        ratio: f32,
        a: Box<PaneLayout>,
        b: Box<PaneLayout>,
    },
    Leaf(PaneMember),
}

/// Mirrors `iced::widget::pane_grid::Axis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

/// One pane in the saved arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneMember {
    pub source: PaneSource,
    /// Override-with-fallback: when set, runs instead of the host's own
    /// `initial_command` for this pane; when empty/None, the host's
    /// `initial_command` runs (local shells have no host, so only this runs).
    pub initial_script: Option<String>,
}

/// What a pane reconnects to on open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaneSource {
    /// Live reference to a saved Connection by id.
    Host(Uuid),
    /// A local terminal; program/args/label captured so the same shell is restored.
    LocalShell {
        program: String,
        args: Vec<String>,
        label: String,
    },
}

impl SessionGroup {
    pub fn new(label: impl Into<String>, layout: PaneLayout) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            group_id: None,
            color: None,
            icon_style: None,
            layout,
            last_used: None,
            created_at: now,
            updated_at: now,
        }
    }
}
