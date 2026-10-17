//! A snippet with actions, run step by step on one pane.
//!
//! This half is pure: what a run IS (the flattened steps, where it waits),
//! which app actions a step may name, how a nested snippet is expanded,
//! how a save step names its file. The half that talks to panes, the
//! clipboard and the disk is `dispatch_snippet_run.rs`.
//!
//! Snippets travel (sync, `.oryxis` import), so the DATA never decides
//! what runs on this device: an action step names an id, and only the
//! ids [`allowed_step_actions`] lists are run; a save step names a file,
//! and only inside the output folder this device chose.

use oryxis_core::login_script::OutputMatcher;
use oryxis_core::models::{Snippet, SnippetStep};
use uuid::Uuid;

/// App actions that are safe to run against the pane owned by a snippet.
/// These are deliberately separate from global hotkeys: a snippet action is
/// never exposed as a user shortcut and always targets its run's pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnippetAction {
    ClearScrollback,
    CopyScreen,
    CopyLastCommand,
    SaveOutput,
    PasteAndRun,
}

impl SnippetAction {
    pub(crate) fn all() -> &'static [Self] {
        use SnippetAction::*;
        &[
            ClearScrollback,
            CopyScreen,
            CopyLastCommand,
            SaveOutput,
            PasteAndRun,
        ]
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::ClearScrollback => "terminal_clear_scrollback",
            Self::CopyScreen => "terminal_copy_screen",
            Self::CopyLastCommand => "terminal_copy_last_command",
            Self::SaveOutput => "terminal_save_output",
            Self::PasteAndRun => "terminal_paste_and_run",
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::ClearScrollback => "terminal_clear_scrollback",
            Self::CopyScreen => "terminal_copy_screen",
            Self::CopyLastCommand => "terminal_copy_last_command",
            Self::SaveOutput => "terminal_save_output",
            Self::PasteAndRun => "snippet_run",
        }
    }
}

/// The app actions a snippet step may run: pane actions only, each aimed
/// at the pane the run belongs to. Anything else a step names (a newer
/// build's action, a tab or vault verb arriving by sync) stops the run.
pub(crate) fn allowed_step_actions() -> &'static [SnippetAction] {
    SnippetAction::all()
}

/// The allowed action a step's id names, if any.
pub(crate) fn step_action(id: &str) -> Option<SnippetAction> {
    allowed_step_actions()
        .iter()
        .copied()
        .find(|a| a.id() == id)
}

/// Why a snippet's actions cannot be laid out for a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanError {
    /// A snippet runs itself, directly or through others (its label).
    Cycle(String),
    /// A "run snippet" step names one that is not in this vault.
    Missing,
    /// Expansion past [`MAX_PLAN_STEPS`] (a snippet called many times
    /// over through others).
    TooLong,
    /// A "run snippet" step names an install script (its label), which
    /// only ever runs on its own, after its confirmation shows the body
    /// (#147); copied into another run it would skip both that and the
    /// per-host "already ran here" memory.
    Install(String),
}

/// The most steps one run may expand to.
pub(crate) const MAX_PLAN_STEPS: usize = 1000;

/// `root`'s actions with every "run snippet" step replaced by that
/// snippet's own actions, recursively. A snippet that reaches itself is
/// refused, never run in a loop.
pub(crate) fn flatten(snippets: &[Snippet], root: &Snippet) -> Result<Vec<SnippetStep>, PlanError> {
    let mut out = Vec::new();
    let mut stack = vec![root.id];
    expand(snippets, root, &mut stack, &mut out)?;
    Ok(out)
}

fn expand(
    snippets: &[Snippet],
    snippet: &Snippet,
    stack: &mut Vec<Uuid>,
    out: &mut Vec<SnippetStep>,
) -> Result<(), PlanError> {
    for step in snippet.actions() {
        match step {
            SnippetStep::RunSnippet { id } => {
                let inner = snippets
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or(PlanError::Missing)?;
                if inner.install {
                    return Err(PlanError::Install(inner.label.clone()));
                }
                if stack.contains(&id) {
                    return Err(PlanError::Cycle(inner.label.clone()));
                }
                stack.push(id);
                expand(snippets, inner, stack, out)?;
                stack.pop();
            }
            other => {
                if out.len() >= MAX_PLAN_STEPS {
                    return Err(PlanError::TooLong);
                }
                out.push(other);
            }
        }
    }
    Ok(())
}

