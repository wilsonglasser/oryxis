//! Catalog of SSH algorithm names available for per-host overrides
//! (legacy-cipher support). Mirrors russh's registered algorithm sets,
//! filtered to the names that make sense to pin from the UI. The strings
//! are the on-the-wire algorithm names stored in
//! `Connection.{ciphers,kex,macs,host_key_algorithms}`.

/// Symmetric ciphers russh can negotiate, in its own preference order,
/// minus the no-encryption placeholders (`clear` / `none`) which must
/// never be offered as a real choice. Includes the legacy cbc / 3des
/// entries (3des only when the `des` feature is on, which it is here).
pub fn supported_ciphers() -> Vec<&'static str> {
    russh::cipher::ALL_CIPHERS
        .iter()
        .map(|n| (*n).as_ref())
        .filter(|s| !matches!(*s, "clear" | "none"))
        .collect()
}

/// Key-exchange algorithms, minus the `ext-info-*` negotiation markers
/// (not selectable) and `none`. Includes legacy dh-group1/14-sha1.
pub fn supported_kex() -> Vec<&'static str> {
    russh::kex::ALL_KEX_ALGORITHMS
        .iter()
        .map(|n| (*n).as_ref())
        .filter(|s| !s.starts_with("ext-info") && *s != "none")
        .collect()
}

/// MAC algorithms, minus `none`. Includes legacy hmac-sha1 (+ etm).
pub fn supported_macs() -> Vec<&'static str> {
    russh::mac::ALL_MAC_ALGORITHMS
        .iter()
        .map(|n| (*n).as_ref())
        .filter(|s| *s != "none")
        .collect()
}

/// Host-key signature algorithms russh accepts by default. The legacy
/// SHA-1 `ssh-rsa` is already in this set (on by default), so pinning is
/// mostly for restricting a host to a specific algorithm.
pub fn supported_host_keys() -> Vec<&'static str> {
    russh::Preferred::DEFAULT
        .key
        .iter()
        .map(|a| a.as_str())
        .collect()
}

/// The safe default subset russh negotiates for each category (the
/// `Auto` set). Used to pre-fill a per-host pin when the user switches a
/// category from Auto to a custom list, so they start from a working set
/// and add the legacy entries (or trim) rather than from nothing.
pub fn default_ciphers() -> Vec<&'static str> {
    russh::Preferred::DEFAULT.cipher.iter().map(|n| n.as_ref()).collect()
}

pub fn default_kex() -> Vec<&'static str> {
    russh::Preferred::DEFAULT
        .kex
        .iter()
        .map(|n| n.as_ref())
        .filter(|s| !s.starts_with("ext-info"))
        .collect()
}

pub fn default_macs() -> Vec<&'static str> {
    russh::Preferred::DEFAULT.mac.iter().map(|n| n.as_ref()).collect()
}

pub fn default_host_keys() -> Vec<&'static str> {
    supported_host_keys()
}
