//! MCP (Model Context Protocol) setup helpers, command path resolution, config
//! JSON generation, and installation into Claude Code's user-scope config
//! (`~/.claude.json`). The setup info panel that renders these lives in
//! `views/settings/mcp.rs`.

use crate::mcp_install;

/// Binary command external MCP clients (Claude Desktop / Code,
/// Cursor) should spawn. Resolves to the stable launcher path the
/// plugin install layer maintains (`~/.oryxis/bin/oryxis-mcp[.exe]`),
/// so the JSON snippet the user copies stays valid across plugin
/// updates. Falls back to the launcher path even when no plugin is
/// installed yet, the install flow gates the surface, so the user
/// shouldn't see this snippet with a missing binary.
pub(crate) fn mcp_binary_command() -> String {
    mcp_install::launcher_path()
        .map(|p| {
            if cfg!(target_os = "windows") {
                // JSON in the snippet needs `\\` to escape backslashes
                // when rendered into a `command` string. The display
                // form embeds them as-is; the JSON builder doubles
                // them.
                p.display().to_string()
            } else {
                p.display().to_string()
            }
        })
        .unwrap_or_else(|_| "oryxis-mcp".to_string())
}

/// WSL-side path for Windows users whose AI client runs inside WSL.
/// Translates the Windows launcher path (`C:\Users\<user>\.oryxis\bin\
/// oryxis-mcp.exe`) into its WSL mount equivalent
/// (`/mnt/c/Users/<user>/.oryxis/bin/oryxis-mcp.exe`). Returns an
/// empty string when `USERPROFILE` isn't available; the WSL block in
/// the info panel only renders on Windows, where it always is.
pub(crate) fn mcp_wsl_command() -> String {
    // The launcher path is computed against `dirs::home_dir`, which
    // reads `USERPROFILE` on Windows. We post-process the result into
    // the WSL form rather than going through `USERPROFILE` again so
    // both helpers stay in lockstep.
    let Ok(path) = mcp_install::launcher_path() else {
        return String::new();
    };
    let s = path.to_string_lossy();
    // Drive-letter form: `C:\Users\...` -> `/mnt/c/Users/...`.
    if let Some(rest) = s.strip_prefix("C:\\").or_else(|| s.strip_prefix("c:\\")) {
        return format!("/mnt/c/{}", rest.replace('\\', "/"));
    }
    // Any other layout (network share, non-C drive) is too unusual to
    // guess at; fall back to the bare Windows path so the user can fix
    // it by hand.
    s.into_owned()
}

/// JSON entry for the `oryxis` MCP server: the `command` path plus
/// the optional `env` block carrying the auth token and, when the
/// user confirmed their master password in the setup panel, the
/// `ORYXIS_VAULT_PASSWORD` a password-protected vault needs (without
/// it the server exits at startup and the client reports a failed
/// connection). Shared between the copy-to-clipboard snippet and the
/// on-disk merge so escaping stays consistent on Windows.
fn oryxis_mcp_entry(cmd: &str, token: &str, vault_pw: Option<&str>) -> serde_json::Value {
    let mut env = serde_json::Map::new();
    if !token.is_empty() {
        env.insert("ORYXIS_MCP_TOKEN".into(), serde_json::json!(token));
    }
    if let Some(pw) = vault_pw {
        env.insert("ORYXIS_VAULT_PASSWORD".into(), serde_json::json!(pw));
    }
    if env.is_empty() {
        serde_json::json!({ "command": cmd })
    } else {
        serde_json::json!({ "command": cmd, "env": env })
    }
}

/// Escape a value for a `set VAR=value` segment inside a `cmd /c`
/// string: cmd metacharacters are neutralized with `^`. Two characters
/// have no reliable escape in this position and stay as-is: `%`
/// (cmd expands `%VAR%` even inside quotes) and `"` (the WSL argv ->
/// Windows command-line translation rewrites embedded quotes to `\"`,
/// which cmd rejects). A vault password containing those can't ride
/// the WSL wrapper; the native env block handles every character.
fn cmd_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '^' | '&' | '|' | '<' | '>' | '(' | ')') {
            out.push('^');
        }
        out.push(c);
    }
    out
}

