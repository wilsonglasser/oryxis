//! Modal prompt content builders (snippet variables, careful paste,
//! tab / folder rename, folder delete). Split out of
//! views/layout/main_layout.rs; each returns just the dialog `Element`.

use super::*;
use iced::widget::column;

impl Oryxis {
    /// Content for the snippet-variables prompt (`{name}` placeholders
    /// filled before the snippet reaches the session).
    pub(crate) fn build_snippet_vars_dialog<'a>(
        &'a self,
        pending: &'a crate::state::PendingSnippetVars,
    ) -> Element<'a, Message> {
        let c = OryxisColors::t();
        self.modal_nav_reset();
        let mut fields = column![].spacing(10);
        for (i, (name, value)) in pending.vars.iter().enumerate() {
            let input_id = iced::widget::Id::from(format!("snippet-var-{i}"));
            fields = fields.push(
                column![
                    text(name.clone()).size(12).color(c.text_secondary),
                    Space::new().height(4),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::input(input_id.clone()),
                        crate::widgets::INPUT_RADIUS,
                        false,
                        text_input("", value)
                            .id(input_id)
                            .on_input(move |v| Message::Snippet(SnippetMessage::SnippetVarChanged(i, v)))
                            .on_submit(Message::Snippet(SnippetMessage::ConfirmSnippetVars))
                            .padding(10)
                            .style(crate::widgets::rounded_input_style)
                            .align_x(dir_align_x())
                            .into(),
                    ),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x()),
            );
        }
        let confirm_label = if pending.run {
            crate::i18n::t("snippet_run")
        } else {
            crate::i18n::t("snippet_paste")
        };
        let dialog = container(
            column![
                dir_row(vec![
                    iced_fonts::lucide::braces().size(16).color(c.accent).into(),
                    Space::new().width(8).into(),
                    container(
                        text(crate::i18n::t("snippet_vars_title"))
                            .size(16)
                            .color(c.text_primary),
                    )
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
                Space::new().height(12),
                fields,
                Space::new().height(16),
                dir_row(vec![
                    self.modal_nav_slot_default(
                        crate::keynav::RowAction::activate(Message::Snippet(SnippetMessage::ConfirmSnippetVars)),
                        6.0,
                        true,
                        styled_button(confirm_label, Message::Snippet(SnippetMessage::ConfirmSnippetVars), c.accent),
                    ),
                    Space::new().width(8).into(),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Snippet(SnippetMessage::CancelSnippetVars)),
                        6.0,
                        false,
                        styled_button(
                            crate::i18n::t("cancel"),
                            Message::Snippet(SnippetMessage::CancelSnippetVars),
                            c.text_muted,
                        ),
                    ),
                ]),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .padding(24),
        )
        .width(Length::Fixed(420.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(c.bg_surface)),
            border: Border { radius: Radius::from(12.0), color: c.border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }

    /// Content for the careful-paste confirmation (line-count, preview,
    /// trailing-newline + paste-guard warnings, Confirm + Cancel).
    pub(crate) fn build_paste_dialog<'a>(&'a self, pending: &'a str) -> Element<'a, Message> {
        let c = OryxisColors::t();
        // Count "\n", "\r\n" and bare-"\r" breaks alike; `str::lines`
        // already folds the first two, normalize lone \r (old-Mac /
        // bracketed-paste-hostile clipboards) before counting.
        let normalized = pending.replace('\r', "\n");
        let line_count = normalized.lines().count().max(1);
        let ends_with_newline = pending.ends_with('\n') || pending.ends_with('\r');

        // Preview: the first lines in the terminal's monospace, each
        // clipped so a minified one-liner can't blow the dialog up.
        const PREVIEW_LINES: usize = 8;
        const PREVIEW_COLS: usize = 100;
        let mut preview_col = column![].spacing(2);
        for line in normalized.lines().take(PREVIEW_LINES) {
            let clipped: String = if line.chars().count() > PREVIEW_COLS {
                let mut s: String = line.chars().take(PREVIEW_COLS).collect();
                s.push('…');
                s
            } else {
                line.to_string()
            };
            // Preserve blank lines' height (empty text collapses).
            let shown = if clipped.is_empty() { " ".to_string() } else { clipped };
            preview_col = preview_col.push(
                text(shown)
                    .size(12)
                    .font(iced::Font::MONOSPACE)
                    .color(c.text_secondary)
                    // Wrap long lines inside the fixed-width dialog
                    // instead of letting them bleed past its edge.
                    // Glyph (not Word) so an unbroken token still wraps.
                    .wrapping(iced::widget::text::Wrapping::Glyph),
            );
        }
        if line_count > PREVIEW_LINES {
            preview_col = preview_col.push(
                text("…").size(12).font(iced::Font::MONOSPACE).color(c.text_muted),
            );
        }
        let preview = container(preview_col)
            .width(Length::Fill)
            .padding(10)
            .style(move |_| container::Style {
                background: Some(Background::Color(c.bg_primary)),
                border: Border { radius: Radius::from(8.0), color: c.border, width: 1.0 },
                ..Default::default()
            });

        // The trailing-newline case is the whole reason this dialog
        // exists; call it out explicitly when it applies.
        let newline_note: Element<'_, Message> = if ends_with_newline {
            column![
                Space::new().height(8),
                dir_row(vec![
                    iced_fonts::lucide::triangle_alert()
                        .size(13)
                        .color(c.warning)
                        .into(),
                    Space::new().width(6).into(),
                    container(
                        text(crate::i18n::t("careful_paste_trailing"))
                            .size(11)
                            .color(c.warning),
                    )
                    .width(Length::Fill)
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
            ]
            .into()
        } else {
            Space::new().into()
        };

        // Content-heuristic warnings (paste_guard): one line per
        // detected class, re-derived from the parked text.
        let mut guard_notes = column![].spacing(4);
        for w in crate::paste_guard::paste_warnings(pending) {
            guard_notes = guard_notes.push(
                dir_row(vec![
                    iced_fonts::lucide::triangle_alert()
                        .size(13)
                        .color(c.error)
                        .into(),
                    Space::new().width(6).into(),
                    container(
                        text(crate::i18n::t(w.label_key())).size(11).color(c.error),
                    )
                    .width(Length::Fill)
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
            );
        }
        let guard_notes: Element<'_, Message> = column![
            Space::new().height(8),
            guard_notes,
        ]
        .into();

        // Broadcast input (C2): when the active tab is armed, this paste fans
        // out to every participating pane. Name the blast radius so the user
        // confirms with the count in view.
        let broadcast_note: Element<'_, Message> = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .filter(|t| t.broadcast)
            .map(|t| {
                t.pane_grid
                    .panes
                    .values()
                    .filter(|p| !p.broadcast_opt_out && p.zmodem.is_none())
                    .count()
            })
            .filter(|n| *n > 1)
            .map(|n| {
                column![
                    Space::new().height(8),
                    dir_row(vec![
                        iced_fonts::lucide::radio().size(13).color(c.warning).into(),
                        Space::new().width(6).into(),
                        container(
                            text(
                                crate::i18n::t("broadcast_paste_notice")
                                    .replace("{count}", &n.to_string()),
                            )
                            .size(11)
                            .color(c.warning),
                        )
                        .width(Length::Fill)
                        .into(),
                    ])
                    .align_y(iced::Alignment::Center),
                ]
                .into()
            })
            .unwrap_or_else(|| Space::new().into());

        let dialog = container(
            column![
                dir_row(vec![
                    iced_fonts::lucide::clipboard_list()
                        .size(16)
                        .color(c.accent)
                        .into(),
                    Space::new().width(8).into(),
                    container(
                        // An install script (issue #147) parks in the
                        // same dialog; the title says what confirming
                        // actually does there (runs a host setup, not
                        // "pastes some lines").
                        text(crate::i18n::t(if self.pending_paste_install.is_some() {
                            "install_script_title"
                        } else {
                            "careful_paste_title"
                        }))
                        .size(16)
                        .color(c.text_primary),
                    )
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .into(),
                ])
                .align_y(iced::Alignment::Center),
                Space::new().height(6),
                text(crate::i18n::line_count(line_count))
                    .size(12)
                    .color(c.text_muted)
                    .width(Length::Fill)
                    .align_x(dir_align_x()),
                Space::new().height(10),
                preview,
                newline_note,
                guard_notes,
                broadcast_note,
                Space::new().height(14),
                dir_row(vec![
                    // Keyboard: Confirm is the default row (Enter
                    // pastes), arrows/Tab reach Cancel.
                    {
                        self.modal_nav_reset();
                        self.modal_nav_slot_default(
                            crate::keynav::RowAction::activate(
                                Message::Terminal(TerminalMessage::ConfirmPendingPaste),
                            ),
                            6.0,
                            true,
                            styled_button(
                                crate::i18n::t(
                                    match self.pending_paste_install {
                                        // Run executes, Paste only types.
                                        Some((_, true)) => "install_script_run",
                                        Some((_, false)) => "careful_paste_confirm",
                                        None => "careful_paste_confirm",
                                    },
                                ),
                                Message::Terminal(TerminalMessage::ConfirmPendingPaste),
                                c.accent,
                            ),
                        )
                    },
                    Space::new().width(8).into(),
                    self.modal_nav_slot(
                        crate::keynav::RowAction::activate(Message::Terminal(TerminalMessage::CancelPendingPaste)),
                        6.0,
                        false,
                        styled_button(
                            crate::i18n::t("cancel"),
                            Message::Terminal(TerminalMessage::CancelPendingPaste),
                            c.text_muted,
                        ),
                    ),
                ]),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .padding(24),
        )
        .width(Length::Fixed(520.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(c.bg_surface)),
            border: Border { radius: Radius::from(12.0), color: c.border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }

    /// Content for the tab-rename modal (an empty name restores the
    /// automatic label).
    pub(crate) fn build_tab_rename_dialog<'a>(&'a self, input: &'a str) -> Element<'a, Message> {
        let dialog = container(
            column![
                text(crate::i18n::t("rename_tab"))
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .width(Length::Fill)
                    .align_x(dir_align_x()),
                Space::new().height(12),
                text_input(crate::i18n::t("tab_name"), input)
                    .id(iced::widget::Id::new(TAB_RENAME_INPUT_ID))
                    .on_input(|v| Message::Tabs(TabsMessage::TabRenameInput(v)))
                    .on_submit(Message::Tabs(TabsMessage::ConfirmTabRename))
                    .padding(10)
                    .width(Length::Fill)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(6),
                text(crate::i18n::t("tab_name_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .width(Length::Fill)
                    .align_x(dir_align_x()),
                Space::new().height(12),
                dir_row(vec![
                    styled_button(crate::i18n::t("save"), Message::Tabs(TabsMessage::ConfirmTabRename), OryxisColors::t().accent),
                    Space::new().width(8).into(),
                    styled_button(crate::i18n::t("cancel"), Message::Tabs(TabsMessage::CancelTabRename), OryxisColors::t().text_muted),
                ]),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .padding(24),
        )
        .width(Length::Fixed(360.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }

    /// Content for the folder-rename modal.
    pub(crate) fn build_folder_rename_dialog<'a>(&'a self, input: &'a str) -> Element<'a, Message> {
        let dialog = container(
            column![
                text(crate::i18n::t("rename_folder"))
                    .size(16)
                    .color(OryxisColors::t().text_primary)
                    .width(Length::Fill)
                    .align_x(dir_align_x()),
                Space::new().height(12),
                text_input(crate::i18n::t("folder_name"), input)
                    .on_input(|v| Message::Tabs(TabsMessage::FolderRenameInput(v)))
                    .on_submit(Message::Tabs(TabsMessage::ConfirmRenameFolder))
                    .padding(10)
                    .width(Length::Fill)
                    .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                Space::new().height(12),
                dir_row(vec![
                    styled_button(crate::i18n::t("save"), Message::Tabs(TabsMessage::ConfirmRenameFolder), OryxisColors::t().accent),
                    Space::new().width(8).into(),
                    styled_button(crate::i18n::t("cancel"), Message::Tabs(TabsMessage::CancelFolderModal), OryxisColors::t().text_muted),
                ]),
            ]
            .width(Length::Fill)
            .align_x(dir_align_x())
            .padding(24),
        )
        .width(Length::Fixed(360.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }

    /// Content for the three-way folder-delete confirmation (keep hosts /
    /// delete with hosts, collapsing to a single action for empty folders).
    pub(crate) fn build_folder_delete_dialog(&self, gid: uuid::Uuid) -> Element<'_, Message> {
        let folder_name = self
            .groups
            .iter()
            .find(|g| g.id == gid)
            .map(|g| g.label.clone())
            .unwrap_or_default();
        let host_count = self
            .connections
            .iter()
            .filter(|c| c.group_id == Some(gid))
            .count();
        // Nested groups (manual subgroups) are
        // never deleted with the folder, they get promoted one level
        // up; the copy below says so instead of calling the folder
        // "empty".
        let sub_count = self
            .groups
            .iter()
            .filter(|g| g.parent_id == Some(gid))
            .count();
        let c = OryxisColors::t();

        // Tinted circular warning badge anchoring the dialog.
        let badge = container(
            iced_fonts::lucide::triangle_alert().size(22).color(c.error),
        )
        .width(Length::Fixed(48.0))
        .height(Length::Fixed(48.0))
        .center_x(Length::Fixed(48.0))
        .center_y(Length::Fixed(48.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.12, ..c.error })),
            border: Border { radius: Radius::from(24.0), ..Default::default() },
            ..Default::default()
        });

        // Subtitle: the folder name, plus the host count when it carries
        // any (an empty folder has nothing to qualify).
        let subtitle = if host_count == 0 {
            format!("\"{}\"", folder_name)
        } else {
            format!("\"{}\"  ·  {}", folder_name, crate::i18n::host_count(host_count))
        };
        let header = column![
            badge,
            Space::new().height(14),
            text(crate::i18n::t("delete_folder_question"))
                .size(17)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(c.text_primary),
            Space::new().height(6),
            text(subtitle)
                .size(12)
                .color(c.text_muted)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        ]
        .width(Length::Fill)
        .align_x(iced::Alignment::Center);

        // Empty folders have no hosts to move or destroy, so the
        // three-way choice collapses to a single, honest "remove the
        // folder" action. Keyboard: the first (safest) choice card
        // is the default row; Up/Down walk cards + Cancel.
        self.modal_nav_reset();
        use crate::keynav::RowAction;
        let actions = if host_count == 0 {
            // No direct hosts: one action. With nested groups inside,
            // say they get promoted instead of calling the folder
            // empty (the deletion itself never removes them).
            let desc = if sub_count == 0 {
                crate::i18n::t("delete_folder_empty_desc")
            } else {
                crate::i18n::t("delete_folder_only_subgroups_desc")
            };
            column![self.modal_nav_slot_default(
                RowAction::activate(Message::Tabs(TabsMessage::DeleteFolderWithHosts)),
                12.0,
                false,
                folder_choice_card(
                    iced_fonts::lucide::trash(),
                    crate::i18n::t("delete_folder_empty"),
                    desc,
                    Message::Tabs(TabsMessage::DeleteFolderWithHosts),
                    c.error,
                ),
            )]
        } else {
            let with_hosts_desc = if sub_count == 0 {
                crate::i18n::t("delete_folder_with_hosts_desc")
            } else {
                crate::i18n::t("delete_folder_with_hosts_subgroups_desc")
            };
            column![
                self.modal_nav_slot_default(
                    RowAction::activate(Message::Tabs(TabsMessage::DeleteFolderKeepHosts)),
                    12.0,
                    false,
                    folder_choice_card(
                        iced_fonts::lucide::folder_open(),
                        crate::i18n::t("delete_folder_keep_hosts"),
                        crate::i18n::t("delete_folder_keep_hosts_desc"),
                        Message::Tabs(TabsMessage::DeleteFolderKeepHosts),
                        c.accent,
                    ),
                ),
                Space::new().height(10),
                self.modal_nav_slot(
                    RowAction::activate(Message::Tabs(TabsMessage::DeleteFolderWithHosts)),
                    12.0,
                    false,
                    folder_choice_card(
                        iced_fonts::lucide::trash(),
                        crate::i18n::t("delete_folder_with_hosts"),
                        with_hosts_desc,
                        Message::Tabs(TabsMessage::DeleteFolderWithHosts),
                        c.error,
                    ),
                ),
            ]
        }
        .width(Length::Fill);

        let dialog = container(
            column![
                header,
                Space::new().height(20),
                actions,
                Space::new().height(14),
                self.modal_nav_slot(
                    RowAction::activate(Message::Tabs(TabsMessage::CancelFolderModal)),
                    8.0,
                    false,
                    ghost_button(crate::i18n::t("cancel"), Message::Tabs(TabsMessage::CancelFolderModal)),
                ),
            ]
            .width(Length::Fill)
            .padding(24),
        )
        .width(Length::Fixed(400.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(c.bg_surface)),
            border: Border { radius: Radius::from(14.0), color: c.border, width: 1.0 },
            ..Default::default()
        });
        dialog.into()
    }
}
