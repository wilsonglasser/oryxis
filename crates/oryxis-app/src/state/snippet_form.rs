//! The snippet editor's fields.
//!
//! A form like the ones in `forms.rs`, and the last one still spread
//! across `Oryxis` as loose fields. `editing_id` travels with it
//! because it is what makes this an edit rather than a create.

use oryxis_core::login_script::ExpectPattern;
use oryxis_core::models::{SaveFileContent, SaveFileMode, SaveFileStep, SnippetStep, SnippetWait};
use uuid::Uuid;

use crate::snippet_run::SnippetAction;

#[derive(Debug)]
pub(crate) struct SnippetForm {
    pub(crate) label: String,
    /// The snippet's actions, in order; one Command row for a new one.
    pub(crate) steps: Vec<StepForm>,
    /// Snippet editor: free-form group name (empty = ungrouped).
    pub(crate) group: String,
    /// Options state for the snippet Group combo (same type-ahead
    /// combo_box as the host editor's Parent Group): filters the
    /// existing snippet groups as you type, still accepts a new name.
    /// Rebuilt whenever the editor opens.
    pub(crate) group_combo: iced::widget::combo_box::State<String>,
    /// Snippet editor: comma-separated tags as typed; parsed
    /// (trim/dedup/drop-empty) on save. Chips are display-only, so
    /// the field stays fully keyboard-editable.
    pub(crate) tags_input: String,
    pub(crate) editing_id: Option<Uuid>,
    /// Snippet editor: the custom run hotkey being edited (parsed form
    /// of `Snippet.hotkey`).
    pub(crate) hotkey: Option<crate::hotkeys::HotkeyBinding>,
    /// True while the snippet editor's shortcut recorder waits for a
    /// key combo (next chord becomes the binding, Esc cancels).
    pub(crate) hotkey_capturing: bool,
    /// "Install script" category toggle (issue #147).
    pub(crate) install: bool,
    pub(crate) error: Option<String>,
}

impl Default for SnippetForm {
    fn default() -> Self {
        Self {
            label: String::new(),
            steps: vec![StepForm::new(StepKind::Command)],
            group: String::new(),
            group_combo: iced::widget::combo_box::State::new(Vec::new()),
            tags_input: String::new(),
            editing_id: None,
            hotkey: None,
            hotkey_capturing: false,
            install: false,
            error: None,
        }
    }
}

/// What kind of action a row of the editor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepKind {
    Command,
    Action,
    WaitCommandEnd,
    WaitOutput,
    WaitTime,
    RunSnippet,
    SaveFile,
    /// A step from a newer build: shown, kept, never edited.
    Unknown,
}

impl StepKind {
    /// The kinds a row may be switched to, in picker order.
    pub(crate) const PICKABLE: [StepKind; 7] = [
        StepKind::Command,
        StepKind::Action,
        StepKind::WaitCommandEnd,
        StepKind::WaitOutput,
        StepKind::WaitTime,
        StepKind::RunSnippet,
        StepKind::SaveFile,
    ];

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            StepKind::Command => "snippet_step_command",
            StepKind::Action => "snippet_step_action",
            StepKind::WaitCommandEnd => "snippet_step_wait_command_end",
            StepKind::WaitOutput => "snippet_step_wait_output",
            StepKind::WaitTime => "snippet_step_wait_time",
            StepKind::RunSnippet => "snippet_step_run_snippet",
            StepKind::SaveFile => "snippet_step_save_file",
            StepKind::Unknown => "snippet_step_unknown",
        }
    }
}

/// One row of the editor. Every kind's fields live side by side, so
/// switching a row's kind back and forth keeps what was typed.
#[derive(Debug)]
pub(crate) struct StepForm {
    pub(crate) kind: StepKind,
    pub(crate) command: iced::widget::text_editor::Content,
    pub(crate) action: Option<SnippetAction>,
    pub(crate) pattern: String,
    pub(crate) regex: bool,
    /// Seconds as typed; empty = the default.
    pub(crate) timeout: String,
    pub(crate) continue_on_error: bool,
    /// Seconds as typed, for a time wait.
    pub(crate) seconds: String,
    pub(crate) snippet: Option<Uuid>,
    pub(crate) file_name: String,
    pub(crate) file_mode: SaveFileMode,
    pub(crate) file_content: SaveFileContent,
    /// The step a newer build wrote, written back as it came.
    pub(crate) unknown: Option<serde_json::Value>,
}

