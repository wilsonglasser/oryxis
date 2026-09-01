//! Settings dispatch helpers: curated local terminals. The Local*
//! arm family, the open/spawn decision flow, and the shell
//! detection + spawn plumbing (formerly shell.rs).

use super::*;

impl Oryxis {
    /// The curated local terminals as launch payloads, in list order.
    /// Empty when never scanned or genuinely empty (the caller decides
    /// what to do with an empty list).
    pub(crate) fn local_terminal_specs(&self) -> Vec<crate::state::LocalShellSpec> {
        self.local_terminals
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|e| e.to_spec())
            .collect()
    }

    /// Persist the curated local-terminal list to the `local_terminals`
    /// setting as JSON. Machine-local config, never synced or exported.
    pub(crate) fn persist_local_terminals(&self) {
        let json = serde_json::to_string(self.local_terminals.as_deref().unwrap_or(&[]))
            .unwrap_or_else(|_| "[]".to_string());
        if let Some(vault) = self.vault.as_ref()
            && let Err(e) = vault.set_setting("local_terminals", &json)
        {
            tracing::warn!("Failed to persist local_terminals: {e}");
        }
    }

    /// Persist the "always open X" preference (the entry key, or empty
    /// for "always ask").
    pub(crate) fn persist_local_terminal_default(&self) {
        let value = self
            .local_terminal_default
            .map(|id| id.to_string())
            .unwrap_or_default();
        if let Some(vault) = self.vault.as_ref()
            && let Err(e) = vault.set_setting("local_terminal_default", &value)
        {
            tracing::warn!("Failed to persist local_terminal_default: {e}");
        }
    }

    /// Decide how to satisfy an "open local terminal" intent against the
    /// already-scanned list: honor a valid "always open X" default, else
    /// spawn directly when there's nothing to choose (0 or 1 entry), else
    /// show the picker. Assumes `local_terminals` is `Some`.
    fn decide_open_local_terminal(&mut self) -> Task<Message> {
        // "Always open X": a default id still present in the list spawns
        // straight away. A dangling id falls through to the count logic.
        if let Some(id) = self.local_terminal_default
            && let Some(spec) = self
                .local_terminals
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.to_spec())
        {
            return self.open_local_shell_resolved(Some((spec.program, spec.args, spec.label)));
        }
        let specs = self.local_terminal_specs();
        match specs.as_slice() {
            // Nothing curated: fall back to the OS default shell.
            [] => self.open_local_shell_resolved(None),
            [only] => self.open_local_shell_resolved(Some((
                only.program.clone(),
                only.args.clone(),
                only.label.clone(),
            ))),
            _ => Task::done(Message::Settings(SettingsMessage::ShowLocalShellPicker)),
        }
    }

    /// Send a resolved local-shell choice wherever the user asked for it:
    /// into the pane they were splitting, or into a new tab.
    ///
    /// Both entry points funnel through here so the curated list, the
    /// "always open X" default and the picker apply identically. Splitting
    /// used to bypass the decision entirely and always spawn the OS default
    /// shell (issue #108). Taking the pending split here (rather than when
    /// the picker row is clicked) is what lets the shell picker be raised
    /// in between without losing the target pane.
    pub(crate) fn open_local_shell_resolved(
        &mut self,
        pick: Option<(String, Vec<String>, String)>,
    ) -> Task<Message> {
        if let Some((tab_id, target, axis)) = self.pending_pane_split.take()
            && let Some(tab_idx) = self.tab_index_by_id(tab_id)
        {
            return self.local_shell_into_pane(tab_idx, target, axis, pick);
        }
        spawn_local_shell(self, pick)
    }
}

