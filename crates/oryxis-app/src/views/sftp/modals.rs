//! SFTP view helpers: modals. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::{column, row};
/// Confirmation dialog for the "Delete" action. Single targets show
/// the file name and a folder-recursive hint; bulk deletes show a count
/// and folder-vs-file breakdown so the user understands the blast
/// radius.
pub(crate) fn delete_confirm_modal<'a>(
    targets: &[crate::state::SftpDeleteTarget],
) -> Element<'a, Message> {
    let owned: Vec<(&str, bool)> = targets
        .iter()
        .map(|t| (t.path.as_str(), t.is_dir))
        .collect();
    let (title, detail) = crate::sftp_helpers::delete_confirm_copy(&owned);
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(detail).size(13).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            modal_footer(
                t("cancel"),
                Message::Sftp(SftpMessage::SftpCancelDelete),
                t("delete"),
                Some(Message::Sftp(SftpMessage::SftpConfirmDelete)),
                OryxisColors::t().error,
            ),
        ]
        .padding(24)
        .width(420),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });

    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::Sftp(SftpMessage::SftpCancelDelete))
    .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Close-guard modal: shown when the user tries to close an SFTP tab that has
/// an in-flight transfer or an unsaved edit-session. Rendered at the global
/// layer (`view_main`) so it shows regardless of the current surface.
pub(crate) fn close_guard_modal<'a>() -> Element<'a, Message> {
    let dialog = container(
        column![
            text(t("sftp_close_guard_title")).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(t("sftp_close_guard_detail")).size(13).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    t("close_anyway"),
                    Message::Sftp(SftpMessage::ConfirmCloseSftpTab),
                    OryxisColors::t().error,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::Sftp(SftpMessage::CancelCloseSftpTab),
                    OryxisColors::t().text_muted,
                ),
            ],
        ]
        .padding(24)
        .width(420),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });
    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::Sftp(SftpMessage::CancelCloseSftpTab))
    .into();
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Widget id of the new-entry name input, focused when the modal opens.
pub(crate) const NEW_ENTRY_INPUT_ID: &str = "sftp-new-entry-input";

/// Modal for "New folder" / "New file", single text input + create/cancel.
/// `Enter` in the input commits, mirroring the inline rename behaviour.
pub(crate) fn new_entry_modal<'a>(entry: &'a crate::state::SftpNewEntry) -> Element<'a, Message> {
    let title = match entry.kind {
        SftpEntryKind::Folder => t("new_folder"),
        SftpEntryKind::File => t("new_file"),
    };
    let placeholder = match entry.kind {
        SftpEntryKind::Folder => t("folder_name_placeholder"),
        SftpEntryKind::File => t("file_name_placeholder"),
    };
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(12),
            text_input(placeholder, &entry.input)
                .id(iced::widget::Id::new(NEW_ENTRY_INPUT_ID))
                .on_input(|v| Message::Sftp(SftpMessage::SftpNewEntryInput(v)))
                .on_submit(Message::Sftp(SftpMessage::SftpNewEntryCommit))
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    t("create"),
                    Message::Sftp(SftpMessage::SftpNewEntryCommit),
                    OryxisColors::t().accent,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    t("cancel"),
                    Message::Sftp(SftpMessage::SftpNewEntryCancel),
                    OryxisColors::t().text_muted,
                ),
            ],
        ]
        .padding(24)
        .width(380),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(12.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        ..Default::default()
    });

    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::Sftp(SftpMessage::SftpNewEntryCancel))
    .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Reopen-or-redownload dialog: the user asked to open a remote file that
