//! SFTP view helpers: menus. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::{column, row};
pub(crate) fn pane_actions_btn<'a>(toggle_msg: Message) -> Element<'a, Message> {
    crate::widgets::card_kebab_button(
        OryxisColors::t().text_secondary,
        true,
        toggle_msg,
    )
    .into()
}

/// The collapsed-filter input card. Positioned + scrimmed by the caller at
/// the `view_sftp` level.
pub(crate) fn filter_card<'a>(side: SftpPaneSide, filter: &str) -> Element<'a, Message> {
    let id = match side {
        SftpPaneSide::Left => "sftp-filter-pop-left",
        SftpPaneSide::Right => "sftp-filter-pop-right",
    };
    let input = text_input(t("filter_placeholder"), filter)
        .id(iced::widget::Id::new(id))
        .on_input(move |s| Message::SftpFilter(side, s))
        .on_submit(Message::SftpToggleFilterSearch(side))
        .padding(Padding { top: 9.0, right: 12.0, bottom: 9.0, left: 12.0 })
        .size(13)
        .width(Length::Fixed(220.0))
        .style(crate::widgets::rounded_input_style)
        .align_x(dir_align_x());
    let card = container(input)
        .padding(6)
        .width(Length::Fixed(232.0))
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
    card.into()
}

/// Floating Actions menu for a pane, anchored to the top-right via a
/// container that pushes it to the corner.
/// The actions (`⋮`) menu card. Positioned + scrimmed by the caller at the
/// `view_sftp` level so a click anywhere (including the other pane) closes it.
pub(crate) fn actions_menu_card<'a>(
    side: SftpPaneSide,
    is_remote: bool,
    remote_path: &str,
    local_path: &std::path::Path,
    show_hidden: bool,
    cols: crate::state::SftpColumns,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    // Same directory-level actions as the cursor-anchored background menu,
    // shared via `dir_action_items` so the two never drift apart.
    let mut menu_col = column![].spacing(2).padding(4);
    let dir_ctx = DirActionCtx { pane_dir: remote_path, local_dir: local_path, show_hidden };
    for it in dir_action_items(side, is_remote, dir_ctx, true) {
        menu_col = menu_col.push(it);
    }
    // Columns section: toggle each optional column. The menu stays open on
    // click so several can be flipped in one pass.
    menu_col = menu_col.push(menu_separator());
    menu_col = menu_col.push(
        container(
            text(t("columns"))
                .size(10)
                .color(OryxisColors::t().text_muted),
        )
        .padding(Padding { top: 4.0, right: 10.0, bottom: 2.0, left: 10.0 }),
    );
    for (label, col, on) in [
        (t("col_modified"), SftpColumn::Modified, cols.modified),
        (t("col_size"), SftpColumn::Size, cols.size),
        (t("col_type"), SftpColumn::Kind, cols.kind),
        (t("col_permissions"), SftpColumn::Permissions, cols.permissions),
        (t("col_owner"), SftpColumn::Owner, cols.owner),
    ] {
        menu_col = menu_col.push(column_toggle_item(side, label, col, on));
    }
    let menu = container(menu_col)
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
    menu.into()
}

pub(crate) fn menu_separator<'a>() -> Element<'a, Message> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().border)),
            ..Default::default()
        })
        .into()
}

