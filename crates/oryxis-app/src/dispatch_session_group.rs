//! `Oryxis::handle_session_group`, match arms for session groups (saved
//! split-panel arrangements): snapshot a tab into a group, edit an existing
//! group, save/delete, and open one back into a splitted tab.
//!
//! Pure tree work (snapshot / merge / prune) lives in
//! `session_group_helpers`; this file is the dispatch glue.

#![allow(clippy::result_large_err)]

use std::sync::{Arc, Mutex};

use iced::widget::pane_grid::{self, Configuration};
use iced::Task;

use oryxis_core::models::group::Group;
use oryxis_core::models::{PaneLayout, PaneSource, SessionGroup};
use oryxis_terminal::widget::TerminalState;

use crate::app::{SessionGroupMessage, Message, Oryxis, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS};
use crate::session_group_helpers::{
    apply_scripts, from_split_axis, prune_layout, rows_from_layout, snapshot_tab_layout,
};
use crate::state::{LocalShellSpec, Pane, PaneOrigin, SessionGroupForm, TerminalTab, View};

/// A host pane built but not yet connected: the SSH connect runs after the
/// whole grid is assembled.
struct PendingHost {
    pane_id: uuid::Uuid,
    conn_idx: usize,
}

/// A local pane whose PTY is already spawned; its byte stream just needs to
/// be wired to the pane.
struct PendingLocal {
    pane_id: uuid::Uuid,
    rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    /// The pane's child-exit signal, taken at spawn time because the
    /// `TerminalState` is wrapped for the pane in the same breath.
    exited: Option<tokio::sync::oneshot::Receiver<()>>,
}

#[derive(Default)]
struct OpenPending {
    hosts: Vec<PendingHost>,
    locals: Vec<PendingLocal>,
    /// (pane_id, script) for non-empty per-pane scripts; folded into
    /// `pane_script_overrides` after the grid is built.
    scripts: Vec<(uuid::Uuid, String)>,
}

