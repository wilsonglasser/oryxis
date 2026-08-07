//! The Shortcuts editor: capturing a chord, resetting one, resetting all.
//!
//! Capture is armed here and answered by the routers in `shortcuts`,
//! which is why a mouse press is a settings message: it can be a
//! binding being recorded rather than one being fired.

use super::*;

impl Oryxis {
    pub(super) fn handle_settings_hotkeys(
        &mut self,
        message: SettingsMessage,
    ) -> Result<Task<Message>, SettingsMessage> {
        match message {
            SettingsMessage::StartEditingHotkey(action, slot) => {
                self.editing_hotkey = Some((action, slot));
            }
            SettingsMessage::MouseButtonPressed(button) => {
                return Ok(self.handle_mouse_button_press(button));
            }
            SettingsMessage::ResetHotkey(action) => {
                let mut defaults = crate::hotkeys::default_bindings();
                match defaults.remove(&action) {
                    Some(d) => self.hotkey_bindings.insert(action, d),
                    None => self.hotkey_bindings.remove(&action),
                };
                // Empty value persists the absence of an override, so
                // future boots rehydrate to the default. Same
                // semantics as deleting the row, and distinct from the
                // UNBOUND token a deliberate unbind writes.
                self.persist_setting(&format!("hotkey_{}", action.id()), "");
            }
            SettingsMessage::ResetAllHotkeys => {
                self.hotkey_bindings = crate::hotkeys::default_bindings();
                for action in crate::hotkeys::HotkeyAction::all() {
                    self.persist_setting(&format!("hotkey_{}", action.id()), "");
                }
            }
            SettingsMessage::ToggleSecretVisibility(field) => {
                if self.revealed_secrets.remove(&field) {
                    // Hiding: the host editor's stored proxy password was
                    // decrypted into the buffer on reveal; drop it right
                    // away. Typed buffers (AI key, vault passwords,
                    // export/share) keep their text.
                    if field == crate::state::SecretField::ProxyPassword
                        && !self.editor_form.proxy_password.touched()
                    {
                        self.editor_form.proxy_password.clear();
                    }
                } else {
                    self.revealed_secrets.insert(field);
                    // Revealing a stored proxy password on demand: decrypt
                    // it into the buffer only for the moment it is shown
                    // (prefill stays untouched, so an unedited field still
                    // preserves the stored value on save).
                    if field == crate::state::SecretField::ProxyPassword
                        && !self.editor_form.proxy_password.touched()
                        && let Some(id) = self.editor_form.editing_id
                        && let Some(pw) = self.vault.as_ref()
                            .and_then(|v| v.get_proxy_password(&id).ok().flatten())
                    {
                        self.editor_form.proxy_password.prefill(pw);
                    }
                }
            }
            m => return Err(m),
        }
        Ok(Task::none())
    }
}
