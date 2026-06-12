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
            Message::PluginSessionEnded(pane_id) => {
                // The plugin process (session-manager-plugin / kubectl)
                // exited: stale task, idle timeout, remote close. The
                // pane used to just go silently dead; now the tab marks
                // itself disconnected, prints a notice, and (for tabs
                // that carry a relaunch message) re-arms the dormant
                // `pending_reopen` machinery so selecting the tab again
                // reconnects, same gesture as a pinned dormant tab.
                let Some(tidx) = self.tabs.iter().position(|t| {
                    t.pane_grid.panes.values().any(|p| p.id == pane_id)
                }) else {
                    // Tab already closed by the user; nothing to mark.
                    return Ok(Task::none());
                };
                // Derive the reopen spec from the relaunch message
                // before touching the label (pin_spec trims suffixes
                // itself, the order just keeps intent obvious).
                let spec = self.tabs[tidx].pin_spec();
                let tab = &mut self.tabs[tidx];
                let reconnectable =
                    tab.relaunch.is_some() && spec.is_some();
                let hint = if reconnectable {
                    crate::i18n::t("cloud_session_ended_hint")
                } else {
                    crate::i18n::t("cloud_session_ended")
                };
                if let Some(pane) =
                    tab.pane_grid.panes.values().find(|p| p.id == pane_id)
                    && let Ok(mut term) = pane.terminal.lock()
                {
                    let notice = format!("\r\n\x1b[2m  {}\x1b[0m\r\n", hint);
                    term.process(notice.as_bytes());
                }
                if !tab.label.ends_with(" (disconnected)") {
                    tab.label.push_str(" (disconnected)");
                }
                if reconnectable {
                    tab.pending_reopen = spec;
                }
                Ok(Task::none())
            }
            Message::ConnectEcsExecTask {
                group_id,
                task_id,
                task_label,
                container,
            } => {
                // When launched from the new-tab picker, close it and drop
                // any pending split-pane target. ECS Exec opens a full tab
                // via the session plugin, it can't fill an SSH-only split
                // pane, so leaving pending_pane_split set would misroute the
                // next SSH pick into a pane. No-op when the picker is closed.
                self.show_new_tab_picker = false;
                self.pending_pane_split = None;
                // Resolve the dynamic group + its cloud_query. These
                // used to fail silently, which left a reopened pinned
                // tab as a dead placeholder with no explanation when
                // its backing group had been deleted; surface a dialog
                // instead.
                let Some(group) = self.groups.iter().find(|g| g.id == group_id).cloned()
                else {
                    self.show_error_dialog(
                        crate::i18n::t("ecs_exec_start_failed_title").to_string(),
                        crate::i18n::t("ecs_exec_group_missing").to_string(),
                    );
                    return Ok(Task::none());
                };
                let Some(query) = group.cloud_query.clone() else {
                    self.show_error_dialog(
                        crate::i18n::t("ecs_exec_start_failed_title").to_string(),
                        crate::i18n::t("ecs_exec_group_missing").to_string(),
                    );
                    return Ok(Task::none());
                };
                let oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                    cluster,
                    service: _,
                    container: _,
                } = query.kind.clone()
                else {
                    // K8s tasks live behind a different transport
                    // (kubectl exec), silently ignore here.
                    return Ok(Task::none());
                };
                // Use the per-row container (the one the user
                // actually clicked) instead of the query's. Under
                // wildcard mode the query's is empty; under
                // single-container mode the two match anyway.
                let Some(profile) = self.resolve_cloud_profile(query.profile_id) else {
                    tracing::warn!(
                        target = "oryxis::dispatch_cloud",
                        group_id = %group_id,
                        "ECS Exec abort: profile no longer exists"
                    );
                    return Ok(Task::none());
                };

                let Some(provider) =
                    self.cloud_provider_registry.get(&profile.provider)
                else {
                    tracing::warn!(
                        target = "oryxis::dispatch_cloud",
                        provider = %profile.provider,
                        "ECS Exec abort: provider not registered"
                    );
                    return Ok(Task::none());
                };

                // Region comes from the profile's config. Fall back
                // to an empty string if it's absent, the ECS API
                // call itself rejects with a clear "region required".
                let region = self
                    .cloud_profiles
                    .iter()
                    .find(|p| p.id == query.profile_id)
                    .map(|p| super::region_from_profile_config(&p.config))
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
                // Cloned for the Ready message, which rebuilds a
                // ConnectEcsExecTask relaunch payload; the originals move
                // into the async start_ecs_exec call below.
                let task_id_for_msg = task_id.clone();
                let container_for_msg = container.clone();
                Ok(Task::perform(
                    async move {
                        provider
                            .start_ecs_exec(
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
                        group_id,
                        task_label: task_label_for_msg.clone(),
                        task_id: task_id_for_msg.clone(),
                        container: container_for_msg.clone(),
                        result,
                    },
                ))
            }
            Message::ConnectKubectlExecPod {
                group_id,
                namespace,
                pod,
                container,
            } => {
                // See ConnectEcsExecTask: close the picker and drop any
                // pending split-pane target, kubectl exec opens a full tab.
                self.show_new_tab_picker = false;
                self.pending_pane_split = None;
                let Some(group) = self.groups.iter().find(|g| g.id == group_id).cloned() else {
                    return Ok(Task::none());
                };
                let Some(query) = group.cloud_query.clone() else {
                    return Ok(Task::none());
                };
                let oryxis_core::models::cloud::CloudQueryKind::K8sPods { context, .. } =
                    query.kind.clone()
                else {
                    return Ok(Task::none());
                };
                let Some(profile) = self.resolve_cloud_profile(query.profile_id) else {
                    return Ok(Task::none());
                };

                // `kubectl` is required on PATH to open the shell (discovery
                // can run through the plugin, but the interactive session is
                // a local kubectl process). Guard with a friendly dialog
                // instead of a cryptic spawn failure.
                if !kubectl_on_path() {
                    self.show_error_dialog(
                        crate::i18n::t("k8s_kubectl_missing_title").to_string(),
                        crate::i18n::t("k8s_kubectl_missing_body").to_string(),
                    );
                    return Ok(Task::none());
                }

                // kubeconfig + context come from the profile config; the
                // group's captured context wins when set.
                let cfg: serde_json::Value =
                    serde_json::from_str(&profile.config).unwrap_or(serde_json::Value::Null);
                let kubeconfig = cfg
                    .get("kubeconfig")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let context = if !context.trim().is_empty() {
                    Some(context)
                } else {
                    cfg.get("context")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                };

                // The shell to run inside the pod: the template's
                // initial_command when set, else a login-ish shell that
                // prefers $SHELL and falls back to sh.
                let shell_cmd = query
                    .template
                    .initial_command
                    .clone()
                    .unwrap_or_else(|| "exec ${SHELL:-sh}".to_string());

                let args = build_kubectl_exec_args(
                    kubeconfig.as_deref(),
                    context.as_deref(),
                    &namespace,
                    &pod,
                    &container,
                    &shell_cmd,
                );

                let label = format!("k8s: {pod}");
                tracing::info!(
                    target = "oryxis::dispatch_cloud",
                    %namespace, %pod,
                    "kubectl exec: opening pod shell"
                );
                // Relaunch payload for Duplicate Tab: pod shells have no
                // saved Connection, so the tab carries its own re-open
                // message instead of being looked up by label.
                let relaunch = Message::ConnectKubectlExecPod {
                    group_id,
                    namespace,
                    pod,
                    container,
                };
                Ok(self.spawn_plugin_tab(&label, "kubectl".into(), args, Some(relaunch)))
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
                        if self.should_record_history()
                            && let Some(vault) = &self.vault {
                            let entry = oryxis_core::models::log_entry::LogEntry::new(
                                &host_label,
                                &host_label,
                                oryxis_core::models::log_entry::LogEvent::Error,
                                &msg,
                            );
                            let _ = vault.add_log(&entry);
                        }
                        self.show_error_dialog(
                            crate::i18n::t("ssm_start_failed_title").to_string(),
                            msg,
                        );
                        return Ok(Task::none());
                    }
                };
                let plugin_path =
                    match crate::session_manager_plugin::find_plugin() {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                target = "oryxis::dispatch_cloud",
                                error = %e,
                                "session-manager-plugin missing, install it to use SSM Session"
                            );
                            if self.should_record_history()
                                && let Some(vault) = &self.vault {
                                let entry = oryxis_core::models::log_entry::LogEntry::new(
                                    &host_label,
                                    &host_label,
                                    oryxis_core::models::log_entry::LogEvent::Error,
                                    &format!("session-manager-plugin missing: {e}"),
                                );
                                let _ = vault.add_log(&entry);
                            }
                            self.show_plugin_missing_dialog();
                            return Ok(Task::none());
                        }
                    };
                let args = oryxis_plugin_protocol::plugin_invocation(&session);
                tracing::info!(
                    target = "oryxis::dispatch_cloud",
                    plugin = %plugin_path.display(),
                    arg_count = args.len(),
                    "SSM: spawning session-manager-plugin"
                );
                let tab_label = format!("{}{host_label}", crate::app::SSM_TAB_PREFIX);
                // No relaunch payload: SSM tabs are backed by a saved
                // Connection, so Duplicate Tab re-finds it by label
                // (prefix stripped) and re-dispatches ConnectSsh, which
                // routes back here via the cloud_ref transport check.
                Ok(self.spawn_plugin_tab(
                    &tab_label,
                    plugin_path.to_string_lossy().to_string(),
                    args,
                    None,
                ))
            }
            Message::EcsExecSessionReady {
                group_id,
                task_label,
                task_id,
                container,
                result,
            } => {
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
                        if self.should_record_history()
                            && let Some(vault) = &self.vault {
                            let entry = oryxis_core::models::log_entry::LogEntry::new(
                                &task_label,
                                &task_label,
                                oryxis_core::models::log_entry::LogEvent::Error,
                                &msg,
                            );
                            let _ = vault.add_log(&entry);
                        }
                        self.show_error_dialog(
                            crate::i18n::t("ecs_exec_start_failed_title").to_string(),
                            msg,
                        );
                        // A failed connect usually means the cached task
                        // recycled out from under us (the clicked task_id
                        // no longer exists). Re-resolve the group so the
                        // live task replaces the dead row, sparing the
                        // user the manual Refresh click before retrying.
                        return Ok(self
                            .handle_cloud(Message::DynamicGroupResolve(group_id))
                            .unwrap_or_else(|_| Task::none()));
                    }
                };

                // Locate the plugin binary or bail with a visible
                // dialog. The plugin is a system-level dependency the
                // user installs themselves, we just point at the AWS
                // docs install page.
                let plugin_path =
                    match crate::session_manager_plugin::find_plugin() {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                target = "oryxis::dispatch_cloud",
                                error = %e,
                                "session-manager-plugin missing, install it to use ECS Exec"
                            );
                            if self.should_record_history()
                                && let Some(vault) = &self.vault {
                                let entry = oryxis_core::models::log_entry::LogEntry::new(
                                    &task_label,
                                    &task_label,
                                    oryxis_core::models::log_entry::LogEvent::Error,
                                    &format!("session-manager-plugin missing: {e}"),
                                );
                                let _ = vault.add_log(&entry);
                            }
                            self.show_plugin_missing_dialog();
                            return Ok(Task::none());
                        }
                    };

                let args = oryxis_plugin_protocol::plugin_invocation(&session);
                tracing::info!(
                    target = "oryxis::dispatch_cloud",
                    plugin = %plugin_path.display(),
                    arg_count = args.len(),
                    "ECS Exec: spawning session-manager-plugin"
                );
                // Tab title: prefer the human name (service, falling
                // back to container) over the bare task id, which is an
                // opaque hex string truncated to uselessness in a tab.
                // A short task-id suffix keeps two tasks of the same
                // service distinguishable.
                let human = self
                    .groups
                    .iter()
                    .find(|g| g.id == group_id)
                    .and_then(|g| g.cloud_query.as_ref())
                    .and_then(|q| match &q.kind {
                        oryxis_core::models::cloud::CloudQueryKind::EcsTasks {
                            service,
                            ..
                        } if !service.is_empty() => Some(service.clone()),
                        _ => None,
                    })
                    .or_else(|| {
                        (!container.is_empty()).then(|| container.clone())
                    });
                let tab_label = match human {
                    Some(name) => {
                        let short: String = task_label.chars().take(8).collect();
                        format!("ECS · {name} ({short})")
                    }
                    None => format!("ECS · {task_label}"),
                };
                // Relaunch payload for Duplicate Tab: ECS tasks have no
                // saved Connection, so the tab carries the message that
                // re-opens an exec session into the same task/container.
                let relaunch = Message::ConnectEcsExecTask {
                    group_id,
                    task_id,
                    task_label,
                    container,
                };
                Ok(self.spawn_plugin_tab(
                    &tab_label,
                    plugin_path.to_string_lossy().to_string(),
                    args,
                    Some(relaunch),
                ))
            }
            m => Err(m),
        }
    }
}

