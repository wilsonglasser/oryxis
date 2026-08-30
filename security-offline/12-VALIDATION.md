# 12 — Validation

## Clean-room status

Built and tested in the existing working copy (the transformation is the
deliverable; a pristine `git worktree` at base HEAD was used for exactly
one differential check — the PTY test — and removed after).

## 1. Compile gates — PASS

- `cargo check --workspace --all-targets`: **0 errors**, 1 warning
  (`is_packaged`, platform-gated by design, annotated `cfg_attr`).
- `cargo build --locked -p oryxis-app --features harness`: PASS (40 s).

## 2. Unit / integration suites — PASS (1 pre-existing env failure)

- `cargo test --locked --workspace`: **1108 passed / 0 failed**, plus
  `oryxis-serial::pty_interop::round_trips_over_a_real_pty` **FAILED:
  ENOTTY ("Not a typewriter")** — reproduced identically in a pristine
  worktree at base `4b8ef3b8` (evidence/pty-preexisting.txt): a
  darwin-27 platform behavior, not a regression. Excluding it, every
  crate is green, including:
  - `oryxis-vault` (257 tests: connection round-trips through the
    re-aligned SQL, portable-import deny-list hardening, crypto
    rotations, legacy-vault migration);
  - `oryxis-app` (825 tests incl. deep-link strictness, privacy
    redaction, agent protocol, AI wire parser, plugin signature
    verify);
  - MCP crate protocol tests (16).

## 3. Headless e2e suite — PASS on darwin for everything reachable; Linux CI remains the authoritative gate

Context: upstream CI runs the `.ice` suite **only on ubuntu-latest**; several
scenarios encode Linux geometry (terminal row pitch) and Linux modifier
conventions. On this darwin host:

- **Transformation-caused drift, found and fixed (measured with the harness
  `find` inspector, not guessed):** the removed Sync feature row shifted the
  Features-column toggles up one row — `agent-allow-add.ice` 395→350,
  `monitor-dash.ice` 298→260. Both scenarios now pass on darwin.
- **Verified green on darwin:** agent-allow-add, monitor-dash, settings-tab,
  font-pack, vault-stats-keychain, plus every scenario preceding each batch
  abort (the `a*` group).
- **Failing on darwin, classified environment (not regression):**
  - `clipboard-paths.ice` — deterministic here; pixel analysis shows the
    selection row y=123 lands in the inter-glyph gap (row pitch ~35px on
    darwin vs Linux metrics). Terminal-widget code untouched (git-verified).
  - `command-palette.ice` — ctrl+shift+p is a Linux chord; the default
    binding is `primary_logo`-based (Cmd on macOS). Binding tables untouched.
  - `theme-import-redirect.ice` — serve-mode replay shows the wheel scroll
    and overlay paste-field coordinates miss on darwin metrics; every file
    in the redirect logic path is git-identical to base HEAD.
- Deleted scenarios for removed features: `cloud-open-plugins`,
  `download-mirror`, `sync-passphrase-reopen`.
- Boot log from the suite confirms the new MCP gate on an empty cache:
  `MCP launcher not refreshed from plugin cache … plugin binary not found`
  (DEBUG), boot continues — fail-soft at boot, fail-closed at install.

**Open item for the Linux CI run** (infrastructure unchanged, not executable
here): the full suite on ubuntu-latest with the two repaired coordinates.

## 4. Removal verification — PASS

`evidence/removal-greps.txt`; highlights (production code, i18n excluded):
`CloudMessage` 0 · `View::Cloud` 0 · `oryxis-sync|oryxis-relay` 0 ·
`dl.oryxis.app` 0 · `api.github.com` 0 · `net_mirror` 1 (a historical
comment in the portable deny-list, which itself is protective) ·
`oryxis-cloud` 6 (the plugin cache's binary-naming convention and its
test — intentional, the cache layout is provider-generic) · `signaling`
4 (deny-listed legacy setting keys + their tests — protective) ·
`download_mirror` 3 (deny-list entry + comment + hardening test fixture
— protective).

## 5. Supply chain — PASS with disclosed residuals

- Lockfile pruned 834 → 777 packages (−1,181 lines), still fully pinned;
  `--locked` enforced on every build/test.
- 8 unique git deps, all exact-rev pinned (AEGIS-0007 fork-trust
  residual stands).
- SBOM: `sbom.cdx.json` (CycloneDX 1.5, generated locally from
  `cargo metadata` + lockfile digest).
- Advisory scan: **not run** (no cargo-audit/deny available; AEGIS-0009).
  Operator step: run `cargo audit` from a trusted toolchain install.

## 6. Offline compliance — static PASS, dynamic PENDING (operator)

- Code-level egress inventory complete (04-EGRESS-MATRIX): 5 families,
  all user-initiated or integrity-pinned; zero vendor/automatic
  callbacks remain in code.
- Network-denial rehearsal scripts ready (network-policy/) — mandatory
  operator step before final trust (AEGIS-0010).

## 7. Data preservation — PASS

No user data touched; vault format only lost *creation* of dead sync
tables; legacy vaults open unchanged (migration tests green); portable
import deny-list still blocks re-introduction of removed-subsystem
settings.

## 8. Regression tests for findings

- AEGIS-0002: existing round-trip suites now exercise the corrected
  SQL (65/65 arity asserted at edit time; mapper alignment covered by
  `connections` tests).
- AEGIS-0001: launcher gate covered by compile + existing
  `plugins::verify` tests (malformed signature → IntegrityError); a
  full signed-cache fixture is an operator-runbook procedure (the CI
  signing pipeline for releases was removed with the workflows).
