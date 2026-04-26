//! SFTP browser view — dual-pane (local | remote) file manager.

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, row, scrollable, text, text_input, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::state::{SftpEntryKind, SftpPaneSide};
use crate::theme::OryxisColors;

const ROW_HEIGHT: f32 = 28.0;

impl Oryxis {
    pub(crate) fn view_sftp(&self) -> Element<'_, Message> {
        let panes = row![
            self.view_sftp_local_pane(),
            container(Space::new().width(1))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
            self.view_sftp_remote_pane(),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        // Stack the panes with the optional progress strip below — when a
        // folder transfer is running we surface a thin status bar with
        // counts + a cancel button, otherwise the panes own all the space.
        let body: Element<'_, Message> = if let Some(transfer) = &self.sftp.transfer {
            column![panes, transfer_progress_strip(transfer)]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            panes.into()
        };

        let mut stack = iced::widget::Stack::new()
            .push(body)
            .width(Length::Fill)
            .height(Length::Fill);

        // The right-click row context menu is rendered at the layout
        // root instead — its position is in window coordinates so
        // clamping it from inside view_sftp would be off by the title +
        // tab bar height.
        if !self.sftp.delete_confirm.is_empty() {
            stack = stack.push(delete_confirm_modal(&self.sftp.delete_confirm));
        }
        if let Some(entry) = &self.sftp.new_entry {
            stack = stack.push(new_entry_modal(entry));
        }
        if let Some(session) = &self.sftp.edit_session {
            stack = stack.push(edit_in_place_modal(session));
        }
        if let Some(prompt) = &self.sftp.overwrite_prompt {
            stack = stack.push(overwrite_modal(prompt));
        }
        if let Some(props) = &self.sftp.properties {
            stack = stack.push(properties_modal(props));
        }
        if self.sftp.picker_open {
            stack = stack.push(self.view_sftp_picker());
        }
        stack.into()
    }

    fn view_sftp_local_pane(&self) -> Element<'_, Message> {
        // Match the right-pane chip dimensions exactly so the two
        // toolbars sit on the same baseline — earlier the host chip
        // (icon + chevron + button padding) was a few pixels taller
        // than the bare "Local" label, throwing the breadcrumbs and
        // column headers out of alignment between panes.
        let local_badge = container(
            iced_fonts::lucide::monitor()
                .size(12)
                .color(Color::WHITE),
        )
        .center_x(Length::Fixed(20.0))
        .center_y(Length::Fixed(20.0))
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(4.0), ..Default::default() },
            ..Default::default()
        });
        let header_title = container(
            row![
                local_badge,
                Space::new().width(8),
                text("Local").size(14).color(OryxisColors::t().text_primary),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 4.0 });

        let actions_btn: Element<'_, Message> = pane_actions_btn(Message::SftpToggleLocalActions);

        let toolbar = row![
            header_title,
            Space::new().width(Length::Fill),
            text_input("Filter…", &self.sftp.local_filter)
                .on_input(Message::SftpLocalFilter)
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .size(11)
                .width(140)
                .style(crate::widgets::rounded_input_style),
            Space::new().width(8),
            actions_btn,
        ]
        .align_y(iced::Alignment::Center)
        .padding(Padding { top: 12.0, right: 14.0, bottom: 8.0, left: 14.0 });

        // The path bar swaps between a clickable breadcrumb and a text
        // input — same area, two modes, like Finder / Files / Explorer.
        let path_bar: Element<'_, Message> = if let Some(input) = &self.sftp.local_path_editing {
            text_input(
                &self.sftp.local_path.display().to_string(),
                input,
            )
            .on_input(Message::SftpEditLocalPath)
            .on_submit(Message::SftpCommitLocalPath)
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .size(11)
            .style(crate::widgets::rounded_input_style)
            .into()
        } else {
            // Wrap the breadcrumb in a MouseArea so any unused gutter
            // turns into "click to edit" — keeps the inline crumb buttons
            // working for direct navigation.
            MouseArea::new(local_breadcrumb(&self.sftp.local_path))
                .on_press(Message::SftpStartEditLocalPath)
                .into()
        };

        let needle = self.sftp.local_filter.to_lowercase();
        let header_band = pane_header_band(
            column![
                toolbar,
                container(path_bar)
                    .padding(Padding { top: 0.0, right: 14.0, bottom: 8.0, left: 14.0 })
                    .width(Length::Fill),
                column_headers(true, self.sftp.local_sort),
            ]
            .width(Length::Fill),
        );

        let body: Element<'_, Message> = if let Some(err) = &self.sftp.local_error {
            container(text(err.clone()).size(12).color(OryxisColors::t().error))
                .padding(12)
                .into()
        } else {
            let mut col = column![].spacing(0);
            if self.sftp.local_path.parent().is_some() {
                col = col.push(parent_row(true));
            }
            for entry in &self.sftp.local_entries {
                if !self.sftp.local_show_hidden && entry.name.starts_with('.') {
                    continue;
                }
                if !needle.is_empty() && !entry.name.to_lowercase().contains(&needle) {
                    continue;
                }
                let path = self.sftp.local_path.join(&entry.name);
                let path_str = path.to_string_lossy().into_owned();
                let rename_input = self.sftp.rename.as_ref().and_then(|r| {
                    if r.side == SftpPaneSide::Local && r.original_path == path_str {
                        Some(r.input.as_str())
                    } else {
                        None
                    }
                });
                let is_selected = self
                    .sftp
                    .selected_rows
                    .iter()
                    .any(|(s, p)| *s == SftpPaneSide::Local && p == &path_str);
                // Tint a local folder row that's the drop target while
                // a remote→local internal drag is in flight.
                let internal_cross_pane = self
                    .sftp
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.active && d.origin_side != SftpPaneSide::Local);
                let is_drop_target = internal_cross_pane
                    && entry.is_dir
                    && self
                        .sftp
                        .hovered_row
                        .as_ref()
                        .is_some_and(|(s, p, _)| {
                            *s == SftpPaneSide::Local && p == &path_str
                        });
                col = col.push(file_row_local(
                    entry.name.clone(),
                    entry.is_dir,
                    if entry.is_dir { String::new() } else { format_size(entry.size) },
                    entry.modified,
                    path,
                    rename_input,
                    is_selected,
                    is_drop_target,
                ));
            }
            scrollable(col).width(Length::Fill).height(Length::Fill).into()
        };

        let mut stack = iced::widget::Stack::new()
            .push(
                column![header_band, body]
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill);

        if self.sftp.local_actions_open {
            stack = stack.push(actions_menu_overlay(
                true,
                self.sftp.local_show_hidden,
            ));
        }
        if self.sftp.local_drives_open {
            stack = stack.push(drives_menu_overlay());
        }
        // Local-pane drop highlight when the user is dragging *from* the
        // remote pane. Mirrors the right-pane outline so the user has
        // symmetric visual feedback for both directions.
        let internal_drag_in =
            self.sftp.drag.as_ref().is_some_and(|d| {
                d.active && d.origin_side != SftpPaneSide::Local
            });
        if internal_drag_in {
            let outline = container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    border: Border {
                        radius: Radius::from(0.0),
                        color: OryxisColors::t().accent,
                        width: 2.0,
                    },
                    ..Default::default()
                });
            stack = stack.push(outline);
        }
        stack.into()
    }

    fn view_sftp_remote_pane(&self) -> Element<'_, Message> {
        // Right-pane title — same OS-coloured badge the host cards use,
        // followed by the host label and a chevron. Click anywhere on
        // the chip opens the picker, same affordance as Termius.
        let chip_label = self
            .sftp
            .host_label
            .clone()
            .unwrap_or_else(|| "Pick a host".to_string());
        let mounted_conn = self.sftp.host_label.as_ref().and_then(|label| {
            self.connections.iter().find(|c| &c.label == label)
        });
        let chip_icon: Element<'_, Message> = if let Some(conn) = mounted_conn {
            let (glyph, badge_color) = crate::os_icon::resolve_icon(
                conn.detected_os.as_deref(),
                OryxisColors::t().accent,
            );
            container(glyph.size(11).color(Color::WHITE))
                .center_x(Length::Fixed(20.0))
                .center_y(Length::Fixed(20.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(badge_color)),
                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                    ..Default::default()
                })
                .into()
        } else {
            // No mount yet — show a faint server placeholder.
            container(
                iced_fonts::lucide::server()
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .center_x(Length::Fixed(20.0))
            .center_y(Length::Fixed(20.0))
            .into()
        };
        let header_title: Element<'_, Message> = button(
            row![
                chip_icon,
                Space::new().width(8),
                text(chip_label).size(14).color(OryxisColors::t().text_primary),
                Space::new().width(8),
                iced_fonts::lucide::chevron_down().size(10).color(OryxisColors::t().text_muted),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 4.0 })
        .on_press(Message::SftpOpenPicker)
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
        .into();

        let actions_btn: Element<'_, Message> = pane_actions_btn(Message::SftpToggleRemoteActions);

        let toolbar = row![
            header_title,
            Space::new().width(Length::Fill),
            text_input("Filter…", &self.sftp.remote_filter)
                .on_input(Message::SftpRemoteFilter)
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .size(11)
                .width(140)
                .style(crate::widgets::rounded_input_style),
            Space::new().width(8),
            actions_btn,
        ]
        .align_y(iced::Alignment::Center)
        .padding(Padding { top: 12.0, right: 14.0, bottom: 8.0, left: 14.0 });

        let path_bar: Element<'_, Message> = if let Some(input) = &self.sftp.remote_path_editing {
            text_input(&self.sftp.remote_path, input)
                .on_input(Message::SftpEditRemotePath)
                .on_submit(Message::SftpCommitRemotePath)
                .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
                .size(11)
                .style(crate::widgets::rounded_input_style)
                .into()
        } else {
            MouseArea::new(remote_breadcrumb(&self.sftp.remote_path))
                .on_press(Message::SftpStartEditRemotePath)
                .into()
        };
        let needle = self.sftp.remote_filter.to_lowercase();

        let header_band = pane_header_band(
            column![
                toolbar,
                container(path_bar)
                    .padding(Padding { top: 0.0, right: 14.0, bottom: 8.0, left: 14.0 })
                    .width(Length::Fill),
                column_headers(false, self.sftp.remote_sort),
            ]
            .width(Length::Fill),
        );

        let body: Element<'_, Message> = if let Some(err) = &self.sftp.remote_error {
            // Retry routes through SftpRetryRemote which knows whether
            // the session is still up (re-list) or whether the connect
            // itself failed (re-run the full pick flow). The simpler
            // SftpNavigateRemote path silently no-ops when client is
            // None, which is exactly what happens after a connect
            // failure — that was the original "retry does nothing" bug.
            container(
                column![
                    row![
                        iced_fonts::lucide::circle_alert()
                            .size(14)
                            .color(OryxisColors::t().error),
                        Space::new().width(8),
                        text(err.clone())
                            .size(12)
                            .color(OryxisColors::t().error)
                            .width(Length::Fill),
                    ]
                    .align_y(iced::Alignment::Center),
                    Space::new().height(10),
                    row![
                        crate::widgets::styled_button(
                            "Retry",
                            Message::SftpRetryRemote,
                            OryxisColors::t().accent,
                        ),
                        Space::new().width(8),
                        crate::widgets::styled_button(
                            "Pick another host",
                            Message::SftpOpenPicker,
                            OryxisColors::t().text_muted,
                        ),
                    ],
                ]
                .padding(16),
            )
            .into()
        } else if self.sftp.remote_loading {
            container(
                column![
                    text("Loading…").size(12).color(OryxisColors::t().text_muted),
                    Space::new().height(10),
                    crate::widgets::styled_button(
                        "Cancel",
                        Message::SftpCancelRemoteLoad,
                        OryxisColors::t().text_muted,
                    ),
                ]
                .padding(12),
            )
            .into()
        } else if self.sftp.host_label.is_none() {
            container(
                text("Pick a host to start.")
                    .size(12)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(12)
            .into()
        } else {
            let mut col = column![].spacing(0);
            if self.sftp.remote_path != "/" && !self.sftp.remote_path.is_empty() {
                col = col.push(parent_row(false));
            }
            for entry in &self.sftp.remote_entries {
                if !self.sftp.remote_show_hidden && entry.name.starts_with('.') {
                    continue;
                }
                if !needle.is_empty() && !entry.name.to_lowercase().contains(&needle) {
                    continue;
                }
                let parent = self.sftp.remote_path.trim_end_matches('/');
                let full = if parent.is_empty() {
                    format!("/{}", entry.name)
                } else {
                    format!("{}/{}", parent, entry.name)
                };
                let rename_input = self.sftp.rename.as_ref().and_then(|r| {
                    if r.side == SftpPaneSide::Remote && r.original_path == full {
                        Some(r.input.as_str())
                    } else {
                        None
                    }
                });
                // Drop target: applies to both OS drag (drop_active) and
                // internal cross-pane drag, but only when the hovered
                // row IS this entry, lives on the remote pane, and is a
                // folder. Internal drag from the remote pane onto its
                // own pane is a no-op so we don't highlight in that case.
                let internal_cross_pane = self
                    .sftp
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.active && d.origin_side != SftpPaneSide::Remote);
                let drop_phase = self.sftp.drop_active || internal_cross_pane;
                let is_drop_target = drop_phase
                    && entry.is_dir
                    && self
                        .sftp
                        .hovered_row
                        .as_ref()
                        .is_some_and(|(s, p, _)| {
                            *s == SftpPaneSide::Remote && p == &full
                        });
                let is_selected = self
                    .sftp
                    .selected_rows
                    .iter()
                    .any(|(s, p)| *s == SftpPaneSide::Remote && p == &full);
                col = col.push(file_row_remote(
                    entry.name.clone(),
                    entry.is_dir,
                    entry.is_symlink,
                    if entry.is_dir { String::new() } else { format_size(entry.size) },
                    entry.mtime,
                    full,
                    rename_input,
                    is_drop_target,
                    is_selected,
                ));
            }
            scrollable(col).width(Length::Fill).height(Length::Fill).into()
        };

        let mut stack = iced::widget::Stack::new()
            .push(
                column![header_band, body]
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill);

        if self.sftp.remote_actions_open {
            stack = stack.push(actions_menu_overlay(
                false,
                self.sftp.remote_show_hidden,
            ));
        }

        // While a drag is in progress, surround the pane with an accent
        // outline so the user sees that the remote pane is the drop
        // target. We layer a transparent container with a colored border
        // on top instead of restyling the pane container itself, so the
        // border shows above the file rows.
        let internal_drag_in =
            self.sftp.drag.as_ref().is_some_and(|d| {
                d.active && d.origin_side != SftpPaneSide::Remote
            });
        if self.sftp.drop_active || internal_drag_in {
            let outline = container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| container::Style {
                    border: Border {
                        radius: Radius::from(0.0),
                        color: OryxisColors::t().accent,
                        width: 2.0,
                    },
                    ..Default::default()
                });
            stack = stack.push(outline);
        }
        stack.into()
    }

    fn view_sftp_picker(&self) -> Element<'_, Message> {
        let needle = self.sftp.picker_search.to_lowercase();
        let matches: Vec<usize> = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                if needle.is_empty() {
                    true
                } else {
                    c.label.to_lowercase().contains(&needle)
                        || c.hostname.to_lowercase().contains(&needle)
                }
            })
            .map(|(i, _)| i)
            .collect();

        let mut list = column![].spacing(4);
        for ci in matches {
            let conn = &self.connections[ci];
            let active = self
                .tabs
                .iter()
                .any(|t| t.label.trim_end_matches(" (disconnected)") == conn.label);
            let status_color = if active {
                OryxisColors::t().success
            } else {
                OryxisColors::t().text_muted
            };
            let status_text = if active { "reuse open session" } else { conn.hostname.as_str() };
            let fallback = if active {
                OryxisColors::t().success
            } else {
                OryxisColors::t().accent
            };
            let (glyph, badge_color) =
                crate::os_icon::resolve_icon(conn.detected_os.as_deref(), fallback);
            let badge = container(glyph.size(11).color(Color::WHITE))
                .center_x(Length::Fixed(22.0))
                .center_y(Length::Fixed(22.0))
                .style(move |_| container::Style {
                    background: Some(Background::Color(badge_color)),
                    border: Border { radius: Radius::from(5.0), ..Default::default() },
                    ..Default::default()
                });
            let row_btn = button(
                row![
                    badge,
                    Space::new().width(10),
                    column![
                        text(conn.label.clone()).size(13).color(OryxisColors::t().text_primary),
                        text(status_text).size(10).color(status_color),
                    ],
                ]
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SftpPickHost(ci))
            .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .width(Length::Fill)
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
            });
            list = list.push(row_btn);
        }

        let dialog = container(
            column![
                row![
                    text("Select a host").size(15).color(OryxisColors::t().text_primary),
                    Space::new().width(Length::Fill),
                    button(
                        iced_fonts::lucide::x()
                            .size(13)
                            .color(OryxisColors::t().text_muted),
                    )
                    .on_press(Message::SftpClosePicker)
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
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
                    }),
                ]
                .align_y(iced::Alignment::Center)
                .width(Length::Fill),
                Space::new().height(8),
                text_input("Search hosts…", &self.sftp.picker_search)
                    .on_input(Message::SftpPickerSearch)
                    .padding(10)
                    .style(crate::widgets::rounded_input_style),
                Space::new().height(8),
                scrollable(list).height(Length::Fixed(360.0)),
            ]
            .padding(20)
            .width(Length::Fixed(440.0)),
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
        .on_press(Message::SftpClosePicker)
        .into();

        // Wrap the dialog in a MouseArea that swallows clicks via
        // `NoOp`, otherwise events fall through the Stack to the scrim
        // underneath and the picker closes on every click inside it.
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
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pane_actions_btn<'a>(toggle_msg: Message) -> Element<'a, Message> {
    button(
        text("\u{22EE}").size(14).color(OryxisColors::t().text_secondary),
    )
    .on_press(toggle_msg)
    .padding(Padding { top: 2.0, right: 8.0, bottom: 2.0, left: 8.0 })
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

/// Floating Actions menu for a pane — anchored to the top-right via a
/// container that pushes it to the corner.
fn actions_menu_overlay<'a>(local: bool, show_hidden: bool) -> Element<'a, Message> {
    let refresh_msg = if local {
        Message::SftpRefreshLocal
    } else {
        Message::SftpCloseMenus
    };
    let side = if local { SftpPaneSide::Local } else { SftpPaneSide::Remote };
    let hidden_msg = if local {
        Message::SftpToggleLocalHidden
    } else {
        Message::SftpToggleRemoteHidden
    };
    let hidden_label = if show_hidden { "Hide hidden files" } else { "Show hidden files" };
    let menu = container(
        column![
            menu_item(
                iced_fonts::lucide::folder_plus(),
                "New folder",
                Message::SftpStartNewEntry(side, SftpEntryKind::Folder),
            ),
            menu_item(
                iced_fonts::lucide::file_plus(),
                "New file",
                Message::SftpStartNewEntry(side, SftpEntryKind::File),
            ),
            menu_separator(),
            menu_item(iced_fonts::lucide::rotate_cw(), "Refresh", refresh_msg),
            menu_item(iced_fonts::lucide::eye(), hidden_label, hidden_msg),
        ]
        .spacing(2)
        .padding(4),
    )
    // Pin the menu to the same width as the rows inside it. Without
    // this, `menu_separator`'s `Length::Fill` propagates up through
    // `column![]` and the outer container, stretching the dropdown
    // across the entire pane.
    .width(Length::Fixed(228.0))
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Message::SftpCloseMenus)
    .into();
    let positioned = container(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top)
        .padding(Padding { top: 48.0, right: 14.0, bottom: 0.0, left: 0.0 });
    iced::widget::Stack::new()
        .push(scrim)
        .push(positioned)
        .into()
}

