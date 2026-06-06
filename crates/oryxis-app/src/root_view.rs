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

        // App-level modals: surface even over the lock screen. All route
        // through `widgets::modal_overlay`, which owns the absorbing scrim,
        // the card click-trap, and the 40 px chrome reserve so the title bar
        // stays draggable. `None` = no outside-click dismiss (auth modals);
        // `Some(msg)` = backdrop click dismisses.
        let composed: Element<'_, Message> = if self.pending_update.is_some() {
            crate::widgets::modal_overlay(base, self.view_update_modal(), None, 40.0)
        } else if self.local_shell_picker_open {
            crate::widgets::modal_overlay(
                base,
                self.view_local_shell_picker(),
                Some(Message::HideLocalShellPicker),
                40.0,
            )
        } else if self.plugin_install_modal.is_some() {
            crate::widgets::modal_overlay(
                base,
                self.view_plugin_install_modal(),
                Some(Message::HidePluginInstallModal),
                40.0,
            )
        } else if self.pending_kbi_prompt.is_some() && self.connecting.is_none() {
            // Keyboard-interactive (2FA / OTP) for a split-pane connect (no
            // connect-progress screen). No outside-click dismiss: the user
            // must submit or cancel so the in-flight auth gets an answer.
            crate::widgets::modal_overlay(base, self.view_kbi_modal(), None, 40.0)
        } else if self.pending_host_key.is_some() && self.connecting.is_none() {
            // Host-key prompt for a backgrounded action (a manually toggled
            // port forward). No outside-click dismiss for the same reason.
            crate::widgets::modal_overlay(base, self.view_host_key_modal(), None, 40.0)
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