impl Oryxis {
    /// Local-terminal arms: the curated list (scan / add / edit /
    /// remove / default), the shell picker and the spawn entry points.
    pub(super) fn handle_settings_local_terminals(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::OpenLocalShell => {
                // Burger menu is the most common entry point for
                // this action; dismiss it so the spawned shell
                // doesn't appear behind the still-open dropdown.
                self.panels.burger_menu = false;
                // First open ever: run the one-time scan, then act on the
                // result. Every later open reads the persisted list (the
                // scan never repeats unless the user asks for a re-scan).
                if self.local_terminals.is_none() {
                    return Ok(Task::perform(
                        tokio::task::spawn_blocking(detect_local_shells),
                        |result| Message::Settings(SettingsMessage::LocalShellsDetected(result.unwrap_or_default())),
                    ));
                }
                return Ok(self.decide_open_local_terminal());
            }
            SettingsMessage::ShowLocalShellPicker => {
                self.local_shell_picker_open = true;
                // The list is already populated by the time we get here
                // (OpenLocalShell scans first). Guard the never-scanned
                // case anyway so a direct dispatch still fills the picker.
                if self.local_terminals.is_none() {
                    return Ok(Task::perform(
                        tokio::task::spawn_blocking(detect_local_shells),
                        |result| Message::Settings(SettingsMessage::LocalShellsDetected(result.unwrap_or_default())),
                    ));
                }
            }
            SettingsMessage::LocalShellsDetected(shells) => {
                // One-time scan result: seed the curated list and persist.
                let entries: Vec<crate::state::LocalTerminalEntry> =
                    shells.into_iter().map(detected_entry).collect();
                self.local_terminals = Some(entries);
                self.persist_local_terminals();
                // If the picker overlay is open it was opened explicitly,
                // so just leave it showing the freshly filled list. If it
                // isn't, this scan was triggered by an open intent, so
                // continue the open decision now that we have the list.
                if !self.local_shell_picker_open {
                    return Ok(self.decide_open_local_terminal());
                }
            }
            SettingsMessage::HideLocalShellPicker => {
                self.local_shell_picker_open = false;
                // Abandoning the picker abandons the split intent that
                // raised it, so an unrelated later open can't inherit a
                // stale target pane (same rule as `HideNewTabPicker`).
                self.pending_pane_split = None;
            }
            SettingsMessage::OpenLocalShellWith { program, args, label } => {
                self.local_shell_picker_open = false;
                return Ok(self.open_local_shell_resolved(Some((program, args, label))));
            }
            SettingsMessage::OpenLocalTerminalsSettings => {
                self.local_shell_picker_open = false;
                self.active_view = View::Settings;
                self.settings_section = crate::state::SettingsSection::Terminal;
                // Direct view assignment rather than ChangeView, so the
                // strip entry is this handler's own responsibility or
                // Settings would show with no chip (issue #120).
                self.ensure_panel_tab(crate::state::PanelKind::Settings);
            }
            SettingsMessage::RescanLocalTerminals => {
                return Ok(Task::perform(
                    tokio::task::spawn_blocking(detect_local_shells),
                    |result| Message::Settings(SettingsMessage::LocalTerminalsRescanned(result.unwrap_or_default())),
                ));
            }
            SettingsMessage::LocalTerminalsRescanned(shells) => {
                // Merge: keep everything already curated (manual entries and
                // user edits), append only detected entries whose command
                // isn't present yet. A previously-removed-but-still-detected
                // entry reappearing on an explicit re-scan is expected.
                let mut list = self.local_terminals.take().unwrap_or_default();
                let mut seen: std::collections::HashSet<String> =
                    list.iter().map(|e| e.cmd_key()).collect();
                for s in shells {
                    let entry = detected_entry(s);
                    if seen.insert(entry.cmd_key()) {
                        list.push(entry);
                    }
                }
                self.local_terminals = Some(list);
                self.persist_local_terminals();
            }
            SettingsMessage::RemoveLocalTerminal(id) => {
                if let Some(list) = self.local_terminals.as_mut() {
                    list.retain(|e| e.id != id);
                }
                // Drop a default pointing at the now-removed entry.
                if self.local_terminal_default == Some(id) {
                    self.local_terminal_default = None;
                    self.persist_local_terminal_default();
                }
                self.persist_local_terminals();
            }
            SettingsMessage::SetDefaultLocalTerminal(id) => {
                self.local_terminal_default = id;
                self.persist_local_terminal_default();
            }
            SettingsMessage::OpenLocalTerminalAddModal => {
                self.local_terminal_form = crate::state::LocalTerminalForm::default();
                self.local_terminal_add_open = true;
            }
            SettingsMessage::OpenLocalTerminalEditModal(id) => {
                if let Some(entry) = self
                    .local_terminals
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|e| e.id == id)
                {
                    self.local_terminal_form = crate::state::LocalTerminalForm {
                        editing_id: Some(id),
                        label: entry.label.clone(),
                        program: entry.program.clone(),
                        args: entry.args.join(" "),
                        color: entry.color.clone(),
                        icon: entry.icon.clone(),
                        tags: entry.tags.join(", "),
                        error: None,
                    };
                    self.local_terminal_add_open = true;
                }
            }
            SettingsMessage::CloseLocalTerminalAddModal => {
                self.local_terminal_add_open = false;
            }
            SettingsMessage::OpenLocalTerminalIconPicker => {
                // Seed the shared host icon picker from the form and target
                // it back at the form (deferred save on IconPickerSave).
                // Fall back to the label's OS hint (then a terminal glyph)
                // so the preview matches the card when there's no override.
                self.icon_picker.icon = self.local_terminal_form.icon.clone().or_else(|| {
                    crate::os_icon::local_shell_os_hint(&self.local_terminal_form.label)
                        .or_else(|| Some("terminal".to_string()))
                });
                self.icon_picker.color = self.local_terminal_form.color.clone();
                self.icon_picker.hex_input =
                    self.local_terminal_form.color.clone().unwrap_or_default();
                self.icon_picker.icon_search = String::new();
                self.icon_color_popover = None;
                self.icon_picker.for_id = None;
                self.icon_picker.for_group_form = false;
                self.icon_picker.for_session_group = false;
                self.icon_picker.for_group_edit = false;
                self.icon_picker.for_local_terminal = true;
                self.panels.icon_picker = true;
            }
            SettingsMessage::LocalTerminalCardHovered(idx) => {
                self.hover.local_terminal_card = Some(idx);
            }
            SettingsMessage::LocalTerminalCardUnhovered(idx) => {
                self.hover.leave_local_terminal_card(idx);
            }
            SettingsMessage::LocalTerminalFormLabelChanged(v) => {
                self.local_terminal_form.label = v;
                self.local_terminal_form.error = None;
            }
            SettingsMessage::LocalTerminalFormProgramChanged(v) => {
                self.local_terminal_form.program = v;
                self.local_terminal_form.error = None;
            }
            SettingsMessage::LocalTerminalFormArgsChanged(v) => {
                self.local_terminal_form.args = v;
                self.local_terminal_form.error = None;
            }
            SettingsMessage::LocalTerminalFormTagsChanged(v) => {
                self.local_terminal_form.tags = v;
                self.local_terminal_form.error = None;
            }
            SettingsMessage::AddLocalTerminalSubmit => {
                let label = self.local_terminal_form.label.trim().to_string();
                let program = self.local_terminal_form.program.trim().to_string();
                if label.is_empty() || program.is_empty() {
                    self.local_terminal_form.error = Some("local_terminal_invalid");
                } else {
                    let args: Vec<String> = self
                        .local_terminal_form
                        .args
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();
                    let color = self.local_terminal_form.color.clone();
                    let icon = self.local_terminal_form.icon.clone();
                    let tags = crate::util::parse_tags(&self.local_terminal_form.tags);
                    let mut list = self.local_terminals.take().unwrap_or_default();
                    match self.local_terminal_form.editing_id {
                        // Edit in place: program/args/label/appearance all
                        // change; the id and manual flag are preserved.
                        Some(id) => {
                            if let Some(e) = list.iter_mut().find(|e| e.id == id) {
                                e.label = label;
                                e.program = program;
                                e.args = args;
                                e.color = color;
                                e.icon = icon;
                                e.tags = tags;
                            }
                        }
                        // Add a new manual entry, skipping an exact command
                        // duplicate (label-only difference isn't worth a dup).
                        None => {
                            let entry = crate::state::LocalTerminalEntry {
                                id: uuid::Uuid::new_v4(),
                                label,
                                program,
                                args,
                                manual: true,
                                color,
                                icon,
                                tags,
                            };
                            if !list.iter().any(|e| e.cmd_key() == entry.cmd_key()) {
                                list.push(entry);
                            }
                        }
                    }
                    self.local_terminals = Some(list);
                    self.persist_local_terminals();
                    self.local_terminal_form = crate::state::LocalTerminalForm::default();
                    self.local_terminal_add_open = false;
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}

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
        tags: Vec::new(),
    }
}