/// is already being edited locally. Reopening hands the existing copy back
/// to the application (keeping any save that is still waiting to upload);
/// downloading again throws that copy away. FileZilla asks the same
/// question, and for the same reason: a blind re-download silently
/// discards work. Non-dismissable on backdrop click.
pub(crate) fn edit_reopen_modal<'a>(
    app: &crate::app::Oryxis,
    prompt: &crate::state::EditReopenPrompt,
) -> Element<'a, Message> {
    use crate::state::SftpEditReopenChoice as C;
    let choice = |c: C| Message::Sftp(SftpMessage::SftpEditReopenChoice(c));

    let mut body = column![
        text(t("sftp_edit_reopen_title"))
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        Space::new().height(6),
        text(t("sftp_edit_reopen_text").replacen("{file}", &prompt.label, 1))
            .size(12)
            .color(OryxisColors::t().text_secondary),
        Space::new().height(2),
        text(prompt.temp_path.display().to_string())
            .size(11)
            .color(OryxisColors::t().text_muted),
    ]
    .width(Length::Fill);
    // Only warn about losing work when there actually is a save pending:
    // an unmodified copy costs nothing to re-download.
    if prompt.pending_save {
        body = body.push(Space::new().height(8)).push(
            text(t("sftp_edit_reopen_pending"))
                .size(11)
                .color(OryxisColors::t().accent),
        );
    }

    app.modal_nav_reset();
    let buttons = crate::widgets::dir_row(vec![
        app.modal_nav_slot(
            crate::keynav::RowAction::activate(choice(C::Cancel)),
            8.0,
            false,
            ghost_button(t("cancel"), choice(C::Cancel)),
        ),
        Space::new().width(Length::Fill).into(),
        app.modal_nav_slot(
            crate::keynav::RowAction::activate(choice(C::Fresh)),
            8.0,
            false,
            outlined_button(t("sftp_edit_reopen_fresh"), choice(C::Fresh)),
        ),
        Space::new().width(8).into(),
        app.modal_nav_slot_default(
            crate::keynav::RowAction::activate(choice(C::Reopen)),
            8.0,
            true,
            primary_button(
                t("sftp_edit_reopen_local"),
                choice(C::Reopen),
                OryxisColors::t().accent,
            ),
        ),
    ])
    .align_y(iced::Alignment::Center);

    let content = body.push(Space::new().height(18)).push(buttons);
    let dialog = container(content.padding(22).width(560))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(12.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        });

    let scrim: Element<'_, Message> = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
            ..Default::default()
        })
        .into();
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Floating preview shown next to the cursor while a row is being
/// dragged across panes. Solid pill with the dragged item's label or
/// "N items"; non-interactive so the eventual mouse-release still hits
/// the underlying drop target.
pub(crate) fn drag_ghost<'a>(label: &str) -> Element<'a, Message> {
    container(
        row![
            iced_fonts::lucide::file().size(12).color(Color::WHITE),
            Space::new().width(8),
            text(label.to_string())
                .size(12)
                .color(Color::WHITE)
                .font(iced::Font {
                    weight: iced::font::Weight::Medium,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                }),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
    .style(|_| container::Style {
        background: Some(Background::Color(Color { a: 0.92, ..OryxisColors::t().accent })),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().accent,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    })
    .into()
}

/// Properties dialog, shows the standard file metadata (path, size,
/// mtime, owner) and a 3×3 grid of permission checkboxes for r/w/x
/// across owner / group / others. Apply runs chmod; bits the dialog
/// doesn't render (setuid/setgid/sticky) are preserved verbatim from
/// the original mode.
pub(crate) fn properties_modal<'a>(
    props: &'a crate::state::PropertiesView,
) -> Element<'a, Message> {
    let basename = props
        .path
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(&props.path)
        .to_string();
    let kind = if props.is_dir { t("kind_folder") } else { t("kind_file") };
    let mtime_str = props
        .mtime
        .and_then(|secs| chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0))
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_string());
    let owner_str = match (props.owner_uid, props.owner_gid) {
        (Some(u), Some(g)) => format!("uid {u} · gid {g}"),
        (Some(u), None) => format!("uid {u}"),
        _ => "-".to_string(),
    };
    let info_row = |label: &str, value: String| -> Element<'a, Message> {
        row![
            text(label.to_string())
                .size(11)
                .color(OryxisColors::t().text_muted)
                .width(Length::Fixed(80.0)),
            text(value).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center)
        .into()
    };

    let header_row = |label: &str| -> Element<'a, Message> {
        text(label.to_string())
            .size(11)
            .color(OryxisColors::t().text_muted)
            .into()
    };

    let perm_check = |checked: bool, bit: crate::state::PermBit| -> Element<'a, Message> {
        let mark = if checked {
            iced_fonts::lucide::circle_check()
                .size(14)
                .color(OryxisColors::t().accent)
        } else {
            iced_fonts::lucide::circle_minus()
                .size(14)
                .color(OryxisColors::t().text_muted)
        };
        button(mark)
            .on_press(Message::Sftp(SftpMessage::SftpPropertiesToggleBit(bit)))
            .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
    };

    let perm_row = |label: &str, r: (bool, crate::state::PermBit), w: (bool, crate::state::PermBit), x: (bool, crate::state::PermBit)| -> Element<'a, Message> {
        row![
            text(label.to_string())
                .size(12)
                .color(OryxisColors::t().text_secondary)
                .width(Length::Fixed(80.0)),
            perm_check(r.0, r.1),
            Space::new().width(8),
            perm_check(w.0, w.1),
            Space::new().width(8),
            perm_check(x.0, x.1),
        ]
        .align_y(iced::Alignment::Center)
        .into()
    };

    // Column header for the R / W / X permission grid. Rendered in the
    // secondary (not muted) color and semibold so the labels read clearly
    // above the checkboxes instead of nearly disappearing.
    let perm_header = |glyph: &'a str| -> Element<'a, Message> {
        container(
            text(glyph)
                .size(11)
                .font(iced::Font {
                    weight: iced::font::Weight::Semibold,
                    ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                })
                .color(OryxisColors::t().text_secondary),
        )
        .width(Length::Fixed(26.0))
        .center_x(Length::Fixed(26.0))
        .into()
    };
    let perm_grid = column![
        row![
            Space::new().width(Length::Fixed(80.0)),
            perm_header("R"),
            Space::new().width(8),
            perm_header("W"),
            Space::new().width(8),
            perm_header("X"),
        ],
        Space::new().height(4),
        perm_row(
            t("perm_owner"),
            (props.bits.user_r, crate::state::PermBit::UserR),
            (props.bits.user_w, crate::state::PermBit::UserW),
            (props.bits.user_x, crate::state::PermBit::UserX),
        ),
        Space::new().height(2),
        perm_row(
            t("perm_group"),
            (props.bits.group_r, crate::state::PermBit::GroupR),
            (props.bits.group_w, crate::state::PermBit::GroupW),
            (props.bits.group_x, crate::state::PermBit::GroupX),
        ),
        Space::new().height(2),
        perm_row(
            t("perm_others"),
            (props.bits.other_r, crate::state::PermBit::OtherR),
            (props.bits.other_w, crate::state::PermBit::OtherW),
            (props.bits.other_x, crate::state::PermBit::OtherX),
        ),
    ];

    let mut content = column![
        text(basename)
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        Space::new().height(4),
        text(props.path.clone())
            .size(11)
            .color(OryxisColors::t().text_muted),
        Space::new().height(14),
        info_row(t("type_label"), kind.to_string()),
        Space::new().height(4),
        info_row(t("col_size"), format_size(props.size)),
        Space::new().height(4),
        info_row(t("col_modified"), mtime_str),
        Space::new().height(4),
        info_row(t("perm_owner"), owner_str),
        Space::new().height(4),
        // Editable numeric (octal) mode, WinSCP-style: type e.g. 777 and the
        // checkboxes follow. Two-way bound with the grid below.
        row![
            text(t("info_mode"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .width(Length::Fixed(80.0)),
            text_input("644", &props.mode_input)
                .on_input(|v| Message::Sftp(SftpMessage::SftpPropertiesModeInput(v)))
                .on_submit(Message::Sftp(SftpMessage::SftpPropertiesApply))
                .size(12)
                .padding(Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 8.0 })
                .width(Length::Fixed(90.0))
                .style(crate::widgets::rounded_input_style),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(16),
        header_row(t("permissions")),
        Space::new().height(8),
    ];
    content = content.push(perm_grid);
    if let Some(err) = &props.error {
        content = content.push(Space::new().height(10));
        content = content.push(
            text(err.clone()).size(11).color(OryxisColors::t().error),
        );
    }
    content = content.push(Space::new().height(18));
    let apply_label = if props.applying { t("applying") } else { t("apply") };
    // While the chmod is in flight the action is disabled (None) so the user
    // can't double-fire; the handler also guards on `applying`.
    let apply_msg = (!props.applying).then_some(Message::Sftp(SftpMessage::SftpPropertiesApply));
    content = content.push(modal_footer(
        t("close"),
        Message::Sftp(SftpMessage::SftpPropertiesClose),
        apply_label,
        apply_msg,
        OryxisColors::t().accent,
    ));

    let dialog = container(content.padding(22).width(440))
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
        });

    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                ..Default::default()
            }),
    )
    .on_press(Message::Sftp(SftpMessage::SftpPropertiesClose))
    .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Modal shown when an upload would clobber an existing remote file.
