//! Vault access UI state: the lock screen, master-password setup, and the
//! destroy-confirm flag. Grouped off the `Oryxis` god-struct as part of the
//! modules-by-feature direction (field grouping only).
//!
//! Note: the live `VaultStore` handle stays at `Oryxis::vault`; only the
//! transient unlock/setup UI lives here. The struct is named `VaultUi` to
//! avoid colliding with that `vault` field.

use super::VaultState;

/// Lock screen + master-password setup + destroy-confirm UI state.
#[derive(Debug, Clone, Default)]
pub(crate) struct VaultUi {
    /// Loading / NeedSetup / Locked / Unlocked.
    pub(crate) state: VaultState,
    /// Password typed on the lock / setup screen.
    pub(crate) password_input: String,
    /// Whether the lock-screen password is shown as plain text.
    pub(crate) password_visible: bool,
    /// Error shown on the lock / setup screen.
    pub(crate) error: Option<String>,
    /// Whether a master password is set (vs the empty-password vault).
    pub(crate) has_user_password: bool,
    /// When no master password is set yet, whether the inline set-password
    /// form is revealed. Flipped by the header switch so the toggle has a
    /// visible effect before a password exists; ignored once one is set.
    pub(crate) show_password_form: bool,
    /// Two-step confirm latch for removing the master password. Toggling
    /// the switch off (or the removal path) arms this; the destructive
    /// action only runs once the user confirms, so an accidental flip
    /// doesn't silently drop encryption.
    pub(crate) confirm_remove_password: bool,
    /// Whether the "change master password" form is open (only meaningful
    /// once a password is set). Reuses `new_password` / `confirm_password`
    /// for the new value and adds `current_password` for verification.
    pub(crate) change_password_open: bool,
    /// Current master password typed in the change-password form, checked
    /// against the vault before the rotation runs.
    pub(crate) current_password: String,
    /// New master password (Settings > Security).
    pub(crate) new_password: String,
    /// Confirm new master password (Settings > Security).
    pub(crate) confirm_password: String,
    /// Inline error on the master-password change form.
    pub(crate) password_error: Option<String>,
    /// Two-step confirm latch for the destroy-vault action.
    pub(crate) destroy_confirm: bool,
}