/// Whether saving `steps` as snippet `id` (named `label`) would make it
/// reach itself; `Some` names the snippet the loop comes back to. The
/// editor refuses such a save, so a cycle can only arrive by sync, where
/// [`flatten`] refuses to run it.
pub(crate) fn would_cycle(
    snippets: &[Snippet],
    id: Uuid,
    label: &str,
    steps: &[SnippetStep],
) -> Option<String> {
    let mut probe = Snippet::new(label, "");
    probe.id = id;
    probe.steps = steps.to_vec();
    let others: Vec<Snippet> = snippets
        .iter()
        .filter(|s| s.id != id)
        .cloned()
        .chain([probe.clone()])
        .collect();
    match flatten(&others, &probe) {
        Err(PlanError::Cycle(label)) => Some(label),
        _ => None,
    }
}

/// The `{name}` placeholders of every command step, in order, each once
/// (the first default wins): what the variables dialog asks for.
pub(crate) fn plan_placeholders(steps: &[SnippetStep]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for step in steps {
        if let SnippetStep::Command { text } = step {
            for (name, default) in crate::util::snippet_placeholders(text) {
                if !out.iter().any(|(n, _)| *n == name) {
                    out.push((name, default));
                }
            }
        }
    }
    out
}

/// `steps` with the variables filled into every command step.
pub(crate) fn substitute(steps: Vec<SnippetStep>, vars: &[(String, String)]) -> Vec<SnippetStep> {
    steps
        .into_iter()
        .map(|step| match step {
            SnippetStep::Command { text } => SnippetStep::Command {
                text: crate::util::substitute_snippet_vars(&text, vars),
            },
            other => other,
        })
        .collect()
}

/// The file name a save step writes, `{host}` and `{date}` expanded and
/// reduced to one safe name: no folder part, no `..`, no leading dot or
/// dash. `None` when nothing usable is left.
pub(crate) fn save_file_name(template: &str, host: &str, date: &str) -> Option<String> {
    let expanded = template.replace("{host}", host).replace("{date}", date);
    let mut name: String = expanded
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    name = name.trim().to_string();
    while name.starts_with('.') || name.starts_with('-') {
        name.replace_range(0..1, "_");
    }
    // 120 characters, and never past 240 bytes: ext4 and APFS take 255
    // BYTES per name, which 120 CJK characters (360) overrun.
    let mut name: String = name.chars().take(120).collect();
    while name.len() > 240 {
        name.pop();
    }
    // Windows drops trailing dots and spaces, and reads these stems as
    // devices (`NUL.txt` "succeeds" and writes nowhere), the superscript
    // digits included (`COM¹` is COM1). `$` never survives the mapping
    // above, so `CONIN$` / `CONOUT$` cannot arrive here.
    while name.ends_with('.') || name.ends_with(' ') {
        name.pop();
    }
    if name.chars().all(|c| c == '_' || c == '.') {
        return None;
    }
    let stem: Vec<char> = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_uppercase()
        .chars()
        .collect();
    let word: String = stem.iter().take(3).collect();
    let device = (matches!(word.as_str(), "CON" | "PRN" | "AUX" | "NUL") && stem.len() == 3)
        || (matches!(word.as_str(), "COM" | "LPT")
            && stem.len() == 4
            && matches!(stem[3], '0'..='9' | '¹' | '²' | '³'));
    if device {
        name.insert(0, '_');
    }
    Some(name)
}

/// One snippet run in progress.
pub(crate) struct SnippetRun {
    pub id: u64,
    /// What the user started, for the status line and the toasts.
    pub label: String,
    /// The pane everything goes to and is read from.
    pub pane_id: Uuid,
    pub steps: Vec<SnippetStep>,
    /// Index of the next step to run.
    pub next: usize,
    /// Bumped at every wait: a timer or a reply carrying an older value
    /// belongs to a wait that is over, and is dropped.
    pub generation: u64,
    pub wait: Option<RunWait>,
    /// The wait's timer, aborted when the wait ends or the run is
    /// dropped: a 60 s timeout must not keep sleeping after the command
    /// it guarded has ended.
    pub timer: Option<iced::task::Handle>,
    /// Commands sent since the last wait: a command-end wait has
    /// something to wait for even before the shell marks it running. Not
    /// a count of the ends to come: a step is a line typed, and a line
    /// read by a program (a REPL, a heredoc body, the inside of a `for`)
    /// is no command of the shell's, so it marks no end.
    pub sent_commands: u32,
    /// Whether the shell sat at its prompt when the first of those went
    /// out, read BEFORE the send: typing the command moves the pane's
    /// prompt state to Busy at once (the input capture), so reading it
    /// after would always say Busy on the pane in front.
    pub at_prompt_at_send: bool,
}

