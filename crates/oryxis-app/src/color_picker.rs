//! A graphical HSV color picker (saturation/value square + hue bar) built
//! on iced's canvas. Third-party color-picker crates (iced_aw) target the
//! upstream iced and don't compile against the `wilsonglasser/iced` fork, so
//! this is a small custom widget. It edits one `ThemeColorSlot` of the
//! custom-theme editor, emitting `ThemeEditorColorChanged` with the new hex.

use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{Action, Frame, Geometry, Gradient, Path, Program, Stroke};
use iced::{mouse, Color, Element, Event, Length, Point, Rectangle, Size};

use crate::app::Message;
use crate::widgets::dir_row;

const SQUARE: f32 = 180.0;
const BAR_W: f32 = 22.0;

/// Shared closure: maps a picked `#rrggbb` to the editor's color-change
/// message. Lets the same picker drive both the terminal and UI theme
/// editors.
type OnChange = std::rc::Rc<dyn Fn(String) -> Message>;

/// The picker: an SV square + a hue bar, both editing the slot the
/// `on_change` closure targets.
pub(crate) fn color_picker<'a>(
    current: Color,
    on_change: impl Fn(String) -> Message + 'static,
) -> Element<'a, Message> {
    let on_change: OnChange = std::rc::Rc::new(on_change);
    let (h, s, v) = rgb_to_hsv(current);
    let sv = iced::widget::canvas(SvSquare { h, s, v, on_change: on_change.clone() })
        .width(Length::Fixed(SQUARE))
        .height(Length::Fixed(SQUARE));
    let hue = iced::widget::canvas(HueBar { h, s, v, on_change })
        .width(Length::Fixed(BAR_W))
        .height(Length::Fixed(SQUARE));
    dir_row(vec![sv.into(), iced::widget::Space::new().width(12).into(), hue.into()])
        .into()
}

fn to_hex(c: Color) -> String {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", q(c.r), q(c.g), q(c.b))
}

/// Saturation / value square for a fixed hue.
struct SvSquare {
    h: f32,
    s: f32,
    v: f32,
    on_change: OnChange,
}

impl Program<Message> for SvSquare {
    type State = bool; // dragging

    fn update(
        &self,
        dragging: &mut bool,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        // Map the cursor (clamped to the square) to (saturation, value).
        let sv = |cur: mouse::Cursor| -> Option<(f32, f32)> {
            let p = cur.position()?;
            let x = (p.x - bounds.x).clamp(0.0, bounds.width);
            let y = (p.y - bounds.y).clamp(0.0, bounds.height);
            Some((x / bounds.width, 1.0 - y / bounds.height))
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.position_in(bounds).is_some() =>
            {
                *dragging = true;
                let (s, v) = sv(cursor)?;
                return Some(emit(self.h, s, v, &self.on_change));
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if *dragging => {
                let (s, v) = sv(cursor)?;
                return Some(emit(self.h, s, v, &self.on_change));
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                *dragging = false;
            }
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let hue_color = hsv_to_rgb(self.h, 1.0, 1.0);
        // White -> full-hue, left to right.
        let base = Gradient::Linear(
            Linear::new(Point::new(0.0, 0.0), Point::new(w, 0.0))
                .add_stop(0.0, Color::WHITE)
                .add_stop(1.0, hue_color),
        );
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), base);
        // Transparent -> black, top to bottom (darkens toward the bottom).
        let shade = Gradient::Linear(
            Linear::new(Point::new(0.0, 0.0), Point::new(0.0, h))
                .add_stop(0.0, Color::TRANSPARENT)
                .add_stop(1.0, Color::BLACK),
        );
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), shade);
        // Marker.
        let mx = self.s * w;
        let my = (1.0 - self.v) * h;
        frame.stroke(
            &Path::circle(Point::new(mx, my), 6.0),
            Stroke::default().with_color(Color::WHITE).with_width(2.0),
        );
        frame.stroke(
            &Path::circle(Point::new(mx, my), 7.5),
            Stroke::default().with_color(Color::BLACK).with_width(1.0),
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Vertical hue bar.
struct HueBar {
    h: f32,
    s: f32,
    v: f32,
    on_change: OnChange,
}

impl Program<Message> for HueBar {
    type State = bool;

    fn update(
        &self,
        dragging: &mut bool,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let hue = |cur: mouse::Cursor| -> Option<f32> {
            let p = cur.position()?;
            let y = (p.y - bounds.y).clamp(0.0, bounds.height);
            Some((y / bounds.height) * 360.0)
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.position_in(bounds).is_some() =>
            {
                *dragging = true;
                let hh = hue(cursor)?;
                return Some(emit(hh, self.s, self.v, &self.on_change));
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if *dragging => {
                let hh = hue(cursor)?;
                return Some(emit(hh, self.s, self.v, &self.on_change));
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                *dragging = false;
            }
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let mut grad = Linear::new(Point::new(0.0, 0.0), Point::new(0.0, h));
        for i in 0..=6 {
            let off = i as f32 / 6.0;
            grad = grad.add_stop(off, hsv_to_rgb(off * 360.0, 1.0, 1.0));
        }
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), Gradient::Linear(grad));
        // Marker line at the current hue.
        let my = (self.h / 360.0) * h;
        frame.fill_rectangle(
            Point::new(0.0, (my - 1.5).max(0.0)),
            Size::new(w, 3.0),
            Color::WHITE,
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

fn emit(h: f32, s: f32, v: f32, on_change: &OnChange) -> Action<Message> {
    let hex = to_hex(hsv_to_rgb(h, s, v));
    Action::publish(on_change(hex)).and_capture()
}

/// HSV (`h` in degrees, `s`/`v` in 0..1) to an iced `Color`.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::from_rgb(r + m, g + m, b + m)
}

/// iced `Color` to HSV (`h` degrees, `s`/`v` 0..1).
fn rgb_to_hsv(c: Color) -> (f32, f32, f32) {
    let (r, g, b) = (c.r, c.g, c.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h.rem_euclid(360.0), s, max)
}
