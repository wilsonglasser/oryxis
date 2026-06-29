//! Settings dispatch helpers: shell. Split out of dispatch_settings/mod.rs.

use super::*;
/// Build a fresh auto-detected curated entry from a scan result. Gets a
/// new id and no manual flag / appearance override (the OS hint supplies
/// the icon + color at render time until the user customizes it).
pub(crate) fn detected_entry(s: crate::state::LocalShellSpec) -> crate::state::LocalTerminalEntry {
    crate::state::LocalTerminalEntry {
        id: uuid::Uuid::new_v4(),
        label: s.label,
        program: s.program,
        args: s.args,
        manual: false,
        color: None,
        icon: None,
    }
}

/// Spawn either the default shell (`pick = None`) or a specific
/// program (`pick = Some((program, args, label))`) and wire it up
/// as a new terminal tab.
pub(crate) fn spawn_local_shell(
    app: &mut Oryxis,
    pick: Option<(String, Vec<String>, String)>,
) -> Task<Message> {
    app.connecting = None; // Clear any pending SSH connection progress
    let (program_label, args_label) = match &pick {
        Some((p, a, _)) => (p.clone(), a.clone()),
        None => ("<default-shell>".into(), Vec::new()),
    };
    // Open in the focused pane's directory when it's a local shell that
    // reported one via OSC 7 (a remote SSH cwd wouldn't exist locally).
    let inherit_cwd = app
        .active_tab
        .and_then(|i| app.tabs.get(i))
        .map(|t| t.active())
        .filter(|p| matches!(p.origin, crate::state::PaneOrigin::Local(_)))
        .and_then(|p| p.cwd.clone());
    let result = match &pick {
        Some((program, args, _)) => TerminalState::new_with_command(
            DEFAULT_TERM_COLS as u16,
            DEFAULT_TERM_ROWS as u16,
            program,
            args,
            inherit_cwd.as_deref(),
        ),
        None => TerminalState::new(
            DEFAULT_TERM_COLS as u16,
            DEFAULT_TERM_ROWS as u16,
            inherit_cwd.as_deref(),
        ),
    };
    match result {
        Ok((mut state, rx)) => {
            tracing::info!(
                "Spawned local shell: program={} args={:?}",
                program_label, args_label
            );
            state.palette = app.terminal_palette.clone();
            let tab_idx = app.tabs.len();
            let label = pick
                .as_ref()
                .map(|(_, _, l)| l.clone())
                .unwrap_or_else(|| "Local Shell".to_string());
            // Capture the exact shell so a saved session group restores it.
            // No pick = default OS shell (empty program).
            let origin = crate::state::PaneOrigin::Local(crate::state::LocalShellSpec {
                label: label.clone(),
                program: pick.as_ref().map(|(p, _, _)| p.clone()).unwrap_or_default(),
                args: pick.as_ref().map(|(_, a, _)| a.clone()).unwrap_or_default(),
            });
            app.tabs.push(TerminalTab::new_single(
                label,
                Arc::new(Mutex::new(state)),
            ));
            app.tabs[tab_idx].active_mut().origin = origin;
            let pane_id = app.tabs[tab_idx].active().id;
            app.active_tab = Some(tab_idx);
            app.remember_terminal_tab_focus(tab_idx);
            app.active_view = View::Terminal;
            let stream = UnboundedReceiverStream::new(rx);
            Task::batch(vec![
                app.tab_scroll_to_active(),
                Task::stream(stream).map(move |bytes| Message::PtyOutput(pane_id, bytes)),
            ])
        }
        Err(e) => {
            tracing::error!(
                "Failed to spawn local shell program={} args={:?}: {}",
                program_label, args_label, e
            );
            Task::none()
        }
    }
}

