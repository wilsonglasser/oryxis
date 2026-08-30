# Status ledger (append-only, updated per phase)

| Phase | Status | Note |
|---|---|---|
| 0 Scope | DONE | 00-SCOPE.md. Profile: Controlled Network; core = SSH client + vault. |
| 1 Provenance & hostile intake | DONE | Base 4b8ef3b8 + dirty tree (inherited transformation). build.rs ×2, install.sh, 14 CI workflows, scripts/ inspected before any cargo execution. No binaries/installers executed. |
| 2 Architecture & attack surface | DONE | 01–05 artifacts. Survey via subagent + direct verification of load-bearing paths. |
| 3 Threat model | DONE | 06-THREAT-MODEL.md (STRIDE + concrete abuse cases). |
| 4 Static analysis | DONE | Full-text sweeps (network, telemetry, exec, secrets, persistence); findings in 10-FINDINGS. |
| 5 Dependency & supply chain | DONE | Lockfile review, git-dep inventory, CI trust review; SBOM generated locally (sbom.cdx.json). |
| 6 Dynamic analysis | **BLOCKED** | No disposable VM / packet-capture harness available in this environment. Substituted: offline build + full unit/e2e suite + grep-level egress proof + operator rehearsal scripts (network-policy/). Caps verdict at CONDITIONAL PASS. |
| 7 Finding triage | DONE | 10-FINDINGS.{md,json} canonical register. |
| 8 Disposition & plan | DONE | 08-CAPABILITY-DISPOSITION.csv, 09-OFFLINE-TRANSFORMATION-PLAN.md. |
| 9 Implementation | DONE | Slices A–F, see 11-SECURITY-CHANGES.md. Includes completing the inherited half-transformation (build was broken in 3 places incl. a silent SQL column shift + INSERT arity bug). |
| 10 Validation | DONE (static+tests) | 12-VALIDATION.md: workspace 1108/1108 green (1 pre-existing darwin PTY env failure, reproduced at base HEAD); e2e on darwin with 2 measured coordinate repairs (agent-allow-add, monitor-dash) after the Sync feature-row removal; clipboard-paths skipped as macOS-geometry-limited (CI e2e is Linux-only; suite + fixes ready for CI); removal greps zero-hit; lockfile 834→777. Dynamic rehearsal = operator runbook item. |
| 11 Decision | **CONDITIONAL PASS** | 14-GO-NO-GO.md. |
