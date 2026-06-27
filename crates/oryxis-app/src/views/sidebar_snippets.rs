//! Snippets sidebar tab: the snippet list, the inline snippet editor, and
//! the per-row hover actions. Split out of `views/terminal.rs` so that
//! file stays focused on the terminal pane + the sidebar shell. The shared
//! `chat_header_btn` chrome helper stays in `terminal.rs` (pub(crate)).

use iced::border::Radius;
use iced::widget::button::Status as BtnStatus;
use iced::widget::{button, column, container, scrollable, text, MouseArea, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use super::terminal::chat_header_btn;
use crate::app::{Message, Oryxis};
use crate::i18n::t;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

impl Oryxis {
    pub(crate) fn snippets_tab_content(&self) -> Element<'_, Message> {
        // The editor lives inline in the sidebar (the workspace is never
        // shown while a terminal tab is active, so navigating there is a
        // no-op). `show_snippet_panel` is the shared "editing a snippet"
        // flag, set by New / Edit and cleared on Save / close.
        if self.show_snippet_panel {
            return self.sidebar_snippet_editor();
        }

        let c = OryxisColors::t();

        let new_btn = button(
            container(
                dir_row(vec![
                    iced_fonts::lucide::plus().size(12).color(c.button_text).into(),
                    Space::new().width(6).into(),
                    text(t("snippet_btn"))
                        .size(11)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::new(crate::theme::SYSTEM_UI_FAMILY)
                        })
                        .color(c.button_text)
                        .into(),
                ])
                .align_y(iced::Alignment::Center),
            )
            .center_y(Length::Fixed(22.0))
            .padding(Padding { top: 0.0, right: 12.0, bottom: 0.0, left: 12.0 }),
        )
        .on_press(Message::ShowSnippetPanel)
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
        // focused input (with a close X) takes over the whole row.
        let header_row: iced::widget::Row<'_, Message> = if self.sidebar_search_open {
            dir_row(vec![
                iced::widget::text_input(t("search"), &self.sidebar_snippet_search)
                    .id(iced::widget::Id::new("sidebar-snippet-search"))
                    .on_input(Message::SidebarSnippetSearchChanged)
                    .padding(8)
                    .size(13)
                    .style(crate::widgets::rounded_input_style)
                    .into(),
                Space::new().width(6).into(),
                chat_header_btn(iced_fonts::lucide::x(), Message::ToggleSidebarSearch),
            ])
        } else {
            dir_row(vec![
                new_btn.into(),
                Space::new().width(Length::Fill).into(),
                chat_header_btn(sort_glyph(self.snippets_sort), Message::ToggleSidebarSort),
                Space::new().width(2).into(),
                chat_header_btn(iced_fonts::lucide::search(), Message::ToggleSidebarSearch),
            ])
        };
        let header = container(header_row.width(Length::Fill).align_y(iced::Alignment::Center))
            .padding(Padding { top: 10.0, right: 12.0, bottom: 8.0, left: 12.0 });

        // Sort then filter, carrying original indices so Run/Paste/Edit
        // address the right snippet (the list reorders, `self.snippets`
        // does not).
        let needle = self.sidebar_snippet_search.to_lowercase();
        let mut order: Vec<usize> = (0..self.snippets.len()).collect();
        self.snippets_sort.sort_items(
            &mut order,
            |&i| self.snippets[i].label.clone(),
            |&i| self.snippets[i].created_at,
        );
        let mut list = column![]
            .spacing(6)
            .padding(Padding { top: 0.0, right: 12.0, bottom: 12.0, left: 12.0 });
        let mut any = false;
        for idx in order {
            let snip = &self.snippets[idx];
            if !needle.is_empty()
                && !snip.label.to_lowercase().contains(&needle)
                && !snip.command.to_lowercase().contains(&needle)
            {
                continue;
            }
            any = true;
            list = list.push(snippet_row(
                idx,
                &snip.label,
                &snip.command,
                self.hovered_snippet_card == Some(idx),
            ));
        }
        if !any {
            list = list.push(sidebar_placeholder(t("no_matches")));
        }

        // Built-in "global snippet": type the host's stored password +
        // Enter (e.g. to answer a sudo prompt). Shown only for a live SSH
        // session; the click no-ops with a toast if no password is stored.
        let ssh_active = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.active().ssh_session.is_some())
            .unwrap_or(false);
        let sudo_row: Element<'_, Message> = if ssh_active {
            container(
                button(
                    container(
                        dir_row(vec![
                            iced_fonts::lucide::shield_check().size(13).color(c.accent).into(),
                            Space::new().width(8).into(),
                            text(t("apply_sudo_password")).size(12).color(c.text_primary).into(),
                        ])
                        .align_y(iced::Alignment::Center),
                    )
                    .padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 10.0 })
                    .width(Length::Fill),
                )
                .on_press(Message::ApplySudoPassword)
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
                }),
            )
            .padding(Padding { top: 0.0, right: 12.0, bottom: 8.0, left: 12.0 })
            .into()
        } else {
            Space::new().height(0).into()
        };

        let base = column![header, sudo_row, scrollable(list).height(Length::Fill)]
            .width(Length::Fill)
            .height(Length::Fill);

        if self.sidebar_sort_open {
            use crate::state::{ListSort, SortMenuKind};
            let menu = container(column![
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::LabelAsc,
                    iced_fonts::lucide::arrow_down_a_z::<iced::Theme, iced::Renderer>(),
                    "sort_label_asc",
                    self.snippets_sort == ListSort::LabelAsc,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::LabelDesc,
                    iced_fonts::lucide::arrow_down_z_a::<iced::Theme, iced::Renderer>(),
                    "sort_label_desc",
                    self.snippets_sort == ListSort::LabelDesc,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::NewestFirst,
                    iced_fonts::lucide::calendar_arrow_down::<iced::Theme, iced::Renderer>(),
                    "sort_newest_first",
                    self.snippets_sort == ListSort::NewestFirst,
                ),
                crate::widgets::sort_menu_row(
                    SortMenuKind::Snippets,
                    ListSort::OldestFirst,
                    iced_fonts::lucide::calendar_arrow_up::<iced::Theme, iced::Renderer>(),
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
            .on_press(Message::ToggleSidebarSort)
            .into();
            iced::widget::Stack::new()
                .push(base)
                .push(backdrop)
                .push(positioned)
                .into()
        } else {
            base.into()
        }
    }

    /// Compact New / Edit snippet form rendered inline in the Snippets
    /// tab (reuses the same `snippet_*` state + messages as the workspace
    /// editor). A back arrow cancels; Save persists and returns to the
    /// list; Delete shows only when editing an existing snippet.
    fn sidebar_snippet_editor(&self) -> Element<'_, Message> {
        let c = OryxisColors::t();
        let title = if self.snippet_editing_id.is_some() {
            t("edit_snippet")
        } else {
            t("new_snippet")
        };

        let header = dir_row(vec![
            chat_header_btn(iced_fonts::lucide::arrow_left(), Message::HideSnippetPanel),
            Space::new().width(6).into(),
            text(title).size(14).color(c.text_primary).into(),
        ])
        .align_y(iced::Alignment::Center);

        let label_input: Element<'_, Message> =
            iced::widget::text_input("restart-nginx", &self.snippet_label)
                .on_input(Message::SnippetLabelChanged)
                .padding(8)
                .size(13)
                .style(crate::widgets::rounded_input_style)
                .into();
        // Multi-line, auto-grows with content; container caps the height
        // (~8 lines) and then it scrolls internally.
        let command_input: Element<'_, Message> = container(
            iced::widget::text_editor(&self.snippet_command)
                .placeholder("sudo systemctl restart nginx")
                .on_action(Message::SnippetCommandAction)
                .padding(8)
                .size(13)
                .height(Length::Shrink)
                .style(crate::widgets::rounded_editor_style),
        )
        .max_height(180.0)
        .into();

        let error: Element<'_, Message> = if let Some(err) = &self.snippet_error {
            text(err.clone()).size(11).color(c.error).into()
        } else {
            Space::new().height(0).into()
        };

        let save = button(
            container(text(t("save")).size(13).color(c.button_text))
                .center_x(Length::Fill)
                .padding(Padding { top: 9.0, right: 0.0, bottom: 9.0, left: 0.0 }),
        )
        .on_press(Message::SaveSnippet)
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

        if let Some(edit_id) = self.snippet_editing_id
            && let Some(idx) = self.snippets.iter().position(|s| s.id == edit_id)
        {
            let delete = button(
                container(text(t("delete")).size(13).color(OryxisColors::t().error))
                    .center_x(Length::Fill)
                    .padding(Padding { top: 9.0, right: 0.0, bottom: 9.0, left: 0.0 }),
            )
            .on_press(Message::RequestDeleteSnippet(idx))
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
            form = form.push(Space::new().height(8)).push(delete);
        }

        form.width(Length::Fill).height(Length::Fill).into()
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
        .into()
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
    .into()
}

