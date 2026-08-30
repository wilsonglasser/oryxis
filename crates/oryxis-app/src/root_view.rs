//! `Oryxis::view`, the top-level view router. Picks vault setup /
//! unlock / main, layers the auto-update modal, and wraps the whole
//! thing in a 1px frame. Pulled out of `app.rs` so it's easier to find.

use iced::Element;

use crate::app::{SettingsMessage, KeysMessage, Message, Oryxis};
use crate::state::VaultState;
use crate::theme::OryxisColors;

impl Oryxis {
    pub fn view(&self) -> Element<'_, Message> {
        // Stall watchdog (#104): proves iced is still drawing. A live
        // update heartbeat with this one dead means the freeze sits in
        // presentation, not in our handlers.
        crate::stall_watchdog::beat_view();
        let base = match self.vault_ui.state {
            VaultState::Loading => self.view_vault_error("Failed to open vault database"),
            // First-run welcome / onboarding carousel, full page.
            VaultState::NeedSetup => crate::views::vault::with_chrome(
                self.view_onboarding_page(),
                self.window_maximized,
            ),
            VaultState::Locked => self.view_vault_unlock(),
            VaultState::Unlocked => self.view_main(),
        };

        // App-level modals: surface even over the lock screen. All route
        // through `widgets::modal_overlay_opt`, which owns the absorbing
        // scrim, the card click-trap, and the 40 px chrome reserve so the
        // title bar stays draggable. `None` = no outside-click dismiss
        // (auth modals); `Some(msg)` = backdrop click dismisses.
        //
        // The whole chain resolves to ONE card (or none) and hands it to a
        // single `modal_overlay_opt` call, so `base` sits at the same tree
        // depth whether or not a modal is up. Returning a bare `base` from
        // the empty arm used to reset every scrollable behind the modal to
        // the top the moment one opened; `layer_modals` documents the same
        // rule for the in-view modals.
        let modal: Option<(Element<'_, Message>, Option<Message>, f32)> =
            if self.local_shell_picker_open {
                Some((
                    self.view_local_shell_picker(),
                    Some(Message::Settings(SettingsMessage::HideLocalShellPicker)),
                    40.0,
                ))
            } else if self.local_terminal_add_open && !self.panels.icon_picker {
                // The add / edit modal yields while the shared icon picker is
                // up (the picker layers inside `view_main`, below this overlay);
                // it reappears with the chosen icon / color on picker save.
                Some((
                    self.view_local_terminal_add_modal(),
                    Some(Message::Settings(SettingsMessage::CloseLocalTerminalAddModal)),
                    40.0,
                ))
            } else if self.pending_kbi_prompt.is_some() && self.connecting.is_none() {
                // Keyboard-interactive (2FA / OTP) for a split-pane connect (no
                // connect-progress screen). No outside-click dismiss: the user
                // must submit or cancel so the in-flight auth gets an answer.
                Some((self.view_kbi_modal(), None, 40.0))
            } else if self.pending_proxy_command.is_some() && self.connecting.is_none() {
                // Command-proxy approval for a dial with no
                // connect-progress screen (split pane, manually toggled
                // forward, SFTP mount, backup, remote-desktop launcher).
                // Stacked ABOVE the host-key prompt because it is asked
                // first: the proxy spawns before the handshake, so the
                // two can never be pending at once, and the order here
                // says which one the dial reaches first. No
                // outside-click dismiss: the parked dial needs an answer.
                Some((self.view_proxy_command_modal(), None, 40.0))
            } else if self.pending_host_key.is_some() && self.connecting.is_none() {
                // Host-key prompt for a backgrounded action (a manually toggled
                // port forward). No outside-click dismiss for the same reason.
                Some((self.view_host_key_modal(), None, 40.0))
            } else if self.cert_viewer.is_some()
                && matches!(self.vault_ui.state, VaultState::Unlocked)
            {
                // Read-only certificate viewer (B2). Vault-area modal: gated on
                // Unlocked, and swept by the soft-lock so it can't linger over
                // the lock screen. Backdrop click closes it.
                Some((
                    self.view_cert_viewer_modal(),
                    Some(Message::Keys(KeysMessage::CloseCertViewer)),
                    40.0,
                ))
            } else {
                None
            };
        let composed: Element<'_, Message> = crate::widgets::modal_overlay_opt(base, modal);

        // SFTP dialogs (picker / rename / new / properties / overwrite /
        // delete) layer here so they blanket the whole window like the
        // global pickers above, instead of only the SFTP panes. No-op when
        // no SFTP modal is open. This keeps the invariant that a set modal
        // flag always corresponds to a visible, input-owning overlay.
        // Gated on Unlocked: unlike the app-level modals above (update /
        // plugin install, which are meant to surface over the lock screen),
        // an SFTP modal carries remote paths and live action buttons
        // (Save & Upload / Delete) and must never render or accept input
        // over a soft-locked vault. `SoftLockVault` also sweeps these
        // fields so nothing stale reappears after unlock.
        let composed = if matches!(self.vault_ui.state, VaultState::Unlocked) {
            self.layer_sftp_modals(composed)
        } else {
            composed
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

        // The toast chip floats over everything while unlocked (Dashboard,
        // Settings, modal scrims): smart-tab / OSC 9 notifications and
        // copy feedback fire from any view, and a toast mounted only
        // inside the terminal area is invisible from the vault views.
        // The lock screen deliberately drops it, a background session's
        // notification must not leak onto a locked UI.
        // Always wrap in a Stack, even when the toast is absent, so
        // the root widget type never changes and scrollable positions
        // survive the view rebuild. The placeholder stays Shrink-sized
        // (the repo's void-Space discipline): a Fixed(0.0) child is
        // exactly the shape that once made child counts vary.
        let composed = if matches!(self.vault_ui.state, VaultState::Unlocked) {
            let overlay = self
                .toast_overlay()
                .unwrap_or_else(|| iced::widget::Space::new().into());
            iced::widget::Stack::new()
                .push(composed)
                .push(overlay)
                .into()
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