fn menu_separator<'a>() -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().border)),
            ..Default::default()
        })
        .into()
}

/// Right-click row context menu — items vary by pane side and entry
/// type. When the clicked row is part of a multi-selection (same pane),
/// the menu switches to bulk variants: count-aware Delete; single-only
/// ops (Rename, Edit) hide.
pub(crate) fn row_context_menu_box<'a>(
    menu: &crate::state::SftpRowMenu,
    remote_connected: bool,
    selection_count_same_pane: usize,
) -> Element<'a, Message> {
    let multi = selection_count_same_pane > 1;
    let mut items = column![].spacing(2).padding(4);
    // Upload — local files/folders when a remote session is mounted.
    // Multi mode batches the whole same-pane selection.
    let accent = OryxisColors::t().accent;
    let secondary = OryxisColors::t().text_secondary;
    let danger = OryxisColors::t().error;
    if menu.side == SftpPaneSide::Local {
        if remote_connected {
            if multi {
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::upload(),
                    format!("Upload {} items", selection_count_same_pane),
                    Message::SftpUploadSelection,
                    accent,
                ));
            } else {
                let upload_msg = if menu.is_dir {
                    Message::SftpUploadFolder(std::path::PathBuf::from(&menu.path))
                } else {
                    Message::SftpUpload(std::path::PathBuf::from(&menu.path))
                };
                items = items.push(menu_item_tinted(
                    iced_fonts::lucide::upload(),
                    "Upload to remote",
                    upload_msg,
                    accent,
                ));
            }
        }
        // Open the file in the OS default editor. Unlike the remote
        // Edit (which has to download to temp + watch mtime), here we
        // just hand the path to `open` — the user's edits hit the
        // file directly with no roundtrip.
        if !multi && !menu.is_dir {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::SftpOpenLocal(std::path::PathBuf::from(&menu.path)),
                secondary,
            ));
        }
    }
    if menu.side == SftpPaneSide::Remote {
        if multi {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::download(),
                format!("Download {} items", selection_count_same_pane),
                Message::SftpDownloadSelection,
                accent,
            ));
        } else {
            let download_msg = if menu.is_dir {
                Message::SftpDownloadFolder(menu.path.clone())
            } else {
                Message::SftpDownload(menu.path.clone())
            };
            items = items.push(menu_item_tinted(
                iced_fonts::lucide::download(),
                "Download to local",
                download_msg,
                accent,
            ));
            // Edit-in-place — only meaningful for single files.
            if !menu.is_dir {
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::pencil(),
                    crate::i18n::t("edit").to_string(),
                    Message::SftpStartEdit(menu.path.clone()),
                    secondary,
                ));
            }
        }
    }
    if multi {
        items = items.push(menu_item_owned_tinted(
            iced_fonts::lucide::copy(),
            format!("Duplicate {} items", selection_count_same_pane),
            Message::SftpDuplicateSelection,
            secondary,
        ));
    } else {
        let duplicate_msg = if menu.is_dir {
            Message::SftpDuplicateFolder(menu.side, menu.path.clone())
        } else {
            Message::SftpDuplicate(menu.side, menu.path.clone())
        };
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::copy(),
            "Duplicate",
            duplicate_msg,
            secondary,
        ));
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::pencil(),
            "Rename",
            Message::SftpStartRename(menu.side, menu.path.clone()),
            secondary,
        ));
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::cog(),
            "Properties",
            Message::SftpShowProperties(menu.side, menu.path.clone(), menu.is_dir),
            secondary,
        ));
    }
    let delete_label = if multi {
        format!("Delete {} items", selection_count_same_pane)
    } else {
        "Delete".to_string()
    };
    let delete_msg = if multi {
        Message::SftpAskDeleteSelection
    } else {
        Message::SftpAskDelete(menu.side, menu.path.clone(), menu.is_dir)
    };
    items = items.push(menu_item_owned_tinted(
        iced_fonts::lucide::trash(),
        delete_label,
        delete_msg,
        danger,
    ));

    container(items)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        })
        .into()
}

