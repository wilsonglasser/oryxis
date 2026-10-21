//! The window root's `ime_host`: the terminal claims the OS input method
//! only when no field of the view claimed it, wherever that field sits
//! (PR #264: wrapped around the pane, the terminal won over every field
//! laid out after it).

use std::sync::{Arc, Mutex};

use iced::advanced::input_method::{InputMethod, Purpose};
use iced::advanced::renderer::Headless as _;
use iced::advanced::{mouse, shell};
use iced::widget::{column, container, text_input, Id};
use iced::{window, Element, Event, Length, Rectangle, Size};
use iced::Widget as _;
use iced_runtime::user_interface::{Cache, State, UserInterface};

use super::{ime_host, BoundsCell, ImeTarget};

const FIELD: &str = "ime-root-probe-field";

fn cell(rect: Rectangle) -> BoundsCell {
    std::rc::Rc::new(std::cell::Cell::new(rect))
}

fn target(bounds: BoundsCell) -> ImeTarget {
    let state = oryxis_terminal::TerminalState::new_no_pty(80, 24)
        .expect("a PTY-less terminal builds");
    ImeTarget {
        bounds,
        terminal: Arc::new(Mutex::new(state)),
        font_size: 14.0,
        font_name: String::new(),
        font_weight: iced::font::Weight::Normal,
    }
}

/// A pane first, a text field AFTER it: the order that let the pane's own
/// request win when the terminal asked from inside the tree.
fn view<'a>(target: Option<ImeTarget>) -> Element<'a, ()> {
    let pane = container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill);
    let field = text_input::<(), iced::Theme>("", "")
        .id(Id::new(FIELD))
        .on_input(|_| ());
    ime_host(column![pane.boxed(), field.boxed()].boxed(), target)
}

/// The input method one redraw settles on, with the field focused or not.
fn settled(target: Option<ImeTarget>, focus_field: bool) -> InputMethod {
    let mut renderer = crate::renderer_probe::block_on(iced::Renderer::new(
        iced::advanced::renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("tiny-skia renders headless");
    let mut ui = UserInterface::build(
        view(target),
        Size::new(800.0, 600.0),
        Cache::default(),
        &mut renderer,
    );
    if focus_field {
        ui.operate(
            &renderer,
            &mut iced::advanced::widget::operation::focusable::focus::<()>(Id::new(FIELD)),
        );
    }
    let mut messages = shell::Bus::new();
    let (state, _) = ui.update(
        &window::Headless,
        &shell::Waker::noop(),
        &[Event::Window(window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut messages,
    );
    match state {
        State::Updated { input_method, .. } => input_method,
        State::Outdated => panic!("one redraw cannot outdate the interface"),
    }
}

fn purpose(method: &InputMethod) -> Option<Purpose> {
    match method {
        InputMethod::Enabled { purpose, .. } => Some(*purpose),
        InputMethod::Disabled => None,
    }
}

#[test]
fn a_focused_field_after_the_pane_keeps_the_input_method() {
    let pane = Rectangle::new(iced::Point::new(0.0, 0.0), Size::new(800.0, 560.0));
    let method = settled(Some(target(cell(pane))), true);

    assert_eq!(purpose(&method), Some(Purpose::Normal), "{method:?}");
}

#[test]
fn with_no_field_focused_the_terminal_claims_it_at_its_pane() {
    let pane = Rectangle::new(iced::Point::new(120.0, 80.0), Size::new(400.0, 300.0));
    let method = settled(Some(target(cell(pane))), false);

    let InputMethod::Enabled { cursor, purpose, preedit } = method else {
        panic!("the terminal must enable the input method: {method:?}");
    };
    assert_eq!(purpose, Purpose::Terminal);
    assert!(preedit.is_none(), "the grid draws its own composition");
    assert!(
        pane.contains(cursor.position()),
        "the candidate window is anchored inside the pane, not the view: {cursor:?}"
    );
}

#[test]
fn an_undrawn_pane_anchors_inside_the_whole_view() {
    let method = settled(Some(target(cell(Rectangle::default()))), false);

    let InputMethod::Enabled { cursor, .. } = method else {
        panic!("the terminal must enable the input method: {method:?}");
    };
    assert!(
        Rectangle::with_size(Size::new(800.0, 600.0)).contains(cursor.position()),
        "{cursor:?}"
    );
}

#[test]
fn without_a_terminal_on_screen_nothing_is_claimed() {
    assert_eq!(settled(None, false), InputMethod::Disabled);
}
