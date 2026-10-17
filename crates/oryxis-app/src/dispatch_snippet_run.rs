//! Running a snippet's actions on its pane, one step after another.
//!
//! A run belongs to ONE pane, the active pane of the tab it was fired at,
//! and stays with it while the user moves around: commands are written to
//! that pane only (never broadcast: the waits and saves read one pane,
//! and other panes would drift out of step), actions are aimed at it, the
//! waits read its output. A pane holds one run at a time. The pure half
//! (what may run, nesting, file names) is `snippet_run.rs`.
//!
//! A step that cannot run stops the run and says why; nothing is skipped
//! in silence, because the steps after it were written expecting it.

use iced::Task;
use oryxis_core::models::{
    DEFAULT_WAIT_TIMEOUT_MS, SaveFileContent, SaveFileMode, SaveFileStep, SnippetStep, SnippetWait,
};
use uuid::Uuid;

use crate::app::{Message, Oryxis};
use crate::messages::{SnippetMessage, TerminalMessage};
use crate::snippet_run::{RunMark, RunWait, SnippetAction, SnippetRun};

impl Oryxis {
    /// Start `steps` (already expanded, variables filled) on the active
    /// pane of `tab_idx`.
    pub(crate) fn start_snippet_run(
        &mut self,
        tab_idx: usize,
        label: String,
        steps: Vec<SnippetStep>,
    ) -> Task<Message> {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return Task::none();
        };
        let pane_id = tab.active().id;
        self.start_snippet_run_on_pane(pane_id, label, steps)
    }

    /// Start a run on a given pane: the trigger and startup paths name
    /// the pane the event came from, whichever pane has the focus.
    pub(crate) fn start_snippet_run_on_pane(
        &mut self,
        pane_id: Uuid,
        label: String,
        steps: Vec<SnippetStep>,
    ) -> Task<Message> {
        if let Some(busy) = self.snippet_runs.iter().find(|r| r.pane_id == pane_id) {
            return self
                .show_toast(crate::i18n::t("snippet_run_busy").replace("{name}", &busy.label));
        }
        self.snippet_run_seq += 1;
        let id = self.snippet_run_seq;
        self.snippet_runs.push(SnippetRun {
            id,
            label,
            pane_id,
            steps,
            next: 0,
            generation: 0,
            wait: None,
            timer: None,
            sent_commands: 0,
            at_prompt_at_send: false,
        });
        self.advance_snippet_run(id)
    }

    /// Run steps until one has to wait, or the run is over.
    fn advance_snippet_run(&mut self, run_id: u64) -> Task<Message> {
        let mut tasks = Vec::new();
        while let Some(run) = self.snippet_runs.iter_mut().find(|r| r.id == run_id) {
            let Some(step) = run.steps.get(run.next).cloned() else {
                self.snippet_runs.retain(|r| r.id != run_id);
                break;
            };
            run.next += 1;
            let pane_id = run.pane_id;
            if self.pane_by_id(pane_id).is_none() {
                self.snippet_runs.retain(|r| r.id != run_id);
                break;
            }
            match step {
                SnippetStep::Command { text } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    // Read before the send: the input capture moves the
                    // pane to Busy as the command goes out.
                    let at_prompt = self.pane_by_id(pane_id).is_some_and(|p| {
                        matches!(p.prompt, crate::state::PromptState::AtPrompt { .. })
                    });
                    if !self.write_snippet_command(pane_id, &text) {
                        tasks.push(self.stop_snippet_run(
                            run_id,
                            "snippet_run_pane_unavailable",
                            None,
                        ));
                        break;
                    }
                    if let Some(run) = self.snippet_run_mut(run_id) {
                        if run.sent_commands == 0 {
                            run.at_prompt_at_send = at_prompt;
                        }
                        run.sent_commands += 1;
                    }
                }
                SnippetStep::Action { id } => {
                    let Some(action) = crate::snippet_run::step_action(&id) else {
                        tasks.push(self.stop_snippet_run(
                            run_id,
                            "snippet_run_action_refused",
                            Some(&id),
                        ));
                        break;
                    };
                    use SnippetAction as A;
                    let msg = match action {
                        A::ClearScrollback => {
                            TerminalMessage::TerminalClearScrollback(pane_id)
                        }
                        A::CopyScreen => TerminalMessage::TerminalCopyScreen(pane_id),
                        A::CopyLastCommand => {
                            TerminalMessage::TerminalCopyLastCommand(pane_id)
                        }
                        A::SaveOutput => TerminalMessage::TerminalSaveOutput(pane_id),
                        A::PasteAndRun => {
                            let task = self.arm_snippet_wait(run_id, RunWait::Clipboard, None);
                            let generation = self
                                .snippet_runs
                                .iter()
                                .find(|r| r.id == run_id)
                                .map_or(0, |r| r.generation);
                            tasks.push(task);
                            tasks.push(crate::dispatch_global::read_clipboard_text(move |text| {
                                Message::Snippet(SnippetMessage::SnippetClipboardResolved(
                                    run_id,
                                    generation,
                                    text.map(Into::into),
                                ))
                            }));
                            break;
                        }
                    };
                    tasks.push(self.update(Message::Terminal(msg)));
                    // Save Output asks where the text goes: the next
                    // command waits for the answer instead of being
                    // typed under the dialog.
                    if action == A::SaveOutput && self.save_output.is_some() {
                        tasks.push(self.arm_snippet_wait(run_id, RunWait::Dialog, None));
                        break;
                    }
                }
                SnippetStep::Wait(SnippetWait::CommandEnd {
                    timeout_ms,
                    continue_on_error,
                }) => {
                    let (integrated, busy) = self
                        .pane_by_id(pane_id)
                        .map(|p| {
                            use crate::state::PromptState as P;
                            (
                                !matches!(p.prompt, P::NoIntegration),
                                matches!(p.prompt, P::Busy),
                            )
                        })
                        .unwrap_or((false, false));
                    // A command this run just sent is waited for even
                    // before the shell has marked anything: at the start
                    // of a session the first prompt (and its marks) may
                    // not have arrived yet. A shell that never marks lets
                    // the timeout say so.
                    let (sent, at_prompt_at_send) = self
                        .snippet_run_mut(run_id)
                        .map(|r| (r.sent_commands, r.at_prompt_at_send))
                        .unwrap_or((0, false));
                    if sent == 0 && !(integrated && busy) {
                        // Nothing is running: there is no end to wait for.
                        continue;
                    }
                    // D marks belong to the run's commands at once when
                    // the shell sat at its prompt as the first went out,
                    // or when the subject is the command already running.
                    let ready = if sent > 0 { at_prompt_at_send } else { busy };
                    tasks.push(self.arm_snippet_wait(
                        run_id,
                        RunWait::CommandEnd {
                            continue_on_error,
                            ready,
                        },
                        Some(timeout_ms),
                    ));
                    break;
                }
                SnippetStep::Wait(SnippetWait::Output {
                    pattern,
                    timeout_ms,
                }) => match oryxis_core::login_script::OutputMatcher::new(&pattern) {
                    Ok(matcher) => {
                        tasks.push(self.arm_snippet_wait(
                            run_id,
                            RunWait::Output(matcher),
                            Some(timeout_ms),
                        ));
                        break;
                    }
                    Err(_) => {
                        tasks.push(self.stop_snippet_run(run_id, "snippet_run_bad_pattern", None));
                        break;
                    }
                },
                SnippetStep::Wait(SnippetWait::Time { ms }) => {
                    tasks.push(self.arm_snippet_wait(run_id, RunWait::Timer, Some(ms.max(1))));
                    break;
                }
                SnippetStep::SaveFile(file) => {
                    tasks.push(self.arm_snippet_wait(run_id, RunWait::File, None));
                    match self.snippet_save_task(run_id, pane_id, &file) {
                        Ok(task) => tasks.push(task),
                        Err(key) => tasks.push(self.stop_snippet_run(run_id, key, None)),
                    }
                    break;
                }
                // Expanded before the run started (`snippet_run::flatten`).
                SnippetStep::RunSnippet { .. } => continue,
                SnippetStep::Unknown(_) => {
                    tasks.push(self.stop_snippet_run(run_id, "snippet_run_unknown_step", None));
                    break;
                }
            }
        }
        Task::batch(tasks)
    }

    fn snippet_run_mut(&mut self, run_id: u64) -> Option<&mut SnippetRun> {
        self.snippet_runs.iter_mut().find(|r| r.id == run_id)
    }

    /// Park the run on `wait`; a wait with a timeout arms its timer (a
    /// time wait's timer is its duration). `Some(0)` is the default
    /// timeout.
    fn arm_snippet_wait(
        &mut self,
        run_id: u64,
        wait: RunWait,
        timeout_ms: Option<u32>,
    ) -> Task<Message> {
        let Some(run) = self.snippet_run_mut(run_id) else {
            return Task::none();
        };
        run.generation += 1;
        run.wait = Some(wait);
        run.timer = None;
        match timeout_ms {
            Some(ms) => {
                let ms = if ms == 0 { DEFAULT_WAIT_TIMEOUT_MS } else { ms };
                let (task, handle) =
                    snippet_run_timer(run_id, run.generation, u64::from(ms)).abortable();
                run.timer = Some(handle.abort_on_drop());
                task
            }
            None => Task::none(),
        }
    }

    /// The wait is over: go on with the next step.
    fn resume_snippet_run(&mut self, run_id: u64) -> Task<Message> {
        if let Some(run) = self.snippet_run_mut(run_id) {
            run.wait = None;
            run.timer = None;
            run.generation += 1;
            run.sent_commands = 0;
            run.at_prompt_at_send = false;
        }
        self.advance_snippet_run(run_id)
    }

    /// Finish a clipboard-and-run action after the paste guard, if any, has
    /// been confirmed. The newline is sent separately so bracketed paste
    /// remains intact.
    pub(crate) fn finish_snippet_clipboard_paste(&mut self, tab_id: Uuid) -> Task<Message> {
        let Some((run_id, pane_id, execute)) = self.snippet_runs.iter().find_map(|run| {
            let tab = self
                .pane_tab_index(run.pane_id)
                .and_then(|i| self.tabs.get(i))?;
            (tab._id == tab_id).then_some((
                run.id,
                run.pane_id,
                matches!(run.wait, Some(RunWait::Clipboard)),
            ))
        }) else {
            return Task::none();
        };
        if execute && let Some(tab_idx) = self.pane_tab_index(pane_id) {
            self.write_input_to_tab(tab_idx, b"\n");
        }
        self.resume_snippet_run(run_id)
    }

    pub(crate) fn cancel_snippet_clipboard_paste(&mut self, tab_id: Uuid) -> Task<Message> {
        let run_id = self.snippet_runs.iter().find_map(|run| {
            let tab = self
                .pane_tab_index(run.pane_id)
                .and_then(|i| self.tabs.get(i))?;
            (tab._id == tab_id && matches!(run.wait, Some(RunWait::Clipboard)))
                .then_some(run.id)
        });
        run_id.map_or_else(Task::none, |id| {
            self.stop_snippet_run(id, "snippet_run_paste_cancelled", None)
        })
    }

    /// End the run and say why (`key` an i18n key, `{step}` its detail).
    fn stop_snippet_run(&mut self, run_id: u64, key: &str, detail: Option<&str>) -> Task<Message> {
        let Some(run) = self.snippet_runs.iter().find(|r| r.id == run_id) else {
            return Task::none();
        };
        let label = run.label.clone();
        let step = run.next;
        self.snippet_runs.retain(|r| r.id != run_id);
        let why = crate::i18n::t(key).replace("{detail}", detail.unwrap_or(""));
        self.show_toast_secs(
            crate::i18n::t("snippet_run_stopped")
                .replace("{name}", &label)
                .replace("{step}", &step.to_string())
                .replace("{why}", &why),
            6,
        )
    }

    /// Type one command into the run's pane and press Enter: bracketed
    /// when the shell asked for it, the newline outside the bracket, as
    /// the snippet of old is sent. `false` when the pane cannot take it
    /// (gone, showing its Files, owned by a ZMODEM transfer).
    fn write_snippet_command(&mut self, pane_id: Uuid, text: &str) -> bool {
        let Some(tab_idx) = self.pane_tab_index(pane_id) else {
            return false;
        };
        if self.tabs.get(tab_idx).is_some_and(|t| t.files_mode) {
            return false;
        }
        let snap = self.prefs.scrollback_reset_keypress && self.cur_active_tab() == Some(tab_idx);
        // A typed command ends whatever a login script was waiting for,
        // like the user's own keystroke would.
        self.abort_login_script(pane_id);
        let Some(pane) = self
            .tabs
            .get_mut(tab_idx)
            .and_then(|t| t.pane_by_id_mut(pane_id))
        else {
            return false;
        };
        if pane.zmodem.is_some() {
            return false;
        }
        let bracketed = pane
            .terminal
            .lock()
            .map(|s| s.bracketed_paste_enabled())
            .unwrap_or(false);
        let mut payload = oryxis_terminal::wrap_paste(text, bracketed);
        payload.push(b'\n');
        Self::write_bytes_to_pane(pane, &payload, snap);
        // The command history reads the ACTIVE pane's input; a run on a
        // pane that has lost the focus still runs, it is just not recorded.
        if self
            .tabs
            .get(tab_idx)
            .is_some_and(|t| t.active().id == pane_id)
        {
            self.feed_input_capture(tab_idx, &payload);
        }
        true
    }

    /// Read what a save step writes, now, and hand the write to a blocking
    /// task. `Err` = an i18n key saying why nothing can be written.
    fn snippet_save_task(
        &mut self,
        run_id: u64,
        pane_id: Uuid,
        file: &SaveFileStep,
    ) -> Result<Task<Message>, &'static str> {
        let pane = self
            .pane_by_id(pane_id)
            .ok_or("snippet_run_pane_unavailable")?;
        let text = {
            let state = pane
                .terminal
                .lock()
                .map_err(|_| "snippet_run_pane_unavailable")?;
            match file.content {
                SaveFileContent::Screen => state.visible_text(),
                SaveFileContent::Buffer => state.all_text(),
                SaveFileContent::LastCommand => state
                    .last_command()
                    .map_err(|_| "snippet_run_no_last_command")?
                    .to_text(),
            }
        };
        let label = pane.label.clone();
        let host = self.privacy_display_label(&label, &label, &self.privacy_terms());
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let name = crate::snippet_run::save_file_name(&file.name, &host, &date)
            .ok_or("snippet_run_bad_file_name")?;
        let dir = self.snippet_output_dir();
        let mode = file.mode;
        let generation = self
            .snippet_runs
            .iter()
            .find(|r| r.id == run_id)
            .map_or(0, |r| r.generation);
        Ok(Task::perform(
            tokio::task::spawn_blocking(move || write_snippet_output(&dir, &name, mode, &text)),
            move |res| {
                let outcome = res.unwrap_or_else(|e| Err(e.to_string()));
                Message::Snippet(SnippetMessage::SnippetRunFileWritten(
                    run_id, generation, outcome,
                ))
            },
        ))
    }

    /// Where save steps write: the setting, else `<Documents>/Oryxis`,
    /// else `<home>/Oryxis`.
    pub(crate) fn snippet_output_dir(&self) -> std::path::PathBuf {
        match &self.prefs.snippet_output_dir {
            Some(dir) => std::path::PathBuf::from(dir),
            None => dirs::document_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("Oryxis"),
        }
    }

    /// The output wait on `pane_id` this batch belongs to, read BEFORE the
    /// batch is processed: (run, generation). The batch is fed to it only
    /// after the emulator drew it (`snippet_run_output`), so an action
    /// right after the match (Copy Screen, a save) sees the matched text;
    /// a wait that ended or changed in between (the same batch's marks
    /// resumed the run) is not fed output that predates it.
    pub(crate) fn snippet_output_wait(&self, pane_id: Uuid) -> Option<(u64, u64)> {
        self.snippet_runs
            .iter()
            .find(|r| r.pane_id == pane_id && matches!(r.wait, Some(RunWait::Output(_))))
            .map(|r| (r.id, r.generation))
    }

    /// `snippet_output_wait` for a run that sent nothing before arming
    /// its output wait: a run started inside the batch reads that batch
    /// only then.
    pub(crate) fn snippet_output_wait_unsent(&self, pane_id: Uuid) -> Option<(u64, u64)> {
        self.snippet_runs
            .iter()
            .find(|r| {
                r.pane_id == pane_id
                    && r.sent_commands == 0
                    && matches!(r.wait, Some(RunWait::Output(_)))
            })
            .map(|r| (r.id, r.generation))
    }

    /// One output batch of `pane_id` (the cleaned bytes the emulator
    /// got), for the output wait `wait` (`snippet_output_wait`).
    pub(crate) fn snippet_run_output(
        &mut self,
        pane_id: Uuid,
        bytes: &[u8],
        wait: (u64, u64),
    ) -> Task<Message> {
        let Some(run) = self
            .snippet_runs
            .iter_mut()
            .find(|r| r.pane_id == pane_id && (r.id, r.generation) == wait)
        else {
            return Task::none();
        };
        let Some(RunWait::Output(matcher)) = run.wait.as_mut() else {
            return Task::none();
        };
        if matcher.feed(bytes) {
            let id = run.id;
            return self.resume_snippet_run(id);
        }
        Task::none()
    }

    /// The shell-integration marks one batch of `pane_id` carried, in
    /// order, after the emulator processed it. A command-end wait takes
    /// the D of ITS command (`command_end_in`); a failure stops the run
    /// unless the wait says to go on (Ctrl+C on the command is then also
    /// how a run is stopped).
    pub(crate) fn snippet_run_marks(&mut self, pane_id: Uuid, marks: &[RunMark]) -> Task<Message> {
        let Some(run) = self.snippet_runs.iter_mut().find(|r| r.pane_id == pane_id) else {
            return Task::none();
        };
        let Some(RunWait::CommandEnd {
            continue_on_error,
            ready,
        }) = run.wait.as_mut()
        else {
            return Task::none();
        };
        let continue_on_error = *continue_on_error;
        let Some(exit) = crate::snippet_run::command_end_in(ready, marks) else {
            return Task::none();
        };
        let id = run.id;
        match exit {
            Some(code) if code != 0 && !continue_on_error => {
                self.stop_snippet_run(id, "snippet_run_command_failed", Some(&code.to_string()))
            }
            _ => self.resume_snippet_run(id),
        }
    }

    /// Once per update: drop the runs whose pane is gone (closed, its tab
    /// torn down), and resume a run whose Save Output dialog has closed,
    /// whichever way it closed (a destination, Cancel, Esc).
    pub(crate) fn snippet_runs_housekeeping(&mut self) -> Vec<Task<Message>> {
        if self.snippet_runs.is_empty() {
            return Vec::new();
        }
        let alive: Vec<u64> = self
            .snippet_runs
            .iter()
            .filter(|r| self.pane_by_id(r.pane_id).is_some())
            .map(|r| r.id)
            .collect();
        self.snippet_runs.retain(|r| alive.contains(&r.id));
        if self.save_output.is_some() {
            return Vec::new();
        }
        let answered: Vec<u64> = self
            .snippet_runs
            .iter()
            .filter(|r| matches!(r.wait, Some(RunWait::Dialog)))
            .map(|r| r.id)
            .collect();
        answered
            .into_iter()
            .map(|id| self.resume_snippet_run(id))
            .collect()
    }

    /// The run on `pane_id`, for the status line: (label, step, steps).
    pub(crate) fn snippet_run_progress(&self, pane_id: Uuid) -> Option<(u64, &str, usize, usize)> {
        self.snippet_runs
            .iter()
            .find(|r| r.pane_id == pane_id)
            .map(|r| (r.id, r.label.as_str(), r.next.max(1), r.steps.len()))
    }

    pub(crate) fn handle_snippet_run(
        &mut self,
        message: SnippetMessage,
    ) -> Result<Task<Message>, SnippetMessage> {
        match message {
            SnippetMessage::SnippetRunTimer(run_id, generation) => {
                let Some(run) = self.snippet_runs.iter().find(|r| r.id == run_id) else {
                    return Ok(Task::none());
                };
                if run.generation != generation {
                    return Ok(Task::none());
                }
                Ok(match run.wait {
                    Some(RunWait::Timer) => self.resume_snippet_run(run_id),
                    Some(RunWait::CommandEnd { .. }) => {
                        // No mark ever seen on the pane: the shell does
                        // not report command ends, which is the useful
                        // thing to say.
                        let integrated = self.pane_by_id(run.pane_id).is_some_and(|p| {
                            !matches!(p.prompt, crate::state::PromptState::NoIntegration)
                        });
                        let key = if integrated {
                            "snippet_run_timed_out"
                        } else {
                            "snippet_run_no_integration"
                        };
                        self.stop_snippet_run(run_id, key, None)
                    }
                    Some(RunWait::Output(_)) => {
                        self.stop_snippet_run(run_id, "snippet_run_timed_out", None)
                    }
                    Some(RunWait::File)
                    | Some(RunWait::Dialog)
                    | Some(RunWait::Clipboard)
                    | None => Task::none(),
                })
            }
            SnippetMessage::SnippetClipboardResolved(run_id, generation, text) => {
                let Some(pane_id) = self.snippet_runs.iter().find_map(|r| {
                    (r.id == run_id
                        && r.generation == generation
                        && matches!(r.wait, Some(RunWait::Clipboard)))
                        .then_some(r.pane_id)
                }) else {
                    return Ok(Task::none());
                };
                let Some(tab_id) = self
                    .pane_tab_index(pane_id)
                    .and_then(|i| self.tabs.get(i))
                    .map(|tab| tab._id)
                else {
                    return Ok(self.stop_snippet_run(run_id, "snippet_run_pane_unavailable", None));
                };
                let Some(text) = text
                    .map(crate::messages::Redacted::into_inner)
                    .filter(|t| !t.is_empty())
                else {
                    return Ok(self.stop_snippet_run(run_id, "snippet_run_clipboard_empty", None));
                };
                self.paste_text_into_tab(tab_id, &text);
                if self
                    .pending_paste
                    .as_ref()
                    .is_some_and(|(id, _)| *id == tab_id)
                {
                    return Ok(Task::none());
                }
                Ok(self.finish_snippet_clipboard_paste(tab_id))
            }
            SnippetMessage::SnippetRunFileWritten(run_id, generation, outcome) => {
                let current = self.snippet_runs.iter().any(|r| {
                    r.id == run_id
                        && r.generation == generation
                        && matches!(r.wait, Some(RunWait::File))
                });
                match outcome {
                    Ok(path) => {
                        let toast = self.show_toast(
                            crate::i18n::t("snippet_run_saved").replace("{path}", &path),
                        );
                        if current {
                            return Ok(Task::batch([toast, self.resume_snippet_run(run_id)]));
                        }
                        Ok(toast)
                    }
                    Err(e) if current => {
                        Ok(self.stop_snippet_run(run_id, "snippet_run_write_failed", Some(&e)))
                    }
                    Err(e) => Ok(self.show_toast(
                        crate::i18n::t("terminal_save_output_failed").replace("{error}", &e),
                    )),
                }
            }
            SnippetMessage::CancelSnippetRun(run_id) => {
                self.snippet_runs.retain(|r| r.id != run_id);
                Ok(Task::none())
            }
            other => Err(other),
        }
    }
}

