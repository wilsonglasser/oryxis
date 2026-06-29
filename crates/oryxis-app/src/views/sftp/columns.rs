//! SFTP view helpers: columns. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::row;
/// Width of the leading icon + gap column, used both as the header's
/// alignment pad and to size the fixed-width row when columns overflow.
pub(crate) const ICON_COL_W: f32 = 21.0;

/// Left + right padding applied to every header / row container.
pub(crate) const ROW_PAD_X: f32 = 24.0;

/// Width of the resize handle (and its visible divider) on the right edge of
/// each column header.
pub(crate) const COL_RESIZE_HANDLE_W: f32 = 7.0;

/// Right inset on each cell's text so an ellipsised value (`abc…`) breathes
/// instead of touching the column divider. Kept below the 18px auto-fit
/// breathing room so an auto-fit column still fully fits its content.
pub(crate) const CELL_PAD_RIGHT: f32 = 8.0;
/// Widget id of the inline rename input, focused when a rename starts.
pub(crate) const RENAME_INPUT_ID: &str = "sftp-rename-input";

/// Sum of the visible columns (Name included) at their current widths.
pub(crate) fn cols_total_width(
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
) -> f32 {
    visible.iter().map(|c| widths.get(*c)).sum()
}

/// Rendered width of `s` at `size` px in the UI font, measured through the
/// same global font system the renderer uses, so column auto-fit (issue #45)
/// matches the text actually drawn on screen. Allocates a throwaway
/// paragraph; only called on a divider double-click, never per frame.
pub(crate) fn measure_text_width(s: &str, size: f32) -> f32 {
    use iced::advanced::text::Paragraph as _;
    if s.is_empty() {
        return 0.0;
    }
    let text = iced::advanced::text::Text {
        content: s,
        bounds: iced::Size::INFINITE,
        size: iced::Pixels(size),
        line_height: iced::advanced::text::LineHeight::default(),
        // The app's configured default UI font (Noto Sans); see main.rs.
        font: crate::theme::SYSTEM_UI,
        align_x: iced::advanced::text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        // Matches the `text()` widget default so the measurement lines up
        // with what the rows render.
        shaping: iced::advanced::text::Shaping::Auto,
        wrapping: iced::advanced::text::Wrapping::None,
        ellipsis: iced::advanced::text::Ellipsis::None,
        hint_factor: None,
    };
    iced::advanced::graphics::text::Paragraph::with_text(text)
        .min_bounds()
        .width
}

/// Auto-fit width for `col`: the widest rendered value across every row in the
/// pane (visible or not), with the header label as a floor, plus cell padding,
/// the resize-handle grab zone, and - for Name - the leading icon area.
/// Returned unclamped; the caller's `SftpColWidths::set` applies the column's
/// min/max.
pub(crate) fn autofit_column_width(
    is_remote: bool,
    remote: &[crate::state::SftpEntry],
    local: &[crate::state::LocalEntry],
    col: crate::state::SftpColumn,
) -> f32 {
    use crate::state::SftpColumn;
    // Name renders its filename at size 12; every data cell at size 11.
    let text_size = if col == SftpColumn::Name { 12.0 } else { 11.0 };
    // Floor: the header label (size 11) must never clip either.
    let mut max_w = measure_text_width(data_col_label(col), 11.0);
    if is_remote {
        for e in remote {
            let s = match col {
                SftpColumn::Name => e.name.clone(),
                SftpColumn::Modified => format_modified_remote(e.mtime),
                SftpColumn::Size => format_size(e.size),
                SftpColumn::Kind => format_kind(&e.name, e.is_dir, e.is_symlink),
                SftpColumn::Permissions => format_perms(e.permissions, e.is_dir, e.is_symlink),
                SftpColumn::Owner => format_owner(e.uid, e.gid),
            };
            max_w = max_w.max(measure_text_width(&s, text_size));
        }
    } else {
        for e in local {
            let s = match col {
                SftpColumn::Name => e.name.clone(),
                SftpColumn::Modified => format_modified_local(e.modified),
                SftpColumn::Size => format_size(e.size),
                SftpColumn::Kind => format_kind(&e.name, e.is_dir, false),
                SftpColumn::Permissions => format_perms(e.mode, e.is_dir, false),
                SftpColumn::Owner => format_owner(e.uid, e.gid),
            };
            max_w = max_w.max(measure_text_width(&s, text_size));
        }
    }
    // Cell text breathing room (both sides) + the divider grab zone; Name also
    // holds the leading file icon + gap.
    let mut extra = 18.0 + COL_RESIZE_HANDLE_W;
    if col == SftpColumn::Name {
        extra += ICON_COL_W + 8.0;
    }
    max_w + extra
}

