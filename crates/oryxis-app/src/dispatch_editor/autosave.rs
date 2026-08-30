//! Host editor auto-save: edits to an EXISTING host persist when the
//! drawer CLOSES, so it carries no Save button and no footer legend
//! either. New hosts keep the explicit Save / Connect pair: a
//! half-typed host must never enter the vault by itself.
//!
//! **Closing is the write, and that is the design, not a shortcut.**
//! An earlier revision persisted on a 700ms debounce while the user
//! typed, and every save re-sorts `connections` (the list is ordered
//! by label) and republishes the row to sync. Two review rounds found
//! the same bug five times: a menu, a connect, an edit or a delete
//! resolving a host through a position the mid-typing save had just
//! moved; a Parent Group materialised from a half-typed word; the
//! machine's monitor ring reset per keystroke. One write per editing
//! session removes the class instead of guarding each site, and the
//! only thing it costs is the edits of a session that ends in a hard
//! crash, which the explicit Save button never protected either.
//!
//! Mechanics: `editor_autosave_kick` (called from the `Message::Editor`
//! and `Message::Settings` dispatch in `dispatch.rs`) only records the
//! post-open baseline. The flush runs on every path that takes the
//! drawer off screen, and `dispatch.rs`'s post-update net is what makes
//! "every" true: a handler that merely navigates away, focuses a tab or
//! lets another Dashboard panel shadow this one never mentions the
//! editor at all.
//!
//! Dirtiness is a SIGNATURE comparison (the built `Connection` plus the
//! row-adjacent fields), never a per-arm flag: a flag would have to be
//! remembered in every one of the ~100 field arms, and the first
//! forgotten one would silently stop saving that field. Secrets stay
//! out of the signature (their buffers must not be serialized); their
//! tri-state `touched()` flags are the dirty signal instead.

use super::*;

impl Oryxis {
    /// What the form would persist right now, as a comparable string.
    /// `updated_at` is zeroed (it is stamped per build and would read
    /// as a permanent diff); the group is compared by the TYPED path
    /// (the build runs with `persist_group: false` so a mere check
    /// never materializes group rows); `use_totp` rides along because
    /// its effect (clearing the stored secret) lives in a side column
    /// the `Connection` JSON does not carry. `None` = the form does
    /// not build (half-typed state), which is never dirty on its own.
    fn editor_form_signature(&mut self) -> Option<String> {
        let mut conn = self.connection_from_editor_form(super::GroupWrite::Skip).ok()?;
        conn.updated_at = chrono::DateTime::<chrono::Utc>::MIN_UTC;
        let json = serde_json::to_string(&conn).ok()?;
        Some(format!(
            "{json}|{}|{}",
            self.editor_form.group_name.trim(),
            self.editor_form.use_totp
        ))
    }

    fn editor_secrets_touched(&self) -> bool {
        let f = &self.editor_form;
        f.password.touched()
            || f.proxy_password.touched()
            || f.totp_secret.touched()
            || f.target_password.touched()
    }

    /// Whether the form currently OWNS a pending write: an editing
    /// session that opened and has not been closed out yet.
    ///
    /// Deliberately NOT `panels.host_panel`: the closing flush runs on
    /// the edge, when the flag is already down, and a flag test there
    /// would silently skip the very write it exists for. The baseline
    /// snapshot is the honest token instead, since it is recorded when
    /// an existing host opens and dropped when the session closes out.
    /// That also makes a stale form inert: after the drawer is gone,
    /// nothing can re-persist it over a row that sync has moved on.
    ///
    /// The id must still RESOLVE, which is what keeps a delete deleted.
    /// `DeleteConnection` closes the drawer without clearing the form
    /// (it deliberately skips its own flush, so resurrecting the row it
    /// just removed would be worse), and the post-update net then sees
    /// the visibility edge and flushes. With only an `is_some()` test
    /// here, that flush upserted the deleted id straight back into the
    /// vault: `editor_group_pending` reads the vanished row's group as
    /// `""`, so a host in ANY group was dirty by construction and came
    /// back on every delete-from-the-editor. Reproduced in the harness
    /// before this guard, gone after.
    fn editor_owns_write(&self) -> bool {
        self.editor_form
            .editing_id
            .is_some_and(|id| self.connections.iter().any(|c| c.id == id))
            && self.editor_saved_snapshot.is_some()
    }

    /// Whether the host editor is ACTUALLY on screen, which is what the
    /// post-update flush net in `dispatch.rs` watches. The flag alone
    /// is not the answer: the drawer only renders on the Dashboard with
    /// no tab focused, and it sits LAST in the Dashboard's panel chain
    /// (`views/layout/mod.rs::side_panel_open`), so a group editor
    /// opening takes the slot while `host_panel` stays true. Mirrors
    /// that chain; a condition added there belongs here.
    pub(crate) fn host_editor_visible(&self) -> bool {
        self.panels.host_panel
            && self.active_tab.is_none()
            && self.active_view == crate::state::View::Dashboard
            && !self.group_edit.visible
    }

    /// Whether the open editor holds changes the vault does not.
    pub(crate) fn editor_autosave_dirty(&mut self) -> bool {
        if !self.editor_owns_write() {
            return false;
        }
        if self.editor_secrets_touched() {
            return true;
        }
        match (self.editor_form_signature(), &self.editor_saved_snapshot) {
            (Some(sig), Some(snap)) => sig != *snap,
            // An unbuildable form (half-typed port, cleared hostname):
            // nothing coherent to persist.
            _ => false,
        }
    }