/// Wake the run `run_id` at `generation` after `ms`.
fn snippet_run_timer(run_id: u64, generation: u64, ms: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        },
        move |_| Message::Snippet(SnippetMessage::SnippetRunTimer(run_id, generation)),
    )
}

/// Write `text` as `name` inside `dir` (created when missing). Append adds
/// to the end; NewFile never replaces a file, it takes `name-1`, `name-2`,
/// ... (atomically: `create_new`). Answers the path written.
fn write_snippet_output(
    dir: &std::path::Path,
    name: &str,
    mode: SaveFileMode,
    text: &str,
) -> Result<String, String> {
    use std::io::Write as _;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut body = text.to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    let (mut file, path) = match mode {
        SaveFileMode::Append => {
            let path = dir.join(name);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            (file, path)
        }
        SaveFileMode::NewFile => {
            let (stem, ext) = match name.rsplit_once('.') {
                Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
                _ => (name.to_string(), String::new()),
            };
            let mut found = None;
            for n in 0..10_000u32 {
                let candidate = match n {
                    0 => dir.join(name),
                    n => dir.join(format!("{stem}-{n}{ext}")),
                };
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&candidate)
                {
                    Ok(file) => {
                        found = Some((file, candidate));
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e.to_string()),
                }
            }
            found.ok_or_else(|| "no free file name".to_string())?
        }
    };
    file.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// What a toast says when a snippet's actions cannot be laid out.