/// Compute the approximate height of the row context menu given the
/// current target — keeps the layout-level clamp accurate so the menu
/// never spills off the bottom or right edge of the window.
pub(crate) fn row_context_menu_height(
    menu: &crate::state::SftpRowMenu,
    remote_connected: bool,
    selection_count_same_pane: usize,
) -> f32 {
    let multi = selection_count_same_pane > 1;
    // Multi mode: Upload OR Download (one based on side) + Duplicate +
    // Delete. Single mode: Duplicate + Rename + Properties + Delete +
    // optional Upload/Download/Edit.
    let mut count = if multi { 2.0 } else { 4.0 };
    if menu.side == SftpPaneSide::Local && remote_connected {
        count += 1.0; // Upload
    }
    if menu.side == SftpPaneSide::Remote {
        count += 1.0; // Download
        if !multi && !menu.is_dir {
            count += 1.0; // Edit
        }
    }
    // Each item ~30px (padding 6+6 + ~12px text + 2px gap) plus 8px container padding.
    count * 30.0 + 8.0
}

/// Width is fixed because every item uses the same `menu_item` width.
pub(crate) const ROW_CONTEXT_MENU_WIDTH: f32 = 220.0;

/// Owned-label variant of `menu_item` for cases where the label is
/// computed at runtime (e.g. "Delete N items" with a dynamic count).
/// Owned-label variant that lets the caller pick the icon tint —
/// used for destructive (red) and primary (accent / success) actions
/// to match the host-card context menu's color coding.
fn menu_item_owned_tinted<'a>(
    icon: iced::widget::Text<'a>,
    label: String,
    msg: Message,
    tint: Color,
) -> Element<'a, Message> {
    button(
        row![
            icon.size(12).color(tint),
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
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

fn menu_item<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
) -> Element<'a, Message> {
    menu_item_tinted(icon, label, msg, OryxisColors::t().text_secondary)
}

