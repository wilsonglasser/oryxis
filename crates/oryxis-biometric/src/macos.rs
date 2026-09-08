//! macOS provider: a Keychain generic-password item guarded by a
//! `SecAccessControl` user-presence policy, so every read intrinsically
//! raises the Touch ID (or Apple Watch / device-passcode) prompt.
//!
//! The item lives in the data-protection keychain
//! (`kSecUseDataProtectionKeychain`), the only one that services an
//! access-control policy cleanly on macOS. Access to it is granted by the
//! app's `keychain-access-groups` entitlement (`resources/oryxis.entitlements`,
//! stamped with the Team ID at sign time). A build signed without that
//! entitlement gets `errSecMissingEntitlement` (-34018) on every call
//! here, so a local `cargo run` or an ad-hoc-signed bundle cannot enroll;
//! a Developer ID (or later) signature is required.
//!
//! The Security-framework entry points and the `kSec*` attribute keys are
//! declared here as externs against the framework's stable C ABI, rather
//! than pulled from a `-sys` crate whose exact Rust surface we cannot
//! compile-check from the Linux dev host. Only Core Foundation is used for
//! the CFString / CFData / CFDictionary plumbing. Verified by CI on
//! macos-latest and by manual QA.
//!
//! Policy choice: `kSecAccessControlUserPresence` (== 1) accepts biometry
//! OR the device passcode, which matches how password managers gate an
//! unlock (Touch ID with a passcode fallback). The stricter
//! `kSecAccessControlBiometryCurrentSet` would invalidate the stored
//! secret whenever the user edits their enrolled fingerprints, silently
//! breaking unlock; user-presence is the friendlier, still-real gate.

use core_foundation::base::{CFType, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::base::{kCFAllocatorDefault, CFOptionFlags, CFTypeRef};
use core_foundation_sys::error::CFErrorRef;
use core_foundation_sys::string::CFStringRef;

use crate::provider::{BioError, BiometricProvider};

/// Keychain service attribute; the per-vault account is the account
/// attribute, so two vaults are two distinct items.
const SERVICE: &str = "oryxis-vault-unlock";

// OSStatus codes we branch on (stable public values).
type OSStatus = i32;
const ERR_SEC_SUCCESS: OSStatus = 0;
const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25299;
const ERR_SEC_USER_CANCELED: OSStatus = -128;
const ERR_SEC_AUTH_FAILED: OSStatus = -25293;
/// `errSecMissingEntitlement`: the process is not signed with a
/// `keychain-access-groups` entitlement covering this item. Every
/// data-protection-keychain call returns it, so it gets a message that
/// names the cause rather than a bare number.
const ERR_SEC_MISSING_ENTITLEMENT: OSStatus = -34018;

/// `kSecAccessControlUserPresence` (1 << 0): biometry or device passcode.
const ACCESS_CONTROL_USER_PRESENCE: CFOptionFlags = 1;

type SecAccessControlRef = CFTypeRef;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecClass: CFStringRef;
    static kSecClassGenericPassword: CFStringRef;
    static kSecAttrService: CFStringRef;
    static kSecAttrAccount: CFStringRef;
    static kSecValueData: CFStringRef;
    static kSecReturnData: CFStringRef;
    static kSecMatchLimit: CFStringRef;
    static kSecMatchLimitOne: CFStringRef;
    static kSecUseOperationPrompt: CFStringRef;
    static kSecUseDataProtectionKeychain: CFStringRef;
    static kSecAttrAccessControl: CFStringRef;
    static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: CFStringRef;

    fn SecAccessControlCreateWithFlags(
        allocator: CFTypeRef,
        protection: CFTypeRef,
        flags: CFOptionFlags,
        error: *mut CFErrorRef,
    ) -> SecAccessControlRef;

    fn SecItemAdd(attributes: CFTypeRef, result: *mut CFTypeRef) -> OSStatus;
    fn SecItemCopyMatching(query: CFTypeRef, result: *mut CFTypeRef) -> OSStatus;
    fn SecItemDelete(query: CFTypeRef) -> OSStatus;
}

pub struct TouchId;

impl TouchId {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TouchId {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrap a static `kSec*` CFStringRef as an owned-by-get-rule `CFType` so it
/// can go into a `CFDictionary` of `CFType` pairs.
fn key(k: CFStringRef) -> CFType {
    unsafe { CFString::wrap_under_get_rule(k).as_CFType() }
}

/// The (service, account) pair every operation keys on, plus the
/// data-protection-keychain selector.
///
/// `kSecUseDataProtectionKeychain` opts every call (add / copy / delete)
/// into the modern keychain instead of the legacy file-based one. That is
/// where an access-control-protected item (see [`TouchId::enroll`])
/// belongs on macOS, and access to it is what the app's
/// `keychain-access-groups` entitlement grants. On the legacy keychain,
/// adding a user-presence item fails under the hardened runtime with
/// `errSecMissingEntitlement` (-34018). Add and match must agree on this
/// flag or they look at different stores.
fn base_pairs(account: &str) -> Vec<(CFType, CFType)> {
    vec![
        (key(unsafe { kSecClass }), key(unsafe { kSecClassGenericPassword })),
        (
            key(unsafe { kSecAttrService }),
            CFString::new(SERVICE).as_CFType(),
        ),
        (
            key(unsafe { kSecAttrAccount }),
            CFString::new(account).as_CFType(),
        ),
        (key(unsafe { kSecUseDataProtectionKeychain }), cf_true()),
    ]
}

impl BiometricProvider for TouchId {
    fn is_available(&self) -> bool {
        // The Keychain is always present on macOS. A device with no Touch
        // ID and no passcode would fail the presence check at read time
        // (surfaced then as Denied/Backend), which is rare enough that we
        // advertise availability and let the typed password cover it.
        true
    }

