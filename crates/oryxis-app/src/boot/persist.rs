use crate::app::Oryxis;

impl Oryxis {
    /// Best-effort persist a key/value pair to the vault. Logs failures
    /// instead of bubbling them up so a flaky disk doesn't take the
    /// whole settings panel down, the worst case is the user has to
    /// re-type on next boot.
    pub(crate) fn persist_setting(&self, key: &str, value: &str) {
        if let Some(vault) = &self.vault
            && let Err(e) = vault.set_setting(key, value)
        {
            tracing::warn!("failed to persist setting {key}: {e}");
        }
    }

    /// Persist the window geometry (last windowed size + outer position +
    /// the maximized / fullscreen flags) so the next launch reopens the
    /// window exactly as the user left it, on the same monitor. Called
    /// from every exit path (window close, tray quit, update restart,
    /// renderer relaunch), on the maximize / fullscreen toggles and on
    /// focus loss as a crash-safe checkpoint. Plaintext settings rows,
    /// so this works while the vault is locked.
    pub(crate) fn persist_window_geometry(&self) {
        let w = self.window_windowed_size.width.round() as u32;
        let h = self.window_windowed_size.height.round() as u32;
        self.persist_setting("window_width", &w.to_string());
        self.persist_setting("window_height", &h.to_string());
        // No Moved event ever fired (fresh session on Wayland, or the
        // window was never dragged after a restore): keep whatever the
        // previous run stored rather than overwriting it with nothing.
        if let Some(pos) = self.window_windowed_pos {
            let x = pos.x.round() as i32;
            let y = pos.y.round() as i32;
            self.persist_setting("window_pos_x", &x.to_string());
            self.persist_setting("window_pos_y", &y.to_string());
        }
        self.persist_setting(
            "window_maximized",
            if self.window_maximized { "true" } else { "false" },
        );
        self.persist_setting(
            "window_fullscreen",
            if self.window_fullscreen { "true" } else { "false" },
        );
    }

    /// Persist the current column template (visibility + order + widths) so
    /// new panes/tabs inherit it across restarts.
    pub(crate) fn persist_sftp_columns(&self) {
        self.persist_setting("sftp_columns", &self.sftp_chrome.columns_template.visibility_storage());
        self.persist_setting("sftp_col_order", &self.sftp_chrome.columns_template.order_storage());
        self.persist_setting("sftp_col_widths", &self.sftp_chrome.columns_template.width_storage());
    }

    /// Snapshot the currently-pinned tabs (those with a reopenable spec) to
    /// the `pinned_tabs` setting so they reappear, dormant, next launch.
    /// Cloud / ephemeral pinned tabs have no spec and are skipped.
    pub(crate) fn persist_pinned_tabs(&self) {
        // De-duplicate by pin identity: a dormant placeholder and its
        // freshly-reopened live tab can briefly coexist (or a missed
        // replacement can leave both around), and persisting both
        // turns into duplicate chips on the next boot.
        let mut seen = std::collections::HashSet::new();
        // Persist in `tab_order` (the drag-reorderable display order) so the
        // restored pinned sequence matches what the user arranged, across both
        // terminal and SFTP tabs.
        let mut specs: Vec<crate::state::PinnedTabSpec> = Vec::new();
        for r in &self.tab_order {
            let spec = match r {
                crate::state::TabRef::Terminal(id) => self
                    .tabs
                    .iter()
                    .find(|t| t._id == *id)
                    .filter(|t| t.pinned)
                    .and_then(|t| t.pin_spec()),
                crate::state::TabRef::Sftp(id) => self
                    .sftp_tabs
                    .iter()
                    .position(|t| t.id == *id)
                    .filter(|&i| self.sftp_tabs[i].pinned)
                    .and_then(|i| self.sftp_pin_spec(i)),
                // Transient by design (issue #120): a restart should open
                // on real work, not on the settings screen.
                crate::state::TabRef::Panel(_) => None,
            };
            if let Some(spec) = spec
                && seen.insert(spec.dedupe_key())
            {
                specs.push(spec);
            }
        }
        let json = serde_json::to_string(&specs).unwrap_or_else(|_| "[]".into());
        self.persist_setting("pinned_tabs", &json);
    }

    /// Recreate pinned tabs as dormant placeholders at boot. They show in the
    /// strip with their saved label but hold no live session; selecting one
    /// the first time reopens it (see `reopen_dormant_tab`). Called once data
    /// is loaded so the reopen path can resolve host ids.
    pub(crate) fn restore_pinned_tabs_dormant(&mut self) {
        let json = self
            .vault
            .as_ref()
            .and_then(|v| v.get_setting("pinned_tabs").ok().flatten());
        let Some(json) = json else { return };
        let specs: Vec<crate::state::PinnedTabSpec> =
            serde_json::from_str(&json).unwrap_or_default();
        if specs.is_empty() {
            return;
        }
        // Heal any duplicates an older version persisted: one chip
        // per pin identity.
        let mut seen = std::collections::HashSet::new();
        // Pre-seed with pinned tabs already in the strip so a *re-run* of
        // `load_data_from_vault` (it fires on connection save, vault reload,
        // sync, ...) doesn't recreate dormant duplicates of live/dormant tabs
        // that already exist.
        for t in self.tabs.iter().filter(|t| t.pinned) {
            if let Some(s) = t.pin_spec() {
                seen.insert(s.dedupe_key());
            }
        }
        let existing_sftp_keys: Vec<String> = (0..self.sftp_tabs.len())
            .filter(|&i| self.sftp_tabs[i].pinned)
            .filter_map(|i| self.sftp_pin_spec(i).map(|s| s.dedupe_key()))
            .collect();
        seen.extend(existing_sftp_keys);
        for spec in specs {
            if !seen.insert(spec.dedupe_key()) {
                continue;
            }
            let label = spec.label().to_string();
            if matches!(spec, crate::state::PinnedTabSpec::Sftp { .. }) {
                // SFTP pinned tabs restore into `sftp_tabs` as dormant chips;
                // they re-mount their panes on first focus (see SelectSftpTab).
                let mut tab = crate::state::SftpTab::new_dormant(label, spec);
                tab.pinned = true;
                // Seed `tab_order` in the persisted (interleaved terminal+SFTP)
                // order so the restored strip matches what was saved, instead of
                // reconcile grouping all terminals before all SFTP tabs.
                self.tab_order.push(crate::state::TabRef::Sftp(tab.id));
                self.sftp_tabs.push(tab);
            } else {
                let tab = crate::state::TerminalTab::new_dormant_pinned(label, spec);
                self.tab_order.push(crate::state::TabRef::Terminal(tab._id));
                self.tabs.push(tab);
            }
        }
        // The tabs sit dormant in the strip; the app still boots to its
        // default view (Hosts). We deliberately do not focus a pinned tab or
        // switch to the terminal: opening always lands on Hosts, and a
        // dormant tab only connects on an explicit select.
    }
}
