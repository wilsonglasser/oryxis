//! Cloud-domain dispatch handlers, the `Oryxis::handle_cloud` router
//! that fans `Message` variants out to per-area submodules:
//!
//! - `form`    : Cloud Accounts wizard (CRUD on `CloudProfile`).
//! - `discovery`: discovery panel + import flow.
//! - `dynamic_group`: ECS dynamic group resolve / edit / delete.
//! - `transports`: connect actions (ECS Exec, SSM Session).
//!
//! Shared helpers used by more than one of those (profile hydration,
//! plugin spawn, error-dialog setters) live here in `mod.rs`.

#![allow(clippy::result_large_err)]

mod discovery;
mod dynamic_group;
mod form;
mod transports;

use std::sync::Arc;

use iced::Task;
use oryxis_cloud::CloudProviderRegistry;
use oryxis_core::models::cloud_profile::CloudProfile;
use uuid::Uuid;

use crate::app::{TerminalMessage, CloudMessage, Message, Oryxis};
use crate::state::{CloudAuthChoice, CloudDiscoverState, CloudProviderChoice};

impl Oryxis {
    /// Route a cloud message straight to the submodule that owns its
    /// variant. Exhaustive on purpose: a new `CloudMessage` variant
    /// fails to compile until it is listed in its owner's group, so it
    /// can never be silently dropped.
    pub(crate) fn handle_cloud(&mut self, message: CloudMessage) -> Task<Message> {
        match message {
            m @ (CloudMessage::CloudSearchChanged(..)
            | CloudMessage::ShowCloudForm(..)
            | CloudMessage::HideCloudForm
            | CloudMessage::CloudFormLabelChanged(..)
            | CloudMessage::CloudFormProviderChanged(..)
            | CloudMessage::CloudFormAuthKindChanged(..)
            | CloudMessage::CloudFormAwsProfileNameChanged(..)
            | CloudMessage::CloudFormAwsRegionDraftChanged(..)
            | CloudMessage::CloudFormAwsRegionAdd
            | CloudMessage::CloudFormAwsRegionRemove(..)
            | CloudMessage::CloudFormAwsAccessKeyIdChanged(..)
            | CloudMessage::CloudFormAwsAccessKeySecretChanged(..)
            | CloudMessage::CloudFormAwsAccessKeySessionTokenChanged(..)
            | CloudMessage::CloudFormAwsAccessKeySecretToggleVisibility
            | CloudMessage::CloudFormAwsSsoStartUrlChanged(..)
            | CloudMessage::CloudFormAwsSsoRegionChanged(..)
            | CloudMessage::CloudFormAwsSsoAccountIdChanged(..)
            | CloudMessage::CloudFormAwsSsoRoleNameChanged(..)
            | CloudMessage::CloudFormKubeconfigPathChanged(..)
            | CloudMessage::CloudFormContextChanged(..)
            | CloudMessage::CloudFormGcpProjectChanged(..)
            | CloudMessage::CloudFormAzureSubscriptionChanged(..)
            | CloudMessage::CloudFormTestCredentials
            | CloudMessage::CloudFormTestResult(..)
            | CloudMessage::SaveCloudProfile
            | CloudMessage::DeleteCloudProfile(..)
            | CloudMessage::ShowCloudCardMenu(..)
            | CloudMessage::CloudCardHovered(..)
            | CloudMessage::CloudCardUnhovered
            | CloudMessage::ShowCloudProviderPicker) => self
                .handle_cloud_form(m)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (CloudMessage::ShowCloudDiscover(..)
            | CloudMessage::HideCloudDiscover
            | CloudMessage::CloudDiscoverRefresh
            | CloudMessage::CloudDiscoverResult(..)
            | CloudMessage::CloudDiscoverToggleEc2(..)
            | CloudMessage::CloudDiscoverToggleEcs(..)
            | CloudMessage::CloudDiscoverToggleK8s(..)
            | CloudMessage::CloudDiscoverImport
            | CloudMessage::CloudDiscoverImportConfirmed
            | CloudMessage::CloudDiscoverImportCancelled
            | CloudMessage::CloudDiscoverFilterChanged(..)
            | CloudMessage::CloudDiscoverToggleSection(..)
            | CloudMessage::CloudDiscoverAddGke{ .. }
            | CloudMessage::CloudDiscoverGkeCredentials(..)
            | CloudMessage::CloudDiscoverGkeAdded(..)
            | CloudMessage::CloudDiscoverAddAks{ .. }
            | CloudMessage::CloudDiscoverAksCredentials(..)
            | CloudMessage::CloudDiscoverAksAdded(..)
            | CloudMessage::CloudDiscoverDefaultTransportChanged(..)
            | CloudMessage::CloudDiscoverDefaultGroupNameChanged(..)
            | CloudMessage::CloudDiscoverDefaultGroupPick(..)
            | CloudMessage::ToggleCloudDiscoverGroupPicker
            | CloudMessage::CloudDiscoverDefaultGroupPickerSearchChanged(..)
            | CloudMessage::CloudProfileSync(..)
            | CloudMessage::CloudProfileSyncResult(..)
            | CloudMessage::CloudAutoRefreshTick) => self
                .handle_cloud_discovery(m)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (CloudMessage::DynamicGroupFormLabelChanged(..)
            | CloudMessage::DynamicGroupFormParentChanged(..)
            | CloudMessage::DynamicGroupFormClusterChanged(..)
            | CloudMessage::DynamicGroupFormServiceChanged(..)
            | CloudMessage::DynamicGroupFormContainerChanged(..)
            | CloudMessage::DynamicGroupFormK8sContextChanged(..)
            | CloudMessage::DynamicGroupFormNamespaceChanged(..)
            | CloudMessage::DynamicGroupFormK8sSelectorKindChanged(..)
            | CloudMessage::DynamicGroupFormK8sSelectorValueChanged(..)
            | CloudMessage::ShowIconPickerForDynamicGroupForm
            | CloudMessage::DynamicGroupResolve(..)
            | CloudMessage::DynamicGroupResolved(..)
            | CloudMessage::EditDynamicGroup(..)
            | CloudMessage::HideDynamicGroupForm
            | CloudMessage::DynamicGroupFormUsernameChanged(..)
            | CloudMessage::DynamicGroupFormInitialCommandChanged(..)
            | CloudMessage::DynamicGroupFormTransportChanged(..)
            | CloudMessage::DynamicGroupFormKeyChanged(..)
            | CloudMessage::DynamicGroupFormIdentityChanged(..)
            | CloudMessage::SaveDynamicGroup
            | CloudMessage::DeleteDynamicGroup(..)
            | CloudMessage::ShowDynamicGroupCardMenu(..)
            | CloudMessage::DynamicGroupCardHovered(..)
            | CloudMessage::DynamicGroupCardUnhovered) => self
                .handle_cloud_dynamic_group(m)
                .unwrap_or_else(crate::dispatch::unrouted),
            m @ (CloudMessage::PluginSessionEnded(..)
            | CloudMessage::EcsExecConnectFreshTask{ .. }
            | CloudMessage::ConnectEcsExecTask{ .. }
            | CloudMessage::ConnectKubectlExecPod{ .. }
            | CloudMessage::EcsExecSessionReady{ .. }
            | CloudMessage::SsmSessionReady{ .. }) => self
                .handle_cloud_transports(m)
                .unwrap_or_else(crate::dispatch::unrouted),
        }
    }

