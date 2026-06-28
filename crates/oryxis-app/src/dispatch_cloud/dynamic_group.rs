//! Dynamic group handlers, the live ECS-tasks (etc.) resolve, the
//! "edit dynamic group" right-panel form (open, field changes, save),
//! the delete + per-card overlay menu, and the resolved-result fan-out.

use std::sync::Arc;

use iced::Task;
use oryxis_cloud::CloudProviderRegistry;

use crate::app::{Message, Oryxis};
use crate::state::{DynamicGroupState, OverlayContent, OverlayState};

impl Oryxis {
    /// `true` when a dynamic (cloud-query) group's resolved children are
    /// missing or stale (older than the cache TTL) and should be
    /// re-resolved. Manual groups and in-flight / failed resolves return
    /// `false`: don't restart an in-flight resolve, and let the user retry
    /// a failure explicitly. Shared by the dashboard `OpenGroup` path and
    /// the new-tab picker drill-in so both honour the same TTL.
    pub(crate) fn dynamic_group_needs_resolve(&self, gid: uuid::Uuid) -> bool {
        self.groups
            .iter()
            .any(|g| g.id == gid && g.cloud_query.is_some())
            && match self.cloud_dynamic_group_state.get(&gid) {
                None => true,
                Some(DynamicGroupState::Loaded { fetched_at, .. }) => {
                    chrono::Utc::now().signed_duration_since(*fetched_at)
                        > chrono::Duration::seconds(
                            crate::dispatch::DYNAMIC_GROUP_CACHE_TTL_SECS,
                        )
                }
                Some(_) => false,
            }
    }

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
                let loaded = matches!(next, DynamicGroupState::Loaded { .. });
                self.cloud_dynamic_group_state.insert(gid, next);
                // A deferred connect-to-current-task was waiting on this
                // resolve: replay it now that the listing is fresh. Only
                // on Loaded; a Failed resolve must not loop the resolve.
                if let Some(pending) = self
                    .pending_ecs_autoconnect
                    .take_if(|p| p.group_id == gid)
                {
                    if loaded {
                        return Ok(self.update(Message::EcsExecConnectFreshTask {
                            group_id: pending.group_id,
                            container: pending.container,
                            fallback_task_id: pending.fallback_task_id,
                        }));
                    }
                    self.show_error_dialog(
                        crate::i18n::t("ecs_exec_start_failed_title").to_string(),
                        crate::i18n::t("ecs_exec_no_running_tasks").to_string(),
                    );
                }
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
                self.group_edit_visible = false;
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
                        self.cloud_dynamic_form_is_k8s = false;
                        self.cloud_dynamic_form_cluster = cluster.clone();
                        self.cloud_dynamic_form_service = service.clone();
                        self.cloud_dynamic_form_container = container.clone();
                        self.cloud_dynamic_form_k8s_context = String::new();
                        self.cloud_dynamic_form_namespace = String::new();
                        self.cloud_dynamic_form_k8s_selector_kind =
                            crate::state::K8sSelectorKind::Labels;
                        self.cloud_dynamic_form_k8s_selector_value = String::new();
                    }
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods {
                        context,
                        namespace,
                        selector,
                    } => {
                        self.cloud_dynamic_form_is_k8s = true;
                        self.cloud_dynamic_form_k8s_context = context.clone();
                        self.cloud_dynamic_form_namespace = namespace.clone();
                        let (kind, value) = selector_to_form(selector);
                        self.cloud_dynamic_form_k8s_selector_kind = kind;
                        self.cloud_dynamic_form_k8s_selector_value = value;
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
            Message::DynamicGroupFormK8sContextChanged(v) => {
                self.cloud_dynamic_form_k8s_context = v;
            }
            Message::DynamicGroupFormNamespaceChanged(v) => {
                self.cloud_dynamic_form_namespace = v;
            }
            Message::DynamicGroupFormK8sSelectorKindChanged(k) => {
                self.cloud_dynamic_form_k8s_selector_kind = k;
            }
            Message::DynamicGroupFormK8sSelectorValueChanged(v) => {
                self.cloud_dynamic_form_k8s_selector_value = v;
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
                self.icon_picker_for_local_terminal = false;
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
                match &mut query.kind {
                    oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                        cluster,
                        service,
                        container,
                    } => {
                        *cluster = self.cloud_dynamic_form_cluster.trim().to_string();
                        *service = self.cloud_dynamic_form_service.trim().to_string();
                        *container = self.cloud_dynamic_form_container.trim().to_string();
                    }
                    // K8s query fields: persist context / namespace and the
                    // parsed `k=v,k=v` label selector.
                    oryxis_core::models::cloud::CloudQueryKind::K8sPods {
                        context,
                        namespace,
                        selector,
                    } => {
                        *context = self.cloud_dynamic_form_k8s_context.trim().to_string();
                        *namespace = self.cloud_dynamic_form_namespace.trim().to_string();
                        *selector = form_to_selector(
                            self.cloud_dynamic_form_k8s_selector_kind,
                            &self.cloud_dynamic_form_k8s_selector_value,
                        );
                    }
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

/// Turn a `PodSelector` into the editor's `(kind, value)` pair. For
/// `Labels` the value is the `k=v,k=v` string; for the name-based kinds
/// it's the resource name.
fn selector_to_form(
    selector: &oryxis_core::models::cloud::PodSelector,
) -> (crate::state::K8sSelectorKind, String) {
    use crate::state::K8sSelectorKind as K;
    use oryxis_core::models::cloud::PodSelector;
    match selector {
        PodSelector::Labels(m) => (
            K::Labels,
            m.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(","),
        ),
        PodSelector::Deployment(name) => (K::Deployment, name.clone()),
        PodSelector::StatefulSet(name) => (K::StatefulSet, name.clone()),
        PodSelector::Name(name) => (K::Name, name.clone()),
    }
}

/// Build a `PodSelector` from the editor's `(kind, value)`. `Labels` parses
/// the `k=v,k=v` string (blank / malformed pairs dropped); the name-based
/// kinds take the trimmed value as the resource name.
fn form_to_selector(
    kind: crate::state::K8sSelectorKind,
    value: &str,
) -> oryxis_core::models::cloud::PodSelector {
    use crate::state::K8sSelectorKind as K;
    use oryxis_core::models::cloud::PodSelector;
    match kind {
        K::Labels => {
            let map: std::collections::BTreeMap<String, String> = value
                .split(',')
                .filter_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    let (k, v) = (k.trim(), v.trim());
                    (!k.is_empty()).then(|| (k.to_string(), v.to_string()))
                })
                .collect();
            PodSelector::Labels(map)
        }
        K::Deployment => PodSelector::Deployment(value.trim().to_string()),
        K::StatefulSet => PodSelector::StatefulSet(value.trim().to_string()),
        K::Name => PodSelector::Name(value.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::K8sSelectorKind as K;
    use oryxis_core::models::cloud::PodSelector;

    #[test]
    fn labels_round_trip_through_form() {
        let (kind, value) = selector_to_form(&form_to_selector(K::Labels, "app=nginx, tier=frontend"));
        assert_eq!(kind, K::Labels);
        // Whitespace trimmed; BTreeMap sorts keys.
        assert_eq!(value, "app=nginx,tier=frontend");
    }

    #[test]
    fn malformed_label_pairs_are_dropped() {
        match form_to_selector(K::Labels, "garbage,app=nginx,=novalue") {
            PodSelector::Labels(m) => {
                assert_eq!(m.len(), 1);
                assert_eq!(m.get("app").map(String::as_str), Some("nginx"));
            }
            _ => panic!("expected Labels"),
        }
    }

    #[test]
    fn name_kinds_round_trip_without_flattening() {
        for (kind, ctor) in [
            (K::Deployment, PodSelector::Deployment("web".into())),
            (K::StatefulSet, PodSelector::StatefulSet("db".into())),
            (K::Name, PodSelector::Name("pod-xyz".into())),
        ] {
            let (k, v) = selector_to_form(&ctor);
            assert_eq!(k, kind);
            // Re-building from the form yields the same selector, no flatten.
            assert_eq!(form_to_selector(k, &v), ctor);
        }
    }
}