/// Build the menu of available local shells: cmd / PowerShell /
/// Git Bash / Nushell / Cygwin / MSYS2 / WSL on Windows, or the
/// login shell plus any other common shells on `PATH` on Unix.
pub(crate) fn detect_local_shells() -> Vec<crate::state::LocalShellSpec> {
    #[cfg(unix)]
    {
        detect_unix_shells()
    }
    #[cfg(target_os = "windows")]
    {
        use crate::state::LocalShellSpec;
        let mut out: Vec<LocalShellSpec> = Vec::new();
        // PowerShell, prefer pwsh.exe (PS7+) over the bundled
        // powershell.exe; both detect via `where.exe` to cope with
        // the fact that PS7 isn't on every machine.
        if which("pwsh.exe").is_some() {
            out.push(LocalShellSpec {
                label: "PowerShell".into(),
                program: "pwsh.exe".into(),
                args: vec![],
            });
        } else {
            out.push(LocalShellSpec {
                label: "Windows PowerShell".into(),
                program: "powershell.exe".into(),
                args: vec![],
            });
        }
        out.push(LocalShellSpec {
            label: "Command Prompt".into(),
            program: "cmd.exe".into(),
            args: vec![],
        });
        // Git Bash, the MSYS2 bash that ships with Git for Windows.
        // `where bash.exe` is unreliable (it usually resolves to the
        // WSL bash shim), so probe the canonical install locations.
        // `--login` sources `/etc/profile` so the MSYS `/usr/bin` PATH
        // is set up and `git`/`ls`/... resolve.
        if let Some(path) = find_git_bash() {
            out.push(LocalShellSpec {
                label: "Git Bash".into(),
                program: path,
                args: vec!["--login".into(), "-i".into()],
            });
        }
        // Nushell, cross-platform and normally on PATH.
        if which("nu.exe").is_some() {
            out.push(LocalShellSpec {
                label: "Nushell".into(),
                program: "nu.exe".into(),
                args: vec![],
            });
        }
        // Cygwin / MSYS2 bash, niche but still alive on dev boxes.
        // Same `where` ambiguity as Git Bash, so fixed roots only.
        for (label, path) in [
            ("MSYS2", r"C:\msys64\usr\bin\bash.exe"),
            ("Cygwin", r"C:\cygwin64\bin\bash.exe"),
        ] {
            if std::path::Path::new(path).is_file() {
                out.push(LocalShellSpec {
                    label: label.into(),
                    program: path.into(),
                    args: vec!["--login".into(), "-i".into()],
                });
            }
        }
        // WSL distros, `wsl --list --quiet` outputs UTF-16 LE BOM
        // by default. Decode and split on lines to get distro names.
        for distro in list_wsl_distros() {
            out.push(LocalShellSpec {
                label: format!("{distro} (WSL)"),
                program: "wsl.exe".into(),
                args: vec!["-d".into(), distro],
            });
        }
        out
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Resolve the bash that ships with Git for Windows by probing the
/// canonical install roots (system 64/32-bit and per-user). Returns
/// the first `bin\bash.exe` that exists.
#[cfg(target_os = "windows")]
pub(crate) fn find_git_bash() -> Option<String> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(base) = std::env::var(var) {
            candidates.push(std::path::PathBuf::from(base).join(r"Git\bin\bash.exe"));
        }
    }
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        candidates.push(std::path::PathBuf::from(base).join(r"Programs\Git\bin\bash.exe"));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Build the Unix local-shell menu: the user's login `$SHELL` first
/// (marked as the default), then any other common interactive shells
/// found on `PATH`. Deduplicated by resolved path.
#[cfg(unix)]
pub(crate) fn detect_unix_shells() -> Vec<crate::state::LocalShellSpec> {
    use crate::state::LocalShellSpec;
    let mut out: Vec<LocalShellSpec> = Vec::new();
    // Dedup by canonical path so `/bin/bash` and `/usr/bin/bash` (same
    // binary via a symlinked `/bin`) don't show up as two entries.
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    let canonical = |path: &std::path::Path| -> std::path::PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    };
    let basename = |path: &str| -> String {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string())
    };
    // Login shell goes first, flagged so the user knows which is theirs.
    if let Ok(shell) = std::env::var("SHELL")
        && !shell.is_empty()
        && std::path::Path::new(&shell).is_file()
        && seen.insert(canonical(std::path::Path::new(&shell)))
    {
        out.push(LocalShellSpec {
            label: format!("{} ({})", basename(&shell), crate::i18n::t("shell_default")),
            program: shell,
            args: vec![],
        });
    }
    for name in ["bash", "zsh", "fish", "nu"] {
        if let Some(path) = unix_which(name)
            && seen.insert(canonical(&path))
        {
            out.push(LocalShellSpec {
                label: name.into(),
                program: path.to_string_lossy().into_owned(),
                args: vec![],
            });
        }
    }
    out
}

/// Minimal `which`: first `PATH` entry that holds the named program.
#[cfg(unix)]
pub(crate) fn unix_which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|cand| cand.is_file())
}

#[cfg(target_os = "windows")]
pub(crate) fn which(program: &str) -> Option<std::path::PathBuf> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW (0x0800_0000), without this each `where.exe`
    // call briefly flashes a cmd console behind oryxis.
    let out = std::process::Command::new("where")
        .arg(program)
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next().map(|l| std::path::PathBuf::from(l.trim()))
}

#[cfg(target_os = "windows")]
pub(crate) fn list_wsl_distros() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let out = match std::process::Command::new("wsl")
        .args(["--list", "--quiet"])
        .creation_flags(0x0800_0000)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    // wsl.exe emits UTF-16 LE with a BOM. Decode by reading
    // u16 pairs.
    let bytes = out.stdout;
    let utf16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&utf16)
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}
