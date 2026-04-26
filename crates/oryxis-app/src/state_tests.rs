//! Unit tests for `state.rs` — loaded via `#[path] mod tests` so we
//! stay in the same module and can reach private items directly.

use super::PermBits;

#[test]
fn from_to_mode_round_trip_preserves_low_9_bits() {
    // Sweep a representative set of POSIX modes — all-set, common
    // 0o755 / 0o644, and the empty case.
    for mode in [0o000_u32, 0o644, 0o755, 0o700, 0o777, 0o604, 0o011] {
        assert_eq!(
            PermBits::from_mode(mode).to_mode(),
            mode,
            "round-trip failed for {:o}",
            mode
        );
    }
}

#[test]
fn from_mode_ignores_high_bits() {
    // setuid (0o4000), setgid (0o2000), sticky (0o1000) — the dialog
    // explicitly drops these and the caller is responsible for OR-ing
    // them back from the original mode. Confirm we don't accidentally
    // set them in `to_mode`.
    let high_plus_low = 0o7755;
    let bits = PermBits::from_mode(high_plus_low);
    assert_eq!(bits.to_mode(), 0o755);
}

#[test]
fn each_bit_corresponds_to_correct_octal() {
    // Spot-check a single bit at a time so a typo in the mask table
    // fails loudly.
    let cases = [
        (0o400_u32, "user_r"),
        (0o200, "user_w"),
        (0o100, "user_x"),
        (0o040, "group_r"),
        (0o020, "group_w"),
        (0o010, "group_x"),
        (0o004, "other_r"),
        (0o002, "other_w"),
        (0o001, "other_x"),
    ];
    for (mode, label) in cases {
        assert_eq!(
            PermBits::from_mode(mode).to_mode(),
            mode,
            "single-bit case {label}",
        );
    }
}