/// Whether a `kubectl` executable is resolvable on PATH. Used to fail a
/// pod-shell launch with a friendly dialog instead of a cryptic spawn
/// error when the user hasn't installed kubectl.
fn kubectl_on_path() -> bool {
    let exe = if cfg!(windows) { "kubectl.exe" } else { "kubectl" };
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|p| p.join(exe).is_file()))
        .unwrap_or(false)
}

/// Build the `kubectl exec -it` argument vector for a pod shell. Persistent
/// `--kubeconfig` / `--context` flags come first (before the subcommand,
/// where kubectl expects them), then `exec -it -n NS POD`, an optional
/// `-c CONTAINER`, and the `-- sh -c <shell_cmd>` tail. Pure + tested.
fn build_kubectl_exec_args(
    kubeconfig: Option<&str>,
    context: Option<&str>,
    namespace: &str,
    pod: &str,
    container: &str,
    shell_cmd: &str,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if let Some(kc) = kubeconfig.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--kubeconfig".into());
        args.push(kc.to_string());
    }
    if let Some(ctx) = context.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("--context".into());
        args.push(ctx.to_string());
    }
    args.push("exec".into());
    args.push("-it".into());
    args.push("-n".into());
    args.push(namespace.to_string());
    args.push(pod.to_string());
    if !container.trim().is_empty() {
        args.push("-c".into());
        args.push(container.to_string());
    }
    args.push("--".into());
    args.push("sh".into());
    args.push("-c".into());
    args.push(shell_cmd.to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::build_kubectl_exec_args;

    #[test]
    fn minimal_args_omit_flags_and_container() {
        let a = build_kubectl_exec_args(None, None, "default", "pod-1", "", "exec ${SHELL:-sh}");
        assert_eq!(
            a,
            vec!["exec", "-it", "-n", "default", "pod-1", "--", "sh", "-c", "exec ${SHELL:-sh}"]
        );
    }

    #[test]
    fn kubeconfig_and_context_precede_the_subcommand() {
        let a = build_kubectl_exec_args(
            Some("/tmp/kc"),
            Some("prod"),
            "ns",
            "pod-1",
            "",
            "sh",
        );
        // Global flags must come before `exec`, else kubectl rejects them.
        assert_eq!(&a[..4], &["--kubeconfig", "/tmp/kc", "--context", "prod"]);
        assert_eq!(a[4], "exec");
    }

    #[test]
    fn container_adds_dash_c_before_the_separator() {
        let a = build_kubectl_exec_args(None, None, "ns", "pod-1", "sidecar", "sh");
        let pos = a.iter().position(|s| s == "--").unwrap();
        assert_eq!(&a[pos - 2..pos], &["-c", "sidecar"]);
    }

    #[test]
    fn blank_flags_are_skipped() {
        let a = build_kubectl_exec_args(Some("  "), Some(""), "ns", "p", "  ", "sh");
        // No kubeconfig/context flags: the subcommand leads.
        assert_eq!(a[0], "exec");
        // Blank container: the `--` separator sits right after the pod, no
        // container `-c` is inserted. (The `sh -c` tail still carries a
        // `-c`, which is why we check the separator position, not `any -c`.)
        let sep = a.iter().position(|s| s == "--").unwrap();
        assert_eq!(a[sep - 1], "p");
    }
}
