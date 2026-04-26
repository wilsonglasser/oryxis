//! `Oryxis::handle_keys` — match arms for the Keys + Identities
//! panels: import/edit/delete keys, manage identities, keychain menu.

#![allow(clippy::result_large_err)]

use iced::widget::text_editor;
use iced::Task;

use oryxis_core::models::identity::Identity;

use crate::app::{Message, Oryxis};
use crate::state::{OverlayContent, OverlayState, View};

impl Oryxis {
    pub(crate) fn handle_keys(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Keys --
            Message::ShowKeyPanel => {
                // Also navigate to the Keys screen — the import panel is rendered
                // inside view_keys(), so the user needs to be there to see it
                // (e.g. when they click "+ Key" from the host editor).
                self.active_view = View::Keys;
                self.active_tab = None;
                self.show_key_panel = true;
                self.key_import_label.clear();
                self.key_import_content = text_editor::Content::new();
                self.key_import_pem.clear();
                self.key_error = None;
                self.key_success = None;
                self.editing_key_id = None;
                self.key_context_menu = None;
                self.overlay = None;
            }
            Message::HideKeyPanel => {
                self.show_key_panel = false;
                self.editing_key_id = None;
            }
            Message::KeyImportLabelChanged(v) => self.key_import_label = v,
            Message::KeyContentAction(action) => {
                self.key_import_content.perform(action);
                self.key_import_pem = self.key_import_content.text();
            }
            Message::BrowseKeyFile => {
                return Ok(Task::perform(
                    tokio::task::spawn_blocking(|| {
                        let file = rfd::FileDialog::new()
                            .set_title("Select SSH Private Key")
                            .pick_file();
                        match file {
                            Some(path) => {
                                let filename = path
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "imported-key".into());
                                let content = std::fs::read_to_string(&path)
                                    .map_err(|e| format!("Failed to read: {}", e))?;
                                Ok((filename, content))
                            }
                            None => Err("cancelled".to_string()),
                        }
                    }),
                    |result| match result {
                        Ok(Ok((filename, content))) => Message::KeyFileLoaded(filename, content),
                        Ok(Err(e)) => Message::KeyFileBrowseError(e),
                        Err(e) => Message::KeyFileBrowseError(format!("Thread error: {}", e)),
                    },
                ));
            }
            Message::KeyFileLoaded(filename, content) => {
                if self.key_import_label.is_empty() {
                    self.key_import_label = filename;
                }
                self.key_import_content = text_editor::Content::with_text(&content);
                self.key_import_pem = content;
                self.show_key_panel = true;
                self.key_error = None;
                self.key_success = Some("Key file loaded".into());
            }
            Message::KeyFileBrowseError(err) => {
                if !err.contains("cancelled") {
                    self.key_error = Some(err);
                }
            }
            Message::ImportKey => {
                if self.key_import_pem.is_empty() {
                    self.key_error = Some("Select a key file first".into());
                    return Ok(Task::none());
                }
                let label = if self.key_import_label.is_empty() {
                    "imported-key".to_string()
                } else {
                    self.key_import_label.clone()
                };
                match oryxis_vault::import_key(&label, &self.key_import_pem) {
                    Ok(mut generated) => {
                        // If editing an existing key, preserve its ID
                        if let Some(existing_id) = self.editing_key_id {
                            generated.key.id = existing_id;
                        }
                        if let Some(vault) = &self.vault {
                            match vault.save_key(&generated.key, Some(&generated.private_pem)) {
                                Ok(()) => {
                                    let verb = if self.editing_key_id.is_some() { "updated" } else { "imported" };
                                    self.key_error = None;
                                    self.key_success = Some(format!("Key '{}' {}", label, verb));
                                    self.key_import_label.clear();
                                    self.key_import_content = text_editor::Content::new();
                                    self.key_import_pem.clear();
                                    self.show_key_panel = false;
                                    self.editing_key_id = None;
                                    self.load_data_from_vault();
                                }
                                Err(e) => self.key_error = Some(e.to_string()),
                            }
                        }
                    }
                    Err(e) => self.key_error = Some(format!("Import failed: {}", e)),
                }
            }
            Message::DeleteKey(idx) => {
                if let Some(key) = self.keys.get(idx) {
                    let id = key.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_key(&id);
                        self.load_data_from_vault();
                        self.key_success = Some("Key deleted".into());
                    }
                }
                self.key_context_menu = None;
                self.overlay = None;
            }
            Message::ShowKeyMenu(idx) => {
                if self.key_context_menu == Some(idx) {
                    self.key_context_menu = None;
                    self.overlay = None;
                } else {
                    self.key_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::KeyActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::HideKeyMenu => {
                self.key_context_menu = None;
                self.identity_context_menu = None;
                self.show_keychain_add_menu = false;
                self.overlay = None;
            }
            Message::EditKey(idx) => {
                if let Some(key) = self.keys.get(idx) {
                    self.editing_key_id = Some(key.id);
                    self.key_import_label = key.label.clone();
                    // Load existing private key PEM from vault
                    let pem = self.vault.as_ref()
                        .and_then(|v| v.get_key_private(&key.id).ok().flatten())
                        .unwrap_or_default();
                    self.key_import_content = text_editor::Content::with_text(&pem);
                    self.key_import_pem = pem;
                    self.show_key_panel = true;
                    self.key_error = None;
                    self.key_success = None;
                    self.key_context_menu = None;
                    self.overlay = None;
                }
            }
            Message::KeySearchChanged(v) => {
                self.key_search = v;
            }

            // ── Identities ──
            Message::ShowIdentityPanel => {
                self.show_identity_panel = true;
                self.identity_form_label.clear();
                self.identity_form_username.clear();
                self.identity_form_password.clear();
                self.identity_form_key = None;
                self.identity_form_password_visible = false;
                self.identity_form_password_touched = false;
                self.identity_form_has_existing_password = false;
                self.editing_identity_id = None;
                self.show_keychain_add_menu = false;
                self.identity_context_menu = None;
                self.overlay = None;
            }
            Message::HideIdentityPanel => {
                self.show_identity_panel = false;
            }
            Message::IdentityLabelChanged(v) => {
                self.identity_form_label = v;
            }
            Message::IdentityUsernameChanged(v) => {
                self.identity_form_username = v;
            }
            Message::IdentityPasswordChanged(v) => {
                self.identity_form_password_touched = true;
                self.identity_form_password = v;
            }
            Message::IdentityTogglePasswordVisibility => {
                self.identity_form_password_visible = !self.identity_form_password_visible;
            }
            Message::IdentityKeyChanged(v) => {
                self.identity_form_key = if v == "(none)" { None } else { Some(v) };
            }
            Message::SaveIdentity => {
                if self.identity_form_label.trim().is_empty() {
                    return Ok(Task::none());
                }
                let mut identity = if let Some(id) = self.editing_identity_id {
                    self.identities.iter().find(|i| i.id == id).cloned()
                        .unwrap_or_else(|| Identity::new(""))
                } else {
                    Identity::new("")
                };
                identity.label = self.identity_form_label.clone();
                identity.username = if self.identity_form_username.is_empty() {
                    None
                } else {
                    Some(self.identity_form_username.clone())
                };
                identity.key_id = self.identity_form_key.as_ref().and_then(|label| {
                    self.keys.iter().find(|k| k.label == *label).map(|k| k.id)
                });
                identity.updated_at = chrono::Utc::now();

                let password = if !self.identity_form_password_touched {
                    None
                } else if self.identity_form_password.is_empty() {
                    Some("")
                } else {
                    Some(self.identity_form_password.as_str())
                };

                if let Some(vault) = &self.vault {
                    let _ = vault.save_identity(&identity, password);
                    self.load_data_from_vault();
                }
                self.show_identity_panel = false;
            }
            Message::EditIdentity(idx) => {
                if let Some(identity) = self.identities.get(idx) {
                    self.editing_identity_id = Some(identity.id);
                    self.identity_form_label = identity.label.clone();
                    self.identity_form_username = identity.username.clone().unwrap_or_default();
                    self.identity_form_password.clear();
                    self.identity_form_password_touched = false;
                    self.identity_form_password_visible = false;
                    self.identity_form_has_existing_password = self.vault.as_ref()
                        .and_then(|v| v.get_identity_password(&identity.id).ok().flatten())
                        .is_some();
                    self.identity_form_key = identity.key_id.and_then(|kid| {
                        self.keys.iter().find(|k| k.id == kid).map(|k| k.label.clone())
                    });
                    self.show_identity_panel = true;
                    self.identity_context_menu = None;
                    self.overlay = None;
                }
            }
            Message::DeleteIdentity(idx) => {
                if let Some(identity) = self.identities.get(idx) {
                    let id = identity.id;
                    if let Some(vault) = &self.vault {
                        let _ = vault.delete_identity(&id);
                        self.load_data_from_vault();
                    }
                }
                self.identity_context_menu = None;
                self.overlay = None;
            }
            Message::ShowIdentityMenu(idx) => {
                if self.identity_context_menu == Some(idx) {
                    self.identity_context_menu = None;
                    self.overlay = None;
                } else {
                    self.identity_context_menu = Some(idx);
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::IdentityActions(idx),
                        x: self.mouse_position.x,
                        y: self.mouse_position.y,
                    });
                }
            }
            Message::ToggleKeychainAddMenu => {
                if self.show_keychain_add_menu {
                    self.show_keychain_add_menu = false;
                    self.overlay = None;
                } else {
                    self.show_keychain_add_menu = true;
                    // Push the menu a bit below the click point so it appears
                    // under the button instead of overlapping it. Also nudge
                    // left so the menu's left edge roughly aligns with the
                    // left half of the split button (rather than the cursor).
                    self.overlay = Some(OverlayState {
                        content: OverlayContent::KeychainAdd,
                        x: (self.mouse_position.x - 60.0).max(0.0),
                        y: self.mouse_position.y + 16.0,
                    });
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
