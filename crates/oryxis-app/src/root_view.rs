//! `Oryxis::view`, the top-level view router. Picks vault setup /
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
        } else if self.local_shell_picker_open {
            use iced::widget::{column, container, Space, Stack};
            use iced::{Color, Length};
            let modal = self.view_local_shell_picker();
            let scrim: Element<'_, Message> = column![
                Space::new().height(Length::Fixed(40.0)),
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(
                            0.0, 0.0, 0.0, 0.5,
                        ))),
                        ..Default::default()
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
            // The scrim itself dismisses the picker on click, outside-
            // click-to-close pattern shared with the SFTP modals. The
            // `interaction(Idle)` is what makes `Stack` levitate the
            // cursor so the cards underneath stop firing hover events;
            // without a non-`None` interaction the scrim is transparent
            // to mouse motion even though it eats clicks.
            let scrim: Element<'_, Message> = iced::widget::MouseArea::new(scrim)
                .interaction(iced::mouse::Interaction::Idle)
                .on_press(Message::HideLocalShellPicker)
                .into();
            Stack::new()
                .push(base)
                .push(scrim)
                .push(modal)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.plugin_install_modal.is_some() {
            use iced::widget::{column, container, Space, Stack};
            use iced::{Color, Length};
            let modal = self.view_plugin_install_modal();
            let scrim: Element<'_, Message> = column![
                Space::new().height(Length::Fixed(40.0)),
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(
                            0.0, 0.0, 0.0, 0.5,
                        ))),
                        ..Default::default()
                    }),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
            // Outside-click dismisses, same pattern as the other modals.
            // `interaction(Idle)` blocks hover bleed-through to widgets
            // below; see the local-shell scrim above for the rationale.
            let scrim: Element<'_, Message> = iced::widget::MouseArea::new(scrim)
                .interaction(iced::mouse::Interaction::Idle)
                .on_press(Message::HidePluginInstallModal)
                .into();
            Stack::new()
                .push(base)
                .push(scrim)
                .push(modal)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.pending_kbi_prompt.is_some() && self.connecting.is_none() {
            // Keyboard-interactive (2FA / OTP) prompt for a split-pane
            // connect, which has no connect-progress screen. During a normal
            // terminal connect `connecting` is Some and the prompt renders
            // inline, so this app-level overlay only fires otherwise.
            use iced::widget::{column, container, Space, Stack};
            use iced::{Color, Length};
            let modal = self.view_kbi_modal();
            let scrim: Element<'_, Message> = column![
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
            // Absorb clicks on the scrim so they don't fall through to the
            // live terminal behind. `interaction(Idle)` also blocks hover
            // bleed-through. No outside-click dismiss (on_press is a NoOp
            // sink): the user must submit or cancel so the in-flight auth
            // gets a definite answer.
            let scrim: Element<'_, Message> = iced::widget::MouseArea::new(scrim)
                .interaction(iced::mouse::Interaction::Idle)
                .on_press(Message::NoOp)
                .into();
            Stack::new()
                .push(base)
                .push(scrim)
                .push(modal)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if self.pending_host_key.is_some() && self.connecting.is_none() {
            // Host-key prompt for a backgrounded action (a manually toggled
            // port forward). During a terminal connect `connecting` is Some
            // and the prompt renders inline in the connect-progress view, so
            // this app-level overlay only fires when there's no such screen.
            use iced::widget::{column, container, Space, Stack};
            use iced::{Color, Length};
            let modal = self.view_host_key_modal();
            let scrim: Element<'_, Message> = column![
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
            // No outside-click dismiss: the user must pick reject / continue /
            // accept-and-save so the in-flight connect gets a definite answer.
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

        // Browser-style fullscreen overlays: on-enter hint banner and
        // hover-only round X. Both stack above any modal scrim so the
        // user can always escape immersive mode even when a picker is
        // open underneath.
        let composed = if self.window_fullscreen {
            self.layer_fullscreen_overlays(composed)
        } else {
            composed
        };

        // 1 px border around the entire app, drops to 0 when maximized
        // or in immersive fullscreen, since in both cases the OS / our
        // own chrome-hiding already clips the window to the monitor
        // edge and the border would be wasted (or worse, visible as a
        // halfway cut).
        //
        // The matching `padding(1)` is what makes the border actually
        // visible: without it, the inner Length::Fill children paint right
        // up to the container bounds and cover the 1 px frame.
        use iced::widget::container;
        use iced::{Background, Border, Length, Padding};
        let border_width = if self.window_maximized || self.window_fullscreen { 0.0 } else { 1.0 };
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