pub(crate) fn plan_error_text(label: &str, error: &crate::snippet_run::PlanError) -> String {
    use crate::snippet_run::PlanError as E;
    match error {
        E::Cycle(name) => crate::i18n::t("snippet_run_cycle").replace("{name}", name),
        E::Missing => crate::i18n::t("snippet_run_missing").replace("{name}", label),
        E::TooLong => crate::i18n::t("snippet_run_too_long").replace("{name}", label),
        E::Install(name) => crate::i18n::t("snippet_run_nested_install").replace("{name}", name),
    }
}

impl Oryxis {
    /// One line saying what `step` does, for the trigger confirmation and
    /// the editor: a command as typed, everything else in words.
    pub(crate) fn describe_snippet_step(&self, step: &SnippetStep) -> String {
        let t = crate::i18n::t;
        match step {
            SnippetStep::Command { text } => format!("$ {text}"),
            SnippetStep::Action { id } => {
                let name = crate::snippet_run::step_action(id)
                    .map(|a| t(a.label_key()).to_string())
                    .unwrap_or_else(|| id.clone());
                t("snippet_step_action_line").replace("{action}", &name)
            }
            SnippetStep::Wait(SnippetWait::CommandEnd { .. }) => {
                t("snippet_step_wait_command_end").to_string()
            }
            SnippetStep::Wait(SnippetWait::Output { pattern, .. }) => {
                let shown = match pattern {
                    oryxis_core::login_script::ExpectPattern::Suffix(s) => s.clone(),
                    oryxis_core::login_script::ExpectPattern::Regex(r) => format!("/{r}/"),
                };
                t("snippet_wait_output_line").replace("{pattern}", &shown)
            }
            SnippetStep::Wait(SnippetWait::Time { ms }) => {
                t("snippet_wait_time_line").replace("{seconds}", &format_seconds(*ms))
            }
            SnippetStep::RunSnippet { id } => {
                let name = self
                    .snippets
                    .iter()
                    .find(|s| s.id == *id)
                    .map_or_else(|| "?".to_string(), |s| s.label.clone());
                t("snippet_step_run_line").replace("{name}", &name)
            }
            SnippetStep::SaveFile(file) => t("snippet_step_save_line")
                .replace("{content}", t(save_content_key(file.content)))
                .replace("{name}", &file.name),
            SnippetStep::Unknown(_) => t("snippet_step_unknown").to_string(),
        }
    }

