//! SFTP view helpers: rows. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::{column, row};
pub(crate) fn parent_row<'a>(
    side: SftpPaneSide,
    selected: bool,
    suppress_hover: bool,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
) -> Element<'a, Message> {
    let msg = Message::SftpUp(side);
    let icon = iced_fonts::lucide::folder()
        .size(13)
        .color(OryxisColors::t().text_muted)
        .into();
    let label = text("..")
        .size(12)
        .color(OryxisColors::t().text_muted)
        .width(Length::Fill)
        .into();
    // Blank trailing cells keep the ".." row aligned with the columns.
    let children = row_cells(visible, widths, icon, label, None, |_| String::new());
    let inner = iced::widget::Row::with_children(children).align_y(iced::Alignment::Center);
    button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(layout.row_len)
        .height(Length::Fixed(ROW_HEIGHT))
        .on_press(msg)
        .style(move |_, status| {
            // Keyboard cursor highlight mirrors a selected file row.
            let bg = if selected {
                Color { a: 0.20, ..OryxisColors::t().accent }
            } else {
                match status {
                    BtnStatus::Hovered if !suppress_hover => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                ..Default::default()
            }
        })
        .into()
}

/// One data cell: a single, fixed-width line of text that truncates with an
/// ellipsis at the column edge instead of wrapping or bleeding into the next
/// column (issue #45).
pub(crate) fn data_cell<'a>(value: String, width: f32, color: Color) -> Element<'a, Message> {
    // The cell box stays `Fixed(width)` so columns line up with the headers;
    // a right inset on the inner text keeps an ellipsis off the divider.
    container(
        text(value)
            .size(11)
            .color(color)
            .width(Length::Fill)
            .wrapping(iced::widget::text::Wrapping::None)
            .ellipsis(iced::widget::text::Ellipsis::End),
    )
    .width(Length::Fixed(width))
    .padding(Padding { top: 0.0, right: CELL_PAD_RIGHT, bottom: 0.0, left: 0.0 })
    .into()
}

/// The Name cell: the file icon, then the filename (or, while renaming, the
/// rename input). The label fills the remaining cell width and ellipsises on
/// overflow. When the (non-editing) name is wide enough to be clipped, it's
/// wrapped in a tooltip that reveals the full name on hover, FileZilla-style.
pub(crate) fn name_cell<'a>(
    icon: Element<'a, Message>,
    label: Element<'a, Message>,
    full_name: Option<String>,
    width: f32,
) -> Element<'a, Message> {
    let inner = row![icon, Space::new().width(8), label]
        .width(Length::Fill)
        .align_y(iced::Alignment::Center);
    let cell = container(inner)
        .width(Length::Fixed(width))
        // Right inset so a truncated filename's ellipsis breathes off the
        // divider, matching the data cells.
        .padding(Padding { top: 0.0, right: CELL_PAD_RIGHT, bottom: 0.0, left: 0.0 })
        // Clip so an un-ellipsised child (e.g. the rename input) can't bleed
        // past the cell into the next column.
        .clip(true);
    match full_name {
        Some(name) => iced::widget::tooltip(
            cell,
            container(
                text(name)
                    .size(11)
                    .color(OryxisColors::t().text_primary)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
            iced::widget::tooltip::Position::Bottom,
        )
        .into(),
        None => cell.into(),
    }
}

/// Build the Name cell's inner widget: the rename input while editing,
/// otherwise an ellipsised filename. Also returns the full name to use as a
/// hover tooltip when (and only when) the label is wide enough to be clipped.
pub(crate) fn name_label_widget<'a>(
    name: &str,
    rename_input: Option<&str>,
    widths: crate::state::SftpColWidths,
) -> (Element<'a, Message>, Option<String>) {
    use crate::state::SftpColumn;
    if let Some(input) = rename_input {
        let w = text_input(name, input)
            .id(iced::widget::Id::new(RENAME_INPUT_ID))
            .on_input(Message::SftpRenameInput)
            .on_submit(Message::SftpRenameCommit)
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .size(11)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x())
            .width(Length::Fill)
            .into();
        return (w, None);
    }
    let label = text(name.to_string())
        .size(12)
        .color(OryxisColors::t().text_primary)
        .width(Length::Fill)
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::widget::text::Ellipsis::End)
        .into();
    // Space available to the filename inside the Name cell: cell width minus
    // the leading icon area, the inter-column gap, and the right inset (the
    // same assumptions auto-fit's `extra` uses, so an auto-fit column
    // resolves to "fits" here too).
    let avail = widths.get(SftpColumn::Name) - ICON_COL_W - 8.0 - CELL_PAD_RIGHT;
    // Two-tier: the cheap proportional estimate gates the expensive accurate
    // measure, so only borderline rows pay for shaping (the list isn't
    // virtualized, so every row runs this). The estimate is biased high, so
    // it never misses a truncation; the accurate measure then rejects the
    // false positives (e.g. a column just auto-fit to the value's real
    // width, which previously kept showing a tooltip).
    let tooltip = (approx_text_width(name, 12.0) > avail
        && measure_text_width(name, 12.0) > avail)
        .then(|| name.to_string());
    (label, tooltip)
}

