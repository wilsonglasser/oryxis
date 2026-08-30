use super::*;

#[test]
fn session_group_layout_roundtrips_tree_and_scripts() {
    let vault = temp_vault();
    let host_a = Uuid::new_v4();
    // Split { Vertical, 0.4, Leaf(host A, script), Leaf(local, script) }
    let layout = PaneLayout::Split {
        axis: SplitAxis::Vertical,
        ratio: 0.4,
        a: Box::new(PaneLayout::Leaf(PaneMember {
            source: PaneSource::Host(host_a),
            initial_script: Some("htop".to_string()),
        })),
        b: Box::new(PaneLayout::Leaf(PaneMember {
            source: PaneSource::LocalShell {
                program: "bash".to_string(),
                args: vec!["-l".to_string()],
                label: "Local".to_string(),
            },
            initial_script: Some("cd /tmp".to_string()),
        })),
    };
    let mut sg = SessionGroup::new("Dashboard", layout);
    sg.color = Some("#ff8800".to_string());
    sg.icon_style = Some("boxes".to_string());
    vault.save_session_group(&sg).unwrap();

    let loaded = vault.list_session_groups().unwrap();
    assert_eq!(loaded.len(), 1);
    let g = &loaded[0];
    assert_eq!(g.id, sg.id);
    assert_eq!(g.label, "Dashboard");
    assert_eq!(g.color.as_deref(), Some("#ff8800"));
    assert_eq!(g.icon_style.as_deref(), Some("boxes"));
    match &g.layout {
        PaneLayout::Split { axis, ratio, a, b } => {
            assert_eq!(*axis, SplitAxis::Vertical);
            assert!((*ratio - 0.4).abs() < f32::EPSILON);
            match a.as_ref() {
                PaneLayout::Leaf(m) => {
                    assert!(matches!(m.source, PaneSource::Host(id) if id == host_a));
                    assert_eq!(m.initial_script.as_deref(), Some("htop"));
                }
                _ => panic!("expected leaf A"),
            }
            match b.as_ref() {
                PaneLayout::Leaf(m) => {
                    assert!(matches!(&m.source, PaneSource::LocalShell { program, .. } if program == "bash"));
                    assert_eq!(m.initial_script.as_deref(), Some("cd /tmp"));
                }
                _ => panic!("expected leaf B"),
            }
        }
        _ => panic!("expected split root"),
    }

    vault.delete_session_group(&sg.id).unwrap();
    assert!(vault.list_session_groups().unwrap().is_empty());
}

// ── Session logs (append-only chunk recording) ──


#[test]
fn save_and_list_groups() {
    let vault = unlocked_vault();
    let g = Group::new("Production");
    vault.save_group(&g).unwrap();

    let groups = vault.list_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Production");
}


#[test]
fn delete_group() {
    let vault = unlocked_vault();
    let g = Group::new("Temp");
    vault.save_group(&g).unwrap();
    vault.delete_group(&g.id).unwrap();
    assert_eq!(vault.list_groups().unwrap().len(), 0);
}

// ── Snippets CRUD ──


#[test]
fn group_has_timestamps() {
    let vault = unlocked_vault();
    let g = Group::new("test-group");
    assert!(g.created_at <= g.updated_at);
    vault.save_group(&g).unwrap();

    let groups = vault.list_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert!(groups[0].created_at.timestamp() > 0);
    assert!(groups[0].updated_at.timestamp() > 0);
}


// ── Sync device identity persistence ─────────────────────────────
//
// The blob layout is opaque to the vault (its consumer owned it).
// What we pin here is the encrypt-at-rest contract: bytes round
// trip exactly, the underlying setting is not stored as plaintext,
// and the value survives a master-password rotation.


/// The whole point of the D4 storage layer: a group's defaults survive
/// a save/load cycle.
///
/// This test exists because the column is named in TWO places, the
/// INSERT in `save_group` and the SELECT in `list_groups`, and missing
/// either fails silently: forget the SELECT and every read comes back
/// `None`, forget the INSERT and every save wipes what was there.
#[test]
fn group_defaults_roundtrip() {
    let vault = temp_vault();
    let identity = Uuid::new_v4();
    let proxy_identity = Uuid::new_v4();
    let snippet = Uuid::new_v4();
    let mut group = Group::new("prod");
    group.defaults = Some(GroupDefaults {
        username: Some("deploy".to_string()),
        identity_id: Some(identity),
        proxy_identity_id: Some(proxy_identity),
        port: Some(2222),
        env_vars: vec![EnvVar { key: "TERM".into(), value: "xterm-256color".into() }],
        terminal_theme: Some("nord".to_string()),
        startup_snippet_id: Some(snippet),
    });
    vault.save_group(&group).unwrap();

    let loaded = vault.list_groups().unwrap();
    assert_eq!(loaded.len(), 1);
    let d = loaded[0].defaults.as_ref().expect("defaults survived the round trip");
    assert_eq!(d.username.as_deref(), Some("deploy"));
    assert_eq!(d.identity_id, Some(identity));
    assert_eq!(d.proxy_identity_id, Some(proxy_identity));
    assert_eq!(d.port, Some(2222));
    assert_eq!(d.env_vars.len(), 1);
    assert_eq!(d.env_vars[0].key, "TERM");
    assert_eq!(d.terminal_theme.as_deref(), Some("nord"));
    assert_eq!(d.startup_snippet_id, Some(snippet));
}

/// A group that sets nothing stores NULL, not `{}`, so a vault that
/// never used the feature keeps the rows it always had.
#[test]
fn a_group_without_defaults_stores_null() {
    let vault = temp_vault();
    let mut group = Group::new("plain");
    vault.save_group(&group).unwrap();
    assert!(vault.list_groups().unwrap()[0].defaults.is_none());

    // An all-unset struct is the same answer as never having one.
    group.defaults = Some(GroupDefaults::default());
    vault.save_group(&group).unwrap();
    assert!(vault.list_groups().unwrap()[0].defaults.is_none());
}
