//! The Discover panel itself: open it on a profile, close it, ask
//! the provider again, and take the answer.
//!
//! Opening is also the place that closes every other right-panel,
//! since the slot holds one at a time.

use super::*;

impl Oryxis {
    pub(super) fn handle_discover_panel(
        &mut self,
        message: CloudMessage,
    ) -> Result<Task<Message>, CloudMessage> {
        match message {
            CloudMessage::ShowCloudDiscover(profile_id) => {
                // Dismiss the "+ Host [▾]" provider picker that
                // dispatched this message, without it the dropdown
                // hangs on top of the freshly-opened discovery panel.
                self.overlay = None;
                // Close any other right-panel (mutually exclusive slot).
                self.panels.host_panel = false;
                // Sweep revealed stored secrets (typed edits survive).
                self.sweep_editor_secrets();
                self.cloud_form.visible = false;
                self.cloud_dynamic_form.visible = false;
                self.group_edit.visible = false;
                self.cloud_discover.visible = true;
                self.cloud_discover.profile_id = Some(profile_id);
                self.cloud_discover.selected_ec2.clear();
                self.cloud_discover.selected_ecs.clear();
                self.cloud_discover.selected_k8s.clear();
                self.cloud_discover.filter.clear();
                self.cloud_discover.state = CloudDiscoverState::Idle;
                // Default the input to the profile's own label so the
                // most common case (one folder per profile) requires
                // zero typing. The user can clear or change before
                // hitting Import.
                self.cloud_discover.default_group_name = self
                    .cloud_profiles
                    .iter()
                    .find(|p| p.id == profile_id)
                    .map(|p| p.label.clone())
                    .unwrap_or_default();
                return Ok(self.spawn_discover(profile_id));
            }
            CloudMessage::HideCloudDiscover => {
                self.cloud_discover.visible = false;
                self.cloud_discover.profile_id = None;
                self.cloud_discover.state = CloudDiscoverState::Idle;
                self.cloud_discover.selected_ec2.clear();
                self.cloud_discover.selected_ecs.clear();
                self.cloud_discover.selected_k8s.clear();
                self.cloud_discover.filter.clear();
            }
            CloudMessage::CloudDiscoverRefresh => {
                if let Some(id) = self.cloud_discover.profile_id {
                    return Ok(self.spawn_discover(id));
                }
            }
            CloudMessage::CloudDiscoverResult(result) => {
                self.cloud_discover.state = match result {
                    Ok(boxed) => CloudDiscoverState::Loaded(*boxed),
                    Err(msg) => CloudDiscoverState::Failed(msg),
                };
                // Reset selection on every refresh, the upstream might
                // have changed (instance terminated, new ones spun up),
                // and silently keeping a checked id that no longer
                // exists in the new list would be misleading.
                self.cloud_discover.selected_ec2.clear();
                self.cloud_discover.selected_ecs.clear();
                self.cloud_discover.selected_k8s.clear();
                // Stamp the profile's last_discovered when we got real
                // results, so the cards list shows fresh metadata.
                if matches!(self.cloud_discover.state, CloudDiscoverState::Loaded(_))
                    && let Some(id) = self.cloud_discover.profile_id
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
            // The parent routed us here, so a message that is not
            // in this family is a grouping mistake. Hand it back
            // rather than swallow it.
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