/// Lays out the choices as a single horizontal row of buttons
/// destructive primary on the right, secondary outlined options in the
/// middle, ghost-style cancel on the left, so the modal stays compact
/// instead of stacking four heavy buttons vertically. The scrim is
/// non-dismissable: the user must pick something explicitly.
/// MobaXterm-style save-confirmation dialog for an "Open with" edit
/// watch (issue #84): a save was detected on the local temp copy and the
/// user decides whether it replaces the remote file. Yes / Yes to all /
/// Autosave escalate the grant; No skips this save; Cancel stops
/// watching the file.
pub(crate) fn edit_prompt_modal<'a>(
    app: &crate::app::Oryxis,
    session: &crate::state::EditSession,
) -> Element<'a, Message> {
    use crate::state::SftpEditPromptChoice as C;
    let host = if session.host.is_empty() {
        t("the_other_host").to_string()
    } else {
        session.host.clone()
    };
    let body = t("sftp_edit_prompt_text")
        .replacen("{file}", &session.label, 1)
        .replacen("{host}", &host, 1);

    // Every choice names the watch it answers for by temp file: the dialog
    // can be resolving a save that belongs to a parked tab, so "the first
    // dirty watch" is not a safe target.
    let temp = session.temp_path.clone();
    let choice = move |c: C| {
        Message::Sftp(SftpMessage::SftpEditPromptChoice(c, temp.clone()))
    };

    // Keyboard rows in visual order (Tab/arrows walk them, Enter fires
    // the ringed row); Yes is the surface default so a bare Enter
    // confirms the save, mirroring the click-path primary.
    app.modal_nav_reset();
    let buttons = crate::widgets::dir_row(vec![
        app.modal_nav_slot(
            crate::keynav::RowAction::activate(choice(C::Cancel)),
            8.0,
            false,
            ghost_button(t("sftp_edit_stop_watching"), choice(C::Cancel)),
        ),
        Space::new().width(Length::Fill).into(),
        app.modal_nav_slot(
            crate::keynav::RowAction::activate(choice(C::No)),
            8.0,
            false,
            outlined_button(t("no"), choice(C::No)),
        ),
        Space::new().width(8).into(),
        app.modal_nav_slot(
            crate::keynav::RowAction::activate(choice(C::Autosave)),
            8.0,
            false,
            outlined_button(t("sftp_edit_autosave_btn"), choice(C::Autosave)),
        ),
        Space::new().width(8).into(),
        app.modal_nav_slot(
            crate::keynav::RowAction::activate(choice(C::YesToAll)),
            8.0,
            false,
            outlined_button(t("sftp_edit_yes_all"), choice(C::YesToAll)),
        ),
        Space::new().width(8).into(),
        app.modal_nav_slot_default(
            crate::keynav::RowAction::activate(choice(C::Yes)),
            8.0,
            true,
            primary_button(t("yes"), choice(C::Yes), OryxisColors::t().accent),
        ),
    ])
    .align_y(iced::Alignment::Center);

    let content = column![
        text(t("sftp_edit_prompt_title"))
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        Space::new().height(6),
        text(body).size(12).color(OryxisColors::t().text_secondary),
        Space::new().height(2),
        text(session.remote_path.clone())
            .size(11)
            .color(OryxisColors::t().text_muted),
        Space::new().height(18),
        buttons,
    ]
    .width(Length::Fill);

    let dialog = container(content.padding(22).width(620))
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
        });

    // Non-dismissable scrim (same rationale as the overwrite prompt):
    // clicking outside is not a valid answer for a data-bearing decision.
    let scrim: Element<'_, Message> = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
            ..Default::default()
        })
        .into();

    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub(crate) fn overwrite_modal<'a>(
    prompt: &crate::state::OverwritePrompt,
) -> Element<'a, Message> {
    let size_hint = if prompt.src_size == prompt.dst_size {
        t("size_both_identical")
            .replacen("{size}", &format_size(prompt.src_size), 1)
    } else {
        let (local, remote) = prompt.local_remote_sizes();
        t("size_local_remote")
            .replacen("{local}", &format_size(local), 1)
            .replacen("{remote}", &format_size(remote), 1)
    };

    let mut content = column![
        text(t("already_exists").replacen("{name}", &prompt.basename, 1))
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        Space::new().height(4),
        text(prompt.dst_dir.clone())
            .size(11)
            .color(OryxisColors::t().text_muted),
        Space::new().height(2),
        text(size_hint).size(11).color(OryxisColors::t().text_muted),
    ]
    .width(Length::Fill);

    // Every answer travels back to the owner that asked. Re-deriving
    // it on arrival does not work: a sidebar transfer routinely runs
    // with no SFTP surface focused, and an unaddressed answer would
    // be declined outright, leaving the queue paused forever.
    let reply = |action: crate::state::OverwriteAction| match prompt.owner {
        Some(id) => Message::SftpFor(id, Box::new(SftpMessage::SftpResolveOverwrite(action))),
        None => Message::Sftp(SftpMessage::SftpResolveOverwrite(action)),
    };
    // Continuing is only meaningful when what is already there could BE a
    // partial copy of what is being sent: shorter than the source, and
    // not empty. Uploads only, because a download's own partial lives in
    // a scratch file and resumes without asking, so a finished file at
    // this destination is not a partial of anything.
    // `resume_offset` is what the engine will actually do, so asking it
      // here is the only way the button cannot lie. It answers 0 when
      // there is nothing provable to continue, which for a partial of a
      // windowed transfer means "smaller than the rewind".
    let can_resume = prompt.direction == crate::state::OverwriteDirection::Upload
        && oryxis_ssh::resume_offset(prompt.src_size, prompt.dst_size) > 0;

    if prompt.multi {
        // Sticky decision lets the user clear out a long upload's
        // collisions in one click instead of answering N times.
        content = content.push(Space::new().height(14));
        content = content.push(overwrite_apply_to_all_checkbox(prompt.apply_to_all, prompt.owner));
    }
    if can_resume {
        content = content.push(Space::new().height(10));
        content = content.push(
            text(t("resume_hint"))
                .size(11)
                .color(OryxisColors::t().text_muted),
        );
    }

    content = content.push(Space::new().height(18));
    let mut actions = row![
        ghost_button(
            t("cancel"),
            reply(crate::state::OverwriteAction::Cancel),
        ),
        Space::new().width(Length::Fill),
    ]
    .align_y(iced::Alignment::Center);
    if can_resume {
        actions = actions.push(outlined_button(
            t("resume_transfer"),
            reply(crate::state::OverwriteAction::Resume),
        ));
        actions = actions.push(Space::new().width(8));
    }
    actions = actions.push(outlined_button(
        t("replace_if_different"),
        reply(crate::state::OverwriteAction::ReplaceIfDifferent),
    ));
    actions = actions.push(Space::new().width(8));
    actions = actions.push(outlined_button(
        t("duplicate"),
        reply(crate::state::OverwriteAction::Duplicate),
    ));
    actions = actions.push(Space::new().width(8));
    actions = actions.push(primary_button(
        t("replace"),
        reply(crate::state::OverwriteAction::Replace),
        OryxisColors::t().error,
    ));
    content = content.push(actions);

    let dialog = container(content.padding(22).width(560))
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
        });

    // Non-dismissable scrim, clicking outside is not a valid answer
    // (users could lose data by deciding the wrong way), so we swallow
    // the press without doing anything. The user must pick a button.
    let scrim: Element<'_, Message> = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
            ..Default::default()
        })
        .into();

    // Wrap the dialog in a MouseArea that swallows clicks via `NoOp`,
    // otherwise events fall through the Stack to the scrim underneath
    // and the modal closes on every click inside the dialog body.
    let centered = container(MouseArea::new(dialog).on_press(Message::NoOp))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::Stack::new()
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Filled primary action button, destructive variants pass red, neutral
/// pass accent. Slightly more compact than `widgets::styled_button` so it
/// sits well in a horizontal modal footer row.
pub(crate) fn primary_button<'a>(label: &'a str, msg: Message, color: Color) -> Element<'a, Message> {
    // Accent CTAs share the theme's `button_text`; semantic colors
    // (success/error/etc.) auto-pick by luminance.
    let fg = if color == OryxisColors::t().accent {
        OryxisColors::t().button_text
    } else {
        crate::theme::contrast_text_for(color)
    };
    button(
        text(label.to_owned())
            .size(12)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(fg),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 16.0 })
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color {
                a: 1.0,
                r: (color.r + 0.06).min(1.0),
                g: (color.g + 0.06).min(1.0),
                b: (color.b + 0.06).min(1.0),
            },
            _ => color,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Standard modal footer: a neutral cancel/close action on the leading edge