/// Like `menu_item` but with an explicit icon tint (red for delete,
/// accent for primary actions, etc.).
fn menu_item_tinted<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
    tint: Color,
) -> Element<'a, Message> {
    button(
        row![
            icon.size(12).color(tint),
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 10.0 })
    .width(Length::Fixed(220.0))
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

/// Drive picker dropdown for Windows local pane. Lists `C:`, `D:`, etc.
/// based on what's actually mounted. Closed via the scrim.
fn drives_menu_overlay<'a>() -> Element<'a, Message> {
    let drives = list_windows_drives();
    let mut col = column![].spacing(2).padding(4);
    if drives.is_empty() {
        col = col.push(
            container(text("No drives detected").size(11).color(OryxisColors::t().text_muted))
                .padding(8),
        );
    } else {
        for drive in drives {
            let drive_path: std::path::PathBuf = format!("{}\\", drive).into();
            col = col.push(
                button(
                    row![
                        iced_fonts::lucide::hard_drive()
                            .size(12)
                            .color(OryxisColors::t().accent),
                        Space::new().width(8),
                        text(drive.clone()).size(12).color(OryxisColors::t().text_primary),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::SftpNavigateLocal(drive_path))
                .padding(Padding { top: 6.0, right: 16.0, bottom: 6.0, left: 10.0 })
                .width(Length::Fixed(160.0))
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
                }),
            );
        }
    }
    let menu = container(col).style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            radius: Radius::from(8.0),
            color: OryxisColors::t().border,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    let scrim: Element<'_, Message> = MouseArea::new(
        container(Space::new()).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Message::SftpCloseMenus)
    .into();
    let positioned = container(menu)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Left)
        .align_y(iced::alignment::Vertical::Top)
        .padding(Padding { top: 70.0, right: 0.0, bottom: 0.0, left: 14.0 });
    iced::widget::Stack::new().push(scrim).push(positioned).into()
}

