//! `Oryxis::handle_vault`, the vault-domain router.
//!
//! The groups are the four things a vault does over its life: you get
//! in (`open`), you leave (`lock`), you let the OS keystore do the
//! typing (`biometric`), or you change the password that protects it
//! (`password`). The split follows the section banners the 760-line
//! match already carried in comments.

#![allow(clippy::result_large_err)]

// Dispatch sub-handlers, one file per arm family.
mod open;
mod lock;
mod biometric;
mod password;

use iced::Task;

use crate::app::{SshMessage, VaultMessage, Message, Oryxis};
use crate::state::{VaultState, View};
use oryxis_vault::VaultError;

impl Oryxis {
    pub(crate) fn handle_vault(
        &mut self,
        message: VaultMessage,
    ) -> Task<Message> {
        match message {
            m @ (
                VaultMessage::VaultPasswordChanged(..)
                | VaultMessage::VaultTogglePasswordVisibility
                | VaultMessage::VaultSetup
                | VaultMessage::VaultSkipPassword
                | VaultMessage::VaultDestroyConfirm
                | VaultMessage::VaultDestroy
                | VaultMessage::VaultUnlock
            ) => self.handle_vault_open(m),
            m @ (
                VaultMessage::SoftLockVault
                | VaultMessage::LockVault
                | VaultMessage::LockVaultConfirm
                | VaultMessage::CancelLockVaultConfirm
                | VaultMessage::LockVaultConfirmRememberToggled
                | VaultMessage::LockVaultConfirmProceed { .. }
            ) => self.handle_vault_lock(m),
            m @ (
                VaultMessage::ToggleBiometricUnlock
                | VaultMessage::BiometricUnlockRequested
                | VaultMessage::BiometricUnlockResult(..)
                | VaultMessage::VaultShowPasswordFallback
                | VaultMessage::ToggleSetupBiometric
            ) => self.handle_vault_biometric(m),
            m @ (
                VaultMessage::ToggleVaultPassword
                | VaultMessage::ConfirmRemoveVaultPassword
                | VaultMessage::CancelRemoveVaultPassword
                | VaultMessage::VaultNewPasswordChanged(..)
                | VaultMessage::VaultConfirmPasswordChanged(..)
                | VaultMessage::SetVaultPassword
                | VaultMessage::OpenChangeVaultPassword
                | VaultMessage::CancelChangeVaultPassword
                | VaultMessage::VaultCurrentPasswordChanged(..)
                | VaultMessage::ConfirmChangeVaultPassword
                | VaultMessage::VaultKdfCalibrated(..)
            ) => self.handle_vault_password(m),
            VaultMessage::VaultChangedOutside { writer } => {
                tracing::info!(writer = %writer, "vault written by another process, reloading");
                self.reload_after_external_write();
                Task::none()
            }
        }
    }
}

/// Phase 1 of an E1 set / change-password flow: run the Argon2id KDF
/// calibration on a blocking worker thread (it does several ~100 ms
/// hashes), then fire `VaultKdfCalibrated` so the handler applies the
/// vault mutation with the tuned parameters. A calibration that somehow
/// fails resolves to the default profile rather than blocking the flow.
fn calibrate_kdf_task(op: crate::state::VaultPwOp) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(oryxis_vault::calibrate_kdf)
                .await
                .unwrap_or(oryxis_vault::KdfParams::DEFAULT)
        },
        move |params| Message::Vault(VaultMessage::VaultKdfCalibrated(op, params)),
    )
}
