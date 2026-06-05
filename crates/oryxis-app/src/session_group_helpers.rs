//! Pure helpers for session groups: snapshotting a live pane grid into a
//! serializable `PaneLayout`, merging per-pane scripts back in, and pruning
//! leaves (ephemeral panes on save, dangling host references on open).
//!
//! Kept free of dispatch / `Task` machinery so it stays unit-testable; the
//! `Oryxis` handlers live in `dispatch_session_group.rs`.

use iced::widget::pane_grid::{Axis, Node};
use oryxis_core::models::{PaneLayout, PaneMember, PaneSource, SplitAxis};

use crate::state::{PaneOrigin, PaneScriptRow, TerminalTab};

pub(crate) fn to_split_axis(axis: Axis) -> SplitAxis {
    match axis {
        Axis::Horizontal => SplitAxis::Horizontal,
        Axis::Vertical => SplitAxis::Vertical,
    }
}

pub(crate) fn from_split_axis(axis: SplitAxis) -> Axis {
    match axis {
        SplitAxis::Horizontal => Axis::Horizontal,
        SplitAxis::Vertical => Axis::Vertical,
    }
}

/// Walk a tab's live pane grid into a serializable layout, pruning panes
/// that can't be referenced by id (cloud / ephemeral). Returns `None` if
/// every leaf was pruned (nothing savable). The second tuple element is the
/// ordered editor rows (one per surviving leaf, scripts start empty), in the
/// same left-to-right order as the layout's leaf walk.
pub(crate) fn snapshot_tab_layout(
    tab: &TerminalTab,
) -> Option<(PaneLayout, Vec<PaneScriptRow>)> {
    let mut rows = Vec::new();
    let layout = walk_snapshot(tab.pane_grid.layout(), tab, &mut rows);
    layout.map(|l| (l, rows))
}