/// What a run is waiting for.
pub(crate) enum RunWait {
    /// The shell's end-of-command mark (OSC 133 D) of the command this
    /// wait is about; `continue_on_error` = a failure does not stop the
    /// run. `ready` says whether D marks already belong to the commands
    /// the run sent: it is when the first of them went out with the
    /// shell idle at its prompt, or when nothing was sent and a command
    /// is already running (that one is the subject). Otherwise a D may
    /// still be in flight from before the shell read the commands (the
    /// previous command's, in the same read; a command still running
    /// when a trigger fired; the D a PS1 scheme prints with the session's
    /// first prompt), so D marks are passed over until the shell shows it
    /// is ready (`RunMark::Ready`). After several commands the wait ends
    /// at the first end the shell marks (one wait per command when each
    /// has to finish): counting steps as ends would hang on any line a
    /// program reads.
    CommandEnd {
        continue_on_error: bool,
        ready: bool,
    },
    /// A pattern in the pane's output.
    Output(OutputMatcher),
    /// A timer.
    Timer,
    /// A save step's write.
    File,
    /// The Save Output dialog an action step opened.
    Dialog,
    /// Waiting for the clipboard read started by a paste action.
    Clipboard,
}

/// The shell-integration marks a command-end wait reads, in the order a
/// batch carried them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunMark {
    /// The shell is at a prompt (OSC 133 B) or has started a command
    /// (OSC 133 C): a D after this one ends a command the shell read
    /// after it was ready.
    Ready,
    /// OSC 133 D, with the exit code when the shell reported one.
    End(Option<i32>),
}