/// One row of the Columns section in the actions menu: a check glyph
/// (shown only when the column is visible) plus the column label. Firing
/// `SftpToggleColumn` flips the column without closing the menu.
pub(crate) fn column_toggle_item<'a>(
    side: SftpPaneSide,
    label: &'a str,
    col: crate::state::SftpColumn,
    visible: bool,
) -> Element<'a, Message> {
    let check = iced_fonts::lucide::check().size(12).color(if visible {
        OryxisColors::t().accent
    } else {
        Color::TRANSPARENT
    });
    button(
        row![
            check,
            Space::new().width(10),
            text(label).size(12).color(OryxisColors::t().text_primary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::SftpToggleColumn(side, col))
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

/// Right-click row context menu, items vary by pane side and entry
/// type. When the clicked row is part of a multi-selection (same pane),
/// the menu switches to bulk variants: count-aware Delete; single-only
/// ops (Rename, Edit) hide.
/// Pane context the directory-level actions need: the current directory
/// (target of New folder / New file / Refresh), the local path for
/// "Open in File Manager", and the hidden-files toggle state.
#[derive(Clone, Copy)]
pub(crate) struct DirActionCtx<'a> {
    pub pane_dir: &'a str,
    pub local_dir: &'a std::path::Path,
    pub show_hidden: bool,
}

pub(crate) fn row_context_menu_box<'a>(
    menu: &crate::state::SftpRowMenu,
    cross_pane_ready: bool,
    source_is_remote: bool,
    other_is_remote: bool,
    other_label: Option<String>,
    selection_count_same_pane: usize,
    dir_ctx: DirActionCtx<'_>,
) -> Element<'a, Message> {
    let multi = selection_count_same_pane > 1;
    let mut items = column![].spacing(2).padding(4);
    let accent = OryxisColors::t().accent;
    let secondary = OryxisColors::t().text_secondary;
    let danger = OryxisColors::t().error;
    // Background right-click (empty area): only directory-level actions,
    // no per-entry target exists. Same items as the pane's `⋮` menu.
    if menu.is_background {
        for it in dir_action_items(menu.side, source_is_remote, dir_ctx, true) {
            items = items.push(it);
        }
        return context_menu_shell(items);
    }
    // Cross-pane action, picked by the source and the opposite pane's
    // natures: Local -> remote uploads, remote -> Local downloads,
    // remote -> remote relays. Only offered when the other pane is a
    // ready destination (connected remote, or a Local pane).
    if !source_is_remote && other_is_remote {
        // Upload to the (remote) other pane.
        if cross_pane_ready {
            if multi {
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::upload(),
                    t("upload_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
                    Message::SftpUploadSelection,
                    accent,
                ));
            } else {
                let upload_msg = if menu.is_dir {
                    Message::SftpUploadFolder(std::path::PathBuf::from(&menu.path))
                } else {
                    // Route even a single file through the batch queue so the
                    // transfer shows the progress strip + per-file panel
                    // (SftpUpload alone creates no TransferState, hence no
                    // on-screen indicator).
                    Message::SftpUploadBatch(vec![std::path::PathBuf::from(&menu.path)])
                };
                let upload_label = match &other_label {
                    Some(h) => t("upload_to_host").replacen("{host}", h, 1),
                    None => t("upload_to_host").replacen("{host}", t("the_other_host"), 1),
                };
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::upload(),
                    upload_label,
                    upload_msg,
                    accent,
                ));
            }
        }
        // Open the local file in the OS default editor.
        if !multi && !menu.is_dir {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::SftpOpenLocal(std::path::PathBuf::from(&menu.path)),
                secondary,
            ));
        }
    } else if source_is_remote && !other_is_remote {
        // Download to the (Local) other pane.
        if cross_pane_ready {
            if multi {
                items = items.push(menu_item_owned_tinted(
                    iced_fonts::lucide::download(),
                    t("download_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
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
                    t("download_to_local"),
                    download_msg,
                    accent,
                ));
            }
        }
        // Edit-in-place for a single remote file.
        if !multi && !menu.is_dir {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::SftpStartEdit(menu.path.clone()),
                secondary,
            ));
        }
    } else if source_is_remote && other_is_remote {
        // Relay to the other (remote) host. Single-item only for now,
        // multi falls back to per-item relays via the row each.
        if cross_pane_ready {
            let label = match &other_label {
                Some(h) => t("relay_to_remote").replacen("{host}", h, 1),
                None => t("relay_to_remote").replacen("{host}", t("the_other_host"), 1),
            };
            let relay_msg = if menu.is_dir {
                Message::SftpRelayFolder(menu.side, menu.path.clone())
            } else {
                Message::SftpRelay(menu.side, menu.path.clone())
            };
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::arrow_right_left(),
                label,
                relay_msg,
                accent,
            ));
        }
        // Edit-in-place for a single remote file.
        if !multi && !menu.is_dir {
            items = items.push(menu_item_owned_tinted(
                iced_fonts::lucide::pencil(),
                crate::i18n::t("edit").to_string(),
                Message::SftpStartEdit(menu.path.clone()),
                secondary,
            ));
        }
    }
    // Reveal in the OS file manager, local pane only (no notion of an
    // "explorer" for a remote host). Single selection: a folder opens in
    // place, a file opens its folder with the file selected.
    if !source_is_remote && !multi {
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::folder_open(),
            crate::i18n::open_in_file_manager_label(),
            Message::SftpRevealInExplorer(std::path::PathBuf::from(&menu.path), menu.is_dir),
            secondary,
        ));
    }
    if multi {
        items = items.push(menu_item_owned_tinted(
            iced_fonts::lucide::copy(),
            t("duplicate_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1),
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
            t("duplicate"),
            duplicate_msg,
            secondary,
        ));
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::pencil(),
            t("rename"),
            Message::SftpStartRename(menu.side, menu.path.clone()),
            secondary,
        ));
        items = items.push(menu_item_tinted(
            iced_fonts::lucide::cog(),
            t("properties"),
            Message::SftpShowProperties(menu.side, menu.path.clone(), menu.is_dir),
            secondary,
        ));
    }
    let delete_label = if multi {
        t("delete_n_items").replacen("{n}", &selection_count_same_pane.to_string(), 1)
    } else {
        t("delete").to_string()
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

    // Directory-level actions appended below the per-entry block, like
    // FileZilla's row menu (create folder/file + refresh act on the
    // pane's current directory, not the clicked entry).
    items = items.push(menu_separator());
    for it in dir_action_items(menu.side, source_is_remote, dir_ctx, false) {
        items = items.push(it);
    }

    context_menu_shell(items)
}

