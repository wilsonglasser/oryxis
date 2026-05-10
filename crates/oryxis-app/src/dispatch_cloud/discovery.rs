//! Discovery panel + import flow handlers, open / hide / refresh,
//! the result fan-out, the per-row toggles, the import confirmation
//! modal trigger, and the actual import that materializes EC2 hosts +
//! ECS dynamic groups in the vault.

use iced::Task;
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
                self.cloud_discover_filter.clear();
                self.cloud_discover_state = CloudDiscoverState::Idle;
                return self.spawn_discover(profile_id);
            }
            Message::HideCloudDiscover => {
                self.cloud_discover_visible = false;
                self.cloud_discover_profile_id = None;
                self.cloud_discover_state = CloudDiscoverState::Idle;
                self.cloud_discover_selected_ec2.clear();
                self.cloud_discover_selected_ecs.clear();
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
            Message::CloudDiscoverImport => {
                // Decide whether the user needs to be asked which
                // transport to use:
                //   - Any EC2 selected → modal asks for transport
                //   - ECS-only        → no modal, import straight (
                //     dynamic groups always use ECS Exec)
                if self.cloud_discover_selected_ec2.is_empty()
                    && !self.cloud_discover_selected_ecs.is_empty()
                {
                    return Ok(self
                        .handle_cloud(Message::CloudDiscoverImportConfirmed)
                        .unwrap_or_else(|_| Task::none()));
                }
                if self.cloud_discover_selected_ec2.is_empty()
                    && self.cloud_discover_selected_ecs.is_empty()
                {
                    return Ok(Task::none());
                }
                self.cloud_import_confirm_visible = true;
            }
            Message::CloudDiscoverImportCancelled => {
                self.cloud_import_confirm_visible = false;
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
                if selected_ec2.is_empty() && selected_ecs.is_empty() {
                    return Ok(Task::none());
                }

                if let Some(vault) = &self.vault {
                    // Provider-folder layout (per user feedback round
                    // 3): every imported entity nests under a single
                    // top-level folder named after the cloud profile.
                    // EC2 hosts get the folder as their `group_id`;
                    // ECS dynamic groups get it as `parent_id`. Folder
                    // is auto-created the first time and reused on
                    // subsequent imports of the same profile.
                    let profile_label = self
                        .cloud_profiles
                        .iter()
                        .find(|p| p.id == profile_id)
                        .map(|p| p.label.clone())
                        .unwrap_or_default();
                    let provider_id_str = self
                        .cloud_profiles
                        .iter()
                        .find(|p| p.id == profile_id)
                        .map(|p| p.provider.clone())
                        .unwrap_or_default();
                    let provider_group_id = self
                        .groups
                        .iter()
                        .find(|g| g.label == profile_label && g.cloud_query.is_none())
                        .map(|g| g.id)
                        .or_else(|| {
                            let mut g = Group::new(profile_label.clone());
                            // Provider folder = brand glyph for the
                            // provider id (resolved by the brand-icon
                            // SVG registry via the canonical alias).
                            g.icon = Some(provider_id_str.clone());
                            let id = g.id;
                            vault.save_group(&g).ok().map(|_| id)
                        });

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

                    self.cloud_discover_visible = false;
                    self.cloud_discover_profile_id = None;
                    self.cloud_discover_selected_ec2.clear();
                    self.cloud_discover_selected_ecs.clear();
                    self.cloud_discover_state = CloudDiscoverState::Idle;
                    self.load_data_from_vault();
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