/// MCP entry for an AI client running *inside* WSL on a Windows host.
///
/// A Windows process spawned from WSL inherits the *Linux* environment,
/// and WSL does not forward custom variables (`ORYXIS_MCP_TOKEN`)
/// across the boundary. A plain `env` block on a `/mnt/c/...exe` entry
/// therefore never reaches `oryxis-mcp.exe`: the token gate sees an
/// empty token and rejects every call with "token mismatch". (The
/// binary still resolves the correct Windows vault via the interop
/// user's profile, so the token is the only thing missing.)
///
/// When a token is set we launch through `cmd.exe`, which rebuilds the
/// Windows environment, and inject the token Windows-side with `set`
/// before invoking the binary. `cmd.exe`'s UNC-cwd warning lands on
/// stderr, so the JSON-RPC stream on stdout stays clean. `win_exe` is
/// the native `C:\...exe` path (cmd is a Windows program), emitted
/// unquoted: the WSL argv -> Windows command-line translation rewrites
/// embedded quotes to `\"`, which cmd rejects. A username with a space
/// in `C:\Users\<name>` is the one unsupported case; standard profile
/// folders have none.
///
/// With neither a token nor a vault password, the plain
/// `/mnt/c/...exe` launch already works, so we keep `wsl_exe` for that
/// path. The vault password rides the same cmd.exe wrapper as the
/// token (an `env` block would never cross the WSL boundary either),
/// escaped for cmd; see [`cmd_escape`] for the two characters that
/// can't be carried.
fn oryxis_mcp_entry_wsl(
    wsl_exe: &str,
    win_exe: &str,
    token: &str,
    vault_pw: Option<&str>,
) -> serde_json::Value {
    if token.is_empty() && vault_pw.is_none() {
        return serde_json::json!({ "command": wsl_exe });
    }
    // cmd.exe lives under the Windows root; the WSL mount is the only
    // path the Linux-side client can exec it through.
    const CMD: &str = "/mnt/c/Windows/System32/cmd.exe";
    let mut inner = String::new();
    if !token.is_empty() {
        inner.push_str(&format!("set ORYXIS_MCP_TOKEN={token}&& "));
    }
    if let Some(pw) = vault_pw {
        inner.push_str(&format!("set ORYXIS_VAULT_PASSWORD={}&& ", cmd_escape(pw)));
    }
    inner.push_str(win_exe);
    serde_json::json!({
        "command": CMD,
        "args": ["/c", inner],
    })
}

/// The JSON snippet users need to copy. When `token` is non-empty
/// the snippet includes an `env` block that passes
/// `ORYXIS_MCP_TOKEN` to the spawned MCP server; the server refuses
/// every call when the token mismatches the value stored in the
/// vault. Empty token keeps the legacy unauth path. `vault_pw` is the
/// master password of a password-protected vault, embedded only after
/// the user confirmed it in the setup panel.
pub(crate) fn mcp_config_json(token: &str, vault_pw: Option<&str>) -> String {
    let cmd = mcp_binary_command();
    let root = serde_json::json!({
        "mcpServers": {
            "oryxis": oryxis_mcp_entry(&cmd, token, vault_pw),
        }
    });
    serde_json::to_string_pretty(&root).unwrap_or_else(|_| String::from("{}"))
}

/// Same as [`mcp_config_json`] but for an AI client (Claude Code,
/// Cursor) running *inside* a WSL distro on a Windows host. See
/// [`oryxis_mcp_entry_wsl`] for the cmd.exe wrapper that carries the
/// token and vault password across the WSL -> Windows boundary; the
/// Windows app produces this so the user doesn't have to assemble it
/// by hand.
pub(crate) fn mcp_config_json_wsl(token: &str, vault_pw: Option<&str>) -> String {
    let entry = oryxis_mcp_entry_wsl(&mcp_wsl_command(), &mcp_binary_command(), token, vault_pw);
    let root = serde_json::json!({
        "mcpServers": {
            "oryxis": entry,
        }
    });
    serde_json::to_string_pretty(&root).unwrap_or_else(|_| String::from("{}"))
}

/// Home directory resolved the way external clients see it.
fn home_dir_for_config() -> Result<std::path::PathBuf, String> {
    let home_str = if cfg!(target_os = "windows") {
        std::env::var("USERPROFILE").map_err(|_| "USERPROFILE not set")?
    } else {
        std::env::var("HOME").map_err(|_| "HOME not set")?
    };
    Ok(std::path::PathBuf::from(home_str))
}