impl StepForm {
    pub(crate) fn new(kind: StepKind) -> Self {
        Self {
            kind,
            command: iced::widget::text_editor::Content::new(),
            action: None,
            pattern: String::new(),
            regex: false,
            timeout: String::new(),
            continue_on_error: false,
            seconds: "1".into(),
            snippet: None,
            file_name: "{host}-{date}.log".into(),
            file_mode: SaveFileMode::Append,
            file_content: SaveFileContent::LastCommand,
            unknown: None,
        }
    }

    pub(crate) fn from_step(step: &SnippetStep) -> Self {
        match step {
            SnippetStep::Command { text } => Self {
                command: iced::widget::text_editor::Content::with_text(text),
                ..Self::new(StepKind::Command)
            },
            SnippetStep::Action { id } => Self {
                action: crate::snippet_run::step_action(id),
                // An id this build does not offer is kept as it came.
                unknown: crate::snippet_run::step_action(id)
                    .is_none()
                    .then(|| serde_json::to_value(step).unwrap_or_default()),
                ..Self::new(if crate::snippet_run::step_action(id).is_some() {
                    StepKind::Action
                } else {
                    StepKind::Unknown
                })
            },
            SnippetStep::Wait(SnippetWait::CommandEnd { timeout_ms, continue_on_error }) => Self {
                timeout: seconds_text(*timeout_ms),
                continue_on_error: *continue_on_error,
                ..Self::new(StepKind::WaitCommandEnd)
            },
            SnippetStep::Wait(SnippetWait::Output { pattern, timeout_ms }) => {
                let (pattern, regex) = match pattern {
                    ExpectPattern::Suffix(s) => (s.clone(), false),
                    ExpectPattern::Regex(r) => (r.clone(), true),
                };
                Self { pattern, regex, timeout: seconds_text(*timeout_ms), ..Self::new(StepKind::WaitOutput) }
            }
            SnippetStep::Wait(SnippetWait::Time { ms }) => {
                Self { seconds: crate::dispatch_snippet_run::format_seconds(*ms), ..Self::new(StepKind::WaitTime) }
            }
            SnippetStep::RunSnippet { id } => Self { snippet: Some(*id), ..Self::new(StepKind::RunSnippet) },
            SnippetStep::SaveFile(f) => Self {
                file_name: f.name.clone(),
                file_mode: f.mode,
                file_content: f.content,
                ..Self::new(StepKind::SaveFile)
            },
            SnippetStep::Unknown(raw) => Self { unknown: Some(raw.clone()), ..Self::new(StepKind::Unknown) },
        }
    }

    /// The step this row describes, or the i18n key of what is wrong.
    pub(crate) fn to_step(&self) -> Result<SnippetStep, &'static str> {
        Ok(match self.kind {
            StepKind::Command => {
                let text = self.command.text().trim_end().to_string();
                if text.trim().is_empty() {
                    return Err("snippet_step_command_empty");
                }
                SnippetStep::Command { text }
            }
            StepKind::Action => SnippetStep::Action {
                id: self.action.ok_or("snippet_step_action_missing")?.id().to_string(),
            },
            StepKind::WaitCommandEnd => SnippetStep::Wait(SnippetWait::CommandEnd {
                timeout_ms: parse_timeout(&self.timeout)?,
                continue_on_error: self.continue_on_error,
            }),
            StepKind::WaitOutput => {
                if self.pattern.trim().is_empty() {
                    return Err("snippet_step_pattern_empty");
                }
                let pattern = if self.regex {
                    if regex::Regex::new(&self.pattern).is_err() {
                        return Err("snippet_step_pattern_invalid");
                    }
                    ExpectPattern::Regex(self.pattern.clone())
                } else {
                    ExpectPattern::Suffix(self.pattern.clone())
                };
                SnippetStep::Wait(SnippetWait::Output { pattern, timeout_ms: parse_timeout(&self.timeout)? })
            }
            StepKind::WaitTime => {
                let ms = parse_seconds(&self.seconds).ok_or("snippet_step_bad_number")?;
                if ms == 0 {
                    return Err("snippet_step_bad_number");
                }
                SnippetStep::Wait(SnippetWait::Time { ms })
            }
            StepKind::RunSnippet => SnippetStep::RunSnippet { id: self.snippet.ok_or("snippet_step_snippet_missing")? },
            StepKind::SaveFile => {
                if crate::snippet_run::save_file_name(&self.file_name, "host", "date").is_none() {
                    return Err("snippet_run_bad_file_name");
                }
                SnippetStep::SaveFile(SaveFileStep {
                    name: self.file_name.trim().to_string(),
                    mode: self.file_mode,
                    content: self.file_content,
                })
            }
            StepKind::Unknown => match &self.unknown {
                Some(raw) => serde_json::from_value(raw.clone()).unwrap_or_else(|_| SnippetStep::Unknown(raw.clone())),
                None => return Err("snippet_step_unknown"),
            },
        })
    }
}

