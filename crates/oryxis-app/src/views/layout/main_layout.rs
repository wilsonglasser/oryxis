//! Root layout: main_layout. Split out of views/layout/mod.rs.

use super::*;
use crate::messages::MonitorMessage;
use iced::widget::column;

/// Widget id of the tab-rename dialog's input, focused on open (see
/// `|v| Message::Tabs(TabsMessage::StartRenameTab(v))` / `StartRenameSftpTab` in `dispatch_tabs.rs`).
pub(crate) const TAB_RENAME_INPUT_ID: &str = "tab-rename-input";

impl Oryxis {
    pub(crate) fn view_main(&self) -> Element<'_, Message> {
        let base = self.build_base();

        // Edge/corner resize handles, only when the window isn't
        // maximized or in immersive fullscreen (no borders to grab in
        // either case). Placed as the topmost stack layer so they win
        // over tab-bar buttons near the frame, while the Space in the
        // middle is pass-through.
        let resize_overlay: Option<Element<'_, Message>> =
            if self.window_maximized || self.window_fullscreen {
                None
            } else {
                Some(resize_border())
            };
        self.layer_modals(base, resize_overlay)
    }

    /// Assemble the chrome-wrapped main-window content (`base`): the tab
    /// bar (or bottom-dock chrome), the active-view content router with its
    /// vault sub-nav / rail and side panel, the accent hairline, and the
    /// status bar, composed in a single background container. The modal /
    /// overlay layering is applied on top by `layer_modals`.
    fn build_base(&self) -> Element<'_, Message> {
        let immersive = self.window_fullscreen;
        // Opt-in docking (Settings -> Interface -> Tab bar position):
        // `bottom` moves the strip above the status bar; `left` /
        // `right` dock it as a vertical list on that window edge
        // (issue #87). In every docked mode the top row shrinks to a
        // slim chrome bar (burger + drag area + window buttons), so
        // the titlebar affordances stay where every OS puts them.
        let tab_pos =
            crate::views::tab_bar::TabBarPos::from_setting(&self.prefs.tab_bar_position);
        let bottom_tabs = tab_pos == crate::views::tab_bar::TabBarPos::Bottom;
        let side_tabs = tab_pos.is_side();
        // The side dock can hide the top bar entirely (`side_hide_top_bar`):
        // the titlebar contract moves into the strip's header row.
        let side_hidden_bar = side_tabs && self.prefs.side_hide_top_bar;
        let tab_bar: Element<'_, Message> = if immersive || side_hidden_bar {
            Space::new().into()
        } else if bottom_tabs || side_tabs {
            self.view_top_chrome_bar()
        } else {
            self.view_tab_bar()
        };
        let content = self.view_content();
        // Status bar is opt-out (Interface → Show status bar) and
        // also suppressed in immersive fullscreen. Carried as an Option
        // because the side dock's full-height mode moves it inside the
        // content column instead of the window-wide bottom slot.
        let mut status_bar: Option<Element<'_, Message>> =
            (self.prefs.show_status_bar && !immersive).then(|| self.view_status_bar());

        // Tab-bar bottom hairline. When a connection tab is active and
        // it has a per-host accent color, paint the hairline 2 px and
        // tint it that color (JetBrains-style "respiração" of the
        // active project). Falls back to the global accent for tabs
        // without a per-host color, and the neutral border for non-
        // connection screens so settings / dashboard don't look like
        // they belong to whichever host happened to be open last.
        let accent_tint: Option<Color> = if self.prefs.tab_accent_line {
            // Run the host colour through the same contrast validator the
            // tab text uses: a near-background brand (AlmaLinux black on a
            // dark theme, a pale one on a light theme) would paint an
            // invisible line, which reads as "the setting is broken", the
            // exact #79 class this line belongs to.
            Some(crate::theme::readable_accent_on(
                self.top_accent_tint(),
                OryxisColors::t().bg_sidebar,
            ))
        } else {
            None
        };
        let (hair_height, hair_color) = match accent_tint {
            Some(c) => (2.0_f32, c),
            None => (1.0_f32, OryxisColors::t().border),
        };
        let h_separator: Element<'_, Message> = if immersive {
            Space::new().into()
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
        let vertical_rail = self.prefs.nav_orientation == "vertical";
        // Horizontal pill strip pinned above the content. The hidden
        // placeholder is a Shrink Space on purpose: a zero-FIXED Space
        // is void-filtered out of the column and the content would
        // change child index between vault and non-vault views (see the
        // slot skeleton note below).
        let sub_nav: Element<'_, Message> = if in_vault_area && !vertical_rail {
            self.view_vault_sub_nav()
        } else {
            Space::new().into()
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
        // ── Constant-shape chrome tree ──
        // Every dock mode fills the SAME slot skeleton (unused slots are
        // zero-sized Spaces): iced keys widget state, scrollable offsets
        // included, by tree position, so if the tab bar position or the
        // side-dock toggles reshaped the tree, every scrollable in the
        // content (including the Settings page the toggle was clicked
        // on) would snap back to the top.
        //
        // CRITICAL: the placeholder must be `Space::new()` (Shrink), NOT
        // `.width(0).height(0)`. This iced fork's Column/Row/Stack `push`
        // silently DROPS children whose size hint is void (a Fixed(0.0)
        // axis), so a zero-fixed Space never enters the child list, the
        // slot count varies by mode after all, and the positional diff
        // pairs every following subtree against the wrong state (all the
        // stateless wrappers share one tag, so nothing catches it; the
        // first stateful widget inside, a scrollable, silently loses its
        // offset). A Shrink Space passes the void filter and still lays
        // out at zero pixels.
        let empty = || -> Element<'_, Message> { Space::new().into() };
        let chrome_sep = || -> Element<'_, Message> {
            // Neutral 1 px separator under the slim chrome bar of the
            // docked layouts, so it still reads as a titlebar.
            container(Space::new().height(1.0))
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(OryxisColors::t().border)),
                    ..Default::default()
                })
                .into()
        };
        let mut slot_top: Element<'_, Message> = empty();
        let mut slot_top_sep: Element<'_, Message> = empty();
        let mut slot_left: Element<'_, Message> = empty();
        let mut slot_left_sep: Element<'_, Message> = empty();
        let mut slot_right_sep: Element<'_, Message> = empty();
        let mut slot_right: Element<'_, Message> = empty();
        let mut slot_inner_status: Element<'_, Message> = empty();
        let mut slot_bottom_sep: Element<'_, Message> = empty();
        let mut slot_bottom_strip: Element<'_, Message> = empty();
        let mut slot_status: Element<'_, Message> = empty();
        if !immersive {
            if side_tabs {
                // Side docking: the vertical strip sits on the chosen
                // PHYSICAL edge, the user picked "left" / "right"
                // explicitly, so RTL must not flip it (hence the plain
                // slot Row below, not dir_row), with the accent
                // hairline standing between strip and content.
                if !side_hidden_bar {
                    slot_top = tab_bar;
                    slot_top_sep = chrome_sep();
                }
                // Vertical twin of `h_separator`: the accent washes top
                // -> bottom (bright at the chrome fading toward the
                // status bar), matching the strip's own wash direction.
                let v_separator: Element<'_, Message> =
                    container(Space::new().width(hair_height))
                        .height(Length::Fill)
                        .style(move |_| container::Style {
                            background: Some(match accent_tint {
                                Some(c) => Background::Gradient(iced::Gradient::Linear(
                                    iced::gradient::Linear::new(iced::Radians(
                                        std::f32::consts::PI,
                                    ))
                                    .add_stop(0.0, c)
                                    .add_stop(0.85, Color { a: 0.0, ..c }),
                                )),
                                None => Background::Color(hair_color),
                            }),
                            ..Default::default()
                        })
                        .into();
                let strip = self.view_side_tab_strip();
                if tab_pos == crate::views::tab_bar::TabBarPos::Left {
                    slot_left = strip;
                    slot_left_sep = v_separator;
                } else {
                    slot_right_sep = v_separator;
                    slot_right = strip;
                }
                // Full-height strip: the status bar moves inside the
                // content column, so the strip runs to the window's
                // bottom edge instead of sitting on a window-wide bar.
                if self.prefs.side_full_height
                    && let Some(sb) = status_bar.take()
                {
                    slot_inner_status = sb;
                }
            } else if bottom_tabs {
                // Bottom docking: the accent hairline (the active
                // host's "respiração") moves with the strip, sitting on
                // its top edge.
                slot_top = tab_bar;
                slot_top_sep = chrome_sep();
                slot_bottom_sep = h_separator;
                slot_bottom_strip = self.view_bottom_tab_strip();
            } else {
                slot_top = tab_bar;
                slot_top_sep = h_separator;
            }
            if let Some(sb) = status_bar.take() {
                slot_status = sb;
            }
        }
        let center: Element<'_, Message> =
            iced::widget::Column::with_children(vec![body, slot_inner_status])
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        let middle: Element<'_, Message> = iced::widget::Row::with_children(vec![
            slot_left,
            slot_left_sep,
            center,
            slot_right_sep,
            slot_right,
        ])
        .height(Length::Fill)
        .into();
        let layout = column![
            slot_top,
            slot_top_sep,
            middle,
            slot_bottom_sep,
            slot_bottom_strip,
            slot_status,
        ];

        // The window-wide backdrop, dropped while a translucent terminal
        // is on screen: it sits under the terminal, so painting it would
        // mean the terminal's alpha reveals the app's own background
        // instead of the desktop. Every piece of chrome around the
        // terminal (tab strip, status bar, sidebars, separators) paints
        // its own background, which is what keeps them readable with
        // nothing behind them.
        let backdrop = if self.terminal_backdrop_alpha().is_some() {
            None
        } else {
            Some(Background::Color(OryxisColors::t().bg_primary))
        };
        let base: Element<'_, Message> = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: backdrop,
                ..Default::default()
            })
            .into();
        base
    }

    /// Layer every modal, dropdown, context menu and floating overlay on
    /// top of `base`, in strict precedence order (the first matching
    /// surface wins and short-circuits). Returns `base` wrapped in a
    /// single-child `Stack` when nothing is open, keeping the tree depth
    /// constant so scrollable offsets survive a modal open / close.
    fn layer_modals<'a>(
        &'a self,
        base: Element<'a, Message>,
        resize_overlay: Option<Element<'a, Message>>,
    ) -> Element<'a, Message> {
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
        if self.panels.burger_menu {
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
        if self.panels.subnav_overflow {
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
        if self.panels.share_dialog {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_share_dialog(),
                    Some(Message::Share(ShareMessage::ShareDismiss)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // The one-entry Import hub: supported-format list + the
        // detect-on-pick file button.
        if self.panels.import_hub {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_import_hub_dialog(),
                    Some(Message::Share(ShareMessage::ImportHubDismiss)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // SSH config import preview. Lists every parsed host with a
        // checkbox so the user picks which to add; hosts whose label
        // already exists are flagged and start unticked.
        if self.panels.ssh_import_dialog {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_ssh_import_dialog(),
                    Some(Message::Share(ShareMessage::SshImportDismiss)),
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
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_error_dialog(dialog),
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
            return self.layer_cloud_import_confirm(base, resize_overlay);
        }

        // Snippet-variables prompt: a snippet with `{name}` placeholders
        // parks here so the values are filled before anything reaches
        // the session. Same dialog shell as the careful paste below.
        if let Some(ref pending) = self.pending_snippet_vars {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_snippet_vars_dialog(pending),
                    Some(Message::Snippet(SnippetMessage::CancelSnippetVars)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // ssh-agent per-signature confirmation: a security prompt with a
        // 60s deny-by-default timeout, so it takes priority over the
        // lower-stakes modals below. Clicking outside denies (safe
        // default), the same as pressing Deny.
        if let Some(ref card) = self.agent.pending_confirm {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_agent_confirm_dialog(card),
                    Some(Message::Agent(AgentMessage::AgentConfirmDecision { allow: false, always: false })),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // "A highlight rule wants to run a snippet" (C6): the same class
        // of prompt as the agent one above, and for the same reason, so
        // it sits with it. Clicking outside refuses.
        if let Some(ref card) = self.trigger_confirm {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_trigger_confirm_dialog(card),
                    Some(Message::Terminal(TerminalMessage::TriggerConfirmDecision(
                        false,
                    ))),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // "Open this link?" (Ctrl+click in a remote pane): same family as
        // the two prompts above, and layered with them, because what
        // raised it is also remote output. Clicking outside opens
        // nothing.
        if let Some(ref card) = self.link_confirm {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_link_confirm_dialog(card),
                    Some(Message::Terminal(TerminalMessage::TerminalLinkDecision(false))),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Careful-paste confirmation: a clipboard paste containing a line
        // break is parked in `pending_paste` and previewed here (line
        // count + first lines) before anything reaches the session, so a
        // hidden trailing newline can't auto-run a command.
        if let Some((_, ref pending)) = self.pending_paste {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_paste_dialog(pending),
                    Some(Message::Terminal(TerminalMessage::CancelPendingPaste)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Tab rename modal, shown after the user picks "Rename" from a
        // tab's context menu. Same shape as the folder rename below; an
        // empty name restores the automatic label.
        if let Some((_tab_ref, ref input)) = self.tab_rename {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_tab_rename_dialog(input),
                    Some(Message::Tabs(TabsMessage::CancelTabRename)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Folder rename modal, shown after the user picks "Rename" from
        // the folder context menu.
        if let Some((_gid, ref input)) = self.folder_rename {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_folder_rename_dialog(input),
                    Some(Message::Tabs(TabsMessage::CancelFolderModal)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Folder delete confirmation, three-way choice instead of a yes/no
        // since destroying hosts vs only the folder are very different
        // intentions and deserve explicit affordances.
        if let Some(gid) = self.folder_delete {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_folder_delete_dialog(gid),
                    Some(Message::Tabs(TabsMessage::CancelFolderModal)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // "Clear all" confirmation for the Logs view: states exactly
        // what gets wiped (recordings + connection events) before the
        // irreversible ClearLogs runs.
        if self.clear_history_confirm {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_clear_history_dialog(),
                    Some(Message::History(HistoryMessage::CancelClearHistory)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // "Kill the process on this port" confirmation (issue #96):
        // remote, irreversible and able to take down a live service, so
        // it blocks input and nothing reaches the host until it is
        // confirmed. Backdrop = Cancel.
        if let Some(pending) = &self.monitor.kill {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_monitor_kill_dialog(pending),
                    Some(Message::Monitor(MonitorMessage::CancelKillPort)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Manual-lock confirmation: Lock Vault tears down every live SSH
        // session and tab, so the button asks first. Backdrop / Esc /
        // Cancel all decline (the safe default); only the Lock button
        // commits.
        if self.vault_ui.lock_confirm {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.build_lock_confirm_dialog(),
                    Some(Message::Vault(VaultMessage::CancelLockVaultConfirm)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // New-tab picker (opens via the "+" button in the tab bar).
        if self.panels.new_tab_picker {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_new_tab_picker(),
                    Some(Message::Tabs(TabsMessage::HideNewTabPicker)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Tab-jump modal, Termius-style "Jump to" list. Opens via the
        // ⋯ button in the tab bar or the global Ctrl+J shortcut.
        if self.panels.tab_jump {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_tab_jump_modal(),
                    Some(Message::Tabs(TabsMessage::HideTabJump)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Command palette (C4), VS Code-style Ctrl+Shift+P fuzzy action
        // search. Opens via the global hotkey; same overlay shell as the
        // tab-jump modal above.
        if self.palette.open {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_command_palette(),
                    Some(Message::Tabs(TabsMessage::HideCommandPalette)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Icon/color picker (from the host editor). Intentionally NOT routed
        // through `widgets::modal_overlay`: it injects a color-popover layer
        // into its own Stack, which the simple helper can't host. Stays
        // mouse-safe via `opaque` and keyboard-safe via `any_modal_blocks_input`.
        if self.panels.icon_picker {
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
        if self.panels.chain_editor {
            let on_scrim = if self.chain_editor_adding {
                Message::Editor(EditorMessage::ChainEditorCancelAdd)
            } else {
                Message::Editor(EditorMessage::CloseChainEditor)
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
        if self.panels.theme_picker {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_terminal_theme_picker(),
                    Some(Message::Editor(EditorMessage::EditorCloseThemePicker)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Highlight-rule editor (C6), opened from the rule list in
        // Settings -> Terminal or from a host's own list in the editor
        // panel. One modal for both: the form carries the scope it
        // commits to. Backdrop click cancels, like every other form
        // modal.
        if self.highlight_rule_editor_open() {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_highlight_rule_modal(),
                    Some(Message::Settings(SettingsMessage::HighlightRuleCancelEdit)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Custom terminal theme editor (from the "+" card / edit affordance
        // in Settings -> Terminal). Exempt from `modal_overlay` (nested color
        // popover in its own Stack); mouse-safe via `opaque`, keyboard-safe
        // via `any_modal_blocks_input`.
        if self.theme_ui.editor.is_some() {
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
        // AHEAD of the gallery below, because the gallery is where the
        // Import card now lives: this chain returns on the first match,
        // so with the gallery first the import modal opened into a state
        // nothing rendered (field report: "Import stopped working").
        // `ESC_ORDER` carries the same order so Esc answers what is
        // actually on screen.
        if self.panels.theme_import {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_theme_import_modal(),
                    Some(Message::Settings(SettingsMessage::ThemeImportClose)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Global terminal-theme gallery (Settings -> Terminal). The grid
        // it holds used to sit inline and dominate the page.
        if self.panels.terminal_theme_gallery {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.terminal_theme_gallery(),
                    Some(Message::Settings(SettingsMessage::CloseTerminalThemeGallery)),
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

        // Import-a-UI-theme modal (Settings -> Interface "Import" card).
        if self.panels.ui_theme_import {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.view_ui_theme_import_modal(),
                    Some(Message::Settings(SettingsMessage::UiThemeImportClose)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // App-theme gallery (Settings -> Interface). Last of the theme
        // modals: the UI editor and the UI import above are both opened
        // FROM it, so they have to win the chain.
        if self.panels.ui_theme_gallery {
            return wrap_with_resize(
                crate::widgets::modal_overlay(
                    base,
                    self.ui_theme_gallery(),
                    Some(Message::Settings(SettingsMessage::CloseUiThemeGallery)),
                    0.0,
                ),
                resize_overlay,
            );
        }

        // Note: the update modal is rendered at the top-level `view()`
        // dispatcher (see `Oryxis::view`) so it overlays the lock screen
        // too. Don't re-render it here.

        if let Some(ref overlay) = self.overlay {
            return self.layer_overlay_menu(base, resize_overlay, overlay);
        }

        // SFTP row right-click menu, rendered at the layout root so the
        // window-coord click position lines up with the menu origin
        // without having to compensate for the title + tab bar height.
        if let Some(ref row_menu) = self.sftp.row_menu {
            return self.layer_sftp_row_menu(base, resize_overlay, row_menu);
        }

        // Floating drag ghost, rendered last so it sits above
        // everything else. Tracks the cursor while a file drag is in
        // flight; non-interactive so it doesn't swallow the release
        // event that ends the drag.
        //
        // Two gestures feed it and one press can arm BOTH (an SFTP file
        // row arms the cross-pane transfer and the drag-out together),
        // so exactly one draws. The internal drag keeps its own rule
        // (it appears on cross-pane activation, unchanged); the
        // drag-out raises the same pill at its movement threshold,
        // which is what puts something under the cursor while the
        // gesture is still over the window. Same pill either way, so a
        // file row that armed both reads as one continuous drag.
        if let Some(drag) = &self.sftp.drag
            && drag.active
        {
            return self.layer_drag_ghost(base, resize_overlay, &drag.label);
        }
        if let Some(arm) = &self.drag_out_arm
            && arm.dragging()
        {
            return self.layer_drag_ghost(base, resize_overlay, &arm.label);
        }

        // A tab dragged off the strip and over the content area: the
        // split anchor it proposes, plus its ghost chip (issue #112).
        // The strip stops drawing the chip at the same boundary, so
        // exactly one of the two is up at any moment.
        if self.tab_drag.is_some_and(|d| d.active) && !self.cursor_in_tab_strip() {
            return self.layer_tab_drop(base, resize_overlay);
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
            .on_press(Message::Tabs(TabsMessage::WindowFullscreenToggle))
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
}
