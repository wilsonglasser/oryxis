//! Overlay layers that build their own `Stack` around `base` (the
//! positioned overlay / context menus, and the SFTP row menu + drag
//! ghost). Split out of views/layout/main_layout.rs; each returns the
//! finished, resize-wrapped `Element`.

use super::*;
use iced::widget::{column, row};

impl Oryxis {
    pub(crate) fn layer_overlay_menu<'a>(
        &'a self,
        base: Element<'a, Message>,
        resize_overlay: Option<Element<'a, Message>>,
        overlay: &'a OverlayState,
    ) -> Element<'a, Message> {
        let menu = self.render_overlay_menu(overlay);

        // The `+` split popover is hover-driven: it opens on hover and
        // dismisses on mouse-out (`SplitMenuLeave`), so a click-dismiss
        // backdrop is redundant for it. Worse, a full-screen backdrop sits
        // on top of the `+` button and swallows the click, so the first
        // click on `+` only closes the popover and a second is needed to
        // open a new tab. Skip the backdrop here so the click reaches the
        // button. Every other overlay through this path is click-triggered
        // and keeps its click-outside dismissal.
        //
        // The password-suggest popup (#117) skips it for the same
        // reason from the other direction: it floats over a live
        // terminal, and a full-screen backdrop would turn every click
        // on the session underneath into "close the popup" instead of
        // a click on the terminal. It dismisses through the pane's own
        // interactions (FocusPane, typing, tab switch) instead.
        let is_hover_popover = matches!(
            overlay.content,
            OverlayContent::SplitMenu | OverlayContent::PasswordSuggest { .. }
        );

        // Position the menu, clamping to window bounds to prevent clipping.
        // Under RTL, anchor by the menu's right edge so it grows toward
        // the leading (left) side, mirroring native OS dropdown behavior.
        // Width must match the value used in `render_overlay_menu` so
        // clamping lines up with the rendered box.
        let menu_width = self.overlay_menu_width(overlay);
        let menu_height = self.overlay_menu_height(overlay);
        let raw_x = if crate::i18n::is_rtl_layout() {
            overlay.x - menu_width
        } else {
            overlay.x
        };
        let x = raw_x.min(self.window_size.width - menu_width).max(0.0);
        // Vertically the box flips over its anchor when it does not fit
        // under it, and falls back to the clamp; see `overlay_menu_y`.
        let y = self.overlay_menu_y(overlay, menu_height);
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
            .on_press(Message::Tabs(TabsMessage::HideOverlayMenu))
            .into();
            stack = stack.push(backdrop);
        }
        wrap_with_resize(
            stack
                .push(positioned_menu)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            resize_overlay,
        )
    }

    /// SFTP row right-click menu, rendered at the layout root so the
    /// window-coordinate click position lines up with the menu origin.
    pub(crate) fn layer_sftp_row_menu<'a>(
        &'a self,
        base: Element<'a, Message>,
        resize_overlay: Option<Element<'a, Message>>,
        row_menu: &'a crate::state::SftpRowMenu,
    ) -> Element<'a, Message> {
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
        // Archive context: what the probe found on the mounted host (or
        // the in-process codecs for a local pane) decides which archive
        // actions the menu can offer for this row.
        let archive_ctx = {
            use oryxis_archive::names::ArchiveKind;
            use oryxis_archive::remote as remote_cmd;
            let in_zip = src_pane.zip.is_some();
            let name = crate::dispatch_sftp_archive::base_name(&row_menu.path);
            let kind = ArchiveKind::from_name(&name);
            let (extractable, compress_zip, compress_tgz) = if source_is_remote {
                match src_pane.archive_tools {
                    Some((shell, tools)) => (
                        kind.is_some_and(|k| remote_cmd::can_extract(shell, tools, k)),
                        remote_cmd::can_compress(shell, tools, ArchiveKind::Zip),
                        remote_cmd::can_compress(shell, tools, ArchiveKind::TarGz),
                    ),
                    None => (false, false, false),
                }
            } else {
                (
                    matches!(
                        kind,
                        Some(ArchiveKind::Zip | ArchiveKind::TarGz | ArchiveKind::Tar)
                    ),
                    true,
                    true,
                )
            };
            crate::views::sftp::RowArchiveCtx {
                in_zip,
                copy_out_ready: in_zip
                    && other.zip.is_none()
                    && (!other.is_remote || other.client.is_some()),
                browsable: !in_zip && matches!(kind, Some(ArchiveKind::Zip)),
                extractable: !in_zip && extractable,
                compress_zip: !in_zip && compress_zip,
                compress_tgz: !in_zip && compress_tgz,
            }
        };
        // Record the menu's rows into the modal keynav layer (only one
        // such surface renders per frame) so the SFTP row menu is
        // keyboard-navigable.
        self.modal_nav_reset();
        let menu = crate::views::sftp::row_context_menu_box(
            self,
            row_menu,
            cross_pane_ready,
            source_is_remote,
            other_is_remote,
            other_label,
            selection_count_same_pane,
            archive_ctx,
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
        .on_press(Message::Sftp(SftpMessage::SftpRowMenuClose))
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
            self,
            row_menu,
            cross_pane_ready,
            source_is_remote,
            other_is_remote,
            selection_count_same_pane,
            archive_ctx,
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
        wrap_with_resize(
            Stack::new()
                .push(base)
                .push(backdrop)
                .push(positioned_menu)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            resize_overlay,
        )
    }

    /// A tab being dragged out of the strip and over the content area
    /// (issue #112): the split anchor it is currently proposing, painted
    /// as the space the arriving session will occupy, plus the tab's own
    /// ghost chip now free in both axes.
    ///
    /// Lives at the window root for two reasons: the proposal's rectangle
    /// is already in window coordinates, and the strip's own Stack is
    /// clipped to the bar, which is exactly why the chip could never
    /// follow the cursor down here. Purely decorative, no `MouseArea`
    /// anywhere, so the release that ends the drag reaches the app.
    pub(crate) fn layer_tab_drop<'a>(
        &'a self,
        base: Element<'a, Message>,
        resize_overlay: Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let mut stack = Stack::new().push(base);
        let accent = OryxisColors::t().accent;
        if let Some((_, proposal)) = self.tab_drop_proposal() {
            let rect = proposal.highlight;
            // Inset by the border width so the outline reads as the edge
            // OF the region rather than a line spilling into the pane
            // next door.
            let fill: Element<'_, Message> = container(Space::new())
                .width(Length::Fixed((rect.width - 4.0).max(0.0)))
                .height(Length::Fixed((rect.height - 4.0).max(0.0)))
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color { a: 0.18, ..accent })),
                    border: Border {
                        color: accent,
                        width: 2.0,
                        radius: Radius::from(4.0),
                    },
                    ..Default::default()
                })
                .into();
            let positioned: Element<'_, Message> = column![
                Space::new().height(rect.y + 2.0),
                row![Space::new().width(rect.x + 2.0), fill],
            ]
            .into();
            stack = stack.push(positioned);
        }
        // The chip itself. Centered on the cursor horizontally like the
        // strip does, and lifted half a row so the pointer sits on it
        // rather than under it.
        if let Some((ghost, ghost_w)) = self.strip_drag_ghost_el(
            crate::views::tab_bar::TAB_NATURAL_WIDTH,
            false,
            &self.privacy_terms(),
        ) {
            let x = (self.mouse_position.x - ghost_w / 2.0)
                .min(self.window_size.width - ghost_w)
                .max(0.0);
            let y = (self.mouse_position.y - crate::views::tab_bar::TAB_HEIGHT / 2.0)
                .min(self.window_size.height - crate::views::tab_bar::TAB_HEIGHT)
                .max(0.0);
            let positioned: Element<'_, Message> =
                column![Space::new().height(y), row![Space::new().width(x), ghost]].into();
            stack = stack.push(positioned);
        }
        wrap_with_resize(stack.width(Length::Fill).height(Length::Fill).into(), resize_overlay)
    }

    /// Floating drag ghost for an in-flight file drag, tracking the
    /// cursor above everything else and non-interactive so it never
    /// swallows the release that ends the drag. Serves both drags a file
    /// row can start (cross-pane transfer, drag-out) from one pill, so
    /// the gesture looks the same until it lands.
    pub(crate) fn layer_drag_ghost<'a>(
        &'a self,
        base: Element<'a, Message>,
        resize_overlay: Option<Element<'a, Message>>,
        label: &str,
    ) -> Element<'a, Message> {
        let ghost = crate::views::sftp::drag_ghost(label);
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
        wrap_with_resize(
            Stack::new()
                .push(base)
                .push(positioned)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            resize_overlay,
        )
    }
}