    /// Post-dispatch hook (`dispatch.rs`): record the baseline the
    /// dirty check compares against, on the first message following an
    /// open. Over-calling is safe: it only ever writes the snapshot
    /// once per opened host.
    pub(crate) fn editor_autosave_kick(&mut self) {
        if !self.panels.host_panel
            || self.editor_form.editing_id.is_none()
            || self.editor_saved_snapshot.is_some()
        {
            return;
        }
        // The open arm cleared the snapshot; this very message is the
        // first one after it (usually the open itself), so the form
        // still equals the stored row.
        self.editor_saved_snapshot = self.editor_form_signature();
    }

    /// Whether the typed Parent Group value differs from the host's
    /// stored one. Read on its own because a group change can be the
    /// ONLY change (`GroupWrite::Skip` builds the signature, so the
    /// signature cannot see it) and because the interrupted flush
    /// deliberately declines to apply it.
    fn editor_group_pending(&self) -> bool {
        if !self.editor_owns_write() {
            return false;
        }
        let stored = self
            .editor_form
            .editing_id
            .and_then(|id| self.connections.iter().find(|c| c.id == id))
            .and_then(|c| c.group_id)
            .map(|gid| oryxis_core::models::Group::path_of(&self.groups, gid))
            .unwrap_or_default();
        self.editor_form.group_name.trim() != stored.trim()
    }

    /// Persist the open editor NOW, on a path the USER concluded: the
    /// X / Esc cancel, opening another host, navigating away, focusing
    /// a tab. That gesture is what makes the typed Parent Group value
    /// an answer, so this is the commit point for it
    /// (`GroupWrite::Create`).
    pub(crate) fn editor_flush_pending(&mut self) {
        self.editor_flush_with(super::GroupWrite::Create);
    }

    /// Persist the open editor NOW on a path NOTHING concluded: the
    /// vault locks under an idle user, the window closes. The edits are theirs and are kept, but the Parent Group
    /// value stays whatever the host already had: an interrupted
    /// "Staging" must not mint a permanent, synced group named "Sta".
    pub(crate) fn editor_flush_interrupted(&mut self) {
        self.editor_flush_with(super::GroupWrite::Keep);
    }

    /// The editing session is over: the drawer just left the screen
    /// (`dispatch.rs`'s post-update net). Persists like any concluded
    /// gesture, then drops the baseline, which is what stops a form
    /// nobody can see from ever being written again.
    pub(crate) fn editor_flush_on_close(&mut self) {
        self.editor_flush_with(super::GroupWrite::Create);
        self.editor_saved_snapshot = None;
    }

    /// Shared body. Silent on an invalid form: the vault keeps the last
    /// valid save, which is the only coherent answer for a surface
    /// that is going away. A failed WRITE is not silent: it raises the
    /// inline panel error (still on screen on the gesture paths) AND a
    /// toast, and is always logged, because the surfaces that outlive
    /// neither are exactly where the loss would otherwise be silent.
    fn editor_flush_with(&mut self, groups: super::GroupWrite) {
        let group_pending = groups == super::GroupWrite::Create && self.editor_group_pending();
        if !self.editor_autosave_dirty() && !group_pending {
            return;
        }
        match self.persist_editor_form(groups) {
            Ok(_) => self.editor_autosave_settle(),
            Err(super::PersistError::Invalid(_)) => {}
            Err(super::PersistError::Vault(e)) => {
                tracing::warn!("host editor flush failed: {e}");
                self.host_panel_error = Some(e.clone());
                self.set_toast(format!("{}: {e}", crate::i18n::t("editor_autosave_failed")));
            }
        }
    }

    /// Post-persist bookkeeping: re-baseline the signature (a flush
    /// can be followed by another one, e.g. a close right after a
    /// panel swap) and return every touched secret
    /// buffer to the untouched "preserve the stored value" state (the
    /// value it holds IS the stored value now), syncing the
    /// has-a-stored-secret placeholders the views read.
    ///
    /// The three side-column flags are NOT recomputed here: the persist
    /// that just ran is the only thing that knows whether it stored the
    /// buffer or performed a DERIVED CLEAR (proxy disabled, TOTP off,
    /// script detached), and it wrote each flag to match what actually
    /// landed. Recomputing them from the buffer alone contradicted that
    /// and disabled the rescue restore: typing a proxy password and
    /// then switching the proxy off cleared the column, then set
    /// has_existing back to true, so re-enabling wrote nothing while
    /// the field still showed the secret.
    fn editor_autosave_settle(&mut self) {
        self.editor_saved_snapshot = self.editor_form_signature();
        let f = &mut self.editor_form;
        // The main password has no derived clear (no toggle governs it),
        // so the buffer IS the authority for its flag.
        if f.password.touched() {
            f.has_existing_password = !f.password.as_str().is_empty();
            let v = f.password.as_str().to_string();
            f.password.prefill(v);
        }
        for buffer in [
            &mut f.proxy_password,
            &mut f.totp_secret,
            &mut f.target_password,
        ] {
            if buffer.touched() {
                let v = buffer.as_str().to_string();
                buffer.prefill(v);
            }
        }
    }

}
