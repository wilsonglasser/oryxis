//! UI helper widgets: layout. Split out of widgets/mod.rs.

use super::*;
/// Build a `Row` from elements written in left-to-right *reading order*,
/// reversing them when the active layout direction is RTL. Use anywhere the
/// physical placement of children should mirror with the layout setting
/// e.g. sidebar vs. content, leading/trailing icon pairs.
///
/// The `iced::widget::row!` macro takes positional children and can't be
/// reversed after construction, so callers that need direction-awareness
/// should switch to this helper instead.
pub fn dir_row<'a, M: 'a>(items: Vec<Element<'a, M>>) -> Row<'a, M> {
    if crate::i18n::is_rtl_layout() {
        Row::with_children(items.into_iter().rev().collect::<Vec<_>>())
    } else {
        Row::with_children(items)
    }
}

/// Horizontal alignment for content that should hug the *leading* edge
/// `Left` under LTR, `Right` under RTL. Use on `Column::align_x`,
/// `Container::align_x`, or `text(...).align_x(...)` inside `Length::Fill`
/// regions where children would otherwise glue to the physical left edge.
pub fn dir_align_x() -> iced::alignment::Horizontal {
    if crate::i18n::is_rtl_layout() {
        iced::alignment::Horizontal::Right
    } else {
        iced::alignment::Horizontal::Left
    }
}

/// Pick a column count for a card grid given the available content width.
/// Floor-divides slack by `min_card_width + h_gap`, clamped to `>= 1`.
/// Callers compute `available_width` from `window_size` minus the visible
/// chrome (left sidebar, optional right panel, padding).
pub fn card_grid_columns(available_width: f32, min_card_width: f32, h_gap: f32) -> usize {
    if available_width <= 0.0 || min_card_width <= 0.0 {
        return 1;
    }
    let n = ((available_width + h_gap) / (min_card_width + h_gap)).floor() as usize;
    n.max(1)
}

/// Distribute pre-built cards into rows of `cols` cards each. Cards must be
/// built with `Length::Fill` width so the row evenly divides the slack;
/// partial last rows are padded with invisible fillers so the trailing
/// card keeps the same per-card width as the full rows above.
///
/// Honours the active layout direction via `dir_row`, under RTL each
/// row's children are reversed, but the row order (top-to-bottom) stays
/// the same.
pub fn distribute_card_grid<'a, M: 'a>(
    cards: Vec<Element<'a, M>>,
    cols: usize,
    h_gap: f32,
    v_gap: f32,
) -> Element<'a, M> {
    use iced::widget::column;

    if cards.is_empty() {
        return Space::new().height(0).into();
    }
    let cols = cols.max(1);
    let mut grid_rows: Vec<Element<'a, M>> = Vec::new();
    let mut row_buf: Vec<Element<'a, M>> = Vec::with_capacity(cols);
    let total = cards.len();

    for (i, card) in cards.into_iter().enumerate() {
        row_buf.push(card);
        if row_buf.len() == cols {
            grid_rows.push(dir_row(std::mem::take(&mut row_buf)).spacing(h_gap).into());
            if i + 1 < total {
                grid_rows.push(Space::new().height(v_gap).into());
            }
        }
    }
    if !row_buf.is_empty() {
        while row_buf.len() < cols {
            row_buf.push(Space::new().width(Length::Fill).into());
        }
        grid_rows.push(dir_row(row_buf).spacing(h_gap).into());
    }
    column(grid_rows).width(Length::Fill).into()
}