/// Directory-level actions for the current pane: New folder, New file,
/// Refresh, and (when `full`) Show hidden + Open in File Manager. `full`
/// is set for the background / `⋮` menus where these are the whole menu;
/// the row menu appends only the create + refresh trio. Open in File
/// Manager stays local-only (no OS explorer for a remote host); the
/// create/refresh/hidden actions apply to both panes.
pub(crate) fn dir_action_items<'a>(
    side: SftpPaneSide,
    is_remote: bool,
    ctx: DirActionCtx<'_>,
    full: bool,
) -> Vec<Element<'a, Message>> {
    let refresh_msg = if is_remote {
        Message::SftpNavigateRemote(side, ctx.pane_dir.to_string())
    } else {
        Message::SftpRefreshLocal(side)
    };
    let mut items: Vec<Element<'a, Message>> = vec![
        menu_item(
            iced_fonts::lucide::folder_plus(),
            t("new_folder"),
            Message::SftpStartNewEntry(side, SftpEntryKind::Folder),
        ),
        menu_item(
            iced_fonts::lucide::file_plus(),
            t("new_file"),
            Message::SftpStartNewEntry(side, SftpEntryKind::File),
        ),
    ];
    if full {
        items.push(menu_separator());
    }
    items.push(menu_item(iced_fonts::lucide::rotate_cw(), t("refresh"), refresh_msg));
    if full {
        let hidden_label =
            if ctx.show_hidden { t("hide_hidden_files") } else { t("show_hidden_files") };
        items.push(menu_item(
            iced_fonts::lucide::eye(),
            hidden_label,
            Message::SftpToggleHidden(side),
        ));
        if !is_remote {
            items.push(menu_separator());
            items.push(menu_item(
                iced_fonts::lucide::folder_open(),
                crate::i18n::open_in_file_manager_label(),
                Message::SftpRevealInExplorer(ctx.local_dir.to_path_buf(), true),
            ));
        }
    }
    items
}