    fn enroll(&self, account: &str, secret: &str) -> Result<(), BioError> {
        // Overwrite semantics: delete any prior item, then add fresh. This
        // sidesteps SecItemUpdate and errSecDuplicateItem entirely.
        let _ = self.clear(account);

        let access = unsafe {
            let mut err: CFErrorRef = std::ptr::null_mut();
            let protection = kSecAttrAccessibleWhenUnlockedThisDeviceOnly as CFTypeRef;
            let ac = SecAccessControlCreateWithFlags(
                kCFAllocatorDefault,
                protection,
                ACCESS_CONTROL_USER_PRESENCE,
                &mut err,
            );
            if ac.is_null() {
                return Err(BioError::Backend(
                    "SecAccessControlCreateWithFlags returned null".into(),
                ));
            }
            // Adopt under the create rule so it is released with the dict.
            CFType::wrap_under_create_rule(ac)
        };

        let mut pairs = base_pairs(account);
        pairs.push((
            key(unsafe { kSecValueData }),
            CFData::from_buffer(secret.as_bytes()).as_CFType(),
        ));
        pairs.push((key(unsafe { kSecAttrAccessControl }), access));

        let dict = CFDictionary::from_CFType_pairs(&pairs);
        let status = unsafe { SecItemAdd(dict.as_CFTypeRef(), std::ptr::null_mut()) };
        match status {
            ERR_SEC_SUCCESS => Ok(()),
            ERR_SEC_DUPLICATE_ITEM => Err(BioError::Backend("keychain item already exists".into())),
            ERR_SEC_MISSING_ENTITLEMENT => Err(BioError::Backend(
                "SecItemAdd failed: -34018 (errSecMissingEntitlement); the app \
                 was signed without a keychain-access-groups entitlement"
                    .into(),
            )),
            other => Err(BioError::Backend(format!("SecItemAdd failed: {other}"))),
        }
    }

    fn retrieve(&self, account: &str, prompt: &str) -> Result<String, BioError> {
        let mut pairs = base_pairs(account);
        pairs.push((key(unsafe { kSecReturnData }), cf_true()));
        pairs.push((
            key(unsafe { kSecMatchLimit }),
            key(unsafe { kSecMatchLimitOne }),
        ));
        // The (localized) reason string shown above the Touch ID sheet.
        pairs.push((
            key(unsafe { kSecUseOperationPrompt }),
            CFString::new(prompt).as_CFType(),
        ));

        let dict = CFDictionary::from_CFType_pairs(&pairs);
        let mut result: CFTypeRef = std::ptr::null_mut();
        let status = unsafe { SecItemCopyMatching(dict.as_CFTypeRef(), &mut result) };
        match status {
            ERR_SEC_SUCCESS if !result.is_null() => {
                let data = unsafe { CFData::wrap_under_create_rule(result as _) };
                String::from_utf8(data.to_vec())
                    .map_err(|e| BioError::Backend(format!("stored secret not UTF-8: {e}")))
            }
            ERR_SEC_ITEM_NOT_FOUND => Err(BioError::NotEnrolled),
            ERR_SEC_USER_CANCELED | ERR_SEC_AUTH_FAILED => Err(BioError::Denied),
            ERR_SEC_MISSING_ENTITLEMENT => Err(BioError::Backend(
                "SecItemCopyMatching failed: -34018 (errSecMissingEntitlement); \
                 the app was signed without a keychain-access-groups entitlement"
                    .into(),
            )),
            other => Err(BioError::Backend(format!("SecItemCopyMatching failed: {other}"))),
        }
    }

    fn clear(&self, account: &str) -> Result<(), BioError> {
        let dict = CFDictionary::from_CFType_pairs(&base_pairs(account));
        match unsafe { SecItemDelete(dict.as_CFTypeRef()) } {
            ERR_SEC_SUCCESS | ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            other => Err(BioError::Backend(format!("SecItemDelete failed: {other}"))),
        }
    }
}

/// `kCFBooleanTrue` as a `CFType` for the `kSecReturnData` flag.
fn cf_true() -> CFType {
    use core_foundation::boolean::CFBoolean;
    CFBoolean::true_value().as_CFType()
}