    /// Kick off an SSM Session for a cloud-imported EC2 connection.
    /// Mirrors the ECS Exec entry point but targets the bare instance
    /// id. Caller has already verified `cloud_ref.transport_pref ==
    /// Ssm`. Errors at any step (no profile, missing region, AWS
    /// rejection) surface via `tracing::error!`; UI feedback comes
    /// later when we wire the SSH-progress UI to the SSM path too.
    pub(crate) fn start_ssm_session_for_connection(
        &self,
        conn: &oryxis_core::models::connection::Connection,
    ) -> Task<Message> {
        let Some(cref) = conn.cloud_ref.as_ref() else {
            return Task::none();
        };
        let Some(region) = cref.region.clone() else {
            tracing::error!(
                target = "oryxis::dispatch_cloud",
                "SSM Session abort: cloud_ref has no region"
            );
            return Task::none();
        };
        let Some(profile) = self.resolve_cloud_profile(cref.profile_id) else {
            tracing::error!(
                target = "oryxis::dispatch_cloud",
                "SSM Session abort: cloud profile gone"
            );
            return Task::none();
        };
        let Some(provider) = self.cloud_provider_registry.get(&profile.provider) else {
            tracing::error!(
                target = "oryxis::dispatch_cloud",
                provider = %profile.provider,
                "SSM Session abort: provider not registered"
            );
            return Task::none();
        };
        let instance_id = cref.resource_id.clone();
        let host_label = conn.label.clone();
        tracing::info!(
            target = "oryxis::dispatch_cloud",
            %instance_id,
            %region,
            "SSM: starting session"
        );
        Task::perform(
            async move {
                provider
                    .start_ssm_session(&profile, &region, &instance_id)
                    .await
                    .map(Box::new)
                    .map_err(|e| e.to_string())
            },
            move |result| Message::Cloud(CloudMessage::SsmSessionReady {
                host_label: host_label.clone(),
                result,
            }),
        )
    }

