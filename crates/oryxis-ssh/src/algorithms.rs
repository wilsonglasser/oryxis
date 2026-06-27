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

/// The legacy-fallback set for a category: the safe defaults first, then
/// every other supported algorithm appended. SSH negotiation is
/// client-order-authoritative, so a modern server still lands on a secure
/// algorithm while a legacy-only server can reach the appended entries.
/// This is what the "connect anyway" action pins, NOT raw `supported_*`
/// (which is in registration order and would demote secure ciphers).
fn secure_first(defaults: Vec<&'static str>, supported: Vec<&'static str>) -> Vec<&'static str> {
    let mut out = defaults;
    for s in supported {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

pub fn expanded_ciphers() -> Vec<&'static str> {
    secure_first(default_ciphers(), supported_ciphers())
}

pub fn expanded_kex() -> Vec<&'static str> {
    secure_first(default_kex(), supported_kex())
}

pub fn expanded_macs() -> Vec<&'static str> {
    secure_first(default_macs(), supported_macs())
}

pub fn expanded_host_keys() -> Vec<&'static str> {
    secure_first(default_host_keys(), supported_host_keys())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The legacy-fallback set must keep modern AEAD ciphers ahead of the
    /// legacy cbc/3des entries: SSH negotiation is client-order-authoritative,
    /// so a capable server still lands on a secure cipher after a fallback.
    /// Guards against pinning the raw registration-order `supported_ciphers`.
    #[test]
    fn expanded_ciphers_is_secure_first_and_deduped() {
        let e = expanded_ciphers();
        let pos = |n: &str| e.iter().position(|x| *x == n);
        if let (Some(chacha), Some(tdes)) =
            (pos("chacha20-poly1305@openssh.com"), pos("3des-cbc"))
        {
            assert!(chacha < tdes, "secure cipher must precede 3des-cbc");
        }
        // The merge must not duplicate the default entries.
        let mut sorted = e.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), e.len(), "expanded set has duplicates");
        // And it must include a legacy entry (the whole point of expanding).
        assert!(e.contains(&"aes256-cbc"));
    }
}