    /// Every step, one per line.
    pub(crate) fn describe_snippet_steps(&self, steps: &[SnippetStep]) -> String {
        steps
            .iter()
            .map(|s| self.describe_snippet_step(s))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The i18n key naming what a save step writes.
pub(crate) fn save_content_key(content: SaveFileContent) -> &'static str {
    match content {
        SaveFileContent::Screen => "snippet_save_content_screen",
        SaveFileContent::Buffer => "snippet_save_content_buffer",
        SaveFileContent::LastCommand => "snippet_save_content_last_command",
    }
}

/// Milliseconds as seconds, exact and without trailing zeros:
/// `1500` -> `1.5`, `2000` -> `2`, `250` -> `0.25`.
pub(crate) fn format_seconds(ms: u32) -> String {
    let (whole, frac) = (ms / 1000, ms % 1000);
    if frac == 0 {
        return whole.to_string();
    }
    let frac = format!("{frac:03}");
    format!("{whole}.{}", frac.trim_end_matches('0'))
}

impl Oryxis {
    /// A startup snippet's run, nested snippets expanded; `None` (and a
    /// toast) when it cannot be laid out.
    pub(crate) fn startup_snippet_run(
        &mut self,
        snip: &oryxis_core::models::Snippet,
    ) -> Option<(String, Vec<SnippetStep>)> {
        match crate::snippet_run::flatten(&self.snippets, snip) {
            Ok(steps) => Some((snip.label.clone(), steps)),
            Err(e) => {
                let text = plan_error_text(&snip.label, &e);
                self.set_toast(text);
                None
            }
        }
    }