/// One row in the Snippets tab. Label + a single ellipsized line of the
/// command read inline; the Edit / Paste / Run actions float over the
/// trailing edge and only appear on hover (see the card-icon convention
/// in CLAUDE.md). `hovered` is `self.hovered_snippet_card == Some(idx)`.
fn snippet_row<'a>(
    idx: usize,
    label: &'a str,
    command: &'a str,
    hovered: bool,
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
    let info = column![
        text(label).size(13).color(c.text_primary),
        text(preview).size(11).color(c.text_muted),
    ]
    .spacing(2)
    .width(Length::Fill);

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
                action_btn(iced_fonts::lucide::pencil(), Message::EditSnippet(idx), t("edit_snippet")),
                action_btn(iced_fonts::lucide::clipboard_copy(), Message::PasteSnippet(idx), t("snippet_paste")),
                action_btn(iced_fonts::lucide::play(), Message::RunSnippet(idx), t("snippet_run")),
            ])
            .spacing(2)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding { top: 3.0, right: 5.0, bottom: 3.0, left: 5.0 })
        .style(|_| container::Style {
            background: Some(Background::Color(OryxisColors::t().bg_selected)),
            border: Border { radius: Radius::from(8.0), ..Default::default() },
            ..Default::default()
        });
        let overlay = container(actions)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center)
            .padding(Padding { top: 0.0, right: 6.0, bottom: 0.0, left: 0.0 });
        iced::widget::Stack::new().push(card).push(overlay).into()
    } else {
        card.into()
    };

    MouseArea::new(row_el)
        .on_enter(Message::SnippetCardHovered(idx))
        .on_exit(Message::SnippetCardUnhovered)
        .into()
}
