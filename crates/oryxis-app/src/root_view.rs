//! `Oryxis::view` — the top-level view router. Picks vault setup /
//! unlock / main, layers the auto-update modal, and wraps the whole
//! thing in a 1px frame. Pulled out of `app.rs` so it's easier to find.

use iced::Element;

use crate::app::{Message, Oryxis};
use crate::state::VaultState;
use crate::theme::OryxisColors;

impl Oryxis {
    pub fn view(&self) -> Element<'_, Message> {
        let base = match self.vault_state {
            VaultState::Loading => self.view_vault_error("Failed to open vault database"),
            VaultState::NeedSetup => self.view_vault_setup(),
            VaultState::Locked => self.view_vault_unlock(),
            VaultState::Unlocked => self.view_main(),
        };

        // Auto-update modal is application-level so it surfaces on the lock
        // screen too. Rendered via Stack with a scrim that carves out the
        // top 28 px so the window chrome stays draggable.
        let composed: Element<'_, Message> = if self.pending_update.is_some() {
            use iced::widget::{column, container, Space, Stack};
            use iced::{Color, Length};
            let modal = self.view_update_modal();
            let scrim: Element<'_, Message> = column![
                // Reserve the chrome bar area so drag / min / max / close
                // still land on the underlying buttons.
                Space::new().height(Length::Fixed(40.0)),
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
                        ..Default::default()
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
            Stack::new()
                .push(base)
                .push(scrim)
                .push(modal)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            base
        };

        // 1 px border around the entire app — drops to 0 when maximized,
        // since the OS already clips the window to the monitor edge and the
        // border would be wasted (or worse, visible as a halfway cut).
        //
        // The matching `padding(1)` is what makes the border actually
        // visible: without it, the inner Length::Fill children paint right
        // up to the container bounds and cover the 1 px frame.
        use iced::widget::container;
        use iced::{Background, Border, Length, Padding};
        let border_width = if self.window_maximized { 0.0 } else { 1.0 };
        container(composed)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::from(border_width))
            .style(move |_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                border: Border {
                    radius: iced::border::Radius::from(0.0),
                    color: OryxisColors::t().border,
                    width: border_width,
                },
                ..Default::default()
            })
            .into()
    }
}