/// Enumerate available drive letters on Windows. Empty on non-Windows
/// hosts (the dropdown isn't rendered there). When running under WSL,
/// surface `\\wsl.localhost` as a synthetic root so the user can hop
/// between WSL distros without dropping to a terminal.
fn list_windows_drives() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        let mut drives = Vec::new();
        for letter in b'A'..=b'Z' {
            let path = format!("{}:\\", letter as char);
            if std::path::Path::new(&path).exists() {
                drives.push(format!("{}:", letter as char));
            }
        }
        // WSL distros live under \\wsl.localhost (or the legacy
        // \\wsl$). `Path::exists()` on a UNC root returns false until
        // the SMB redirector lazily mounts it, so we detect WSL via
        // `wsl.exe` in System32 — present iff the user has WSL
        // installed at all. We expose `\\wsl$` as the entry point
        // because it's the alias that always resolves; navigating into
        // it lists distros as folders.
        let wsl_exe = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
            .join("System32")
            .join("wsl.exe");
        if wsl_exe.exists() {
            drives.push(r"\\wsl$".to_string());
        }
        drives
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Build a clickable breadcrumb for a remote POSIX path. The root is
/// the only `/` rendered — subsequent segments are added with separators
/// in between, never *after* the root crumb itself, which avoids the
/// `/ / home` doubling that crept in when separators were emitted at the
/// start of every iteration.
fn remote_breadcrumb<'a>(path: &str) -> Element<'a, Message> {
    let mut row = iced::widget::Row::new().align_y(iced::Alignment::Center).spacing(2);
    row = row.push(crumb_remote("/", "/"));
    let mut accumulated = String::new();
    let mut first_segment = true;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        accumulated.push('/');
        accumulated.push_str(segment);
        if !first_segment {
            row = row.push(text("/").size(11).color(OryxisColors::t().text_muted));
        }
        first_segment = false;
        row = row.push(crumb_remote(segment, &accumulated));
    }
    row.into()
}

/// Build a clickable breadcrumb for a local filesystem path. On Windows
/// the first crumb is the drive letter and clicking it opens the drive
/// picker dropdown. The Unix root chip swallows the next separator so
/// the visual reads `/ home / user` instead of `/ / home / user`. On
/// Windows the implicit `RootDir` component after the drive prefix is
/// skipped (its job is taken by the drive chip itself).
fn local_breadcrumb<'a>(path: &std::path::Path) -> Element<'a, Message> {
    let mut row = iced::widget::Row::new().align_y(iced::Alignment::Center).spacing(2);
    let mut accumulated = std::path::PathBuf::new();
    let mut first = true;
    let mut last_was_root_or_drive = false;
    let mut had_drive = false;
    for component in path.components() {
        let (label, is_drive, is_root) = match component {
            std::path::Component::Prefix(p) => {
                had_drive = true;
                (p.as_os_str().to_string_lossy().into_owned(), true, false)
            }
            std::path::Component::RootDir => {
                // Skip the implicit root component on Windows — the drive
                // chip already represents the volume root.
                if had_drive {
                    accumulated.push(component.as_os_str());
                    last_was_root_or_drive = true;
                    continue;
                }
                ("/".to_string(), false, true)
            }
            std::path::Component::Normal(s) => (s.to_string_lossy().into_owned(), false, false),
            std::path::Component::CurDir | std::path::Component::ParentDir => continue,
        };
        accumulated.push(component.as_os_str());
        if !first && !last_was_root_or_drive {
            row = row.push(text("/").size(11).color(OryxisColors::t().text_muted));
        }
        first = false;
        last_was_root_or_drive = is_root || is_drive;
        if is_drive {
            // Drive-letter chip toggles the drives dropdown so the user
            // can jump to another mount without typing.
            row = row.push(
                button(
                    row![
                        iced_fonts::lucide::hard_drive()
                            .size(11)
                            .color(OryxisColors::t().accent),
                        Space::new().width(4),
                        text(label).size(11).color(OryxisColors::t().text_secondary),
                        Space::new().width(2),
                        iced_fonts::lucide::chevron_down()
                            .size(9)
                            .color(OryxisColors::t().text_muted),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::SftpToggleLocalDrives)
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
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
                }),
            );
        } else {
            row = row.push(local_crumb(label, accumulated.clone()));
        }
    }
    row.into()
}

fn crumb_remote<'a>(label: &str, full: &str) -> Element<'a, Message> {
    let label = label.to_string();
    let full = full.to_string();
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::SftpNavigateRemote(full))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
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

fn local_crumb<'a>(label: String, full: std::path::PathBuf) -> Element<'a, Message> {
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::SftpNavigateLocal(full))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
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

fn parent_row<'a>(local: bool) -> Element<'a, Message> {
    let msg = if local { Message::SftpLocalUp } else { Message::SftpRemoteUp };
    let inner = row![
        iced_fonts::lucide::folder()
            .size(13)
            .color(OryxisColors::t().text_muted),
        Space::new().width(8),
        text("..").size(12).color(OryxisColors::t().text_muted).width(Length::Fill),
        text(String::new()).size(11).color(OryxisColors::t().text_muted).width(Length::Fixed(MOD_COL_W)),
        text(String::new()).size(11).color(OryxisColors::t().text_muted).width(Length::Fixed(SIZE_COL_W)),
    ]
    .align_y(iced::Alignment::Center);
    button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .on_press(msg)
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                ..Default::default()
            }
        })
        .into()
}

const MOD_COL_W: f32 = 140.0;
const SIZE_COL_W: f32 = 80.0;

/// Visually distinct band that wraps the toolbar / breadcrumb / column
/// headers — gives the file list a clean separation from the chrome,
/// matching how Finder / Explorer / Termius split the two regions.
fn pane_header_band<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            border: Border {
                width: 0.0,
                color: OryxisColors::t().border,
                radius: Radius::from(0.0),
            },
            ..Default::default()
        })
        .into()
}

/// Sortable column header strip. Click on a column to set / flip the
/// active sort. The active column shows an arrow indicator.
fn column_headers<'a>(
    local: bool,
    sort: crate::state::SftpSort,
) -> Element<'a, Message> {
    use crate::state::SftpSortColumn;
    let header = |label: &'static str, col: SftpSortColumn, width: Option<f32>| -> Element<'a, Message> {
        let arrow = if sort.column == col {
            if sort.ascending { " \u{2191}" } else { " \u{2193}" }
        } else {
            ""
        };
        let txt = text(format!("{}{}", label, arrow))
            .size(11)
            .color(if sort.column == col {
                OryxisColors::t().text_primary
            } else {
                OryxisColors::t().text_muted
            });
        let msg = if local { Message::SftpSortLocal(col) } else { Message::SftpSortRemote(col) };
        let mut btn = button(txt)
            .on_press(msg)
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
            });
        if let Some(w) = width {
            btn = btn.width(Length::Fixed(w));
        } else {
            btn = btn.width(Length::Fill);
        }
        btn.into()
    };
    container(
        row![
            // Pad-icon column to align with file rows below.
            Space::new().width(Length::Fixed(21.0)),
            header("Name", SftpSortColumn::Name, None),
            header("Modified", SftpSortColumn::Modified, Some(MOD_COL_W)),
            header("Size", SftpSortColumn::Size, Some(SIZE_COL_W)),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_surface)),
        border: Border {
            width: 0.0,
            color: OryxisColors::t().border,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    })
    .into()
}

