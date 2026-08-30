//! Settings screen, terminal, AI, theme, shortcuts, security, about.

pub(crate) use iced::border::Radius;
pub(crate) use iced::widget::{button, checkbox, container, pick_list, scrollable, text, text_input, Space};
// `column` carries both a fn and a `column!` macro; re-exporting it through the
// `use super::*` glob makes the macro ambiguous in the section submodules, so it
// is imported directly here and in each section file instead.
use iced::widget::column;
pub(crate) use iced::widget::button::Status as BtnStatus;
pub(crate) use iced::{Background, Border, Color, Element, Length, Padding};

pub(crate) use crate::app::{SettingsMessage, McpMessage, NavigationMessage, CommandHistoryMessage, ProxyIdentityMessage, AgentMessage, ZmodemMessage, Message, Oryxis, VaultMessage, AiMessage, ShareMessage, NAV_RAIL_WIDTH_EXPANDED};
pub(crate) use crate::i18n::t;
pub(crate) use crate::state::SettingsSection;
pub(crate) use crate::theme::OryxisColors;
pub(crate) use crate::widgets::{
    dir_align_x, dir_row, key_badge, panel_field, panel_section, settings_row, shortcut_row,
    styled_button, styled_button_opt,
};

// Per-section view methods, split into sibling files.
mod about;
mod advanced;
mod agent;
mod ai;
mod connection;
mod highlight_rule_modal;
mod highlight_rules;
mod interface;
mod local_terminals;
pub(crate) mod login_scripts;
mod mcp;
mod previews;
mod proxies;
mod security;
mod monitoring;
mod sftp;
mod shortcuts;
mod terminal;

