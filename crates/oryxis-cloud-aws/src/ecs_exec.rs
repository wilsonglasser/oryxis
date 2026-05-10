//! ECS Exec session orchestration.
//!
//! AWS's `aws ecs execute-command` doesn't talk SSH, it spawns the
//! `session-manager-plugin` binary and pipes stdio through. The plugin
//! handles the binary WebSocket protocol against the SSM endpoint
//! (~15k lines of Go, not something we want to reimplement). Our
//! wrapper here owns the *orchestration*: resolving the runtime-id,
//! calling `ExecuteCommand`, and formatting the 6-positional-arg
//! invocation the plugin expects.
//!
//! Caller responsibility: locate the plugin binary (see
//! `session_manager_plugin`), spawn it inside a PTY, route stdout to
//! the terminal widget. The plugin terminates when the remote shell
//! exits.

use aws_sdk_ecs::Client as EcsClient;
use oryxis_cloud::{CloudError, CloudProfile};

use crate::auth::{build_sdk_config, AwsConfigJson};

/// Everything `session-manager-plugin` needs to start an ECS Exec
/// session. Build with `start_ecs_exec`, hand to `plugin_invocation`
/// to format the actual subprocess argv.
#[derive(Debug, Clone)]
pub struct EcsExecSession {
    /// The full JSON object the plugin reads as its first positional
    /// arg, `{SessionId, StreamUrl, TokenValue}`. Already
    /// JSON-serialized; the plugin wants a string, not an object.
    pub session_json: String,
    /// AWS region the session lives in. Same region the SSM endpoint
    /// resolves to.
    pub region: String,
    /// AWS CLI profile name used when starting the session. The
    /// plugin echoes this back if it needs to refresh credentials.
    /// Pass an empty string when the profile uses env-var creds.
    pub profile_name: String,
    /// SSM `StartSession` request as JSON. Encodes the ECS-specific
    /// target string (`ecs:cluster_task_runtimeId`) and the document
    /// to run.
    pub start_session_request: String,
    /// Region-specific SSM endpoint URL. Plugin uses this for any
    /// follow-up control calls (e.g. terminate-session).
    pub endpoint: String,
}

/// Run the full ECS-Exec preamble: resolve the container runtime-id,
/// call `ExecuteCommand`, and assemble the plugin invocation payload.
///
/// Defaults to `/bin/sh -i` because that's the shell every base ECS
/// image carries (alpine / ubuntu-slim / al2-minimal). Caller can
/// override `command` to e.g. `bash -i` for richer shells when they
/// know the image has it.
pub async fn start_ecs_exec(
    profile: &CloudProfile,
    region: &str,
    cluster: &str,
    task_id: &str,
    container: &str,
    command: &str,
) -> Result<EcsExecSession, CloudError> {
    if region.is_empty() {
        return Err(CloudError::InvalidConfig(
            "ECS Exec needs the resource's region to be set on its CloudRef".into(),
        ));
    }
    let sdk = build_sdk_config(profile, Some(region), None).await?;
    let client = EcsClient::new(&sdk);

    // 1) DescribeTasks → find the container's `runtimeId`. SSM's
    //    target string for ECS Exec needs `cluster_taskId_runtimeId`,
    //    not the bare task arn.
    let task_resp = client
        .describe_tasks()
        .cluster(cluster)
        .tasks(task_id)
        .send()
        .await
        .map_err(|e| {
            CloudError::Upstream(format!(
                "ecs:DescribeTasks {region}/{cluster}/{task_id}: {e}"
            ))
        })?;
    let task = task_resp
        .tasks()
        .first()
        .ok_or_else(|| {
            CloudError::NotFound(format!("ECS task {task_id} in cluster {cluster}"))
        })?
        .clone();
    let runtime_id = task
        .containers()
        .iter()
        .find(|c| c.name() == Some(container))
        .and_then(|c| c.runtime_id().map(str::to_string))
        .ok_or_else(|| {
            CloudError::NotFound(format!(
                "container {container} in task {task_id} (or container has no runtimeId, task may still be PROVISIONING)"
            ))
        })?;

    // 2) ExecuteCommand → get the SSM session. Interactive=true is
    //    required for shell sessions; non-interactive is for one-off
    //    commands which we don't surface to the user.
    let cmd_resp = client
        .execute_command()
        .cluster(cluster)
        .task(task_id)
        .container(container)
        .command(command)
        .interactive(true)
        .send()
        .await
        .map_err(|e| {
            CloudError::Upstream(format!(
                "ecs:ExecuteCommand {region}/{cluster}/{task_id}/{container}: {e}"
            ))
        })?;

    let session = cmd_resp.session().ok_or_else(|| {
        CloudError::Upstream(
            "ecs:ExecuteCommand returned no Session, is ECS Exec enabled on this task?"
                .into(),
        )
    })?;
    let session_id = session.session_id().unwrap_or_default().to_string();
    let stream_url = session.stream_url().unwrap_or_default().to_string();
    let token_value = session.token_value().unwrap_or_default().to_string();

    // 3) Build the JSON blobs the plugin wants. The shape mirrors
    //    what the AWS CLI passes, keep field names exact.
    let session_json = serde_json::json!({
        "SessionId": session_id,
        "StreamUrl": stream_url,
        "TokenValue": token_value,
    })
    .to_string();
    let target = format!("ecs:{cluster}_{task_id}_{runtime_id}");
    let start_session_request = serde_json::json!({
        "Target": target,
        "DocumentName": "AWS-StartInteractiveCommand",
        "Parameters": { "command": [command] },
    })
    .to_string();

    // Profile name is used by the plugin only when refreshing creds
    // mid-session. AWS profile auth carries the name through; for
    // SSO / access-key auth the plugin doesn't actually need a
    // profile name because the session token's already short-lived.
    let profile_name = AwsConfigJson::parse(profile)?
        .profile_name
        .unwrap_or_default();
    let endpoint = format!("https://ssm.{region}.amazonaws.com");

    Ok(EcsExecSession {
        session_json,
        region: region.to_string(),
        profile_name,
        start_session_request,
        endpoint,
    })
}

/// Format the 6-positional-arg invocation the
/// `session-manager-plugin` binary expects. Pair with
/// `crate::session_manager_plugin::find_plugin` to resolve the binary
/// path.
pub fn plugin_invocation(session: &EcsExecSession) -> Vec<String> {
    vec![
        session.session_json.clone(),
        session.region.clone(),
        "StartSession".to_string(),
        session.profile_name.clone(),
        session.start_session_request.clone(),
        session.endpoint.clone(),
    ]
}