/// The longest wait or timeout the editor takes: one day.
const MAX_WAIT_MS: f64 = 86_400_000.0;

/// Seconds as typed (`1`, `1.5`, `0,5`) to milliseconds.
fn parse_seconds(text: &str) -> Option<u32> {
    let secs: f64 = text.trim().replace(',', ".").parse().ok()?;
    let ms = (secs * 1000.0).round();
    (ms.is_finite() && (0.0..=MAX_WAIT_MS).contains(&ms)).then_some(ms as u32)
}

/// A timeout as typed: empty = the default (`0`).
fn parse_timeout(text: &str) -> Result<u32, &'static str> {
    if text.trim().is_empty() {
        return Ok(0);
    }
    match parse_seconds(text) {
        Some(ms) if ms > 0 => Ok(ms),
        _ => Err("snippet_step_bad_number"),
    }
}

/// A stored timeout as the field shows it: `0` (the default) is empty.
fn seconds_text(ms: u32) -> String {
    if ms == 0 { String::new() } else { crate::dispatch_snippet_run::format_seconds(ms) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_survives_the_form() {
        let steps = vec![
            SnippetStep::Command { text: "make\ndeploy".into() },
            SnippetStep::Action { id: "terminal_copy_screen".into() },
            SnippetStep::Wait(SnippetWait::CommandEnd { timeout_ms: 0, continue_on_error: true }),
            SnippetStep::Wait(SnippetWait::Output { pattern: ExpectPattern::Regex("ok$".into()), timeout_ms: 1500 }),
            SnippetStep::Wait(SnippetWait::Time { ms: 250 }),
            SnippetStep::RunSnippet { id: Uuid::nil() },
            SnippetStep::SaveFile(SaveFileStep {
                name: "{host}.log".into(),
                mode: SaveFileMode::NewFile,
                content: SaveFileContent::Screen,
            }),
            SnippetStep::Unknown(serde_json::json!({"kind": "teleport", "to": "mars"})),
            SnippetStep::Action { id: "from_a_newer_build".into() },
        ];
        for step in steps {
            assert_eq!(StepForm::from_step(&step).to_step().unwrap(), step);
        }
    }

    #[test]
    fn the_form_refuses_what_cannot_run() {
        let mut row = StepForm::new(StepKind::Command);
        assert_eq!(row.to_step(), Err("snippet_step_command_empty"));
        row.kind = StepKind::WaitOutput;
        row.pattern = "(".into();
        row.regex = true;
        assert_eq!(row.to_step(), Err("snippet_step_pattern_invalid"));
        row.kind = StepKind::WaitTime;
        row.seconds = "soon".into();
        assert_eq!(row.to_step(), Err("snippet_step_bad_number"));
        row.seconds = "0,5".into();
        assert_eq!(row.to_step(), Ok(SnippetStep::Wait(SnippetWait::Time { ms: 500 })));
        row.kind = StepKind::WaitCommandEnd;
        row.timeout = "-3".into();
        assert_eq!(row.to_step(), Err("snippet_step_bad_number"));
        row.kind = StepKind::SaveFile;
        row.file_name = "..".into();
        assert_eq!(row.to_step(), Err("snippet_run_bad_file_name"));
    }
}
