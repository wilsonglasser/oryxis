//! Biometric (OS-keystore) unlock: the settings opt-in that enrolls
//! the master password, the lock-screen prompt, and the fallback to
//! the typed form when the OS says no.
//!
//! The retrieval blocks on the OS presence sheet, so it runs off the
//! UI thread and returns as `BiometricUnlockResult`, which feeds the
//! released password into the ordinary unlock path.

use super::*;

impl Oryxis {
    pub(super) fn handle_vault_biometric(
        &mut self,
        message: VaultMessage,
    ) -> Task<Message> {
        match message {

            // ── Biometric (OS-keystore) unlock ──
            VaultMessage::ToggleBiometricUnlock => {
                if self.prefs.biometric_unlock_enabled {
                    // Opt out: forget the stored secret unconditionally, then
                    // flip the setting off and persist.
                    self.biometric_forget();
                    self.prefs.biometric_unlock_enabled = false;
                    self.persist_setting("biometric_unlock_enabled", "false");
                } else {
                    // Opt in: needs an available backend and the master
                    // password in hand (we are unlocked). Enroll first; only
                    // turn the setting on if the store accepted it. The three
                    // ways this fails are distinct and used to collapse into
                    // one "enter your master password" toast, which is
                    // undiagnosable (nothing logged) and, for a keystore
                    // rejection, simply the wrong advice.
                    if !self.biometric_available {
                        tracing::warn!("biometric opt-in refused: backend unavailable");
                        return self.show_toast(
                            crate::i18n::t("biometric_unlock_failed").to_string(),
                        );
                    }
                    let Some(pw) = self.master_password.clone() else {
                        tracing::warn!(
                            "biometric opt-in refused: master password not held in memory"
                        );
                        return self.show_toast(
                            crate::i18n::t("biometric_unlock_failed").to_string(),
                        );
                    };
                    match self.biometric_vault().map(|bv| bv.enroll(&pw)) {
                        Some(Ok(())) => {
                            self.prefs.biometric_unlock_enabled = true;
                            self.persist_setting("biometric_unlock_enabled", "true");
                        }
                        Some(Err(e)) => {
                            // The OS keystore refused the write (on macOS the
                            // SecItemAdd OSStatus is in the string). Keep the
                            // setting off and say what actually happened.
                            tracing::warn!("biometric enroll failed: {e}");
                            return self.show_toast(
                                crate::i18n::t("biometric_enroll_failed").to_string(),
                            );
                        }
                        None => {
                            tracing::warn!("biometric enroll skipped: no vault open");
                            return self.show_toast(
                                crate::i18n::t("biometric_enroll_failed").to_string(),
                            );
                        }
                    }
                }
            }
            VaultMessage::BiometricUnlockRequested => {
                let Some(bv) = self.biometric_vault() else {
                    return Task::none();
                };
                // Localized reason line for the OS prompt (Touch ID sheet /
                // Hello dialog); captured before the move into the worker.
                let prompt = crate::i18n::t("biometric_unlock").to_string();
                // The retrieval blocks on the OS presence prompt, so run it
                // off the UI thread and route the outcome back as a message.
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            bv.unlock_secret(&prompt).map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    |v| Message::Vault(VaultMessage::BiometricUnlockResult(v)),
                );
            }
            VaultMessage::BiometricUnlockResult(res) => match res {
                Ok(password) => {
                    // Feed the released password into the ordinary unlock
                    // path (which sets `master_password`, boots sync, etc).
                    self.vault_ui.password_input = password;
                    return Task::done(Message::Vault(VaultMessage::VaultUnlock));
                }
                Err(e) => {
                    tracing::warn!("biometric unlock failed: {e}");
                    self.vault_ui.error =
                        Some(crate::i18n::t("biometric_unlock_failed").to_string());
                    // Drop to the typed-password layout so the user is
                    // never stuck on a prompt the OS keeps rejecting, and
                    // focus the input the error just told them to use.
                    self.vault_ui.password_fallback = true;
                    return crate::widgets::focus_input(iced::widget::Id::new(
                        "vault-unlock-password",
                    ));
                }
            },
            VaultMessage::VaultShowPasswordFallback => {
                // Biometric-first lock screen: reveal the typed-password
                // form. The biometric button stays available below it, so
                // this is a per-lock choice, not a mode switch.
                self.vault_ui.password_fallback = true;
                self.vault_ui.error = None;
                return crate::widgets::focus_input(iced::widget::Id::new(
                    "vault-unlock-password",
                ));
            }
            VaultMessage::ToggleSetupBiometric => {
                self.vault_ui.setup_enable_biometric = !self.vault_ui.setup_enable_biometric;
            }
            // The parent routed us here, so anything else is a
            // grouping mistake, not a runtime case.
            m => return crate::dispatch::unrouted(m),
        }
        Task::none()
    }
}
