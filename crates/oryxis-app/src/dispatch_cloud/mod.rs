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

use crate::app::{Message, Oryxis};
use crate::state::{CloudAuthChoice, CloudDiscoverState, CloudProviderChoice};

impl Oryxis {
    /// Dispatch a cloud-related `Message` to the matching submodule
    /// handler. Each submodule returns `Err(message)` for variants it
    /// doesn't handle so the chain falls through to the next; the
    /// final `Err` propagates back to `dispatch::update` so non-cloud
    /// handlers (or the inline match) get their turn.
    pub(crate) fn handle_cloud(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        let message = match self.handle_cloud_form(message) {
            Ok(task) => return Ok(task),
            Err(m) => m,
        };
        let message = match self.handle_cloud_discovery(message) {
            Ok(task) => return Ok(task),
            Err(m) => m,
        };
        let message = match self.handle_cloud_dynamic_group(message) {
            Ok(task) => return Ok(task),
            Err(m) => m,
        };
        let message = match self.handle_cloud_transports(message) {
            Ok(task) => return Ok(task),
            Err(m) => m,
        };
        Err(message)
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
            move |result| Message::SsmSessionReady {
                host_label: host_label.clone(),
                result,
            },
        )
    }

    /// Spawn `session-manager-plugin` inside a PTY-backed tab,
    /// mirroring the local-shell flow. The plugin's stdout flows into
    /// the terminal, the user's keystrokes flow back via the standard
    /// `Message::PtyInput` path. Tab title is fully formatted by the
    /// caller so SSM and ECS sessions render with their own prefix.
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
        ) {
            Ok((mut state, rx)) => {
                state.palette = self.terminal_palette.clone();
                let tab_idx = self.tabs.len();
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
                self.tabs.push(plugin_tab);
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
                    let at = if let Some(dpos) =
                        self.tabs.iter().position(|t| t._id == dormant_id)
                    {
                        // `tab_idx` is the just-pushed live tab (end); `dpos`
                        // the dormant (before it). Drop the live, drop the
                        // dormant, reinsert the live at the dormant's slot.
                        let live = self.tabs.remove(tab_idx);
                        self.tabs.remove(dpos);
                        let at = dpos.min(self.tabs.len());
                        self.tabs.insert(at, live);
                        at
                    } else {
                        // Dormant gone (e.g. closed mid-connect): leave the
                        // live tab where it was pushed.
                        tab_idx
                    };
                    self.tabs[at].pinned = true;
                    self.active_tab = Some(at);
                    self.remember_terminal_tab_focus(at);
                    self.persist_pinned_tabs();
                }
                // ECS Exec and SSM Session don't go through SshConnected,
                // so the History view never picked them up. Mirror the
                // SSH path's add_log call here so cloud sessions show up
                // alongside regular hosts.
                if let Some(vault) = &self.vault {
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
                    Task::stream(stream).map(move |bytes| Message::PtyOutput(pane_id, bytes)),
                ])
            }
            Err(e) => {
                tracing::error!(
                    target = "oryxis::dispatch_cloud",
                    error = %e,
                    "Failed to spawn session-manager-plugin in PTY"
                );
                if let Some(vault) = &self.vault {
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
    ) -> Result<Task<Message>, Message> {
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
    /// state, used by `test_credentials` so the user can verify
    /// without saving first. Returns `None` when the label is empty.
    pub(super) fn build_cloud_profile_for_test(&self) -> Option<CloudProfile> {
        let label = self.cloud_form_label.trim();
        if label.is_empty() {
            return None;
        }
        let mut profile = CloudProfile::new(label, self.cloud_form_provider.id());
        profile.auth_kind = self.cloud_form_auth_kind.id().to_string();
        profile.config = self.serialize_cloud_form_config();
        // Test Credentials runs against the *current* form values
        // (not what's persisted in the vault yet), so feed the form's
        // secret straight in. For the "edit existing profile, didn't
        // touch the secret field" case, fall back to the stored
        // secret so the test still works without re-typing.
        profile.secret = if self.cloud_form_aws_access_key_secret_touched {
            if self.cloud_form_aws_access_key_secret.is_empty() {
                None
            } else {
                Some(self.cloud_form_aws_access_key_secret.clone())
            }
        } else {
            self.editing_cloud_profile_id.and_then(|id| {
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
        match self.cloud_form_provider {
            CloudProviderChoice::Aws => {
                // Workload regions are shared across all AWS auth
                // kinds. Persist both the legacy `region` key (= first
                // entry) and the `regions` array so older builds keep
                // working. SSO has its own `sso_region` for the IdC
                // endpoint, unrelated.
                if let Some(first) = self.cloud_form_aws_regions.first() {
                    put(&mut obj, "region", first);
                }
                if !self.cloud_form_aws_regions.is_empty() {
                    let arr: Vec<serde_json::Value> = self
                        .cloud_form_aws_regions
                        .iter()
                        .map(|r| serde_json::Value::String(r.clone()))
                        .collect();
                    obj.insert("regions".into(), serde_json::Value::Array(arr));
                }
                match self.cloud_form_auth_kind {
                    CloudAuthChoice::Profile => {
                        put(&mut obj, "profile_name", &self.cloud_form_aws_profile_name);
                    }
                    CloudAuthChoice::AccessKey => {
                        put(&mut obj, "access_key_id", &self.cloud_form_aws_access_key_id);
                        put(
                            &mut obj,
                            "access_key_session_token",
                            &self.cloud_form_aws_access_key_session_token,
                        );
                    }
                    CloudAuthChoice::Sso => {
                        put(&mut obj, "sso_start_url", &self.cloud_form_aws_sso_start_url);
                        put(&mut obj, "sso_region", &self.cloud_form_aws_sso_region);
                        put(&mut obj, "sso_account_id", &self.cloud_form_aws_sso_account_id);
                        put(&mut obj, "sso_role_name", &self.cloud_form_aws_sso_role_name);
                    }
                    CloudAuthChoice::Kubeconfig => {
                        // Kubeconfig auth belongs to the K8s provider; under
                        // AWS it's an impossible combo, so write nothing.
                    }
                }
            }
            CloudProviderChoice::K8s => {
                // Both optional: a blank kubeconfig falls back to
                // kubectl's default file, a blank context to the
                // kubeconfig's current-context. `put` skips empties.
                put(&mut obj, "kubeconfig", &self.cloud_form_kubeconfig_path);
                put(&mut obj, "context", &self.cloud_form_context);
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
