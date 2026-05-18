//! Dynamic group handlers, the live ECS-tasks (etc.) resolve, the
//! "edit dynamic group" right-panel form (open, field changes, save),
//! the delete + per-card overlay menu, and the resolved-result fan-out.

use std::sync::Arc;

use iced::Task;
use oryxis_cloud::CloudProviderRegistry;

use crate::app::{Message, Oryxis};
use crate::state::{DynamicGroupState, OverlayContent, OverlayState};

impl Oryxis {
    pub(super) fn handle_cloud_dynamic_group(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::DynamicGroupResolve(gid) => {
                let Some(group) = self.groups.iter().find(|g| g.id == gid).cloned() else {
                    return Ok(Task::none());
                };
                let Some(query) = group.cloud_query.clone() else {
                    return Ok(Task::none());
                };
                let Some(profile) = self.resolve_cloud_profile(query.profile_id) else {
                    self.cloud_dynamic_group_state.insert(
                        gid,
                        DynamicGroupState::Failed(
                            "linked cloud profile no longer exists".into(),
                        ),
                    );
                    return Ok(Task::none());
                };
                let registry: Arc<CloudProviderRegistry> =
                    self.cloud_provider_registry.clone();
                let Some(provider) = registry.get(&profile.provider) else {
                    self.cloud_dynamic_group_state.insert(
                        gid,
                        DynamicGroupState::Failed(format!(
                            "provider \"{}\" not registered",
                            profile.provider
                        )),
                    );
                    return Ok(Task::none());
                };
                self.cloud_dynamic_group_state
                    .insert(gid, DynamicGroupState::Loading);
                return Ok(Task::perform(
                    async move { provider.resolve_query(&profile, &query).await },
                    move |result| {
                        Message::DynamicGroupResolved(gid, result.map_err(|e| e.to_string()))
                    },
                ));
            }
            Message::DynamicGroupResolved(gid, result) => {
                let next = match result {
                    Ok(hosts) => DynamicGroupState::Loaded {
                        hosts,
                        fetched_at: chrono::Utc::now(),
                    },
                    Err(msg) => DynamicGroupState::Failed(msg),
                };
                self.cloud_dynamic_group_state.insert(gid, next);
            }

            // ---- Edit dynamic group panel ----
            Message::EditDynamicGroup(gid) => {
                self.overlay = None;
                let Some(group) = self.groups.iter().find(|g| g.id == gid).cloned() else {
                    return Ok(Task::none());
                };
                let Some(query) = group.cloud_query.clone() else {
                    return Ok(Task::none());
                };
                // Right-panel slot is mutually exclusive, close any
                // other panel that's currently open so the user
                // doesn't end up with two side-by-side editors.
                self.show_host_panel = false;
                self.cloud_form_visible = false;
                self.cloud_discover_visible = false;
                self.cloud_dynamic_form_visible = true;
                self.cloud_dynamic_form_group_id = Some(gid);
                self.cloud_dynamic_form_username =
                    query.template.username.clone().unwrap_or_default();
                self.cloud_dynamic_form_initial_command =
                    query.template.initial_command.clone().unwrap_or_default();
                self.cloud_dynamic_form_transport = query.template.transport;
                self.cloud_dynamic_form_selected_key = query.template.key_id.and_then(|kid| {
                    self.keys.iter().find(|k| k.id == kid).map(|k| k.label.clone())
                });
                self.cloud_dynamic_form_selected_identity =
                    query.template.identity_id.and_then(|iid| {
                        self.identities
                            .iter()
                            .find(|i| i.id == iid)
                            .map(|i| i.label.clone())
                    });
                self.cloud_dynamic_form_label = group.label.clone();
                self.cloud_dynamic_form_color = group.color.clone().unwrap_or_default();
                self.cloud_dynamic_form_icon = group.icon.clone().unwrap_or_default();
                self.cloud_dynamic_form_parent_label = group
                    .parent_id
                    .and_then(|pid| self.groups.iter().find(|g| g.id == pid))
                    .map(|g| g.label.clone())
                    .unwrap_or_default();
                match &query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                        cluster,
                        service,
                        container,
                    } => {
                        self.cloud_dynamic_form_cluster = cluster.clone();
                        self.cloud_dynamic_form_service = service.clone();
                        self.cloud_dynamic_form_container = container.clone();
                    }
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods { .. } => {
                        // K8s editing surface lands in a follow-up;
                        // clear the ECS-specific buffers so they don't
                        // leak in from a previous edit session.
                        self.cloud_dynamic_form_cluster = String::new();
                        self.cloud_dynamic_form_service = String::new();
                        self.cloud_dynamic_form_container = String::new();
                    }
                }
            }
            Message::HideDynamicGroupForm => {
                self.cloud_dynamic_form_visible = false;
                self.cloud_dynamic_form_group_id = None;
            }
            Message::DynamicGroupFormUsernameChanged(v) => {
                self.cloud_dynamic_form_username = v;
            }
            Message::DynamicGroupFormInitialCommandChanged(v) => {
                self.cloud_dynamic_form_initial_command = v;
            }
            Message::DynamicGroupFormTransportChanged(t) => {
                self.cloud_dynamic_form_transport = t;
            }
            Message::DynamicGroupFormKeyChanged(label) => {
                self.cloud_dynamic_form_selected_key = if label == "(none)" {
                    None
                } else {
                    Some(label)
                };
            }
            Message::DynamicGroupFormIdentityChanged(label) => {
                self.cloud_dynamic_form_selected_identity = if label == "(none)" {
                    None
                } else {
                    Some(label)
                };
            }
            Message::DynamicGroupFormLabelChanged(v) => {
                self.cloud_dynamic_form_label = v;
            }
            Message::DynamicGroupFormParentChanged(v) => {
                self.cloud_dynamic_form_parent_label = v;
            }
            Message::DynamicGroupFormClusterChanged(v) => {
                self.cloud_dynamic_form_cluster = v;
            }
            Message::DynamicGroupFormServiceChanged(v) => {
                self.cloud_dynamic_form_service = v;
            }
            Message::DynamicGroupFormContainerChanged(v) => {
                self.cloud_dynamic_form_container = v;
            }
            Message::ShowIconPickerForDynamicGroupForm => {
                // Pre-fill the picker from the current form values so
                // re-opens preserve the user's in-flight selection.
                // Fallback to `server` so the preview always renders.
                let icon = if self.cloud_dynamic_form_icon.trim().is_empty() {
                    "server".to_string()
                } else {
                    self.cloud_dynamic_form_icon.trim().to_string()
                };
                self.icon_picker_icon = Some(icon);
                let color = self.cloud_dynamic_form_color.trim().to_string();
                self.icon_picker_color = if color.is_empty() { None } else { Some(color.clone()) };
                self.icon_picker_hex_input = color;
                self.icon_picker_for = None;
                self.icon_picker_for_group_form = true;
                self.show_icon_picker = true;
            }
            Message::SaveDynamicGroup => {
                let Some(gid) = self.cloud_dynamic_form_group_id else {
                    return Ok(Task::none());
                };
                let Some(mut group) = self.groups.iter().find(|g| g.id == gid).cloned()
                else {
                    return Ok(Task::none());
                };
                let Some(mut query) = group.cloud_query.clone() else {
                    return Ok(Task::none());
                };
                query.template.username =
                    if self.cloud_dynamic_form_username.trim().is_empty() {
                        None
                    } else {
                        Some(self.cloud_dynamic_form_username.trim().to_string())
                    };
                query.template.initial_command =
                    if self.cloud_dynamic_form_initial_command.trim().is_empty() {
                        None
                    } else {
                        Some(self.cloud_dynamic_form_initial_command.clone())
                    };
                query.template.transport = self.cloud_dynamic_form_transport;
                query.template.key_id = self
                    .cloud_dynamic_form_selected_key
                    .as_ref()
                    .and_then(|label| {
                        self.keys.iter().find(|k| &k.label == label).map(|k| k.id)
                    });
                query.template.identity_id = self
                    .cloud_dynamic_form_selected_identity
                    .as_ref()
                    .and_then(|label| {
                        self.identities
                            .iter()
                            .find(|i| &i.label == label)
                            .map(|i| i.id)
                    });
                // ECS query fields: persist the user's edits so the
                // next resolve targets the new cluster/service/container
                // triple. Blank values are kept as-is (the user can
                // intentionally clear; AWS-side resolve will error
                // visibly).
                if let oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                    cluster,
                    service,
                    container,
                } = &mut query.kind
                {
                    *cluster = self.cloud_dynamic_form_cluster.trim().to_string();
                    *service = self.cloud_dynamic_form_service.trim().to_string();
                    *container = self.cloud_dynamic_form_container.trim().to_string();
                }
                group.cloud_query = Some(query);
                let new_label = self.cloud_dynamic_form_label.trim();
                if !new_label.is_empty() {
                    group.label = new_label.to_string();
                }
                group.color = if self.cloud_dynamic_form_color.trim().is_empty() {
                    None
                } else {
                    Some(self.cloud_dynamic_form_color.trim().to_string())
                };
                group.icon = if self.cloud_dynamic_form_icon.trim().is_empty() {
                    None
                } else {
                    Some(self.cloud_dynamic_form_icon.trim().to_string())
                };
                // Parent picker uses label matching like the host
                // editor's `parent_group`. Empty / unmatched = root.
                let parent_trimmed = self.cloud_dynamic_form_parent_label.trim();
                group.parent_id = if parent_trimmed.is_empty() {
                    None
                } else {
                    self.groups
                        .iter()
                        .find(|g| g.label == parent_trimmed && g.id != gid)
                        .map(|g| g.id)
                };
                group.updated_at = chrono::Utc::now();
                if let Some(vault) = &self.vault
                    && vault.save_group(&group).is_ok()
                {
                    self.cloud_dynamic_form_visible = false;
                    self.cloud_dynamic_form_group_id = None;
                    self.load_data_from_vault();
                }
            }
            Message::DeleteDynamicGroup(gid) => {
                self.overlay = None;
                if let Some(vault) = &self.vault {
                    let _ = vault.delete_group(&gid);
                    self.cloud_dynamic_group_state.remove(&gid);
                    // If the user was viewing this group, kick them
                    // back to root so they don't see a blank panel
                    // pointing at a deleted row.
                    if self.active_group == Some(gid) {
                        self.active_group = None;
                    }
                    self.load_data_from_vault();
                }
            }
            Message::ShowDynamicGroupCardMenu(gid) => {
                self.overlay = Some(OverlayState {
                    content: OverlayContent::DynamicGroupActions(gid),
                    x: self.mouse_position.x,
                    y: self.mouse_position.y,
                });
            }
            Message::DynamicGroupCardHovered(gid) => {
                self.hovered_dynamic_group_card = Some(gid);
            }
            Message::DynamicGroupCardUnhovered => {
                self.hovered_dynamic_group_card = None;
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