fn format_modified_local(modified: Option<std::time::SystemTime>) -> String {
    let Some(t) = modified else { return String::new() };
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn format_modified_remote(mtime: Option<u32>) -> String {
    let Some(secs) = mtime else { return String::new() };
    match chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn file_row_local<'a>(
    name: String,
    is_dir: bool,
    size_str: String,
    modified: Option<std::time::SystemTime>,
    path: std::path::PathBuf,
    rename_input: Option<&str>,
    is_selected: bool,
    is_drop_target: bool,
) -> Element<'a, Message> {
    let icon = file_icon(&name, is_dir, false);

    // Inline rename mode swaps the row's label for a text input; the
    // icon + columns stay put so the row geometry doesn't jump.
    let label_widget: Element<'_, Message> = if let Some(input) = rename_input {
        text_input(&name, input)
            .on_input(Message::SftpRenameInput)
            .on_submit(Message::SftpRenameCommit)
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .size(11)
            .style(crate::widgets::rounded_input_style)
            .into()
    } else {
        text(name).size(12).color(OryxisColors::t().text_primary).width(Length::Fill).into()
    };

    let inner = row![
        icon,
        Space::new().width(8),
        label_widget,
        text(format_modified_local(modified))
            .size(11)
            .color(OryxisColors::t().text_muted)
            .width(Length::Fixed(MOD_COL_W)),
        text(size_str)
            .size(11)
            .color(OryxisColors::t().text_muted)
            .width(Length::Fixed(SIZE_COL_W)),
    ]
    .align_y(iced::Alignment::Center);

    // Click action priority: while renaming, swallow clicks; folders
    // navigate; files mark themselves selected so the user has visible
    // confirmation that the row is interactive (was previously a disabled
    // button — no hover, no pointer cursor, looked dead).
    let path_str = path.to_string_lossy().into_owned();
    // SftpSelectRow handles plain folder click (navigate), file click
    // (single-select), and modifier clicks (toggle / range). Routing it
    // all through one message means modifier state can be consulted
    // server-side instead of being stored at button-build time.
    let on_click = if rename_input.is_some() {
        None
    } else {
        Some(Message::SftpSelectRow(
            SftpPaneSide::Local,
            path_str.clone(),
            is_dir,
        ))
    };
    let path_for_enter = path_str.clone();
    let mut btn = button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(move |_, status| {
            // Drop highlight beats selection while a drag is in flight,
            // matches the right-pane logic.
            let bg = if is_drop_target || is_selected {
                Color { a: 0.20, ..OryxisColors::t().accent }
            } else {
                match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                ..Default::default()
            }
        });
    if let Some(msg) = on_click {
        btn = btn.on_press(msg);
    }
    // Hover events feed both the OS drag drop targeting and the new
    // internal drag-drop press handler — needed even on file rows since
    // a file is a valid drag *source* (just not a drop *target*).
    MouseArea::new(btn)
        .on_right_press(Message::SftpRowRightClick(
            SftpPaneSide::Local,
            path_str,
            is_dir,
        ))
        .on_enter(Message::SftpRowEnter(
            SftpPaneSide::Local,
            path_for_enter,
            is_dir,
        ))
        .on_exit(Message::SftpRowExit)
        .into()
}

#[allow(clippy::too_many_arguments)]
fn file_row_remote<'a>(
    name: String,
    is_dir: bool,
    is_symlink: bool,
    size_str: String,
    mtime: Option<u32>,
    full_path: String,
    rename_input: Option<&str>,
    is_drop_target: bool,
    is_selected: bool,
) -> Element<'a, Message> {
    let icon = file_icon(&name, is_dir, is_symlink);

    let label_widget: Element<'_, Message> = if let Some(input) = rename_input {
        text_input(&name, input)
            .on_input(Message::SftpRenameInput)
            .on_submit(Message::SftpRenameCommit)
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .size(11)
            .style(crate::widgets::rounded_input_style)
            .into()
    } else {
        text(name).size(12).color(OryxisColors::t().text_primary).width(Length::Fill).into()
    };

    // Single message routes folder navigation, file single-select, and
    // ctrl/shift modifier selection — see the local row counterpart.
    // Symlinks behave like folders for click (treat as nav target) since
    // we can't tell from the listing whether they point at a file vs dir.
    let nav_target = if rename_input.is_some() {
        None
    } else {
        Some(Message::SftpSelectRow(
            SftpPaneSide::Remote,
            full_path.clone(),
            is_dir || is_symlink,
        ))
    };
    let inner = row![
        icon,
        Space::new().width(8),
        label_widget,
        text(format_modified_remote(mtime))
            .size(11)
            .color(OryxisColors::t().text_muted)
            .width(Length::Fixed(MOD_COL_W)),
        text(size_str)
            .size(11)
            .color(OryxisColors::t().text_muted)
            .width(Length::Fixed(SIZE_COL_W)),
    ]
    .align_y(iced::Alignment::Center);
    let mut btn = button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(move |_, status| {
            // Drop highlight beats selection (transient, communicates
            // imminent action), selection beats default hover.
            let bg = if is_drop_target || is_selected {
                Color { a: 0.20, ..OryxisColors::t().accent }
            } else {
                match status {
                    BtnStatus::Hovered => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                ..Default::default()
            }
        });
    if let Some(msg) = nav_target {
        btn = btn.on_press(msg);
    }
    // Hover events update the global hovered_row state. That state
    // serves the OS drop target picker, the internal drag-drop press
    // handler, and the cross-pane folder drop highlight.
    MouseArea::new(btn)
        .on_right_press(Message::SftpRowRightClick(
            SftpPaneSide::Remote,
            full_path.clone(),
            is_dir,
        ))
        .on_enter(Message::SftpRowEnter(
            SftpPaneSide::Remote,
            full_path,
            is_dir,
        ))
        .on_exit(Message::SftpRowExit)
        .into()
}

