//! The snippet editor's list of actions, shared by the vault panel and
//! the sidebar editor. Each row is a card with the same skeleton (number,
//! kind picker, move / remove, then the kind's own fields), so a row that
//! changes kind keeps its place and the rows around it keep theirs.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, pick_list, text, text_editor, text_input, Space};
use iced::{Background, Border, Color, Element, Length};
use oryxis_core::models::{SaveFileContent, SaveFileMode};
use uuid::Uuid;

use crate::app::{Message, Oryxis, SnippetMessage};
use crate::i18n::t;
use crate::keynav::RowAction;
use crate::messages::NavigationMessage;
use crate::state::{StepForm, StepKind};
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

/// The timeout field's placeholder: the default it stands for.
static DEFAULT_TIMEOUT_TEXT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    crate::dispatch_snippet_run::format_seconds(oryxis_core::models::DEFAULT_WAIT_TIMEOUT_MS)
});

const SAVE_MODES: [SaveFileMode; 2] = [SaveFileMode::Append, SaveFileMode::NewFile];
const SAVE_CONTENTS: [SaveFileContent; 3] =
    [SaveFileContent::LastCommand, SaveFileContent::Buffer, SaveFileContent::Screen];

fn mode_key(mode: SaveFileMode) -> &'static str {
    match mode {
        SaveFileMode::Append => "snippet_save_mode_append",
        SaveFileMode::NewFile => "snippet_save_mode_new_file",
    }
}

fn snip(m: SnippetMessage) -> Message {
    Message::Snippet(m)
}