/// Build the ordered cells of one file row. The Name column (wherever it sits
/// in the order) renders the leading icon + `name_label`; every other column
/// renders an ellipsised value from `value`. `tooltip_name` is `Some` when the
/// filename overflows and should get a hover tooltip.
pub(crate) fn row_cells<'a>(
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    icon: Element<'a, Message>,
    name_label: Element<'a, Message>,
    tooltip_name: Option<String>,
    value: impl Fn(crate::state::SftpColumn) -> String,
) -> Vec<Element<'a, Message>> {
    use crate::state::SftpColumn;
    let mut icon = Some(icon);
    let mut name_label = Some(name_label);
    let mut tooltip_name = tooltip_name;
    let mut cells: Vec<Element<'a, Message>> = Vec::with_capacity(visible.len());
    for &c in visible {
        if c == SftpColumn::Name {
            // `ordered_visible()` always yields Name exactly once, so these
            // takes are safe; fall back to an empty cell if that ever changes.
            let icon = icon.take().unwrap_or_else(|| Space::new().into());
            let label = name_label.take().unwrap_or_else(|| Space::new().into());
            cells.push(name_cell(icon, label, tooltip_name.take(), widths.get(c)));
        } else {
            cells.push(data_cell(value(c), widths.get(c), OryxisColors::t().text_muted));
        }
    }
    cells
}

/// Visually distinct band that wraps the toolbar / breadcrumb / column
/// headers, gives the file list a clean separation from the chrome,
/// matching how Finder / Explorer / Termius split the two regions.
pub(crate) fn pane_header_band<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
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