/// Resolved horizontal layout for the file list of one pane. Every column
/// (Name included) has an explicit width, so resizing one grows the total
/// instead of squeezing a neighbour. When the total exceeds the pane the row
/// gets a fixed width and a horizontal scrollbar; otherwise it fills the pane
/// (columns left-packed, the trailing slack left empty).
#[derive(Clone, Copy)]
pub(crate) struct ColLayout {
    /// Whole-row width: `Fill` when everything fits, `Fixed(total)` when it
    /// overflows (so the horizontal scrollable has something to pan).
    pub(crate) row_len: Length,
    /// True when the row width is fixed (columns overflow the pane).
    pub(crate) overflow: bool,
}

impl ColLayout {
    /// Build the layout for a pane `avail` px wide given the visible columns'
    /// total width (Name included).
    pub(crate) fn resolve(cols_total: f32, avail: f32) -> Self {
        let total = ROW_PAD_X + cols_total;
        ColLayout {
            row_len: if total <= avail {
                Length::Fill
            } else {
                Length::Fixed(total)
            },
            overflow: total > avail,
        }
    }
}

/// Label for a column header.
pub(crate) fn data_col_label(col: crate::state::SftpColumn) -> &'static str {
    use crate::state::SftpColumn;
    match col {
        SftpColumn::Name => t("col_name"),
        SftpColumn::Modified => t("col_modified"),
        SftpColumn::Size => t("col_size"),
        SftpColumn::Kind => t("col_type"),
        SftpColumn::Permissions => t("col_permissions"),
        SftpColumn::Owner => t("col_owner"),
    }
}

/// Floating ghost shown while a column header is dragged for reorder:
/// the column label in an accent-bordered, shadowed chip (mirrors the
/// host-tab drag ghost).
pub(crate) fn col_drag_ghost<'a>(label: &str) -> Element<'a, Message> {
    let accent = OryxisColors::t().accent;
    container(
        text(label.to_string())
            .size(11)
            .color(accent)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.96, ..OryxisColors::t().bg_hover })),
        border: Border { radius: Radius::from(6.0), color: accent, width: 1.5 },
        shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        ..Default::default()
    })
    .into()
}

/// A resize handle: a centered 1px divider line inside a wider grab zone,
/// living on the right edge of a header. Dragging resizes `target`; double-
/// clicking auto-fits it to the widest value in the column (issue #45).
pub(crate) fn col_resize_handle<'a>(
    side: SftpPaneSide,
    target: crate::state::SftpColumn,
) -> Element<'a, Message> {
    MouseArea::new(
        container(
            container(Space::new().width(Length::Fixed(1.0)).height(Length::Fill))
                .width(Length::Fixed(1.0))
                .height(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
        )
        .width(Length::Fixed(COL_RESIZE_HANDLE_W))
        .height(Length::Fill)
        .center_x(Length::Fixed(COL_RESIZE_HANDLE_W)),
    )
    .on_press(Message::SftpColResizeStart(side, target))
    .on_double_click(Message::SftpColAutoFit(side, target))
    .interaction(iced::mouse::Interaction::ResizingHorizontally)
    .into()
}

