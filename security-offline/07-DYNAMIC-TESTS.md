# 07 — Dynamic tests

**Status: BLOCKED in this environment.** No disposable VM, network
namespace isolation, or packet-capture harness controllable from this
session; the mandate forbids weakening the analyst machine or running
the GUI app with real user data. Dynamic coverage was therefore NOT
performed, and the final decision is capped accordingly (see
14-GO-NO-GO: CONDITIONAL PASS, mandatory operator rehearsal).

## What substitutes for it (static + build-time evidence)

1. Egress inventory at code level: every `reqwest`/socket use enumerated
   (04-EGRESS-MATRIX.csv), each mapped to a trigger and integrity
   contract; removal classes proven by zero-hit greps (12-VALIDATION.md).
2. Full unit/integration suites executed (`cargo test --locked
   --workspace`), including vault round-trips, portable-import
   hardening (deny-list fixtures), redaction tests, agent/PTY patience
   tests, and the headless e2e `.ice` suite via the harness build
   (results in 12-VALIDATION.md).
3. Build-time network behavior: `--locked` builds resolve from the
   pinned lockfile; the only network cargo touched was fetching pinned
   git revisions into the local cache (build-time, disclosed).

## Operator rehearsal (mandatory before trusting the build)

Run `network-policy/rehearsal-{macos,linux}.sh` while exercising: boot,
5-minute idle, vault unlock/lock, host connect, SFTP browse, settings
tour, font picker (offline path), MCP toggle (empty-cache path). Any
drop-log line outside the documented allowlist is a failed gate and a
bug report against this audit.

## What dynamic testing would additionally cover (unresolved)

- Runtime confirmation that no delayed retry/fallback exists in the
  font path after a failed fetch (static read says none).
- Behavioral check of the harness daemon under release builds (static:
  feature-gated out).
- Memory/CPU baseline of the transformed binary vs baseline.
