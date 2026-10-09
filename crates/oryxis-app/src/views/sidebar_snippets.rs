//! Snippets sidebar tab: the snippet list, the inline snippet editor, and
//! the per-row hover actions. Split out of `views/terminal.rs` so that
//! file stays focused on the terminal pane + the sidebar shell. The shared
//! `chat_header_btn` chrome helper stays in `terminal.rs` (pub(crate)).

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use super::terminal::chat_header_btn;
use crate::app::{TabsMessage, SnippetMessage, AiMessage, Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn snippets_tab_content(&self) -> Element<'_, Message> {
        // The editor lives inline in the sidebar (the workspace is never
        // shown while a terminal tab is active, so navigating there is a
        // no-op). `show_snippet_panel` is the shared "editing a snippet"
        // flag, set by New / Edit and cleared on Save / close.
        if self.panels.snippet_panel {
            return self.sidebar_snippet_editor();
        }

        let c = OryxisColors::t();

        let new_btn = button(
            container(
                dir_row(vec![
                    iced_fonts::lucide::plus().size(12).color(c.button_text).boxed(),
                    Space::new().width(6).boxed(),
                    text(t("snippet_btn"))
                        .size(11)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        })
                        .color(c.button_text)
                        .boxed(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .center_y(Length::Fixed(22.0))
            .padding(Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 12.0 }),
        )
        .on_press(Message::Snippet(SnippetMessage::ShowSnippetPanel))
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().button_bg_hover,
                _ => OryxisColors::t().button_bg,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(6.0), ..Default::default() },
                ..Default::default()
            }
        });

        // Header: New + sort + search icons; when search is expanded, a
        // focused input (with a close X) takes over the whole row. Every
        // control is recorded into the sidebar keyboard layer (display
        // order), so Tab reaches them.
        let stab = crate::state::TerminalSidebarTab::Snippets;
        let header_row: iced::widget::Row<iced::Element<'_, Message>> = if self.sidebar_search_open {
            dir_row(vec![
                self.sidebar_nav_slot(
                    crate::keynav::SidebarRow::input(iced::widget::Id::new(
                        "sidebar-snippet-search",
                    )),
                    stab,
                    crate::widgets::INPUT_RADIUS,
                    iced::widget::text_input(t("search"), &self.sidebar_snippet_search)
                        .id(iced::widget::Id::new("sidebar-snippet-search"))
                        .on_input(|v| Message::Ai(AiMessage::SidebarSnippetSearchChanged(v)))
                        .padding(8)
                        .size(13)
                        .style(crate::widgets::rounded_input_style)
                        .boxed(),
                ),
                Space::new().width(6).boxed(),
                self.sidebar_nav_slot(
                    crate::keynav::SidebarRow::button(Message::Ai(AiMessage::ToggleSidebarSearch)),
                    stab,
                    6.0,
                    chat_header_btn(iced_fonts::lucide::x(), Message::Ai(AiMessage::ToggleSidebarSearch)),
                ),
            ])
        } else {
            // Tag filter: only snippets sharing a tag with the focused
            // host. Accent-lit while active (chat_header_btn pins the
            // muted color, so the active state gets its own chrome).
            let filter_active = self.prefs.snippet_tag_filter;
            let filter_btn: Element<'_, Message> = button(
                container(iced_fonts::lucide::tag().size(13).color(if filter_active {
                    c.accent
                } else {
                    c.text_muted
                }))
                .center_x(Length::Fixed(28.0))
                .center_y(Length::Fixed(24.0)),
            )
            .padding(0)
            .on_press(Message::Snippet(SnippetMessage::ToggleSnippetTagFilter))
            .style(move |_, status| {
                let bg = if filter_active {
                    Color { a: 0.15, ..OryxisColors::t().accent }
                } else {
                    match status {
                        BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    }
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(4.0), ..Default::default() },
                    ..Default::default()
                }
            })
            .boxed();
            let filter_btn = crate::views::terminal::icon_tooltip(
                filter_btn,
                t("snippet_tag_filter_tip"),
            );
            dir_row(vec![
                // Contrast ring: the button is accent-filled, an
                // accent ring would vanish into it.
                self.sidebar_nav_slot_contrast(
                    crate::keynav::SidebarRow::button(Message::Snippet(SnippetMessage::ShowSnippetPanel)),
                    stab,
                    6.0,
                    new_btn.boxed(),
                ),
                Space::new().width(Length::Fill).boxed(),
                self.sidebar_nav_slot(
                    crate::keynav::SidebarRow::button(Message::Snippet(SnippetMessage::ToggleSnippetTagFilter)),
                    stab,
                    6.0,
                    filter_btn,
                ),
                Space::new().width(2).boxed(),
                self.sidebar_nav_slot(
                    crate::keynav::SidebarRow::button(Message::Ai(AiMessage::ToggleSidebarSort)),
                    stab,
                    6.0,
                    chat_header_btn(sort_glyph(self.snippets_sort), Message::Ai(AiMessage::ToggleSidebarSort)),
                ),
                Space::new().width(2).boxed(),
                self.sidebar_nav_slot(
                    crate::keynav::SidebarRow::button(Message::Ai(AiMessage::ToggleSidebarSearch)),
                    stab,
                    6.0,
                    chat_header_btn(iced_fonts::lucide::search(), Message::Ai(AiMessage::ToggleSidebarSearch)),
                ),
            ])
        };
        let header = container(header_row.width(Length::Fill).align_y(iced::Alignment::Center))
            .padding(Padding { top: 10.0, right: 12.0, bottom: 8.0, left: 12.0 });

        // Built-in "global snippet": type the host's stored password +
        // Enter (e.g. to answer a sudo prompt). Shown only for a live
        // remote session (SSH or Telnet); the click no-ops with a toast
        // if no password is stored.
        // Keynav-recorded by owner request (2026-07-03): reaching it takes
        // a deliberate Tab/arrow walk plus Enter, the same intent bar as a
        // click, and it carries no paste/delete verbs.
        let remote_active = self
            .cur_active_tab()
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.active().session.is_some())
            .unwrap_or(false);
        let sudo_row: Element<'_, Message> = if remote_active {
            // The ring wraps the button itself, INSIDE the padded
            // container, so it hugs the visible border instead of
            // floating 12 px away (owner QA).
            let btn = self.sidebar_nav_slot(
                crate::keynav::SidebarRow::button(Message::Snippet(SnippetMessage::ApplySudoPassword)),
                stab,
                8.0,
                button(
                    container(
                        dir_row(vec![
                            iced_fonts::lucide::shield_check().size(13).color(c.accent).boxed(),
                            Space::new().width(8).boxed(),
                            text(t("apply_sudo_password")).size(12).color(c.text_primary).boxed(),
                        ])
                        .align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                    .width(Length::Fill),
                )
                .on_press(Message::Snippet(SnippetMessage::ApplySudoPassword))
                .width(Length::Fill)
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => OryxisColors::t().bg_surface,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            radius: Radius::from(8.0),
                            color: Color { a: 0.5, ..OryxisColors::t().accent },
                            width: 1.0,
                        },
                        ..Default::default()
                    }
                })
                .boxed(),
            );
            container(btn)
                .padding(Padding { top: 0.0, right: 12.0, bottom: 8.0, left: 12.0 })
                .boxed()
        } else {
            Space::new().boxed()
        };

        // Sort then filter, carrying original indices so Run/Paste/Edit
        // address the right snippet (the list reorders, `self.snippets`
        // does not). The search needle also matches tags and the group
        // name; the tag filter (when on and a tagged saved host is
        // focused) keeps only snippets sharing at least one tag.
        let needle = self.sidebar_snippet_search.to_lowercase();
        let host_tags = self.focused_host_tags_lower();
        let mut order: Vec<usize> = (0..self.snippets.len()).collect();
        self.snippets_sort.sort_items(
            &mut order,
            |&i| self.snippets[i].label.clone(),
            |&i| self.snippets[i].created_at,
        );
        let visible: Vec<usize> = order
            .into_iter()
            .filter(|&idx| {
                let snip = &self.snippets[idx];
                let needle_ok = needle.is_empty()
                    || snip.label.to_lowercase().contains(&needle)
                    || snip.command.to_lowercase().contains(&needle)
                    || snip.tags.iter().any(|tg| tg.to_lowercase().contains(&needle))
                    || snip
                        .group
                        .as_ref()
                        .is_some_and(|g| g.to_lowercase().contains(&needle));
                let tags_ok = !self.prefs.snippet_tag_filter
                    || match &host_tags {
                        Some(ht) => snip
                            .tags
                            .iter()
                            .any(|tg| ht.contains(&tg.to_lowercase())),
                        // No focused saved host (or an untagged one):
                        // the filter has nothing to compare, show all.
                        None => true,
                    };
                needle_ok && tags_ok
            })
            .collect();

        let mut list = iced::widget::Column::<iced::Element<'_, _>>::new()
            .spacing(6)
            .padding(Padding { top: 0.0, right: 12.0, bottom: 12.0, left: 12.0 });
        // Rows are recorded into the sidebar keynav layer; the
        // recording index is the display position within the
        // sorted/filtered/grouped list. Enter RUNS the snippet (owner
        // call: there is no keyboard path to the terminal's own Enter
        // afterwards), Shift+Enter pastes without the newline.
        // Floating actions stay hover-only (owner call); the ring
        // border alone marks the keyboard selection.
        // Mirror the vault view's drill-in: at root, group FOLDER ROWS
        // (click / Enter to open) above the ungrouped snippets; inside
        // a group, a back row plus that group's snippets. An active
        // search flattens everything.
        let searching = !needle.is_empty();
        let snippet_rows: Vec<usize>;
        let mut folder_rows: Vec<(String, usize)> = Vec::new();
        if let Some(open) = self.sidebar_snippet_group.clone().filter(|_| !searching) {
            // Back row first (recorded, so Enter on it goes up).
            let back = button(
                container(
                    dir_row(vec![
                        iced_fonts::lucide::arrow_left()
                            .size(13)
                            .color(c.accent)
                            .boxed(),
                        Space::new().width(8).boxed(),
                        iced_fonts::lucide::folder().size(13).color(c.accent).boxed(),
                        Space::new().width(6).boxed(),
                        text(open.clone()).size(12).color(c.text_primary).boxed(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
                .width(Length::Fill),
            )
            .on_press(Message::Snippet(SnippetMessage::CloseSidebarSnippetGroup))
            .width(Length::Fill)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                    _ => Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }
            });
            list = list.push(self.sidebar_nav_slot(
                crate::keynav::SidebarRow::button(Message::Snippet(SnippetMessage::CloseSidebarSnippetGroup)),
                stab,
                8.0,
                back.boxed(),
            ));
            snippet_rows = visible
                .iter()
                .copied()
                .filter(|&i| {
                    self.snippets[i]
                        .group
                        .as_ref()
                        .is_some_and(|g| g.eq_ignore_ascii_case(&open))
                })
                .collect();
        } else if searching {
            snippet_rows = visible.clone();
        } else {
            for name in self.snippet_group_names() {
                let count = visible
                    .iter()
                    .filter(|&&i| {
                        self.snippets[i]
                            .group
                            .as_ref()
                            .is_some_and(|g| g.eq_ignore_ascii_case(&name))
                    })
                    .count();
                if count > 0 {
                    folder_rows.push((name, count));
                }
            }
            snippet_rows = visible
                .iter()
                .copied()
                .filter(|&i| self.snippets[i].group.is_none())
                .collect();
        }
        for (name, count) in folder_rows {
            let folder = button(
                container(
                    dir_row(vec![
                        iced_fonts::lucide::folder().size(14).color(c.accent).boxed(),
                        Space::new().width(8).boxed(),
                        column![
                            text(name.clone()).size(13).color(c.text_primary),
                            text(crate::i18n::snippet_count(count))
                                .size(10)
                                .color(c.text_muted),
                        ]
                        .spacing(2)
                        .width(Length::Fill)
                        .boxed(),
                        iced_fonts::lucide::chevron_right()
                            .size(13)
                            .color(c.text_muted)
                            .boxed(),
                    ])
                    .align_y(iced::Alignment::Center),
                )
                .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                .width(Length::Fill),
            )
            .on_press(Message::Snippet(SnippetMessage::OpenSidebarSnippetGroup(name.clone())))
            .width(Length::Fill)
            .style(|_, status| {
                let bg = match status {
                    BtnStatus::Hovered | BtnStatus::Pressed => OryxisColors::t().bg_hover,
                    _ => OryxisColors::t().bg_surface,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border { radius: Radius::from(8.0), ..Default::default() },
                    ..Default::default()
                }
            });
            list = list.push(self.sidebar_nav_slot(
                // A group card is list body: the arrow hover-entry may
                // land here (folders-only frames have no snippet rows).
                crate::keynav::SidebarRow::list_button(Message::Snippet(SnippetMessage::OpenSidebarSnippetGroup(name))),
                stab,
                8.0,
                folder.boxed(),
            ));
        }
        let any_rows = !snippet_rows.is_empty();
        // The focused pane's saved host, for the install-script hint
        // (issue #147): quick-connect and local panes have no install
        // memory, so their rows show the plain category marker.
        let install_host = self
            .cur_active_tab()
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| match t.active().origin {
                crate::state::PaneOrigin::Host(id) => Some(id),
                _ => None,
            });
        for idx in snippet_rows {
            let snip = &self.snippets[idx];
            let row = snippet_row(
                idx,
                &snip.label,
                &snip.command,
                self.cur_nav().hover.snippet_card == Some(idx),
                snip.install.then(|| {
                    install_host
                        .and_then(|h| self.install_runs.get(&(h, snip.id)).copied())
                }),
            );
            list = list.push(self.sidebar_nav_slot(
                crate::keynav::SidebarRow::item(
                    Message::Snippet(SnippetMessage::RunSnippet(idx)),
                    Message::Snippet(SnippetMessage::PasteSnippet(idx)),
                    Message::Snippet(SnippetMessage::RequestDeleteSnippet(idx)),
                ),
                stab,
                8.0,
                row,
            ));
        }
        if visible.is_empty() || (searching && !any_rows) {
            list = list.push(sidebar_placeholder(t("no_matches")));
        }

        // Shared id with the History list (only one renders): the
        // sidebar keynav router snaps the ringed row into view by it.
        let base = column![
            header,
            sudo_row,
            scrollable(list)
                .id(crate::keynav::sidebar_scroll_id(crate::state::TerminalSidebarTab::Snippets))
                .height(Length::Fill)
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        // Always a Stack, the list at layer 0: wrapping it only while the
        // sort popover is up re-parented the list on every open and close,
        // which reset its scroll to the top.
        let mut stack = iced::widget::Stack::<iced::Element<'_, _>>::new().push(base.boxed());
        if self.sidebar_sort_open {
            use crate::state::{ListSort, SortMenuKind};
            let menu = container(column![
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::LabelAsc,
                    iced_fonts::lucide::arrow_down_a_z::<iced::Theme>(),
                    "sort_label_asc",
                    self.snippets_sort == ListSort::LabelAsc,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::LabelDesc,
                    iced_fonts::lucide::arrow_down_z_a::<iced::Theme>(),
                    "sort_label_desc",
                    self.snippets_sort == ListSort::LabelDesc,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::NewestFirst,
                    iced_fonts::lucide::calendar_arrow_down::<iced::Theme>(),
                    "sort_newest_first",
                    self.snippets_sort == ListSort::NewestFirst,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::OldestFirst,
                    iced_fonts::lucide::calendar_arrow_up::<iced::Theme>(),
                    "sort_oldest_first",
                    self.snippets_sort == ListSort::OldestFirst,
                ),
            ])
            .width(Length::Fixed(190.0))
            .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
            // Anchor under the header, hugging the trailing edge.
            let positioned = container(column![
                Space::new().height(Length::Fixed(46.0)),
                container(menu)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 0.0 }),
            ])
            .width(Length::Fill)
            .height(Length::Fill);
            // Transparent backdrop dismisses the popover on any click.
            let backdrop: Element<'_, Message> = MouseArea::new(
                container(Space::new()).width(Length::Fill).height(Length::Fill),
            )
            .on_press(Message::Ai(AiMessage::ToggleSidebarSort))
            .boxed();
            stack = stack.push(backdrop).push(positioned.boxed());
        }
        stack.boxed()
    }

    /// Compact New / Edit snippet form rendered inline in the Snippets
    /// tab (reuses the same `snippet_*` state + messages as the workspace
    /// editor). A back arrow cancels; Save persists and returns to the
    /// list; Delete shows only when editing an existing snippet.
    fn sidebar_snippet_editor(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let title = if self.snippet_form.editing_id.is_some() {
            t("edit_snippet")
        } else {
            t("new_snippet")
        };

        let header = dir_row(vec![
            chat_header_btn(iced_fonts::lucide::arrow_left(), Message::Snippet(SnippetMessage::HideSnippetPanel)),
            Space::new().width(6).boxed(),
            text(title).size(14).color(c.text_primary).boxed(),
        ])
        .align_y(iced::Alignment::Center);

        let label_input: Element<'_, Message> =
            iced::widget::text_input("restart-nginx", &self.snippet_form.label)
                .on_input(|v| Message::Snippet(SnippetMessage::SnippetLabelChanged(v)))
                .padding(8)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .boxed();
        // Same type-ahead combo as the vault snippet panel: existing
        // groups filter as you type, a new name is accepted as-is.
        // `on_close` makes the fork's combo restore the committed value
        // when the field is focused and then abandoned (it clears its
        // own input on focus, and restores only on that branch).
        let group_selection = (!self.snippet_form.group.is_empty()).then_some(&self.snippet_form.group);
        let group_input: Element<'_, Message> = iced::widget::combo_box(
            &self.snippet_form.group_combo,
            t("group_optional_placeholder"),
            group_selection,
            |v| Message::Snippet(SnippetMessage::SnippetGroupChanged(v)),
        )
        .on_input(|v| Message::Snippet(SnippetMessage::SnippetGroupChanged(v)))
        .on_close(Message::NoOp)
        .padding(8)
        .size(13.0)
        .input_style(crate::widgets::rounded_input_style)
        .menu_style(crate::widgets::combo_menu_style)
        .width(Length::Fill)
        .boxed();
        let tags_input: Element<'_, Message> =
            iced::widget::text_input(t("tags_placeholder"), &self.snippet_form.tags_input)
                .on_input(|v| Message::Snippet(SnippetMessage::SnippetTagsChanged(v)))
                .padding(8)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .boxed();
        // Multi-line, auto-grows with content; container caps the height
        // (~8 lines) and then it scrolls internally.
        let command_input: Element<'_, Message> = container(
            iced::widget::text_editor(&self.snippet_form.command)
                .placeholder("sudo systemctl restart nginx")
                .on_action(|v| Message::Snippet(SnippetMessage::SnippetCommandAction(v)))
                .padding(8)
                .size(13)
                .height(Length::Shrink)
                .style(crate::widgets::rounded_editor_style),
        )
        .height(Length::Shrink.max(180.0))
        .boxed();

        let error: Element<'_, Message> = if let Some(err) = &self.snippet_form.error {
            text(err.clone()).size(11).color(c.error).boxed()
        } else {
            Space::new().boxed()
        };

        let save = button(
            container(text(t("save")).size(13).color(c.button_text))
                .center_x(Length::Fill)
                .padding(Padding { top: 9.0, right: 0.0, bottom: 9.0, left: 0.0 }),
        )
        .on_press(Message::Snippet(SnippetMessage::SaveSnippet))
        .width(Length::Fill)
        .style(|_, _| button::Style {
            background: Some(Background::Color(OryxisColors::t().accent)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

        let mut form = column![
            header,
            Space::new().height(12),
            text(t("name")).size(12).color(c.text_secondary),
            Space::new().height(4),
            label_input,
            Space::new().height(12),
            text(t("group")).size(12).color(c.text_secondary),
            Space::new().height(4),
            group_input,
            Space::new().height(12),
            text(t("tags")).size(12).color(c.text_secondary),
            Space::new().height(4),
            tags_input,
            Space::new().height(12),
            text(t("snippet_hotkey")).size(12).color(c.text_secondary),
            Space::new().height(4),
            self.snippet_hotkey_row(false),
            Space::new().height(12),
            text(t("command_label")).size(12).color(c.text_secondary),
            Space::new().height(4),
            command_input,
            Space::new().height(10),
            error,
            Space::new().height(12),
            save,
        ]
        .spacing(0)
        .padding(12);

        if let Some(edit_id) = self.snippet_form.editing_id
            && let Some(idx) = self.snippets.iter().position(|s| s.id == edit_id)
        {
            let delete = button(
                container(text(t("delete")).size(13).color(OryxisColors::t().error))
                    .center_x(Length::Fill)
                    .padding(Padding { top: 9.0, right: 0.0, bottom: 9.0, left: 0.0 }),
            )
            .on_press(Message::Snippet(SnippetMessage::RequestDeleteSnippet(idx)))
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().error,
                    width: 1.0,
                },
                ..Default::default()
            });
            form = form.push(Space::new().height(8).boxed()).push(delete.boxed());
        }

        form.width(Length::Fill).height(Length::Fill).boxed()
    }
}

