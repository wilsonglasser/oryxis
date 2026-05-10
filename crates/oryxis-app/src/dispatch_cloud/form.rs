//! Cloud Accounts wizard handlers, the form open/close, every field
//! change, the live "Test credentials" round-trip, and the Save /
//! Delete actions plus the per-card overlay menu / hover state.

use std::sync::Arc;

use iced::Task;
use oryxis_cloud::CloudProviderRegistry;
use oryxis_core::models::cloud_profile::CloudProfile;

use crate::app::{Message, Oryxis};
use crate::state::{
    CloudAuthChoice, CloudProviderChoice, CloudTestState, OverlayContent, OverlayState,
};

impl Oryxis {
    pub(super) fn handle_cloud_form(
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
                // Close any other right-panel (mutually exclusive slot).
                self.show_host_panel = false;
                self.cloud_dynamic_form_visible = false;
                self.cloud_discover_visible = false;
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
                    let str_field = |key: &str| {
                        cfg.get(key)
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    };
                    self.cloud_form_aws_profile_name = str_field("profile_name");
                    self.cloud_form_aws_region = str_field("region");
                    self.cloud_form_aws_access_key_id = str_field("access_key_id");
                    self.cloud_form_aws_access_key_session_token =
                        str_field("access_key_session_token");
                    self.cloud_form_aws_sso_start_url = str_field("sso_start_url");
                    self.cloud_form_aws_sso_region = str_field("sso_region");
                    self.cloud_form_aws_sso_account_id = str_field("sso_account_id");
                    self.cloud_form_aws_sso_role_name = str_field("sso_role_name");
                    // Never pre-fill the secret. Same convention as
                    // identity / proxy passwords, we just flag that
                    // one exists so the user knows leaving the field
                    // blank preserves it.
                    self.cloud_form_aws_access_key_secret = String::new();
                    self.cloud_form_aws_access_key_secret_touched = false;
                    self.cloud_form_aws_has_existing_secret = self
                        .vault
                        .as_ref()
                        .and_then(|v| v.get_cloud_profile_secret(&id).ok().flatten())
                        .is_some();
                } else {
                    self.editing_cloud_profile_id = None;
                    self.cloud_form_label = String::new();
                    self.cloud_form_provider = CloudProviderChoice::Aws;
                    self.cloud_form_auth_kind = CloudAuthChoice::Profile;
                    self.cloud_form_aws_profile_name = String::new();
                    self.cloud_form_aws_region = String::new();
                    self.cloud_form_aws_access_key_id = String::new();
                    self.cloud_form_aws_access_key_secret = String::new();
                    self.cloud_form_aws_access_key_secret_touched = false;
                    self.cloud_form_aws_access_key_session_token = String::new();
                    self.cloud_form_aws_has_existing_secret = false;
                    self.cloud_form_aws_sso_start_url = String::new();
                    self.cloud_form_aws_sso_region = String::new();
                    self.cloud_form_aws_sso_account_id = String::new();
                    self.cloud_form_aws_sso_role_name = String::new();
                }
                self.cloud_form_aws_access_key_secret_visible = false;
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
                // Reset auth choice when provider switches, Profile is
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
            Message::CloudFormAwsAccessKeyIdChanged(v) => {
                self.cloud_form_aws_access_key_id = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsAccessKeySecretChanged(v) => {
                self.cloud_form_aws_access_key_secret = v;
                self.cloud_form_aws_access_key_secret_touched = true;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsAccessKeySessionTokenChanged(v) => {
                self.cloud_form_aws_access_key_session_token = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsAccessKeySecretToggleVisibility => {
                self.cloud_form_aws_access_key_secret_visible =
                    !self.cloud_form_aws_access_key_secret_visible;
            }
            Message::CloudFormAwsSsoStartUrlChanged(v) => {
                self.cloud_form_aws_sso_start_url = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsSsoRegionChanged(v) => {
                self.cloud_form_aws_sso_region = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsSsoAccountIdChanged(v) => {
                self.cloud_form_aws_sso_account_id = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormAwsSsoRoleNameChanged(v) => {
                self.cloud_form_aws_sso_role_name = v;
                self.cloud_form_test_state = CloudTestState::Idle;
            }
            Message::CloudFormTestCredentials => {
                let provider_id = self.cloud_form_provider.id();
                let registry: Arc<CloudProviderRegistry> =
                    self.cloud_provider_registry.clone();
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
                // Capture the previous label *before* we mutate the
                // profile so we can rename the matching provider folder
                // (linked by label until v0.7 adds a stable cloud
                // profile id column to groups).
                let old_label = self.editing_cloud_profile_id.and_then(|id| {
                    self.cloud_profiles
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.label.clone())
                });
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
                profile.label = label.clone();
                profile.provider = self.cloud_form_provider.id().to_string();
                profile.auth_kind = self.cloud_form_auth_kind.id().to_string();
                profile.config = self.serialize_cloud_form_config();
                profile.updated_at = now;

                if let Some(vault) = &self.vault {
                    // Tri-state secret: only override the encrypted
                    // column when the user actually typed in the
                    // field. Empty + touched = explicit clear.
                    // Touched + value = set. Untouched = preserve.
                    let secret_arg: Option<&str> = if self
                        .cloud_form_aws_access_key_secret_touched
                    {
                        if self.cloud_form_aws_access_key_secret.is_empty() {
                            Some("")
                        } else {
                            Some(self.cloud_form_aws_access_key_secret.as_str())
                        }
                    } else {
                        None
                    };
                    match vault.save_cloud_profile(&profile, secret_arg) {
                        Ok(()) => {
                            // Rename the matching provider folder when
                            // the profile label changed. Match is by
                            // old label + no cloud_query (provider
                            // folders never carry one) to avoid
                            // touching dynamic groups that share names.
                            if let Some(old) = old_label
                                && old != label
                            {
                                let to_rename: Vec<_> = self
                                    .groups
                                    .iter()
                                    .filter(|g| g.label == old && g.cloud_query.is_none())
                                    .map(|g| g.id)
                                    .collect();
                                for gid in to_rename {
                                    if let Some(g) =
                                        self.groups.iter().find(|g| g.id == gid).cloned()
                                    {
                                        let mut renamed = g;
                                        renamed.label = label.clone();
                                        renamed.updated_at = chrono::Utc::now();
                                        let _ = vault.save_group(&renamed);
                                    }
                                }
                            }
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
                // to their toolbar, independent of cursor location.
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
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