impl Oryxis {
    pub(crate) fn handle_session_group(
        &mut self,
        message: SessionGroupMessage,
    ) -> Task<Message> {
        match message {
            SessionGroupMessage::ShowSaveSessionGroup(idx) => {
                self.overlay = None;
                let Some(tab) = self.tabs.get(idx) else {
                    return Task::none();
                };
                let Some((layout, rows)) = snapshot_tab_layout(tab) else {
                    // Every pane was non-referenceable (e.g. a cloud-only
                    // tab). Nothing to save; tell the user instead of
                    // opening an empty editor.
                    self.set_toast(crate::i18n::t("session_group_nothing_to_save").to_string());
                    return toast_clear(3);
                };

                let mut form = SessionGroupForm {
                    source_tab: Some(idx),
                    layout: Some(layout),
                    pane_rows: rows,
                    ..SessionGroupForm::default()
                };

                // Editing an arrangement that already came from a saved group:
                // carry its name / folder / look and seed the script rows from
                // the stored leaves (by order).
                if let Some(existing_id) = tab.session_group_id
                    && let Some(existing) =
                        self.session_groups.iter().find(|g| g.id == existing_id)
                {
                    form.editing_id = Some(existing.id);
                    form.label = existing.label.clone();
                    form.color = existing.color.clone();
                    form.icon_style = existing.icon_style.clone();
                    form.group_name = existing
                        .group_id
                        .filter(|gid| self.groups.iter().any(|g| g.id == *gid))
                        .map(|gid| oryxis_core::models::Group::path_of(&self.groups, gid))
                        .unwrap_or_default();
                    let stored = rows_from_layout(&existing.layout, &|_| String::new());
                    for (row, src) in form.pane_rows.iter_mut().zip(stored.iter()) {
                        row.script = src.script.clone();
                    }
                }

                self.open_session_group_editor(form)
            }

            SessionGroupMessage::EditSessionGroup(idx) => {
                self.overlay = None;
                let Some(group) = self.session_groups.get(idx) else {
                    return Task::none();
                };
                let connections = &self.connections;
                let resolve = |src: &PaneSource| resolve_pane_label(connections, src);
                let rows = rows_from_layout(&group.layout, &resolve);
                let form = SessionGroupForm {
                    editing_id: Some(group.id),
                    label: group.label.clone(),
                    color: group.color.clone(),
                    icon_style: group.icon_style.clone(),
                    group_name: group
                        .group_id
                        .filter(|gid| self.groups.iter().any(|g| g.id == *gid))
                        .map(|gid| oryxis_core::models::Group::path_of(&self.groups, gid))
                        .unwrap_or_default(),
                    source_tab: None,
                    layout: Some(group.layout.clone()),
                    pane_rows: rows,
                    current_pane: 0,
                };
                self.open_session_group_editor(form)
            }

            SessionGroupMessage::SessionGroupFormLabelChanged(v) => {
                self.editor_session_group.label = v;
                Task::none()
            }
            SessionGroupMessage::SessionGroupFormGroupChanged(v) => {
                self.editor_session_group.group_name = v;
                Task::none()
            }
            SessionGroupMessage::SessionGroupScriptAction(action) => {
                self.session_group_script_editor.perform(action);
                // text_editor always reports a trailing newline; drop the one
                // it appends so a single-line script doesn't gain a blank
                // Enter when injected.
                let txt = self.session_group_script_editor.text();
                let txt = txt.strip_suffix('\n').unwrap_or(&txt).to_string();
                let cur = self.editor_session_group.current_pane;
                if let Some(r) = self.editor_session_group.pane_rows.get_mut(cur) {
                    r.script = txt;
                }
                Task::none()
            }
            SessionGroupMessage::SessionGroupPaneNav(next) => {
                let len = self.editor_session_group.pane_rows.len();
                if len > 0 {
                    let cur = self.editor_session_group.current_pane;
                    let new = if next {
                        (cur + 1).min(len - 1)
                    } else {
                        cur.saturating_sub(1)
                    };
                    self.editor_session_group.current_pane = new;
                    // Current pane's text is already synced to its row on each
                    // action, so just reload the target row into the buffer.
                    let script = self
                        .editor_session_group
                        .pane_rows
                        .get(new)
                        .map(|r| r.script.clone())
                        .unwrap_or_default();
                    self.session_group_script_editor =
                        iced::widget::text_editor::Content::with_text(&script);
                }
                Task::none()
            }

            SessionGroupMessage::SessionGroupFormSave => {
                self.save_session_group_form()
            }

            SessionGroupMessage::SessionGroupFormCancel => {
                self.panels.session_group_panel = false;
                self.session_group_panel_error = None;
                Task::none()
            }

            SessionGroupMessage::ShowSessionGroupIconPicker => {
                // Reuse the host icon/color picker, seeded from the form. The
                // choice flows back into `editor_session_group` on the
                // picker's Save (deferred, like the dynamic-group form).
                let form = &self.editor_session_group;
                self.icon_picker.icon =
                    form.icon_style.clone().or_else(|| Some("boxes".to_string()));
                self.icon_picker.color = form.color.clone();
                self.icon_picker.hex_input = form.color.clone().unwrap_or_default();
                self.icon_picker.for_id = None;
                self.icon_picker.for_group_form = false;
                self.icon_picker.for_session_group = true;
                self.icon_picker.for_local_terminal = false;
                self.panels.icon_picker = true;
                Task::none()
            }

            SessionGroupMessage::DuplicateSessionGroup(idx) => {
                self.overlay = None;
                if let Some(src) = self.session_groups.get(idx).cloned() {
                    let mut dup = oryxis_core::models::SessionGroup::new(
                        format!("{} (copy)", src.label),
                        src.layout.clone(),
                    );
                    dup.group_id = src.group_id;
                    dup.color = src.color.clone();
                    dup.icon_style = src.icon_style.clone();
                    if let Some(vault) = &self.vault {
                        let _ = vault.save_session_group(&dup);
                        self.load_data_from_vault();
                    }
                }
                Task::none()
            }

            SessionGroupMessage::RequestDeleteSessionGroup(idx) => {
                if let Some(group) = self.session_groups.get(idx) {
                    let name = group.label.clone();
                    self.confirm_remove(name, Message::SessionGroup(SessionGroupMessage::DeleteSessionGroup(idx)));
                }
                Task::none()
            }
            SessionGroupMessage::DeleteSessionGroup(idx) => {
                self.overlay = None;
                if let Some(group) = self.session_groups.get(idx)
                    && let Some(vault) = &self.vault
                {
                    let _ = vault.delete_session_group(&group.id);
                    self.load_data_from_vault();
                }
                Task::none()
            }

            SessionGroupMessage::ShowSessionGroupMenu(idx) => {
                use crate::state::{OverlayContent, OverlayState};
                if matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::SessionGroupActions(i)) if *i == idx
                ) {
                    self.overlay = None;
                } else {
                    let anchor = self.keynav_take_menu_anchor();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SessionGroupActions(idx),
                        x: anchor.0,
                        y: anchor.1,
                    });
                }
                Task::none()
            }

            SessionGroupMessage::SessionGroupCardHovered(idx) => {
                self.hover.session_group_card = Some(idx);
                Task::none()
            }
            SessionGroupMessage::SessionGroupCardUnhovered(idx) => {
                self.hover.leave_session_group_card(idx);
                Task::none()
            }

            SessionGroupMessage::OpenSessionGroup(idx) => {
                self.overlay = None;
                self.open_session_group(idx)
            }
        }
    }

    /// Shared editor-open bookkeeping: claim the right-panel slot and reset
    /// any stale error.
    fn open_session_group_editor(&mut self, mut form: SessionGroupForm) -> Task<Message> {
        // Mutually exclusive right-panel slot, close other panels first.
        self.editor_flush_pending();
        self.panels.host_panel = false;
        // Drop what the host editor's eyes revealed.
        self.editor_form.sweep_secrets();
        self.panel_nav_clear();
        self.cloud_form.visible = false;
        self.cloud_dynamic_form.visible = false;
        self.cloud_discover.visible = false;
        self.group_edit.visible = false;
        // Seed the multi-line script buffer from the first pane.
        form.current_pane = 0;
        let first_script = form
            .pane_rows
            .first()
            .map(|r| r.script.clone())
            .unwrap_or_default();
        self.session_group_script_editor =
            iced::widget::text_editor::Content::with_text(&first_script);
        self.editor_session_group = form;
        self.session_group_panel_error = None;
        self.panels.session_group_panel = true;
        // Land focus in the name field so the user can type immediately
        // (otherwise the first keystrokes go nowhere, reading as broken).
        crate::widgets::focus_input(iced::widget::Id::new("session-group-name"))
    }

    fn save_session_group_form(&mut self) -> Task<Message> {
        let form = &self.editor_session_group;
        if form.label.trim().is_empty() {
            self.session_group_panel_error =
                Some(crate::i18n::t("session_group_label_required").to_string());
            return Task::none();
        }
        let Some(layout) = form.layout.clone() else {
            self.panels.session_group_panel = false;
            return Task::none();
        };

        // Find or create the folder, same convention as the host
        // editor: breadcrumb-path match first, bare label fallback, and
        // an unmatched value is materialised as a nested PATH
        // ("Prod / NewTeam" builds the chain, reusing existing
        // segments) so it can't mint a single group whose own label
        // contains the separator and impersonates a real path.
        let group_id = if !form.group_name.trim().is_empty() {
            let name = form.group_name.trim().to_string();
            let mut created = Vec::new();
            let gid = Group::resolve_or_create_path(&mut self.groups, &name, &mut created);
            if let Some(vault) = &self.vault {
                for g in &created {
                    let _ = vault.save_group(g);
                }
            }
            gid
        } else {
            None
        };

        let merged = apply_scripts(layout, &form.pane_rows);
        let now = chrono::Utc::now();

        // Update in place when editing an existing group, else create.
        let group = match form
            .editing_id
            .and_then(|id| self.session_groups.iter().find(|g| g.id == id).cloned())
        {
            Some(mut existing) => {
                existing.label = form.label.trim().to_string();
                existing.group_id = group_id;
                existing.color = form.color.clone();
                existing.icon_style = form.icon_style.clone();
                existing.layout = merged;
                existing.updated_at = now;
                existing
            }
            None => {
                let mut g = SessionGroup::new(form.label.trim().to_string(), merged);
                g.group_id = group_id;
                g.color = form.color.clone();
                g.icon_style = form.icon_style.clone();
                g
            }
        };

        let group_id_saved = group.id;
        let group_label = group.label.clone();
        let source_tab = form.source_tab;
        if let Some(vault) = &self.vault {
            match vault.save_session_group(&group) {
                Ok(()) => {
                    // Stamp the originating tab so its context menu reads
                    // "Edit group" next time, and rename it to the group.
                    if let Some(idx) = source_tab
                        && let Some(tab) = self.tabs.get_mut(idx)
                    {
                        tab.session_group_id = Some(group_id_saved);
                        tab.label = group_label.clone();
                    }
                    self.panels.session_group_panel = false;
                    self.session_group_panel_error = None;
                    self.load_data_from_vault();
                }
                Err(e) => {
                    self.session_group_panel_error = Some(e.to_string());
                }
            }
        }
        Task::none()
    }

    /// Rebuild a saved group into a single splitted tab (exact axes +
    /// ratios) and connect each pane. Dangling host references are pruned
    /// with a warning; if everything was dangling, nothing opens.
    fn open_session_group(&mut self, idx: usize) -> Task<Message> {
        let Some(group) = self.session_groups.get(idx).cloned() else {
            return Task::none();
        };

        // Prune leaves whose host reference no longer resolves.
        let conn_ids: std::collections::HashSet<uuid::Uuid> =
            self.connections.iter().map(|c| c.id).collect();
        let Some(layout) = prune_layout(group.layout.clone(), &|src| match src {
            PaneSource::Host(id) => conn_ids.contains(id),
            PaneSource::LocalShell { .. } => true,
        }) else {
            self.set_toast(crate::i18n::t("session_group_empty_on_open").to_string());
            return toast_clear(4);
        };

        // Build the full split tree with placeholder terminals up front, then
        // connect each pane afterwards.
        let mut pending = OpenPending::default();
        let config = self.build_session_pane_config(&layout, &mut pending);

        let grid = pane_grid::State::with_configuration(config);
        let Some(&focused) = grid.panes.keys().next() else {
            return Task::none();
        };
        let mut tab = TerminalTab {
            _id: uuid::Uuid::new_v4(),
            label: group.label.clone(),
            custom_name: None,
            pane_grid: grid,
            focused,
            chat_history: Vec::new(),
            sidebar_open: [false; 2],
            chat_always_run_commands: Vec::new(),
            chat_auto_run_history: Vec::new(),
            chat_auto_run_streak: 0,
            chat_loading: false,
            chat_task: None,
            chat_mode: crate::state::default_chat_mode(),
            chat_last_md_parse: None,
            chat_saved_id: None,
            chat_persisted: 0,
            ssm_keepalive: false,
            relaunch: None,
            session_group_id: Some(group.id),
            pinned: false,
            pending_reopen: None,
            inherited_pin: None,
            files_mode: false,
            files_state: Box::default(),
            broadcast: false,
        };
        // Auto-open the terminal sidebar for the restored tab: the first
        // host pane's override wins, falling back to the global setting
        // (a group tab can mix hosts; the first is the tab's identity).
        // Same region choice as the single-host connect path (issue
        // #102): the default tab's region, else right-unless-empty.
        let auto_open = tab
            .pane_grid
            .panes
            .values()
            .find_map(|p| match p.origin {
                crate::state::PaneOrigin::Host(id) => Some(id),
                _ => None,
            })
            .and_then(|id| self.connections.iter().find(|c| c.id == id))
            .map(|c| c.sidebar_auto_open.unwrap_or(self.prefs.sidebar_auto_open))
            .unwrap_or(self.prefs.sidebar_auto_open);
        if auto_open
            && let Some(side) = self.sidebar_auto_open_side()
        {
            tab.sidebar_open[side.idx()] = true;
        }
        let tab_idx = self.tabs.len();
        self.tabs.push(tab);
        self.active_tab = Some(tab_idx);
        self.active_view = View::Terminal;
        self.remember_terminal_tab_focus(tab_idx);

        // Register per-pane script overrides (consumed on connect / first
        // local output).
        for (pane_id, script) in pending.scripts {
            self.pane_script_overrides.insert(pane_id, script);
        }

        // Connect hosts + wire local PTY streams.
        let mut tasks: Vec<Task<Message>> = Vec::new();
        for h in pending.hosts {
            tasks.push(self.spawn_ssh_for_pane(h.conn_idx, tab_idx, h.pane_id));
        }
        for l in pending.locals {
            // A session-group tab is split by definition, so these are
            // exactly the local panes whose exit raises the ended card.
            tasks.push(self.local_pane_stream(l.pane_id, l.exited, l.rx));
        }
        tasks.push(self.tab_scroll_to_active());
        Task::batch(tasks)
    }

    /// Recursively turn a pruned `PaneLayout` into an iced
    /// `pane_grid::Configuration<Pane>`, spawning each leaf's placeholder
    /// terminal and recording the connect work in `pending`. `&self` (not
    /// `&mut`) so per-host palette resolution stays available; the side
    /// effects accumulate in `pending`.
    fn build_session_pane_config(
        &self,
        layout: &PaneLayout,
        pending: &mut OpenPending,
    ) -> Configuration<Pane> {
        match layout {
            PaneLayout::Split { axis, ratio, a, b } => Configuration::Split {
                axis: from_split_axis(*axis),
                ratio: *ratio,
                a: Box::new(self.build_session_pane_config(a, pending)),
                b: Box::new(self.build_session_pane_config(b, pending)),
            },
            PaneLayout::Leaf(member) => {
                let cols = DEFAULT_TERM_COLS as u16;
                let rows = DEFAULT_TERM_ROWS as u16;
                let pane = match &member.source {
                    PaneSource::Host(id) => {
                        // Pruning guarantees the connection exists.
                        let conn_idx = self
                            .connections
                            .iter()
                            .position(|c| c.id == *id)
                            .expect("pruned layout keeps only live hosts");
                        let conn = &self.connections[conn_idx];
                        let mut term = TerminalState::new_no_pty(cols, rows)
                            .expect("display-only terminal");
                        term.set_palette(self.resolve_terminal_palette_for_connection(conn));
                        term.process(
                            format!(
                                "Connecting to {} ({}:{})...\r\n",
                                conn.label, conn.hostname, conn.port
                            )
                            .as_bytes(),
                        );
                        let mut pane =
                            Pane::new(conn.label.clone(), Arc::new(Mutex::new(term)));
                        pane.origin = PaneOrigin::Host(*id);
                        pending.hosts.push(PendingHost {
                            pane_id: pane.id,
                            conn_idx,
                        });
                        if let Some(script) = non_empty(member.initial_script.as_deref()) {
                            pending.scripts.push((pane.id, script));
                        }
                        pane
                    }
                    PaneSource::LocalShell {
                        program,
                        args,
                        label,
                    } => {
                        let spawned = if program.is_empty() {
                            TerminalState::new(cols, rows, None)
                        } else {
                            TerminalState::new_with_command(cols, rows, program, args, None)
                        };
                        match spawned {
                            Ok((mut state, rx)) => {
                                state.set_palette(self.terminal_palette.clone());
                                let exited =
                                    state.pty.as_mut().and_then(|p| p.take_child_exit());
                                let mut pane =
                                    Pane::new(label.clone(), Arc::new(Mutex::new(state)));
                                pane.origin = PaneOrigin::Local(LocalShellSpec {
                                    label: label.clone(),
                                    program: program.clone(),
                                    args: args.clone(),
                                });
                                pending.locals.push(PendingLocal {
                                    pane_id: pane.id,
                                    rx,
                                    exited,
                                });
                                if let Some(script) = non_empty(member.initial_script.as_deref())
                                {
                                    pending.scripts.push((pane.id, script));
                                }
                                pane
                            }
                            Err(e) => {
                                // PTY spawn failed: show an inert pane with the
                                // error rather than dropping it from the tree.
                                let mut term = TerminalState::new_no_pty(cols, rows)
                                    .expect("display-only terminal");
                                term.process(
                                    format!("Failed to start local shell: {e}\r\n").as_bytes(),
                                );
                                let mut pane =
                                    Pane::new(label.clone(), Arc::new(Mutex::new(term)));
                                pane.origin = PaneOrigin::Local(LocalShellSpec {
                                    label: label.clone(),
                                    program: program.clone(),
                                    args: args.clone(),
                                });
                                pane
                            }
                        }
                    }
                };
                Configuration::Pane(pane)
            }
        }
    }
}

/// Trim and keep a script only when it carries something runnable.
fn non_empty(script: Option<&str>) -> Option<String> {
    script
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Display label for a saved pane source: the live host label when it still
/// exists, an explicit "deleted" marker when the reference is dangling, or the
/// captured local-shell label.
fn resolve_pane_label(
    connections: &[oryxis_core::models::Connection],
    src: &PaneSource,
) -> String {
    match src {
        PaneSource::Host(id) => connections
            .iter()
            .find(|c| c.id == *id)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| crate::i18n::t("session_group_deleted_host").to_string()),
        PaneSource::LocalShell { label, .. } => label.clone(),
    }
}

fn toast_clear(secs: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        },
        |_| Message::ToastClear,
    )
}