/// and the primary action on the trailing edge, both equal width. RTL-aware
/// (leading/trailing mirror) via `dir_row`. `action_msg` is `None` to render
/// the primary action disabled (e.g. while an apply is in flight). Use this
/// for every confirm/cancel modal so their footers stay consistent.
pub(crate) fn modal_footer<'a>(
    cancel_label: &'a str,
    cancel_msg: Message,
    action_label: &'a str,
    action_msg: Option<Message>,
    action_color: Color,
) -> Element<'a, Message> {
    const FOOTER_BTN_W: f32 = 132.0;
    let action_fg = if action_color == OryxisColors::t().accent {
        OryxisColors::t().button_text
    } else {
        crate::theme::contrast_text_for(action_color)
    };
    let cancel = footer_button(
        cancel_label,
        Some(cancel_msg),
        OryxisColors::t().bg_selected,
        OryxisColors::t().text_primary,
        FOOTER_BTN_W,
    );
    let action = footer_button(action_label, action_msg, action_color, action_fg, FOOTER_BTN_W);
    crate::widgets::dir_row(vec![
        cancel,
        Space::new().width(Length::Fill).into(),
        action,
    ])
    .align_y(iced::Alignment::Center)
    .into()
}

/// Equal-width footer button with centered label. `msg = None` renders it
/// disabled (muted fill, no `on_press`).
pub(crate) fn footer_button<'a>(
    label: &'a str,
    msg: Option<Message>,
    color: Color,
    fg: Color,
    width: f32,
) -> Element<'a, Message> {
    let enabled = msg.is_some();
    let fg = if enabled { fg } else { OryxisColors::t().text_muted };
    let disabled_bg = OryxisColors::t().bg_selected;
    let mut b = button(
        text(label.to_owned())
            .size(12)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(fg)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .width(Length::Fixed(width))
    .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 })
    .style(move |_, status| {
        let bg = if !enabled {
            disabled_bg
        } else {
            match status {
                BtnStatus::Hovered => Color {
                    a: 1.0,
                    r: (color.r + 0.06).min(1.0),
                    g: (color.g + 0.06).min(1.0),
                    b: (color.b + 0.06).min(1.0),
                },
                _ => color,
            }
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    });
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    b.into()
}