/// Spawn either the default shell (`pick = None`) or a specific
/// program (`pick = Some((program, args, label))`) and wire it up
/// as a new terminal tab.
pub(crate) fn spawn_local_shell(
    app: &mut Oryxis,
    pick: Option<(String, Vec<String>, String)>,
) -> Task<Message> {
    // Open in the focused pane's directory when it's a local shell that
    // reported one via OSC 7 (a remote SSH cwd wouldn't exist locally).
    let inherit_cwd = app
        .active_tab
        .and_then(|i| app.tabs.get(i))
        .map(|t| t.active())
        .filter(|p| matches!(p.origin, crate::state::PaneOrigin::Local(_)))
        .and_then(|p| p.cwd.clone());
    spawn_local_shell_in(app, pick, inherit_cwd)
}

/// `spawn_local_shell` with an explicit working directory (`None` =
/// the process default), bypassing the focused-pane inheritance above.
/// Used by the reconnect respawn, which runs after its tab was removed
/// and `active_tab` repointed at a NEIGHBOR tab: inheriting at spawn
/// time would read that neighbor's cwd instead of the dead pane's own.
pub(crate) fn spawn_local_shell_in(
    app: &mut Oryxis,
    pick: Option<(String, Vec<String>, String)>,
    inherit_cwd: Option<String>,
) -> Task<Message> {
    app.connecting = None; // Clear any pending SSH connection progress
    let (program_label, args_label) = match &pick {
        Some((p, a, _)) => (p.clone(), a.clone()),
        None => ("<default-shell>".into(), Vec::new()),
    };
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
            state.set_palette(app.terminal_palette.clone());
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
                Task::stream(stream).map(move |bytes| Message::Terminal(TerminalMessage::PtyOutput(pane_id, bytes))),
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
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&utf16)
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}