/// Wrap a built file-list column in the right scrollable. With columns
/// that fit, this is the plain vertical scroll used before. When the
/// columns overflow, the header strip is prepended (so it pans with the
/// rows) and the scrollable gains a horizontal scrollbar.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sftp_list_scrollable<'a>(
    col: iced::widget::Column<'a, Message>,
    scroll_id: &str,
    side: SftpPaneSide,
    sort: crate::state::SftpSort,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
    drag_col: Option<crate::state::SftpColumn>,
    hovered: Option<crate::state::SftpColumn>,
) -> Element<'a, Message> {
    // Report scroll position + viewport height so keyboard navigation can
    // scroll only when the cursor reaches a viewport edge (not on every step).
    let on_scroll = move |vp: scrollable::Viewport| {
        Message::SftpListScrolled(side, vp.absolute_offset().y, vp.bounds().height)
    };
    if layout.overflow {
        // Sticky header + horizontal scroll (FileZilla-style): the rows get
        // their own vertical scrollable, and that plus the header sit inside
        // an outer horizontal scrollable. Panning sideways moves header and
        // rows together (kept aligned), while the header never scrolls away
        // vertically because the vertical scroll lives on the inner list only.
        let inner_list = scrollable(col)
            .id(iced::widget::Id::from(scroll_id.to_string()))
            .width(layout.row_len)
            .height(Length::Fill)
            .on_scroll(on_scroll);
        let header = column_headers(side, sort, visible, widths, layout, drag_col, hovered);
        let stacked = column![header, inner_list]
            .width(layout.row_len)
            .height(Length::Fill);
        scrollable(stacked)
            .width(Length::Fill)
            .height(Length::Fill)
            .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new()))
            .into()
    } else {
        scrollable(col)
            .id(iced::widget::Id::from(scroll_id.to_string()))
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(on_scroll)
            .into()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn file_row_local<'a>(
    side: SftpPaneSide,
    name: String,
    is_dir: bool,
    size_str: String,
    modified: Option<std::time::SystemTime>,
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
    path: std::path::PathBuf,
    rename_input: Option<&str>,
    is_selected: bool,
    is_drop_target: bool,
    suppress_hover: bool,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    let icon = file_icon(&name, is_dir, false);
    let kind = format_kind(&name, is_dir, false);
    let modified_s = format_modified_local(modified);
    let perms_s = format_perms(mode, is_dir, false);
    let owner_s = format_owner(uid, gid);

    // Inline rename mode swaps the row's label for a text input; the
    // icon + columns stay put so the row geometry doesn't jump.
    let (label_widget, tooltip_name) = name_label_widget(&name, rename_input, widths);

    let children = row_cells(visible, widths, icon.into(), label_widget, tooltip_name, |c| {
        match c {
            SftpColumn::Modified => modified_s.clone(),
            SftpColumn::Size => size_str.clone(),
            SftpColumn::Kind => kind.clone(),
            SftpColumn::Permissions => perms_s.clone(),
            SftpColumn::Owner => owner_s.clone(),
            SftpColumn::Name => String::new(),
        }
    });
    let inner = iced::widget::Row::with_children(children)
        // Fill the button's fixed height so the content centers vertically.
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

    // Click action priority: while renaming, swallow clicks; folders
    // navigate; files mark themselves selected so the user has visible
    // confirmation that the row is interactive (was previously a disabled
    // button, no hover, no pointer cursor, looked dead).
    let path_str = path.to_string_lossy().into_owned();
    // SftpSelectRow handles plain folder click (navigate), file click
    // (single-select), and modifier clicks (toggle / range). Routing it
    // all through one message means modifier state can be consulted
    // server-side instead of being stored at button-build time.
    let on_click = if rename_input.is_some() {
        None
    } else {
        Some(Message::SftpSelectRow(side, path_str.clone(), is_dir))
    };
    let path_for_enter = path_str.clone();
    let mut btn = button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(layout.row_len)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(move |_, status| {
            // Drop highlight beats selection while a drag is in flight,
            // matches the right-pane logic.
            let bg = if is_drop_target || is_selected {
                Color { a: 0.20, ..OryxisColors::t().accent }
            } else {
                match status {
                    // Keyboard nav suppresses the mouse-hover tint until the
                    // cursor moves again, so it doesn't compete with the
                    // keyboard cursor on a stationary mouse.
                    BtnStatus::Hovered if !suppress_hover => OryxisColors::t().bg_hover,
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
    // internal drag-drop press handler, needed even on file rows since
    // a file is a valid drag *source* (just not a drop *target*).
    MouseArea::new(btn)
        .on_right_press(Message::SftpRowRightClick(side, path_str, is_dir))
        .on_enter(Message::SftpRowEnter(side, path_for_enter, is_dir))
        .on_exit(Message::SftpRowExit)
        .into()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn file_row_remote<'a>(
    side: SftpPaneSide,
    name: String,
    is_dir: bool,
    is_symlink: bool,
    size_str: String,
    mtime: Option<u32>,
    permissions: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
    full_path: String,
    rename_input: Option<&str>,
    is_drop_target: bool,
    is_selected: bool,
    suppress_hover: bool,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    let icon = file_icon(&name, is_dir, is_symlink);
    let kind = format_kind(&name, is_dir, is_symlink);
    let modified_s = format_modified_remote(mtime);
    let perms_s = format_perms(permissions, is_dir, is_symlink);
    let owner_s = format_owner(uid, gid);

    let (label_widget, tooltip_name) = name_label_widget(&name, rename_input, widths);

    // Single message routes folder navigation, file single-select, and
    // ctrl/shift modifier selection, see the local row counterpart.
    // Symlinks behave like folders for click (treat as nav target) since
    // we can't tell from the listing whether they point at a file vs dir.
    let nav_target = if rename_input.is_some() {
        None
    } else {
        Some(Message::SftpSelectRow(
            side,
            full_path.clone(),
            is_dir || is_symlink,
        ))
    };
    let children = row_cells(visible, widths, icon.into(), label_widget, tooltip_name, |c| {
        match c {
        SftpColumn::Modified => modified_s.clone(),
        SftpColumn::Size => size_str.clone(),
        SftpColumn::Kind => kind.clone(),
        SftpColumn::Permissions => perms_s.clone(),
        SftpColumn::Owner => owner_s.clone(),
        SftpColumn::Name => String::new(),
        }
    });
    let inner = iced::widget::Row::with_children(children)
        // Fill the button's fixed height so `align_y(Center)` actually centers
        // the content vertically inside the row (otherwise a shrink-height row
        // sits at the top of the slot and reads as misaligned).
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);
    let mut btn = button(inner)
        .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
        .width(layout.row_len)
        .height(Length::Fixed(ROW_HEIGHT))
        .style(move |_, status| {
            // Drop highlight beats selection (transient, communicates
            // imminent action), selection beats default hover.
            let bg = if is_drop_target || is_selected {
                Color { a: 0.20, ..OryxisColors::t().accent }
            } else {
                match status {
                    // See file_row_local: keyboard nav mutes hover until the
                    // mouse moves.
                    BtnStatus::Hovered if !suppress_hover => OryxisColors::t().bg_hover,
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
        .on_right_press(Message::SftpRowRightClick(side, full_path.clone(), is_dir))
        .on_enter(Message::SftpRowEnter(side, full_path, is_dir))
        .on_exit(Message::SftpRowExit)
        .into()
}

pub(crate) fn file_icon<'a>(name: &str, is_dir: bool, is_symlink: bool) -> iced::widget::Text<'a> {
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
