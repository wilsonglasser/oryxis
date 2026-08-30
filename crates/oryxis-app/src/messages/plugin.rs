//! Plugin review / uninstall lifecycle, wrapped by [`crate::messages::Message::Plugin`]. Handled by `Oryxis::handle_plugins`.

#[derive(Debug, Clone)]
pub enum PluginMessage {
    /// Toggle the kebab menu on a plugin row (secondary actions:
    /// remove cached downloads, uninstall).
    ShowPluginMenu(String),
    /// Remove a provider's cached binaries.
    PluginUninstall(String),
    /// Confirmed from the uninstall dialog: actually remove the
    /// cached binaries (and the MCP launcher copy for `mcp`).
    PluginUninstallConfirmed(String),
}
