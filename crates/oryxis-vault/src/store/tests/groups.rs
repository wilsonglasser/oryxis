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


#[test]
fn group_cloud_query_round_trip() {
    use oryxis_core::models::cloud::{
        CloudQuery, CloudQueryKind, ConnectionTemplate, TransportKind,
    };

    let vault = unlocked_vault();
    let profile_id = uuid::Uuid::new_v4();
    let mut g = Group::new("payments / api");
    g.cloud_query = Some(CloudQuery {
        profile_id,
        kind: CloudQueryKind::EcsTasks {
            cluster: "payments-cluster".into(),
            service: "api-svc".into(),
            container: "api".into(),
        },
        template: ConnectionTemplate {
            username: None,
            initial_command: Some("exec bash".into()),
            transport: TransportKind::EcsExec,
            key_id: None,
            identity_id: None,
            terminal_theme: None,
        },
    });
    vault.save_group(&g).unwrap();

    let listed = vault.list_groups().unwrap();
    let back = listed.iter().find(|gg| gg.id == g.id).unwrap();
    let q = back.cloud_query.as_ref().expect("cloud_query preserved");
    assert_eq!(q.profile_id, profile_id);
    assert_eq!(q.template.transport, TransportKind::EcsExec);
    assert_eq!(q.template.initial_command.as_deref(), Some("exec bash"));
    match &q.kind {
        CloudQueryKind::EcsTasks { cluster, service, container } => {
            assert_eq!(cluster, "payments-cluster");
            assert_eq!(service, "api-svc");
            assert_eq!(container, "api");
        }
        _ => panic!("wrong kind variant"),
    }
}

// ── Sync device identity persistence ─────────────────────────────
//
// The blob layout is opaque to the vault (oryxis-sync owns it).
// What we pin here is the encrypt-at-rest contract: bytes round
// trip exactly, the underlying setting is not stored as plaintext,
// and the value survives a master-password rotation.