    /// Start the startup runs a finished login script handed over.
    pub(crate) fn start_due_startup_runs(&mut self) -> Task<Message> {
        if self.startup_runs_due.is_empty() {
            return Task::none();
        }
        let due = std::mem::take(&mut self.startup_runs_due);
        Task::batch(
            due.into_iter()
                .map(|(pane_id, label, steps)| {
                    self.start_snippet_run_on_pane(pane_id, label, steps)
                })
                .collect::<Vec<_>>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_file_never_replaces_one() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_snippet_output(dir.path(), "out.log", SaveFileMode::NewFile, "one").unwrap();
        let b = write_snippet_output(dir.path(), "out.log", SaveFileMode::NewFile, "two").unwrap();
        assert!(a.ends_with("out.log"));
        assert!(b.ends_with("out-1.log"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "two\n");
    }

    #[test]
    fn seconds_read_back_exactly() {
        assert_eq!(format_seconds(250), "0.25");
        assert_eq!(format_seconds(1500), "1.5");
        assert_eq!(format_seconds(2000), "2");
        assert_eq!(format_seconds(60_001), "60.001");
    }

    #[test]
    fn append_adds_to_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("made");
        write_snippet_output(&sub, "log", SaveFileMode::Append, "one").unwrap();
        let p = write_snippet_output(&sub, "log", SaveFileMode::Append, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(p).unwrap(), "one\ntwo\n");
    }
}
