//! AWS SSM Session Manager, connects to an EC2 instance directly
//! (no SSH layer). Used as the third EC2 transport alongside SSH and
//! Instance Connect.
//!
//! Same plugin runner as ECS Exec under the hood. The difference is
//! the *Target*: SSM accepts the bare instance id (`i-xxx`) instead of
//! ECS's `cluster_task_runtimeId` triple. The plugin pipeline is
//! otherwise identical, which is why this module is small.

use aws_sdk_ssm::Client as SsmClient;
use oryxis_cloud::{CloudError, CloudProfile};

use crate::auth::{build_sdk_config, AwsConfigJson};
use crate::ecs_exec::EcsExecSession;

/// Start an SSM Session against an EC2 instance and return the
/// payload `session-manager-plugin` needs to attach. Reuses
/// `EcsExecSession` as the carrier struct because the plugin
/// invocation format is identical regardless of whether the target
/// is an ECS task or an EC2 instance.
pub async fn start_ssm_session(
    profile: &CloudProfile,
    region: &str,
    instance_id: &str,
) -> Result<EcsExecSession, CloudError> {
    if region.is_empty() {
        return Err(CloudError::InvalidConfig(
            "SSM Session needs the resource's region to be set on its CloudRef".into(),
        ));
    }
    let sdk = build_sdk_config(profile, Some(region), None).await?;
    let client = SsmClient::new(&sdk);

    // SSM `StartSession` against the bare instance id. Document
    // defaults to `SSM-SessionManagerRunShell` (the AWS default) when
    // unset, gives the user an interactive shell as the SSM agent's
    // user (`ssm-user` on Linux).
    let resp = client
        .start_session()
        .target(instance_id)
        .send()
        .await
        .map_err(|e| {
            CloudError::Upstream(format!(
                "ssm:StartSession {region}/{instance_id}: {e}"
            ))
        })?;

    let session_id = resp.session_id().unwrap_or_default().to_string();
    let stream_url = resp.stream_url().unwrap_or_default().to_string();
    let token_value = resp.token_value().unwrap_or_default().to_string();

    let session_json = serde_json::json!({
        "SessionId": session_id,
        "StreamUrl": stream_url,
        "TokenValue": token_value,
    })
    .to_string();
    let start_session_request = serde_json::json!({
        "Target": instance_id,
    })
    .to_string();

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
