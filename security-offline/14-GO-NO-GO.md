# 14 — Go / No-Go

# Decision: **CONDITIONAL PASS**

**Profile:** Controlled Network (offline edition of Oryxis: SSH-family core; everything cloud/sync/update removed)
**Revision:** base `4b8ef3b8e323f21d5665ce00e3fe43a75a18a7e4` + this working tree (309 changed paths, +882/−49,319 lines vs HEAD; uncommitted by instruction — nothing was committed or pushed)
**Platforms tested:** darwin 27 / arm64 (build + full unit/integration suites + partial e2e). Linux/Windows: compile-verified only (`cargo check --workspace --all-targets` is host-independent for the app crate's cfg set; CI matrix not executable here).

## Outcome

Safe to use **under the documented threat model and operating conditions**:
a single-user personal endpoint where the operator accepts (a) the inherent
supply-chain trust in the upstream maintainer's fork pins, (b) the opt-in AI
and font-fetch carve-outs, and (c) runs the network rehearsal once. The
transformation removed every automatic, vendor-owned, and cloud-dependent
behavior; what remains is the SSH family you dial, two disclosed opt-in
egress paths, and local files/sockets.

## Conditions (mandatory)

1. Run `security-offline/network-policy/rehearsal-*` once and confirm the
   allowlist behavior (the dynamic phase was BLOCKED here — see 07).
2. Do not configure an AI provider on machines where terminal-context
   egress is unacceptable (AEGIS-0003); decline the master-password embed
   in `~/.claude.json` unless explicitly wanted (AEGIS-0004).
3. Self-built MCP binaries must be signed per the runbook (release builds
   fail closed on unsigned plugin binaries — AEGIS-0001 fix).
4. Run `cargo audit` from a trusted toolchain at least once per dependency
   refresh (AEGIS-0009: no advisory DB available during this audit).
5. Push through Linux CI to confirm the full `.ice` suite with the two
   repaired coordinates (12-VALIDATION §3 open item).

## Highest-risk residuals

- Fork-trust supply chain (AEGIS-0007, MEDIUM, inherent).
- Same-user compromise remains outside the model (a pre-existing launcher
  on disk from before the transformation is not re-verified at boot).
- ProxyCommand `sh -c` with consent gate (AEGIS-0005, kept).
- Terminal parser fuzzing and DoS limits: NOT TESTED here.

## Unresolved Critical/High findings: **none** (both High findings fixed and
tested this session: AEGIS-0001 launcher gate, AEGIS-0002 build/SQL
integrity).

## Artifacts

`security-offline/` — 00-SCOPE … 14-GO-NO-GO, REMOVALS.json, sbom.cdx.json,
provenance.json, checksums.sha256, network-policy/, evidence/,
STATUS-LEDGER.md. The transformed application is the working tree itself.