/// Path this app wrote MCP config to before it was corrected: Claude
/// Code never reads `~/.claude/.mcp.json` (only `~/.claude.json` and a
/// project-root `.mcp.json`), so entries installed there were dead.
/// Kept only so installs can sweep the stale `oryxis` entry (issue #72).
fn legacy_mcp_config_path() -> Result<std::path::PathBuf, String> {
    Ok(home_dir_for_config()?.join(".claude").join(".mcp.json"))
}

/// Where Claude Code reads its user-scope config from. Claude Code
/// relocates `.claude.json` into `$CLAUDE_CONFIG_DIR` when that
/// variable is set, so honor it; the plain home profile is the
/// default. Best-effort: a GUI launch may not carry a shell-only
/// export, in which case the default path is what Claude Code
/// launched the same way would read anyway.
fn claude_code_config_path() -> Result<std::path::PathBuf, String> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Ok(std::path::PathBuf::from(dir).join(".claude.json"));
        }
    }
    Ok(home_dir_for_config()?.join(".claude.json"))
}

/// Merge the given oryxis server entry into a parsed config root,
/// creating the `mcpServers` object when absent. Every unrelated key
/// in the file is preserved untouched.
fn merge_oryxis_entry(
    root: &mut serde_json::Map<String, serde_json::Value>,
    entry: serde_json::Value,
) -> Result<(), String> {
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let servers_map = servers
        .as_object_mut()
        .ok_or("mcpServers is not an object")?;
    servers_map.insert("oryxis".to_string(), entry);
    Ok(())
}

/// Remove the `oryxis` entry from a parsed config root. Returns
/// whether anything was actually removed.
fn strip_oryxis_entry(root: &mut serde_json::Map<String, serde_json::Value>) -> bool {
    root.get_mut("mcpServers")
        .and_then(|s| s.as_object_mut())
        .map(|m| m.remove("oryxis").is_some())
        .unwrap_or(false)
}

/// Remove the `oryxis` entry from the legacy dead-letter config, if
/// present, so it can't mislead anyone debugging their MCP setup.
/// Best-effort: unparsable or missing files are left alone.
fn sweep_legacy_mcp_config() {
    let Ok(path) = legacy_mcp_config_path() else { return };
    let Ok(content) = std::fs::read_to_string(&path) else { return };
    let Ok(mut root) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content)
    else {
        return;
    };
    if strip_oryxis_entry(&mut root)
        && let Ok(output) = serde_json::to_string_pretty(&root)
    {
        let _ = std::fs::write(&path, output);
    }
}


/// Whether the ACTIVE Claude Code config (`~/.claude.json` only, never
/// the legacy dead-letter) currently carries an `oryxis` MCP entry. The
/// vault-password removal gates on this so it rewrites a live config in
/// place and can never promote a dead legacy entry into an active one,
/// nor create a config where none existed.
fn active_config_has_oryxis() -> bool {
    claude_code_config_path().is_ok_and(|p| {
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .map(|v| v.get("mcpServers").and_then(|s| s.get("oryxis")).is_some())
            .unwrap_or(false)
    })
}