fn file_icon<'a>(name: &str, is_dir: bool, is_symlink: bool) -> iced::widget::Text<'a> {
    if is_dir {
        return iced_fonts::lucide::folder()
            .size(13)
            .color(OryxisColors::t().accent);
    }
    if is_symlink {
        return iced_fonts::lucide::file_symlink()
            .size(13)
            .color(OryxisColors::t().accent);
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let (glyph, color) = match ext.as_deref() {
        Some("rs") | Some("ts") | Some("js") | Some("py") | Some("go") | Some("c") | Some("cpp")
        | Some("h") | Some("hpp") | Some("java") | Some("kt") | Some("rb") | Some("php")
        | Some("sh") | Some("bash") | Some("zsh") | Some("fish") | Some("vim") | Some("lua") => (
            iced_fonts::lucide::file_code(),
            OryxisColors::t().success,
        ),
        Some("json") | Some("yaml") | Some("yml") | Some("toml") | Some("ini") | Some("env")
        | Some("conf") | Some("cfg") => (
            iced_fonts::lucide::file_cog(),
            OryxisColors::t().warning,
        ),
        Some("md") | Some("txt") | Some("rst") | Some("log") => (
            iced_fonts::lucide::file_text(),
            OryxisColors::t().text_secondary,
        ),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("svg") | Some("webp")
        | Some("bmp") | Some("ico") => (
            iced_fonts::lucide::file_image(),
            OryxisColors::t().accent,
        ),
        Some("mp4") | Some("mkv") | Some("mov") | Some("avi") | Some("webm") => (
            iced_fonts::lucide::file_video(),
            OryxisColors::t().accent,
        ),
        Some("mp3") | Some("wav") | Some("flac") | Some("ogg") | Some("m4a") => (
            iced_fonts::lucide::file_audio(),
            OryxisColors::t().accent,
        ),
        Some("zip") | Some("tar") | Some("gz") | Some("bz2") | Some("xz") | Some("7z")
        | Some("rar") | Some("deb") | Some("rpm") => (
            iced_fonts::lucide::file_archive(),
            OryxisColors::t().warning,
        ),
        Some("pdf") => (
            iced_fonts::lucide::file_text(),
            OryxisColors::t().error,
        ),
        Some("csv") | Some("xlsx") | Some("xls") => (
            iced_fonts::lucide::file_spreadsheet(),
            OryxisColors::t().success,
        ),
        Some("html") | Some("htm") | Some("css") | Some("scss") => (
            iced_fonts::lucide::file_code(),
            OryxisColors::t().accent,
        ),
        Some("key") | Some("pem") | Some("crt") | Some("cer") => (
            iced_fonts::lucide::file_key(),
            OryxisColors::t().warning,
        ),
        _ => (
            iced_fonts::lucide::file(),
            OryxisColors::t().text_muted,
        ),
    };
    glyph.size(13).color(color)
}

/// Confirmation dialog for the "Delete" action. Single targets show
/// the file name and a folder-recursive hint; bulk deletes show a count
/// and folder-vs-file breakdown so the user understands the blast
/// radius.
fn delete_confirm_modal<'a>(
    targets: &[crate::state::SftpDeleteTarget],
) -> Element<'a, Message> {
    let (title, detail) = if targets.len() == 1 {
        let target = &targets[0];
        let basename = target
            .path
            .rsplit(['/', '\\'])
            .find(|s| !s.is_empty())
            .unwrap_or(&target.path)
            .to_string();
        let detail = if target.is_dir {
            format!("\"{}\" — folder and all its contents", basename)
        } else {
            format!("\"{}\"", basename)
        };
        ("Delete this item?".to_string(), detail)
    } else {
        let folder_count = targets.iter().filter(|t| t.is_dir).count();
        let file_count = targets.len() - folder_count;
        let detail = match (folder_count, file_count) {
            (0, n) => format!("{} files", n),
            (n, 0) => format!("{} folders (recursive)", n),
            (f, fi) => format!("{} folders (recursive) and {} files", f, fi),
        };
        (format!("Delete {} items?", targets.len()), detail)
    };
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(detail).size(13).color(OryxisColors::t().text_muted),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    "Delete",
                    Message::SftpConfirmDelete,
                    OryxisColors::t().error,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    "Cancel",
                    Message::SftpCancelDelete,
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
    .on_press(Message::SftpCancelDelete)
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

/// Modal for "New folder" / "New file" — single text input + create/cancel.
/// `Enter` in the input commits, mirroring the inline rename behaviour.
fn new_entry_modal<'a>(entry: &crate::state::SftpNewEntry) -> Element<'a, Message> {
    let title = match entry.kind {
        SftpEntryKind::Folder => "New folder",
        SftpEntryKind::File => "New file",
    };
    let placeholder = match entry.kind {
        SftpEntryKind::Folder => "folder name",
        SftpEntryKind::File => "file name",
    };
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(12),
            text_input(placeholder, &entry.input)
                .on_input(Message::SftpNewEntryInput)
                .on_submit(Message::SftpNewEntryCommit)
                .padding(10)
                .size(13)
                .style(crate::widgets::rounded_input_style),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    "Create",
                    Message::SftpNewEntryCommit,
                    OryxisColors::t().accent,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    "Cancel",
                    Message::SftpNewEntryCancel,
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
    .on_press(Message::SftpNewEntryCancel)
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

/// Modal shown while an edit-in-place session is active. The user has
/// the temp file open in their OS editor — when they're done they come
/// back here and either save the changes back to the remote or discard.
/// Backdrop is non-dismissable on click; the user must explicitly choose
/// so a stray click can't drop their edits.
fn edit_in_place_modal<'a>(
    session: &crate::state::EditSession,
) -> Element<'a, Message> {
    let (status_text, status_color) = if session.dirty {
        (
            "Changes detected — ready to upload back.".to_string(),
            OryxisColors::t().accent,
        )
    } else {
        (
            "Waiting for changes… save in the editor and we'll pick it up.".to_string(),
            OryxisColors::t().text_muted,
        )
    };
    let title = if session.dirty {
        "Editing file (modified)"
    } else {
        "Editing file"
    };
    let dialog = container(
        column![
            text(title).size(16).color(OryxisColors::t().text_primary),
            Space::new().height(6),
            text(session.label.clone())
                .size(13)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            text(format!("Local copy: {}", session.temp_path.display()))
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(4),
            text(session.remote_path.clone())
                .size(11)
                .color(OryxisColors::t().text_muted),
            Space::new().height(16),
            text(status_text).size(12).color(status_color),
            Space::new().height(16),
            row![
                crate::widgets::styled_button(
                    "Save back to remote",
                    Message::SftpEditSave,
                    OryxisColors::t().accent,
                ),
                Space::new().width(8),
                crate::widgets::styled_button(
                    "Discard",
                    Message::SftpEditDiscard,
                    OryxisColors::t().text_muted,
                ),
            ],
        ]
        .padding(24)
        .width(440),
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

    // Solid scrim with no on_press — the modal is intentionally
    // non-dismissable on backdrop click. Clicking outside does nothing
    // so the user is forced to make an explicit save/discard choice and
    // can't lose their edits to a misclick.
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
                    ..iced::Font::with_name(crate::theme::SYSTEM_UI_FAMILY)
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

/// Properties dialog — shows the standard file metadata (path, size,
/// mtime, owner) and a 3×3 grid of permission checkboxes for r/w/x
/// across owner / group / others. Apply runs chmod; bits the dialog
/// doesn't render (setuid/setgid/sticky) are preserved verbatim from
/// the original mode.
fn properties_modal<'a>(
    props: &crate::state::PropertiesView,
) -> Element<'a, Message> {
    let basename = props
        .path
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(&props.path)
        .to_string();
    let kind = if props.is_dir { "Folder" } else { "File" };
    let mtime_str = props
        .mtime
        .and_then(|secs| chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0))
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".to_string());
    let owner_str = match (props.owner_uid, props.owner_gid) {
        (Some(u), Some(g)) => format!("uid {u} · gid {g}"),
        (Some(u), None) => format!("uid {u}"),
        _ => "—".to_string(),
    };
    let mode_octal = format!("{:o}", (props.original_mode & !0o777) | props.bits.to_mode());

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
            .on_press(Message::SftpPropertiesToggleBit(bit))
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

    let perm_grid = column![
        row![
            Space::new().width(Length::Fixed(80.0)),
            container(text("R").size(11).color(OryxisColors::t().text_muted))
                .width(Length::Fixed(26.0))
                .center_x(Length::Fixed(26.0)),
            Space::new().width(8),
            container(text("W").size(11).color(OryxisColors::t().text_muted))
                .width(Length::Fixed(26.0))
                .center_x(Length::Fixed(26.0)),
            Space::new().width(8),
            container(text("X").size(11).color(OryxisColors::t().text_muted))
                .width(Length::Fixed(26.0))
                .center_x(Length::Fixed(26.0)),
        ],
        Space::new().height(4),
        perm_row(
            "Owner",
            (props.bits.user_r, crate::state::PermBit::UserR),
            (props.bits.user_w, crate::state::PermBit::UserW),
            (props.bits.user_x, crate::state::PermBit::UserX),
        ),
        Space::new().height(2),
        perm_row(
            "Group",
            (props.bits.group_r, crate::state::PermBit::GroupR),
            (props.bits.group_w, crate::state::PermBit::GroupW),
            (props.bits.group_x, crate::state::PermBit::GroupX),
        ),
        Space::new().height(2),
        perm_row(
            "Others",
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
                ..iced::Font::with_name(crate::theme::SYSTEM_UI_FAMILY)
            })
            .color(OryxisColors::t().text_primary),
        Space::new().height(4),
        text(props.path.clone())
            .size(11)
            .color(OryxisColors::t().text_muted),
        Space::new().height(14),
        info_row("Type", kind.to_string()),
        Space::new().height(4),
        info_row("Size", format_size(props.size)),
        Space::new().height(4),
        info_row("Modified", mtime_str),
        Space::new().height(4),
        info_row("Owner", owner_str),
        Space::new().height(4),
        info_row("Mode", format!("0{}", mode_octal)),
        Space::new().height(16),
        header_row("Permissions"),
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
    let apply_label = if props.applying { "Applying…" } else { "Apply" };
    content = content.push(
        row![
            ghost_button("Close", Message::SftpPropertiesClose),
            Space::new().width(Length::Fill),
            primary_button(apply_label, Message::SftpPropertiesApply, OryxisColors::t().accent),
        ]
        .align_y(iced::Alignment::Center),
    );

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
    .on_press(Message::SftpPropertiesClose)
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
/// Lays out the choices as a single horizontal row of buttons —
/// destructive primary on the right, secondary outlined options in the
/// middle, ghost-style cancel on the left — so the modal stays compact
/// instead of stacking four heavy buttons vertically. The scrim is
/// non-dismissable: the user must pick something explicitly.
fn overwrite_modal<'a>(
    prompt: &crate::state::OverwritePrompt,
) -> Element<'a, Message> {
    let size_hint = if prompt.src_size == prompt.dst_size {
        format!(
            "{} on both sides — likely identical",
            format_size(prompt.src_size)
        )
    } else {
        format!(
            "Local {} · Remote {}",
            format_size(prompt.src_size),
            format_size(prompt.dst_size)
        )
    };

    let mut content = column![
        text(format!("\"{}\" already exists", prompt.basename))
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..iced::Font::with_name(crate::theme::SYSTEM_UI_FAMILY)
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

    if prompt.multi {
        // Sticky decision lets the user clear out a long upload's
        // collisions in one click instead of answering N times.
        content = content.push(Space::new().height(14));
        content = content.push(overwrite_apply_to_all_checkbox(prompt.apply_to_all));
    }

    content = content.push(Space::new().height(18));
    content = content.push(
        row![
            ghost_button(
                "Cancel",
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::Cancel),
            ),
            Space::new().width(Length::Fill),
            outlined_button(
                "Replace if different",
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::ReplaceIfDifferent),
            ),
            Space::new().width(8),
            outlined_button(
                "Duplicate",
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::Duplicate),
            ),
            Space::new().width(8),
            primary_button(
                "Replace",
                Message::SftpResolveOverwrite(crate::state::OverwriteAction::Replace),
                OryxisColors::t().error,
            ),
        ]
        .align_y(iced::Alignment::Center),
    );

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

    // Non-dismissable scrim — clicking outside is not a valid answer
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

