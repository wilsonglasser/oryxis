//! A window's root, drawn in its vault's theme.
//!
//! Colours are read where they are used: style closures call
//! `OryxisColors::t()` while the widgets DRAW, which iced does after every
//! window's view was built and once per window. Two windows on two vaults
//! with two themes therefore cannot rely on one process-wide theme: each
//! window's root is wrapped here, and its draw (and the draw of every
//! overlay it opens: menus, pickers, tooltips) runs with the window's
//! theme in scope (`theme::with_selection`). Everything else delegates
//! straight to the content.

use iced::Widget as _;
use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{layout, mouse, overlay, renderer, Layout, Shell};
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector};

use crate::theme::{with_selection, ThemeSelection};

/// Wrap a window's root so it draws in `theme`.
pub(crate) fn theme_scope<'a, Message: 'a>(
    content: Element<'a, Message>,
    theme: ThemeSelection,
) -> Element<'a, Message> {
    ThemeScope { content, theme }.boxed()
}

struct ThemeScope<'a, Message> {
    content: Element<'a, Message>,
    theme: ThemeSelection,
}

impl<Message> iced::advanced::widget::Meta for ThemeScope<'_, Message> {}

impl<Message> Widget<Message, Theme, iced::Renderer> for ThemeScope<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.content.tag()
    }
    fn state(&self) -> tree::State {
        self.content.state()
    }
    fn diff(&mut self, tree: &mut Tree) {
        self.content.diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.size()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
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
        with_selection(self.theme, || {
            self.content.draw(tree, renderer, theme, style, layout, cursor, viewport);
        });
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.operate(tree, layout, viewport, renderer, operation);
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
        self.content.update(tree, event, layout, cursor, renderer, shell, viewport);
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.mouse_interaction(tree, layout, cursor, viewport, renderer)
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
        let theme = self.theme;
        scoped_overlays(
            self.content.overlay(tree, layout, renderer, viewport, translation, window),
            theme,
        )
    }
}

fn scoped_overlays<'b, Message: 'b>(
    overlays: Vec<overlay::Element<'b, Message, Theme, iced::Renderer>>,
    theme: ThemeSelection,
) -> Vec<overlay::Element<'b, Message, Theme, iced::Renderer>> {
    overlays
        .into_iter()
        .map(|inner| overlay::Element::new(Box::new(ScopedOverlay { inner, theme })))
        .collect()
}

/// An overlay of the window, drawn (with its own nested overlays) in the
/// window's theme.
struct ScopedOverlay<'b, Message> {
    inner: overlay::Element<'b, Message, Theme, iced::Renderer>,
    theme: ThemeSelection,
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for ScopedOverlay<'_, Message> {
    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        cursor: mouse::Cursor,
    ) {
        with_selection(self.theme, || {
            self.inner.as_overlay().draw(renderer, theme, style, cursor);
        });
    }
    fn operate(&mut self, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        self.inner.as_overlay_mut().operate(renderer, operation);
    }
    fn update(
        &mut self,
        event: &Event,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        self.inner.as_overlay_mut().update(event, cursor, renderer, shell);
    }
    fn mouse_interaction(&self, cursor: mouse::Cursor, renderer: &iced::Renderer) -> mouse::Interaction {
        self.inner.as_overlay().mouse_interaction(cursor, renderer)
    }
    fn overlay<'c>(
        &'c mut self,
        renderer: &iced::Renderer,
    ) -> Vec<overlay::Element<'c, Message, Theme, iced::Renderer>> {
        let theme = self.theme;
        scoped_overlays(self.inner.as_overlay_mut().overlay(renderer), theme)
    }
    fn index(&self) -> f32 {
        self.inner.as_overlay().index()
    }
}
