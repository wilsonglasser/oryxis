//! UI helper widgets: overlay. Split out of widgets/mod.rs.

use iced::Widget as _;
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

    impl<Message> iced::advanced::widget::Meta for BoundsReporter<'_, Message> {}

    impl<Message> Widget<Message, Theme, iced::Renderer> for BoundsReporter<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.tag()
        }
        fn state(&self) -> tree::State {
            self.content.state()
        }
        fn diff(&mut self, tree: &mut Tree) {
            self.content.diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.size()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) {
            self.content
                .layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            // Draw runs after final positioning, so `layout.bounds()`
            // here is the screen-space rect (offset by parent
            // translations). Cache it so anchor lookups in `update`
            // hit the correct on-screen coordinates.
            self.cell.set(layout.bounds());
            self.content
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .operate(tree, layout, viewport, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content.update(
                tree, event, layout, cursor, renderer, shell, viewport,
            );
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
            window: Size,
        ) -> Vec<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content.overlay(
                tree,
                layout,
                renderer,
                viewport,
                translation,
                window,
            )
        }
    }

    BoundsReporter {
        content: content.into(),
        cell,
    }.boxed()
}

/// Shared cell type for `press_hit_reporter`: holds the value of the
/// last wrapper a left press landed on, consumed with `take()` by the
/// press handler so a stale hit can never outlive its press.
pub(crate) type PressHitCell<T> = std::rc::Rc<std::cell::RefCell<Option<T>>>;

/// Build a fresh, empty `PressHitCell` ready to be cloned into
/// `press_hit_reporter` wrappers and held in app state for later takes.
pub(crate) fn new_press_hit_cell<T>() -> PressHitCell<T> {
    std::rc::Rc::new(std::cell::RefCell::new(None))
}

/// Wraps `content` and writes `value` into `cell` whenever a left mouse
/// press lands inside the widget's on-screen bounds. The test runs in
/// `update`, where every ancestor scrollable hands its children a cursor
/// already translated into their content space (and levitated while the
/// cursor is outside the viewport or over a scrollbar), so the answer
/// stays correct with the content scrolled, panned or clipped.
/// `bounds_reporter` cannot give that answer: `draw` receives
/// content-space bounds while the scroll translation lives in the
/// renderer, so a rect recorded there drifts from the screen by exactly
/// the scroll offset (issue #127). Everything delegates to the inner
/// widget, so behaviour is otherwise identical to the unwrapped child.
pub(crate) fn press_hit_reporter<'a, Message: 'a, T: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    cell: PressHitCell<T>,
    value: T,
) -> Element<'a, Message> {
    press_tap(content, move |_, _| {
        *cell.borrow_mut() = Some(value.clone());
    })
}

/// The press half of a row's click, as a message.
///
/// A row is a `button`, which publishes on RELEASE, so the press it
/// began with (where and when it landed) is lost by the time its message
/// is handled; that is what a double click is judged on. This wrapper
/// publishes `on_press(position)` the moment a left press lands inside
/// its bounds, AHEAD of whatever the wrapped child publishes for the
/// same gesture: both go through the one ordered widget-message queue,
/// so the row's own release message always finds its press already
/// handled, however many clicks a slow frame batched together. (The
/// global press subscription cannot give that guarantee: subscription
/// messages are delivered on their own channel, after or before the
/// widget's.) The position is in the row's content space, see
/// [`press_hit_reporter`]; two presses on one row compare correctly
/// because they see the same translation.
pub(crate) fn press_publisher<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    on_press: impl Fn(iced::Point) -> Message + 'a,
) -> Element<'a, Message> {
    press_tap(content, move |at, shell| shell.publish(on_press(at)))
}