/// Whether the WSL distro's `~/.claude.json` carries an `oryxis` entry.
/// Same gate as [`active_config_has_oryxis`] for the WSL target, so a
/// removal never creates a config inside a distro that never had one.
#[cfg(target_os = "windows")]
fn wsl_config_has_oryxis() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(out) = Command::new("wsl.exe")
        .args(["--", "bash", "-c", "cat ~/.claude.json 2>/dev/null || true"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<serde_json::Value>(text.trim())
        .ok()
        .map(|v| v.get("mcpServers").and_then(|s| s.get("oryxis")).is_some())
        .unwrap_or(false)
}

/// Scrub the embedded `ORYXIS_VAULT_PASSWORD` from every Claude Code
/// config that actually carries an `oryxis` entry, in place, WITHOUT
/// creating one anywhere. Covers both the native config and (on Windows)
/// the WSL distro's config, because the password may have been installed
/// into either regardless of the currently selected target. Blocking
/// I/O; call from a background task. `Ok(())` means nothing failed; the
/// caller surfaces any `Err` so a "revoked" claim never hides a
/// plaintext credential still on disk.
pub(crate) fn strip_vault_password_everywhere(token: &str) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    if active_config_has_oryxis()
        && let Err(e) = install_mcp_config_to_file(token, None)
    {
        errors.push(e);
    }
    #[cfg(target_os = "windows")]
    if wsl_config_has_oryxis()
        && let Err(e) = install_mcp_config_to_wsl(token, None)
    {
        errors.push(e);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Write/merge the oryxis MCP entry into Claude Code's user-scope
/// config, `~/.claude.json` (top-level `mcpServers` key, the same
/// place `claude mcp add -s user` writes). Claude Code does NOT read
/// `~/.claude/.mcp.json`, the path earlier releases wrote to; any
/// stale `oryxis` entry there is swept as part of the install.
/// Threads `token` and the opt-in vault password through so the
/// on-disk config always carries whatever the current settings hold
/// (a `None` password strips a previously installed one).
pub(crate) fn install_mcp_config_to_file(
    token: &str,
    vault_pw: Option<&str>,
) -> Result<String, String> {
    let config_path = claude_code_config_path()?;

    // `~/.claude.json` is Claude Code's main state file: merge into it,
    // never replace it. A parse failure aborts rather than clobbering
    // whatever Claude Code has stored there.
    let mut root: serde_json::Map<String, serde_json::Value> = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read {}: {e}", config_path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {e}", config_path.display()))?
    } else {
        serde_json::Map::new()
    };

    let cmd = mcp_binary_command();
    merge_oryxis_entry(&mut root, oryxis_mcp_entry(&cmd, token, vault_pw))?;

    let output = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize: {e}"))?;
    write_config_private(&config_path, &output)?;

    sweep_legacy_mcp_config();

    Ok(config_path.display().to_string())
}

/// Write `~/.claude.json` with owner-only permissions (0600) on Unix.
/// The opt-in vault password embed puts a plaintext credential in this
/// file, so it must never be left world-readable under the default
/// umask (0644). On non-Unix the ACL story differs and there is no
/// mode bit to set, so this is a plain write there.
fn write_config_private(path: &std::path::Path, contents: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        // `mode` only bites on creation; tighten an existing looser file too.
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = file.set_permissions(perms);
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))
    }
}

