//! Free-standing UI helper widgets used across views.
//!
//! Each helper is a `pub(crate) fn` returning an `Element<'_, Message>`. None of
//! them borrow from the top-level `Oryxis` struct, keeping them here lets view
//! modules compose these building blocks without polluting the state machine file.

pub(crate) use iced::border::Radius;
pub(crate) use iced::widget::button::Status as BtnStatus;
pub(crate) use iced::widget::{button, container, pick_list, text, text_editor, text_input, Row, Space, Stack};
pub(crate) use iced::{Background, Border, Color, Element, Length, Padding, Theme};

pub(crate) use crate::app::Message;
pub(crate) use crate::theme::OryxisColors;

/// Corner radius used for text inputs and pick lists across the UI.
/// Bumped from the iced default (~2 px) so form controls feel modern and
/// match the rounded look of the cards and buttons.
pub const INPUT_RADIUS: f32 = 10.0;

// Helper widgets split into themed sibling files.
mod buttons;
mod cards;
mod forms;
mod host_icon;
mod inputs;
mod layout;
mod overlay;
mod privacy;
mod toolbar;

pub(crate) use buttons::*;
pub(crate) use cards::*;
pub(crate) use forms::*;
pub(crate) use host_icon::*;
pub(crate) use inputs::*;
pub(crate) use layout::*;
pub(crate) use overlay::*;
pub(crate) use privacy::*;
pub(crate) use toolbar::*;
