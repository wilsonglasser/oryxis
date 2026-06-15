use super::*;

#[test]
fn save_and_list_snippets() {
    let vault = unlocked_vault();
    let s = Snippet::new("restart-nginx", "sudo systemctl restart nginx");
    vault.save_snippet(&s).unwrap();

    let snippets = vault.list_snippets().unwrap();
    assert_eq!(snippets.len(), 1);
    assert_eq!(snippets[0].command, "sudo systemctl restart nginx");
}


#[test]
fn delete_snippet() {
    let vault = unlocked_vault();
    let s = Snippet::new("temp", "echo hi");
    vault.save_snippet(&s).unwrap();
    vault.delete_snippet(&s.id).unwrap();
    assert_eq!(vault.list_snippets().unwrap().len(), 0);
}

// ── Known Hosts ──


#[test]
fn snippet_has_updated_at() {
    let vault = unlocked_vault();
    let s = Snippet::new("test", "echo hi");
    assert!(s.updated_at.timestamp() > 0);
    vault.save_snippet(&s).unwrap();

    let snippets = vault.list_snippets().unwrap();
    assert_eq!(snippets.len(), 1);
    assert!(snippets[0].updated_at.timestamp() > 0);
}