/// The one press-observing decorator behind [`press_hit_reporter`] and
/// [`press_publisher`]: runs `on_press` with the (content-space) cursor
/// position when a left press lands inside the child's bounds, BEFORE
/// the child sees the event, and never captures it. Shape-stable: tag,
/// state and children are the child's, so wrapping a row or not never
/// re-keys the widget tree (issue #241).
pub(crate) fn press_tap<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_press: impl Fn(iced::Point, &mut iced::advanced::Shell<'_, Message>) + 'a,
) -> Element<'a, Message> {
    use iced::advanced::widget::{tree, Operation, Tree, Widget};
    use iced::advanced::{layout, mouse, overlay, renderer, Layout, Shell};
    use iced::{Event, Length as L, Rectangle, Size, Vector};

    type OnPress<'a, Message> = Box<dyn Fn(iced::Point, &mut Shell<'_, Message>) + 'a>;

    struct PressTap<'a, Message> {
        content: Element<'a, Message>,
        on_press: OnPress<'a, Message>,
    }

    impl<Message> iced::advanced::widget::Meta for PressTap<'_, Message> {}

    impl<Message> Widget<Message, Theme, iced::Renderer> for PressTap<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.tag()
        }
        fn state(&self) -> tree::State {
            self.content.state()
        }
        fn diff(&mut self, tree: &mut Tree) {
            self.content.diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.size()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) {
            self.content
                .layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .operate(tree, layout, viewport, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            // Run BEFORE delegating: the child (a button) captures the
            // press, but capture doesn't erase the event and rows don't
            // overlap, so at most one wrapper of a list runs per press.
            if matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            ) && let Some(at) = cursor.position_over(layout.bounds())
            {
                (self.on_press)(at, shell);
            }
            self.content.update(
                tree, event, layout, cursor, renderer, shell, viewport,
            );
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
            window: Size,
        ) -> Vec<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content.overlay(
                tree,
                layout,
                renderer,
                viewport,
                translation,
                window,
            )
        }
    }

    PressTap {
        content: content.into(),
        on_press: Box::new(on_press),
    }.boxed()
}

/// Transparent decorator that announces its laid-out bounds under `id`
/// via the standard `container()` operation hook during `operate` (NOT
/// draw). A scroll-into-view operation can then find the row's layout
/// position even when it is scrolled far off-screen: `operate` traverses
/// the whole widget tree, while `draw` (and thus `bounds_reporter`) is
/// culled per child against the viewport, so the draw path can never see
/// an off-screen row. Everything except `operate` delegates to the child,
/// so it is layout- and paint-transparent.
pub(crate) fn report_container_id<'a, Message: 'a>(
    id: &'static str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    use iced::advanced::widget::{tree, Id, Operation, Tree, Widget};
    use iced::advanced::{layout, mouse, overlay, renderer, Layout, Shell};
    use iced::{Event, Length as L, Rectangle, Size, Vector};

    struct ReportId<'a, Message> {
        content: Element<'a, Message>,
        id: &'static str,
    }

    impl<Message> iced::advanced::widget::Meta for ReportId<'_, Message> {}

    impl<Message> Widget<Message, Theme, iced::Renderer> for ReportId<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.tag()
        }
        fn state(&self) -> tree::State {
            self.content.state()
        }
        fn diff(&mut self, tree: &mut Tree) {
            self.content.diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.size()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) {
            self.content.layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            operation.container(Some(&Id::new(self.id)), layout.bounds(), viewport);
            self.content
                .operate(tree, layout, viewport, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content.update(
                tree, event, layout, cursor, renderer, shell, viewport,
            );
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
            window: Size,
        ) -> Vec<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content
                .overlay(tree, layout, renderer, viewport, translation, window)
        }
    }

    ReportId {
        content: content.into(),
        id,
    }.boxed()
}