/// A small bordered tooltip chip carrying `tip`, matching the snippet-action
/// tooltips elsewhere in the app.
pub(crate) fn tooltip_chip<'a>(tip: &'a str) -> Element<'a, Message> {
    container(text(tip).size(11).color(OryxisColors::t().text_primary))
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// One column header: an ellipsised, draggable label with a trailing (right-
/// aligned, RTL-aware) sort arrow, plus a right-edge resize handle. The arrow
/// carries its own pointer cursor + tooltip; pressing anywhere on the header
/// arms a reorder drag that falls back to a sort toggle on a click without
/// movement (issue #45).
#[allow(clippy::too_many_arguments)]
pub(crate) fn header_cell<'a>(
    side: SftpPaneSide,
    col: crate::state::SftpColumn,
    sort: crate::state::SftpSort,
    w: f32,
    is_target: bool,
    dragging: bool,
) -> Element<'a, Message> {
    use crate::state::SftpColumn;
    let sortable = col.sort_column();
    let active_sort = sortable.is_some_and(|sc| sort.column == sc);
    let label_color = if active_sort {
        OryxisColors::t().text_primary
    } else {
        OryxisColors::t().text_muted
    };
    let label_w = (w - COL_RESIZE_HANDLE_W).max(8.0);

    let label = text(data_col_label(col).to_string())
        .size(11)
        .color(label_color)
        .width(Length::Fill)
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::widget::text::Ellipsis::End);

    // Right-aligned sort arrow, shown only on the active sort column so the
    // header stays uncluttered. It gets a pointer cursor + tooltip; with no
    // press handler of its own, the drag/sort press falls through to the
    // header MouseArea below.
    let mut label_children: Vec<Element<'a, Message>> = vec![label.into()];
    if active_sort {
        let glyph = if sort.ascending {
            iced_fonts::lucide::chevron_up()
        } else {
            iced_fonts::lucide::chevron_down()
        };
        let tip = if sort.ascending {
            t("sftp_sort_asc")
        } else {
            t("sftp_sort_desc")
        };
        // Center the glyph in the full header height so it lines up with the
        // label baseline (the chevron's mark otherwise sits high in its box).
        let arrow = container(
            MouseArea::new(iced::widget::tooltip(
                glyph.size(12).color(OryxisColors::t().accent),
                tooltip_chip(tip),
                iced::widget::tooltip::Position::Top,
            ))
            .interaction(iced::mouse::Interaction::Pointer),
        )
        .height(Length::Fill)
        // A hair of bottom padding nudges the glyph ~1px up off the centre so
        // it sits exactly on the label's optical line.
        .padding(Padding { bottom: 2.0, ..Padding::ZERO })
        .center_y(Length::Fill);
        label_children.push(Space::new().width(4).into());
        label_children.push(arrow.into());
    }
    let label_row = crate::widgets::dir_row(label_children)
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

    // Name keeps a leading icon-width pad so the "Name" header aligns with the
    // filenames below, even after Name is dragged out of first position.
    let inner_content: Element<'a, Message> = if col == SftpColumn::Name {
        row![Space::new().width(Length::Fixed(ICON_COL_W)), label_row]
            .height(Length::Fill)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        label_row.into()
    };

    let label_area = MouseArea::new(
        container(inner_content)
            .width(Length::Fixed(label_w))
            .height(Length::Fill)
            .padding(Padding { top: 4.0, right: 6.0, bottom: 4.0, left: 6.0 })
            .align_y(iced::alignment::Vertical::Center),
    )
    .on_press(Message::SftpColDragStart(side, col))
    .on_enter(Message::SftpColHovered(side, col))
    .on_exit(Message::SftpColUnhovered)
    // Grab hint normally; grabbing while a reorder drag is active so the
    // user gets cursor feedback that the drag took.
    .interaction(if dragging {
        iced::mouse::Interaction::Grabbing
    } else {
        iced::mouse::Interaction::Grab
    });

    let cell = row![label_area, col_resize_handle(side, col)]
        .width(Length::Fixed(w))
        .align_y(iced::Alignment::Center);

    // Hard rule: the Name column never shows a drop effect, even when it's the
    // hovered target. Other columns get an accent insertion bar at their
    // leading edge (overlaid, so no layout shift) marking where the dragged
    // column lands.
    if is_target && col != SftpColumn::Name {
        let bar = container(Space::new().width(Length::Fixed(2.0)).height(Length::Fill))
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().accent)),
                ..Default::default()
            });
        iced::widget::stack![cell, bar].into()
    } else {
        cell.into()
    }
}

/// Column header strip. Every column (Name included) has an explicit width, a
/// right-edge resize handle (a visible divider, double-click to auto-fit), and
/// is draggable to reorder. Sortable headers flip the sort on a plain click
/// (release without a drag, handled in the mouse-up dispatcher) and show a
/// right-aligned sort arrow.
pub(crate) fn column_headers<'a>(
    side: SftpPaneSide,
    sort: crate::state::SftpSort,
    visible: &[crate::state::SftpColumn],
    widths: crate::state::SftpColWidths,
    layout: ColLayout,
    drag_col: Option<crate::state::SftpColumn>,
    hovered: Option<crate::state::SftpColumn>,
) -> Element<'a, Message> {
    let dragging = drag_col.is_some();
    let mut children: Vec<Element<'a, Message>> = Vec::with_capacity(visible.len());
    for &col in visible {
        children.push(header_cell(
            side,
            col,
            sort,
            widths.get(col),
            dragging && hovered == Some(col) && drag_col != Some(col),
            dragging,
        ));
    }

    container(
        iced::widget::Row::with_children(children)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    // Fixed height so the Fill-height resize handles don't balloon the strip
    // (and thus the whole header band) to fill the pane vertically.
    .height(Length::Fixed(28.0))
    .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 })
    .width(layout.row_len)
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
