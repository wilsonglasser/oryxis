//! AWS implementation of `CloudProvider`.
//!
//! Authentication strategies covered in v0.6:
//!
//! - `profile` — read a named profile from `~/.aws/config` /
//!   `~/.aws/credentials`. No secret stored in the vault.
//! - `access_key` — access key id + secret access key pasted into
//!   the wizard, secret stored encrypted in `cloud_profiles.secret`.
//! - `sso` — IAM Identity Center (formerly AWS SSO). Token cache
//!   reused from `~/.aws/sso/cache/` so `aws sso login` carries over.
//!
//! Discovery in this PR is EC2-only. ECS lands together with the SSM
//! transport in a follow-up PR.

pub mod auth;
pub mod ec2;
pub mod ecs;
pub mod provider;

pub use provider::AwsProvider;
