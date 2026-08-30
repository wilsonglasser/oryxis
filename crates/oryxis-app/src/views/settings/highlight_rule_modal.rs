//! The highlight-rule editor (C6), as a modal.
//!
//! It used to expand inline under the rule it edits. That worked while
//! the form was three fields, but the block sits at the bottom of a long
//! Settings section and inside a narrow host panel, so the form was read
//! through whatever slice of the page happened to be on screen. A card
//! over a scrim gets the whole form in one view and lets the colour pick
//! be the real HSV picker instead of six presets.
//!
//! One modal serves both lists: `form.scope` says which one it commits
//! to, exactly as the inline editor did. Its rows record on the MODAL
//! keyboard ring (not the Settings / panel rings the list rows use),
//! because that is the layer that owns the keyboard while a modal is up.

use super::*;
use iced::widget::column;

use oryxis_core::models::TriggerAction;

/// Width of the card. Wide enough for the HSV picker plus its hue bar
/// with room to spare, and for the "label ... picker" rows to read as
/// two columns rather than a wrapped line.
const CARD_WIDTH: f32 = 460.0;

impl Oryxis {
    /// Whether the rule editor is on screen. The form's `editing` index
    /// is the source of truth (the modal registry's rule: no second
    /// `show_*` flag), gated on the surface that owns the list actually
    /// being up, so a panel closed by something other than a click (the
    /// soft auto-lock, an import finishing) can never leave a modal
    /// floating over a screen that has nothing to do with it.
    pub(crate) fn highlight_rule_editor_open(&self) -> bool {
        if self.highlight_rule_form.editing.is_none() {
            return false;
        }
        // Both arms mirror the predicate the LAYOUT uses, not just the
        // flag: `panels.host_panel` is read inside a chain that a live
        // tab and the other Dashboard panels win over
        // (`active_side_panel`), and `active_view == Settings` says
        // nothing while a terminal tab is on screen.
        match self.highlight_rule_form.scope {
            crate::state::RuleScope::Host => self.side_panel_open() && self.panels.host_panel,
            crate::state::RuleScope::Global => {
                self.active_tab.is_none() && self.active_view == crate::state::View::Settings
            }
        }
    }

    /// The card. Built in display order, which is also the order the
    /// keyboard walk records: every row here is a `modal_nav_*` slot.
    pub(crate) fn view_highlight_rule_modal(&self) -> Element<'_, Message> {
        self.modal_nav_reset();
        let c = OryxisColors::t();
        let form = &self.highlight_rule_form;
        let rule = &form.rule;

        let title = if form.creating {
            t("hl_rule_new_title")
        } else {
            t("hl_rule_edit_title")
        };

