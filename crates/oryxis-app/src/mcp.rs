//! MCP (Model Context Protocol) setup helpers: command path resolution,
//! the config snippet in each client's shape, detection of the clients
//! installed here (and inside a WSL distro), and the merge-in-place
//! install into each client's own config. The client table itself is
//! `mcp_clients.rs`; the setup info panel that renders all this lives in
//! `views/settings/mcp.rs`.

use crate::mcp_clients::{self, McpClient};
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

/// The `oryxis` server entry for a client on this machine: the
/// launcher path plus the env block the token and the opt-in vault
/// password ride in.
fn native_entry(token: &str, vault_pw: Option<&str>) -> serde_json::Value {
    oryxis_mcp_entry(&mcp_binary_command(), token, vault_pw)
}

/// The entry for a client living inside a WSL distro: see
/// [`oryxis_mcp_entry_wsl`] for the cmd.exe wrapper that carries the
/// secrets across the boundary.
fn wsl_entry(token: &str, vault_pw: Option<&str>) -> serde_json::Value {
    oryxis_mcp_entry_wsl(&mcp_wsl_command(), &mcp_binary_command(), token, vault_pw)
}

/// The snippet users copy for `client`, in that client's own shape
/// (JSON under `mcpServers`, or Codex's TOML table). When `token` is
/// non-empty the entry carries an env block passing `ORYXIS_MCP_TOKEN`;
/// the server refuses every call when the token mismatches the value
/// stored in the vault. Empty token keeps the legacy unauth path.
/// `vault_pw` is the master password of a password-protected vault,
/// embedded only after the user confirmed it in the setup panel.
pub(crate) fn mcp_config_snippet(client: McpClient, token: &str, vault_pw: Option<&str>) -> String {
    mcp_clients::render_snippet(client.format(), &native_entry(token, vault_pw))
}

/// Same as [`mcp_config_snippet`] but for a client running *inside* a
/// WSL distro on a Windows host; the Windows app produces this so the
/// user doesn't have to assemble the cmd.exe wrapper by hand.
pub(crate) fn mcp_config_snippet_wsl(
    client: McpClient,
    token: &str,
    vault_pw: Option<&str>,
) -> String {
    mcp_clients::render_snippet(client.format(), &wsl_entry(token, vault_pw))
}

/// Cap on the token's bullet run, mirroring the token row so both
/// spellings of the same value line up.
const TOKEN_MASK_CAP: usize = 48;

/// Fixed bullet run for the embedded vault password. Fixed rather than
/// proportional because it is the user's master password: the row above
/// never shows it at all, and its length is not ours to publish.
const VAULT_PW_MASK_WIDTH: usize = 12;

/// Bullet run of `n` characters, the on-screen stand-in for a secret.
pub(crate) fn secret_mask(n: usize) -> String {
    "\u{2022}".repeat(n)
}

/// Mask for the MCP token, capped so a long value can't run the row off
/// the panel.
pub(crate) fn token_mask(token: &str) -> String {
    secret_mask(token.chars().count().min(TOKEN_MASK_CAP))
}