/// Shared shell for the cursor-anchored SFTP context menus (row +
/// background): fixed width so the `Length::Fill` separators don't
/// stretch the popover, plus the surface/border/shadow styling.
pub(crate) fn context_menu_shell<'a>(
    items: iced::widget::Column<'a, Message>,
) -> Element<'a, Message> {
    container(items)
        .width(Length::Fixed(ROW_CONTEXT_MENU_WIDTH))
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
/// current target, keeps the layout-level clamp accurate so the menu
/// never spills off the bottom or right edge of the window.
pub(crate) fn row_context_menu_height(
    menu: &crate::state::SftpRowMenu,
    cross_pane_ready: bool,
    source_is_remote: bool,
    other_is_remote: bool,
    selection_count_same_pane: usize,
) -> f32 {
    // Background menu: directory actions only. New folder + New file +
    // Refresh + Show hidden (4), plus Open in File Manager on a local
    // pane (5), plus ~2 thin separators.
    if menu.is_background {
        let items = if source_is_remote { 4.0 } else { 5.0 };
        let separators = if source_is_remote { 1.0 } else { 2.0 };
        return items * 30.0 + separators * 4.0 + 8.0;
    }
    let multi = selection_count_same_pane > 1;
    // Always present: Duplicate + Rename + Properties + Delete (single),
    // or Duplicate + Delete (multi).
    let mut count = if multi { 2.0 } else { 4.0 };
    // Cross-pane action (Upload / Download / Relay) when the other pane
    // is a ready destination.
    if cross_pane_ready {
        count += 1.0;
    }
    // Edit-in-place / open-local for a single remote-source file, or a
    // single local file when uploading.
    if !multi && !menu.is_dir {
        let editable = source_is_remote || other_is_remote;
        if editable {
            count += 1.0;
        }
    }
    // "Open in File Manager" for a single local-pane entry.
    if !source_is_remote && !multi {
        count += 1.0;
    }
    // Appended directory actions (New folder + New file + Refresh) plus
    // their leading separator.
    count += 3.0;
    // Each item ~30px (padding 6+6 + ~12px text + 2px gap) plus 8px
    // container padding, plus one thin separator above the dir actions.
    count * 30.0 + 4.0 + 8.0
}

/// Width is fixed because every item uses the same `menu_item` width.
pub(crate) const ROW_CONTEXT_MENU_WIDTH: f32 = 220.0;

/// Owned-label variant of `menu_item` for cases where the label is
/// computed at runtime (e.g. "Delete N items" with a dynamic count).
/// Owned-label variant that lets the caller pick the icon tint
/// used for destructive (red) and primary (accent / success) actions
/// to match the host-card context menu's color coding.
pub(crate) fn menu_item_owned_tinted<'a>(
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

pub(crate) fn menu_item<'a>(
    icon: iced::widget::Text<'a>,
    label: &'a str,
    msg: Message,
) -> Element<'a, Message> {
    menu_item_tinted(icon, label, msg, OryxisColors::t().text_secondary)
}

/// Like `menu_item` but with an explicit icon tint (red for delete,
/// accent for primary actions, etc.).
pub(crate) fn menu_item_tinted<'a>(
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
pub(crate) fn drives_menu_overlay<'a>(side: SftpPaneSide) -> Element<'a, Message> {
    let drives = list_windows_drives_cached();
    let mut col = column![].spacing(2).padding(4);
    if drives.is_empty() {
        col = col.push(
            container(text(t("no_drives_detected")).size(11).color(OryxisColors::t().text_muted))
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
                .on_press(Message::SftpNavigateLocal(side, drive_path))
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

/// True when the path's first component is a real Windows volume
/// (`C:\`, `D:\`, including the `\\?\C:\` verbatim form). UNC paths
/// like `\\server\share` or `\\wsl$\Ubuntu` return false, those are
/// served by Unix-style filesystems where `/` reads more naturally.
pub(crate) fn is_windows_disk_path(path: &std::path::Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(p))
            if matches!(
                p.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            )
    )
}

/// Cached front for `list_windows_drives`. The raw probe touches the
/// filesystem for every drive letter (A: through Z:) and stats
/// `wsl.exe`, far too heavy to run per frame while the drive popover
/// is open, so the result is reused for a few seconds. Plugging or
/// unplugging a drive shows up on the next refresh window.
pub(crate) fn list_windows_drives_cached() -> Vec<String> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    const TTL: Duration = Duration::from_secs(5);
    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap();
    if let Some((probed_at, drives)) = guard.as_ref()
        && probed_at.elapsed() < TTL
    {
        return drives.clone();
    }
    let drives = list_windows_drives();
    *guard = Some((Instant::now(), drives.clone()));
    drives
}

/// Enumerate available drive letters on Windows. Empty on non-Windows
/// hosts (the dropdown isn't rendered there). When running under WSL,
/// surface `\\wsl.localhost` as a synthetic root so the user can hop
/// between WSL distros without dropping to a terminal.
pub(crate) fn list_windows_drives() -> Vec<String> {
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
        // `wsl.exe` in System32, present iff the user has WSL
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