        let mut body = column![
            text(title)
                .size(15)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(c.text_primary),
            Space::new().height(16),
            panel_field(t("name"), self.hl_modal_input(
                "set-hl-rule-name",
                t("hl_rule_name_ph"),
                &rule.name,
                false,
                |v| Message::Settings(SettingsMessage::HighlightRuleNameChanged(v)),
            )),
            Space::new().height(12),
            panel_field(t("hl_rule_pattern"), self.hl_modal_input(
                "set-hl-rule-pattern",
                if rule.is_regex {
                    t("hl_rule_pattern_re_ph")
                } else {
                    t("hl_rule_pattern_ph")
                },
                &rule.pattern,
                true,
                |v| Message::Settings(SettingsMessage::HighlightRulePatternChanged(v)),
            )),
            // What "Pattern" means is the one thing the field cannot say
            // for itself, and an example alone reads as a value the app
            // put there (a placeholder of "ERROR" was reported as an
            // error message).
            Space::new().height(6),
            text(t("hl_rule_pattern_desc")).size(11).color(c.text_muted),
            Space::new().height(14),
            self.hl_modal_toggle(
                t("hl_rule_regex"),
                rule.is_regex,
                Message::Settings(SettingsMessage::HighlightRuleToggleRegex),
            ),
            Space::new().height(10),
            self.hl_modal_toggle(
                t("hl_rule_case"),
                rule.case_sensitive,
                Message::Settings(SettingsMessage::HighlightRuleToggleCaseSensitive),
            ),
            Space::new().height(16),
            text(t("hl_rule_color")).size(12).color(c.text_muted),
            Space::new().height(8),
            self.hl_modal_color(&rule.color),
            Space::new().height(16),
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // The action, and the snippet picker it needs.
        body = body.push(self.hl_modal_pick(
            t("hl_rule_action"),
            crate::dispatch_settings::action_options()
                .into_iter()
                .map(|(_, l)| l.to_string())
                .collect(),
            crate::dispatch_settings::action_label(&rule.action).to_string(),
            |l| Message::Settings(SettingsMessage::HighlightRuleActionChanged(l)),
        ));
        if let TriggerAction::Snippet { id } = &rule.action {
            let selected = self
                .snippets
                .iter()
                .find(|s| s.id.to_string() == *id)
                .map(|s| s.label.clone())
                .unwrap_or_default();
            body = body.push(Space::new().height(10)).push(self.hl_modal_pick(
                t("hl_rule_snippet"),
                self.snippets.iter().map(|s| s.label.clone()).collect(),
                selected,
                |l| Message::Settings(SettingsMessage::HighlightRuleSnippetChanged(l)),
            ));
        }
        if rule.action.is_trigger() {
            body = body.push(Space::new().height(8)).push(
                text(t("hl_rule_trigger_note"))
                    .size(11)
                    .color(c.text_muted),
            );
        }

        if let Some(err) = &form.error {
            body = body
                .push(Space::new().height(10))
                .push(text(err.clone()).size(11).color(c.error));
        }

        // Cancel then the primary action, matching the app's other form
        // modals (the local-terminal editor is the reference). Save is
        // the default row, so Enter commits the form from any field.
        let cancel = self.modal_nav_slot(
            crate::keynav::RowAction::activate(Message::Settings(
                SettingsMessage::HighlightRuleCancelEdit,
            )),
            6.0,
            false,
            styled_button(
                t("cancel"),
                Message::Settings(SettingsMessage::HighlightRuleCancelEdit),
                c.bg_selected,
            ),
        );
        let save = self.modal_nav_slot_default(
            crate::keynav::RowAction::activate(Message::Settings(
                SettingsMessage::HighlightRuleSave,
            )),
            6.0,
            true,
            styled_button(
                t("save"),
                Message::Settings(SettingsMessage::HighlightRuleSave),
                c.accent,
            ),
        );
        body = body.push(Space::new().height(18)).push(dir_row(vec![
            Space::new().width(Length::Fill).into(),
            cancel,
            Space::new().width(8).into(),
            save,
        ]));

        container(body)
            .width(Length::Fixed(CARD_WIDTH))
            .padding(20)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.30),
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 24.0,
                },
                ..Default::default()
            })
            .into()
    }

    /// A text field, recorded so Tab reaches it and Enter focuses it.
    fn hl_modal_input<'a>(
        &self,
        id: &'static str,
        placeholder: &'a str,
        value: &'a str,
        mono: bool,
        on_input: impl Fn(String) -> Message + 'a,
    ) -> Element<'a, Message> {
        let mut input = text_input(placeholder, value)
            .id(iced::widget::Id::new(id))
            .on_input(on_input)
            .padding(10)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());
        if mono {
            input = input.font(iced::Font::MONOSPACE);
        }
        self.modal_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new(id)),
            crate::widgets::INPUT_RADIUS,
            false,
            input.into(),
        )
    }

    /// A "label ......... switch" row on the modal ring.
    fn hl_modal_toggle<'a>(
        &self,
        label: &'a str,
        value: bool,
        msg: Message,
    ) -> Element<'a, Message> {
        self.modal_nav_slot(
            crate::keynav::RowAction::activate(msg.clone()),
            8.0,
            false,
            crate::widgets::toggle_row(label, value, msg),
        )
    }

    /// A "label ......... pick_list" row. The ring hugs the picker, not
    /// the whole row, so Left / Right visibly act on that control (same
    /// rule as `nav_pick_row` in Settings).
    fn hl_modal_pick<'a>(
        &self,
        label: &'a str,
        options: Vec<String>,
        selected: String,
        on_change: impl Fn(String) -> Message + Clone + 'a,
    ) -> Element<'a, Message> {
        let (prev, next) = crate::keynav::slots::cycle_pair(&options, &selected, on_change.clone());
        let picker = self.modal_nav_slot(
            crate::keynav::RowAction::picker(prev, next),
            crate::widgets::INPUT_RADIUS,
            false,
            pick_list(Some(selected), options, |l: &String| l.clone())
                .on_select(on_change)
                .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
                .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
                .width(Length::Fixed(220.0))
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style)
                .into(),
        );
        dir_row(vec![
            text(label).size(13).color(OryxisColors::t().text_primary).into(),
            Space::new().width(Length::Fill).into(),
            picker,
        ])
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// The colour block: the app's own HSV picker, the presets, and the
    /// hex field. The presets stay because they are the terminal-legible
    /// ones and picking a rule colour is usually "give me a red"; the
    /// picker is what makes any other colour reachable without knowing
    /// its hex.
    fn hl_modal_color<'a>(&'a self, current: &'a str) -> Element<'a, Message> {
        let color = oryxis_terminal::parse_hex_color(current)
            .unwrap_or(crate::highlight_rules::FALLBACK_COLOR);

        let mut swatches: Vec<Element<'a, Message>> = Vec::new();
        for preset in crate::highlight_rules::RULE_COLOR_PRESETS {
            let preset_color = oryxis_terminal::parse_hex_color(preset)
                .unwrap_or(crate::highlight_rules::FALLBACK_COLOR);
            let selected = current.eq_ignore_ascii_case(preset);
            let msg =
                Message::Settings(SettingsMessage::HighlightRuleColorChanged(preset.to_string()));
            swatches.push(self.modal_nav_slot(
                crate::keynav::RowAction::activate(msg.clone()),
                6.0,
                false,
                button(Space::new().width(18).height(18))
                    .on_press(msg)
                    .padding(2)
                    .style(move |_, status| button::Style {
                        background: Some(Background::Color(preset_color)),
                        border: Border {
                            radius: Radius::from(4.0),
                            width: if selected || status != BtnStatus::Active { 2.0 } else { 0.0 },
                            color: OryxisColors::t().text_primary,
                        },
                        ..Default::default()
                    })
                    .into(),
            ));
            swatches.push(Space::new().width(6).into());
        }
        swatches.push(Space::new().width(6).into());
        swatches.push(self.modal_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("set-hl-rule-color")),
            crate::widgets::INPUT_RADIUS,
            false,
            text_input("#RRGGBB", current)
                .id(iced::widget::Id::new("set-hl-rule-color"))
                .on_input(|v| Message::Settings(SettingsMessage::HighlightRuleColorChanged(v)))
                .padding(7)
                .size(12)
                .width(Length::Fixed(110.0))
                .style(crate::widgets::rounded_input_style)
                .into(),
        ));

        column![
            crate::color_picker::color_picker(color, |hex| {
                Message::Settings(SettingsMessage::HighlightRuleColorChanged(hex))
            }),
            Space::new().height(10),
            dir_row(swatches).align_y(iced::Alignment::Center),
        ]
        .align_x(dir_align_x())
        .into()
    }
}
