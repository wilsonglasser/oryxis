//! App-side glue for local biometric unlock (see the `oryxis-biometric`
//! crate). This module owns the mapping from "the currently open vault" to
//! a per-vault [`BiometricVault`], plus the stable account id the OS
//! keystore is keyed on.
//!
//! The lifecycle (enroll on opt-in, refresh on rotation, disable on
//! opt-out or password removal, retrieve on the lock screen) is driven
//! from `dispatch_vault.rs`; the crate's own orchestrator holds the
//! platform-independent logic and is unit-tested there.

use oryxis_biometric::BiometricVault;
use sha2::{Digest, Sha256};

use crate::app::Oryxis;

/// Derive the OS-keystore account id for a vault from its database path.
///
/// A SHA-256 of the canonical path keeps the id stable across app updates
/// (a non-cryptographic `Hash` is not guaranteed stable across builds, so
/// it would silently orphan the stored secret after an upgrade) while
/// keeping the raw path (which contains the user's home directory) out of
/// the credential label. The `bio1:` prefix namespaces the scheme so a
/// future format change can coexist.
fn account_for(vault_path: &str) -> String {
    let digest = Sha256::digest(vault_path.as_bytes());
    // 16 bytes of the digest is ample to avoid collisions across a user's
    // handful of vaults while staying short in the credential store.
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("bio1:{hex}")
}

/// Platform-named label for the lock-screen unlock button. "Biometric" is
/// only accurate on Windows/macOS; on Linux the backend is the login
/// keyring, so the label says what actually happens (honest UX, same
/// rationale as the Telnet cleartext note).
pub(crate) fn bio_unlock_label() -> &'static str {
    if cfg!(target_os = "windows") {
        crate::i18n::t("biometric_unlock_windows")
    } else if cfg!(target_os = "macos") {
        crate::i18n::t("biometric_unlock_macos")
    } else {
        crate::i18n::t("biometric_unlock_linux")
    }
}

/// Platform-named title for the Settings toggle row.
pub(crate) fn bio_setting_label() -> &'static str {
    if cfg!(target_os = "windows") {
        crate::i18n::t("biometric_setting_windows")
    } else if cfg!(target_os = "macos") {
        crate::i18n::t("biometric_setting_macos")
    } else {
        crate::i18n::t("biometric_setting_linux")
    }
}

/// Platform-named label for the "also enable ... unlock" opt-in shown on
/// the set-password forms (Settings and the onboarding final slide).
pub(crate) fn bio_setup_label() -> &'static str {
    if cfg!(target_os = "windows") {
        crate::i18n::t("setup_biometric_windows")
    } else if cfg!(target_os = "macos") {
        crate::i18n::t("setup_biometric_macos")
    } else {
        crate::i18n::t("setup_biometric_linux")
    }
}

/// Platform glyph for the unlock affordance: a fingerprint where the
/// backend is an OS presence check, a key where it is the login keyring.
pub(crate) fn bio_icon<'a>() -> iced::widget::Text<'a> {
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        iced_fonts::lucide::fingerprint()
    } else {
        iced_fonts::lucide::key_round()
    }
}

impl Oryxis {
    /// Build a [`BiometricVault`] bound to the open vault's account, or
    /// `None` when no vault is open. Cheap to construct (it only wraps the
    /// platform provider), so callers make one per operation rather than
    /// caching it.
    pub(crate) fn biometric_vault(&self) -> Option<BiometricVault> {
        let path = self.vault.as_ref()?.db_path().to_string_lossy().to_string();
        Some(BiometricVault::new(account_for(&path)))
    }

    /// Whether the biometric-unlock affordance should be offered at all:
    /// the platform supports it, the user opted in, and the vault actually
    /// has a master password to store (a passwordless vault has nothing to
    /// gate).
    pub(crate) fn biometric_unlock_offered(&self) -> bool {
        self.prefs.biometric_unlock_enabled
            && self.biometric_available
            && self.vault_ui.has_user_password
    }

    /// Best-effort refresh of the stored secret after a successful unlock
    /// or a password rotation, so the OS keystore always holds the current
    /// master password. Silent by design (enroll never prompts); a backend
    /// error is logged, not surfaced, since the typed password still works.
    pub(crate) fn biometric_reenroll(&self, master_password: &str) {
        if !self.biometric_unlock_enabled_and_available() {
            return;
        }
        if let Some(bv) = self.biometric_vault()
            && let Err(e) = bv.enroll(master_password)
        {
            tracing::warn!("biometric re-enroll failed: {e}");
        }
    }

    /// Enroll from the set-password forms (Settings / onboarding) when the
    /// "also enable biometric unlock" opt-in was checked. Called with the
    /// freshly-set master password, before the form buffers are cleared.
    /// Failure is non-fatal: the password itself is already committed, so
    /// surface a toast and leave the setting off. Returns the toast task
    /// on failure, `None` on success or when the opt-in doesn't apply.
    pub(crate) fn biometric_setup_enroll(
        &mut self,
        master_password: &str,
    ) -> Option<iced::Task<crate::app::Message>> {
        if !(self.vault_ui.setup_enable_biometric && self.biometric_available) {
            return None;
        }
        match self.biometric_vault().map(|bv| bv.enroll(master_password)) {
            Some(Ok(())) => {
                self.prefs.biometric_unlock_enabled = true;
                self.persist_setting("biometric_unlock_enabled", "true");
                None
            }
            Some(Err(e)) => {
                // The OS keystore refused the write (on macOS the SecItemAdd
                // OSStatus is in the string). The password itself is already
                // committed, so leave the setting off and say what failed.
                tracing::warn!("biometric enroll failed: {e}");
                Some(self.show_toast(crate::i18n::t("biometric_enroll_failed").to_string()))
            }
            None => {
                tracing::warn!("biometric enroll skipped: no vault open");
                Some(self.show_toast(crate::i18n::t("biometric_enroll_failed").to_string()))
            }
        }
    }

    /// Forget any stored secret for the open vault. Idempotent; used when
    /// the user opts out or removes the vault password.
    pub(crate) fn biometric_forget(&self) {
        if let Some(bv) = self.biometric_vault()
            && let Err(e) = bv.disable()
        {
            tracing::warn!("biometric disable failed: {e}");
        }
    }

    /// The setting is on and the platform can service it. Distinct from
    /// [`Self::biometric_unlock_offered`] in that it does not require a
    /// user password (the caller has already established one when
    /// enrolling).
    fn biometric_unlock_enabled_and_available(&self) -> bool {
        self.prefs.biometric_unlock_enabled && self.biometric_available
    }
}

#[cfg(test)]
mod tests {
    use super::account_for;

    #[test]
    fn account_is_stable_and_path_specific() {
        // Same path -> same id (stable across calls / builds via SHA-256).
        assert_eq!(account_for("/home/u/.oryxis/vault.db"), account_for("/home/u/.oryxis/vault.db"));
        // Different vaults -> different ids (no cross-vault secret bleed).
        assert_ne!(
            account_for("/home/u/.oryxis/vault.db"),
            account_for("/home/u/.oryxis/work.db")
        );
        // The raw path never appears in the id (only its digest).
        assert!(!account_for("/home/u/.oryxis/vault.db").contains("/home/u"));
    }
}