impl Oryxis {
    /// Every row plus the "Add action" button. `nav` records the controls
    /// as keyboard rows (the vault panel does; the sidebar editor records
    /// none, like the rest of its form).
    pub(crate) fn snippet_steps_editor(&self, nav: bool) -> Element<'_, Message> {
        let rows = &self.snippet_form.steps;
        let mut col = column![].width(Length::Fill);
        for (i, row) in rows.iter().enumerate() {
            col = col.push(self.snippet_step_card(i, row, rows.len(), nav));
            col = col.push(Space::new().height(8).boxed());
        }
        let add = self.step_slot(
            nav,
            RowAction::activate(snip(SnippetMessage::SnippetStepAdd)),
            8.0,
            crate::widgets::styled_button_opt(
                t("snippet_add_action"),
                Some(snip(SnippetMessage::SnippetStepAdd)),
                OryxisColors::t().accent,
            ),
        );
        col.push(add).boxed()
    }

    /// `el` recorded as a keyboard row when `nav`, else as it is.
    fn step_slot<'a>(
        &self,
        nav: bool,
        action: RowAction,
        radius: f32,
        el: Element<'a, Message>,
    ) -> Element<'a, Message> {
        if nav { self.panel_nav_slot(action, radius, el) } else { el }
    }

    fn snippet_step_card<'a>(&'a self, i: usize, row: &'a StepForm, total: usize, nav: bool) -> Element<'a, Message> {
        let c = OryxisColors::t();
        let pad = if nav { 10 } else { 8 };
        let size = if nav { 14.0 } else { 13.0 };

        // The kind picker. A step from a newer build shows its own name
        // and can only be replaced by picking a kind this build knows.
        let kinds = StepKind::PICKABLE.to_vec();
        let (prev, next) = crate::keynav::slots::cycle_pair(&kinds, &row.kind, move |k| {
            snip(SnippetMessage::SnippetStepKindPicked(i, k))
        });
        let kind_picker = self.step_slot(
            nav,
            RowAction::picker(prev, next),
            crate::widgets::INPUT_RADIUS,
            pick_list(Some(row.kind), kinds, |k: &StepKind| t(k.label_key()).to_string())
                .on_select(move |k| snip(SnippetMessage::SnippetStepKindPicked(i, k)))
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(Length::Fill)
                .padding(pad)
                .text_size(size)
                .style(crate::widgets::rounded_pick_list_style)
                .boxed(),
        );
        let up = (i > 0).then(|| snip(SnippetMessage::SnippetStepMoveUp(i)));
        let down = (i + 1 < total).then(|| snip(SnippetMessage::SnippetStepMoveDown(i)));
        let remove = (total > 1).then(|| snip(SnippetMessage::SnippetStepRemove(i)));
        let mut header = vec![
            text(format!("{}.", i + 1)).size(12).color(c.text_muted).boxed(),
            Space::new().width(6).boxed(),
            kind_picker,
            Space::new().width(4).boxed(),
        ];
        for (icon, msg, tip) in [
            (iced_fonts::lucide::arrow_up(), up, t("snippet_step_move_up")),
            (iced_fonts::lucide::arrow_down(), down, t("snippet_step_move_down")),
            (iced_fonts::lucide::x(), remove, t("snippet_step_remove")),
        ] {
            let btn = crate::views::terminal::icon_tooltip(step_icon_btn(icon, msg.clone()), tip);
            header.push(match msg {
                Some(m) => self.step_slot(nav, RowAction::activate(m), 4.0, btn),
                None => btn,
            });
        }
        let header = dir_row(header).align_y(iced::Alignment::Center);

        let body = self.snippet_step_body(i, row, nav, pad, size);
        container(column![header, Space::new().height(8), body].width(Length::Fill))
            .padding(10)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(8.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            })
            .boxed()
    }

    fn snippet_step_body<'a>(
        &'a self,
        i: usize,
        row: &'a StepForm,
        nav: bool,
        pad: u16,
        size: f32,
    ) -> Element<'a, Message> {
        let c = OryxisColors::t();
        let hint = |s: String| text(s).size(11).color(OryxisColors::t().text_muted).boxed();
        let field_id = |suffix: &str| iced::widget::Id::from(format!("panel-snippet-step-{i}{suffix}"));
        let input = |placeholder: &'a str, value: &'a str, suffix: &str, on: fn(usize, String) -> SnippetMessage| {
            let id = field_id(suffix);
            self.step_slot(
                nav,
                RowAction::input(id.clone()),
                10.0,
                text_input(placeholder, value)
                    .id(id)
                    .on_input(move |v| snip(on(i, v)))
                    .padding(pad)
                    .size(size)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(crate::widgets::dir_align_x())
                    .boxed(),
            )
        };
        let toggle = |label: &'a str, value: bool, msg: SnippetMessage| {
            self.step_slot(
                nav,
                RowAction::activate(snip(msg.clone())),
                10.0,
                crate::widgets::toggle_row(label, value, snip(msg)),
            )
        };
        let timeout = |row: &'a StepForm| {
            crate::widgets::panel_field(
                t("snippet_step_timeout"),
                input(
                    DEFAULT_TIMEOUT_TEXT.as_str(),
                    &row.timeout,
                    "-timeout",
                    SnippetMessage::SnippetStepTimeoutChanged,
                ),
            )
        };

        match row.kind {
            StepKind::Command => {
                let id = field_id("");
                self.step_slot(
                    nav,
                    RowAction::input(id.clone()),
                    10.0,
                    container(
                        text_editor(&row.command)
                            .id(id)
                            .placeholder("sudo systemctl restart nginx")
                            .on_action(move |a| snip(SnippetMessage::SnippetStepCommandAction(i, a)))
                            .padding(pad)
                            .size(size)
                            .height(Length::Shrink)
                            .style(crate::widgets::rounded_editor_style),
                    )
                    .height(Length::Shrink.max(if nav { 240.0 } else { 180.0 }))
                    .boxed(),
                )
            }
            StepKind::Action => {
                let actions = crate::snippet_run::allowed_step_actions().to_vec();
                let (prev, next) = match row.action {
                    Some(a) => crate::keynav::slots::cycle_pair(&actions, &a, move |a| {
                        snip(SnippetMessage::SnippetStepActionPicked(i, a))
                    }),
                    None => {
                        let first = Some(snip(SnippetMessage::SnippetStepActionPicked(i, actions[0])));
                        (first.clone(), first)
                    }
                };
                self.step_slot(
                    nav,
                    RowAction::picker(prev, next),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(row.action, actions, |a: &crate::snippet_run::SnippetAction| t(a.label_key()).to_string())
                        .placeholder(t("snippet_step_pick_action"))
                        .on_select(move |a| snip(SnippetMessage::SnippetStepActionPicked(i, a)))
                        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                        .width(Length::Fill)
                        .padding(pad)
                        .text_size(size)
                        .style(crate::widgets::rounded_pick_list_style)
                        .boxed(),
                )
            }
            StepKind::WaitCommandEnd => column![
                timeout(row),
                Space::new().height(8),
                toggle(t("snippet_step_continue_on_error"), row.continue_on_error, SnippetMessage::SnippetStepToggleContinue(i)),
                Space::new().height(6),
                hint(t("snippet_step_command_end_hint").to_string()),
            ]
            .boxed(),
            StepKind::WaitOutput => column![
                input(t("snippet_step_pattern_placeholder"), &row.pattern, "", SnippetMessage::SnippetStepPatternChanged),
                Space::new().height(8),
                toggle(t("snippet_step_regex"), row.regex, SnippetMessage::SnippetStepToggleRegex(i)),
                Space::new().height(8),
                timeout(row),
                Space::new().height(6),
                hint(t("snippet_step_output_hint").to_string()),
            ]
            .boxed(),
            StepKind::WaitTime => crate::widgets::panel_field(
                t("snippet_step_seconds"),
                input("1", &row.seconds, "", SnippetMessage::SnippetStepSecondsChanged),
            ),
            StepKind::RunSnippet => {
                let others: Vec<Uuid> = self
                    .snippets
                    .iter()
                    .filter(|s| Some(s.id) != self.snippet_form.editing_id && !s.install)
                    .map(|s| s.id)
                    .collect();
                if others.is_empty() {
                    return hint(t("snippet_step_no_other_snippets").to_string());
                }
                let (prev, next) = match row.snippet.filter(|id| others.contains(id)) {
                    Some(id) => crate::keynav::slots::cycle_pair(&others, &id, move |id| {
                        snip(SnippetMessage::SnippetStepSnippetPicked(i, id))
                    }),
                    None => {
                        let first = Some(snip(SnippetMessage::SnippetStepSnippetPicked(i, others[0])));
                        (first.clone(), first)
                    }
                };
                let label_of = move |id: &Uuid| {
                    self.snippets
                        .iter()
                        .find(|s| s.id == *id)
                        .map_or_else(|| "?".to_string(), |s| s.label.clone())
                };
                self.step_slot(
                    nav,
                    RowAction::picker(prev, next),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(row.snippet, others, label_of)
                        .placeholder(t("snippet_step_pick_snippet"))
                        .on_select(move |id| snip(SnippetMessage::SnippetStepSnippetPicked(i, id)))
                        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                        .width(Length::Fill)
                        .padding(pad)
                        .text_size(size)
                        .style(crate::widgets::rounded_pick_list_style)
                        .boxed(),
                )
            }
            StepKind::SaveFile => {
                let contents = SAVE_CONTENTS.to_vec();
                let (cprev, cnext) = crate::keynav::slots::cycle_pair(&contents, &row.file_content, move |v| {
                    snip(SnippetMessage::SnippetStepSaveContentPicked(i, v))
                });
                let content_picker = self.step_slot(
                    nav,
                    RowAction::picker(cprev, cnext),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(Some(row.file_content), contents, |v: &SaveFileContent| {
                        t(crate::dispatch_snippet_run::save_content_key(*v)).to_string()
                    })
                    .on_select(move |v| snip(SnippetMessage::SnippetStepSaveContentPicked(i, v)))
                    .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                    .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                    .width(Length::Fill)
                    .padding(pad)
                    .text_size(size)
                    .style(crate::widgets::rounded_pick_list_style)
                    .boxed(),
                );
                let modes = SAVE_MODES.to_vec();
                let (mprev, mnext) = crate::keynav::slots::cycle_pair(&modes, &row.file_mode, move |v| {
                    snip(SnippetMessage::SnippetStepSaveModePicked(i, v))
                });
                let mode_picker = self.step_slot(
                    nav,
                    RowAction::picker(mprev, mnext),
                    crate::widgets::INPUT_RADIUS,
                    pick_list(Some(row.file_mode), modes, |v: &SaveFileMode| t(mode_key(*v)).to_string())
                        .on_select(move |v| snip(SnippetMessage::SnippetStepSaveModePicked(i, v)))
                        .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                        .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                        .width(Length::Fill)
                        .padding(pad)
                        .text_size(size)
                        .style(crate::widgets::rounded_pick_list_style)
                        .boxed(),
                );
                column![
                    crate::widgets::panel_field(t("snippet_step_save_content"), content_picker),
                    Space::new().height(8),
                    crate::widgets::panel_field(
                        t("snippet_step_file_name"),
                        input("{host}-{date}.log", &row.file_name, "", SnippetMessage::SnippetStepFileNameChanged),
                    ),
                    Space::new().height(8),
                    crate::widgets::panel_field(t("snippet_step_save_mode"), mode_picker),
                    Space::new().height(6),
                    hint(
                        t("snippet_step_save_hint")
                            .replace("{dir}", &self.snippet_output_dir().display().to_string()),
                    ),
                ]
                .boxed()
            }
            StepKind::Unknown => text(t("snippet_step_unknown_hint")).size(12).color(c.text_secondary).boxed(),
        }
    }
}

