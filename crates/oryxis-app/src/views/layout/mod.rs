//! Root layout, `view_main`, `render_overlay_menu`, and the content dispatcher.

pub(crate) use iced::border::Radius;
pub(crate) use iced::widget::{button, container, text, text_input, MouseArea, Space, Stack};
pub(crate) use iced::window::Direction;
pub(crate) use iced::{Background, Border, Color, Element, Length, Padding};

pub(crate) use crate::app::{Message, Oryxis};
pub(crate) use crate::state::{OverlayContent, OverlayState, View};
pub(crate) use crate::theme::OryxisColors;
pub(crate) use crate::widgets::{context_menu_item, dir_align_x, dir_row, styled_button};

/// One vault sub-nav destination: (i18n label key, target view).
pub(crate) type SubnavPill = (&'static str, View);

/// Thickness of the edge hit-zones used for dragging to resize. Corners are
/// the same thickness but `EDGE × EDGE` squares, a bit generous so the user
/// can actually grab them without millimetre precision.
const RESIZE_EDGE: f32 = 5.0;

// Layout sub-views split into sibling files.
mod chrome;
mod main_layout;
mod menus;
mod toolbar;
pub(crate) use chrome::*;

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
    pub(crate) fn side_panel_open(&self) -> bool {
        if self.active_tab.is_some() {
            return false;
        }
        match self.active_view {
            View::Dashboard => {
                self.cloud_discover_visible
                    || self.cloud_dynamic_form.visible
                    || self.group_edit.visible
                    || self.show_host_panel
                    || self.show_session_group_panel
            }
            View::Keys => self.show_key_panel || self.show_identity_panel,
            View::Snippets => self.show_snippet_panel,
            View::PortForwarding => self.show_port_forward_panel,
            View::Proxies => self.proxy_identity_form.visible,
            View::Cloud => self.cloud_form.visible,
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
    pub(crate) fn active_side_panel(&self) -> Option<Element<'_, Message>> {
        if self.active_tab.is_some() {
            return None;
        }
        match self.active_view {
            View::Dashboard => {
                if self.cloud_discover_visible {
                    Some(self.view_cloud_discover_panel())
                } else if self.cloud_dynamic_form.visible {
                    Some(self.view_dynamic_group_form_panel())
                } else if self.group_edit.visible {
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
                .proxy_identity_form.visible
                .then(|| self.view_proxy_identity_form()),
            View::Cloud => self.cloud_form.visible.then(|| self.view_cloud_form_panel()),
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
            // 3) OS brand colour from a detected OS or a local-shell hint
            //    (PowerShell/cmd -> Windows blue, "Ubuntu (WSL)" -> Ubuntu
            //    orange), matching the per-tab icon tint.
            if let Some(os) = self.tab_detected_os(label) {
                return crate::os_icon::resolve_icon(Some(&os), OryxisColors::t().accent).1;
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
    pub(crate) fn active_view_search_empty(&self) -> bool {
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

    /// `true` when a side-panel editor is open over the active vault
    /// view, so it narrows the content (and therefore the toolbar) by
    /// `PANEL_WIDTH`. Mirrors `active_side_panel`'s conditions cheaply
    /// (no Element built) for the responsive-toolbar width budget.
    pub(crate) fn vault_panel_open(&self) -> bool {
        if self.active_tab.is_some() {
            return false;
        }
        match self.active_view {
            View::Dashboard => {
                self.cloud_discover_visible
                    || self.cloud_dynamic_form.visible
                    || self.group_edit.visible
                    || self.show_host_panel
                    || self.show_session_group_panel
            }
            View::Keys => self.show_key_panel || self.show_identity_panel,
            View::Snippets => self.show_snippet_panel,
            View::PortForwarding => self.show_port_forward_panel,
            View::Proxies => self.proxy_identity_form.visible,
            View::Cloud => self.cloud_form.visible,
            _ => false,
        }
    }

}
