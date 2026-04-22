//! Bottom status bar — connection state, keepalive info, and host summary.

use iced::border::Radius;
use iced::widget::{container, row, text, Space};
use iced::{Background, Border, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::theme::OryxisColors;

impl Oryxis {
    pub(crate) fn view_status_bar(&self) -> Element<'_, Message> {
        let status_text = if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(idx) {
                format!("● {} — connected", tab.label)
            } else {
                crate::i18n::t("no_active_connection").into()
            }
        } else {
            crate::i18n::t("no_active_connection").into()
        };

        let status_color = if self.active_tab.is_some() {
            OryxisColors::t().success
        } else {
            OryxisColors::t().text_muted
        };

        container(
            row![
                text(status_text).size(12).color(status_color),
                Space::new().width(Length::Fill),
                text(concat!("Oryxis v", env!("CARGO_PKG_VERSION"))).size(12).color(OryxisColors::t().text_muted),
            ]
            .padding(Padding { top: 4.0, right: 12.0, bottom: 4.0, left: 12.0 }),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
            border: Border { color: OryxisColors::t().border, width: 1.0, radius: Radius::from(0.0) },
            ..Default::default()
        })
        .into()
    }
}
