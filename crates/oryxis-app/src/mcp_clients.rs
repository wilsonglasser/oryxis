//! The AI clients the MCP setup panel can write `oryxis-mcp` into: one
//! table of where each keeps its MCP servers, how that file is shaped,
//! and what on disk says the client is installed at all.
//!
//! Every JSON client here stores its servers under a top-level
//! `mcpServers` object, some inside a larger settings file (Claude
//! Code's `~/.claude.json`, Gemini CLI's `settings.json`), so one
//! merge-in-place writer serves them all. Codex is the odd one out: a
//! TOML `config.toml` with a `[mcp_servers.<name>]` table per server,
//! edited through `toml_edit` so the user's other tables and comments
//! survive the write. The paths come from each client's own
//! documentation (checked 2026-10-09); a client is only ever WRITTEN
//! when the directory it would read from already exists, which is also
//! the detection, so an unverified path can at worst stay unwritten.

use std::path::{Path, PathBuf};

/// An MCP client the setup panel knows how to configure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum McpClient {
    /// Claude Code (CLI). User scope lives in `~/.claude.json`, relocated
    /// by `CLAUDE_CONFIG_DIR` when set.
    ClaudeCode,
    /// Claude Desktop. `claude_desktop_config.json` under the per-OS app
    /// data folder; the app does not exist on Linux.
    ClaudeDesktop,
    /// OpenAI Codex (CLI and IDE extension share it). `~/.codex/config.toml`,
    /// `[mcp_servers.<name>]` tables; `CODEX_HOME` relocates the folder.
    Codex,
    /// Cursor, global scope: `~/.cursor/mcp.json`.
    Cursor,
    /// Gemini CLI, user scope: the `mcpServers` object inside
    /// `~/.gemini/settings.json`.
    GeminiCli,
    /// GitHub Copilot's portable config (`~/.copilot/mcp-config.json`,
    /// `COPILOT_HOME` relocates it), the location VS Code's "Add
    /// Server" flow now recommends over the deprecated profile file.
    Copilot,
    /// Windsurf's Cascade agent (now Devin Desktop): the legacy
    /// `~/.codeium/windsurf/mcp_config.json`, or the current
    /// `~/.config/devin/mcp_config.json` (`%APPDATA%\devin` on Windows),
    /// whichever folder exists.
    Windsurf,
}

/// How a client's config file is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigFormat {
    /// JSON with a top-level `mcpServers` object (merged in place).
    JsonMcpServers,
    /// TOML with `[mcp_servers.<name>]` tables (edited in place).
    TomlMcpServers,
}

impl Default for McpClient {
    /// The panel opens on Claude Code, the client the install has
    /// always targeted.
    fn default() -> Self {
        McpClient::ClaudeCode
    }
}

impl McpClient {
    /// Every client, in the order the panel lists them.
    pub(crate) const ALL: [McpClient; 7] = [
        McpClient::ClaudeCode,
        McpClient::ClaudeDesktop,
        McpClient::Codex,
        McpClient::Cursor,
        McpClient::GeminiCli,
        McpClient::Copilot,
        McpClient::Windsurf,
    ];

    /// The clients that can exist on this OS (native target).
    pub(crate) fn available() -> Vec<McpClient> {
        Self::ALL
            .into_iter()
            .filter(|c| c.exists_on_this_os())
            .collect()
    }

    /// The clients that can live inside a WSL distro: everything but
    /// the desktop app, which is a Windows program.
    pub(crate) fn available_in_wsl() -> Vec<McpClient> {
        Self::ALL
            .into_iter()
            .filter(|c| *c != McpClient::ClaudeDesktop)
            .collect()
    }

    fn exists_on_this_os(self) -> bool {
        self != McpClient::ClaudeDesktop || cfg!(any(target_os = "windows", target_os = "macos"))
    }