/// Outlined secondary button, transparent fill with a subtle border.
/// Hover fills with a faint accent tint to communicate clickability
/// without competing with the primary action visually.
pub(crate) fn outlined_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    button(
        text(label.to_owned())
            .size(12)
            .color(OryxisColors::t().text_primary),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color { a: 0.10, ..OryxisColors::t().accent },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .into()
}

/// Ghost button, pure text on transparent, hover-only background tint.
/// Right for the lowest-emphasis action (Cancel here).
pub(crate) fn ghost_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    button(
        text(label.to_owned())
            .size(12)
            .color(OryxisColors::t().text_secondary),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Small click-to-toggle row with a square indicator + label. iced 0.14
/// has a built-in checkbox widget, but matching it to the rest of the
/// modal's chrome takes more code than just rolling a button-like row.
pub(crate) fn overwrite_apply_to_all_checkbox<'a>(checked: bool, owner: Option<uuid::Uuid>) -> Element<'a, Message> {
    let mark = if checked {
        iced_fonts::lucide::circle_check()
            .size(14)
            .color(OryxisColors::t().accent)
    } else {
        iced_fonts::lucide::circle_minus()
            .size(14)
            .color(OryxisColors::t().text_muted)
    };
    button(
        row![
            mark,
            Space::new().width(8),
            text(t("apply_to_remaining"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(match owner {
        Some(id) => Message::SftpFor(id, Box::new(SftpMessage::SftpToggleApplyToAll)),
        None => Message::Sftp(SftpMessage::SftpToggleApplyToAll),
    })
    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 4.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}
