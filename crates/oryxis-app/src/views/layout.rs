//! Root layout, `view_main`, `render_overlay_menu`, and the content dispatcher.

use iced::border::Radius;
use iced::widget::{button, column, container, row, text, text_input, MouseArea, Space, Stack};
use iced::window::Direction;
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{Message, Oryxis};
use crate::state::{OverlayContent, OverlayState, View};
use crate::theme::OryxisColors;
use crate::widgets::{context_menu_item, dir_align_x, dir_row, styled_button};

/// One vault sub-nav destination: (i18n label key, target view).
type SubnavPill = (&'static str, View);

/// Thickness of the edge hit-zones used for dragging to resize. Corners are
/// the same thickness but `EDGE × EDGE` squares, a bit generous so the user
/// can actually grab them without millimetre precision.
const RESIZE_EDGE: f32 = 5.0;

/// Owned-label variant of `styled_button` for the error dialog link. The
/// label and URL come from `ErrorDialog` clones with no static lifetime,
/// so `styled_button(&str, ...)` would dangle on return.
/// Primary recovery-action button for the error dialog. Owned label
/// (the text comes from dialog state, not a 'static i18n ref); pressing
/// fires `ErrorDialogRunAction`, which dismisses the dialog and
/// dispatches the action's carried message.
fn dialog_action_button<'a>(label: String, danger: bool) -> Element<'a, Message> {
    let color = if danger {
        OryxisColors::t().error
    } else {
        OryxisColors::t().accent
    };
    let fg = OryxisColors::t().button_text;
    button(
        container(
            text(label).size(12).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(fg),
        )
        .padding(Padding { top: 5.0, right: 18.0, bottom: 5.0, left: 18.0 }),
    )
    .on_press(Message::ErrorDialogRunAction)
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color { a: 0.85, ..color },
            _ => color,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

fn open_link_button<'a>(label: String, url: String) -> Element<'a, Message> {
    let color = OryxisColors::t().accent;
    let fg = OryxisColors::t().button_text;
    button(
        container(
            text(label).size(12).font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
            }).color(fg),
        )
        .padding(Padding { top: 5.0, right: 18.0, bottom: 5.0, left: 18.0 }),
    )
    .on_press(Message::OpenUrl(url))
    .style(move |_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color {
                a: 1.0,
                r: (color.r + 0.05).min(1.0),
                g: (color.g + 0.05).min(1.0),
                b: (color.b + 0.05).min(1.0),
            },
            _ => color,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            border: Border { radius: Radius::from(6.0), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Invisible hit-zone used on the window edges and corners. Captures a press
/// and hands off to the OS as a native resize drag. Double-click on N/S
/// expands to full monitor height, same convention Windows uses (no
/// horizontal equivalent, so E/W stays drag-only).
fn resize_handle<'a>(direction: Direction, width: Length, height: Length) -> Element<'a, Message> {
    let mut area = MouseArea::new(container(Space::new()).width(width).height(height))
        .on_press(Message::WindowResizeDrag(direction))
        .interaction(match direction {
            Direction::North | Direction::South => iced::mouse::Interaction::ResizingVertically,
            Direction::East | Direction::West => iced::mouse::Interaction::ResizingHorizontally,
            Direction::NorthEast | Direction::SouthWest => iced::mouse::Interaction::ResizingDiagonallyUp,
            Direction::NorthWest | Direction::SouthEast => iced::mouse::Interaction::ResizingDiagonallyDown,
        });
    if matches!(direction, Direction::North | Direction::South) {
        area = area.on_double_click(Message::WindowExpandVertical);
    }
    area.into()
}

/// Layers the resize border on top of the given content, or returns the
/// content untouched when the window is maximized (no borders to grab).
pub(crate) fn wrap_with_resize<'a>(
    content: Element<'a, Message>,
    overlay: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    match overlay {
        Some(overlay) => Stack::new()
            .push(content)
            .push(overlay)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => content,
    }
}

/// Transparent border frame made of 8 resize hit-zones (4 edges + 4 corners).
/// The centre is a `Space` with fill so pointer events fall through to the
/// base layer underneath.
pub(crate) fn resize_border<'a>() -> Element<'a, Message> {
    let t = RESIZE_EDGE;
    column![
        row![
            resize_handle(Direction::NorthWest, Length::Fixed(t), Length::Fixed(t)),
            resize_handle(Direction::North, Length::Fill, Length::Fixed(t)),
            resize_handle(Direction::NorthEast, Length::Fixed(t), Length::Fixed(t)),
        ]
        .height(Length::Fixed(t)),
        row![
            resize_handle(Direction::West, Length::Fixed(t), Length::Fill),
            Space::new().width(Length::Fill).height(Length::Fill),
            resize_handle(Direction::East, Length::Fixed(t), Length::Fill),
        ]
        .height(Length::Fill),
        row![
            resize_handle(Direction::SouthWest, Length::Fixed(t), Length::Fixed(t)),
            resize_handle(Direction::South, Length::Fill, Length::Fixed(t)),
            resize_handle(Direction::SouthEast, Length::Fixed(t), Length::Fixed(t)),
        ]
        .height(Length::Fixed(t)),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

impl Oryxis {
    /// Wrap a card in the shared accent wash when the
    /// `setting_card_accent_glass` toggle is on, else return it bare.
    /// The single gate every card list (dashboard + internal screens)
    /// routes through so the toggle governs them all uniformly.
    pub(crate) fn card_wash<'a>(
        &self,
        card: Element<'a, Message>,
        color: Color,
    ) -> Element<'a, Message> {
        if self.setting_card_accent_glass {
            crate::widgets::card_accent_wash(card, color)
        } else {
            card
        }
    }

    /// Cheap check: is a vault-area side panel currently open? Mirrors
    /// `active_side_panel` without building the element, so callers (e.g.
    /// the sub-nav width budget) can branch without the render cost.
    fn side_panel_open(&self) -> bool {
        if self.active_tab.is_some() {
            return false;
        }
        match self.active_view {
            View::Dashboard => {
                self.cloud_discover_visible
                    || self.cloud_dynamic_form_visible
                    || self.group_edit_visible
                    || self.show_host_panel
                    || self.show_session_group_panel
            }
            View::Keys => self.show_key_panel || self.show_identity_panel,
            View::Snippets => self.show_snippet_panel,
            View::PortForwarding => self.show_port_forward_panel,
            View::Proxies => self.proxy_identity_form_visible,
            View::Cloud => self.cloud_form_visible,
            _ => false,
        }
    }

    /// Number of vaults the user has. Multi-vault isn't built yet, so
    /// this is always 1 today; the switcher chrome keys off it.
    pub(crate) fn vault_count(&self) -> usize {
        1
    }

    /// Whether to show the vault switcher (the "Personal" chip in the
    /// sub-nav / badge in the rail). Hidden with a single vault, since
    /// there's nothing to switch between.
    pub(crate) fn show_vault_switcher(&self) -> bool {
        self.vault_count() > 1
    }

    /// True when the Home area is active: no connection tab open and the
    /// current view is one of the vault sub-sections. Gates the vault nav
    /// (horizontal sub-nav strip or vertical rail).
    pub(crate) fn in_vault_area(&self) -> bool {
        self.active_tab.is_none()
            && matches!(
                self.active_view,
                View::Dashboard
                    | View::Keys
                    | View::Snippets
                    | View::PortForwarding
                    | View::Cloud
                    | View::Proxies
                    | View::KnownHosts
                    | View::History
            )
    }

    /// Width currently occupied by the left vault nav rail (the vertical
    /// icon rail). Zero in horizontal orientation or outside the vault
    /// area. The single source the content-width / pane-split math reads
    /// instead of the retired sidebar-collapse width.
    pub(crate) fn vault_rail_width(&self) -> f32 {
        if self.in_vault_area() && self.setting_nav_orientation == "vertical" {
            if self.setting_nav_rail_expanded {
                crate::app::NAV_RAIL_WIDTH_EXPANDED
            } else {
                crate::app::SIDEBAR_WIDTH_COLLAPSED
            }
        } else {
            0.0
        }
    }

    /// The side-panel editor currently open over the vault area, if any.
    /// Hoisted out of the individual views so `view_main` can place it
    /// beside the `column![sub_nav, content]` stack, letting the panel
    /// rise to cover the sub-nav band on its side (a full-height
    /// slide-over). Terminal tabs keep their own panel handling inside
    /// `view_terminal`, so this returns `None` whenever a session tab is
    /// active.
    fn active_side_panel(&self) -> Option<Element<'_, Message>> {
        if self.active_tab.is_some() {
            return None;
        }
        match self.active_view {
            View::Dashboard => {
                if self.cloud_discover_visible {
                    Some(self.view_cloud_discover_panel())
                } else if self.cloud_dynamic_form_visible {
                    Some(self.view_dynamic_group_form_panel())
                } else if self.group_edit_visible {
                    Some(self.view_group_edit_panel())
                } else if self.show_host_panel {
                    Some(self.view_host_panel())
                } else if self.show_session_group_panel {
                    Some(self.view_session_group_panel())
                } else {
                    None
                }
            }
            View::Keys => {
                if self.show_key_panel {
                    Some(self.view_key_import_panel())
                } else if self.show_identity_panel {
                    Some(self.view_identity_panel())
                } else {
                    None
                }
            }
            View::Snippets => self
                .show_snippet_panel
                .then(|| self.view_snippet_panel()),
            View::PortForwarding => self
                .show_port_forward_panel
                .then(|| self.view_port_forward_panel()),
            View::Proxies => self
                .proxy_identity_form_visible
                .then(|| self.view_proxy_identity_form()),
            View::Cloud => self.cloud_form_visible.then(|| self.view_cloud_form_panel()),
            _ => None,
        }
    }

    /// The accent colour the top bar "breathes": the active tab's
    /// per-host / per-session-group colour (or cloud brand) when a
    /// connection tab is open, else the app accent for Home / vault /
    /// settings. Independent of the on/off `setting_tab_accent_line`
    /// toggle, so both the bottom hairline and the bar wash share one
    /// source of truth.
    pub(crate) fn top_accent_tint(&self) -> Color {
        if let Some(idx) = self.active_tab
            && let Some(tab) = self.tabs.get(idx)
        {
            let label = tab.label.trim_end_matches(" (disconnected)");
            // 0) session-group tabs breathe the group's own colour.
            if let Some(sg_id) = tab.session_group_id
                && let Some(g) = self.session_groups.iter().find(|g| g.id == sg_id)
                && let Some(col) = g.color.as_deref().and_then(crate::widgets::parse_hex_color)
            {
                return col;
            }
            // 1) per-host colour from a matching saved Connection.
            if let Some(c) = self.connections.iter().find(|c| c.label == label)
                && let Some(hex) = c.custom_color.as_deref().or(c.color.as_deref())
                && let Some(col) = crate::widgets::parse_hex_color(hex)
            {
                return col;
            }
            // 2) cloud-transport tabs inherit the parent dynamic-group
            //    brand colour (AWS orange, K8s blue, ...).
            if let Some(brand) = crate::os_icon::tab_label_cloud_brand(label) {
                return crate::os_icon::provider_icon(brand, OryxisColors::t().accent).1;
            }
        }
        // SFTP surface focused: breathe the active SFTP tab's host colour so the
        // bar wash glows the same as a terminal tab on that host.
        if self.active_tab.is_none()
            && self.active_view == View::Sftp
            && let Some(i) = self.active_sftp
            && let Some(tab) = self.sftp_tabs.get(i)
        {
            if let Some(col) = self
                .connections
                .iter()
                .find(|c| c.label == tab.label)
                .and_then(|c| c.custom_color.as_deref().or(c.color.as_deref()))
                .and_then(crate::widgets::parse_hex_color)
            {
                return col;
            }
            if let Some(os) = self.tab_detected_os(&tab.label) {
                return crate::os_icon::resolve_icon(Some(&os), OryxisColors::t().accent).1;
            }
        }
        OryxisColors::t().accent
    }

    pub(crate) fn view_main(&self) -> Element<'_, Message> {
        // Single top-bar layout: the tab bar (Home icon + session tabs +
        // burger) spans the full width; there is no classic full-height
        // sidebar. The vault sub-sections render either as a horizontal
        // pill strip below the bar or as a vertical icon rail on the left
        // of the content, per `setting_nav_orientation`.
        // Browser-style fullscreen suppresses every piece of chrome (tab
        // bar, status bar) so the content fills the monitor edge-to-edge.
        // The X-close affordance and the on-enter hint banner are drawn as
        // Stack overlays below.
        let immersive = self.window_fullscreen;
        let tab_bar: Element<'_, Message> = if immersive {
            Space::new().height(0).into()
        } else {
            self.view_tab_bar()
        };
        let content = self.view_content();
        // Status bar is opt-out (Interface → Show status bar) and
        // also suppressed in immersive fullscreen.
        let status_bar: Element<'_, Message> = if self.setting_show_status_bar && !immersive {
            self.view_status_bar()
        } else {
            Space::new().height(0).into()
        };

        // Tab-bar bottom hairline. When a connection tab is active and
        // it has a per-host accent color, paint the hairline 2 px and
        // tint it that color (JetBrains-style "respiração" of the
        // active project). Falls back to the global accent for tabs
        // without a per-host color, and the neutral border for non-
        // connection screens so settings / dashboard don't look like
        // they belong to whichever host happened to be open last.
        let accent_tint: Option<Color> = if self.setting_tab_accent_line {
            Some(self.top_accent_tint())
        } else {
            None
        };
        let (hair_height, hair_color) = match accent_tint {
            Some(c) => (2.0_f32, c),
            None => (1.0_f32, OryxisColors::t().border),
        };
        let h_separator: Element<'_, Message> = if immersive {
            Space::new().height(0).into()
        } else {
            container(Space::new().height(hair_height))
                .width(Length::Fill)
                .style(move |_| {
                    // When the accent line is on, the border washes
                    // left→right (bright accent on the leading edge fading
                    // out), matching the card accent wash and ready to
                    // double as an (infinite) progress bar later. Off →
                    // the neutral 1px border.
                    let bg = match accent_tint {
                        Some(c) => Background::Gradient(iced::Gradient::Linear(
                            iced::gradient::Linear::new(iced::Radians(
                                std::f32::consts::FRAC_PI_2,
                            ))
                            .add_stop(0.0, c)
                            .add_stop(0.85, Color { a: 0.0, ..c }),
                        )),
                        None => Background::Color(hair_color),
                    };
                    container::Style {
                        background: Some(bg),
                        ..Default::default()
                    }
                })
                .into()
        };
        // Vault contextual nav: shown only when the Home area is active.
        // On Sftp / Settings / a connection tab it's hidden.
        let in_vault_area = self.in_vault_area();
        let vertical_rail = self.setting_nav_orientation == "vertical";
        // Horizontal pill strip pinned above the content.
        let sub_nav: Element<'_, Message> = if in_vault_area && !vertical_rail {
            self.view_vault_sub_nav()
        } else {
            Space::new().height(0).into()
        };
        // Vertical icon rail on the leading edge of the content.
        let nav_rail: Option<Element<'_, Message>> = if in_vault_area && vertical_rail {
            Some(self.view_vault_nav_rail())
        } else {
            None
        };

        // Compose the content with its nav (rail on the leading edge OR
        // sub-nav strip above) and the side panel (editor) on the trailing
        // edge. The side panel rises full-height, covering the sub-nav band
        // on its own side; the vertical rail stays on the leading edge.
        let inner: Element<'_, Message> = match nav_rail {
            Some(rail) => {
                // With the rail on the side (no sub-nav strip on top), the
                // view toolbars' 16px top padding reads as a tighter top
                // gutter than the 24px left gutter. Add 8px so the content's
                // top spacing matches its left and the corner looks square.
                let content = container(content)
                    .padding(Padding { top: 8.0, right: 0.0, bottom: 0.0, left: 0.0 })
                    .width(Length::Fill)
                    .height(Length::Fill);
                dir_row(vec![rail, content.into()]).height(Length::Fill).into()
            }
            None => column![sub_nav, content].height(Length::Fill).into(),
        };
        let body: Element<'_, Message> = match self.active_side_panel() {
            Some(panel) => dir_row(vec![inner, panel]).height(Length::Fill).into(),
            None => inner,
        };
        let right_side: Element<'_, Message> =
            column![tab_bar, h_separator, body].height(Length::Fill).into();
        let layout = column![right_side, status_bar];

        let base: Element<'_, Message> = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            })
            .into();

        // Edge/corner resize handles, only when the window isn't
        // maximized or in immersive fullscreen (no borders to grab in
        // either case). Placed as the topmost stack layer so they win
        // over tab-bar buttons near the frame, while the Space in the
        // middle is pass-through.
        let resize_overlay: Option<Element<'_, Message>> =
            if self.window_maximized || immersive { None } else { Some(resize_border()) };

        // SFTP close-guard: the close button lives in the always-visible tab
        // strip, so this modal must render globally (not just on the SFTP
        // surface) or a close click from a terminal would set the pending
        // state with no modal to resolve it.
        if self.pending_sftp_close.is_some() {
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(iced::widget::opaque(crate::views::sftp::close_guard_modal()))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Burger menu overlay (top-left dropdown). Renders first so any
        // other modal stacked below still wins, but in practice the
        // burger menu and the bigger modals (share dialog, picker, etc.)
        // never coexist on the user's screen at the same time.
        if self.show_burger_menu {
            let menu = self.view_burger_menu();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Vault sub-nav overflow ("…") dropdown, same overlay shape as
        // the burger menu.
        if self.show_subnav_overflow {
            let menu = self.view_subnav_overflow_menu();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Share dialog overlay
        if self.show_share_dialog {
            let share_include_keys = self.share_include_keys;
            let dialog_content = container(
                column![
                    text(crate::i18n::t("share")).size(16).color(OryxisColors::t().text_primary),
                    Space::new().height(12),
                    container(crate::widgets::password_input_with_eye(
                        crate::i18n::t("export_password"),
                        &self.share_password,
                        Message::SharePasswordChanged,
                        None,
                        self.revealed_secrets
                            .contains(&crate::state::SecretField::SharePassword),
                        Message::ToggleSecretVisibility(
                            crate::state::SecretField::SharePassword,
                        ),
                        10.0,
                    ))
                    .width(280),
                    Space::new().height(8),
                    row![
                        text(crate::i18n::t("include_private_keys")).size(13).color(OryxisColors::t().text_secondary),
                        Space::new().width(Length::Fill),
                        button(
                            text(if share_include_keys { "ON" } else { "OFF" }).size(12)
                        ).on_press(Message::ShareToggleKeys).style(move |_theme, _status| {
                            button::Style {
                                background: Some(Background::Color(if share_include_keys { OryxisColors::t().success } else { OryxisColors::t().bg_hover })),
                                border: Border { radius: Radius::from(4.0), ..Default::default() },
                                text_color: OryxisColors::t().text_primary,
                                ..Default::default()
                            }
                        }),
                    ].align_y(iced::Alignment::Center).width(280),
                    Space::new().height(12),
                    row![
                        styled_button(crate::i18n::t("share"), Message::ShareConfirm, OryxisColors::t().accent),
                        Space::new().width(8),
                        styled_button(crate::i18n::t("cancel"), Message::ShareDismiss, OryxisColors::t().text_muted),
                    ],
                    if let Some(status) = &self.share_status {
                        let (msg, color) = match status {
                            Ok(m) => (m.as_str(), OryxisColors::t().success),
                            Err(m) => (m.as_str(), OryxisColors::t().error),
                        };
                        Element::from(column![Space::new().height(8), text(msg).size(12).color(color)])
                    } else {
                        Element::from(Space::new().height(0))
                    },
                ]
                .padding(24),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog_content.into(),
                    Some(Message::ShareDismiss),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Generic blocking error dialog. Currently surfaces the
        // "AWS session-manager-plugin missing" case but reusable for
        // any "user must read this and act" failure. Title + body +
        // optional "open URL" button (the URL opens in the system
        // browser via Message::OpenUrl).
        if let Some(dialog) = self.error_dialog.clone() {
            let mut buttons = iced::widget::row![styled_button(
                crate::i18n::t("close"),
                Message::ErrorDialogDismiss,
                OryxisColors::t().text_muted,
            )]
            .spacing(8);
            if let Some(link) = dialog.link.clone() {
                buttons = buttons.push(open_link_button(link.label, link.url));
            }
            if let Some(action) = dialog.action.clone() {
                // Recovery action, accent-styled like the link button;
                // dispatching goes through ErrorDialogRunAction so the
                // dialog also dismisses itself.
                buttons = buttons.push(dialog_action_button(action.label, action.danger));
            }

            // Body uses Rich text with `.selectable(true)` so the user
            // can highlight and copy the failure message (key when the
            // dialog explains how to install a missing dependency or
            // includes a path / command to run).
            let body_span: iced::widget::text::Span<'_, ()> =
                iced::widget::text::Span::new(dialog.body.clone())
                    .color(OryxisColors::t().text_secondary);
            let dialog_body = iced::widget::text::Rich::<'_, (), Message>::with_spans(
                [body_span],
            )
            .size(13)
            .selectable(true);

            let dialog_content = container(
                column![
                    text(dialog.title.clone())
                        .size(16)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(12),
                    dialog_body,
                    Space::new().height(20),
                    buttons,
                ]
                .padding(24),
            )
            .max_width(520)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog_content.into(),
                    Some(Message::ErrorDialogDismiss),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Cloud import confirmation modal. Always opens on Import (no
        // ECS-only short-circuit) so the user can set the target
        // group from the same surface that already gates the
        // transport choice. Transport row hides itself when the
        // batch is ECS-only since dynamic groups always run ECS Exec.
        if self.cloud_import_confirm_visible {
            use oryxis_core::models::cloud::TransportKind;
            let n_ec2 = self.cloud_discover_selected_ec2.len();
            let n_ecs = self.cloud_discover_selected_ecs.len();
            let summary = if n_ec2 > 0 && n_ecs > 0 {
                format!("{} EC2 + {} ECS", n_ec2, n_ecs)
            } else if n_ec2 > 0 {
                format!("{} EC2", n_ec2)
            } else {
                format!("{} ECS", n_ecs)
            };

            // Import-into field + chevron. The suggestion dropdown
            // is no longer inline; it's a floating popover rendered
            // via the global OverlayState (`CloudDiscoverGroupPicker`)
            // injected into the modal's own Stack below so it can
            // visually rise above the dialog instead of pushing
            // siblings. Input + chevron heights are explicitly fixed
            // to 36 so they stay aligned in the row.
            const COMBO_HEIGHT: f32 = 36.0;
            let group_input = iced::widget::text_input(
                crate::i18n::t("cloud_discover_import_into_placeholder"),
                &self.cloud_discover_default_group_name,
            )
            .on_input(Message::CloudDiscoverDefaultGroupNameChanged)
            .padding(8)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x());
            let chevron_btn = iced::widget::button(
                container(
                    iced_fonts::lucide::chevron_down::<iced::Theme, iced::Renderer>()
                        .size(12)
                        .color(OryxisColors::t().text_muted),
                )
                .center_x(Length::Fixed(32.0))
                .center_y(Length::Fixed(COMBO_HEIGHT)),
            )
            .on_press(Message::ToggleCloudDiscoverGroupPicker)
            .padding(0)
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => OryxisColors::t().bg_hover,
                    _ => OryxisColors::t().bg_surface,
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(6.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });

            // Transport picker is always rendered. For ECS-only
            // imports the value is ignored on save (dynamic groups
            // always run ECS Exec), but keeping the row in place
            // preserves the row geometry + the explanatory hint
            // beneath it and avoids the modal looking sparse when
            // the user happens to pick zero EC2 hosts.
            let transport_section: Element<'_, Message> = {
                let transport_options = vec![
                    TransportKind::Ssh,
                    TransportKind::InstanceConnect,
                    TransportKind::Ssm,
                ];
                let transport_pick = iced::widget::pick_list(
                    Some(self.cloud_discover_default_transport),
                    transport_options,
                    |t| match t {
                        TransportKind::Ssh => "SSH".to_string(),
                        TransportKind::InstanceConnect => "EC2 Instance Connect".to_string(),
                        TransportKind::Ssm => "SSM Session".to_string(),
                        TransportKind::EcsExec => "ECS Exec".to_string(),
                        TransportKind::KubectlExec => "kubectl exec".to_string(),
                    },
                )
                .on_select(Message::CloudDiscoverDefaultTransportChanged)
                .padding(10)
                .style(crate::widgets::rounded_pick_list_style);
                column![
                    text(crate::i18n::t("cloud_dynamic_form_transport"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    container(transport_pick).width(Length::Fixed(320.0)),
                    Space::new().height(8),
                    text(crate::i18n::t("cloud_import_transport_hint"))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(16),
                ]
                .into()
            };
            // Silence the now-unused n_ec2 binding; kept by name so
            // the summary text above can read it without re-querying.
            let _ = n_ec2;

            let dialog_content = container(
                column![
                    text(crate::i18n::t("cloud_import_confirm_title"))
                        .size(16)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(4),
                    text(summary).size(11).color(OryxisColors::t().text_muted),
                    Space::new().height(16),
                    // "Import into" comes BEFORE Transport: the
                    // dropdown is anchored to the chevron and opens
                    // downward, so having the field higher in the
                    // dialog gives the menu maximum vertical room
                    // to extend without escaping the screen edge.
                    text(crate::i18n::t("cloud_discover_import_into"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    // Wrap the combo row in `bounds_reporter` so the
                    // toggle handler can read its on-screen rect and
                    // anchor the picker overlay right below it. The
                    // cell lives on Oryxis state; the wrapper here
                    // just writes to it on every draw pass. Wrapping
                    // the whole row (input + chevron) means the menu
                    // can mirror the full combo width by default,
                    // covering the empty area between the input and
                    // the chevron edge.
                    crate::widgets::bounds_reporter(
                        dir_row(vec![
                            container(group_input)
                                .width(Length::Fill)
                                .height(Length::Fixed(COMBO_HEIGHT))
                                .into(),
                            Space::new().width(6).into(),
                            container(chevron_btn)
                                .height(Length::Fixed(COMBO_HEIGHT))
                                .into(),
                        ])
                        .width(Length::Fixed(308.0))
                        .align_y(iced::Alignment::Center),
                        self.cloud_discover_default_group_combo_bounds.clone(),
                    ),
                    Space::new().height(16),
                    transport_section,
                    crate::widgets::dir_row(vec![
                        styled_button(
                            crate::i18n::t("import_btn_label"),
                            Message::CloudDiscoverImportConfirmed,
                            OryxisColors::t().accent,
                        ),
                        Space::new().width(8).into(),
                        styled_button(
                            crate::i18n::t("cancel"),
                            Message::CloudDiscoverImportCancelled,
                            OryxisColors::t().text_muted,
                        ),
                    ]),
                ]
                .padding(24),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(12.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });

            let centered = container(
                MouseArea::new(dialog_content).on_press(Message::NoOp),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

            // Intentionally NOT routed through `widgets::modal_overlay`:
            // this modal injects a positioned group-picker popover into its
            // own Stack (below) and uses a context-dependent scrim message,
            // neither of which the simple helper hosts. It stays mouse-safe
            // via `opaque` and keyboard-safe via `any_modal_blocks_input`.
            //
            // Scrim behaviour: while the group picker is open,
            // off-dialog clicks dismiss only the picker so the user
            // doesn't accidentally cancel the whole import. Wrapped
            // in `iced::widget::opaque` so hover events stop here
            // instead of bleeding through to the dashboard cards
            // beneath the modal (otherwise iced's Stack lets mouse
            // hover propagate to lower layers, lighting up rows
            // under the cursor while the modal is open).
            let on_scrim_click = if self.cloud_discover_default_group_picker_open {
                Message::ToggleCloudDiscoverGroupPicker
            } else {
                Message::CloudDiscoverImportCancelled
            };
            let scrim: Element<'_, Message> = iced::widget::opaque(
                MouseArea::new(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Background::Color(Color::from_rgba(
                                0.0, 0.0, 0.0, 0.5,
                            ))),
                            ..Default::default()
                        }),
                )
                .on_press(on_scrim_click),
            );

            // Group-picker context menu: same pattern as the
            // existing kebab menus. Built via the global
            // `OverlayState` + `render_overlay_menu` pipeline so the
            // menu styling, backdrop, and dismiss-on-click-outside
            // all behave like every other context menu in the app.
            // Injected here (inside the modal's Stack) because the
            // modal short-circuits the global overlay path further
            // down in `view_main`.
            let mut modal_stack =
                Stack::new().push(base).push(scrim).push(centered);
            if let Some(ref ovl) = self.overlay
                && matches!(ovl.content, OverlayContent::CloudDiscoverGroupPicker)
            {
                let menu = self.render_overlay_menu(ovl);
                // Width matches the combo's measured width from the
                // bounds_reporter (falls back to 308 on the very
                // first open when the cell is still zeroed). Height
                // clamp keeps tall menus on-screen.
                let combo = self.cloud_discover_default_group_combo_bounds.get();
                let menu_width = if combo.width > 0.0 { combo.width } else { 308.0 };
                let menu_height = 280.0_f32;
                let x = ovl
                    .x
                    .min((self.window_size.width - menu_width).max(0.0))
                    .max(0.0);
                let y = ovl
                    .y
                    .min((self.window_size.height - menu_height).max(0.0))
                    .max(0.0);
                let backdrop: Element<'_, Message> = MouseArea::new(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .on_press(Message::ToggleCloudDiscoverGroupPicker)
                .into();
                let positioned: Element<'_, Message> = column![
                    Space::new().height(y),
                    row![
                        Space::new().width(x),
                        container(menu).width(Length::Fixed(menu_width)),
                    ],
                ]
                .into();
                modal_stack = modal_stack.push(backdrop).push(positioned);
            }

            return wrap_with_resize(
                modal_stack
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Folder rename modal, shown after the user picks "Rename" from
        // the folder context menu.
        if let Some((_gid, ref input)) = self.folder_rename {
            let dialog = container(
                column![
                    text(crate::i18n::t("rename_folder"))
                        .size(16)
                        .color(OryxisColors::t().text_primary)
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                    Space::new().height(12),
                    text_input(crate::i18n::t("folder_name"), input.as_str())
                        .on_input(Message::FolderRenameInput)
                        .on_submit(Message::ConfirmRenameFolder)
                        .padding(10)
                        .width(Length::Fill)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x()),
                    Space::new().height(12),
                    dir_row(vec![
                        styled_button(crate::i18n::t("save"), Message::ConfirmRenameFolder, OryxisColors::t().accent),
                        Space::new().width(8).into(),
                        styled_button(crate::i18n::t("cancel"), Message::CancelFolderModal, OryxisColors::t().text_muted),
                    ]),
                ]
                .width(Length::Fill)
                .align_x(dir_align_x())
                .padding(24),
            )
            .width(Length::Fixed(360.0))
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog.into(),
                    Some(Message::CancelFolderModal),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Folder delete confirmation, three-way choice instead of a yes/no
        // since destroying hosts vs only the folder are very different
        // intentions and deserve explicit affordances.
        if let Some(gid) = self.folder_delete {
            let folder_name = self
                .groups
                .iter()
                .find(|g| g.id == gid)
                .map(|g| g.label.clone())
                .unwrap_or_default();
            let host_count = self
                .connections
                .iter()
                .filter(|c| c.group_id == Some(gid))
                .count();
            let dialog = container(
                column![
                    text(crate::i18n::t("delete_folder_question"))
                        .size(16)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(6),
                    text(format!(
                        "\"{}\", {}",
                        folder_name,
                        crate::i18n::host_count(host_count)
                    ))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(16),
                    styled_button(
                        crate::i18n::t("delete_folder_keep_hosts"),
                        Message::DeleteFolderKeepHosts,
                        OryxisColors::t().accent,
                    ),
                    Space::new().height(8),
                    styled_button(
                        crate::i18n::t("delete_folder_with_hosts"),
                        Message::DeleteFolderWithHosts,
                        OryxisColors::t().error,
                    ),
                    Space::new().height(8),
                    styled_button(
                        crate::i18n::t("cancel"),
                        Message::CancelFolderModal,
                        OryxisColors::t().text_muted,
                    ),
                ]
                .padding(24)
                .width(360),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog.into(),
                    Some(Message::CancelFolderModal),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // "Clear all" confirmation for the Logs view: states exactly
        // what gets wiped (recordings + connection events) before the
        // irreversible ClearLogs runs.
        if self.clear_history_confirm {
            let total = self.logs_total + self.session_logs_total;
            let dialog = container(
                column![
                    text(crate::i18n::t("clear_history_title"))
                        .size(16)
                        .color(OryxisColors::t().text_primary),
                    Space::new().height(6),
                    text(crate::i18n::t("clear_history_confirm"))
                        .size(13)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    text(format!("{} {}", total, crate::i18n::t("entries")))
                        .size(13)
                        .color(OryxisColors::t().text_muted),
                    Space::new().height(16),
                    crate::widgets::dir_row(vec![
                        styled_button(
                            crate::i18n::t("cancel"),
                            Message::CancelClearHistory,
                            OryxisColors::t().text_muted,
                        ),
                        Space::new().width(8).into(),
                        styled_button(
                            crate::i18n::t("clear_all"),
                            Message::ClearLogs,
                            OryxisColors::t().error,
                        ),
                    ])
                    .align_y(iced::Alignment::Center),
                ]
                .padding(24)
                .width(360),
            )
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border { radius: Radius::from(12.0), color: OryxisColors::t().border, width: 1.0 },
                ..Default::default()
            });

            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    dialog.into(),
                    Some(Message::CancelClearHistory),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // New-tab picker (opens via the "+" button in the tab bar).
        if self.show_new_tab_picker {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_new_tab_picker(),
                    Some(Message::HideNewTabPicker),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Tab-jump modal, Termius-style "Jump to" list. Opens via the
        // ⋯ button in the tab bar or the global Ctrl+J shortcut.
        if self.show_tab_jump {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_tab_jump_modal(),
                    Some(Message::HideTabJump),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Icon/color picker (from the host editor). Intentionally NOT routed
        // through `widgets::modal_overlay`: it injects a color-popover layer
        // into its own Stack, which the simple helper can't host. Stays
        // mouse-safe via `opaque` and keyboard-safe via `any_modal_blocks_input`.
        if self.show_icon_picker {
            let picker = self.view_icon_picker();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(picker)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Chain editor (from the host editor's "Host Chaining" row). Scrim
        // dismiss is context-dependent: pop the add-a-hop sub-view first,
        // else close the editor (mirrors Esc).
        if self.show_chain_editor {
            let on_scrim = if self.chain_editor_adding {
                Message::ChainEditorCancelAdd
            } else {
                Message::CloseChainEditor
            };
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_chain_editor(),
                    Some(on_scrim),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Per-host terminal theme picker (from the host editor).
        if self.show_theme_picker {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_terminal_theme_picker(),
                    Some(Message::EditorCloseThemePicker),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Custom terminal theme editor (from the "+" card / edit affordance
        // in Settings -> Terminal). Exempt from `modal_overlay` (nested color
        // popover in its own Stack); mouse-safe via `opaque`, keyboard-safe
        // via `any_modal_blocks_input`.
        if self.theme_editor.is_some() {
            let editor = self.view_theme_editor_modal();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(editor)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Import-a-scheme modal (Settings -> Terminal "Import" card).
        if self.show_theme_import {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_theme_import_modal(),
                    Some(Message::ThemeImportClose),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Custom UI (chrome) theme editor (Settings -> Interface). Exempt
        // from `modal_overlay` (nested color popover in its own Stack);
        // mouse-safe via `opaque`, keyboard-safe via `any_modal_blocks_input`.
        if self.ui_theme_editor.is_some() {
            let editor = self.view_ui_theme_editor_modal();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(editor)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Note: the update modal is rendered at the top-level `view()`
        // dispatcher (see `Oryxis::view`) so it overlays the lock screen
        // too. Don't re-render it here.

        if let Some(ref overlay) = self.overlay {
            let menu = self.render_overlay_menu(overlay);

            // The `+` split popover is hover-driven: it opens on hover and
            // dismisses on mouse-out (`SplitMenuLeave`), so a click-dismiss
            // backdrop is redundant for it. Worse, a full-screen backdrop sits
            // on top of the `+` button and swallows the click, so the first
            // click on `+` only closes the popover and a second is needed to
            // open a new tab. Skip the backdrop here so the click reaches the
            // button. Every other overlay through this path is click-triggered
            // and keeps its click-outside dismissal.
            let is_hover_popover = matches!(overlay.content, OverlayContent::SplitMenu);

            // Position the menu, clamping to window bounds to prevent clipping.
            // Under RTL, anchor by the menu's right edge so it grows toward
            // the leading (left) side, mirroring native OS dropdown behavior.
            // Width must match the value used in `render_overlay_menu` so
            // clamping lines up with the rendered box.
            let menu_width = self.overlay_menu_width(overlay);
            let menu_height = 80.0_f32; // approximate menu height
            let raw_x = if crate::i18n::is_rtl_layout() {
                overlay.x - menu_width
            } else {
                overlay.x
            };
            let x = raw_x.min(self.window_size.width - menu_width).max(0.0);
            let y = overlay.y.min(self.window_size.height - menu_height).max(0.0);
            let positioned_menu: Element<'_, Message> = column![
                Space::new().height(y),
                row![
                    Space::new().width(x),
                    menu,
                ],
            ]
            .into();

            let mut stack = Stack::new().push(base);
            if !is_hover_popover {
                // Transparent backdrop that dismisses the menu on click.
                let backdrop: Element<'_, Message> = MouseArea::new(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .on_press(Message::HideOverlayMenu)
                .into();
                stack = stack.push(backdrop);
            }
            return wrap_with_resize(
                stack
                    .push(positioned_menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // SFTP row right-click menu, rendered at the layout root so the
        // window-coord click position lines up with the menu origin
        // without having to compensate for the title + tab bar height.
        if let Some(ref row_menu) = self.sftp.row_menu {
            // "Cross-pane action available" = the pane opposite the
            // right-clicked row is connected (remote with a client) or is
            // a local destination. The row menu uses this to decide
            // whether to offer Upload / Download / Relay.
            let other_side = if row_menu.side == crate::state::SftpPaneSide::Left {
                crate::state::SftpPaneSide::Right
            } else {
                crate::state::SftpPaneSide::Left
            };
            let other = self.sftp.pane(other_side);
            let cross_pane_ready = if other.is_remote {
                other.client.is_some()
            } else {
                true
            };
            let other_is_remote = other.is_remote;
            let src_pane = self.sftp.pane(row_menu.side);
            let source_is_remote = src_pane.is_remote;
            let other_label = other.host_label.clone();
            // Current directory of the source pane + its local path, fed to
            // the directory-level actions (Refresh / New / Open in FM).
            let pane_dir = if source_is_remote {
                src_pane.remote_path.clone()
            } else {
                src_pane.local_path.to_string_lossy().into_owned()
            };
            let local_dir = src_pane.local_path.clone();
            let show_hidden = src_pane.show_hidden;
            // Count of selected rows in the same pane as the right-
            // clicked row, drives the bulk vs single menu mode.
            let selection_count_same_pane = self
                .sftp
                .selected_rows
                .iter()
                .filter(|(s, _)| *s == row_menu.side)
                .count();
            let menu = crate::views::sftp::row_context_menu_box(
                row_menu,
                cross_pane_ready,
                source_is_remote,
                other_is_remote,
                other_label,
                selection_count_same_pane,
                crate::views::sftp::DirActionCtx {
                    pane_dir: &pane_dir,
                    local_dir: &local_dir,
                    show_hidden,
                },
            );
            let backdrop: Element<'_, Message> = MouseArea::new(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::SftpRowMenuClose)
            .into();
            // Nudge the menu a few px down/right so it doesn't sit
            // directly under the cursor, feels like the OS-native menu
            // anchoring.
            let menu_width = crate::views::sftp::ROW_CONTEXT_MENU_WIDTH;
            let rtl = crate::i18n::is_rtl_layout();
            // Under RTL, nudge toward the leading side so the menu grows
            // left-from-cursor instead of right-from-cursor.
            let nudged_x = if rtl {
                row_menu.x - 2.0 - menu_width
            } else {
                row_menu.x + 2.0
            };
            let nudged_y = row_menu.y + 2.0;
            let menu_height = crate::views::sftp::row_context_menu_height(
                row_menu,
                cross_pane_ready,
                source_is_remote,
                other_is_remote,
                selection_count_same_pane,
            );
            let x = nudged_x
                .min(self.window_size.width - menu_width)
                .max(0.0);
            let y = nudged_y
                .min(self.window_size.height - menu_height)
                .max(0.0);
            let positioned_menu: Element<'_, Message> = column![
                Space::new().height(y),
                row![Space::new().width(x), menu],
            ]
            .into();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(backdrop)
                    .push(positioned_menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // Floating drag ghost, rendered last so it sits above
        // everything else. Tracks the cursor while a cross-pane SFTP
        // drag is in flight; non-interactive so it doesn't swallow the
        // release event that ends the drag.
        if let Some(drag) = &self.sftp.drag
            && drag.active
        {
            let ghost = crate::views::sftp::drag_ghost(&drag.label);
            // Offset slightly off the cursor, matches OS drag previews
            // and keeps the label out from under the pointer. Direction
            // mirrors under RTL so the ghost trails the cursor on the
            // leading side instead of running off-screen at the edge.
            let ghost_width = 200.0_f32;
            let x_offset = if crate::i18n::is_rtl_layout() {
                -ghost_width - 12.0
            } else {
                12.0
            };
            let x = (self.mouse_position.x + x_offset)
                .min(self.window_size.width - ghost_width)
                .max(0.0);
            let y = (self.mouse_position.y + 12.0)
                .min(self.window_size.height - 40.0)
                .max(0.0);
            let positioned: Element<'_, Message> = column![
                Space::new().height(y),
                row![Space::new().width(x), ghost],
            ]
            .into();
            return wrap_with_resize(
                Stack::new()
                    .push(base)
                    .push(positioned)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                resize_overlay,
            );
        }

        // No modal open. Wrap `base` in a single-child Stack so it sits
        // at exactly the same tree position as in the modal branches
        // above (every one of which passes `Stack::new().push(base)
        // .push(modal)` as the content). iced keys scrollable offsets by
        // tree position, not by Id, so if `base`'s depth shifted when a
        // modal opened (bare `base` here vs. nested under a Stack there)
        // every scrollable inside it (host list, editor form, ...) would
        // reset to the top. Keeping the depth constant preserves them.
        wrap_with_resize(
            Stack::new()
                .push(base)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            resize_overlay,
        )
    }

    /// Browser-style immersive-mode overlays: on-enter hint banner and
    /// hover-only round X close button. Stacked on top of whatever the
    /// caller passed so they never get hidden by content underneath.
    /// The X only renders when the mouse sits in the top 60 px so the
    /// affordance is discoverable but unobtrusive once the user gets
    /// used to F11.
    pub(crate) fn layer_fullscreen_overlays<'a>(
        &'a self,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        const TOP_HOVER_ZONE: f32 = 60.0;
        const HINT_BANNER_HEIGHT: f32 = 32.0;
        let in_top_zone = self.mouse_position.y < TOP_HOVER_ZONE;

        let mut layers = Stack::new()
            .push(content)
            .width(Length::Fill)
            .height(Length::Fill);

        if self.fullscreen_hint_visible {
            let hint = container(
                text(crate::i18n::t("fullscreen_exit_hint"))
                    .size(12)
                    .color(OryxisColors::t().text_primary),
            )
            .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.92,
                    ..OryxisColors::t().bg_selected
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
            let centered = column![
                Space::new().height(12.0),
                container(hint).center_x(Length::Fill),
                Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill);
            layers = layers.push(centered);
        }

        if in_top_zone {
            // Round 28×28 button with the lucide `x` glyph centered.
            // Clicking toggles fullscreen off (same Message as F11).
            // Anchored top-center with a small top inset; when the
            // hint banner is also visible the button sits below it
            // so the two affordances don't overlap.
            let close_btn = button(
                container(
                    iced_fonts::lucide::x::<iced::Theme, iced::Renderer>()
                        .size(14)
                        .color(OryxisColors::t().button_text),
                )
                .center(Length::Fixed(28.0)),
            )
            .on_press(Message::WindowFullscreenToggle)
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => OryxisColors::t().error,
                    _ => Color {
                        a: 0.85,
                        ..OryxisColors::t().bg_selected
                    },
                };
                iced::widget::button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: Radius::from(14.0),
                        color: OryxisColors::t().border,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            });
            let top_offset = if self.fullscreen_hint_visible {
                12.0 + HINT_BANNER_HEIGHT + 8.0
            } else {
                12.0
            };
            let positioned = column![
                Space::new().height(top_offset),
                container(close_btn).center_x(Length::Fill),
                Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill);
            layers = layers.push(positioned);
        }

        layers.into()
    }

    /// Resolve the on-screen width of an overlay popover. Group
    /// pickers track their associated combo's measured bounds (so
    /// the popover stays the same width as the input it dropdowns
    /// from). Sort menus get a wider fixed slot so long-translated
    /// labels fit. Everything else uses the default kebab width.
    /// Falls back to the kebab width when a combo's bounds cell
    /// hasn't been populated yet (extremely brief, before the first
    /// draw pass on a freshly opened panel).
    fn overlay_menu_width(&self, overlay: &OverlayState) -> f32 {
        match &overlay.content {
            OverlayContent::SortMenu(_) => 220.0,
            // Wide enough for "Split side by side" / "Duplicate in New
            // Window" / "Close Other Tabs" to sit on one line.
            OverlayContent::SplitMenu | OverlayContent::TabActions(_) => 210.0,
            OverlayContent::CloudDiscoverGroupPicker => {
                let b = self.cloud_discover_default_group_combo_bounds.get();
                if b.width > 0.0 { b.width } else { 308.0 }
            }
            OverlayContent::GroupPicker(target) => {
                let b = match target {
                    crate::state::GroupPickerTarget::DynamicFormParent => {
                        self.dynamic_form_parent_combo_bounds.get()
                    }
                    crate::state::GroupPickerTarget::SessionGroupFolder => {
                        self.session_group_folder_combo_bounds.get()
                    }
                };
                if b.width > 0.0 { b.width } else { 308.0 }
            }
            _ => 150.0,
        }
    }

    pub(crate) fn render_overlay_menu(&self, overlay: &OverlayState) -> Element<'_, Message> {
        // Per-variant width. Group pickers track the live combo width
        // measured by their `bounds_reporter` so the popover always
        // matches the input it dropdowns from; sort menu gets a wider
        // fixed slot so long translations fit; everything else falls
        // back to the default kebab width.
        let menu_width = self.overlay_menu_width(overlay);
        let items: Element<'_, Message> = match &overlay.content {
            OverlayContent::HostActions(idx) => {
                let idx = *idx;
                let conn = self.connections.get(idx);
                let cloud_profile_id = conn
                    .and_then(|c| c.cloud_ref.as_ref())
                    .map(|r| r.profile_id);
                let is_orphan = conn
                    .and_then(|c| c.cloud_ref.as_ref())
                    .and_then(|r| r.orphaned_at)
                    .is_some();
                let mut items = column![
                    context_menu_item(iced_fonts::lucide::play(), crate::i18n::t("connect"), Message::ConnectSsh(idx), OryxisColors::t().success),
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate"), Message::DuplicateConnection(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::share(), crate::i18n::t("share"), Message::ShareConnection(idx), OryxisColors::t().text_secondary),
                ];
                if let Some(pid) = cloud_profile_id {
                    items = items.push(context_menu_item(
                        iced_fonts::lucide::funnel(),
                        crate::i18n::t("host_filter_by_profile"),
                        Message::HostFilterByCloudProfile(Some(pid)),
                        OryxisColors::t().text_secondary,
                    ));
                }
                // Orphan hosts get a "Forget" label (semantically
                // closer to "this resource is gone upstream, drop my
                // local record") instead of the generic "Remove".
                // Same `DeleteConnection` action under the hood.
                let (remove_label, remove_icon) = if is_orphan {
                    (crate::i18n::t("host_orphan_forget"), iced_fonts::lucide::eraser())
                } else {
                    (crate::i18n::t("remove"), iced_fonts::lucide::trash())
                };
                items
                    .push(context_menu_item(remove_icon, remove_label, Message::DeleteConnection(idx), OryxisColors::t().error))
                    .into()
            }
            OverlayContent::SessionGroupActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::play(), crate::i18n::t("open_session_group"), Message::OpenSessionGroup(idx), OryxisColors::t().success),
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditSessionGroup(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate"), Message::DuplicateSessionGroup(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::DeleteSessionGroup(idx), OryxisColors::t().error),
                ]
                .into()
            }
            OverlayContent::KeyActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditKey(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::DeleteKey(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::IdentityActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditIdentity(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("remove"), Message::DeleteIdentity(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::SnippetActions(idx) => {
                let idx = *idx;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditSnippet(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::DeleteSnippet(idx), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::KeychainAdd => {
                column![
                    context_menu_item(iced_fonts::lucide::key_round(), crate::i18n::t("import_key"), Message::ShowKeyPanel, OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::user(), crate::i18n::t("new_identity"), Message::ShowIdentityPanel, OryxisColors::t().text_secondary),
                ].into()
            }
            OverlayContent::FolderActions(gid) => {
                let gid = *gid;
                // Folders that hold cloud-imported hosts used to hide
                // their rename / delete actions to protect the
                // import-by-label dedupe. The decoupling work in v0.7
                // moved import targets to an explicit picker, so
                // renaming or moving the auto folder no longer breaks
                // anything (worst case the next Auto import creates a
                // sibling). Surface the standard actions instead.
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditGroup(gid), OryxisColors::t().accent),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::StartDeleteFolder(gid), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::DynamicGroupActions(id) => {
                let id = *id;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::EditDynamicGroup(id), OryxisColors::t().accent),
                    // Rename = friendly display label only. The
                    // cloud_query (cluster/service/container) and the
                    // import-dedupe key never look at it, so renaming
                    // is safe and the subtitle keeps surfacing the
                    // original ECS path.
                    context_menu_item(iced_fonts::lucide::text_cursor_input(), crate::i18n::t("rename"), Message::StartRenameFolder(id), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::DeleteDynamicGroup(id), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::CloudProfileActions(id) => {
                let id = *id;
                column![
                    context_menu_item(iced_fonts::lucide::pencil(), crate::i18n::t("edit"), Message::ShowCloudForm(Some(id)), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::refresh_cw(), crate::i18n::t("cloud_profile_sync"), Message::CloudProfileSync(id), OryxisColors::t().accent),
                    context_menu_item(iced_fonts::lucide::trash(), crate::i18n::t("delete"), Message::DeleteCloudProfile(id), OryxisColors::t().error),
                ].into()
            }
            OverlayContent::CloudProviderPicker => {
                // The "+ Host ▾" add menu. Always offers importing a
                // `.oryxis` file (a full vault export or a single
                // shared host), then one entry per configured cloud
                // profile for discovery. Import lives here so it's
                // reachable from where hosts are added instead of being
                // buried in Settings.
                let mut items = column![context_menu_item(
                    iced_fonts::lucide::download(),
                    crate::i18n::t("import_from_file"),
                    Message::ImportVault,
                    OryxisColors::t().text_secondary,
                )];
                // Only profiles whose provider plugin is installed can
                // run discovery; hide the rest (they'd fail with a
                // "binary not found" wall) until the plugin is back.
                for cp in self
                    .cloud_profiles
                    .iter()
                    .filter(|p| self.cloud_provider_installed(&p.provider))
                {
                    let (glyph, brand) = crate::os_icon::provider_icon(
                        &cp.provider,
                        OryxisColors::t().accent,
                    );
                    items = items.push(context_menu_item(
                        glyph,
                        cp.label.as_str(),
                        Message::ShowCloudDiscover(cp.id),
                        brand,
                    ));
                }
                items.into()
            }
            OverlayContent::TabActions(idx) => {
                let idx = *idx;
                let mut items = column![
                    context_menu_item(iced_fonts::lucide::columns_two(), crate::i18n::t("split_side_by_side"), Message::SplitTabPane(idx, iced::widget::pane_grid::Axis::Vertical), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::rows_two(), crate::i18n::t("split_stacked"), Message::SplitTabPane(idx, iced::widget::pane_grid::Axis::Horizontal), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::copy(), crate::i18n::t("duplicate_tab"), Message::DuplicateTab(idx), OryxisColors::t().text_secondary),
                ];
                // Save the whole arrangement (panes + splits + per-pane
                // scripts) as a reusable session group, or edit it if this
                // tab already came from one. Only meaningful for a split tab
                // (>1 pane); a single-pane tab is just a host, not a group.
                // Already-saved groups keep the "Edit" entry so they stay
                // editable even if pruned down to one pane.
                let tab_ref = self.tabs.get(idx);
                let is_group = tab_ref.map(|t| t.session_group_id.is_some()).unwrap_or(false);
                let is_split = tab_ref.map(|t| t.pane_count() > 1).unwrap_or(false);
                if is_split || is_group {
                    let sg_label = if is_group {
                        crate::i18n::t("edit_session_group")
                    } else {
                        crate::i18n::t("save_session_group")
                    };
                    items = items.push(context_menu_item(iced_fonts::lucide::boxes(), sg_label, Message::ShowSaveSessionGroup(idx), OryxisColors::t().text_secondary));
                }
                // Pin / unpin: pinned tabs render first and restore on launch.
                // The restore spec captures only a single pane's origin, so
                // pinning is offered only on single-pane, non-group tabs (a
                // split / session-group tab would silently restore just its
                // focused pane). An already-pinned tab always shows "unpin".
                let is_pinned = tab_ref.map(|t| t.pinned).unwrap_or(false);
                if is_pinned || (!is_split && !is_group) {
                    let (pin_icon, pin_label) = if is_pinned {
                        (iced_fonts::lucide::pin_off(), crate::i18n::t("unpin_tab"))
                    } else {
                        (iced_fonts::lucide::pin(), crate::i18n::t("pin_tab"))
                    };
                    items = items.push(context_menu_item(pin_icon, pin_label, Message::ToggleTabPin(idx), OryxisColors::t().text_secondary));
                }
                // "Duplicate in New Window" spawns a fresh process that
                // can only re-open hosts saved in the vault. ECS Exec /
                // kubectl tabs are ephemeral dynamic-group sessions (no
                // saved connection, no uuid to hand the child), flagged
                // by a `relaunch` message, so hide the item there rather
                // than open an empty window.
                let new_window_ok = self
                    .tabs
                    .get(idx)
                    .map(|t| t.relaunch.is_none())
                    .unwrap_or(true);
                if new_window_ok {
                    items = items.push(context_menu_item(iced_fonts::lucide::external_link(), crate::i18n::t("duplicate_new_window"), Message::DuplicateInNewWindow(idx), OryxisColors::t().text_secondary));
                }
                items = items.push(context_menu_item(iced_fonts::lucide::rotate_cw(), crate::i18n::t("reconnect"), Message::ReconnectTab(idx), OryxisColors::t().accent));
                items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_tab"), Message::CloseTab(idx), OryxisColors::t().text_secondary));
                items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_other_tabs"), Message::CloseOtherTabs(idx), OryxisColors::t().text_secondary));
                items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_all_tabs"), Message::CloseAllTabs, OryxisColors::t().error));
                items.into()
            }
            OverlayContent::SftpTabActions(idx) => {
                let idx = *idx;
                let is_pinned = self.sftp_tabs.get(idx).map(|t| t.pinned).unwrap_or(false);
                let (pin_icon, pin_label) = if is_pinned {
                    (iced_fonts::lucide::pin_off(), crate::i18n::t("unpin_tab"))
                } else {
                    (iced_fonts::lucide::pin(), crate::i18n::t("pin_tab"))
                };
                let mut items = column![
                    context_menu_item(iced_fonts::lucide::plus(), crate::i18n::t("new_tab"), Message::NewSftpTab, OryxisColors::t().text_secondary),
                    context_menu_item(pin_icon, pin_label, Message::ToggleSftpTabPin(idx), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_tab"), Message::CloseSftpTab(idx), OryxisColors::t().text_secondary),
                ];
                if self.sftp_tabs.len() > 1 {
                    items = items.push(context_menu_item(iced_fonts::lucide::x(), crate::i18n::t("close_other_tabs"), Message::CloseOtherSftpTabs(idx), OryxisColors::t().text_secondary));
                }
                items.into()
            }
            OverlayContent::SplitMenu => {
                let items = column![
                    context_menu_item(iced_fonts::lucide::plus(), crate::i18n::t("new_tab"), Message::ShowNewTabPicker, OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::columns_two(), crate::i18n::t("split_side_by_side"), Message::SplitPane(iced::widget::pane_grid::Axis::Vertical), OryxisColors::t().text_secondary),
                    context_menu_item(iced_fonts::lucide::rows_two(), crate::i18n::t("split_stacked"), Message::SplitPane(iced::widget::pane_grid::Axis::Horizontal), OryxisColors::t().text_secondary),
                ];
                // Keep the popover open while the cursor is over it (hover
                // bridge from the `+` button into the menu).
                MouseArea::new(items)
                    .on_enter(Message::SplitMenuEnter)
                    .on_exit(Message::SplitMenuLeave)
                    .into()
            }
            OverlayContent::SortMenu(kind) => {
                let kind = *kind;
                let current = match kind {
                    crate::state::SortMenuKind::Hosts => self.hosts_sort,
                    crate::state::SortMenuKind::Keys => self.keys_sort,
                    crate::state::SortMenuKind::Snippets => self.snippets_sort,
                };
                use crate::state::ListSort;
                // Each row: leading lucide icon, label, trailing
                // checkmark when the row matches the active sort.
                // Inlined as four explicit calls so the icon widget's
                // lifetime stays 'static (a closure would force the
                // icon to outlive the returned Element borrow).
                // Hairline divider: the colored fill must sit on the
                // inner 1 px Space, not the outer padded container,
                // otherwise the breathing-room padding inherits the
                // border colour and the line reads ~9 px tall.
                let divider: Element<'_, Message> = container(
                    container(Space::new().width(Length::Fill).height(1))
                        .width(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Background::Color(
                                OryxisColors::t().border,
                            )),
                            ..Default::default()
                        }),
                )
                .padding(Padding {
                    top: 4.0,
                    right: 4.0,
                    bottom: 4.0,
                    left: 4.0,
                })
                .into();
                column![
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::LabelAsc,
                        iced_fonts::lucide::arrow_down_a_z::<iced::Theme, iced::Renderer>(),
                        "sort_label_asc",
                        current == ListSort::LabelAsc,
                    ),
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::LabelDesc,
                        iced_fonts::lucide::arrow_down_z_a::<iced::Theme, iced::Renderer>(),
                        "sort_label_desc",
                        current == ListSort::LabelDesc,
                    ),
                    divider,
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::NewestFirst,
                        iced_fonts::lucide::calendar_arrow_down::<iced::Theme, iced::Renderer>(),
                        "sort_newest_first",
                        current == ListSort::NewestFirst,
                    ),
                    crate::widgets::sort_menu_row(
                        kind,
                        ListSort::OldestFirst,
                        iced_fonts::lucide::calendar_arrow_up::<iced::Theme, iced::Renderer>(),
                        "sort_oldest_first",
                        current == ListSort::OldestFirst,
                    ),
                ]
                .into()
            }
            OverlayContent::CloudDiscoverGroupPicker => {
                // Search input + filtered list. The search field is
                // the menu's own filter (independent of the modal's
                // "Import into" input). Picking a row fills the
                // input and closes the menu.
                let picker_needle = self
                    .cloud_discover_default_group_picker_search
                    .trim()
                    .to_lowercase();
                let mut all_groups: Vec<String> = self
                    .groups
                    .iter()
                    .filter(|g| g.cloud_query.is_none())
                    .filter(|g| {
                        picker_needle.is_empty()
                            || g.label.to_lowercase().contains(&picker_needle)
                    })
                    .map(|g| g.label.clone())
                    .collect();
                all_groups.sort_by_key(|s| s.to_lowercase());
                all_groups.dedup();
                // Width chases the combo bounds via the outer
                // wrapper in `view_main` + `overlay_menu_width`; the
                // inner content fills whatever space that outer
                // container hands down. Padding 4+4 on the outer
                // wrapper means content fills (combo_width - 8).
                let menu_outer_width = self.overlay_menu_width(overlay);
                let menu_content_width = (menu_outer_width - 8.0).max(80.0);
                // Search input uses a distinct surface tint so the
                // user reads it as the popover's own filter (not a
                // second copy of the modal's "Import into" field).
                // Mirrors what most context-menus do with their
                // header row: tinted bg + tighter border than the
                // form inputs underneath.
                let search_input = iced::widget::text_input(
                    crate::i18n::t("search_groups"),
                    &self.cloud_discover_default_group_picker_search,
                )
                .on_input(
                    Message::CloudDiscoverDefaultGroupPickerSearchChanged,
                )
                .padding(8)
                .width(Length::Fixed(menu_content_width))
                .style(|_theme: &iced::Theme, status| {
                    let palette = OryxisColors::t();
                    let bg = match status {
                        iced::widget::text_input::Status::Focused { .. }
                        | iced::widget::text_input::Status::Hovered => palette.bg_hover,
                        _ => palette.bg_selected,
                    };
                    let border_color = match status {
                        iced::widget::text_input::Status::Focused { .. } => palette.accent,
                        _ => palette.border,
                    };
                    iced::widget::text_input::Style {
                        background: Background::Color(bg),
                        border: Border {
                            radius: Radius::from(6.0),
                            color: border_color,
                            width: 1.0,
                        },
                        icon: palette.text_muted,
                        placeholder: palette.text_muted,
                        value: palette.text_primary,
                        selection: Color { a: 0.30, ..palette.accent },
                    }
                });
                let list_el: Element<'_, Message> = if all_groups.is_empty() {
                    container(
                        text(crate::i18n::t("cloud_discover_no_matches"))
                            .size(12)
                            .color(OryxisColors::t().text_muted),
                    )
                    .padding(Padding {
                        top: 12.0,
                        right: 12.0,
                        bottom: 12.0,
                        left: 12.0,
                    })
                    .into()
                } else {
                    // Plain label rows: dropped the leading folder
                    // glyph since every entry is a folder by
                    // definition (the picker only lists groups) and
                    // the icon was just visual noise.
                    let mut items = column![].spacing(2);
                    for label in all_groups {
                        let display = label.clone();
                        items = items.push(
                            iced::widget::button(
                                container(
                                    text(display)
                                        .size(12)
                                        .color(OryxisColors::t().text_primary),
                                )
                                .padding(Padding {
                                    top: 6.0,
                                    right: 10.0,
                                    bottom: 6.0,
                                    left: 10.0,
                                })
                                .width(Length::Fill),
                            )
                            .on_press(
                                Message::CloudDiscoverDefaultGroupPick(label),
                            )
                            .width(Length::Fill)
                            .style(|_, status| {
                                let bg = match status {
                                    iced::widget::button::Status::Hovered => {
                                        OryxisColors::t().bg_hover
                                    }
                                    _ => Color::TRANSPARENT,
                                };
                                iced::widget::button::Style {
                                    background: Some(Background::Color(bg)),
                                    border: Border {
                                        radius: Radius::from(4.0),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }
                            }),
                        );
                    }
                    iced::widget::scrollable(items)
                        .height(Length::Fixed(220.0))
                        .into()
                };
                column![search_input, Space::new().height(8), list_el]
                    .width(Length::Fixed(menu_content_width))
                    .into()
            }
            OverlayContent::GroupPicker(target) => {
                // Same shape as the Discover modal's group picker
                // (search input + filtered scrollable list) but
                // wired to the shared `group_picker_search` /
                // `GroupPickerPick(target)` messages. Lives at the
                // top-level render path because the side-panel
                // editors don't short-circuit the way the modal
                // does.
                let target = *target;
                let menu_outer_width = self.overlay_menu_width(overlay);
                let menu_content_width = (menu_outer_width - 8.0).max(80.0);
                let needle = self.group_picker_search.trim().to_lowercase();
                let mut all_groups: Vec<String> = self
                    .groups
                    .iter()
                    .filter(|g| g.cloud_query.is_none())
                    .filter(|g| {
                        needle.is_empty()
                            || g.label.to_lowercase().contains(&needle)
                    })
                    .map(|g| g.label.clone())
                    .collect();
                all_groups.sort_by_key(|s| s.to_lowercase());
                all_groups.dedup();
                let search_input = iced::widget::text_input(
                    crate::i18n::t("search_groups"),
                    &self.group_picker_search,
                )
                .on_input(Message::GroupPickerSearchChanged)
                .padding(8)
                .width(Length::Fixed(menu_content_width))
                .style(|_theme: &iced::Theme, status| {
                    let palette = OryxisColors::t();
                    let bg = match status {
                        iced::widget::text_input::Status::Focused { .. }
                        | iced::widget::text_input::Status::Hovered => palette.bg_hover,
                        _ => palette.bg_selected,
                    };
                    let border_color = match status {
                        iced::widget::text_input::Status::Focused { .. } => palette.accent,
                        _ => palette.border,
                    };
                    iced::widget::text_input::Style {
                        background: Background::Color(bg),
                        border: Border {
                            radius: Radius::from(6.0),
                            color: border_color,
                            width: 1.0,
                        },
                        icon: palette.text_muted,
                        placeholder: palette.text_muted,
                        value: palette.text_primary,
                        selection: Color { a: 0.30, ..palette.accent },
                    }
                });
                let list_el: Element<'_, Message> = if all_groups.is_empty() {
                    container(
                        text(crate::i18n::t("cloud_discover_no_matches"))
                            .size(12)
                            .color(OryxisColors::t().text_muted),
                    )
                    .padding(Padding {
                        top: 12.0,
                        right: 12.0,
                        bottom: 12.0,
                        left: 12.0,
                    })
                    .into()
                } else {
                    let mut items = column![].spacing(2);
                    for label in all_groups {
                        let display = label.clone();
                        items = items.push(
                            iced::widget::button(
                                container(
                                    text(display)
                                        .size(12)
                                        .color(OryxisColors::t().text_primary),
                                )
                                .padding(Padding {
                                    top: 6.0,
                                    right: 10.0,
                                    bottom: 6.0,
                                    left: 10.0,
                                })
                                .width(Length::Fill),
                            )
                            .on_press(Message::GroupPickerPick(target, label))
                            .width(Length::Fill)
                            .style(|_, status| {
                                let bg = match status {
                                    iced::widget::button::Status::Hovered => {
                                        OryxisColors::t().bg_hover
                                    }
                                    _ => Color::TRANSPARENT,
                                };
                                iced::widget::button::Style {
                                    background: Some(Background::Color(bg)),
                                    border: Border {
                                        radius: Radius::from(4.0),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }
                            }),
                        );
                    }
                    iced::widget::scrollable(items)
                        .height(Length::Fixed(220.0))
                        .into()
                };
                column![search_input, Space::new().height(8), list_el]
                    .width(Length::Fixed(menu_content_width))
                    .into()
            }
        };

        // Min-height (so a single-item menu reads as a real button-
        // height drop-down, not a sliver). Iced 0.13 has no
        // `min_height`, the previous Stack-based workaround
        // collapsed items to zero width in this fork, and stuffing a
        // fixed-height Space inside the column inflates multi-item
        // menus by the spacer height. Compromise: render items in
        // an outer container with a tight vertical padding that
        // approximates the spilt-button height for small menus
        // while letting tall menus grow naturally.
        const SINGLE_ROW_MIN_PAD: f32 = 6.0;
        container(items)
            .width(menu_width)
            .padding(Padding {
                top: SINGLE_ROW_MIN_PAD,
                right: 4.0,
                bottom: SINGLE_ROW_MIN_PAD,
                left: 4.0,
            })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
                ..Default::default()
            })
            .into()
    }
    pub(crate) fn view_content(&self) -> Element<'_, Message> {
        // If a terminal tab is active, show terminal
        // Otherwise show the grid view for the current nav item
        let content: Element<'_, Message> = if self.connecting.is_some() && self.active_tab.is_some() {
            self.view_connection_progress()
        } else if self.active_tab.is_some() && self.connecting.is_none() {
            self.view_terminal()
        } else {
            match self.active_view {
                View::Dashboard => self.view_dashboard(),
                View::Keys => self.view_keys(),
                View::Snippets => self.view_snippets(),
                View::PortForwarding => self.view_port_forwards(),
                View::Cloud => self.view_cloud_accounts(),
                View::Proxies => self.view_settings_proxies(),
                View::KnownHosts => self.view_known_hosts(),
                View::History => self.view_history(),
                View::Sftp => self.view_sftp(),
                View::Settings => self.view_settings(),
                View::Terminal => self.view_terminal(),
            }
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            })
            .into()
    }

    /// Search field for the active vault sub-view, filling its toolbar
    /// slot. Returns an empty widget for views without a search backing
    /// (Cloud / Proxies / Known Hosts). The id matches
    /// `active_view_search_id` so the global Ctrl+F handler can focus it.
    /// True when the active view has no records at all (so there's
    /// nothing to search). Distinct from "search matched nothing": this
    /// is about the underlying data set being empty, which is when we
    /// hide the search box entirely and let the empty state speak.
    fn active_view_search_empty(&self) -> bool {
        match self.active_view {
            View::Dashboard => {
                self.connections.is_empty()
                    && self.groups.is_empty()
                    && self.session_groups.is_empty()
            }
            View::Keys => self.keys.is_empty() && self.identities.is_empty(),
            View::Snippets => self.snippets.is_empty(),
            View::PortForwarding => self.port_forward_rules.is_empty(),
            View::History => self.logs.is_empty() && self.session_logs.is_empty(),
            View::Cloud => self.cloud_profiles.is_empty(),
            View::Proxies => self.proxy_identities.is_empty(),
            _ => true,
        }
    }

    pub(crate) fn vault_search_field(&self) -> Element<'_, Message> {
        // Nothing to search → no search box (the empty state covers it).
        // Return a Fill spacer (not zero-width) so the toolbar slot keeps
        // stretching and the action cluster stays pinned to the trailing
        // edge, exactly as it does when the field is present.
        if self.active_view_search_empty() {
            return Space::new().width(Length::Fill).height(0).into();
        }
        // `ph_key` / `id` are static; only `value` borrows from `self`,
        // so they're kept in separate bindings (a shared tuple lifetime
        // would force the static `id` down to `self`'s lifetime, and
        // `Id::new` needs `'static`).
        let (ph_key, value, on_input): (&'static str, &str, fn(String) -> Message) =
            match self.active_view {
                View::Dashboard => (
                    "search_hosts",
                    self.host_search.as_str(),
                    Message::HostSearchChanged,
                ),
                View::Keys => (
                    "search_keys_identities",
                    self.key_search.as_str(),
                    Message::KeySearchChanged,
                ),
                View::Snippets => (
                    "search_snippets",
                    self.snippet_search.as_str(),
                    Message::SnippetSearchChanged,
                ),
                View::PortForwarding => (
                    "search_port_forwards",
                    self.port_forward_search.as_str(),
                    Message::PortForwardSearchChanged,
                ),
                View::History => (
                    "search_logs",
                    self.history_search.as_str(),
                    Message::HistorySearchChanged,
                ),
                View::Cloud => (
                    "search_cloud_accounts",
                    self.cloud_search.as_str(),
                    Message::CloudSearchChanged,
                ),
                View::Proxies => (
                    "search_proxies",
                    self.proxy_search.as_str(),
                    Message::ProxySearchChanged,
                ),
                _ => return Space::new().width(0).height(0).into(),
            };
        let id: &'static str = match self.active_view {
            View::Dashboard => "search-dashboard",
            View::Keys => "search-keys",
            View::Snippets => "search-snippets",
            View::PortForwarding => "search-port-forwards",
            View::History => "search-history",
            View::Cloud => "search-cloud",
            View::Proxies => "search-proxies",
            _ => "search-vault-subnav",
        };
        // Vertical padding tuned so the field's height matches the
        // toolbar action buttons beside it (24px content + 5px default
        // button padding top/bottom = 34px).
        iced::widget::text_input(crate::i18n::t(ph_key), value)
            .id(iced::widget::Id::new(id))
            .on_input(on_input)
            .padding(Padding { top: 9.0, right: 12.0, bottom: 9.0, left: 12.0 })
            .size(13)
            .width(Length::Fill)
            .style(crate::widgets::rounded_input_style)
            .align_x(dir_align_x())
            .into()
    }

    /// Workspace mode contextual sub-nav: horizontal pill row with
    /// the vault sub-sections. Search now lives in each view's own
    /// toolbar (see `vault_search_field`); this row is just the vault
    /// chip, the section pills, the "…" overflow and the settings gear.
    pub(crate) fn view_vault_sub_nav(&self) -> Element<'_, Message> {
        let pill = |label_key: &'static str, view: View| -> Element<'_, Message> {
            let is_active = self.active_view == view;
            let fg = if is_active {
                OryxisColors::t().accent
            } else {
                OryxisColors::t().text_muted
            };
            let bg = if is_active {
                Color { a: 0.15, ..OryxisColors::t().accent }
            } else {
                Color::TRANSPARENT
            };
            // Fixed-height inner container (28px) so every sub-nav
            // control lines up; button padding zeroed so its default
            // doesn't stack on top.
            button(
                container(
                    text(crate::i18n::t(label_key))
                        .size(12)
                        .color(fg),
                )
                .center_y(Length::Fixed(28.0))
                .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }),
            )
            .padding(0)
            .on_press(Message::ChangeView(view))
            .style(move |_, status| {
                let hover_bg = match status {
                    iced::widget::button::Status::Hovered if !is_active => {
                        Color { a: 0.08, ..OryxisColors::t().text_secondary }
                    }
                    _ => bg,
                };
                button::Style {
                    background: Some(Background::Color(hover_bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        };
        // Priority+ overflow: the pills that fit render inline; the rest
        // live behind the "…" menu (see `subnav_pill_split`). No
        // horizontal scroll, the menu is the overflow affordance.
        let (inline_defs, overflow_defs) = self.subnav_pill_split();
        let pill_items: Vec<Element<'_, Message>> =
            inline_defs.iter().map(|(k, v)| pill(k, *v)).collect();
        let pills = dir_row(pill_items)
            .spacing(3)
            .align_y(iced::Alignment::Center);
        // Search moved out of this row into each view's toolbar
        // (see `vault_search_field`).
        // Static "Personal" vault chip on the leading edge: the active
        // vault's identity. Multi-vault switching isn't wired yet, so
        // this is a non-interactive placeholder (no dropdown). "Personal"
        // is the default vault's name (data, not a translated string).
        let vault_chip: Element<'_, Message> = container(
            dir_row(vec![
                iced_fonts::lucide::lock()
                    .size(13)
                    .color(OryxisColors::t().accent)
                    .into(),
                Space::new().width(6).into(),
                text("Personal")
                    .size(12)
                    .color(OryxisColors::t().text_primary)
                    .into(),
                Space::new().width(6).into(),
                iced_fonts::lucide::chevron_down()
                    .size(12)
                    .color(OryxisColors::t().text_muted)
                    .into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .center_y(Length::Fixed(28.0))
        .padding(Padding { top: 0.0, right: 9.0, bottom: 0.0, left: 9.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into();

        // Settings gear pinned at the trailing edge, outside the
        // scrollable, so it never scrolls out of reach (mirrors how the
        // top strip docks the "+").
        let settings_active = self.active_view == View::Settings;
        let settings_gear: Element<'_, Message> = button(
            container(
                iced_fonts::lucide::settings()
                    .size(15)
                    .color(if settings_active {
                        OryxisColors::t().accent
                    } else {
                        OryxisColors::t().text_muted
                    }),
            )
            .center_y(Length::Fixed(28.0))
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 }),
        )
        .padding(0)
        .on_press(Message::ChangeView(View::Settings))
        .style(move |_, status| {
            let bg = if settings_active {
                Color { a: 0.15, ..OryxisColors::t().accent }
            } else if matches!(status, iced::widget::button::Status::Hovered) {
                Color { a: 0.08, ..OryxisColors::t().text_secondary }
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into();

        // "…" overflow button (priority+): opens the menu with the
        // destinations that didn't fit. Sits right after the visible
        // pills. U+22EF is the midline ellipsis (vertically centered,
        // unlike the baseline-sitting U+2026).
        let overflow_btn: Element<'_, Message> = if overflow_defs.is_empty() {
            Space::new().width(0).into()
        } else {
            let open = self.show_subnav_overflow;
            button(
                container(
                    text("\u{22EF}")
                        .size(16)
                        .color(if open {
                            OryxisColors::t().accent
                        } else {
                            OryxisColors::t().text_muted
                        }),
                )
                .center_y(Length::Fixed(28.0))
                .padding(Padding { top: 0.0, right: 7.0, bottom: 0.0, left: 7.0 }),
            )
            .padding(0)
            .on_press(Message::ToggleSubnavOverflow)
            .style(move |_, status| {
                let bg = if open {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else if matches!(status, iced::widget::button::Status::Hovered) {
                    Color { a: 0.08, ..OryxisColors::t().text_secondary }
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        };

        let mut row_items: Vec<Element<'_, Message>> = Vec::new();
        // Vault switcher chip only when there's more than one vault.
        if self.show_vault_switcher() {
            row_items.push(vault_chip);
            row_items.push(Space::new().width(8).into());
        }
        row_items.push(pills.into());
        row_items.push(overflow_btn);
        row_items.push(Space::new().width(Length::Fill).into());
        row_items.push(settings_gear);
        let row_inner = dir_row(row_items).align_y(iced::Alignment::Center);
        // Vertical padding equals the left padding so the chip sits with
        // equal breathing room on all sides. Background is a vertical
        // gradient from the top-bar color down into the content color,
        // so the row melts into the content below with no hard seam (no
        // bottom separator line).
        let row_content = container(row_inner)
            .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
            .width(Length::Fill)
            .style(|_| container::Style {
                // Flat, same as the content below: the sub-nav reads as
                // a toolbar sitting on the content surface. The accent
                // hairline above it is the only chrome boundary (the
                // distinct-bg / gradient experiments all read worse).
                background: Some(Background::Color(OryxisColors::t().bg_primary)),
                ..Default::default()
            });
        // No bottom separator: the gradient already blends into content.
        row_content.into()
    }

    /// Estimated rendered width of one sub-nav pill, by label key.
    /// iced exposes no pre-render measurement, so this is a heuristic:
    /// ~7.5px per glyph + 21px padding + 2px inter-pill spacing.
    fn subnav_pill_width(key: &str) -> f32 {
        // ~6.3 px/char matches Noto Sans at 12 px; the old 7.5 over-
        // estimated and tripped the "…" collapse a pill or two early.
        // 16 px is the pill's horizontal padding (8+8), +6 for the row gap.
        crate::i18n::t(key).chars().count() as f32 * 6.3 + 16.0 + 6.0
    }

    /// Full ordered list of vault sub-nav destinations (Logs auto-hides
    /// until the feature is real for this user).
    fn subnav_pill_defs(&self) -> Vec<SubnavPill> {
        let mut defs: Vec<SubnavPill> = vec![
            ("hosts", View::Dashboard),
            ("keychain", View::Keys),
            ("snippets", View::Snippets),
            ("port_forwards", View::PortForwarding),
        ];
        if self.logs_surface_visible() {
            defs.push(("logs", View::History));
        }
        // Cloud Accounts / Proxies / Known Hosts were Settings sections
        // in v0.7; they're now first-class vault surfaces.
        defs.push(("cloud_accounts", View::Cloud));
        defs.push(("proxies", View::Proxies));
        defs.push(("known_hosts", View::KnownHosts));
        defs
    }

    /// Split the destinations into the pills that fit inline and the
    /// ones that overflow into the "…" menu (priority+ navigation). The
    /// active destination is always kept inline so its highlight stays
    /// visible in the row.
    fn subnav_pill_split(&self) -> (Vec<SubnavPill>, Vec<SubnavPill>) {
        // Reserve only the pinned flanks of THIS row: chip (~115, only
        // when the vault switcher shows), gear (~31), the "…" button
        // (~28), plus row padding and gaps. Search no longer lives here
        // (it moved to each view's toolbar), so nothing is reserved for
        // it, otherwise the "…" triggered way too early.
        let chip = if self.show_vault_switcher() { 115.0 } else { 0.0 };
        let flank = chip + 31.0 + 28.0 + 24.0;
        // An open side panel sits over the sub-nav's trailing edge now,
        // so subtract its width from the budget or pills would render
        // under the panel before the "…" kicks in.
        let panel_reserve = if self.side_panel_open() {
            crate::app::PANEL_WIDTH
        } else {
            0.0
        };
        let avail = (self.window_size.width - flank - panel_reserve).max(0.0);

        let mut inline: Vec<SubnavPill> = Vec::new();
        let mut overflow: Vec<SubnavPill> = Vec::new();
        let mut used = 0.0;
        for (k, v) in self.subnav_pill_defs() {
            let w = Self::subnav_pill_width(k);
            if overflow.is_empty() && used + w <= avail {
                used += w;
                inline.push((k, v));
            } else {
                overflow.push((k, v));
            }
        }
        // If the active view spilled into the overflow, swap it back in
        // (demoting the last inline pill) so the current location stays
        // highlighted in the strip.
        if let Some(pos) = overflow.iter().position(|(_, v)| *v == self.active_view) {
            let active = overflow.remove(pos);
            if let Some(last) = inline.pop() {
                overflow.insert(0, last);
            }
            inline.push(active);
        }
        (inline, overflow)
    }

    /// Overflow ("…") dropdown for the vault sub-nav: the destinations
    /// that didn't fit inline. Backdrop + pinned panel, like the burger
    /// menu; anchored under the "…" trigger via an estimated x offset.
    pub(crate) fn view_subnav_overflow_menu(&self) -> Element<'_, Message> {
        let (inline, overflow) = self.subnav_pill_split();
        let mut col = iced::widget::Column::new().width(Length::Fill).spacing(1);
        for (k, v) in overflow {
            let active = self.active_view == v;
            let fg = if active {
                OryxisColors::t().accent
            } else {
                OryxisColors::t().text_primary
            };
            let item = button(
                container(text(crate::i18n::t(k)).size(13).color(fg))
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .padding(Padding { top: 7.0, right: 12.0, bottom: 7.0, left: 12.0 }),
            )
            .width(Length::Fill)
            .on_press(Message::ChangeView(v))
            .style(move |_, status| {
                let bg = if matches!(status, iced::widget::button::Status::Hovered) {
                    OryxisColors::t().bg_hover
                } else if active {
                    Color { a: 0.12, ..OryxisColors::t().accent }
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            });
            col = col.push(item);
        }
        let panel = container(col)
            .width(Length::Fixed(200.0))
            .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 6.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
        // Estimated x of the "…" trigger: row left padding + chip + gap
        // + the inline pills. Lands the dropdown just under the cue. The
        // chip is only present when the vault switcher shows (must match
        // `subnav_pill_split`), otherwise the menu lands ~115 px too far
        // right and clips off the window edge.
        let chip = if self.show_vault_switcher() { 115.0 + 8.0 } else { 0.0 };
        let inline_w: f32 = inline
            .iter()
            .map(|(k, _)| Self::subnav_pill_width(k))
            .sum();
        // Clamp so the 200 px panel never runs past the right edge.
        let dots_x = (8.0 + chip + inline_w).min((self.window_size.width - 206.0).max(0.0));
        let pinned = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Left)
            .align_y(iced::alignment::Vertical::Top)
            .padding(Padding {
                top: 78.0,
                right: 0.0,
                bottom: 0.0,
                left: dots_x,
            });
        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::ToggleSubnavOverflow)
        .into();
        Stack::new()
            .push(backdrop)
            .push(pinned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Burger menu overlay anchored to the top-left of the window.
    /// Pairs with the `☰` trigger in the tab bar. A transparent
    /// MouseArea backdrop catches outside clicks to dismiss; the
    /// menu items themselves stop propagation by living inside their
    /// own button widgets.
    pub(crate) fn view_burger_menu(&self) -> Element<'_, Message> {
        // Menu row: label on the leading edge, optional muted hotkey
        // hint on the trailing edge (Termius-style "Ctrl+1" tail).
        // Items dispatch the same Messages the existing sidebar /
        // status bar use, so we don't have to introduce new flows.
        let item = |label: &'static str, msg: Message, shortcut: Option<String>| -> Element<'_, Message> {
            let label_el: Element<'_, Message> = text(crate::i18n::t(label))
                .size(13)
                .color(OryxisColors::t().text_primary)
                .into();
            let inner: Element<'_, Message> = if let Some(s) = shortcut {
                let shortcut_el: Element<'_, Message> = text(s)
                    .size(11)
                    .color(OryxisColors::t().text_muted)
                    .into();
                dir_row(vec![
                    label_el,
                    Space::new().width(Length::Fill).into(),
                    shortcut_el,
                ])
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                label_el
            };
            button(
                container(inner)
                    .padding(Padding {
                        top: 8.0,
                        right: 16.0,
                        bottom: 8.0,
                        left: 16.0,
                    })
                    .width(Length::Fill)
                    .align_x(dir_align_x()),
            )
            .on_press(msg)
            .width(Length::Fill)
            .style(|_, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => OryxisColors::t().bg_hover,
                    iced::widget::button::Status::Pressed => OryxisColors::t().bg_selected,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(6.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .into()
        };
        // Resolve hotkey hints from the live bindings so user
        // overrides flow through to the menu without rebuilds.
        let hk_settings = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenSettings);
        let hk_local_shell = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenLocalShell);
        let hk_new_window = self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::NewWindow);
        // Hosts / SFTP carry the Ctrl+1 / Ctrl+2 hints since the strip
        // always renders them as area tabs.
        let hk_hosts = self.hotkey_label_for_strip_slot(0);
        // SFTP is no longer a fixed strip slot; the menu item opens a new SFTP
        // tab, so show the dedicated OpenSftp shortcut instead.
        let hk_sftp = if self.sftp_enabled {
            self.hotkey_label_for_action(crate::hotkeys::HotkeyAction::OpenSftp)
        } else {
            None
        };
        // Visual separator between item groups: a 1 px hairline with
        // some breathing room above and below. The previous version
        // applied the border color to the outer container *and* its
        // padding, which rendered as a chunky colored bar instead of
        // a thin divider. Wrap the colored hairline in a transparent
        // outer container so only the inner 1 px takes the color.
        let sep: Element<'_, Message> = iced::widget::column![
            Space::new().height(6),
            container(Space::new().width(Length::Fill).height(1))
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                }),
            Space::new().height(6),
        ]
        .width(Length::Fill)
        .into();
        // Mirror every sidebar nav entry here so Workspace mode
        // (where the sidebar is gone) still exposes the full set of
        // vault surfaces. The SFTP entry is gated on `sftp_enabled`,
        // same rule the sidebar applies.
        let sftp_item: Element<'_, Message> = if self.sftp_enabled {
            // SFTP is a tab now: the menu opens a fresh SFTP browser tab.
            item("sftp", Message::NewSftpTab, hk_sftp)
        } else {
            Space::new().height(0).into()
        };
        // Lock Vault only when a master password is set; without one,
        // locking has nothing to protect and the unlock screen has no
        // way to re-enter (mirrors the Settings -> Security gating).
        let lock_item: Element<'_, Message> = if self.vault_has_user_password {
            item("lock_vault", Message::LockVault, None)
        } else {
            Space::new().height(0).into()
        };
        // "VAULT" section header + indented children: the flat list
        // read as if Hosts/Keychain/... sat outside the Vault (issue
        // #38 review feedback); mirroring the top strip's Vault tab
        // here keeps one mental model. Indentation goes through
        // dir_row so it flips under RTL.
        let section = |label: &'static str| -> Element<'_, Message> {
            container(
                text(crate::i18n::t(label).to_uppercase())
                    .size(10)
                    .font(iced::Font {
                        weight: iced::font::Weight::Semibold,
                        ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                    })
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding { top: 8.0, right: 16.0, bottom: 2.0, left: 16.0 })
            .width(Length::Fill)
            .align_x(dir_align_x())
            .into()
        };
        fn indent(inner: Element<'_, Message>) -> Element<'_, Message> {
            dir_row(vec![Space::new().width(10).into(), inner]).into()
        }
        let menu_col = column![
            section("vault"),
            indent(item("hosts", Message::ChangeView(View::Dashboard), hk_hosts)),
            indent(item("keychain", Message::ChangeView(View::Keys), None)),
            indent(item("snippets", Message::ChangeView(View::Snippets), None)),
            indent(item(
                "port_forwards",
                Message::ChangeView(View::PortForwarding),
                None
            )),
            if self.logs_surface_visible() {
                indent(item("logs", Message::ChangeView(View::History), None))
            } else {
                Space::new().height(0).into()
            },
            indent(item("cloud_accounts", Message::ChangeView(View::Cloud), None)),
            indent(item("proxies", Message::ChangeView(View::Proxies), None)),
            indent(item("known_hosts", Message::ChangeView(View::KnownHosts), None)),
            Space::new().height(4),
            sftp_item,
            item("settings", Message::ChangeView(View::Settings), hk_settings),
            sep,
            item("local_shell", Message::OpenLocalShell, hk_local_shell),
            item("new_window", Message::SpawnNewWindow, hk_new_window),
            item("check_for_updates_now", Message::CheckForUpdateManual, None),
            lock_item,
        ]
        .width(Length::Fill);
        let menu_panel = container(menu_col)
            .width(Length::Fixed(240.0))
            .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 6.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
        // Pin the panel to the top-left, just below the tab bar
        // (40 px tall). dir_align_x flips the anchor side under RTL
        // so the dropdown lands under its trigger.
        let pinned = container(menu_panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(dir_align_x())
            .align_y(iced::alignment::Vertical::Top)
            .padding(Padding {
                top: 44.0,
                right: 0.0,
                bottom: 0.0,
                left: 6.0,
            });
        // Backdrop catches outside clicks. Z-stack: backdrop on the
        // bottom, panel on top so the panel's buttons still receive
        // their own clicks.
        let backdrop: Element<'_, Message> = MouseArea::new(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::ToggleBurgerMenu)
        .into();
        Stack::new()
            .push(backdrop)
            .push(pinned)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