/// Scroll the row marked with `target_id` (via [`report_container_id`])
/// to `margin` px below the top of the scrollable `scroll_id`. Runs as a
/// two-pass widget operation: pass one reads the row's and the content's
/// layout tops during `operate` (which sees every widget, off-screen
/// included), pass two sets the scrollable's absolute offset. No draw /
/// timing dependency, unlike a bounds-cell estimate.
pub(crate) fn scroll_into_view_task(
    scroll_id: &'static str,
    target_id: &'static str,
    margin: f32,
) -> iced::Task<crate::app::Message> {
    use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, Scrollable};
    use iced::advanced::widget::operation::{Animation, Outcome};
    use iced::advanced::widget::{Id, Operation};
    use iced::{Rectangle, Size, Vector};

    struct Measure {
        scroll_id: Id,
        target_id: Id,
        margin: f32,
        content_top: Option<f32>,
        row_top: Option<f32>,
    }
    impl Operation for Measure {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn container(&mut self, id: Option<&Id>, bounds: Rectangle, _viewport: &Rectangle) {
            // The scrollable announces its CONTENT's bounds as a
            // container under its own id, right after `scrollable`, and
            // both those bounds and the rows' are untranslated layout
            // positions, so their difference is the row's offset inside
            // the content.
            if id == Some(&self.scroll_id) {
                self.content_top = Some(bounds.y);
            } else if id == Some(&self.target_id) {
                self.row_top = Some(bounds.y);
            }
        }
        fn finish(&self) -> Outcome<()> {
            match (self.content_top, self.row_top) {
                (Some(ct), Some(rt)) => Outcome::Chain(Box::new(DoScroll {
                    scroll_id: self.scroll_id.clone(),
                    // content_top is the (translated) content origin, so
                    // row_top - content_top is the row's offset within
                    // the content; place it `margin` below the top.
                    target: (rt - ct - self.margin).max(0.0),
                })),
                _ => Outcome::None,
            }
        }
    }

    struct DoScroll {
        scroll_id: Id,
        target: f32,
    }
    impl Operation for DoScroll {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Size,
            _translation: Vector,
            state: &mut dyn Scrollable,
        ) {
            if id == Some(&self.scroll_id) {
                // Instant, not the widget's smooth default: the row this
                // brings into view is the one the keyboard ring just moved
                // to, and it has to be on screen in the frame that draws
                // the ring.
                state.scroll_to(
                    AbsoluteOffset {
                        x: None,
                        y: Some(self.target),
                    },
                    Animation::Instant,
                    bounds,
                    content,
                );
            }
        }
    }

    iced::advanced::widget::operate(Measure {
        scroll_id: Id::new(scroll_id),
        target_id: Id::new(target_id),
        margin,
        content_top: None,
        row_top: None,
    })
    .discard()
}

/// The terminal pane the OS input method falls back to: its drawn rect
/// (`bounds_reporter`, filled by the pane's last draw), its emulator (for
/// the caret cell) and the font the grid is drawn in.
pub(crate) struct ImeTarget {
    pub(crate) bounds: BoundsCell,
    pub(crate) terminal: std::sync::Arc<std::sync::Mutex<oryxis_terminal::TerminalState>>,
    pub(crate) font_size: f32,
    pub(crate) font_name: String,
    pub(crate) font_weight: iced::font::Weight,
}

