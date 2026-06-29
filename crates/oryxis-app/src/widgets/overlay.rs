//! UI helper widgets: overlay. Split out of widgets/mod.rs.

use super::*;
/// Shared cell type for `bounds_reporter`. Single-threaded
/// (`Rc<Cell<_>>`) is fine for iced's event loop in 0.13; bump to
/// `Arc<AtomicRefCell<_>>` if iced ever multithreads the layout pass.
pub(crate) type BoundsCell = std::rc::Rc<std::cell::Cell<iced::Rectangle>>;

/// Build a fresh, zeroed `BoundsCell` ready to be cloned into a
/// `bounds_reporter` and held in app state for later reads.
pub(crate) fn new_bounds_cell() -> BoundsCell {
    std::rc::Rc::new(std::cell::Cell::new(iced::Rectangle::new(
        iced::Point::ORIGIN,
        iced::Size::ZERO,
    )))
}

/// Wraps `content` and writes the laid-out screen-space bounds to
/// `cell` on every `draw` pass. Lets other code (typically context-
/// menu anchor logic) read the widget's on-screen rect synchronously
/// instead of going through the async `Operation` round-trip. Cell
/// value reflects the LAST rendered frame, which is what every
/// popover/anchor flow wants anyway. Everything except `draw`
/// delegates straight to the inner widget, so behaviour is otherwise
/// identical to the unwrapped child.
pub(crate) fn bounds_reporter<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    cell: BoundsCell,
) -> Element<'a, Message> {
    use iced::advanced::widget::{tree, Operation, Tree, Widget};
    use iced::advanced::{layout, mouse, overlay, renderer, Layout, Shell};
    use iced::{Event, Length as L, Rectangle, Size, Vector};

    struct BoundsReporter<'a, Message> {
        content: Element<'a, Message>,
        cell: BoundsCell,
    }

    impl<Message> Widget<Message, Theme, iced::Renderer> for BoundsReporter<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }
        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }
        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }
        fn diff(&self, tree: &mut Tree) {
            self.content.as_widget().diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.as_widget().size()
        }
        fn size_hint(&self) -> Size<L> {
            self.content.as_widget().size_hint()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content
                .as_widget_mut()
                .layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            // Draw runs after final positioning, so `layout.bounds()`
            // here is the screen-space rect (offset by parent
            // translations). Cache it so anchor lookups in `update`
            // hit the correct on-screen coordinates.
            self.cell.set(layout.bounds());
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, shell, viewport,
            );
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content.as_widget_mut().overlay(
                tree,
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }

    Element::new(BoundsReporter {
        content: content.into(),
        cell,
    })
}