/// A row's move / remove button: muted and inert where it does not apply.
fn step_icon_btn<'a>(icon: iced::widget::Text<'a>, msg: Option<Message>) -> Element<'a, Message> {
    let enabled = msg.is_some();
    let fg = if enabled { OryxisColors::t().text_secondary } else { Color { a: 0.35, ..OryxisColors::t().text_muted } };
    button(container(icon.size(13).color(fg)).center_x(Length::Fixed(26.0)).center_y(Length::Fixed(26.0)))
        .padding(0)
        .on_press_maybe(msg)
        .style(move |_, status| {
            let bg = match status {
                BtnStatus::Hovered if enabled => OryxisColors::t().bg_hover,
                BtnStatus::Pressed if enabled => OryxisColors::t().bg_selected,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .boxed()
}

/// "N actions" for a snippet with more than one; nothing for one.
pub(crate) fn actions_badge(snip: &oryxis_core::models::Snippet) -> Option<String> {
    (snip.steps.len() > 1).then(|| t("snippet_actions_badge").replace("{n}", &snip.steps.len().to_string()))
}

impl Oryxis {
    /// What a snippet card shows under the name: its commands, or what
    /// its first action does when it has no command.
    /// One line for a snippet with actions (the first, then an ellipsis),
    /// so its card is as tall as its neighbours.
    pub(crate) fn snippet_card_preview(&self, snip: &oryxis_core::models::Snippet) -> String {
        if !snip.has_steps() {
            return snip.command.clone();
        }
        let first = snip.steps.first().map(|s| self.describe_snippet_step(s)).unwrap_or_default();
        let first = match snip.steps.first() {
            Some(oryxis_core::models::SnippetStep::Command { text }) => text.lines().next().unwrap_or("").to_string(),
            _ => first,
        };
        if snip.steps.len() > 1 { format!("{first}\u{2026}") } else { first }
    }
}
