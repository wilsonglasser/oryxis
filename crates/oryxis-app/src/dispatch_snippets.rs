//! `Oryxis::handle_snippets`: settings-panel-independent dispatch arms for the
//! snippets area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{SnippetMessage, Message, Oryxis};

impl Oryxis {
    /// Lowercased tags of the focused pane's saved host; `None` when
    /// the focused pane isn't a saved host or the host has no tags.
    /// Drives the snippet sidebar's filter-by-host-tags toggle.
    pub(crate) fn focused_host_tags_lower(&self) -> Option<Vec<String>> {
        let tab = self.cur_active_tab().and_then(|i| self.tabs.get(i))?;
        let tags: Vec<String> = match &tab.active().origin {
            crate::state::PaneOrigin::Host(id) => self
                .connections
                .iter()
                .find(|c| c.id == *id)?
                .tags
                .clone(),
            // Local panes resolve through the curated terminal list by
            // command identity (program + args), the same key the
            // re-scan dedup uses; the pane itself only carries the
            // spawn spec.
            crate::state::PaneOrigin::Local(spec) => {
                let key = {
                    let mut k = spec.program.clone();
                    for a in &spec.args {
                        k.push('\u{1f}');
                        k.push_str(a);
                    }
                    k
                };
                self.local_terminals
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|e| e.cmd_key() == key)?
                    .tags
                    .clone()
            }
            _ => return None,
        };
        if tags.is_empty() {
            return None;
        }
        Some(tags.iter().map(|t| t.to_lowercase()).collect())
    }

    /// Distinct snippet group names in display order (first spelling
    /// wins, sorted case-insensitively). Shared by the vault view's
    /// folder cards and the keyboard router so `NavItem::SnippetGroup`
    /// indices resolve to the same group either way.
    pub(crate) fn snippet_group_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for snip in &self.snippets {
            if let Some(g) = &snip.group
                && !names.iter().any(|x| x.eq_ignore_ascii_case(g))
            {
                names.push(g.clone());
            }
        }
        names.sort_by_key(|s| s.to_lowercase());
        names
    }

    /// Distinct snippet tags, for the vault tag-filter dropdown.
    pub(crate) fn distinct_snippet_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for snip in &self.snippets {
            for tg in &snip.tags {
                if !tags.iter().any(|x| x.eq_ignore_ascii_case(tg)) {
                    tags.push(tg.clone());
                }
            }
        }
        tags.sort_by_key(|t| t.to_lowercase());
        tags
    }

    /// Send `cmd` to the active terminal: bracketed-paste wrapped, the
    /// submit newline OUTSIDE the bracket when `run`. Ring-preserving
    /// (may be fired by the sidebar ring's own Enter).
    fn inject_snippet_text(&mut self, cmd: &str, run: bool) {
        let Some(tab_idx) = self.snippet_injection_tab() else {
            return;
        };
        self.inject_snippet_text_into(tab_idx, cmd, run);
    }

    /// The half of [`Self::inject_snippet_text`] that takes an explicit
    /// tab, for the parked path: a snippet held in the variables modal
    /// belongs to the tab it was fired at, not to whatever is active by
    /// the time the user confirms.
    pub(crate) fn inject_snippet_text_into(&mut self, tab_idx: usize, cmd: &str, run: bool) {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        let bracketed = tab
            .active()
            .terminal
            .lock()
            .map(|s| s.bracketed_paste_enabled())
            .unwrap_or(false);
        let mut payload = oryxis_terminal::wrap_paste(cmd, bracketed);
        if run {
            payload.push(b'\n');
        }
        self.write_ring_injection_to_tab(tab_idx, &payload);
    }

    /// Run/paste gate: a snippet with `{name}` placeholders parks in
    /// the variables modal instead of sending raw braces; everything
    /// else sends immediately. Returns the first input's focus task
    /// when the modal opens.
    fn snippet_send_or_prompt(&mut self, idx: usize, run: bool) -> Task<Message> {
        let Some(snip) = self.snippets.get(idx) else {
            return Task::none();
        };
        if snip.has_steps() {
            return self.snippet_run_or_prompt(idx, run);
        }
        let cmd = snip.command.clone();
        // Install scripts (issue #147) never send on the click alone:
        // whatever the path (Run or Paste, with or without variables),
        // the full body ends up parked in the careful-paste
        // confirmation, so what is about to reach the host is on
        // screen first.
        let install = snip.install.then_some(snip.id);
        let vars = crate::util::snippet_placeholders(&cmd);
        if vars.is_empty() {
            match install {
                Some(snippet_id) => {
                    if let Some(tab_idx) = self.snippet_injection_tab() {
                        self.park_install_confirm(tab_idx, snippet_id, &cmd, run);
                    }
                }
                None => self.inject_snippet_text(&cmd, run),
            }
            return Task::none();
        }
        // Pin the target now, while it still means the tab the user was
        // looking at when they fired the snippet (see `target_tab`).
        let Some(target_tab) = self
            .snippet_injection_tab()
            .and_then(|i| self.tabs.get(i))
            .map(|t| t._id)
        else {
            return Task::none();
        };
        self.pending_snippet_vars = Some(crate::state::PendingSnippetVars {
            target_tab,
            command: cmd,
            run,
            install,
            vars,
            steps: None,
        });
        crate::widgets::focus_input(iced::widget::Id::new("snippet-var-0"))
    }

    /// [`Self::snippet_send_or_prompt`] for a snippet with actions: it
    /// runs (a paste of several actions has no meaning), expanded first,
    /// its variables asked once for every command.
    fn snippet_run_or_prompt(&mut self, idx: usize, run: bool) -> Task<Message> {
        let Some(snip) = self.snippets.get(idx) else {
            return Task::none();
        };
        if !run {
            return self.show_toast(crate::i18n::t("snippet_steps_cannot_paste").to_string());
        }
        // An install script is one command shown whole before it runs
        // (#147); the editor never saves one with actions, a peer might.
        if snip.install {
            return self.show_toast(crate::i18n::t("snippet_steps_install_refused").to_string());
        }
        let label = snip.label.clone();
        let steps = match crate::snippet_run::flatten(&self.snippets, snip) {
            Ok(steps) => steps,
            Err(e) => return self.show_toast(crate::dispatch_snippet_run::plan_error_text(&label, &e)),
        };
        let Some(tab_idx) = self.snippet_injection_tab() else {
            return Task::none();
        };
        let vars = crate::snippet_run::plan_placeholders(&steps);
        if vars.is_empty() {
            return self.start_snippet_run(tab_idx, label, steps);
        }
        let Some(target_tab) = self.tabs.get(tab_idx).map(|t| t._id) else {
            return Task::none();
        };
        self.pending_snippet_vars = Some(crate::state::PendingSnippetVars {
            target_tab,
            command: String::new(),
            run: true,
            install: None,
            vars,
            steps: Some((label, steps)),
        });
        crate::widgets::focus_input(iced::widget::Id::new("snippet-var-0"))
    }

    /// Park an install script in the careful-paste confirmation
    /// (unconditionally: the guard settings gate accidents, this gates
    /// a deliberate but consequential action) and remember which
    /// snippet it was, so a confirmed run lands in the host's install
    /// memory. The `__ORYXIS_NONCE__` placeholder is resolved here,
    /// with the vault's CURRENT integration key, so the preview shows
    /// exactly what will be sent.
    fn park_install_confirm(
        &mut self,
        tab_idx: usize,
        snippet_id: uuid::Uuid,
        cmd: &str,
        run: bool,
    ) {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        let text =
            crate::shell_integration::fill_nonce(cmd, &self.shell_integration_nonce);
        self.pending_paste = Some((tab._id, text));
        self.pending_paste_install = Some((snippet_id, run));
    }

    /// A confirmed install run: write it into the host's install
    /// memory (vault + in-memory mirror). Only saved hosts have one;
    /// a quick-connect or local pane runs the script without a record.
    pub(crate) fn record_install_run_for_tab(
        &mut self,
        tab_idx: usize,
        snippet_id: uuid::Uuid,
    ) {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        let crate::state::PaneOrigin::Host(host_id) = tab.active().origin else {
            return;
        };
        if let Some(vault) = &self.vault {
            let _ = vault.record_install_run(&host_id, &snippet_id);
        }
        self.install_runs
            .insert((host_id, snippet_id), chrono::Utc::now());
    }

    /// Rebuild the Group combo options from the distinct snippet
    /// groups (first spelling wins, sorted), mirroring the host
    /// editor's `editor_parent_combo` reset. Called when either
    /// snippet editor opens.
    fn reset_snippet_group_combo(&mut self) {
        let mut labels: Vec<String> = Vec::new();
        for snip in &self.snippets {
            if let Some(g) = &snip.group
                && !labels.iter().any(|x| x.eq_ignore_ascii_case(g))
            {
                labels.push(g.clone());
            }
        }
        labels.sort_by_key(|s| s.to_lowercase());
        // The live selection travels through the widget's `selection`
        // argument at view time (sidebar_snippets.rs / snippets.rs);
        // post-refactor the State only carries the options.
        self.snippet_form.group_combo = iced::widget::combo_box::State::new(labels);
    }

    pub(crate) fn handle_snippets(
        &mut self,
        message: SnippetMessage,
    ) -> Task<Message> {
        match message {
            // -- Local shell --
            // -- Snippets --
            SnippetMessage::ShowSnippetPanel => {
                self.overlay = None;
                self.panels.snippet_panel = true;
                self.snippet_form.label.clear();
                self.snippet_form.steps = vec![crate::state::StepForm::new(
                    crate::state::StepKind::Command,
                )];
                self.snippet_form.group.clear();
                self.snippet_form.tags_input.clear();
                self.snippet_form.hotkey = None;
                self.snippet_form.hotkey_capturing = false;
                self.snippet_form.install = false;
                self.snippet_form.editing_id = None;
                self.snippet_form.error = None;
                self.reset_snippet_group_combo();
            }
            SnippetMessage::HideSnippetPanel => {
                self.panels.snippet_panel = false;
                self.snippet_form.hotkey_capturing = false;
            }
            SnippetMessage::SnippetLabelChanged(v) => self.snippet_form.label = v,
            SnippetMessage::SnippetGroupChanged(v) => self.snippet_form.group = v,
            SnippetMessage::SnippetTagsChanged(v) => self.snippet_form.tags_input = v,
            SnippetMessage::ToggleSnippetInstall => {
                self.snippet_form.install = !self.snippet_form.install;
            }
            m @ (SnippetMessage::SnippetStepCommandAction(..)
            | SnippetMessage::SnippetStepAdd
            | SnippetMessage::SnippetStepRemove(_)
            | SnippetMessage::SnippetStepMoveUp(_)
            | SnippetMessage::SnippetStepMoveDown(_)
            | SnippetMessage::SnippetStepKindPicked(..)
            | SnippetMessage::SnippetStepActionPicked(..)
            | SnippetMessage::SnippetStepSnippetPicked(..)
            | SnippetMessage::SnippetStepPatternChanged(..)
            | SnippetMessage::SnippetStepTimeoutChanged(..)
            | SnippetMessage::SnippetStepSecondsChanged(..)
            | SnippetMessage::SnippetStepFileNameChanged(..)
            | SnippetMessage::SnippetStepToggleRegex(_)
            | SnippetMessage::SnippetStepToggleContinue(_)
            | SnippetMessage::SnippetStepSaveModePicked(..)
            | SnippetMessage::SnippetStepSaveContentPicked(..)) => {
                self.handle_snippet_step_edit(m);
            }
            SnippetMessage::ToggleSnippetTagFilter => {
                self.prefs.snippet_tag_filter = !self.prefs.snippet_tag_filter;
                self.persist_setting(
                    "snippet_tag_filter",
                    if self.prefs.snippet_tag_filter { "true" } else { "false" },
                );
            }
            SnippetMessage::ShowSnippetTagFilterMenu => {
                use crate::state::{OverlayContent, OverlayState};
                let already = matches!(
                    self.overlay.as_ref().map(|o| &o.content),
                    Some(OverlayContent::SnippetTagFilter)
                );
                if already {
                    self.overlay = None;
                } else {
                    // Anchor under the tag-filter button rather than the
                    // cursor (mirrors `ShowHostTagFilterMenu`). `x` is the
                    // leading edge; the render subtracts the menu width
                    // under RTL, so hand it the button's leading edge.
                    let b = self.snippet_tag_filter_btn_bounds.get();
                    let (x, y) = if b.width > 0.0 {
                        let lead = if crate::i18n::is_rtl_layout() {
                            b.x + b.width
                        } else {
                            b.x
                        };
                        (lead, b.y + b.height + 6.0)
                    } else {
                        (self.cur_mouse().x, self.cur_mouse().y + 26.0)
                    };
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SnippetTagFilter,
                        x,
                        y,
                    });
                }
            }
            SnippetMessage::ToggleSnippetTagFilterTag(tag) => {
                // Multi-select, dropdown stays open (backdrop closes).
                match self.cur_nav()
                    .snippet_filter_tags
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case(&tag))
                {
                    Some(i) => {
                        self.nav.snippet_filter_tags.remove(i);
                    }
                    None => self.nav.snippet_filter_tags.push(tag),
                }
                self.keynav.focus = None;
            }
            SnippetMessage::ClearSnippetTagFilter => {
                self.nav.snippet_filter_tags.clear();
                self.overlay = None;
                self.keynav.focus = None;
            }
            SnippetMessage::OpenSnippetGroup(name) => {
                self.nav.active_snippet_group = Some(name);
                self.nav.snippet_search.clear();
                self.keynav.focus = None;
            }
            SnippetMessage::CloseSnippetGroup => {
                self.nav.active_snippet_group = None;
                self.keynav.focus = None;
            }
            SnippetMessage::OpenSidebarSnippetGroup(name) => {
                self.sidebar_snippet_group = Some(name);
                // The row list is about to change shape; land the ring
                // back at the top on the next engage.
                self.keynav.sidebar_selected = None;
            }
            SnippetMessage::CloseSidebarSnippetGroup => {
                self.sidebar_snippet_group = None;
                self.keynav.sidebar_selected = None;
            }
            SnippetMessage::ShowSnippetMenu(idx) => {
                use crate::state::{OverlayContent, OverlayState};
                // Toggle: clicking the kebab again (or on the same card)
                // dismisses the popup, mirroring the host-card menu.
                if self.snippet_context_menu == Some(idx) {
                    self.snippet_context_menu = None;
                    self.overlay = None;
                } else {
                    self.snippet_context_menu = Some(idx);
                    let anchor = self.keynav_take_menu_anchor();
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::SnippetActions(idx),
                        x: anchor.0,
                        y: anchor.1,
                    });
                }
            }
            SnippetMessage::EditSnippet(idx) => {
                // Reached from the card kebab menu, close the popup.
                self.snippet_context_menu = None;
                self.overlay = None;
                if let Some(snip) = self.snippets.get(idx) {
                    self.panels.snippet_panel = true;
                    self.snippet_form.label = snip.label.clone();
                    self.snippet_form.steps = snip
                        .actions()
                        .iter()
                        .map(crate::state::StepForm::from_step)
                        .collect();
                    self.snippet_form.group = snip.group.clone().unwrap_or_default();
                    self.snippet_form.tags_input = snip.tags.join(", ");
                    self.snippet_form.hotkey = snip
                        .hotkey
                        .as_deref()
                        .and_then(crate::hotkeys::HotkeyBinding::parse);
                    self.snippet_form.hotkey_capturing = false;
                    self.snippet_form.install = snip.install;
                    self.snippet_form.editing_id = Some(snip.id);
                    self.snippet_form.error = None;
                    self.reset_snippet_group_combo();
                }
            }
            SnippetMessage::SaveSnippet => {
                let steps = match self.snippet_form_steps() {
                    Ok(steps) => steps,
                    Err(e) => {
                        self.snippet_form.error = Some(e);
                        return Task::none();
                    }
                };
                let mut snip = if let Some(id) = self.snippet_form.editing_id {
                    self.snippets.iter().find(|s| s.id == id).cloned()
                        .unwrap_or_else(|| oryxis_core::models::snippet::Snippet::new("", ""))
                } else {
                    oryxis_core::models::snippet::Snippet::new("", "")
                };
                snip.label = self.snippet_form.label.clone();
                snip.set_actions(steps);
                snip.group = {
                    let g = self.snippet_form.group.trim();
                    (!g.is_empty()).then(|| g.to_string())
                };
                snip.tags = crate::util::parse_tags(&self.snippet_form.tags_input);
                snip.hotkey = self.snippet_form.hotkey.map(|b| b.serialize());
                snip.install = self.snippet_form.install;
                if let Some(vault) = &self.vault {
                    match vault.save_snippet(&snip) {
                        Ok(()) => {
                            self.panels.snippet_panel = false;
                            self.snippet_form.error = None;
                            self.load_data_from_vault();
                        }
                        Err(e) => self.snippet_form.error = Some(e.to_string()),
                    }
                }
            }
            SnippetMessage::RequestDeleteSnippet(idx) => {
                if let Some(snip) = self.snippets.get(idx) {
                    let name = snip.label.clone();
                    self.confirm_remove(name, Message::Snippet(SnippetMessage::DeleteSnippet(idx)));
                }
            }
            SnippetMessage::DeleteSnippet(idx) => {
                // Reached from the card kebab menu or the edit panel,
                // close the popup either way.
                self.snippet_context_menu = None;
                self.overlay = None;
                if let Some(snip) = self.snippets.get(idx) {
                    let id = snip.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_snippet(&id);
                        self.panels.snippet_panel = false;
                        self.load_data_from_vault();
                    }
                }
            }
            SnippetMessage::RunSnippet(idx) => {
                return self.snippet_send_or_prompt(idx, true);
            }
            SnippetMessage::ApplySudoPassword => {
                // Decrypt the focused pane's host password and type it +
                // Enter. Same injection path as the password-suggest
                // popup (issue #117): `send_password_to_pane` bypasses
                // the command-history mirror, never broadcasts, and
                // scrubs the buffer after the write.
                //
                // The host is resolved from the PANE's origin, not from
                // the tab label: a split tab holds one label and several
                // hosts, so matching by label sent the wrong host's
                // password (or none) on every split.
                let toast_key = (|| {
                    let tab_idx = self.snippet_injection_tab()?;
                    let pane_id = self.tabs.get(tab_idx)?.active().id;
                    let conn_id = match &self.tabs.get(tab_idx)?.active().origin {
                        crate::state::PaneOrigin::Host(id) => *id,
                        _ => return None,
                    };
                    let pw = self
                        .vault
                        .as_ref()
                        .and_then(|v| v.get_connection_password(&conn_id).ok().flatten())
                        .filter(|p| !p.is_empty())?;
                    self.send_password_to_pane(pane_id, pw);
                    Some("sudo_password_sent")
                })()
                .unwrap_or("no_stored_password");
                self.set_toast(crate::i18n::t(toast_key).to_string());
                return Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
                    },
                    |_| Message::ToastClear,
                );
            }
            SnippetMessage::PasteSnippet(idx) => {
                // Same injection path as RunSnippet, but without the
                // trailing newline so the user reviews and presses
                // Enter themselves.
                return self.snippet_send_or_prompt(idx, false);
            }
            SnippetMessage::SnippetVarChanged(i, v) => {
                if let Some(pending) = self.pending_snippet_vars.as_mut()
                    && let Some(slot) = pending.vars.get_mut(i)
                {
                    slot.1 = v;
                }
            }
            SnippetMessage::ConfirmSnippetVars => {
                if let Some(mut pending) = self.pending_snippet_vars.take() {
                    if let Some((label, steps)) = pending.steps.take() {
                        // Gone means gone, as below.
                        let Some(tab_idx) = self.tab_index_by_id(pending.target_tab) else {
                            return Task::none();
                        };
                        let steps = crate::snippet_run::substitute(steps, &pending.vars);
                        return self.start_snippet_run(tab_idx, label, steps);
                    }
                    let cmd =
                        crate::util::substitute_snippet_vars(&pending.command, &pending.vars);
                    // Gone means gone: the tab that asked for this snippet
                    // was closed while the modal was up, and the nearest
                    // other tab is a different host. Dropping the send is
                    // the only safe answer.
                    if let Some(tab_idx) = self.tab_index_by_id(pending.target_tab) {
                        match pending.install {
                            // An install script goes through the body
                            // confirmation next, vars resolved (#147).
                            Some(snippet_id) => self.park_install_confirm(
                                tab_idx,
                                snippet_id,
                                &cmd,
                                pending.run,
                            ),
                            None => {
                                self.inject_snippet_text_into(tab_idx, &cmd, pending.run)
                            }
                        }
                    }
                }
            }
            SnippetMessage::CancelSnippetVars => {
                self.pending_snippet_vars = None;
            }
            m @ (SnippetMessage::SnippetRunTimer(..)
            | SnippetMessage::SnippetRunFileWritten(..)
            | SnippetMessage::SnippetClipboardResolved(..)
            | SnippetMessage::CancelSnippetRun(_)) => {
                return self.handle_snippet_run(m).unwrap_or_else(crate::dispatch::unrouted);
            }
            SnippetMessage::SnippetHotkeyCaptureStart => {
                self.snippet_form.hotkey_capturing = true;
            }
            SnippetMessage::SnippetHotkeyClear => {
                self.snippet_form.hotkey = None;
                self.snippet_form.hotkey_capturing = false;
            }
        }
        Task::none()
    }
}