    /// Product name, a proper noun that is not translated.
    pub(crate) fn name(self) -> &'static str {
        match self {
            McpClient::ClaudeCode => "Claude Code",
            McpClient::ClaudeDesktop => "Claude Desktop",
            McpClient::Codex => "Codex",
            McpClient::Cursor => "Cursor",
            McpClient::GeminiCli => "Gemini CLI",
            McpClient::Copilot => "Copilot",
            McpClient::Windsurf => "Windsurf",
        }
    }

    pub(crate) fn format(self) -> ConfigFormat {
        match self {
            McpClient::Codex => ConfigFormat::TomlMcpServers,
            _ => ConfigFormat::JsonMcpServers,
        }
    }

    /// Folders (relative to the home directory) whose presence says the
    /// client is installed. The first one that exists is also the one
    /// the config path is built under, for clients that moved. These
    /// are the paths a WSL distro is asked about too, so they stay
    /// POSIX-relative; the Windows-only app data folders are handled by
    /// [`Self::config_path`] directly.
    pub(crate) fn marker_dirs(self) -> &'static [&'static str] {
        match self {
            McpClient::ClaudeCode => &[".claude"],
            McpClient::ClaudeDesktop => &[],
            McpClient::Codex => &[".codex"],
            McpClient::Cursor => &[".cursor"],
            McpClient::GeminiCli => &[".gemini"],
            McpClient::Copilot => &[".copilot"],
            McpClient::Windsurf => &[".codeium/windsurf", ".config/devin"],
        }
    }

    /// Whether the client is installed for the user whose home is
    /// `home` (native target). Environment overrides count: a relocated
    /// Codex or Copilot home is where that client lives.
    pub(crate) fn detected(self, home: &Path) -> bool {
        match self {
            McpClient::ClaudeCode => {
                env_dir("CLAUDE_CONFIG_DIR").is_some_and(|d| d.exists())
                    || home.join(".claude").exists()
                    || home.join(".claude.json").exists()
            }
            McpClient::ClaudeDesktop => {
                desktop_app_dir(home).is_some_and(|d| d.exists())
            }
            McpClient::Codex => {
                env_dir("CODEX_HOME").is_some_and(|d| d.exists()) || home.join(".codex").exists()
            }
            McpClient::Copilot => {
                env_dir("COPILOT_HOME").is_some_and(|d| d.exists())
                    || home.join(".copilot").exists()
            }
            McpClient::Windsurf => {
                self.marker_dirs().iter().any(|d| home.join(d).exists())
                    || windows_appdata().is_some_and(|a| a.join("devin").exists())
            }
            _ => self.marker_dirs().iter().any(|d| home.join(d).exists()),
        }
    }

    /// The file this client reads its MCP servers from, for the user
    /// whose home is `home`. A relocated client (env override) wins; a
    /// client that moved folders gets the one that exists, else its
    /// legacy default.
    pub(crate) fn config_path(self, home: &Path) -> Option<PathBuf> {
        match self {
            McpClient::ClaudeCode => Some(
                env_dir("CLAUDE_CONFIG_DIR")
                    .unwrap_or_else(|| home.to_path_buf())
                    .join(".claude.json"),
            ),
            McpClient::ClaudeDesktop => {
                desktop_app_dir(home).map(|d| d.join("claude_desktop_config.json"))
            }
            McpClient::Codex => Some(
                env_dir("CODEX_HOME")
                    .unwrap_or_else(|| home.join(".codex"))
                    .join("config.toml"),
            ),
            McpClient::Cursor => Some(home.join(".cursor").join("mcp.json")),
            McpClient::GeminiCli => Some(home.join(".gemini").join("settings.json")),
            McpClient::Copilot => Some(
                env_dir("COPILOT_HOME")
                    .unwrap_or_else(|| home.join(".copilot"))
                    .join("mcp-config.json"),
            ),
            McpClient::Windsurf => {
                if let Some(appdata) = windows_appdata()
                    && appdata.join("devin").exists()
                {
                    return Some(appdata.join("devin").join("mcp_config.json"));
                }
                let dir = self
                    .marker_dirs()
                    .iter()
                    .map(|d| home.join(d))
                    .find(|d| d.exists())
                    .unwrap_or_else(|| home.join(".codeium").join("windsurf"));
                Some(dir.join("mcp_config.json"))
            }
        }
    }

    /// The same file as a `~`-relative POSIX path, for a client living
    /// inside a WSL distro (read and written through `wsl.exe`, where
    /// `$HOME` is the distro's). `existing` is the distro's marker
    /// folders that were found, so a moved client lands in the folder
    /// it actually uses. `None` for a client that cannot run in a
    /// distro.
    pub(crate) fn wsl_config_path(self, existing: &[String]) -> Option<String> {
        match self {
            McpClient::ClaudeCode => Some("~/.claude.json".to_string()),
            McpClient::ClaudeDesktop => None,
            McpClient::Codex => Some("~/.codex/config.toml".to_string()),
            McpClient::Cursor => Some("~/.cursor/mcp.json".to_string()),
            McpClient::GeminiCli => Some("~/.gemini/settings.json".to_string()),
            McpClient::Copilot => Some("~/.copilot/mcp-config.json".to_string()),
            McpClient::Windsurf => {
                let dir = self
                    .marker_dirs()
                    .iter()
                    .find(|d| existing.iter().any(|e| e == *d))
                    .copied()
                    .unwrap_or(".codeium/windsurf");
                Some(format!("~/{dir}/mcp_config.json"))
            }
        }
    }

    /// Whether a distro reporting `existing` marker folders has this
    /// client. Claude Code also counts its state file, which exists
    /// before the folder does on a fresh install.
    pub(crate) fn detected_in_wsl(self, existing: &[String]) -> bool {
        if self == McpClient::ClaudeCode && existing.iter().any(|e| e == ".claude.json") {
            return true;
        }
        self.marker_dirs()
            .iter()
            .any(|d| existing.iter().any(|e| e == *d))
    }

    /// Every marker a distro is asked about, in one spawn: the folders
    /// of every client plus Claude Code's state file.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn wsl_markers() -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Self::available_in_wsl()
            .into_iter()
            .flat_map(|c| c.marker_dirs().iter().copied())
            .collect();
        out.push(".claude.json");
        out
    }
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// `%APPDATA%` on Windows, nothing elsewhere.
fn windows_appdata() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env_dir("APPDATA")
    } else {
        None
    }
}