/// Glyph for the collapsed sort button, reflecting the current sort so
/// the icon doubles as a state indicator (matches the workspace toolbar).
fn sort_glyph<'a>(sort: crate::state::ListSort) -> iced::widget::Text<'a> {
    use crate::state::ListSort;
    match sort {
        ListSort::LabelAsc => iced_fonts::lucide::arrow_down_a_z(),
        ListSort::LabelDesc => iced_fonts::lucide::arrow_down_z_a(),
        ListSort::NewestFirst => iced_fonts::lucide::calendar_arrow_down(),
        ListSort::OldestFirst => iced_fonts::lucide::calendar_arrow_up(),
    }
}

/// Centered muted text for an empty / not-yet-built sidebar tab.
fn sidebar_placeholder<'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(12).color(OryxisColors::t().text_muted))
        .center_x(Length::Fill)
        .padding(Padding { top: 40.0, right: 12.0, bottom: 0.0, left: 12.0 })
        .width(Length::Fill)
        .boxed()
}

/// An icon action with a tooltip, used for the floating snippet-row
/// actions so Paste (no newline) and Run (+ Enter) are self-explanatory.
fn action_btn<'a>(
    icon: iced::widget::Text<'a>,
    msg: Message,
    tip: &'a str,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        chat_header_btn(icon, msg),
        container(text(tip).size(11).color(OryxisColors::t().text_primary))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Top,
    )
    .boxed()
}

