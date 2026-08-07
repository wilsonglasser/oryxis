//! Login automation rows of the host editor (issue #122).
//!
//! The picker attaches a shared `LoginScript` to this host, the
//! variable rows fill its `{placeholders}` for this host, and the
//! target-password field holds the credential the script types at the
//! asset's own prompt. The inline draft sub-form creates a script
//! without leaving the editor, because the moment a user needs one is
//! while they are looking at the host that cannot log in.

use super::*;

/// Sentinel shown when the host has no automation. A real script named
/// the same thing is not a problem: the picker resolves by position
/// against the option list, not by parsing this string back.
pub(crate) const LOGIN_SCRIPT_OFF: &str = "login_script_off";
/// Sentinel for "create one".
pub(crate) const LOGIN_SCRIPT_NEW: &str = "login_script_new";

impl Oryxis {
    /// Display strings for the login-automation combo, in the order the
    /// picker shows them: off, every saved script, then "new".
    pub(crate) fn login_script_options(&self) -> Vec<String> {
        let mut opts = Vec::with_capacity(self.login_scripts.len() + 2);
        opts.push(crate::i18n::t(LOGIN_SCRIPT_OFF).to_string());
        opts.extend(self.login_scripts.iter().map(|s| s.name.clone()));
        opts.push(crate::i18n::t(LOGIN_SCRIPT_NEW).to_string());
        opts
    }

