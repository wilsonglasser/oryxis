//! Cloud Accounts panel, lists `CloudProfile` rows and houses the
//! add/edit form, the discovery panel, and the dynamic-group editor.
//!
//! Split into submodules per panel so each file stays focused on one
//! piece of UI, the cards list, the wizard form, the discovery panel
//! (and its results body), and the dynamic-group form.

mod discovery;
mod dynamic_form;
mod list;
mod wizard_form;

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, row, text, Space};
use iced::{Background, Border, Color, Element, Padding};

use crate::app::Message;
use crate::state::{CloudAuthChoice, CloudProviderChoice};
use crate::theme::OryxisColors;

/// Collapsible section header used by the discovery panel, chevron +
/// label, the whole row is a click target that toggles
/// `cloud_discover_collapsed[key]`. Same chevron convention used by
/// file trees: down = expanded, right = collapsed.
pub(super) fn section_header<'a>(
    key: &'static str,
    label: &str,
    collapsed: bool,
) -> Element<'a, Message> {
    let chevron = if collapsed {
        iced_fonts::lucide::chevron_right::<iced::Theme, iced::Renderer>()
    } else {
        iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
    };
    button(
        row![
            chevron.size(12).color(OryxisColors::t().text_muted),
            Space::new().width(6),
            text(label.to_owned())
                .size(13)
                .color(OryxisColors::t().text_secondary),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::CloudDiscoverToggleSection(key.to_string()))
    .padding(Padding {
        top: 4.0,
        right: 6.0,
        bottom: 4.0,
        left: 4.0,
    })
    .style(|_, status| {
        let bg = match status {
            BtnStatus::Hovered => OryxisColors::t().bg_hover,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(4.0),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

// `CloudProviderChoice` and `CloudAuthChoice` need `Display` for
// `pick_list`'s default mapper, but we use the closure form in the
// wizard so these are kept here as a backstop for future call sites.
impl std::fmt::Display for CloudProviderChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aws => write!(f, "AWS"),
            Self::K8s => write!(f, "Kubernetes"),
        }
    }
}

impl std::fmt::Display for CloudAuthChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile => write!(f, "Profile"),
            Self::AccessKey => write!(f, "Access Key"),
            Self::Sso => write!(f, "SSO"),
            Self::Kubeconfig => write!(f, "Kubeconfig"),
        }
    }
}