/// The snippet AS RENDERED in the setup panel: the same generators the
/// Copy and Install paths use, fed masked values while the panel is
/// hidden. Masking the token on its own row and then printing it in
/// full three lines below is a mask in name only, and the embedded
/// vault password had no reveal affordance at all. Copy and Install
/// rebuild from state, so what travels is always the real value.
///
/// Masking the INPUTS rather than the finished string is what keeps
/// this correct for every shape at once, the native `env` block, the
/// WSL `set VAR=...&&` argument and the TOML table, with no second
/// escaping opinion: a password carrying `"` or `\` is escaped by the
/// serializer on the way out, so a search-and-replace over the
/// finished text would silently miss it.
pub(crate) fn mcp_config_snippet_display(
    client: McpClient,
    token: &str,
    vault_pw: Option<&str>,
    wsl: bool,
    revealed: bool,
) -> String {
    // An unset token stays unset: bullets would fabricate an `env` block
    // for a server the user is deliberately running without auth.
    let token_shown = if revealed || token.is_empty() {
        token.to_string()
    } else {
        token_mask(token)
    };
    let pw_shown = vault_pw.map(|pw| {
        if revealed {
            pw.to_string()
        } else {
            secret_mask(VAULT_PW_MASK_WIDTH)
        }
    });
    if wsl {
        mcp_config_snippet_wsl(client, &token_shown, pw_shown.as_deref())
    } else {
        mcp_config_snippet(client, &token_shown, pw_shown.as_deref())
    }
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

/// Where `client` reads its MCP servers from on this machine. An
/// environment relocation (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
/// `COPILOT_HOME`) is honoured, best-effort: a GUI launch may not
/// carry a shell-only export, in which case the default path is what
/// the client launched the same way would read anyway.
fn client_config_path(client: McpClient) -> Result<std::path::PathBuf, String> {
    let home = home_dir_for_config()?;
    client
        .config_path(&home)
        .ok_or_else(|| format!("{} is not available on this platform", client.name()))
}

/// The on-screen hint for where `client`'s config lives: the resolved
/// native path, or the distro-relative one for the WSL target.
pub(crate) fn client_path_hint(client: McpClient, wsl: bool, wsl_markers: &[String]) -> String {
    if wsl {
        client
            .wsl_config_path(wsl_markers)
            .map(|p| format!("{p} (WSL)"))
            .unwrap_or_default()
    } else {
        client_config_path(client)
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }
}

/// Remove the `oryxis` entry from the legacy dead-letter config, if
/// present, so it can't mislead anyone debugging their MCP setup.
/// Best-effort: unparsable or missing files are left alone.
fn sweep_legacy_mcp_config() {
    let Ok(path) = legacy_mcp_config_path() else { return };
    let Ok(content) = std::fs::read_to_string(&path) else { return };
    if let Some(output) =
        mcp_clients::strip_entry(mcp_clients::ConfigFormat::JsonMcpServers, &content)
    {
        let _ = std::fs::write(&path, output);
    }
}

/// Whether `client`'s config on this machine currently carries an
/// `oryxis` entry: the user installed into it at some point, so a
/// plugin update or a vault-password revoke should rewrite it in place.
/// Never true for a file that does not exist, so a rewrite can never
/// create a config where none was.
pub(crate) fn client_has_entry(client: McpClient) -> bool {
    client_config_path(client).is_ok_and(|p| {
        std::fs::read_to_string(&p)
            .ok()
            .is_some_and(|c| mcp_clients::has_entry(client.format(), &c))
    })
}

/// Whether any client's config on this machine (or the legacy
/// dead-letter file) carries an `oryxis` entry, i.e. the user ran
/// Install at some point and a plugin update should refresh.
pub(crate) fn mcp_config_installed() -> bool {
    McpClient::available().into_iter().any(client_has_entry)
        || legacy_mcp_config_path().is_ok_and(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .is_some_and(|c| mcp_clients::has_entry(mcp_clients::ConfigFormat::JsonMcpServers, &c))
        })
}

/// The clients installed on this machine (native target). Blocking
/// (a few `stat`s); call from a task, never from `view()`.
pub(crate) fn detect_native_clients() -> Vec<McpClient> {
    let Ok(home) = home_dir_for_config() else {
        return Vec::new();
    };
    McpClient::available()
        .into_iter()
        .filter(|c| c.detected(&home))
        .collect()
}

