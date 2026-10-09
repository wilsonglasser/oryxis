//! MCP server feature state: the Settings → MCP panel toggles plus the
//! one-shot install/copy status. Grouped out of the `Oryxis` god-struct as
//! the first step of the modules-by-feature direction (field grouping; the
//! dispatch/view split is a separate, larger project).

/// All MCP-server UI + settings state. Persisted bits (`server_enabled`,
/// `server_token`) hydrate from the `settings` table on boot; the rest is
/// transient panel state.
#[derive(Debug, Clone, Default)]
pub(crate) struct McpState {
    /// Whether the bundled MCP server launcher is enabled.
    pub(crate) server_enabled: bool,
    /// Whether the MCP server may create and edit hosts (`create_host` /
    /// `update_host`). Persisted as `mcp_server_allow_writes`, off by
    /// default; the server reads it per call.
    pub(crate) allow_writes: bool,
    /// Whether the "how to connect" info block is expanded.
    pub(crate) show_info: bool,
    /// Latched true briefly after the user copies the client config snippet.
    pub(crate) config_copied: bool,
    /// Result of the last install attempt (`Ok(path)` / `Err(message)`).
    pub(crate) install_status: Option<Result<String, String>>,
    /// Token MCP clients must present (via `ORYXIS_MCP_TOKEN` env) to talk to
    /// the server. Empty disables auth (backward-compat).
    pub(crate) server_token: String,
    /// When true, the token is rendered as plain text in the panel; otherwise
    /// as a row of bullets. Kept masked by default, the user opts in to see it.
    pub(crate) token_visible: bool,
    /// Which client the setup snippet / Copy / Install target: the native
    /// client (`false`) or one running inside WSL (`true`). Only reachable on
    /// Windows, where the toggle that flips it renders.
    pub(crate) target_wsl: bool,
    /// Which AI client the snippet, the path hint, Copy and Install are
    /// for. Picked on the panel's client row; not persisted.
    pub(crate) client: crate::mcp_clients::McpClient,
    /// The clients found installed for the current target (native, or
    /// the WSL distro), filled by the detection task the panel kicks
    /// off when it opens or the target changes. Install is only offered
    /// for these; the others keep Copy.
    pub(crate) detected: Vec<crate::mcp_clients::McpClient>,
    /// The distro's marker folders the WSL detection found, which decide
    /// the config path of a client that moved folders (Windsurf).
    pub(crate) wsl_markers: Vec<String>,
    /// A detection task is in flight (the row says "checking" instead
    /// of "not detected" meanwhile).
    pub(crate) detecting: bool,
    /// Consent flag (persisted as the `mcp_config_vault_pw` setting): the
    /// user confirmed their master password and wants it embedded in the
    /// client config as `ORYXIS_VAULT_PASSWORD`. The password itself is
    /// never stored here; it is read from `Oryxis.master_password` at
    /// snippet/install time, so a vault lock naturally revokes access.
    pub(crate) include_vault_password: bool,
    /// Buffer of the master-password confirm input; `Some` while the
    /// confirm row is open. Swept on vault lock like every typed secret.
    pub(crate) vault_pw_prompt: Option<String>,
    /// The last confirm attempt failed (wrong password).
    pub(crate) vault_pw_error: bool,
    /// Outcome of the last "Remove" of the embedded vault password:
    /// `Ok(())` once the on-disk config(s) were scrubbed, `Err(msg)` if a
    /// rewrite failed (so the UI never claims a revoke that left the
    /// plaintext credential behind). Distinct from `install_status` so a
    /// removal never renders install-success text.
    pub(crate) vault_pw_strip_status: Option<Result<(), String>>,
}
