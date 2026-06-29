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
}