fn walk_snapshot(
    node: &Node,
    tab: &TerminalTab,
    rows: &mut Vec<PaneScriptRow>,
) -> Option<PaneLayout> {
    match node {
        Node::Split {
            axis, ratio, a, b, ..
        } => {
            let a = walk_snapshot(a, tab, rows);
            let b = walk_snapshot(b, tab, rows);
            match (a, b) {
                (Some(a), Some(b)) => Some(PaneLayout::Split {
                    axis: to_split_axis(*axis),
                    ratio: *ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                // One side was entirely pruned: collapse to the survivor so
                // the split node disappears with its dead child.
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            }
        }
        Node::Pane(p) => {
            let pane = tab.pane_grid.get(*p)?;
            let source = match &pane.origin {
                PaneOrigin::Host(id) => PaneSource::Host(*id),
                PaneOrigin::Local(spec) => PaneSource::LocalShell {
                    program: spec.program.clone(),
                    args: spec.args.clone(),
                    label: spec.label.clone(),
                },
                PaneOrigin::Ephemeral => {
                    tracing::warn!(
                        target = "oryxis::session_group",
                        label = %pane.label,
                        "pruning non-referenceable pane from session group snapshot"
                    );
                    return None;
                }
            };
            rows.push(PaneScriptRow {
                label: pane.label.clone(),
                script: String::new(),
            });
            Some(PaneLayout::Leaf(PaneMember {
                source,
                initial_script: None,
            }))
        }
    }
}

/// Build editor rows from an already-saved layout (editing a group from the
/// sidebar, where there is no live tab). `resolve_label` turns a host id into
/// a display label; local shells use their captured label.
pub(crate) fn rows_from_layout(
    layout: &PaneLayout,
    resolve_label: &impl Fn(&PaneSource) -> String,
) -> Vec<PaneScriptRow> {
    let mut rows = Vec::new();
    collect_rows(layout, resolve_label, &mut rows);
    rows
}

fn collect_rows(
    layout: &PaneLayout,
    resolve_label: &impl Fn(&PaneSource) -> String,
    rows: &mut Vec<PaneScriptRow>,
) {
    match layout {
        PaneLayout::Split { a, b, .. } => {
            collect_rows(a, resolve_label, rows);
            collect_rows(b, resolve_label, rows);
        }
        PaneLayout::Leaf(member) => rows.push(PaneScriptRow {
            label: resolve_label(&member.source),
            script: member.initial_script.clone().unwrap_or_default(),
        }),
    }
}

/// Merge edited scripts back into the layout, by leaf order. An empty script
/// stores `None` (fall back to the host's own `initial_command`). Extra rows
/// or extra leaves are tolerated (zip stops at the shorter).
pub(crate) fn apply_scripts(layout: PaneLayout, rows: &[PaneScriptRow]) -> PaneLayout {
    let mut idx = 0;
    apply_scripts_inner(layout, rows, &mut idx)
}

fn apply_scripts_inner(layout: PaneLayout, rows: &[PaneScriptRow], idx: &mut usize) -> PaneLayout {
    match layout {
        PaneLayout::Split {
            axis,
            ratio,
            a,
            b,
        } => PaneLayout::Split {
            axis,
            ratio,
            a: Box::new(apply_scripts_inner(*a, rows, idx)),
            b: Box::new(apply_scripts_inner(*b, rows, idx)),
        },
        PaneLayout::Leaf(mut member) => {
            if let Some(row) = rows.get(*idx) {
                let trimmed = row.script.trim();
                member.initial_script =
                    (!trimmed.is_empty()).then(|| row.script.clone());
            }
            *idx += 1;
            PaneLayout::Leaf(member)
        }
    }
}

/// Prune leaves whose source fails `keep` (dangling host references on open),
/// collapsing split nodes whose child disappears. Returns `None` if nothing
/// survives.
pub(crate) fn prune_layout(
    layout: PaneLayout,
    keep: &impl Fn(&PaneSource) -> bool,
) -> Option<PaneLayout> {
    match layout {
        PaneLayout::Split {
            axis,
            ratio,
            a,
            b,
        } => {
            let a = prune_layout(*a, keep);
            let b = prune_layout(*b, keep);
            match (a, b) {
                (Some(a), Some(b)) => Some(PaneLayout::Split {
                    axis,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            }
        }
        PaneLayout::Leaf(member) => keep(&member.source).then_some(PaneLayout::Leaf(member)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn host_leaf(id: Uuid, script: Option<&str>) -> PaneLayout {
        PaneLayout::Leaf(PaneMember {
            source: PaneSource::Host(id),
            initial_script: script.map(|s| s.to_string()),
        })
    }

    #[test]
    fn apply_scripts_sets_and_clears_by_order() {
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let layout = PaneLayout::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            a: Box::new(host_leaf(id_a, None)),
            b: Box::new(host_leaf(id_b, None)),
        };
        let rows = vec![
            PaneScriptRow { label: "a".into(), script: "  ".into() }, // blank -> None
            PaneScriptRow { label: "b".into(), script: "tmux a".into() },
        ];
        let merged = apply_scripts(layout, &rows);
        match merged {
            PaneLayout::Split { a, b, .. } => {
                assert!(matches!(*a, PaneLayout::Leaf(m) if m.initial_script.is_none()));
                assert!(matches!(*b, PaneLayout::Leaf(m) if m.initial_script.as_deref() == Some("tmux a")));
            }
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn prune_collapses_dangling_split() {
        let alive = Uuid::new_v4();
        let dead = Uuid::new_v4();
        let layout = PaneLayout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.3,
            a: Box::new(host_leaf(dead, None)),
            b: Box::new(host_leaf(alive, None)),
        };
        let pruned = prune_layout(layout, &|s| matches!(s, PaneSource::Host(id) if *id == alive));
        // The dead leaf's parent split collapses to the surviving leaf.
        assert!(matches!(pruned, Some(PaneLayout::Leaf(m)) if matches!(m.source, PaneSource::Host(id) if id == alive)));
    }

    #[test]
    fn prune_all_dead_returns_none() {
        let dead = Uuid::new_v4();
        let layout = host_leaf(dead, None);
        assert!(prune_layout(layout, &|_| false).is_none());
    }

    /// The feature's core promise: a snapshot, rebuilt into a real
    /// `pane_grid` via `Configuration` and snapshotted again, must come back
    /// byte-for-byte (axes, ratios, leaf order, sources). Catches any drift
    /// in `to_split_axis` / `from_split_axis` or the tree walks.
    #[test]
    fn snapshot_restore_preserves_axes_ratios_and_order() {
        use std::sync::{Arc, Mutex};

        use iced::widget::pane_grid::{self, Configuration};
        use oryxis_terminal::widget::TerminalState;

        use crate::state::{Pane, PaneOrigin, TerminalTab};

        // Mirror `build_session_pane_config`'s tree mapping with dummy panes
        // (no PTY): leaf sources become `Host`/`Local` origins on the pane.
        fn to_config(layout: &PaneLayout) -> Configuration<Pane> {
            match layout {
                PaneLayout::Split { axis, ratio, a, b } => Configuration::Split {
                    axis: from_split_axis(*axis),
                    ratio: *ratio,
                    a: Box::new(to_config(a)),
                    b: Box::new(to_config(b)),
                },
                PaneLayout::Leaf(member) => {
                    let term = TerminalState::new_no_pty(80, 24).unwrap();
                    let mut pane = Pane::new("p".to_string(), Arc::new(Mutex::new(term)));
                    pane.origin = match &member.source {
                        PaneSource::Host(id) => PaneOrigin::Host(*id),
                        PaneSource::LocalShell {
                            program,
                            args,
                            label,
                        } => PaneOrigin::Local(crate::state::LocalShellSpec {
                            label: label.clone(),
                            program: program.clone(),
                            args: args.clone(),
                        }),
                    };
                    Configuration::Pane(pane)
                }
            }
        }

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        // Asymmetric, nested tree with distinct axes + non-trivial ratios:
        // Split(V, 0.3, host1, Split(H, 0.62, local, host2)).
        let original = PaneLayout::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.3,
            a: Box::new(host_leaf(id1, None)),
            b: Box::new(PaneLayout::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.62,
                a: Box::new(PaneLayout::Leaf(PaneMember {
                    source: PaneSource::LocalShell {
                        program: "bash".into(),
                        args: vec!["-l".into()],
                        label: "Local".into(),
                    },
                    initial_script: None,
                })),
                b: Box::new(host_leaf(id2, None)),
            }),
        };

        let grid = pane_grid::State::with_configuration(to_config(&original));
        let focused = *grid.panes.keys().next().unwrap();
        let tab = TerminalTab {
            _id: Uuid::new_v4(),
            label: "t".into(),
            pane_grid: grid,
            focused,
            chat_history: Vec::new(),
            chat_visible: false,
            chat_always_run_commands: Vec::new(),
            ssm_keepalive: false,
            relaunch: None,
            session_group_id: None,
        };

        let (restored, rows) = snapshot_tab_layout(&tab).expect("nothing pruned");
        assert_eq!(rows.len(), 3, "one editor row per surviving leaf");
        assert_eq!(restored, original, "layout must round-trip exactly");
    }
}