/// What a command-end wait makes of one batch's marks: the exit of the
/// command it waits for, if this batch ended it. `ready` is updated as the
/// marks go by.
pub(crate) fn command_end_in(ready: &mut bool, marks: &[RunMark]) -> Option<Option<i32>> {
    for mark in marks {
        match *mark {
            RunMark::Ready => *ready = true,
            RunMark::End(code) if *ready => return Some(code),
            RunMark::End(_) => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_install_script_is_never_run_from_another_snippet() {
        let mut install = snip(
            "setup",
            vec![SnippetStep::Command {
                text: "curl x | sh".into(),
            }],
        );
        install.install = true;
        let outer = snip("outer", vec![SnippetStep::RunSnippet { id: install.id }]);
        let all = vec![install.clone(), outer.clone()];
        assert_eq!(
            flatten(&all, &outer),
            Err(PlanError::Install("setup".into()))
        );
    }

    #[test]
    fn a_ready_wait_takes_the_next_end() {
        let mut ready = true;
        assert_eq!(
            command_end_in(&mut ready, &[RunMark::End(Some(0))]),
            Some(Some(0))
        );
    }

    #[test]
    fn an_end_in_flight_before_the_prompt_is_passed_over() {
        // The previous command's D, then the prompt, in one read: the
        // command the run sent has not run yet.
        let mut ready = false;
        let batch = [RunMark::End(Some(0)), RunMark::Ready];
        assert_eq!(command_end_in(&mut ready, &batch), None);
        assert!(ready);
        // Its own C and D come later.
        assert_eq!(
            command_end_in(&mut ready, &[RunMark::Ready, RunMark::End(Some(2))]),
            Some(Some(2))
        );
    }

    #[test]
    fn a_command_with_no_c_mark_still_ends_after_its_prompt() {
        // A PS1-only scheme prints D;A;B with every prompt and nothing at
        // the command's start: the prompt's B is the readiness.
        let mut ready = false;
        assert_eq!(
            command_end_in(&mut ready, &[RunMark::End(None), RunMark::Ready]),
            None
        );
        assert_eq!(
            command_end_in(&mut ready, &[RunMark::End(Some(1)), RunMark::Ready]),
            Some(Some(1))
        );
    }

    fn snip(label: &str, steps: Vec<SnippetStep>) -> Snippet {
        let mut s = Snippet::new(label, "");
        s.set_actions(steps);
        s
    }

    fn cmd(t: &str) -> SnippetStep {
        SnippetStep::Command { text: t.into() }
    }

    #[test]
    fn a_nested_snippet_is_expanded_in_place() {
        let inner = snip("inner", vec![cmd("b"), cmd("c")]);
        let outer = snip(
            "outer",
            vec![cmd("a"), SnippetStep::RunSnippet { id: inner.id }, cmd("d")],
        );
        let all = vec![inner.clone(), outer.clone()];
        assert_eq!(
            flatten(&all, &outer).unwrap(),
            vec![cmd("a"), cmd("b"), cmd("c"), cmd("d")]
        );
    }

    #[test]
    fn a_snippet_that_reaches_itself_is_refused() {
        let mut a = snip("a", vec![cmd("x")]);
        let b = snip("b", vec![SnippetStep::RunSnippet { id: a.id }]);
        a.set_actions(vec![cmd("x"), SnippetStep::RunSnippet { id: b.id }]);
        let all = vec![a.clone(), b.clone()];
        assert_eq!(flatten(&all, &a), Err(PlanError::Cycle("a".into())));
        assert_eq!(would_cycle(&all, b.id, "b", &b.steps), Some("b".into()));
        // The same snippet twice in a row is a repeat, not a cycle.
        let c = snip(
            "c",
            vec![
                SnippetStep::RunSnippet { id: b.id },
                SnippetStep::RunSnippet { id: b.id },
            ],
        );
        let leaf = snip("leaf", vec![cmd("y")]);
        let d = snip(
            "d",
            vec![
                SnippetStep::RunSnippet { id: leaf.id },
                SnippetStep::RunSnippet { id: leaf.id },
            ],
        );
        let all = vec![leaf, d.clone(), c];
        assert_eq!(flatten(&all, &d).unwrap(), vec![cmd("y"), cmd("y")]);
    }

    #[test]
    fn a_missing_snippet_stops_the_plan() {
        let a = snip(
            "a",
            vec![cmd("x"), SnippetStep::RunSnippet { id: Uuid::new_v4() }],
        );
        assert_eq!(
            flatten(std::slice::from_ref(&a), &a),
            Err(PlanError::Missing)
        );
    }

    #[test]
    fn only_pane_actions_may_run() {
        assert_eq!(
            step_action("terminal_clear_scrollback"),
            Some(SnippetAction::ClearScrollback)
        );
        assert_eq!(
            step_action("terminal_paste_and_run"),
            Some(SnippetAction::PasteAndRun)
        );
        assert_eq!(step_action("lock_vault"), None);
        assert_eq!(step_action("close_all_tabs"), None);
        assert_eq!(step_action("from_a_newer_build"), None);
        assert_eq!(allowed_step_actions().len(), 5);
    }

    #[test]
    fn variables_are_asked_once_across_commands() {
        let steps = vec![cmd("deploy {env:prod}"), cmd("tail {env} {lines:100}")];
        assert_eq!(
            plan_placeholders(&steps),
            vec![
                ("env".into(), "prod".into()),
                ("lines".into(), "100".into())
            ]
        );
        let filled = substitute(
            steps,
            &[("env".into(), "qa".into()), ("lines".into(), "5".into())],
        );
        assert_eq!(filled, vec![cmd("deploy qa"), cmd("tail qa 5")]);
    }

    #[test]
    fn a_save_name_never_leaves_the_folder() {
        assert_eq!(
            save_file_name("{host}-{date}.log", "db-01", "2026-10-14").as_deref(),
            Some("db-01-2026-10-14.log")
        );
        assert_eq!(
            save_file_name("../../.bashrc", "h", "d").as_deref(),
            Some("_._.._.bashrc")
        );
        assert_eq!(
            save_file_name("/etc/passwd", "h", "d").as_deref(),
            Some("_etc_passwd")
        );
        assert_eq!(
            save_file_name("C:\\Windows\\x.txt", "h", "d").as_deref(),
            Some("C__Windows_x.txt")
        );
        assert_eq!(save_file_name("-rf", "h", "d").as_deref(), Some("_rf"));
        assert_eq!(save_file_name("..", "h", "d"), None);
        assert_eq!(
            save_file_name("nul.txt", "h", "d").as_deref(),
            Some("_nul.txt")
        );
        assert_eq!(save_file_name("COM1", "h", "d").as_deref(), Some("_COM1"));
        assert_eq!(
            save_file_name("com¹.txt", "h", "d").as_deref(),
            Some("_com¹.txt")
        );
        assert_eq!(save_file_name("LPT³", "h", "d").as_deref(), Some("_LPT³"));
        assert_eq!(save_file_name("COM10", "h", "d").as_deref(), Some("COM10"));
        let cjk = save_file_name(&"日".repeat(200), "h", "d").unwrap();
        assert!(cjk.len() <= 240 && cjk.chars().all(|c| c == '日'));
        assert_eq!(
            save_file_name("console.log", "h", "d").as_deref(),
            Some("console.log")
        );
        assert_eq!(save_file_name("out. . ", "h", "d").as_deref(), Some("out"));
        assert_eq!(save_file_name("   ", "h", "d"), None);
    }
}