/// One row in the Snippets tab. Label + a single ellipsized line of the
/// command read inline; the Edit / Paste / Run actions float over the
/// trailing edge and only appear on hover (see the card-icon convention
/// in CLAUDE.md). `hovered` is `self.cur_nav().hover.snippet_card == Some(idx)`.
fn snippet_row<'a>(
    idx: usize,
    label: &'a str,
    command: &'a str,
    hovered: bool,
    // Install-script hint (issue #147): `None` = ordinary snippet;
    // `Some(None)` = install script, no recorded run on this host;
    // `Some(Some(at))` = it ran here, last at `at`.
    install: Option<Option<chrono::DateTime<chrono::Utc>>>,
) -> Element<'a, Message> {
    let c = OryxisColors::t();
    // First line only, ellipsized, so multi-line snippets stay one row.
    let first = command.lines().next().unwrap_or("");
    let multiline = command.lines().nth(1).is_some();
    let preview: String = {
        let head: String = first.chars().take(48).collect();
        if multiline || first.chars().count() > 48 {
            format!("{head}…")
        } else {
            head
        }
    };
    let mut info = column![
        text(label).size(13).color(c.text_primary),
        text(preview).size(11).color(c.text_muted),
    ]
    .spacing(2)
    .width(Length::Fill);
    // The category marker doubles as the per-host memory: "installed
    // here" is a hint, not a lock, so the row stays runnable.
    if let Some(ran) = install {
        info = info.push((match ran {
            Some(at) => text(format!(
                "{} \u{b7} {}",
                t("snippet_installed_here"),
                at.format("%Y-%m-%d")
            ))
            .size(10)
            .color(c.success),
            None => text(t("snippet_install_badge")).size(10).color(c.warning),
        }).boxed());
    }

    let card = container(info)
        .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_surface)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });

    let row_el: Element<'a, Message> = if hovered {
        let actions = container(
            dir_row(vec![
                action_btn(iced_fonts::lucide::pencil(), Message::Snippet(SnippetMessage::EditSnippet(idx)), t("edit_snippet")),
                action_btn(iced_fonts::lucide::clipboard_copy(), Message::Snippet(SnippetMessage::PasteSnippet(idx)), t("snippet_paste")),
                action_btn(iced_fonts::lucide::play(), Message::Snippet(SnippetMessage::RunSnippet(idx)), t("snippet_run")),
            ])
            .spacing(2)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 3.0, right: 5.0, bottom: 3.0, left: 5.0 })
        .style(|_: &iced::Theme| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let overlay = container::<_, iced::Theme>(actions)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center)
            .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 0.0 });
        iced::widget::Stack::<iced::Element<'_, _>>::new().push(card.boxed()).push(overlay.boxed()).boxed()
    } else {
        card.boxed()
    };

    MouseArea::new(row_el)
        .on_enter(Message::Tabs(TabsMessage::SnippetCardHovered(idx)))
        .on_exit(Message::Tabs(TabsMessage::SnippetCardUnhovered(idx)))
        .boxed()
}