/// Write/merge the oryxis MCP entry into the WSL distro's
/// `~/.claude.json` (Claude Code's user-scope config), for a Claude
/// Code instance running inside WSL on a Windows host. Shells out to
/// `wsl.exe` (default distro): reads the current config, merges in
/// Rust so the JSON stays well-formed, and writes the result back
/// through stdin so the payload never has to survive shell quoting.
/// The entry shape comes from [`oryxis_mcp_entry_wsl`] (cmd.exe
/// wrapper when a token is set). The legacy dead-letter
/// `~/.claude/.mcp.json` gets its `oryxis` entry swept, mirroring the
/// native install.
///
/// Only meaningful on Windows; returns an error elsewhere, where there
/// is no `wsl.exe` to talk to.
pub(crate) fn install_mcp_config_to_wsl(
    token: &str,
    vault_pw: Option<&str>,
) -> Result<String, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (token, vault_pw);
        Err("WSL install is only available on the Windows build".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        // CREATE_NO_WINDOW keeps wsl.exe from flashing a console over
        // the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // Read the current config (empty when the file is absent). A
        // non-login bash keeps rc-file noise out of stdout while still
        // expanding `~` via HOME. The trailing `|| true` keeps the exit
        // code at 0 when the file doesn't exist yet (first install),
        // otherwise `cat`'s failure would look like a WSL error.
        let read = Command::new("wsl.exe")
            .args(["--", "bash", "-c", "cat ~/.claude.json 2>/dev/null || true"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Could not run wsl.exe ({e}). Is WSL installed?"))?;
        if !read.status.success() {
            let err = String::from_utf8_lossy(&read.stderr);
            return Err(format!("wsl.exe failed: {}", err.trim()));
        }

        let existing = String::from_utf8_lossy(&read.stdout);
        // `~/.claude.json` is Claude Code's main state file: merge into
        // it, never replace it. A parse failure aborts rather than
        // clobbering whatever Claude Code has stored there.
        let mut root: serde_json::Map<String, serde_json::Value> = if existing.trim().is_empty() {
            serde_json::Map::new()
        } else {
            serde_json::from_str(existing.trim())
                .map_err(|e| format!("Failed to parse WSL ~/.claude.json: {e}"))?
        };

        let entry =
            oryxis_mcp_entry_wsl(&mcp_wsl_command(), &mcp_binary_command(), token, vault_pw);
        merge_oryxis_entry(&mut root, entry)?;

        let output =
            serde_json::to_string_pretty(&root).map_err(|e| format!("Failed to serialize: {e}"))?;

        // Pipe the merged JSON back through stdin so it never has to be
        // escaped into a shell argument. `umask 077` births the temp 0600,
        // then an atomic `mv` swaps it in: the opt-in vault password embed
        // writes a plaintext credential, so the real config must never be
        // left world-readable nor partially written (a chmod-after-write
        // would leave a loose window, permanently if `cat` died mid-pipe).
        let mut child = Command::new("wsl.exe")
            .args([
                "--",
                "bash",
                "-c",
                "umask 077 && cat > ~/.claude.json.tmp && mv ~/.claude.json.tmp ~/.claude.json",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Could not run wsl.exe ({e})."))?;
        child
            .stdin
            .take()
            .ok_or("failed to open wsl.exe stdin")?
            .write_all(output.as_bytes())
            .map_err(|e| format!("Failed to write to WSL: {e}"))?;
        let status = child
            .wait()
            .map_err(|e| format!("wsl.exe did not finish: {e}"))?;
        if !status.success() {
            return Err("wsl.exe could not write ~/.claude.json".to_string());
        }

        // Sweep a stale `oryxis` entry out of the dead-letter path this
        // app used to write inside the distro. Best-effort, jq-free:
        // read, strip in Rust, write back only when something changed.
        let legacy = Command::new("wsl.exe")
            .args(["--", "bash", "-c", "cat ~/.claude/.mcp.json 2>/dev/null || true"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        if let Ok(legacy) = legacy {
            let content = String::from_utf8_lossy(&legacy.stdout);
            if let Ok(mut root) =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(content.trim())
                && strip_oryxis_entry(&mut root)
                && let Ok(stripped) = serde_json::to_string_pretty(&root)
                && let Ok(mut child) = Command::new("wsl.exe")
                    .args(["--", "bash", "-c", "cat > ~/.claude/.mcp.json"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(stripped.as_bytes());
                }
                let _ = child.wait();
            }
        }

        Ok("~/.claude.json (WSL)".to_string())
    }
}

impl crate::app::Oryxis {
    /// The vault master password to embed in MCP client configs, or
    /// `None` when the user hasn't opted in via the setup panel (or
    /// the vault has no password). Read fresh from `master_password`
    /// at every use: the password is never copied into MCP state, so
    /// a vault lock naturally revokes access to it.
    pub(crate) fn mcp_vault_pw(&self) -> Option<String> {
        (self.mcp.include_vault_password && self.vault_ui.has_user_password)
            .then(|| self.master_password.clone())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cmd_escape, merge_oryxis_entry, oryxis_mcp_entry, oryxis_mcp_entry_wsl,
        strip_oryxis_entry,
    };

    const WSL: &str = "/mnt/c/Users/wilso/.oryxis/bin/oryxis-mcp.exe";
    const WIN: &str = "C:\\Users\\wilso\\.oryxis\\bin\\oryxis-mcp.exe";

    // With a token, the WSL entry must launch through cmd.exe and inject
    // the token Windows-side. A plain `env` block is wrong here: WSL does
    // not forward env vars to a spawned Windows process, so the binary
    // would see an empty token and reject every call.
    #[test]
    fn wsl_entry_with_token_wraps_through_cmd() {
        let v = oryxis_mcp_entry_wsl(WSL, WIN, "deadbeef", None);
        assert_eq!(v["command"], "/mnt/c/Windows/System32/cmd.exe");
        let args = v["args"].as_array().expect("args array");
        assert_eq!(args[0], "/c");
        assert_eq!(args[1], format!("set ORYXIS_MCP_TOKEN=deadbeef&& {WIN}"));
        // The token must not also leak into an env block that never arrives.
        assert!(v.get("env").is_none());
    }

    // No token means auth is off; the direct /mnt/c/...exe launch already
    // works, so no cmd.exe wrapper is emitted.
    #[test]
    fn wsl_entry_without_token_stays_direct() {
        let v = oryxis_mcp_entry_wsl(WSL, WIN, "", None);
        assert_eq!(v["command"], WSL);
        assert!(v.get("args").is_none());
        assert!(v.get("env").is_none());
    }

    // The vault password rides the same cmd.exe wrapper as the token
    // (env blocks never cross the WSL boundary), chained with `set`,
    // and cmd metacharacters in the password are ^-escaped.
    #[test]
    fn wsl_entry_with_vault_password_chains_sets() {
        let v = oryxis_mcp_entry_wsl(WSL, WIN, "deadbeef", Some("p&ss|word"));
        let args = v["args"].as_array().expect("args array");
        assert_eq!(
            args[1],
            format!(
                "set ORYXIS_MCP_TOKEN=deadbeef&& set ORYXIS_VAULT_PASSWORD=p^&ss^|word&& {WIN}"
            )
        );
    }

    // Password without a token still needs the wrapper: the direct
    // launch has no way to carry the env var across the boundary.
    #[test]
    fn wsl_entry_with_only_vault_password_wraps_through_cmd() {
        let v = oryxis_mcp_entry_wsl(WSL, WIN, "", Some("hunter2"));
        assert_eq!(v["command"], "/mnt/c/Windows/System32/cmd.exe");
        let args = v["args"].as_array().expect("args array");
        assert_eq!(args[1], format!("set ORYXIS_VAULT_PASSWORD=hunter2&& {WIN}"));
    }

    #[test]
    fn cmd_escape_neutralizes_metacharacters() {
        assert_eq!(cmd_escape("a&b|c<d>e^f(g)h"), "a^&b^|c^<d^>e^^f^(g^)h");
        assert_eq!(cmd_escape("plain"), "plain");
    }

    // The native entry carries the vault password in the env block next
    // to the token; without either the env block is omitted entirely.
    #[test]
    fn native_entry_env_block_shapes() {
        let v = oryxis_mcp_entry("oryxis-mcp", "tok", Some("pw"));
        assert_eq!(v["env"]["ORYXIS_MCP_TOKEN"], "tok");
        assert_eq!(v["env"]["ORYXIS_VAULT_PASSWORD"], "pw");

        let v = oryxis_mcp_entry("oryxis-mcp", "", Some("pw"));
        assert!(v["env"].get("ORYXIS_MCP_TOKEN").is_none());
        assert_eq!(v["env"]["ORYXIS_VAULT_PASSWORD"], "pw");

        let v = oryxis_mcp_entry("oryxis-mcp", "", None);
        assert!(v.get("env").is_none());
    }

    // `~/.claude.json` is Claude Code's main state file: the install
    // merge must leave every unrelated key (and sibling MCP servers)
    // untouched while inserting/replacing only the `oryxis` entry.
    #[test]
    fn merge_preserves_unrelated_config() {
        let mut root: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{
                "numStartups": 42,
                "projects": {"/home/u/proj": {"allowedTools": []}},
                "mcpServers": {"other": {"command": "other-mcp"}, "oryxis": {"command": "stale"}}
            }"#,
        )
        .unwrap();
        merge_oryxis_entry(&mut root, serde_json::json!({"command": "fresh"})).unwrap();
        assert_eq!(root["numStartups"], 42);
        assert!(root["projects"]["/home/u/proj"].is_object());
        assert_eq!(root["mcpServers"]["other"]["command"], "other-mcp");
        assert_eq!(root["mcpServers"]["oryxis"]["command"], "fresh");
    }

    #[test]
    fn merge_creates_servers_object_when_absent() {
        let mut root = serde_json::Map::new();
        merge_oryxis_entry(&mut root, serde_json::json!({"command": "fresh"})).unwrap();
        assert_eq!(root["mcpServers"]["oryxis"]["command"], "fresh");
    }

    // The legacy-file sweep must remove only the `oryxis` entry and
    // report whether anything changed (an unchanged file is not
    // rewritten).
    #[test]
    fn strip_removes_only_oryxis() {
        let mut root: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{"mcpServers": {"other": {"command": "other-mcp"}, "oryxis": {"command": "x"}}}"#,
        )
        .unwrap();
        assert!(strip_oryxis_entry(&mut root));
        assert!(root["mcpServers"].get("oryxis").is_none());
        assert_eq!(root["mcpServers"]["other"]["command"], "other-mcp");
        // Second pass: nothing left to remove.
        assert!(!strip_oryxis_entry(&mut root));
    }
}