    /// The combo's current display string for this host.
    pub(crate) fn login_script_selected(&self) -> String {
        if self.editor_form.login_script_draft.is_some() {
            return crate::i18n::t(LOGIN_SCRIPT_NEW).to_string();
        }
        self.editor_form
            .login_script_id
            .and_then(|id| self.login_scripts.iter().find(|s| s.id == id))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| crate::i18n::t(LOGIN_SCRIPT_OFF).to_string())
    }

    /// The `{placeholder}` names the selected script references, in
    /// first-appearance order, paired with the value stored for this
    /// host. Empty when no script is attached.
    pub(crate) fn login_script_variables(&self) -> Vec<(String, String)> {
        let Some(script) = self
            .editor_form
            .login_script_id
            .and_then(|id| self.login_scripts.iter().find(|s| s.id == id))
        else {
            return Vec::new();
        };
        crate::util::login_script_placeholders(&script.steps)
            .into_iter()
            .map(|(name, default)| {
                let value = self
                    .editor_form
                    .login_script_vars
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(default);
                (name, value)
            })
            .collect()
    }

    /// Whether the attached script ever types the target password, so
    /// the field only appears when it would actually be used.
    pub(crate) fn login_script_uses_target_password(&self) -> bool {
        self.editor_form
            .login_script_id
            .and_then(|id| self.login_scripts.iter().find(|s| s.id == id))
            .is_some_and(|s| {
                s.steps.iter().any(|step| {
                    matches!(
                        step.send,
                        oryxis_core::login_script::SendPayload::Secret(
                            oryxis_core::login_script::SecretRef::TargetPassword
                        )
                    )
                })
            })
    }

    pub(super) fn handle_editor_login_script(
        &mut self,
        message: EditorMessage,
    ) -> Task<Message> {
        match message {
            EditorMessage::EditorLoginScriptChanged(choice) => {
                let off = crate::i18n::t(LOGIN_SCRIPT_OFF);
                let new = crate::i18n::t(LOGIN_SCRIPT_NEW);
                if choice == off {
                    self.editor_form.login_script_id = None;
                    self.editor_form.login_script_vars.clear();
                    self.editor_form.login_script_draft = None;
                } else if choice == new {
                    self.editor_form.login_script_draft =
                        Some(crate::state::LoginScriptDraft::new(
                            crate::state::ScriptTemplate::JumpServer,
                        ));
                } else if let Some(script) =
                    self.login_scripts.iter().find(|s| s.name == choice)
                {
                    self.editor_form.login_script_id = Some(script.id);
                    self.editor_form.login_script_draft = None;
                    // Drop values whose variable the new script does not
                    // have, so switching scripts cannot smuggle a stale
                    // answer into an unrelated prompt.
                    let names: Vec<String> =
                        crate::util::login_script_placeholders(&script.steps)
                            .into_iter()
                            .map(|(n, _)| n)
                            .collect();
                    self.editor_form
                        .login_script_vars
                        .retain(|(n, _)| names.contains(n));
                }
                self.rebuild_editor_combos();
            }
            EditorMessage::EditorLoginScriptComboOpened => {
                self.rebuild_editor_combos();
            }
            EditorMessage::EditorLoginScriptVarChanged(name, value) => {
                match self
                    .editor_form
                    .login_script_vars
                    .iter_mut()
                    .find(|(n, _)| *n == name)
                {
                    Some(slot) => slot.1 = value,
                    None => self.editor_form.login_script_vars.push((name, value)),
                }
            }
            EditorMessage::EditorTargetPasswordChanged(v) => {
                self.editor_form.target_password.set(v.into_inner());
            }
            EditorMessage::EditorToggleTargetPasswordVisibility => {
                if self.editor_form.target_password_visible {
                    // Hide: drop a prefilled (untouched) stored plaintext
                    // right away; typed edits stay masked in the buffer.
                    if !self.editor_form.target_password.touched() {
                        self.editor_form.target_password.clear();
                    }
                    self.editor_form.target_password_visible = false;
                } else {
                    // Reveal on demand: a stored target password is
                    // decrypted into the buffer only for the moment it is
                    // shown (prefill stays untouched, so an unedited
                    // field still preserves the stored value on save).
                    if !self.editor_form.target_password.touched()
                        && let Some(id) = self.editor_form.editing_id
                        && let Some(pw) = self.vault.as_ref()
                            .and_then(|v| v.get_connection_target_password(&id).ok().flatten())
                    {
                        self.editor_form.target_password.prefill(pw);
                    }
                    self.editor_form.target_password_visible = true;
                }
            }
            EditorMessage::EditorScriptDraftTemplateChanged(choice) => {
                let template = if choice == crate::i18n::t("login_script_tpl_jumpserver") {
                    crate::state::ScriptTemplate::JumpServer
                } else {
                    crate::state::ScriptTemplate::Bastion
                };
                // Switching template re-seeds the prompts: the point of
                // picking JumpServer is not to type its three strings.
                self.editor_form.login_script_draft = Some({
                    let name = self
                        .editor_form
                        .login_script_draft
                        .as_ref()
                        .map(|d| d.name.clone())
                        .unwrap_or_default();
                    let mut draft = crate::state::LoginScriptDraft::new(template);
                    draft.name = name;
                    draft
                });
                self.rebuild_editor_combos();
            }
            EditorMessage::EditorScriptDraftNameChanged(v) => {
                if let Some(d) = &mut self.editor_form.login_script_draft {
                    d.name = v;
                }
            }
            EditorMessage::EditorScriptDraftPromptChanged(field, v) => {
                if let Some(d) = &mut self.editor_form.login_script_draft {
                    match field {
                        crate::state::ScriptPromptField::Asset => d.asset_prompt = v,
                        crate::state::ScriptPromptField::User => d.user_prompt = v,
                        crate::state::ScriptPromptField::Credential => d.password_prompt = v,
                    }
                }
            }
            EditorMessage::EditorScriptDraftCreate => {
                let Some(draft) = self.editor_form.login_script_draft.clone() else {
                    return Task::none();
                };
                let name = draft.name.trim().to_string();
                if name.is_empty() {
                    self.host_panel_error =
                        Some(crate::i18n::t("login_script_name_required").to_string());
                    return Task::none();
                }
                let steps = draft.preset().build();
                // An all-blank preset would attach an automation that
                // never does anything, which reads as a broken feature
                // rather than an empty one.
                if let Err(e) = oryxis_core::login_script::ScriptRunner::validate(&steps) {
                    self.host_panel_error =
                        Some(format!("{}: {e}", crate::i18n::t("login_script_invalid")));
                    return Task::none();
                }
                let mut script = oryxis_core::models::LoginScript::new(name);
                script.steps = steps;
                if let Some(vault) = &self.vault
                    && let Err(e) = vault.save_login_script(&script)
                {
                    self.host_panel_error = Some(e.to_string());
                    return Task::none();
                }
                self.host_panel_error = None;
                self.editor_form.login_script_id = Some(script.id);
                self.editor_form.login_script_draft = None;
                self.login_scripts.push(script);
                self.login_scripts.sort_by(|a, b| a.name.cmp(&b.name));
                self.rebuild_editor_combos();
            }
            EditorMessage::EditorScriptDraftCancel => {
                self.editor_form.login_script_draft = None;
                self.host_panel_error = None;
                self.rebuild_editor_combos();
            }
            // Routed here by the parent; anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