impl Oryxis {
    pub(crate) fn view_settings(&self) -> Element<'_, Message> {
        // ── Settings sidebar ──
        let settings_sidebar = {
            // Order: most-touched at the top (visual + everyday
            // configuration), then per-feature toggles, then network
            // resources, then plugin / system / about. The previous
            // order was historical (followed the implementation
            // sequence) and didn't reflect how users actually move
            // through the panel.
            // Core sections, then the "feature plugin" sections (AI /
            // MCP / SFTP / SSH Agent) which only
            // appear once the feature is enabled on the Plugins screen,
            // then About. The
            // enable/disable toggles live on the Plugins screen, not here.
            // The list (with its feature gating) is shared with the
            // command palette's "Settings: X" rows via this helper.
            // Settings has no vault toolbar, so nothing re-records the
            // toolbar zone here: clear it so Tab can't land on ghost
            // buttons recorded by the previous vault view.
            self.keynav_toolbar_reset();
            let items = self.settings_section_items();
            // Sidebar search over the settings index (JetBrains model):
            // the section tree STAYS; a non-empty query dims sections
            // with no hits, badges the ones that match, auto-opens the
            // best (in the dispatch handler), and the open section's
            // content highlights every matching row in place.
            let results = self.settings_search_results(&self.settings_search);
            let searching = !self.settings_search.trim().is_empty();
            // Per-section hit counts drive the sidebar badges + dimming.
            let mut counts: std::collections::HashMap<SettingsSection, usize> =
                std::collections::HashMap::new();
            for (entry, _) in &results {
                *counts.entry(entry.section).or_insert(0) += 1;
            }
            // Hand the OPEN section's matching row labels to the content
            // render (which runs after this block) so its rows can
            // highlight. Value-compared, so `t(label_key)` must equal
            // the row's own `t(...)` label (true for every index key).
            *self.keynav.settings_match_labels.borrow_mut() = results
                .iter()
                .filter(|(e, _)| e.section == self.settings_section)
                .map(|(e, _)| t(e.label_key))
                .collect();
            // Find-next cursor: the active match's label (accent ring +
            // scroll anchor) and the "n/total" counter come from the
            // document-ordered list.
            let ordered = self.settings_ordered_matches(&self.settings_search);
            self.keynav
                .settings_active_label
                .set(ordered.get(self.settings_active_match).map(|(_, l)| *l));
            // Record the section list for the keyboard router (SubNav
            // zone): dynamic set (feature toggles hide sections), so it
            // comes from this exact list, not the enum.
            *self.keynav.subnav_items.borrow_mut() = items
                .iter()
                .map(|(_, s)| crate::keynav::NavItem::SettingsSection(*s))
                .collect();
            let kb_sel = match self.keynav.selected_in(crate::keynav::FocusZone::SubNav) {
                Some(crate::keynav::NavItem::SettingsSection(s)) => Some(s),
                _ => None,
            };

            let search = text_input(t("settings_search_placeholder"), &self.settings_search)
                // Zone zero of the Settings view: Ctrl+F and the Tab
                // cycle land here via `active_view_search_id`.
                .id(iced::widget::Id::new("search-settings"))
                .on_input(|v| Message::Settings(SettingsMessage::SettingsSearchChanged(v)))
                .padding(Padding { top: 9.0, right: 12.0, bottom: 9.0, left: 12.0 })
                .size(13)
                .width(Length::Fill)
                .style(crate::widgets::rounded_input_style)
                .align_x(dir_align_x());

            let mut col = column![]
                .spacing(4)
                .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 });
            col = col.push(
                container(search).padding(Padding { top: 0.0, right: 0.0, bottom: 2.0, left: 0.0 }),
            );
            // Find-in-page counter + hint: which match Enter is on and
            // how many there are (Enter / Shift+Enter cycle them).
            if searching && !ordered.is_empty() {
                let pos = (self.settings_active_match % ordered.len()) + 1;
                col = col.push(
                    container(
                        text(format!(
                            "{pos}/{}  {}",
                            ordered.len(),
                            t("settings_search_nav_hint")
                        ))
                        .size(11)
                        .color(OryxisColors::t().text_muted),
                    )
                    .padding(Padding { top: 0.0, right: 16.0, bottom: 6.0, left: 4.0 }),
                );
            }
            // Gibberish query: the tree gives no signal on its own, so
            // say it plainly.
            if searching && results.is_empty() {
                col = col.push(
                    container(
                        text(t("settings_search_no_results"))
                            .size(12)
                            .color(OryxisColors::t().text_muted),
                    )
                    .width(Length::Fill)
                    .align_x(dir_align_x())
                    .padding(Padding { top: 4.0, right: 16.0, bottom: 8.0, left: 16.0 }),
                );
            }

            for (label, section) in items {
                let is_active = self.settings_section == section;
                let kb_selected = kb_sel == Some(section);
                let hits = counts.get(&section).copied().unwrap_or(0);
                // Dim non-matching sections while searching so the
                // matches pop, JetBrains-style (the node stays clickable).
                let dimmed = searching && hits == 0;
                let bg = if is_active {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else {
                    Color::TRANSPARENT
                };
                let fg = if is_active {
                    OryxisColors::t().accent
                } else if dimmed {
                    OryxisColors::t().text_muted
                } else {
                    OryxisColors::t().text_secondary
                };
                let mut row_items: Vec<Element<'_, Message>> =
                    vec![text(label).size(13).color(fg).into()];
                if searching && hits > 0 {
                    // Match-count badge: the tree's "this section
                    // contains matches" signal.
                    row_items.push(Space::new().width(Length::Fill).into());
                    row_items.push(
                        text(hits.to_string())
                            .size(11)
                            .color(OryxisColors::t().accent)
                            .into(),
                    );
                }
                let btn: Element<'_, Message> = button(
                    container(dir_row(row_items).align_y(iced::Alignment::Center))
                        .width(Length::Fill)
                        .align_x(crate::widgets::dir_align_x())
                        .padding(Padding { top: 12.0, right: 16.0, bottom: 12.0, left: 16.0 }),
                )
                .on_press(Message::Settings(SettingsMessage::ChangeSettingsSection(section)))
                // Zero the button's default padding so the container's
                // 16/12 is the exact content inset.
                .padding(0)
                .width(Length::Fill)
                .style(move |_, status| {
                    let hover_bg = match status {
                        BtnStatus::Hovered if !is_active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                        BtnStatus::Pressed => Color { a: 0.25, ..OryxisColors::t().accent },
                        // Keyboard selection reads on active rows too
                        // (border alone vanishes on the accent tint).
                        _ if kb_selected && is_active => Color { a: 0.30, ..OryxisColors::t().accent },
                        _ if kb_selected => Color::from_rgba(1.0, 1.0, 1.0, 0.10),
                        _ => bg,
                    };
                    button::Style {
                        background: Some(Background::Color(hover_bg)),
                        border: Border {
                            radius: Radius::from(10.0),
                            color: if kb_selected {
                                OryxisColors::t().accent
                            } else {
                                Color::TRANSPARENT
                            },
                            width: if kb_selected { 2.0 } else { 0.0 },
                        },
                        ..Default::default()
                    }
                })
                .into();
                col = col.push(btn);
            }

            // Wrap the section list in a scrollable so a short window
            // doesn't clip the bottom entries (About / Plugins were
            // disappearing when the height dropped below ~520 px).
            // Width matches the main vertical nav rail; no side hairline
            // so it reads as the same sidebar surface.
            container(
                scrollable(col)
                    // Stable id so the keyboard router can keep the
                    // selected section in view on short windows.
                    .id(iced::widget::Id::new("settings-sidebar-scroll"))
                    .height(Length::Fill),
            )
            .width(NAV_RAIL_WIDTH_EXPANDED)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_sidebar)),
                ..Default::default()
            })
        };

        // ── Settings content ──
        let settings_content: Element<'_, Message> = match self.settings_section {
            SettingsSection::Terminal => self.view_settings_terminal(),

            SettingsSection::Connection => self.view_settings_connection(),

            SettingsSection::Sftp => self.view_settings_sftp(),
            SettingsSection::Monitoring => self.view_settings_monitoring(),

            SettingsSection::AI => self.view_settings_ai(),

            SettingsSection::Interface => self.view_settings_interface(),

            SettingsSection::Shortcuts => self.view_settings_shortcuts(),

            SettingsSection::Security => self.view_settings_security(),

            SettingsSection::Agent => self.view_settings_agent(),

            SettingsSection::Advanced => self.view_settings_advanced(),
            SettingsSection::About => self.view_settings_about(),
            SettingsSection::Plugins => self.view_plugins_panel(),
            SettingsSection::Mcp => self.view_settings_mcp(),
        };

        let layout = container(crate::widgets::dir_row(vec![
            settings_sidebar.into(),
            container(settings_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ]))
        .width(Length::Fill)
        .height(Length::Fill);

        layout.into()
    }
}

/// i18n key for an export/import category's checkbox label.
pub(crate) fn category_label_key(c: oryxis_vault::ExportCategory) -> &'static str {
    use oryxis_vault::ExportCategory as C;
    match c {
        C::Connections => "cat_connections",
        C::Groups => "cat_groups",
        C::Keys => "cat_keys",
        C::Identities => "cat_identities",
        C::ProxyIdentities => "cat_proxies",
        C::Snippets => "cat_snippets",
        C::KnownHosts => "cat_known_hosts",
        C::PortForwardRules => "cat_port_forwards",
        C::SessionGroups => "cat_session_layouts",
        C::Settings => "cat_settings",
    }
}
