//! Connect-action handlers, the entry points that take a cloud
//! resource (an ECS task or an EC2 SSM session) and turn it into a
//! live PTY-backed terminal tab via the AWS session-manager-plugin.

use iced::Task;

use crate::app::{Message, Oryxis};

impl Oryxis {
    pub(super) fn handle_cloud_transports(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            Message::ConnectEcsExecTask { group_id, task_id, task_label } => {
                // Resolve the dynamic group + its cloud_query.
                let Some(group) = self.groups.iter().find(|g| g.id == group_id).cloned()
                else {
                    return Ok(Task::none());
                };
                let Some(query) = group.cloud_query.clone() else {
                    return Ok(Task::none());
                };
                let oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                    cluster,
                    service: _,
                    container,
                } = query.kind.clone()
                else {
                    // K8s tasks live behind a different transport
                    // (kubectl exec), silently ignore here.
                    return Ok(Task::none());
                };
                let Some(profile) = self.resolve_cloud_profile(query.profile_id) else {
                    tracing::warn!(
                        target = "oryxis::dispatch_cloud",
                        group_id = %group_id,
                        "ECS Exec abort: profile no longer exists"
                    );
                    return Ok(Task::none());
                };

                // Region comes from the *currently cached* dynamic
                // group resolve (the resolver tries every region
                // and returns whichever one had tasks). Fall back to
                // profile default if the cache is empty for some
                // reason, the ECS API call itself will reject if
                // the task isn't actually in that region.
                let region = self
                    .cloud_profiles
                    .iter()
                    .find(|p| p.id == query.profile_id)
                    .and_then(|p| {
                        oryxis_cloud_aws::auth::AwsConfigJson::parse(p)
                            .ok()
                            .and_then(|c| c.region.or_else(|| c.regions.first().cloned()))
                    })
                    .unwrap_or_default();

                // The interactive command we run inside the
                // container. Override priority: template's
                // initial_command (if set), else "/bin/sh".
                let command = query
                    .template
                    .initial_command
                    .clone()
                    .unwrap_or_else(|| "/bin/sh".into());

                tracing::info!(
                    target = "oryxis::dispatch_cloud",
                    %task_id,
                    %cluster,
                    %container,
                    %region,
                    "ECS Exec: starting session"
                );
                let task_label_for_msg = task_label;
                Ok(Task::perform(
                    async move {
                        oryxis_cloud_aws::ecs_exec::start_ecs_exec(
                            &profile,
                            &region,
                            &cluster,
                            &task_id,
                            &container,
                            &command,
                        )
                        .await
                        .map(Box::new)
                        .map_err(|e| e.to_string())
                    },
                    move |result| Message::EcsExecSessionReady {
                        task_label: task_label_for_msg.clone(),
                        result,
                    },
                ))
            }
            Message::SsmSessionReady { host_label, result } => {
                // Same payload + plugin pipeline as ECS Exec, so the
                // spawn helper is shared. Different label prefix on
                // the tab so the user can tell SSM-into-EC2 from
                // ECS-Exec-into-container apart.
                let session = match result {
                    Ok(s) => *s,
                    Err(msg) => {
                        tracing::error!(
                            target = "oryxis::dispatch_cloud",
                            %msg,
                            "SSM Session start failed"
                        );
                        self.show_error_dialog(
                            crate::i18n::t("ssm_start_failed_title").to_string(),
                            msg,
                        );
                        return Ok(Task::none());
                    }
                };
                let plugin_path =
                    match oryxis_cloud_aws::session_manager_plugin::find_plugin() {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                target = "oryxis::dispatch_cloud",
                                error = %e,
                                "session-manager-plugin missing, install it to use SSM Session"
                            );
                            self.show_plugin_missing_dialog();
                            return Ok(Task::none());
                        }
                    };
                let args = oryxis_cloud_aws::ecs_exec::plugin_invocation(&session);
                tracing::info!(
                    target = "oryxis::dispatch_cloud",
                    plugin = %plugin_path.display(),
                    arg_count = args.len(),
                    "SSM: spawning session-manager-plugin"
                );
                let tab_label = format!("SSM · {host_label}");
                Ok(self.spawn_plugin_tab(
                    &tab_label,
                    plugin_path.to_string_lossy().to_string(),
                    args,
                ))
            }
            Message::EcsExecSessionReady { task_label, result } => {
                let session = match result {
                    Ok(s) => *s,
                    Err(msg) => {
                        // Surface to the user as a modal, most common
                        // cases (ExecuteCommand permission missing,
                        // task not configured for ECS Exec, container
                        // not yet running) are actionable once the
                        // user can read the SDK error verbatim.
                        tracing::error!(
                            target = "oryxis::dispatch_cloud",
                            %msg,
                            "ECS Exec session start failed"
                        );
                        self.show_error_dialog(
                            crate::i18n::t("ecs_exec_start_failed_title").to_string(),
                            msg,
                        );
                        return Ok(Task::none());
                    }
                };

                // Locate the plugin binary or bail with a visible
                // dialog. The plugin is a system-level dependency the
                // user installs themselves, we just point at the AWS
                // docs install page.
                let plugin_path =
                    match oryxis_cloud_aws::session_manager_plugin::find_plugin() {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                target = "oryxis::dispatch_cloud",
                                error = %e,
                                "session-manager-plugin missing, install it to use ECS Exec"
                            );
                            self.show_plugin_missing_dialog();
                            return Ok(Task::none());
                        }
                    };

                let args = oryxis_cloud_aws::ecs_exec::plugin_invocation(&session);
                tracing::info!(
                    target = "oryxis::dispatch_cloud",
                    plugin = %plugin_path.display(),
                    arg_count = args.len(),
                    "ECS Exec: spawning session-manager-plugin"
                );
                let tab_label = format!("ECS · {task_label}");
                Ok(self.spawn_plugin_tab(
                    &tab_label,
                    plugin_path.to_string_lossy().to_string(),
                    args,
                ))
            }
            m => Err(m),
        }
    }
}
