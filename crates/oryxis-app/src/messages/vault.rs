//! Vault lock / unlock / setup, master-password management, biometric
//! unlock and KDF calibration, wrapped by [`crate::messages::Message::Vault`].

#[derive(Debug, Clone)]
pub enum VaultMessage {
    VaultPasswordChanged(super::Redacted),
    VaultTogglePasswordVisibility,
    VaultUnlock,
    VaultSetup,
    VaultSkipPassword,
    VaultDestroyConfirm,
    VaultDestroy,
    /// Settings toggle: opt in / out of biometric (OS-keystore) unlock.
    /// Enrolling stores the current master password; disabling forgets it.
    ToggleBiometricUnlock,
    /// Lock-screen button: raise the OS presence prompt and, on success,
    /// unlock with the released master password. The retrieval runs off
    /// the UI thread (it blocks on the OS prompt) and returns via
    /// `BiometricUnlockResult`.
    BiometricUnlockRequested,
    /// Result of the off-thread biometric retrieval: `Ok(master_password)`
    /// to feed into the normal unlock, or `Err(message)` to surface.
    BiometricUnlockResult(Result<String, String>),
    /// Lock-screen link on the biometric-first layout: reveal the typed
    /// master-password form (biometrics stay one click away).
    VaultShowPasswordFallback,
    /// Set-password forms: flip the "also enable biometric unlock" opt-in.
    ToggleSetupBiometric,
    /// Arm the manual-lock confirmation dialog. `LockVault` tears every
    /// live session and tab down, so the button asks first (an accidental
    /// click would sever all open connections); the dialog's Sleep / Lock
    /// buttons then commit through `LockVaultConfirmProceed`. When the
    /// user saved a standing choice (`manual_lock_action` = sleep / lock),
    /// this skips the dialog and commits that choice directly.
    LockVaultConfirm,
    /// Dismiss the manual-lock confirmation dialog without locking.
    CancelLockVaultConfirm,
    /// Flip the confirm dialog's "always use the selected option" opt-in.
    LockVaultConfirmRememberToggled,
    /// Commit the confirm dialog's choice: soft lock (`sleep`) or full
    /// teardown. Persists the choice as `manual_lock_action` first when
    /// the remember opt-in is checked, then dispatches `SoftLockVault` /
    /// `LockVault`.
    LockVaultConfirmProceed { sleep: bool },
    /// Commit the manual lock after the confirm dialog: full teardown of
    /// sessions, tabs and secrets. Reached only through the confirm flow
    /// (the trigger sites arm `LockVaultConfirm` instead).
    LockVault,
    /// Soft lock: zeroize the vault key and show the lock screen but keep
    /// live SSH sessions and tabs (unlike the manual `LockVault`, which
    /// tears sessions down). Fired by the idle auto-lock timer and by the
    /// confirm dialog's Sleep button.
    SoftLockVault,
    ToggleVaultPassword,
    /// Commit the master-password removal after the confirm prompt.
    ConfirmRemoveVaultPassword,
    /// Dismiss the remove-password confirm prompt without removing.
    CancelRemoveVaultPassword,
    VaultNewPasswordChanged(super::Redacted),
    VaultConfirmPasswordChanged(super::Redacted),
    SetVaultPassword,
    /// Open / close the change-master-password form.
    OpenChangeVaultPassword,
    CancelChangeVaultPassword,
    /// Current-password field of the change-password form.
    VaultCurrentPasswordChanged(super::Redacted),
    /// Verify the current password and rotate to the new one.
    ConfirmChangeVaultPassword,
    /// E1: Argon2id calibration finished off-thread; apply the pending
    /// set / change-password operation with the tuned parameters.
    VaultKdfCalibrated(crate::state::VaultPwOp, oryxis_vault::KdfParams),
    /// Another process wrote the vault file (the MCP server pinning a
    /// host key, creating or editing a host) and bumped the change
    /// marker; `writer` names it. The in-memory entity lists are
    /// re-read while the vault is unlocked.
    VaultChangedOutside { writer: String },
}
