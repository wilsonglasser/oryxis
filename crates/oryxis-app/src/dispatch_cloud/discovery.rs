//! Discovery panel + import flow handlers, open / hide / refresh,
//! the result fan-out, the per-row toggles, the import confirmation
//! modal trigger, and the actual import that materializes EC2 hosts +
//! ECS dynamic groups in the vault.

use std::sync::Arc;

use iced::Task;
use oryxis_cloud::CloudProviderRegistry;
use oryxis_core::models::cloud::{
    CloudQuery, CloudQueryKind, CloudRef, CloudResourceType, ConnectionTemplate, TransportKind,
};
use oryxis_core::models::connection::Connection;
use oryxis_core::models::group::Group;

use crate::app::{Message, Oryxis};
use crate::state::CloudDiscoverState;

impl Oryxis {
    pub(super) fn handle_cloud_discovery(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::ShowCloudDiscover(profile_id) => {
                // Dismiss the "+ Host [▾]" provider picker that
                // dispatched this message, without it the dropdown
                // hangs on top of the freshly-opened discovery panel.
                self.overlay = None;
                // Close any other right-panel (mutually exclusive slot).
                self.show_host_panel = false;
                self.cloud_form_visible = false;
                self.cloud_dynamic_form_visible = false;
                self.cloud_discover_visible = true;
                self.cloud_discover_profile_id = Some(profile_id);
                self.cloud_discover_selected_ec2.clear();
                self.cloud_discover_selected_ecs.clear();
                self.cloud_discover_selected_k8s.clear();
                self.cloud_discover_filter.clear();
                self.cloud_discover_state = CloudDiscoverState::Idle;
                // Default the input to the profile's own label so the
                // most common case (one folder per profile) requires
                // zero typing. The user can clear or change before
                // hitting Import.
                self.cloud_discover_default_group_name = self
                    .cloud_profiles
                    .iter()
                    .find(|p| p.id == profile_id)
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                return self.spawn_discover(profile_id);
            }
            Message::HideCloudDiscover => {
                self.cloud_discover_visible = false;
                self.cloud_discover_profile_id = None;
                self.cloud_discover_state = CloudDiscoverState::Idle;
                self.cloud_discover_selected_ec2.clear();
                self.cloud_discover_selected_ecs.clear();
                self.cloud_discover_selected_k8s.clear();
                self.cloud_discover_filter.clear();
            }
            Message::CloudDiscoverRefresh => {
                if let Some(id) = self.cloud_discover_profile_id {
                    return self.spawn_discover(id);
                }
            }
            Message::CloudDiscoverResult(result) => {
                self.cloud_discover_state = match result {
                    Ok(boxed) => CloudDiscoverState::Loaded(*boxed),
                    Err(msg) => CloudDiscoverState::Failed(msg),
                };
                // Reset selection on every refresh, the upstream might
                // have changed (instance terminated, new ones spun up),
                // and silently keeping a checked id that no longer
                // exists in the new list would be misleading.
                self.cloud_discover_selected_ec2.clear();
                self.cloud_discover_selected_ecs.clear();
                self.cloud_discover_selected_k8s.clear();
                // Stamp the profile's last_discovered when we got real
                // results, so the cards list shows fresh metadata.
                if matches!(self.cloud_discover_state, CloudDiscoverState::Loaded(_))
                    && let Some(id) = self.cloud_discover_profile_id
                    && let Some(vault) = &self.vault
                    && let Some(mut cp) = self
                        .cloud_profiles
                        .iter()
                        .find(|p| p.id == id)
                        .cloned()
                {
                    cp.last_discovered = Some(chrono::Utc::now());
                    let _ = vault.save_cloud_profile(&cp, None);
                    self.load_data_from_vault();
                }
            }
            Message::CloudDiscoverToggleEc2(instance_id) => {
                if !self.cloud_discover_selected_ec2.remove(&instance_id) {
                    self.cloud_discover_selected_ec2.insert(instance_id);
                }
            }
            Message::CloudDiscoverToggleEcs(key) => {
                if !self.cloud_discover_selected_ecs.remove(&key) {
                    self.cloud_discover_selected_ecs.insert(key);
                }
            }
            Message::CloudDiscoverToggleK8s(key) => {
                if !self.cloud_discover_selected_k8s.remove(&key) {
                    self.cloud_discover_selected_k8s.insert(key);
                }
            }
            Message::CloudDiscoverFilterChanged(s) => {
                self.cloud_discover_filter = s;
            }
            Message::CloudDiscoverToggleSection(key) => {
                if !self.cloud_discover_collapsed.remove(&key) {
                    self.cloud_discover_collapsed.insert(key);
                }
            }
            Message::CloudDiscoverDefaultTransportChanged(t) => {
                self.cloud_discover_default_transport = t;
            }
            Message::CloudDiscoverDefaultGroupNameChanged(v) => {
                self.cloud_discover_default_group_name = v;
            }
            Message::CloudDiscoverDefaultGroupPick(label) => {
                self.cloud_discover_default_group_name = label;
                self.cloud_discover_default_group_picker_open = false;
                // The modal-stack injection at `view_main` only
                // checks `self.overlay`; without also clearing it
                // here the menu would re-render on top after every
                // pick. Mirrors the close-branch of
                // `ToggleCloudDiscoverGroupPicker`.
                if matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(crate::state::OverlayContent::CloudDiscoverGroupPicker)
                ) {
                    self.overlay = None;
                }
            }
            Message::ToggleCloudDiscoverGroupPicker => {
                self.cloud_discover_default_group_picker_open =
                    !self.cloud_discover_default_group_picker_open;
                if self.cloud_discover_default_group_picker_open {
                    self.cloud_discover_default_group_picker_search.clear();
                    // Anchor the menu off the live combo bounds
                    // captured by the `bounds_reporter` wrapping the
                    // Import-into row. `BoundsCell` value is the
                    // last-rendered screen-space rect of the combo
                    // (input + chevron). Menu's top sits 6 px below
                    // the combo's bottom; left edge matches combo
                    // left so the dropdown visually replaces the
                    // input column.
                    let combo = self.cloud_discover_default_group_combo_bounds.get();
                    let gap = 6.0_f32;
                    let x = combo.x.max(0.0);
                    let y = (combo.y + combo.height + gap).max(0.0);
                    self.overlay = Some(crate::state::OverlayState {
                        content: crate::state::OverlayContent::CloudDiscoverGroupPicker,
                        x,
                        y,
                    });
                } else if matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(crate::state::OverlayContent::CloudDiscoverGroupPicker)
                ) {
                    self.overlay = None;
                }
            }
            Message::CloudDiscoverDefaultGroupPickerSearchChanged(v) => {
                self.cloud_discover_default_group_picker_search = v;
            }
            Message::CloudAutoRefreshTick => {
                // Fan out a sync for every configured profile. Each
                // sync is independent (own Task::perform), so a slow /
                // failing profile doesn't hold up the others. Empty
                // profile list short-circuits.
                let profile_ids: Vec<uuid::Uuid> =
                    self.cloud_profiles.iter().map(|p| p.id).collect();
                let mut tasks: Vec<Task<Message>> = Vec::new();
                for pid in profile_ids {
                    if let Ok(task) = self.handle_cloud(Message::CloudProfileSync(pid)) {
                        tasks.push(task);
                    }
                }
                return Ok(Task::batch(tasks));
            }
            Message::CloudProfileSync(profile_id) => {
                // Background refresh, runs the provider's `discover`
                // and routes the result to `CloudProfileSyncResult`
                // where the sticky-fields merge happens. Independent
                // of the Discover panel; the profile card's Sync
                // button can fire this without opening any UI.
                let Some(mut profile) = self
                    .cloud_profiles
                    .iter()
                    .find(|p| p.id == profile_id)
                    .cloned()
                else {
                    return Ok(Task::none());
                };
                let registry: Arc<CloudProviderRegistry> =
                    self.cloud_provider_registry.clone();
                let Some(provider) = registry.get(&profile.provider) else {
                    return Ok(Task::none());
                };
                if let Some(vault) = &self.vault {
                    profile.secret =
                        vault.get_cloud_profile_secret(&profile_id).ok().flatten();
                }
                return Ok(Task::perform(
                    async move { provider.discover(&profile).await },
                    move |result| {
                        Message::CloudProfileSyncResult(
                            profile_id,
                            result.map(Box::new).map_err(|e| e.to_string()),
                        )
                    },
                ));
            }
            Message::CloudProfileSyncResult(profile_id, result) => {
                if self.vault.is_none() {
                    return Ok(Task::none());
                }
                match result {
                    Ok(discovery) => {
                        let now = chrono::Utc::now();
                        // Index AWS-side EC2 results by instance id so
                        // the merge below is O(N+M) instead of O(N*M).
                        let by_id: std::collections::HashMap<
                            String,
                            &oryxis_cloud::DiscoveredEc2,
                        > = discovery
                            .ec2
                            .iter()
                            .map(|e| (e.instance_id.clone(), e))
                            .collect();
                        // Compute merge first so the vault save loop
                        // doesn't have to fight a mutable borrow of
                        // `self.connections` during the diff.
                        let mut updated: Vec<Connection> = Vec::new();
                        for conn in &self.connections {
                            let Some(cref) = conn.cloud_ref.as_ref() else {
                                continue;
                            };
                            if cref.profile_id != profile_id {
                                continue;
                            }
                            if cref.resource_type != CloudResourceType::Ec2 {
                                continue;
                            }
                            let mut next = conn.clone();
                            let mut changed = false;
                            if let Some(found) = by_id.get(&cref.resource_id) {
                                if cref.orphaned_at.is_some()
                                    && let Some(cr) = next.cloud_ref.as_mut()
                                {
                                    cr.orphaned_at = None;
                                    changed = true;
                                }
                                // Field-by-field merge: AWS wins unless
                                // the user flagged the field as
                                // customized post-import.
                                if !next
                                    .customized_fields
                                    .iter()
                                    .any(|s| s == "label")
                                {
                                    let new_label = found
                                        .name
                                        .clone()
                                        .unwrap_or_else(|| found.instance_id.clone());
                                    if next.label != new_label {
                                        next.label = new_label;
                                        changed = true;
                                    }
                                }
                                if !next
                                    .customized_fields
                                    .iter()
                                    .any(|s| s == "hostname")
                                {
                                    let new_hostname = found
                                        .public_dns
                                        .clone()
                                        .or_else(|| found.public_ip.clone())
                                        .or_else(|| found.private_dns.clone())
                                        .or_else(|| found.private_ip.clone())
                                        .unwrap_or_default();
                                    if !new_hostname.is_empty()
                                        && next.hostname != new_hostname
                                    {
                                        next.hostname = new_hostname;
                                        changed = true;
                                    }
                                }
                                if !next
                                    .customized_fields
                                    .iter()
                                    .any(|s| s == "username")
                                {
                                    let new_username = found
                                        .default_username
                                        .clone()
                                        .or_else(|| Some("ec2-user".to_string()));
                                    if next.username != new_username {
                                        next.username = new_username;
                                        changed = true;
                                    }
                                }
                            } else {
                                // Resource absent upstream, mark orphan
                                // on first miss (preserve the
                                // timestamp on subsequent syncs so the
                                // "orphaned for N days" math stays
                                // stable).
                                if cref.orphaned_at.is_none()
                                    && let Some(cr) = next.cloud_ref.as_mut()
                                {
                                    cr.orphaned_at = Some(now);
                                    changed = true;
                                }
                            }
                            if changed {
                                next.updated_at = now;
                                updated.push(next);
                            }
                        }
                        let cp_to_save = self
                            .cloud_profiles
                            .iter()
                            .find(|p| p.id == profile_id)
                            .cloned()
                            .map(|mut cp| {
                                cp.last_discovered = Some(now);
                                cp
                            });
                        if let Some(vault) = &self.vault {
                            // One transaction for the whole refresh batch
                            // (a save per row used to mean a commit per
                            // row), and patch the in-memory lists instead
                            // of re-reading the entire vault.
                            let _ = vault.begin_batch();
                            for conn in &updated {
                                let _ = vault.save_connection(conn, None);
                            }
                            if let Some(cp) = &cp_to_save {
                                let _ = vault.save_cloud_profile(cp, None);
                            }
                            if vault.commit_batch().is_err() {
                                vault.rollback_batch();
                            }
                        }
                        for conn in updated {
                            if let Some(slot) =
                                self.connections.iter_mut().find(|c| c.id == conn.id)
                            {
                                *slot = conn;
                            } else {
                                self.connections.push(conn);
                            }
                        }
                        if let Some(cp) = cp_to_save
                            && let Some(slot) =
                                self.cloud_profiles.iter_mut().find(|p| p.id == cp.id)
                        {
                            *slot = cp;
                        }
                    }
                    Err(msg) => {
                        tracing::error!(
                            target = "oryxis::dispatch_cloud",
                            "cloud profile sync failed: {msg}"
                        );
                    }
                }
            }
            Message::CloudDiscoverImport => {
                // Always route through the confirmation modal so the
                // user gets a chance to set the target group (and the
                // transport, when EC2 hosts are part of the batch).
                // Empty selection short-circuits.
                if self.cloud_discover_selected_ec2.is_empty()
                    && self.cloud_discover_selected_ecs.is_empty()
                {
                    return Ok(Task::none());
                }
                self.cloud_import_confirm_visible = true;
            }
            Message::CloudDiscoverImportCancelled => {
                self.cloud_import_confirm_visible = false;
                self.cloud_discover_default_group_picker_open = false;
            }
            Message::CloudDiscoverImportConfirmed => {
                self.cloud_import_confirm_visible = false;
                let Some(profile_id) = self.cloud_discover_profile_id else {
                    return Ok(Task::none());
                };
                if !self.cloud_profiles.iter().any(|p| p.id == profile_id) {
                    return Ok(Task::none());
                }
                let CloudDiscoverState::Loaded(result) = &self.cloud_discover_state else {
                    return Ok(Task::none());
                };
                let selected_ec2: Vec<_> = result
                    .ec2
                    .iter()
                    .filter(|e| self.cloud_discover_selected_ec2.contains(&e.instance_id))
                    .cloned()
                    .collect();
                let selected_ecs: Vec<_> = result
                    .ecs_services
                    .iter()
                    .filter(|s| {
                        self.cloud_discover_selected_ecs
                            .contains(&format!("{}/{}/{}", s.cluster, s.service, s.container))
                    })
                    .cloned()
                    .collect();
                let selected_k8s: Vec<_> = result
                    .k8s_workloads
                    .iter()
                    .filter(|w| {
                        self.cloud_discover_selected_k8s
                            .contains(&format!("{}/{}/{}", w.namespace, w.kind, w.name))
                    })
                    .cloned()
                    .collect();
                if selected_ec2.is_empty() && selected_ecs.is_empty() && selected_k8s.is_empty() {
                    return Ok(Task::none());
                }

                if let Some(vault) = &self.vault {
                    // Resolve the target group from the typed name.
                    // Empty = root (no parent). Matching label = reuse
                    // existing group. Non-matching = create a new
                    // group with that label on the spot, so the user
                    // can type any folder name (existing or new) and
                    // have it materialised in one go.
                    let typed = self.cloud_discover_default_group_name.trim().to_string();
                    let provider_id_str = self
                        .cloud_profiles
                        .iter()
                        .find(|p| p.id == profile_id)
                        .map(|p| p.provider.clone())
                        .unwrap_or_default();
                    let profile_label = self
                        .cloud_profiles
                        .iter()
                        .find(|p| p.id == profile_id)
                        .map(|p| p.label.clone())
                        .unwrap_or_default();
                    let provider_group_id: Option<uuid::Uuid> = if typed.is_empty() {
                        None
                    } else {
                        self.groups
                            .iter()
                            .find(|g| g.label == typed && g.cloud_query.is_none())
                            .map(|g| g.id)
                            .or_else(|| {
                                let mut g = Group::new(typed.clone());
                                // Brand glyph only when the user kept
                                // the profile-label default. A custom
                                // folder name gets a generic icon so
                                // it doesn't look like an auto-folder
                                // by accident.
                                if typed == profile_label {
                                    g.icon = Some(provider_id_str.clone());
                                }
                                let id = g.id;
                                vault.save_group(&g).ok().map(|_| id)
                            })
                    };

                    for e in &selected_ec2 {
                        // Connection labels prefer the EC2 Name tag
                        // when set, otherwise fall back to the
                        // instance id (always unique inside a region).
                        let label = e.name.clone().unwrap_or_else(|| e.instance_id.clone());
                        let hostname = e
                            .public_dns
                            .clone()
                            .or_else(|| e.public_ip.clone())
                            .or_else(|| e.private_dns.clone())
                            .or_else(|| e.private_ip.clone())
                            .unwrap_or_default();

                        let mut conn = Connection::new(label, hostname);
                        // Fall back to `ec2-user` when the discovery
                        // result didn't infer a username, it's the
                        // default on Amazon Linux 2 / 2023 (the most
                        // common AMI family) and Instance Connect
                        // assumes it. Bitnami / Ubuntu users will
                        // need to edit, but that's a smaller hassle
                        // than landing with an empty field.
                        conn.username = e
                            .default_username
                            .clone()
                            .or_else(|| Some("ec2-user".to_string()));
                        conn.group_id = provider_group_id;
                        conn.cloud_ref = Some(CloudRef {
                            profile_id,
                            resource_type: CloudResourceType::Ec2,
                            resource_id: e.instance_id.clone(),
                            region: Some(e.region.clone()),
                            // Honour the per-discovery default the
                            // user picked at the bottom of the panel.
                            // Saves "import → edit each → set
                            // Instance Connect" on bulk imports.
                            transport_pref: self.cloud_discover_default_transport,
                            // Public IPs change across stop/start, so
                            // re-resolving on each connect is the safer
                            // default for imported EC2.
                            auto_refresh_hostname: true,
                            orphaned_at: None,
                        });
                        let _ = vault.save_connection(&conn, None);
                    }

                    // Each picked ECS service becomes a *dynamic
                    // group*: a `Group` row with `cloud_query` set,
                    // parented under the profile group. When the user
                    // expands it later, the resolver lists current
                    // tasks. The actual `EcsExec` transport doesn't
                    // ship until PR 5, clicking a task today gives a
                    // friendly "not implemented" error, but the
                    // structure is already persisted and syncable.
                    for s in &selected_ecs {
                        let label = format!("{} / {}", s.service, s.container);
                        let mut g = Group::new(label);
                        g.parent_id = provider_group_id;
                        // ECS-specific brand glyph (the orange hex box)
                        //, distinguishes it from the AWS-provider
                        // folder one level up at a glance.
                        g.icon = Some("ecs".into());
                        g.cloud_query = Some(CloudQuery {
                            profile_id,
                            kind: CloudQueryKind::EcsTasks {
                                cluster: s.cluster.clone(),
                                service: s.service.clone(),
                                container: s.container.clone(),
                            },
                            template: ConnectionTemplate {
                                username: None,
                                initial_command: None,
                                transport: TransportKind::EcsExec,
                                key_id: None,
                                identity_id: None,
                                terminal_theme: None,
                            },
                        });
                        let _ = vault.save_group(&g);
                    }

                    // Each picked K8s workload becomes a dynamic group
                    // backed by a `K8sPods` label query. Expanding it
                    // resolves the workload's current pods; clicking a pod
                    // opens `kubectl exec`.
                    for w in &selected_k8s {
                        let label = format!("{} ({})", w.name, w.namespace);
                        let mut g = Group::new(label);
                        g.parent_id = provider_group_id;
                        g.icon = Some("kubernetes".into());
                        let selector = oryxis_core::models::cloud::PodSelector::Labels(
                            w.match_labels.clone(),
                        );
                        g.cloud_query = Some(CloudQuery {
                            profile_id,
                            kind: CloudQueryKind::K8sPods {
                                context: w.context.clone(),
                                namespace: w.namespace.clone(),
                                selector,
                            },
                            template: ConnectionTemplate {
                                username: None,
                                initial_command: None,
                                transport: TransportKind::KubectlExec,
                                key_id: None,
                                identity_id: None,
                                terminal_theme: None,
                            },
                        });
                        let _ = vault.save_group(&g);
                    }

                    self.cloud_discover_visible = false;
                    self.cloud_discover_profile_id = None;
                    self.cloud_discover_selected_ec2.clear();
                    self.cloud_discover_selected_ecs.clear();
                    self.cloud_discover_selected_k8s.clear();
                    self.cloud_discover_state = CloudDiscoverState::Idle;
                    self.load_data_from_vault();
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
