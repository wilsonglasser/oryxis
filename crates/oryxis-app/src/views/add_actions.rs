//! The "add" action catalogs: the single source of truth behind both a
//! toolbar dropdown and the matching empty state's action buttons. Two
//! surfaces, one list, so a new entry can never land on one and be
//! forgotten on the other.
//!
//! - hosts: `add_host_actions` feeds `dashboard_empty_state`.
//! - keychain: `add_key_actions` feeds "+ ADD ▾"
//!   (`build_menu_keychain_add`) and the keychain empty state
//!   (`views/keys/list.rs`). The empty state shipped with only the two
//!   key CTAs, so a vault with nothing in it had no way to reach the
//!   identity form at all (issue #148 follow-up).

use iced::widget::{button, container, text};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{KeysMessage, ShareMessage, TabsMessage, Message, Oryxis};
use crate::os_icon::BrandIcon;
use crate::theme::OryxisColors;
use crate::widgets::dir_row;

/// One entry of an add catalog: how it looks, what it says, what it
/// fires.
pub(crate) struct AddAction<'a> {
    pub(crate) icon: BrandIcon,
    pub(crate) label: &'a str,
    pub(crate) msg: Message,
    /// Icon tint: a neutral secondary for the built-in actions.
    pub(crate) color: Color,
}

impl Oryxis {
    /// Every "add a host" action available right now, in display order:
    /// import a `.oryxis` file (a full vault export or a single shared
    /// host), a new (sub)group, and export the current view (only with
    /// hosts to export). Import / export live here so they're reachable
    /// from where hosts are managed instead of being buried in
    /// Settings.
    pub(crate) fn add_host_actions(&self) -> Vec<AddAction<'_>> {
        let secondary = OryxisColors::t().text_secondary;
        // ONE import entry (owner call): the hub modal names every
        // supported source and detects the picked file's format, so
        // this menu never grows a button per client again.
        let mut actions = vec![AddAction {
            icon: iced_fonts::lucide::download().into(),
            label: crate::i18n::t("import_from_file"),
            msg: Message::Share(ShareMessage::ShowImportHub),
            color: secondary,
        }];
        // Group creation, context-symmetric and always the leading
        // entry. Inside a folder it's "New subgroup" (a child of the
        // open folder): the folder kebab offers the same action from
        // the parent view, this covers creating one while the folder
        // itself is open (its own card, and thus its kebab, isn't
        // visible there). At the vault root it's "New group" (a fresh
        // top-level folder), so an empty group can be born here instead
        // of only by typing a new name in the host editor's group combo.
        match self.active_group {
            Some(gid) => {
                actions.insert(
                    0,
                    AddAction {
                        icon: iced_fonts::lucide::folder_plus().into(),
                        label: crate::i18n::t("new_subgroup"),
                        msg: Message::Tabs(TabsMessage::NewSubgroup(gid)),
                        color: secondary,
                    },
                );
            }
            None => {
                actions.insert(
                    0,
                    AddAction {
                        icon: iced_fonts::lucide::folder_plus().into(),
                        label: crate::i18n::t("new_group"),
                        msg: Message::Tabs(TabsMessage::NewGroup),
                        color: secondary,
                    },
                );
            }
        }
        // Remote desktop has no add entry of its own: it is one of the
        // protocols in the host editor's picker, like Telnet or Serial.
        // A separate entry meant a user looking for RDP opened that
        // picker, failed to find it, and concluded there was none.
        // Export hosts: opens the share dialog with a per-folder
        // include/exclude checklist (keys-off by default), unlike the
        // full-vault export in Settings. Pre-scoped to the active
        // folder when one is open. Nothing to share with an empty
        // vault, so the entry only exists once a host does.
        if !self.connections.is_empty() {
            actions.push(AddAction {
                icon: iced_fonts::lucide::upload().into(),
                label: crate::i18n::t("export_hosts"),
                msg: Message::Share(ShareMessage::ShowExportHosts(self.active_group)),
                color: secondary,
            });
        }
        actions
    }

    /// Every "add to the keychain" action, in display order. The FIRST
    /// entry is the primary one: the empty state renders it as the hero
    /// CTA and the rest as secondary buttons, so this order is
    /// load-bearing beyond the dropdown (see `views/keys/list.rs`).
    ///
    /// "Import public key" and "Certificate" open the same import panel
    /// as "Import key" with a different field focused (a cert lives on
    /// its key, B2.1); they are separate entries because the intent is
    /// what the user is shopping for, not the panel.
    ///
    /// Unlike the host catalog nothing here is conditional: every one of
    /// the five works against an empty vault.
    pub(crate) fn add_key_actions(&self) -> Vec<AddAction<'_>> {
        let secondary = OryxisColors::t().text_secondary;
        vec![
            AddAction {
                icon: iced_fonts::lucide::sparkles().into(),
                label: crate::i18n::t("generate_key"),
                msg: Message::Keys(KeysMessage::ShowKeyGeneratePanel),
                color: secondary,
            },
            AddAction {
                icon: iced_fonts::lucide::key_round().into(),
                label: crate::i18n::t("import_key"),
                msg: Message::Keys(KeysMessage::ShowKeyPanel),
                color: secondary,
            },
            AddAction {
                icon: iced_fonts::lucide::fingerprint().into(),
                label: crate::i18n::t("import_public_key"),
                msg: Message::Keys(KeysMessage::ShowKeyPanelPublicFocus),
                color: secondary,
            },
            AddAction {
                icon: iced_fonts::lucide::badge_check().into(),
                label: crate::i18n::t("certificate"),
                msg: Message::Keys(KeysMessage::ShowKeyPanelCertFocus),
                color: secondary,
            },
            AddAction {
                icon: iced_fonts::lucide::user().into(),
                label: crate::i18n::t("new_identity"),
                msg: Message::Keys(KeysMessage::ShowIdentityPanel),
                color: secondary,
            },
        ]
    }
}

/// One catalog entry as a secondary button: outlined, muted,
/// deliberately quieter than the filled primary CTA above it. Shared by
/// the dashboard and keychain empty states so their action stacks are
/// pixel-identical; `width` is the block width the caller centers on.
pub(crate) fn secondary_action_button(action: AddAction<'_>, width: f32) -> Element<'_, Message> {
    let AddAction { icon, label, msg, color } = action;
    button(
        container(
            dir_row(vec![
                icon.view(14.0, color),
                iced::widget::Space::new().width(8).into(),
                text(label).size(13).color(OryxisColors::t().text_secondary).into(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .width(width)
        .center_x(width),
    )
    .on_press(msg)
    .width(width)
    .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
    .style(|_, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => OryxisColors::t().bg_hover,
            iced::widget::button::Status::Pressed => OryxisColors::t().bg_selected,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: iced::border::Radius::from(8.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .into()
}

/// A hairline rule with the "or" label centered in it, separating an
/// empty state's primary create path from its secondary ones.
pub(crate) fn or_divider<'a>(width: f32) -> Element<'a, Message> {
    let rule = || {
        container(iced::widget::Space::new().width(Length::Fill).height(1))
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().border)),
                ..Default::default()
            })
    };
    // Symmetric, so it needs no direction awareness.
    container(
        iced::widget::row![
            rule(),
            container(text(crate::i18n::t("or_separator")).size(12).color(OryxisColors::t().text_muted))
                .padding(Padding { top: 0.0, right: 10.0, bottom: 0.0, left: 10.0 }),
            rule(),
        ]
        .align_y(iced::Alignment::Center),
    )
    .width(width)
    .into()
}