/// Wraps `content` (a terminal pane canvas) and, while `enabled` is true,
/// asks the runtime to turn the OS IME on for this surface. The terminal is
/// an `iced` canvas, not a `text_input`, so nothing in its widget tree ever
/// requests an input method, and winit defaults `set_ime_allowed(false)`,
/// which locks the IME to direct (English) mode and blocks CJK composition.
/// This decorator closes that gap: every other behaviour delegates straight
/// to the inner widget, so it is transparent apart from the IME request.
///
/// The request is only honoured by the shell during a `RedrawRequested`
/// frame, so we issue it there. Only the focused pane (`enabled`) drives the
/// IME, so split panes don't fight over the cursor area. The committed text
/// itself arrives as `Event::InputMethod(Commit(..))` and is routed to the
/// PTY in `subscription.rs` / `dispatch_terminal.rs`, not here.
///
/// The candidate box is anchored near the bottom-left (the usual prompt row)
/// rather than the live caret. Caret-following is a future polish; this keeps
/// the popup on-screen and functional.
pub(crate) fn ime_host<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    enabled: bool,
    terminal: std::sync::Arc<std::sync::Mutex<oryxis_terminal::TerminalState>>,
    font_size: f32,
    font_name: String,
) -> Element<'a, Message> {
    use iced::advanced::widget::{tree, Operation, Tree, Widget};
    use iced::advanced::{input_method, layout, mouse, overlay, renderer, Layout, Shell};
    use iced::{Event, Length as L, Point, Rectangle, Size, Vector};

    struct ImeHost<'a, Message> {
        content: Element<'a, Message>,
        enabled: bool,
        terminal: std::sync::Arc<std::sync::Mutex<oryxis_terminal::TerminalState>>,
        font_size: f32,
        font_name: String,
    }

    impl<Message> Widget<Message, Theme, iced::Renderer> for ImeHost<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }
        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }
        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }
        fn diff(&self, tree: &mut Tree) {
            self.content.as_widget().diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.as_widget().size()
        }
        fn size_hint(&self) -> Size<L> {
            self.content.as_widget().size_hint()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content.as_widget_mut().layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content
                .as_widget_mut()
                .update(tree, event, layout, cursor, renderer, shell, viewport);

            // The shell only honours an input-method request issued during a
            // redraw frame; only the focused pane requests it.
            if self.enabled
                && matches!(
                    event,
                    Event::Window(iced::window::Event::RedrawRequested(_))
                )
            {
                let b = layout.bounds();
                // Anchor the candidate window at the terminal caret. try_lock
                // so a frame that races the render thread just falls back to
                // the bottom-left instead of blocking the UI.
                let cursor_area = match self.terminal.try_lock() {
                    Ok(state) => oryxis_terminal::ime_caret_rect(
                        b,
                        self.font_size,
                        Some(self.font_name.as_str()),
                        state.cursor_cell(),
                    ),
                    Err(_) => {
                        let h = 18.0_f32.min(b.height);
                        Rectangle::new(
                            Point::new(b.x + 8.0, b.y + (b.height - h).max(0.0)),
                            Size::new(2.0, h),
                        )
                    }
                };
                let ime: input_method::InputMethod = input_method::InputMethod::Enabled {
                    cursor: cursor_area,
                    purpose: input_method::Purpose::Normal,
                    preedit: None,
                };
                shell.request_input_method(&ime);
            }
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content
                .as_widget_mut()
                .overlay(tree, layout, renderer, viewport, translation)
        }
    }

    Element::new(ImeHost {
        content: content.into(),
        enabled,
        terminal,
        font_size,
        font_name,
    })
}

/// The single, canonical full-window modal overlay: `base` view, a scrim
/// that absorbs both click and hover, and a centered `card` that traps its
/// own clicks. Every blocking modal should route through this so the scrim
/// can never reintroduce mouse bleed-through to the live UI behind it.
///
/// - `on_scrim_click`: `Some(msg)` makes an outside click dismiss the modal;
///   `None` is for auth-style modals (host key, 2FA, update) that must not
///   dismiss on a stray backdrop click. Either way the scrim fully absorbs
///   the click, so nothing reaches `base`.
/// - `top_reserve`: a transparent band (px) at the top of the *scrim only*,
///   so the window title bar (drag / minimize / maximize / close) stays
///   hittable while the modal is open. The card still centers over the full
///   height. Pass `40.0` for app-level modals, `0.0` for in-view ones.
///
/// `interaction(Idle)` on the scrim is load-bearing: without it iced lets
/// hover events bleed through the `Stack` to widgets below. The card's own
/// `MouseArea` is what stops a click *on the card* from falling through to
/// the scrim and triggering a dismiss, this helper owns that step because it
/// is the one every hand-rolled modal forgot.
pub(crate) fn modal_overlay<'a>(
    base: Element<'a, Message>,
    card: Element<'a, Message>,
    on_scrim_click: Option<Message>,
    top_reserve: f32,
) -> Element<'a, Message> {
    use iced::widget::{column, MouseArea};

    let scrim_fill = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
            ..Default::default()
        });
    let scrim_body: Element<'a, Message> = if top_reserve > 0.0 {
        column![Space::new().height(Length::Fixed(top_reserve)), scrim_fill]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        scrim_fill.into()
    };

    let scrim: Element<'a, Message> = MouseArea::new(scrim_body)
        .interaction(iced::mouse::Interaction::Idle)
        .on_press(on_scrim_click.unwrap_or(Message::NoOp))
        .into();

    let card_trap: Element<'a, Message> =
        MouseArea::new(card).on_press(Message::NoOp).into();
    let centered = container(card_trap)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    Stack::new()
        .push(base)
        .push(scrim)
        .push(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