/// Claude Desktop's app data folder: `%APPDATA%\Claude` on Windows,
/// `~/Library/Application Support/Claude` on macOS, none on Linux.
fn desktop_app_dir(home: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        windows_appdata().map(|a| a.join("Claude"))
    } else if cfg!(target_os = "macos") {
        Some(home.join("Library").join("Application Support").join("Claude"))
    } else {
        None
    }
}

/// The `oryxis` server entry rendered as the client's whole snippet:
/// what the panel shows and the clipboard carries.
pub(crate) fn render_snippet(format: ConfigFormat, entry: &serde_json::Value) -> String {
    match format {
        ConfigFormat::JsonMcpServers => {
            let root = serde_json::json!({ "mcpServers": { "oryxis": entry } });
            serde_json::to_string_pretty(&root).unwrap_or_else(|_| String::from("{}"))
        }
        ConfigFormat::TomlMcpServers => {
            let mut doc = toml_edit::DocumentMut::new();
            // A fresh document has no `mcp_servers` to be the wrong shape.
            let _ = set_toml_entry(&mut doc, entry);
            doc.to_string()
        }
    }
}

/// Merge the `oryxis` entry into a config file's text, preserving
/// everything else in it. An empty text is a file that does not exist
/// yet. A parse failure is an error, never a clobber.
pub(crate) fn merge_into(
    format: ConfigFormat,
    existing: &str,
    entry: serde_json::Value,
) -> Result<String, String> {
    match format {
        ConfigFormat::JsonMcpServers => {
            let mut root: serde_json::Map<String, serde_json::Value> = if existing.trim().is_empty()
            {
                serde_json::Map::new()
            } else {
                serde_json::from_str(existing).map_err(|e| format!("Failed to parse: {e}"))?
            };
            let servers = root
                .entry("mcpServers")
                .or_insert_with(|| serde_json::json!({}));
            let servers_map = servers
                .as_object_mut()
                .ok_or("mcpServers is not an object")?;
            servers_map.insert("oryxis".to_string(), entry);
            serde_json::to_string_pretty(&root).map_err(|e| format!("Failed to serialize: {e}"))
        }
        ConfigFormat::TomlMcpServers => {
            let mut doc: toml_edit::DocumentMut = if existing.trim().is_empty() {
                toml_edit::DocumentMut::new()
            } else {
                existing
                    .parse()
                    .map_err(|e| format!("Failed to parse: {e}"))?
            };
            set_toml_entry(&mut doc, &entry)?;
            Ok(doc.to_string())
        }
    }
}

/// Whether a config file's text carries an `oryxis` entry.
pub(crate) fn has_entry(format: ConfigFormat, text: &str) -> bool {
    match format {
        ConfigFormat::JsonMcpServers => serde_json::from_str::<serde_json::Value>(text.trim())
            .ok()
            .map(|v| v.get("mcpServers").and_then(|s| s.get("oryxis")).is_some())
            .unwrap_or(false),
        ConfigFormat::TomlMcpServers => text
            .parse::<toml_edit::DocumentMut>()
            .ok()
            .map(|d| {
                d.get("mcp_servers")
                    .and_then(|s| s.as_table_like())
                    .is_some_and(|t| t.contains_key("oryxis"))
            })
            .unwrap_or(false),
    }
}

