//! MCP server settings: enable, token, vault-password consent gate and config install, wrapped by [`crate::messages::Message::Mcp`]. Handled by `Oryxis::handle_mcp`.

#[derive(Debug, Clone)]
pub enum McpMessage {
    ToggleMcpServer,
    /// Flip the opt-in that lets the server add and edit hosts.
    ToggleMcpAllowWrites,
    ShowMcpInfo,
    HideMcpInfo,
    CopyMcpConfig,
    InstallMcpConfig,
    InstallMcpConfigResult(Result<String, String>),
    /// Pick which client the snippet, Copy, and Install target: the
    /// native client (`false`) or one running inside WSL (`true`).
    /// Only the Windows build renders the toggle that emits this, so
    /// elsewhere the variant is constructed nowhere.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    SetMcpTarget(bool),
    /// Pick which AI client the snippet, path hint, Copy and Install
    /// are for (the client row on the setup panel).
    SetMcpClient(crate::mcp_clients::McpClient),
    /// The detection task's answer: which clients are installed for
    /// the target it was asked about (`wsl`), plus the distro's marker
    /// folders when it was the WSL one. An answer for a target no
    /// longer selected is dropped.
    McpClientsDetected {
        wsl: bool,
        found: Vec<crate::mcp_clients::McpClient>,
        markers: Vec<String>,
    },
    /// Generate a fresh random MCP server token and persist it. Wipes
    /// the previous value, every existing MCP config will need to be
    /// reissued (re-copy / re-install) with the new token.
    RegenerateMcpToken,
    /// Show / hide the MCP token in plain text on the settings panel.
    /// Default masked.
    ToggleMcpTokenVisibility,
    /// Put the active MCP token on the clipboard. Logs nothing, the
    /// toast tells the user it was copied.
    CopyMcpToken,
    /// Open the master-password confirm row in the MCP setup panel
    /// (consent gate for embedding `ORYXIS_VAULT_PASSWORD` in the
    /// client config).
    McpVaultPwPromptOpen,
    /// Close the confirm row, discarding whatever was typed.
    McpVaultPwPromptCancel,
    /// Keystrokes into the master-password confirm input.
    McpVaultPwInput(String),
    /// Verify the typed master password; on success persist the
    /// consent flag so the snippet / Copy / Install embed the vault
    /// password from then on.
    McpVaultPwConfirm,
    /// Withdraw the consent: the snippet / Copy / Install stop
    /// embedding the vault password; the config(s) on disk are scrubbed
    /// in place in the background.
    McpVaultPwRemove,
    /// Outcome of that background scrub: `Ok(())` when every config that
    /// carried the password was rewritten without it, `Err(msg)` on a
    /// rewrite failure.
    McpVaultPwStripResult(Result<(), String>),
}
