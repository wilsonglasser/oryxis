//! `Oryxis::handle_vault`: settings-panel-independent dispatch arms for the
//! vault area, split out of dispatch.rs. Returns `Err(message)` for anything
//! it doesn't claim so the try_handler! chain falls through.
#![allow(clippy::result_large_err)]

use iced::Task;

use crate::app::{Message, Oryxis};
use crate::state::VaultState;
use oryxis_vault::VaultError;

impl Oryxis {
    pub(crate) fn handle_vault(
        &mut self,
        message: Message,
    ) -> Result<Task<Message>, Message> {
        match message {
            // -- Vault --
            Message::VaultPasswordChanged(pw) => {
                self.vault_ui.password_input = pw;
            }
            Message::VaultTogglePasswordVisibility => {
                self.vault_ui.password_visible = !self.vault_ui.password_visible;
            }
            Message::VaultSetup => {
                if self.vault_ui.password_input.len() < 4 {
                    self.vault_ui.error =
                        Some(crate::i18n::t("password_too_short").to_string());
                    return Ok(Task::none());
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_master_password(&self.vault_ui.password_input) {
                        Ok(()) => {
                            let _ = vault.set_setting("has_user_password", "1");
                            self.vault_ui.has_user_password = true;
                            self.vault_ui.state = VaultState::Unlocked;
                            self.vault_ui.error = None;
                            // Cache for child-window spawn.
                            self.master_password = Some(self.vault_ui.password_input.clone());
                            self.vault_ui.password_input.clear();
                            self.vault_ui.password_visible = false;
                            self.load_data_from_vault();
                            return Ok(iced::widget::operation::focus(iced::widget::Id::new(
                                "search-dashboard",
                            )));
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::VaultSkipPassword => {
                if let Some(vault) = &mut self.vault {
                    match vault.open_without_password() {
                        Ok(()) => {
                            self.vault_ui.state = VaultState::Unlocked;
                            self.vault_ui.error = None;
                            self.load_data_from_vault();
                            return Ok(iced::widget::operation::focus(iced::widget::Id::new(
                                "search-dashboard",
                            )));
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_ui.error = Some(
                                crate::i18n::t("vault_already_has_password").to_string(),
                            );
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(format!("Failed to create vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultDestroyConfirm => {
                self.vault_ui.destroy_confirm = !self.vault_ui.destroy_confirm;
            }
            Message::VaultDestroy => {
                if let Some(vault) = &mut self.vault {
                    match vault.destroy_and_recreate() {
                        Ok(()) => {
                            self.vault_ui.state = VaultState::NeedSetup;
                            self.vault_ui.error = None;
                            self.vault_ui.destroy_confirm = false;
                            self.vault_ui.password_input.clear();
                            self.vault_ui.password_visible = false;
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(format!("Failed to reset vault: {}", e));
                        }
                    }
                }
            }
            Message::VaultUnlock => {
                // Ignore the submit when no password was typed (pressing
                // Enter on an empty field or clicking Unlock with it blank
                // shouldn't run a doomed unlock attempt or surface an error).
                if self.vault_ui.password_input.is_empty() {
                    return Ok(Task::none());
                }
                if let Some(vault) = &mut self.vault {
                    match vault.unlock(&self.vault_ui.password_input) {
                        Ok(()) => {
                            self.vault_ui.state = VaultState::Unlocked;
                            self.vault_ui.error = None;
                            // Retain the password in memory so we can spawn
                            // child windows with it via stdin pipe.
                            self.master_password = Some(self.vault_ui.password_input.clone());
                            self.vault_ui.password_input.clear();
                            self.vault_ui.password_visible = false;
                            self.load_data_from_vault();
                            // Bring the sync engine up now that the
                            // vault is open, if the user left it on. Only
                            // the P2P transport has a background engine;
                            // SFTP reconciles on the cadence subscription.
                            let sync_task = if self.sync.enabled
                                && self.sync.transport != "sftp"
                            {
                                self.start_sync_engine()
                            } else {
                                Task::none()
                            };
                            // Auto-start port forward rules now that the
                            // vault (and its credentials) is open.
                            let mut unlock_tasks = vec![sync_task];
                            unlock_tasks.extend(self.auto_start_port_forwards());
                            // Plugin migrate-install + auto-update: for a
                            // password vault these are deferred from boot
                            // to here, now that the plugin rows are loaded
                            // (boot saw a locked vault with no rows).
                            unlock_tasks.extend(self.spawn_plugin_unlock_tasks());
                            // After a manual unlock, fire any deferred
                            // `--connect <uuid>` from the launch CLI args.
                            if let Some(connect_id) = self.pending_auto_connect.take()
                                && let Some(idx) = self
                                    .connections
                                    .iter()
                                    .position(|c| c.id == connect_id)
                            {
                                unlock_tasks.push(Task::done(Message::ConnectSsh(idx)));
                            } else {
                                // Land on Home with the host search focused
                                // so the user can type / keyboard-navigate
                                // immediately (matches ChangeView behavior).
                                unlock_tasks.push(iced::widget::operation::focus(
                                    iced::widget::Id::new("search-dashboard"),
                                ));
                            }
                            return Ok(Task::batch(unlock_tasks));
                        }
                        Err(VaultError::InvalidPassword) => {
                            self.vault_ui.error = Some("Invalid password".into());
                        }
                        Err(e) => {
                            self.vault_ui.error = Some(e.to_string());
                        }
                    }
                }
            }

            // ── Vault password management ──
            Message::ToggleVaultPassword => {
                if self.vault_ui.has_user_password {
                    // Removing encryption is destructive: arm the confirm
                    // prompt instead of dropping the password on a single
                    // click. The switch stays on until the user confirms.
                    // Close the change form so the two can't stack.
                    self.vault_ui.confirm_remove_password = true;
                    self.vault_ui.change_password_open = false;
                    self.vault_ui.password_error = None;
                } else {
                    // No password yet: the switch reveals / hides the inline
                    // set-password form. Nothing is committed until the user
                    // types, confirms, and presses Set Password.
                    self.vault_ui.show_password_form = !self.vault_ui.show_password_form;
                    self.vault_ui.new_password.clear();
                    self.vault_ui.confirm_password.clear();
                    self.vault_ui.password_error = None;
                }
            }
            Message::ConfirmRemoveVaultPassword => {
                if let Some(vault) = &mut self.vault {
                    match vault.remove_user_password() {
                        Ok(()) => {
                            self.vault_ui.has_user_password = false;
                            self.vault_ui.show_password_form = false;
                            self.vault_ui.confirm_remove_password = false;
                            self.vault_ui.password_error = None;
                            self.vault_ui.new_password.clear();
                            self.vault_ui.confirm_password.clear();
                        }
                        Err(e) => {
                            self.vault_ui.password_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::CancelRemoveVaultPassword => {
                self.vault_ui.confirm_remove_password = false;
                self.vault_ui.password_error = None;
            }
            Message::VaultNewPasswordChanged(pw) => {
                self.vault_ui.new_password = pw;
            }
            Message::VaultConfirmPasswordChanged(pw) => {
                self.vault_ui.confirm_password = pw;
            }
            Message::SetVaultPassword => {
                if self.vault_ui.new_password.len() < 4 {
                    self.vault_ui.password_error =
                        Some(crate::i18n::t("password_too_short").to_string());
                    return Ok(Task::none());
                }
                // Both fields are hidden, so a typo would otherwise be
                // invisible until the next unlock (when it's too late).
                if self.vault_ui.new_password != self.vault_ui.confirm_password {
                    self.vault_ui.password_error =
                        Some(crate::i18n::t("passwords_do_not_match").to_string());
                    return Ok(Task::none());
                }
                if let Some(vault) = &mut self.vault {
                    match vault.set_user_password(&self.vault_ui.new_password) {
                        Ok(()) => {
                            self.vault_ui.has_user_password = true;
                            self.vault_ui.show_password_form = false;
                            self.vault_ui.password_error = None;
                            self.vault_ui.new_password.clear();
                            self.vault_ui.confirm_password.clear();
                        }
                        Err(e) => {
                            self.vault_ui.password_error = Some(e.to_string());
                        }
                    }
                }
            }
            Message::OpenChangeVaultPassword => {
                // Reveal the change form; start from a clean slate so a
                // stale value from a previous open can't leak in. Dismiss
                // any armed remove-confirm so the two can't stack.
                self.vault_ui.change_password_open = true;
                self.vault_ui.confirm_remove_password = false;
                self.vault_ui.current_password.clear();
                self.vault_ui.new_password.clear();
                self.vault_ui.confirm_password.clear();
                self.vault_ui.password_error = None;
            }
            Message::CancelChangeVaultPassword => {
                self.vault_ui.change_password_open = false;
                self.vault_ui.current_password.clear();
                self.vault_ui.new_password.clear();
                self.vault_ui.confirm_password.clear();
                self.vault_ui.password_error = None;
            }
            Message::VaultCurrentPasswordChanged(pw) => {
                self.vault_ui.current_password = pw;
            }
            Message::ConfirmChangeVaultPassword => {
                if self.vault_ui.new_password.len() < 4 {
                    self.vault_ui.password_error =
                        Some(crate::i18n::t("password_too_short").to_string());
                    return Ok(Task::none());
                }
                if self.vault_ui.new_password != self.vault_ui.confirm_password {
                    self.vault_ui.password_error =
                        Some(crate::i18n::t("passwords_do_not_match").to_string());
                    return Ok(Task::none());
                }
                if let Some(vault) = &mut self.vault {
                    // Verify the current password before rotating. The vault
                    // is already unlocked, so this guards against someone
                    // changing the password at an unattended session, and
                    // against a typo silently rotating to an unknown key.
                    match vault.verify_password(&self.vault_ui.current_password) {
                        Ok(true) => match vault.set_user_password(&self.vault_ui.new_password) {
                            Ok(()) => {
                                self.vault_ui.change_password_open = false;
                                self.vault_ui.password_error = None;
                                self.vault_ui.current_password.clear();
                                self.vault_ui.new_password.clear();
                                self.vault_ui.confirm_password.clear();
                                return Ok(self.show_toast(
                                    crate::i18n::t("password_updated").to_string(),
                                ));
                            }
                            Err(e) => {
                                self.vault_ui.password_error = Some(e.to_string());
                            }
                        },
                        Ok(false) => {
                            self.vault_ui.password_error =
                                Some(crate::i18n::t("current_password_incorrect").to_string());
                        }
                        Err(e) => {
                            self.vault_ui.password_error = Some(e.to_string());
                        }
                    }
                }
            }

            m => return Err(m),
        }
        Ok(Task::none())
    }
}