    /// Spawn `session-manager-plugin` inside a PTY-backed tab,
    /// mirroring the local-shell flow. The plugin's stdout flows into
    /// the terminal, the user's keystrokes flow back through the
    /// terminal widget's PTY write channel like any local shell. Tab
    /// title is fully formatted by the caller so SSM and ECS sessions
    /// render with their own prefix.
    pub(super) fn spawn_plugin_tab(
        &mut self,
        tab_label: &str,
        plugin_path: String,
        args: Vec<String>,
        relaunch: Option<Message>,
    ) -> Task<Message> {
        use crate::app::{DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
        use crate::state::{TerminalTab, View};
        use std::sync::Mutex;
        use tokio_stream::wrappers::UnboundedReceiverStream;

        match oryxis_terminal::widget::TerminalState::new_with_command(
            DEFAULT_TERM_COLS as u16,
            DEFAULT_TERM_ROWS as u16,
            &plugin_path,
            &args,
            None,
        ) {
            Ok((mut state, rx)) => {
                state.set_palette(self.terminal_palette.clone());
                let label = tab_label.to_string();
                let mut plugin_tab = TerminalTab::new_single(
                    label.clone(),
                    Arc::new(Mutex::new(state)),
                );
                // Cloud SSM / ECS tabs get the idle keepalive (see the
                // field doc on `TerminalTab`).
                plugin_tab.ssm_keepalive = true;
                // Cloud tabs without a saved Connection carry the message
                // that re-creates them, so Duplicate Tab can relaunch.
                plugin_tab.relaunch = relaunch.map(Box::new);
                let pane_id = plugin_tab.active().id;
                let tab_idx = self.push_terminal_tab(plugin_tab);
                // SSM/ECS sessions don't go through the SSH connecting
                // pipeline, so a leftover `connecting` (e.g. a previous
                // host's timeout that wasn't cleared) would otherwise
                // render its progress screen over this cloud terminal.
                self.connecting = None;
                self.active_tab = Some(tab_idx);
                self.remember_terminal_tab_focus(tab_idx);
                self.active_view = View::Terminal;
                // Reopening a pinned cloud tab: the dormant placeholder is
                // still in the strip. Replace it in place (by id) with the
                // freshly-spawned live tab so its chip doesn't blink out, keep
                // its slot + pin, and re-persist (reopen skipped persisting to
                // keep the dormant spec as a net).
                if let Some(dormant_id) = self.pin_next_plugin_tab.take() {
                    // Inherit the placeholder's pin state instead of
                    // forcing it: a dead *unpinned* cloud tab also rides
                    // this reopen path now (PluginSessionEnded) and must
                    // not come back pinned.
                    let keep_pin;
                    let at = if let Some(dpos) =
                        self.tabs.iter().position(|t| t._id == dormant_id)
                    {
                        // `tab_idx` is the just-pushed live tab (end); `dpos`
                        // the dormant (before it). Drop the live, drop the
                        // dormant, reinsert the live at the dormant's slot.
                        let live = self.tabs.remove(tab_idx);
                        keep_pin = self.tabs[dpos].pinned;
                        self.tabs.remove(dpos);
                        let at = dpos.min(self.tabs.len());
                        self.tabs.insert(at, live);
                        // Keep the reopened tab at the dormant's spot in the
                        // unified strip order (else reconcile appends the new id
                        // at the end).
                        self.replace_tab_order_id(dormant_id, self.tabs[at]._id);
                        at
                    } else {
                        // Dormant gone (e.g. closed mid-connect): leave the
                        // live tab where it was pushed, and don't force a
                        // pin it wouldn't inherit from anything.
                        keep_pin = false;
                        tab_idx
                    };
                    self.tabs[at].pinned = keep_pin;
                    self.active_tab = Some(at);
                    self.remember_terminal_tab_focus(at);
                    self.persist_pinned_tabs();
                }
                // ECS Exec and SSM Session don't go through SshConnected,
                // so the History view never picked them up. Mirror the
                // SSH path's add_log call here so cloud sessions show up
                // alongside regular hosts.
                if self.should_record_history()
                    && let Some(vault) = &self.vault {
                    let entry = oryxis_core::models::log_entry::LogEntry::new(
                        &label,
                        &label,
                        oryxis_core::models::log_entry::LogEvent::Connected,
                        "Session established",
                    );
                    let _ = vault.add_log(&entry);
                }
                let stream = UnboundedReceiverStream::new(rx);
                Task::batch(vec![
                    self.tab_scroll_to_active(),
                    // When the plugin process exits the stream closes;
                    // chain the end-of-session notice so the pane never
                    // goes silently dead (issue #38 follow-up).
                    Task::stream(stream)
                        .map(move |bytes| Message::Terminal(TerminalMessage::PtyOutput(pane_id, bytes)))
                        .chain(Task::done(Message::Cloud(CloudMessage::PluginSessionEnded(pane_id)))),
                ])
            }
            Err(e) => {
                tracing::error!(
                    target = "oryxis::dispatch_cloud",
                    error = %e,
                    "Failed to spawn session-manager-plugin in PTY"
                );
                if self.should_record_history()
                    && let Some(vault) = &self.vault {
                    let entry = oryxis_core::models::log_entry::LogEntry::new(
                        tab_label,
                        tab_label,
                        oryxis_core::models::log_entry::LogEvent::Error,
                        &format!("Failed to spawn session-manager-plugin: {e}"),
                    );
                    let _ = vault.add_log(&entry);
                }
                self.show_error_dialog(
                    crate::i18n::t("plugin_spawn_failed_title").to_string(),
                    format!("{e}"),
                );
                Task::none()
            }
        }
    }

    /// Look up the registered provider for a profile and dispatch an
    /// async `discover()` call. Lifts boilerplate out of the message
    /// arms so refresh + first-open share the same path.
    pub(super) fn spawn_discover(
        &mut self,
        profile_id: Uuid,
    ) -> Task<Message> {
        let Some(profile) = self
            .cloud_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
        else {
            self.cloud_discover_state =
                CloudDiscoverState::Failed("profile not found".into());
            return Task::none();
        };
        let registry: Arc<CloudProviderRegistry> = self.cloud_provider_registry.clone();
        let Some(provider) = registry.get(&profile.provider) else {
            self.cloud_discover_state = CloudDiscoverState::Failed(format!(
                "provider \"{}\" not registered",
                profile.provider
            ));
            return Task::none();
        };
        self.cloud_discover_state = CloudDiscoverState::Running;
        Task::perform(
            async move { provider.discover(&profile).await },
            |result| {
                Message::Cloud(CloudMessage::CloudDiscoverResult(
                    result
                        .map(Box::new)
                        .map_err(|e| e.to_string()),
                ))
            },
        )
    }

    /// Build an in-memory `CloudProfile` from the current wizard form
    /// state, used by `test_credentials` so the user can verify
    /// without saving first. Returns `None` when the label is empty.
    pub(super) fn build_cloud_profile_for_test(&self) -> Option<CloudProfile> {
        let label = self.cloud_form.label.trim();
        if label.is_empty() {
            return None;
        }
        let mut profile = CloudProfile::new(label, self.cloud_form.provider.id());
        profile.auth_kind = self.cloud_form.auth_kind.id().to_string();
        profile.config = self.serialize_cloud_form_config();
        // Test Credentials runs against the *current* form values
        // (not what's persisted in the vault yet), so feed the form's
        // secret straight in. For the "edit existing profile, didn't
        // touch the secret field" case, fall back to the stored
        // secret so the test still works without re-typing.
        profile.secret = if self.cloud_form.aws_access_key_secret_touched {
            if self.cloud_form.aws_access_key_secret.is_empty() {
                None
            } else {
                Some(self.cloud_form.aws_access_key_secret.clone())
            }
        } else {
            self.cloud_form.editing_id.and_then(|id| {
                self.vault
                    .as_ref()
                    .and_then(|v| v.get_cloud_profile_secret(&id).ok().flatten())
            })
        };
        Some(profile)
    }

    /// Populate the blocking error dialog with a free-form title +
    /// body. No link button. Used for AWS SDK errors where the body
    /// is the SDK-formatted message; the user reads it and acts (fix
    /// IAM, re-auth SSO, etc).
    pub(crate) fn show_error_dialog(&mut self, title: String, body: String) {
        self.error_dialog = Some(crate::state::ErrorDialog {
            title,
            body,
            link: None,
            action: None,
        });
    }

    /// Populate the blocking error dialog telling the user the AWS
    /// session-manager-plugin is missing from their system. Body comes
    /// from i18n; the docs URL is canonical AWS and points at the
    /// per-OS install instructions page that AWS keeps up to date.
    pub(crate) fn show_plugin_missing_dialog(&mut self) {
        self.error_dialog = Some(crate::state::ErrorDialog {
            title: crate::i18n::t("plugin_missing_title").to_string(),
            body: crate::i18n::t("plugin_missing_body").to_string(),
            link: Some(crate::state::ErrorDialogLink {
                label: crate::i18n::t("error_dialog_open_aws_docs").to_string(),
                url: crate::session_manager_plugin::AWS_DOCS_INSTALL_URL.to_string(),
            }),
            action: None,
        });
    }

    /// Clone a cloud profile from in-memory state and hydrate its
    /// transient `secret` field from the vault. Used by every site
    /// that's about to hand the profile off to a `CloudProvider` call:
    /// without the secret hydration, `access_key` and `sso` auth
    /// would fail with "missing secret" even when one is stored.
    pub(crate) fn resolve_cloud_profile(
        &self,
        profile_id: Uuid,
    ) -> Option<CloudProfile> {
        let mut profile = self
            .cloud_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()?;
        if let Some(vault) = &self.vault {
            profile.secret = vault.get_cloud_profile_secret(&profile_id).ok().flatten();
        }
        Some(profile)
    }

    pub(super) fn serialize_cloud_form_config(&self) -> String {
        let mut obj = serde_json::Map::new();
        let put = |obj: &mut serde_json::Map<String, serde_json::Value>, k: &str, v: &str| {
            let v = v.trim();
            if !v.is_empty() {
                obj.insert(k.into(), serde_json::Value::String(v.into()));
            }
        };
        match self.cloud_form.provider {
            CloudProviderChoice::Aws => {
                // Workload regions are shared across all AWS auth
                // kinds. Persist both the legacy `region` key (= first
                // entry) and the `regions` array so older builds keep
                // working. SSO has its own `sso_region` for the IdC
                // endpoint, unrelated.
                if let Some(first) = self.cloud_form.aws_regions.first() {
                    put(&mut obj, "region", first);
                }
                if !self.cloud_form.aws_regions.is_empty() {
                    let arr: Vec<serde_json::Value> = self
                        .cloud_form.aws_regions
                        .iter()
                        .map(|r| serde_json::Value::String(r.clone()))
                        .collect();
                    obj.insert("regions".into(), serde_json::Value::Array(arr));
                }
                match self.cloud_form.auth_kind {
                    CloudAuthChoice::Profile => {
                        put(&mut obj, "profile_name", &self.cloud_form.aws_profile_name);
                    }
                    CloudAuthChoice::AccessKey => {
                        put(&mut obj, "access_key_id", &self.cloud_form.aws_access_key_id);
                        put(
                            &mut obj,
                            "access_key_session_token",
                            &self.cloud_form.aws_access_key_session_token,
                        );
                    }
                    CloudAuthChoice::Sso => {
                        put(&mut obj, "sso_start_url", &self.cloud_form.aws_sso_start_url);
                        put(&mut obj, "sso_region", &self.cloud_form.aws_sso_region);
                        put(&mut obj, "sso_account_id", &self.cloud_form.aws_sso_account_id);
                        put(&mut obj, "sso_role_name", &self.cloud_form.aws_sso_role_name);
                    }
                    CloudAuthChoice::Kubeconfig
                    | CloudAuthChoice::GcloudCli
                    | CloudAuthChoice::AzCli => {
                        // Kubeconfig / gcloud / az auth belong to other
                        // providers; under AWS an impossible combo, so
                        // write nothing.
                    }
                }
            }
            CloudProviderChoice::K8s => {
                // Both optional: a blank kubeconfig falls back to
                // kubectl's default file, a blank context to the
                // kubeconfig's current-context. `put` skips empties.
                put(&mut obj, "kubeconfig", &self.cloud_form.kubeconfig_path);
                put(&mut obj, "context", &self.cloud_form.context);
            }
            CloudProviderChoice::Gcp => {
                // Optional project scope; blank = gcloud's active project.
                put(&mut obj, "project", &self.cloud_form.gcp_project);
            }
            CloudProviderChoice::Azure => {
                // Optional subscription scope; blank = az's active
                // subscription.
                put(&mut obj, "subscription", &self.cloud_form.azure_subscription);
            }
        }
        serde_json::Value::Object(obj).to_string()
    }
}

/// Extract the workload region from a cloud profile's `config` JSON.
///
/// The app no longer carries the AWS provider's config schema (that
/// moved into the plugin), so it just reads the conventional
/// `region` key, falling back to the first entry of `regions`.
/// Returns an empty string when neither is present; the downstream
/// API call then rejects with a clear "region required" error.
pub(super) fn region_from_profile_config(config: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(config) else {
        return String::new();
    };
    value
        .get("region")
        .and_then(|r| r.as_str())
        .map(str::to_string)
        .or_else(|| {
            value
                .get("regions")
                .and_then(|r| r.as_array())
                .and_then(|a| a.first())
                .and_then(|r| r.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}