/// Filled primary action button — destructive variants pass red, neutral
/// pass accent. Slightly more compact than `widgets::styled_button` so it
/// sits well in a horizontal modal footer row.
fn primary_button<'a>(label: &'a str, msg: Message, color: Color) -> Element<'a, Message> {
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
                ..iced::Font::with_name(crate::theme::SYSTEM_UI_FAMILY)
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

/// Outlined secondary button — transparent fill with a subtle border.
/// Hover fills with a faint accent tint to communicate clickability
/// without competing with the primary action visually.
fn outlined_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
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

/// Ghost button — pure text on transparent, hover-only background tint.
/// Right for the lowest-emphasis action (Cancel here).
fn ghost_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
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
fn overwrite_apply_to_all_checkbox<'a>(checked: bool) -> Element<'a, Message> {
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
            text("Apply to remaining files in this transfer")
                .size(12)
                .color(OryxisColors::t().text_secondary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::SftpToggleApplyToAll)
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

/// Bottom-of-view strip that surfaces an in-progress folder transfer:
/// kind label, current item, count, slim progress bar, and a cancel
/// button. Stays compact so the file panes lose as little vertical
/// space as possible.
fn transfer_progress_strip<'a>(
    transfer: &crate::state::TransferState,
) -> Element<'a, Message> {
    let label = match transfer.kind {
        crate::state::TransferKind::Upload => "Uploading",
        crate::state::TransferKind::Download => "Downloading",
        crate::state::TransferKind::DuplicateLocal => "Duplicating",
    };
    let current = transfer
        .current
        .clone()
        .unwrap_or_else(|| "preparing…".to_string());
    let count = format!("{} / {}", transfer.completed, transfer.total);
    let pct = if transfer.total == 0 {
        0.0
    } else {
        (transfer.completed as f32 / transfer.total as f32).clamp(0.0, 1.0)
    };
    // Ratio-based progress bar built from two stacked containers — iced
    // 0.14 has ProgressBar, but a manual bar lets us match the rest of
    // the chrome's styling exactly.
    let bar = container(
        container(Space::new())
            .width(Length::FillPortion((pct * 1000.0) as u16))
            .height(Length::Fixed(4.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().accent)),
                border: Border { radius: Radius::from(2.0), ..Default::default() },
                ..Default::default()
            }),
    )
    .width(Length::Fill)
    .height(Length::Fixed(4.0))
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05))),
        border: Border { radius: Radius::from(2.0), ..Default::default() },
        ..Default::default()
    });

    let info = column![
        row![
            text(format!("{} {}", label, transfer.root_label))
                .size(11)
                .color(OryxisColors::t().text_secondary),
            Space::new().width(Length::Fill),
            text(count).size(11).color(OryxisColors::t().text_muted),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(2),
        text(current)
            .size(10)
            .color(OryxisColors::t().text_muted),
        Space::new().height(6),
        bar,
    ]
    .width(Length::Fill);

    let cancel_btn = button(
        text("Cancel").size(11).color(OryxisColors::t().text_secondary),
    )
    .on_press(Message::SftpCancelTransfer)
    .padding(Padding { top: 4.0, right: 10.0, bottom: 4.0, left: 10.0 })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
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
    });

    container(
        row![info, Space::new().width(12), cancel_btn]
            .align_y(iced::Alignment::Center),
    )
    .padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 })
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
        border: Border {
            width: 1.0,
            color: OryxisColors::t().border,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    })
    .into()
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[idx])
    }
}
