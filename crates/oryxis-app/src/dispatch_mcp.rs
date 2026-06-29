//! `Oryxis::handle_mcp`: settings-panel-independent dispatch arms for the
//! mcp area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;

use crate::app::{Message, Oryxis};
use crate::mcp::{install_mcp_config_to_file, install_mcp_config_to_wsl, mcp_config_json, mcp_config_json_wsl};
use crate::state::{EnvVarForm, PortForwardForm};

impl Oryxis {
    pub(crate) fn handle_mcp(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // ── MCP ──
            Message::EditorToggleMcpEnabled => {
                self.editor_form.mcp_enabled = !self.editor_form.mcp_enabled;
            }
            Message::EditorToggleAgentForwarding => {
                self.editor_form.agent_forwarding = !self.editor_form.agent_forwarding;
            }
            // Cycle the per-host recording override: Default (inherit the
            // global setting) -> On -> Off -> Default.
            Message::EditorCycleSessionLogging => {
                self.editor_form.session_logging = match self.editor_form.session_logging {
                    None => Some(true),
                    Some(true) => Some(false),
                    Some(false) => None,
                };
            }
            Message::EditorAddPortForward => {
                self.editor_form.port_forwards.push(PortForwardForm::default());
            }
            Message::EditorRemovePortForward(i) => {
                if i < self.editor_form.port_forwards.len() {
                    self.editor_form.port_forwards.remove(i);
                }
            }
            Message::EditorPortFwdLocalPortChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.local_port = v;
                }
            }
            Message::EditorPortFwdRemoteHostChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.remote_host = v;
                }
            }
            Message::EditorPortFwdRemotePortChanged(i, v) => {
                if let Some(pf) = self.editor_form.port_forwards.get_mut(i) {
                    pf.remote_port = v;
                }
            }
            Message::EditorAddEnvVar => {
                self.editor_form.env_vars.push(EnvVarForm::default());
            }
            Message::EditorRemoveEnvVar(i) => {
                if i < self.editor_form.env_vars.len() {
                    self.editor_form.env_vars.remove(i);
                }
            }
            Message::EditorEnvVarKeyChanged(i, v) => {
                if let Some(e) = self.editor_form.env_vars.get_mut(i) {
                    e.key = v;
                }
            }
            Message::EditorEnvVarValueChanged(i, v) => {
                if let Some(e) = self.editor_form.env_vars.get_mut(i) {
                    e.value = v;
                }
            }
            Message::ToggleMcpServer => {
                self.mcp.server_enabled = !self.mcp.server_enabled;
                if let Some(vault) = &self.vault {
                    let _ = vault.set_setting("mcp_server_enabled", if self.mcp.server_enabled { "true" } else { "false" });
                }
                // MCP ships as a plugin (~5 MB binary external clients
                // like Claude Desktop spawn). First-time enable triggers
                // the install modal; an already-installed plugin or a
                // dev binary on the side both make this a no-op.
                if self.mcp.server_enabled
                    && !crate::mcp_install::is_installed()
                    && !crate::dispatch_plugins::dev_binary_present("mcp")
                {
                    return Ok(Task::done(Message::ShowPluginInstallModal(
                        "mcp".to_string(),
                    )));
                }
            }
            Message::ShowMcpInfo => {
                self.mcp.show_info = true;
                self.mcp.config_copied = false;
            }
            Message::HideMcpInfo => {
                self.mcp.show_info = false;
                self.mcp.config_copied = false;
            }
            Message::CopyMcpConfig => {
                self.mcp.config_copied = true;
                let json = if self.mcp.target_wsl {
                    mcp_config_json_wsl(&self.mcp.server_token)
                } else {
                    mcp_config_json(&self.mcp.server_token)
                };
                return Ok(iced::clipboard::write(json).discard());
            }
            Message::InstallMcpConfig => {
                self.mcp.install_status = None;
                let token = self.mcp.server_token.clone();
                let wsl = self.mcp.target_wsl;
                return Ok(Task::perform(
                    async move {
                        if wsl {
                            install_mcp_config_to_wsl(&token)
                        } else {
                            install_mcp_config_to_file(&token)
                        }
                    },
                    Message::InstallMcpConfigResult,
                ));
            }
            Message::SetMcpTarget(is_wsl) => {
                self.mcp.target_wsl = is_wsl;
                // The Copy / Install feedback from the previous target no
                // longer reflects what's on screen.
                self.mcp.config_copied = false;
                self.mcp.install_status = None;
            }
            Message::InstallMcpConfigResult(result) => {
                self.mcp.install_status = Some(result);
            }
            Message::RegenerateMcpToken => {
                use rand::RngCore;
                let mut bytes = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut bytes);
                let mut token = String::with_capacity(64);
                for b in bytes {
                    use std::fmt::Write as _;
                    let _ = write!(token, "{b:02x}");
                }
                self.persist_setting("mcp_server_token", &token);
                self.mcp.server_token = token;
                // Reveal once after regenerating so the user can copy
                // it without an extra click; flip it back to masked
                // explicitly via `ToggleMcpTokenVisibility`.
                self.mcp.token_visible = true;
                // The Claude config on disk still carries the old
                // token, prompt the user to re-install.
                self.mcp.install_status = None;
            }
            Message::ToggleMcpTokenVisibility => {
                self.mcp.token_visible = !self.mcp.token_visible;
            }
            Message::CopyMcpToken => {
                return Ok(iced::clipboard::write(self.mcp.server_token.clone()).discard());
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