/// Wraps a window's whole view and, while `target` is a terminal pane, asks
/// the runtime to turn the OS IME on for it. The terminal is an `iced`
/// canvas, not a `text_input`, so nothing in its widget tree ever requests an
/// input method, and winit defaults `set_ime_allowed(false)`, which locks the
/// IME to direct (English) mode and blocks CJK composition. Every other
/// behaviour delegates straight to the inner widget, and the wrapper is
/// always there (`None` = no request), so the window's tree has one shape.
///
/// The terminal is the FALLBACK: a focused field (the find bar, a sidebar
/// search, the chat input, a modal's input) requests its own input method
/// while the tree is traversed, the shell keeps the first request it sees,
/// and this wrapper asks only after the whole view did and only when nothing
/// has. That is why it sits at the window root and not around the canvas:
/// wrapped around the pane, the terminal asked before every field laid out
/// after it (the find bar over the pane, a sidebar on the right, the
/// session-group panel) and the candidate window stayed on the grid. Fields in an overlay need nothing: the runtime updates the
/// overlays first and their request wins the merge.
///
/// The request is only honoured by the shell during a `RedrawRequested`
/// frame, so it is issued there. The committed text arrives as
/// `Event::InputMethod(Commit(..))` and is routed to the PTY in
/// `subscription.rs` / `dispatch_terminal`, not here. The candidate box is
/// anchored at the live caret inside the pane's rect (the whole view while
/// the pane has not been drawn yet), and the composed preedit is drawn
/// INLINE on the grid by the terminal widget itself (terminal font, at the
/// caret), so the request carries `preedit: None`: the runtime's
/// over-the-spot overlay would otherwise double-paint the composition in a
/// second font.
pub(crate) fn ime_host<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    target: Option<ImeTarget>,
) -> Element<'a, Message> {
    use iced::advanced::widget::{tree, Operation, Tree, Widget};
    use iced::advanced::{input_method, layout, mouse, overlay, renderer, Layout, Shell};
    use iced::{Event, Length as L, Point, Rectangle, Size, Vector};

    struct ImeHost<'a, Message> {
        content: Element<'a, Message>,
        target: Option<ImeTarget>,
    }

    impl<Message> iced::advanced::widget::Meta for ImeHost<'_, Message> {}

    impl<Message> Widget<Message, Theme, iced::Renderer> for ImeHost<'_, Message> {
        fn tag(&self) -> tree::Tag {
            self.content.tag()
        }
        fn state(&self) -> tree::State {
            self.content.state()
        }
        fn diff(&mut self, tree: &mut Tree) {
            self.content.diff(tree);
        }
        fn size(&self) -> Size<L> {
            self.content.size()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) {
            self.content.layout(tree, renderer, limits)
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .operate(tree, layout, viewport, renderer, operation);
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content
                .update(tree, event, layout, cursor, renderer, shell, viewport);

            let Some(target) = &self.target else {
                return;
            };
            // The shell only honours an input-method request issued during a
            // redraw frame, and a field the view just traversed may have
            // asked already: it keeps the method.
            if !matches!(event, Event::Window(iced::window::Event::RedrawRequested(_)))
                || shell.input_method().is_enabled()
            {
                return;
            }
            let b = Some(target.bounds.get())
                .filter(|r| r.width > 0.0 && r.height > 0.0)
                .unwrap_or_else(|| layout.bounds());
            // Anchor the OS candidate window at the terminal caret.
            // try_lock so a frame that races the render thread just
            // falls back to the bottom-left instead of blocking the UI.
            let cursor_area = match target.terminal.try_lock() {
                Ok(state) => oryxis_terminal::ime_caret_rect(
                    b,
                    target.font_size,
                    Some(target.font_name.as_str()),
                    target.font_weight,
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
                // A terminal surface: lets Wayland shape the OSK for a
                // PTY (extra keys, layout hints) instead of the generic
                // text-field form.
                purpose: input_method::Purpose::Terminal,
                preedit: None,
            };
            shell.request_input_method(&ime);
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.content
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
            window: Size,
        ) -> Vec<overlay::Element<'b, Message, Theme, iced::Renderer>> {
            self.content
                .overlay(tree, layout, renderer, viewport, translation, window)
        }
    }

    ImeHost {
        content: content.into(),
        target,
    }.boxed()
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
    modal_overlay_opt(base, Some((card, on_scrim_click, top_reserve)))
}

/// [`modal_overlay`] for a call site that may have no modal open: the
/// stack keeps the SAME shape either way (base + scrim slot + card
/// slot), with `Space::new()` placeholders standing in for the two
/// layers when `modal` is `None`.
///
/// That constant shape is the point. iced keys widget state by tree
/// POSITION, so a chain that returns a bare `base` while nothing is
/// open and `Stack{base, scrim, card}` once something is drops `base`
/// one level deeper the instant a modal opens: every scrollable inside
/// it is rebuilt from scratch and jumps back to the top. Opening "Add
/// terminal" from the bottom of Settings did exactly that. `layer_modals`
/// carries the same discipline for the in-view modals.
pub(crate) fn modal_overlay_opt<'a>(
    base: Element<'a, Message>,
    modal: Option<(Element<'a, Message>, Option<Message>, f32)>,
) -> Element<'a, Message> {
    use iced::widget::{column, MouseArea};

    let Some((card, on_scrim_click, top_reserve)) = modal else {
        return Stack::<iced::Element<'_, _>>::new()
            .push(base)
            .push(Space::new().boxed())
            .push(Space::new().boxed())
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed();
    };

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
            .boxed()
    } else {
        scrim_fill.boxed()
    };

    let scrim: Element<'a, Message> = MouseArea::new(scrim_body)
        .interaction(iced::mouse::Interaction::Idle)
        .on_press(on_scrim_click.unwrap_or(Message::NoOp))
        .boxed();

    let card_trap: Element<'a, Message> =
        MouseArea::new(card).on_press(Message::NoOp).boxed();
    let centered = container::<_, iced::Theme>(card_trap)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    Stack::<iced::Element<'_, _>>::new()
        .push(base)
        .push(scrim)
        .push(centered.boxed())
        .width(Length::Fill)
        .height(Length::Fill)
        .boxed()
}
