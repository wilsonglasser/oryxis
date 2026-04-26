//! Minimal `~/.ssh/config` parser + Connection mapper.
//!
//! Handles the directives we actually use today: Host (block start),
//! HostName, Port, User, IdentityFile, ProxyJump. Everything else is
//! ignored. Wildcard host blocks (`Host *`, `Host *.example.com`) are
//! skipped on import — they're templates, not concrete servers.

use std::path::PathBuf;

use oryxis_core::models::connection::{AuthMethod, Connection};

/// One parsed `Host` block from an SSH config file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshConfigHost {
    /// The literal alias from the `Host` line — used as the connection
    /// label and as the fallback hostname when `HostName` is omitted.
    pub alias: String,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_file: Option<PathBuf>,
    pub proxy_jump: Option<String>,
    /// `ForwardAgent` directive — only `yes` flips it on; missing /
    /// `no` / anything else stays off, matching OpenSSH's default.
    pub forward_agent: bool,
}

/// Parse the contents of an `ssh_config` file into a list of concrete
/// host blocks. Wildcards and the universal `*` block are dropped —
/// they're config templates, not importable servers.
pub fn parse(text: &str) -> Vec<SshConfigHost> {
    let mut hosts: Vec<SshConfigHost> = Vec::new();
    let mut current: Option<SshConfigHost> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Tolerate `key value`, `key = value`, and quoted values. Split
        // on first run of whitespace or `=`, strip surrounding quotes.
        let (key, value) = match split_key_value(line) {
            Some(parts) => parts,
            None => continue,
        };
        if key.eq_ignore_ascii_case("Host") {
            // First host name on the line wins — `Host alias1 alias2`
            // creates one block referenced by either alias, but for
            // import we just use the first.
            if let Some(prev) = current.take()
                && !is_wildcard(&prev.alias)
            {
                hosts.push(prev);
            }
            let alias = value.split_whitespace().next().unwrap_or("").to_string();
            current = Some(SshConfigHost {
                alias,
                ..Default::default()
            });
            continue;
        }
        let Some(host) = current.as_mut() else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "hostname" => host.hostname = Some(value.to_string()),
            "port" => host.port = value.parse().ok(),
            "user" => host.user = Some(value.to_string()),
            "identityfile" => host.identity_file = Some(expand_tilde(value)),
            "proxyjump" => host.proxy_jump = Some(value.to_string()),
            "forwardagent" => host.forward_agent = value.eq_ignore_ascii_case("yes"),
            _ => {}
        }
    }
    if let Some(prev) = current.take()
        && !is_wildcard(&prev.alias)
    {
        hosts.push(prev);
    }
    hosts
}

/// Map a parsed entry onto an Oryxis `Connection`. We don't try to
/// resolve `IdentityFile` to a vault key id here — that would require
/// importing the keys first; for now we just flag the auth method as
/// Key so the user finishes the link in the host editor.
pub fn to_connection(host: &SshConfigHost) -> Connection {
    let hostname = host
        .hostname
        .clone()
        .unwrap_or_else(|| host.alias.clone());
    let mut conn = Connection::new(host.alias.clone(), hostname);
    if let Some(port) = host.port {
        conn.port = port;
    }
    if let Some(user) = &host.user {
        conn.username = Some(user.clone());
    }
    // If the user gave an explicit IdentityFile we lean Key; otherwise
    // Auto handles whatever's available (key, agent, password) at
    // connect time.
    conn.auth_method = if host.identity_file.is_some() {
        AuthMethod::Key
    } else {
        AuthMethod::Auto
    };
    conn.agent_forwarding = host.forward_agent;
    // Drop the import provenance into notes so the user can find the
    // origin later — useful when reconciling with a manual edit.
    conn.notes = Some(format!(
        "Imported from ssh_config (alias `{}`)",
        host.alias
    ));
    conn
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    // Recognise `key value`, `key=value`, or `key = value`. The split
    // happens on the first whitespace or `=`, whichever comes first.
    let split_at = line
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || *c == '=')?
        .0;
    let key = line[..split_at].trim();
    let value = line[split_at..]
        .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
        .trim();
    if key.is_empty() {
        return None;
    }
    let value = value.trim_matches('"');
    Some((key, value))
}

fn is_wildcard(alias: &str) -> bool {
    alias.contains('*') || alias.contains('?')
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }
    if path == "~"
        && let Some(home) = home_dir()
    {
        return home;
    }
    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Default location of the user's SSH config file. The import flow
/// uses this as the file picker's starting path.
pub fn default_config_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".ssh").join("config"))
}

#[cfg(test)]
#[path = "ssh_config_tests.rs"]
mod tests;