/// Remove the `oryxis` entry from a config file's text. `None` when
/// there was nothing to remove (or the text does not parse).
pub(crate) fn strip_entry(format: ConfigFormat, text: &str) -> Option<String> {
    match format {
        ConfigFormat::JsonMcpServers => {
            let mut root: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(text.trim()).ok()?;
            let removed = root
                .get_mut("mcpServers")
                .and_then(|s| s.as_object_mut())
                .map(|m| m.remove("oryxis").is_some())
                .unwrap_or(false);
            removed.then(|| serde_json::to_string_pretty(&root).ok()).flatten()
        }
        ConfigFormat::TomlMcpServers => {
            let mut doc: toml_edit::DocumentMut = text.parse().ok()?;
            let removed = doc
                .get_mut("mcp_servers")
                .and_then(|s| s.as_table_like_mut())
                .map(|t| t.remove("oryxis").is_some())
                .unwrap_or(false);
            removed.then(|| doc.to_string())
        }
    }
}

/// Write the entry as the `[mcp_servers.oryxis]` table, replacing any
/// previous one WHOLE: a stripped `env` must not survive as a stale
/// sub-table carrying the old token or the vault password. An
/// `mcp_servers` that is not a table (a value, an array of tables) is
/// an error, never a silent no-op reported as installed.
fn set_toml_entry(doc: &mut toml_edit::DocumentMut, entry: &serde_json::Value) -> Result<(), String> {
    let mut table = toml_edit::Table::new();
    table.set_implicit(false);
    if let Some(cmd) = entry.get("command").and_then(|v| v.as_str()) {
        table["command"] = toml_edit::value(cmd);
    }
    if let Some(args) = entry.get("args").and_then(|v| v.as_array()) {
        let mut arr = toml_edit::Array::new();
        for a in args.iter().filter_map(|a| a.as_str()) {
            arr.push(a);
        }
        table["args"] = toml_edit::value(arr);
    }
    if let Some(env) = entry.get("env").and_then(|v| v.as_object()) {
        let mut env_table = toml_edit::Table::new();
        for (k, v) in env {
            if let Some(s) = v.as_str() {
                env_table[k] = toml_edit::value(s);
            }
        }
        table["env"] = toml_edit::Item::Table(env_table);
    }
    let servers = doc
        .entry("mcp_servers")
        .or_insert(toml_edit::Item::Table({
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            t
        }));
    let servers = servers
        .as_table_like_mut()
        .ok_or("mcp_servers is not a table")?;
    servers.insert("oryxis", toml_edit::Item::Table(table));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> serde_json::Value {
        serde_json::json!({
            "command": "/home/u/.oryxis/bin/oryxis-mcp",
            "env": { "ORYXIS_MCP_TOKEN": "abc" }
        })
    }

    #[test]
    fn json_clients_merge_under_mcp_servers_and_keep_the_rest() {
        let existing = r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#;
        let out = merge_into(ConfigFormat::JsonMcpServers, existing, entry()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["oryxis"]["env"]["ORYXIS_MCP_TOKEN"], "abc");
        assert!(has_entry(ConfigFormat::JsonMcpServers, &out));
    }

    #[test]
    fn an_empty_file_is_a_fresh_config() {
        let out = merge_into(ConfigFormat::JsonMcpServers, "", entry()).unwrap();
        assert!(has_entry(ConfigFormat::JsonMcpServers, &out));
        let out = merge_into(ConfigFormat::TomlMcpServers, "", entry()).unwrap();
        assert!(has_entry(ConfigFormat::TomlMcpServers, &out));
    }

    #[test]
    fn a_broken_file_is_refused_not_clobbered() {
        assert!(merge_into(ConfigFormat::JsonMcpServers, "{not json", entry()).is_err());
        assert!(merge_into(ConfigFormat::TomlMcpServers, "[broken", entry()).is_err());
        // The right file with the wrong shape is refused too, never a
        // silent no-op reported as installed.
        assert!(merge_into(ConfigFormat::TomlMcpServers, "mcp_servers = 3\n", entry()).is_err());
        assert!(merge_into(ConfigFormat::JsonMcpServers, r#"{"mcpServers": 3}"#, entry()).is_err());
        // Codex's own inline-table spelling is the same table to us.
        let out = merge_into(
            ConfigFormat::TomlMcpServers,
            "mcp_servers = { other = { command = \"x\" } }\n",
            entry(),
        )
        .unwrap();
        assert!(has_entry(ConfigFormat::TomlMcpServers, &out), "{out}");
    }

    #[test]
    fn codex_toml_keeps_other_tables_and_comments() {
        let existing = "# my config\nmodel = \"o3\"\n\n[mcp_servers.context7]\ncommand = \"npx\"\nargs = [\"-y\", \"ctx\"]\n";
        let out = merge_into(ConfigFormat::TomlMcpServers, existing, entry()).unwrap();
        assert!(out.starts_with("# my config"));
        assert!(out.contains("model = \"o3\""));
        assert!(out.contains("[mcp_servers.context7]"));
        assert!(out.contains("[mcp_servers.oryxis]"));
        assert!(out.contains("command = \"/home/u/.oryxis/bin/oryxis-mcp\""));
        assert!(out.contains("[mcp_servers.oryxis.env]"));
        assert!(out.contains("ORYXIS_MCP_TOKEN = \"abc\""));
        // It parses back as the shape Codex reads.
        let doc: toml_edit::DocumentMut = out.parse().unwrap();
        assert_eq!(
            doc["mcp_servers"]["oryxis"]["env"]["ORYXIS_MCP_TOKEN"].as_str(),
            Some("abc")
        );
    }

    #[test]
    fn a_rewrite_without_env_drops_the_old_env_table() {
        let with = merge_into(ConfigFormat::TomlMcpServers, "", entry()).unwrap();
        let bare = serde_json::json!({ "command": "/home/u/.oryxis/bin/oryxis-mcp" });
        let out = merge_into(ConfigFormat::TomlMcpServers, &with, bare).unwrap();
        assert!(!out.contains("ORYXIS_MCP_TOKEN"), "{out}");
        assert!(has_entry(ConfigFormat::TomlMcpServers, &out));
    }

    #[test]
    fn strip_removes_only_oryxis_in_both_formats() {
        let json = merge_into(
            ConfigFormat::JsonMcpServers,
            r#"{"mcpServers":{"other":{"command":"x"}}}"#,
            entry(),
        )
        .unwrap();
        let stripped = strip_entry(ConfigFormat::JsonMcpServers, &json).unwrap();
        assert!(!has_entry(ConfigFormat::JsonMcpServers, &stripped));
        assert!(stripped.contains("\"other\""));
        assert!(strip_entry(ConfigFormat::JsonMcpServers, &stripped).is_none());

        let toml = merge_into(
            ConfigFormat::TomlMcpServers,
            "[mcp_servers.other]\ncommand = \"x\"\n",
            entry(),
        )
        .unwrap();
        let stripped = strip_entry(ConfigFormat::TomlMcpServers, &toml).unwrap();
        assert!(!has_entry(ConfigFormat::TomlMcpServers, &stripped));
        assert!(stripped.contains("[mcp_servers.other]"));
        assert!(strip_entry(ConfigFormat::TomlMcpServers, &stripped).is_none());
    }

    #[test]
    fn snippets_take_the_client_shape() {
        let json = render_snippet(ConfigFormat::JsonMcpServers, &entry());
        assert!(json.contains("\"mcpServers\""));
        let toml = render_snippet(ConfigFormat::TomlMcpServers, &entry());
        assert!(toml.contains("[mcp_servers.oryxis]"));
        assert!(!toml.contains("mcpServers"));
    }

    #[test]
    fn detection_is_the_folder_the_client_reads_from() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        assert!(!McpClient::Cursor.detected(home));
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        assert!(McpClient::Cursor.detected(home));
        assert_eq!(
            McpClient::Cursor.config_path(home),
            Some(home.join(".cursor").join("mcp.json"))
        );
        // Windsurf: the folder that exists decides the file.
        std::fs::create_dir_all(home.join(".config").join("devin")).unwrap();
        assert!(McpClient::Windsurf.detected(home));
        assert_eq!(
            McpClient::Windsurf.config_path(home),
            Some(home.join(".config").join("devin").join("mcp_config.json"))
        );
    }

    #[test]
    fn wsl_detection_reads_the_distro_markers() {
        let found = vec![".codex".to_string(), ".claude.json".to_string()];
        assert!(McpClient::Codex.detected_in_wsl(&found));
        assert!(McpClient::ClaudeCode.detected_in_wsl(&found));
        assert!(!McpClient::Cursor.detected_in_wsl(&found));
        assert_eq!(
            McpClient::Windsurf.wsl_config_path(&[".config/devin".to_string()]),
            Some("~/.config/devin/mcp_config.json".to_string())
        );
        assert_eq!(McpClient::ClaudeDesktop.wsl_config_path(&[]), None);
        assert!(McpClient::wsl_markers().contains(&".claude.json"));
    }
}
