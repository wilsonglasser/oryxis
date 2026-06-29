//! `Oryxis::handle_navigation`: settings-panel-independent dispatch arms for the
//! navigation area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::too_many_lines)]

use iced::Task;

use crate::app::{Message, Oryxis};
use crate::state::View;

impl Oryxis {
    pub(crate) fn handle_navigation(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Navigation --
            Message::ChangeView(view) => {
                // Navigating away from the Shortcuts editor cancels
                // any pending capture so the next keystroke doesn't
                // silently rebind an action from another screen.
                self.editing_hotkey = None;
                // Leaving the Logs view re-arms Privacy Mode masking so a
                // revealed timeline doesn't stay exposed on the next visit.
                self.privacy_revealed = false;
                self.active_view = view;
                self.active_tab = None;
                // Drop any keyboard host selection when leaving / changing
                // the surface so a stale highlight doesn't linger.
                self.selected_nav = None;
                // Navigating to the host list (Home tab / Hosts pill)
                // returns to the root, not whichever group was last open.
                if view == View::Dashboard {
                    self.active_group = None;
                }
                // Burger menu auto-dismisses on navigation: the user
                // picked a destination, leaving the overlay open is
                // visual noise.
                self.show_burger_menu = false;
                self.show_subnav_overflow = false;
                // Lazy-load the local SFTP pane when the user first lands
                // on the view (or returns to it after the underlying dir
                // changed). Cheap enough to redo unconditionally.
                if view == View::Sftp {
                    // Back the SFTP surface with a tab entry (adopts the
                    // existing top-level `self.sftp` as the first tab). The
                    // single-tab case behaves exactly as before.
                    self.ensure_sftp_tab();
                    // Refresh whichever pane(s) are Local; remote panes
                    // ignore this (refresh_sftp_local early-returns).
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Left);
                    self.refresh_sftp_local(crate::state::SftpPaneSide::Right);
                }
                // Entering Logs re-reads the timeline from the vault:
                // rows created since boot (a session that just started
                // recording, fresh connection events) only exist in the
                // tables, not in the cached page state.
                if view == View::History
                    && let Some(vault) = &self.vault
                {
                    self.logs_total = vault.count_logs().unwrap_or(0);
                    self.logs = vault
                        .list_logs_page(self.logs_page * 50, 50)
                        .unwrap_or_default();
                    self.session_logs_total = vault.count_session_logs().unwrap_or(0);
                    self.session_logs = vault
                        .list_session_logs_page(self.session_logs_page * 50, 50)
                        .unwrap_or_default();
                }
                // Land on the view with its search field focused so the
                // user can start typing immediately (same ids as Ctrl+F).
                if let Some(id) = self.active_view_search_id() {
                    return Ok(iced::widget::operation::focus(id));
                }
                // Opening Settings directly on the (default) Interface
                // section never goes through ChangeSettingsSection, so
                // fetch the renderer readout here too.
                if view == View::Settings {
                    return Ok(self.renderer_info_task());
                }
            }
            Message::QuickHostInput(v) => {
                self.quick_host_input = v;
            }
            Message::OpenGroup(gid) => {
                self.active_group = Some(gid);
                self.host_search.clear();
                // Auto-trigger resolve when the user opens a dynamic
                // group, saves an extra click. Re-resolve when there's
                // no cache yet, or when the cached list has gone stale
                // (older than the TTL): cloud resources like ECS tasks
                // recycle, and a stale list means clicking a dead task
                // fails until a manual Refresh. A still-`Loading` or
                // `Failed` cache is left alone (don't restart in-flight
                // resolves; let the user retry a failure explicitly).
                if self.dynamic_group_needs_resolve(gid) {
                    return Ok(self
                        .handle_cloud(Message::DynamicGroupResolve(gid))
                        .unwrap_or_else(|_| Task::none()));
                }
            }
            Message::HostSearchChanged(v) => {
                self.host_search = v;
                // The filtered set just changed; drop the keyboard
                // selection so it can't point at a now-hidden host. Enter
                // still connects the top result while a search is active.
                self.selected_nav = None;
            }
            Message::HostFilterByCloudProfile(maybe_pid) => {
                self.host_filter_cloud_profile = maybe_pid;
                // Filter changed the visible set; drop the keyboard
                // selection so Enter can't connect a now-hidden host.
                self.selected_nav = None;
            }
            Message::ToggleGroupPicker(target) => {
                use crate::state::{GroupPickerTarget, OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::GroupPicker(t)) if *t == target
                );
                if already_open {
                    self.overlay = None;
                } else {
                    let bounds = match target {
                        GroupPickerTarget::DynamicFormParent => {
                            self.dynamic_form_parent_combo_bounds.get()
                        }
                        GroupPickerTarget::SessionGroupFolder => {
                            self.session_group_folder_combo_bounds.get()
                        }
                    };
                    self.group_picker_search.clear();
                    // 6 px gap below the combo. Falls back to mouse
                    // coords if the cell hasn't been populated yet
                    // (first ever open before any draw pass).
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::GroupPicker(target),
                        x: if bounds.width > 0.0 {
                            bounds.x
                        } else {
                            self.mouse_position.x
                        },
                        y: if bounds.height > 0.0 {
                            bounds.y + bounds.height + 6.0
                        } else {
                            self.mouse_position.y + 26.0
                        },
                    });
                }
            }
            Message::GroupPickerSearchChanged(v) => {
                self.group_picker_search = v;
            }
            Message::GroupPickerPick(target, label) => {
                use crate::state::{GroupPickerTarget, OverlayContent};
                match target {
                    GroupPickerTarget::DynamicFormParent => {
                        self.cloud_dynamic_form.parent_label = label;
                    }
                    GroupPickerTarget::SessionGroupFolder => {
                        self.editor_session_group.group_name = label;
                    }
                }
                if matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::GroupPicker(_))
                ) {
                    self.overlay = None;
                }
            }
            Message::ToggleSortMenu(kind) => {
                use crate::state::{OverlayContent, OverlayState, SortMenuKind};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::SortMenu(k)) if *k == kind
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor the dropdown to the trailing edge of the
                    // toolbar, just under the button row, matching the
                    // keychain "+ ADD" menu geometry. Pre-compensate for
                    // the panel-on-the-right footprint per view so the
                    // menu's right edge always lands at the visible
                    // content's right edge.
                    let panel_width = match kind {
                        SortMenuKind::Hosts => {
                            if self.show_host_panel { crate::app::PANEL_WIDTH } else { 0.0 }
                        }
                        SortMenuKind::Keys => {
                            if self.show_key_panel || self.show_identity_panel {
                                crate::app::PANEL_WIDTH
                            } else {
                                0.0
                            }
                        }
                        SortMenuKind::Snippets => {
                            if self.show_snippet_panel { crate::app::PANEL_WIDTH } else { 0.0 }
                        }
                    };
                    // Must match the `OverlayContent::SortMenu` width
                    // in `views/layout.rs` so the dropdown lands under
                    // the trigger button instead of being shifted.
                    let menu_width = 220.0_f32;
                    let toolbar_padding = 24.0_f32;
                    let x = if crate::i18n::is_rtl_layout() {
                        panel_width + toolbar_padding + menu_width
                    } else {
                        self.window_size.width
                            - panel_width
                            - toolbar_padding
                            - menu_width
                    };
                    let y = self.dashboard_dropdown_anchor_y();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SortMenu(kind),
                        x: x.max(0.0),
                        y,
                    });
                }
            }
            Message::SetListSort(kind, sort) => {
                use crate::state::SortMenuKind;
                // Selecting from the sidebar's own sort popover dismisses it
                // (harmless for the workspace overlay, which closes itself).
                self.sidebar_sort_open = false;
                let key = match kind {
                    SortMenuKind::Hosts => {
                        self.hosts_sort = sort;
                        "hosts_sort"
                    }
                    SortMenuKind::Keys => {
                        self.keys_sort = sort;
                        "keys_sort"
                    }
                    SortMenuKind::Snippets => {
                        self.snippets_sort = sort;
                        "snippets_sort"
                    }
                };
                if let Some(v) = &self.vault {
                    if let Err(e) = v.set_setting(key, sort.as_storage_str()) {
                        tracing::warn!("failed to persist sort setting {key}: {e}");
                    }
                }
                self.overlay = None;
            }
            Message::ToggleToolbarSearch => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::ToolbarSearch)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Anchor the floating field over the toolbar's search
                    // zone: at the leading edge under LTR, by its trailing
                    // edge under RTL (the render path subtracts the width).
                    let menu_w = self.toolbar_search_width();
                    let pad = 24.0_f32;
                    let panel = if self.vault_panel_open() {
                        crate::app::PANEL_WIDTH
                    } else {
                        0.0
                    };
                    let x = if crate::i18n::is_rtl_layout() {
                        (self.window_size.width - panel - pad).max(menu_w)
                    } else {
                        self.vault_rail_width() + pad
                    };
                    // Sit over the toolbar row itself (the shared anchor is
                    // tuned for dropdowns *below* the button row; back out
                    // the button height + gap to land on the row).
                    let y = (self.dashboard_dropdown_anchor_y() - 42.0).max(0.0);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::ToolbarSearch,
                        x,
                        y,
                    });
                    if let Some(id) = self.active_view_search_id() {
                        return Ok(iced::widget::operation::focus(id));
                    }
                }
            }
            Message::ToggleToolbarOverflow => {
                use crate::state::{OverlayContent, OverlayState};
                let already_open = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::ToolbarOverflow)
                );
                if already_open {
                    self.overlay = None;
                } else {
                    // Trailing-edge anchor, mirroring the sort menu so the
                    // `…` dropdown lands under the toolbar's right cluster.
                    let menu_width = self.overlay_menu_width(&OverlayState {
                        content: OverlayContent::ToolbarOverflow,
                        x: 0.0,
                        y: 0.0,
                    });
                    let pad = 24.0_f32;
                    let panel = if self.vault_panel_open() {
                        crate::app::PANEL_WIDTH
                    } else {
                        0.0
                    };
                    let x = if crate::i18n::is_rtl_layout() {
                        panel + pad + menu_width
                    } else {
                        (self.window_size.width - panel - pad - menu_width).max(0.0)
                    };
                    let y = self.dashboard_dropdown_anchor_y();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::ToolbarOverflow,
                        x,
                        y,
                    });
                }
            }
            Message::QuickHostContinue => {
                if !self.quick_host_input.is_empty() {
                    self.editor_form = self.new_connection_form();
                    self.editor_initial_command =
                        iced::widget::text_editor::Content::new();
                    self.editor_form.hostname = self.quick_host_input.clone();
                    if let Some(gid) = self.active_group
                        && let Some(g) = self.groups.iter().find(|g| g.id == gid)
                    {
                        self.editor_form.group_name = g.label.clone();
                    }
                    self.show_host_panel = true;
                    self.host_panel_error = None;
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
