//! `Oryxis::handle_cloud` — Cloud Accounts panel + wizard messages.
//!
//! v0.6 PR 3 wires only AWS profile auth. Access key + SSO + the
//! Discover & Pick wizard step land in follow-up PRs once this
//! foundation is exercised in the UI.

#![allow(clippy::result_large_err)]

use std::sync::Arc;

use iced::Task;
use oryxis_cloud::CloudProviderRegistry;
use oryxis_core::models::cloud::{
    CloudQuery, CloudQueryKind, CloudRef, CloudResourceType, ConnectionTemplate, TransportKind,
};
use oryxis_core::models::cloud_profile::CloudProfile;
use oryxis_core::models::connection::Connection;
use oryxis_core::models::group::Group;
use uuid::Uuid;

use crate::app::{Message, Oryxis};
use crate::state::{
    CloudAuthChoice, CloudDiscoverState, CloudProviderChoice, CloudTestState, OverlayContent,
    OverlayState,
};

impl Oryxis {
    pub(crate) fn handle_cloud(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::ShowCloudForm(maybe_id) => {
                // Close any open context menu (this message is fired
                // both from the "+ Account" toolbar button and from
                // the per-card "Edit" item). Without this the kebab
                // menu stays floating on top of the editor.
                self.overlay = None;
                self.cloud_form_visible = true;
                self.cloud_form_error = None;
                self.cloud_form_test_state = CloudTestState::Idle;

                if let Some(id) = maybe_id
                    && let Some(cp) = self.cloud_profiles.iter().find(|p| p.id == id)
                {
                    self.editing_cloud_profile_id = Some(id);
                    self.cloud_form_label = cp.label.clone();
                    self.cloud_form_provider = CloudProviderChoice::from_id(&cp.provider);
                    self.cloud_form_auth_kind = CloudAuthChoice::from_id(&cp.auth_kind);
                    let cfg: serde_json::Value =
                        serde_json::from_str(&cp.config).unwrap_or(serde_json::Value::Null);
                    self.cloud_form_aws_profile_name = cfg
                        .get("profile_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.cloud_form_aws_region = cfg
                        .get("region")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                } else {
                    self.editing_cloud_profile_id = None;
                    self.cloud_form_label = String::new();
                    self.cloud_form_provider = CloudProviderChoice::Aws;
                    self.cloud_form_auth_kind = CloudAuthChoice::Profile;
                    self.cloud_form_aws_profile_name = String::new();
                    self.cloud_form_aws_region = String::new();
                }
            }
            Message::HideCloudForm => {
                self.cloud_form_visible = false;
                self.cloud_form_error = None;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormLabelChanged(v) => {
                self.cloud_form_label = v;
            }
            Message::CloudFormProviderChanged(p) => {
                self.cloud_form_provider = p;
                // Reset auth choice when provider switches — Profile is
                // AWS-only, Kubeconfig is K8s-only. Keep them coherent
                // so the user doesn't see a stale auth kind on switch.
                self.cloud_form_auth_kind = match p {
                    CloudProviderChoice::Aws => CloudAuthChoice::Profile,
                    CloudProviderChoice::K8s => CloudAuthChoice::Kubeconfig,
                };
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAuthKindChanged(a) => {
                self.cloud_form_auth_kind = a;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsProfileNameChanged(v) => {
                self.cloud_form_aws_profile_name = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsRegionChanged(v) => {
                self.cloud_form_aws_region = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormTestCredentials => {
                let provider_id = self.cloud_form_provider.id();
                let registry: Arc<CloudProviderRegistry> = self.cloud_provider_registry.clone();
                let Some(provider) = registry.get(provider_id) else {
                    self.cloud_form_test_state = CloudTestState::Failed(
                        format!("provider \"{provider_id}\" not registered"),
                    );
                    return Ok(Task::none());
                };
                let Some(profile) = self.build_cloud_profile_for_test() else {
                    self.cloud_form_test_state =
                        CloudTestState::Failed(crate::i18n::t("cloud_err_label_required").into());
                    return Ok(Task::none());
                };
                self.cloud_form_test_state = CloudTestState::Running;
                return Ok(Task::perform(
                    async move { provider.test_credentials(&profile).await },
                    |result| {
                        Message::CloudFormTestResult(result.map_err(|e| e.to_string()))
                    },
                ));
            }
            Message::CloudFormTestResult(result) => {
                self.cloud_form_test_state = match result {
                    Ok(()) => CloudTestState::Ok,
                    Err(msg) => CloudTestState::Failed(msg),
                };
            }
            Message::SaveCloudProfile => {
                let label = self.cloud_form_label.trim().to_string();
                if label.is_empty() {
                    self.cloud_form_error =
                        Some(crate::i18n::t("cloud_err_label_required").into());
                    return Ok(Task::none());
                }

                let now = chrono::Utc::now();
                let mut profile = if let Some(id) = self.editing_cloud_profile_id {
                    self.cloud_profiles
                        .iter()
                        .find(|p| p.id == id)
                        .cloned()
                        .unwrap_or_else(|| {
                            CloudProfile::new(label.clone(), self.cloud_form_provider.id())
                        })
                } else {
                    CloudProfile::new(label.clone(), self.cloud_form_provider.id())
                };
                profile.label = label;
                profile.provider = self.cloud_form_provider.id().to_string();
                profile.auth_kind = self.cloud_form_auth_kind.id().to_string();
                profile.config = self.serialize_cloud_form_config();
                profile.updated_at = now;

                if let Some(vault) = &self.vault {
                    match vault.save_cloud_profile(&profile, None) {
                        Ok(()) => {
                            self.cloud_form_visible = false;
                            self.cloud_form_error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => {
                            self.cloud_form_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::DeleteCloudProfile(id) => {
                self.overlay = None;
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_cloud_profile(&id);
                    self.load_data_from_vault();
                }
            }
            Message::ShowCloudCardMenu(id) => {
                self.overlay = Some(OverlayState {
                    content: OverlayContent::CloudProfileActions(id),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::CloudCardHovered(id) => {
                self.hovered_cloud_card = Some(id);
            }
            Message::CloudCardUnhovered => {
                self.hovered_cloud_card = None;
            }
            Message::ShowCloudProviderPicker => {
                // Anchor below the "+ Host [▾]" split button. Same
                // computation as the keychain "+ ADD ▼" handler so both
                // split menus drop in the same screen position relative
                // to their toolbar — independent of cursor location.
                let panel_width = if self.cloud_discover_visible || self.show_host_panel {
                    crate::app::PANEL_WIDTH
                } else {
                    0.0
                };
                let menu_width = 180.0;
                let toolbar_right_padding = 24.0;
                let x = self.window_size.width
                    - panel_width
                    - toolbar_right_padding
                    - menu_width;
                let y = 56.0;
                self.overlay = Some(OverlayState {
                    content: OverlayContent::CloudProviderPicker,
                    x: x.max(0.0),
                    y,
                });
            }

            // ---- Discovery & import ----
            Message::ShowCloudDiscover(profile_id) => {
                // Dismiss the "+ Host [▾]" provider picker that
                // dispatched this message — without it the dropdown
                // hangs on top of the freshly-opened discovery panel.
                self.overlay = None;
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
                // Reset selection on every refresh — the upstream might
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
            Message::CloudDiscoverImport => {
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
                    // Flat layout (per user feedback): EC2 hosts go to
                    // the root, ECS services land as root-level
                    // dynamic groups. We deliberately skip
                    // auto-creating a "<profile.label>" container
                    // folder — the user can move things into manual
                    // folders later if they want hierarchy.
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
                        conn.username = e.default_username.clone();
                        // group_id stays None — host lands at root.
                        conn.cloud_ref = Some(CloudRef {
                            profile_id,
                            resource_type: CloudResourceType::Ec2,
                            resource_id: e.instance_id.clone(),
                            region: Some(e.region.clone()),
                            // Default to plain SSH on import — the user
                            // can switch to Instance Connect / SSM in
                            // the host editor once those transports
                            // ship. SSH is the universally-applicable
                            // baseline.
                            transport_pref: TransportKind::Ssh,
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
                    // ship until PR 5 — clicking a task today gives a
                    // friendly "not implemented" error, but the
                    // structure is already persisted and syncable.
                    for s in &selected_ecs {
                        let label = format!("{} / {}", s.service, s.container);
                        let mut g = Group::new(label);
                        // No parent — dynamic groups live at root,
                        // siblings of EC2 hosts.
                        g.icon = Some("si:aws".into());
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

    /// Look up the registered provider for a profile and dispatch an
    /// async `discover()` call. Lifts boilerplate out of the message
    /// arms so refresh + first-open share the same path.
    fn spawn_discover(&mut self, profile_id: Uuid) -> Result<Task<Message>, Message> {
        let Some(profile) = self
            .cloud_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
        else {
            self.cloud_discover_state =
                CloudDiscoverState::Failed("profile not found".into());
            return Ok(Task::none());
        };
        let registry: Arc<CloudProviderRegistry> = self.cloud_provider_registry.clone();
        let Some(provider) = registry.get(&profile.provider) else {
            self.cloud_discover_state = CloudDiscoverState::Failed(format!(
                "provider \"{}\" not registered",
                profile.provider
            ));
            return Ok(Task::none());
        };
        self.cloud_discover_state = CloudDiscoverState::Running;
        Ok(Task::perform(
            async move { provider.discover(&profile).await },
            |result| {
                Message::CloudDiscoverResult(
                    result
                        .map(Box::new)
                        .map_err(|e| e.to_string()),
                )
            },
        ))
    }

    /// Build an in-memory `CloudProfile` from the current wizard form
    /// state — used by `test_credentials` so the user can verify
    /// without saving first. Returns `None` when the label is empty.
    fn build_cloud_profile_for_test(&self) -> Option<CloudProfile> {
        let label = self.cloud_form_label.trim();
        if label.is_empty() {
            return None;
        }
        let mut profile = CloudProfile::new(label, self.cloud_form_provider.id());
        profile.auth_kind = self.cloud_form_auth_kind.id().to_string();
        profile.config = self.serialize_cloud_form_config();
        Some(profile)
    }

    fn serialize_cloud_form_config(&self) -> String {
        let mut obj = serde_json::Map::new();
        match self.cloud_form_provider {
            CloudProviderChoice::Aws => {
                let pn = self.cloud_form_aws_profile_name.trim();
                if !pn.is_empty() {
                    obj.insert("profile_name".into(), serde_json::Value::String(pn.into()));
                }
                let region = self.cloud_form_aws_region.trim();
                if !region.is_empty() {
                    obj.insert("region".into(), serde_json::Value::String(region.into()));
                }
            }
            CloudProviderChoice::K8s => {
                // K8s wizard doesn't write any fields yet — the provider
                // crate ships in a follow-up PR. The empty `{}` config
                // is still valid JSON so the row round-trips through
                // the vault.
            }
        }
        serde_json::Value::Object(obj).to_string()
    }
}