/// `CREATE_NO_WINDOW`: keeps `wsl.exe` from flashing a console over
/// the app.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run `bash -c <script>` in the default WSL distro and return its
/// stdout. A non-login bash keeps rc-file noise out of stdout while
/// still expanding `~` via HOME.
#[cfg(target_os = "windows")]
fn wsl_bash(script: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let out = Command::new("wsl.exe")
        .args(["--", "bash", "-c", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Could not run wsl.exe ({e}). Is WSL installed?"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("wsl.exe failed: {}", err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Read a `~/...` file inside the distro; empty when absent (the
/// trailing `|| true` keeps the exit code at 0 on a first install,
/// otherwise `cat`'s failure would look like a WSL error).
#[cfg(target_os = "windows")]
fn wsl_read_home_file(path: &str) -> Result<String, String> {
    wsl_bash(&format!("cat {path} 2>/dev/null || true"))
}

/// Write a `~/...` file inside the distro with owner-only permissions.
/// The contents go through stdin so they never have to be escaped into
/// a shell argument. `umask 077` births the temp 0600, then an atomic
/// `mv` swaps it in: the opt-in vault password embed writes a plaintext
/// credential, so the real config must never be left world-readable nor
/// partially written. The parent folder must already exist (it is the
/// client's own, and its presence is what made the client "detected").
#[cfg(target_os = "windows")]
fn wsl_write_home_file_private(path: &str, contents: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let script = format!("umask 077 && cat > {path}.tmp && mv {path}.tmp {path}");
    let mut child = Command::new("wsl.exe")
        .args(["--", "bash", "-c", &script])
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
        .write_all(contents.as_bytes())
        .map_err(|e| format!("Failed to write to WSL: {e}"))?;
    let status = child
        .wait()
        .map_err(|e| format!("wsl.exe did not finish: {e}"))?;
    if !status.success() {
        return Err(format!("wsl.exe could not write {path}"));
    }
    Ok(())
}

/// The client marker folders present in the distro's home, in ONE
/// `wsl.exe` spawn (each spawn costs a noticeable fraction of a
/// second). Returns the `~`-relative names found.
#[cfg(target_os = "windows")]
pub(crate) fn wsl_detect_markers() -> Result<Vec<String>, String> {
    let checks: Vec<String> = McpClient::wsl_markers()
        .iter()
        .map(|m| format!("[ -e \"$HOME/{m}\" ] && echo \"{m}\";"))
        .collect();
    let out = wsl_bash(&format!("{} true", checks.join(" ")))?;
    Ok(out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Never-detected elsewhere: the WSL target only renders on Windows.
#[cfg(not(target_os = "windows"))]
pub(crate) fn wsl_detect_markers() -> Result<Vec<String>, String> {
    Err("WSL is only available on the Windows build".to_string())
}

/// The clients installed inside the WSL distro, plus the markers that
/// told us (the config paths of clients that moved depend on them).
pub(crate) fn detect_wsl_clients() -> (Vec<McpClient>, Vec<String>) {
    let markers = wsl_detect_markers().unwrap_or_default();
    let found = McpClient::available_in_wsl()
        .into_iter()
        .filter(|c| c.detected_in_wsl(&markers))
        .collect();
    (found, markers)
}

/// Whether the distro's config for `client` carries an `oryxis`
/// entry. Same gate as [`client_has_entry`] for the WSL target, so a
/// rewrite never creates a config inside a distro that never had one.
#[cfg(target_os = "windows")]
fn wsl_client_has_entry(client: McpClient, markers: &[String]) -> bool {
    let Some(path) = client.wsl_config_path(markers) else {
        return false;
    };
    wsl_read_home_file(&path).is_ok_and(|c| mcp_clients::has_entry(client.format(), &c))
}

/// Scrub the embedded `ORYXIS_VAULT_PASSWORD` from every client config
/// that actually carries an `oryxis` entry, in place, WITHOUT creating
/// one anywhere. Covers every client on this machine and (on Windows)
/// inside the WSL distro, because the password may have been installed
/// into any of them regardless of the currently selected one. Blocking
/// I/O; call from a background task. `Ok(())` means nothing failed; the
/// caller surfaces any `Err` so a "revoked" claim never hides a
/// plaintext credential still on disk.
pub(crate) fn strip_vault_password_everywhere(token: &str) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    for client in McpClient::available() {
        if client_has_entry(client)
            && let Err(e) = install_mcp_config_to_file(client, token, None)
        {
            errors.push(e);
        }
    }
    #[cfg(target_os = "windows")]
    {
        let markers = wsl_detect_markers().unwrap_or_default();
        for client in McpClient::available_in_wsl() {
            if wsl_client_has_entry(client, &markers)
                && let Err(e) = install_mcp_config_to_wsl(client, token, None)
            {
                errors.push(e);
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Re-write the `oryxis` entry, with the current token and vault
/// password, into every client config that already carries one, on
/// this machine and (on Windows) inside the WSL distro. The plugin
/// update's refresh: the launcher path is stable, but the entry's
/// secrets must follow the settings. Errors are logged, not surfaced:
/// nothing the user is looking at asked for this write.
pub(crate) fn refresh_installed_clients(token: &str, vault_pw: Option<&str>) {
    for client in McpClient::available() {
        if client_has_entry(client)
            && let Err(e) = install_mcp_config_to_file(client, token, vault_pw)
        {
            tracing::warn!(client = client.name(), error = %e, "failed to refresh MCP client config");
        }
    }
    #[cfg(target_os = "windows")]
    {
        let markers = wsl_detect_markers().unwrap_or_default();
        for client in McpClient::available_in_wsl() {
            if wsl_client_has_entry(client, &markers)
                && let Err(e) = install_mcp_config_to_wsl(client, token, vault_pw)
            {
                tracing::warn!(client = client.name(), error = %e, "failed to refresh MCP client config (WSL)");
            }
        }
    }
}

/// Write/merge the oryxis MCP entry into `client`'s config on this
/// machine, in that client's shape (the same place its own `mcp add`
/// command writes). Merge, never replace: these are the clients' own
/// state files. Threads `token` and the opt-in vault password through
/// so the on-disk config always carries whatever the current settings
/// hold (a `None` password strips a previously installed one).
///
/// Refuses when the client's folder is absent: the folder's presence
/// is what says the client is installed, and a config file dropped
/// into a folder nothing reads is litter at best and, with the vault
/// password embedded, a credential nobody is guarding.
pub(crate) fn install_mcp_config_to_file(
    client: McpClient,
    token: &str,
    vault_pw: Option<&str>,
) -> Result<String, String> {
    let config_path = client_config_path(client)?;
    let parent = config_path
        .parent()
        .ok_or_else(|| format!("{} has no config folder", config_path.display()))?;
    if !parent.is_dir() {
        return Err(format!(
            "{} is not installed here ({} does not exist)",
            client.name(),
            parent.display()
        ));
    }

    // A parse failure aborts rather than clobbering whatever the
    // client has stored there.
    let existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read {}: {e}", config_path.display()))?
    } else {
        String::new()
    };
    let output = mcp_clients::merge_into(client.format(), &existing, native_entry(token, vault_pw))
        .map_err(|e| format!("{e} ({})", config_path.display()))?;
    write_config_private(&config_path, &output)?;

    if client == McpClient::ClaudeCode {
        sweep_legacy_mcp_config();
    }

    Ok(config_path.display().to_string())
}

/// Write a client config with owner-only permissions (0600) on Unix.
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

/// Write/merge the oryxis MCP entry into `client`'s config inside the
/// WSL distro, for a client running inside WSL on a Windows host.
/// Shells out to `wsl.exe` (default distro): reads the current config,
/// merges in Rust so the file stays well-formed, and writes the result
/// back through stdin so the payload never has to survive shell
/// quoting. The entry shape comes from [`oryxis_mcp_entry_wsl`]
/// (cmd.exe wrapper when a secret is set). Claude Code's legacy
/// dead-letter `~/.claude/.mcp.json` gets its `oryxis` entry swept,
/// mirroring the native install.
///
/// Only meaningful on Windows; returns an error elsewhere, where there
/// is no `wsl.exe` to talk to.
pub(crate) fn install_mcp_config_to_wsl(
    client: McpClient,
    token: &str,
    vault_pw: Option<&str>,
) -> Result<String, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (client, token, vault_pw);
        Err("WSL install is only available on the Windows build".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        let markers = wsl_detect_markers()?;
        if !client.detected_in_wsl(&markers) {
            return Err(format!("{} is not installed in the WSL distro", client.name()));
        }
        let path = client
            .wsl_config_path(&markers)
            .ok_or_else(|| format!("{} cannot run inside WSL", client.name()))?;
        let existing = wsl_read_home_file(&path)?;
        let output = mcp_clients::merge_into(client.format(), existing.trim(), wsl_entry(token, vault_pw))
            .map_err(|e| format!("{e} (WSL {path})"))?;
        wsl_write_home_file_private(&path, &output)?;

        if client == McpClient::ClaudeCode {
            // Sweep a stale `oryxis` entry out of the dead-letter path this
            // app used to write inside the distro. Best-effort, jq-free:
            // read, strip in Rust, write back only when something changed.
            if let Ok(content) = wsl_read_home_file("~/.claude/.mcp.json")
                && let Some(stripped) = mcp_clients::strip_entry(
                    mcp_clients::ConfigFormat::JsonMcpServers,
                    content.trim(),
                )
            {
                let _ = wsl_write_home_file_private("~/.claude/.mcp.json", &stripped);
            }
        }

        Ok(format!("{path} (WSL)"))
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
        cmd_escape, mcp_config_snippet_display, oryxis_mcp_entry, oryxis_mcp_entry_wsl, McpClient,
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

    // The rendered snippet is what a bystander reads off the screen, so
    // a masked panel must not spell out the token nor the embedded
    // master password, in either shape. Structural, because the two
    // spellings of the same value sit three lines apart and a future
    // edit to one is easy to make without the other.
    #[test]
    fn masked_snippet_carries_neither_secret() {
        const TOKEN: &str = "6f1c0b9a4d3e2f108c7b6a5948372615";
        const PW: &str = "correct horse battery staple";
        for client in McpClient::ALL {
            for wsl in [false, true] {
                let masked = mcp_config_snippet_display(client, TOKEN, Some(PW), wsl, false);
                assert!(!masked.contains(TOKEN), "token leaked while masked (wsl={wsl})");
                assert!(!masked.contains(PW), "vault password leaked while masked (wsl={wsl})");
                let revealed = mcp_config_snippet_display(client, TOKEN, Some(PW), wsl, true);
                assert!(revealed.contains(TOKEN), "token missing when revealed (wsl={wsl})");
                assert!(revealed.contains(PW), "vault password missing when revealed (wsl={wsl})");
            }
        }
    }

    // Masking must not invent auth for a server running without it: an
    // empty token keeps producing the env-free entry.
    #[test]
    fn masked_snippet_keeps_an_unset_token_unset() {
        for client in McpClient::ALL {
            let masked = mcp_config_snippet_display(client, "", None, false, false);
            assert!(!masked.contains("ORYXIS_MCP_TOKEN"));
            assert!(!masked.contains('\u{2022}'));
        }
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
}
