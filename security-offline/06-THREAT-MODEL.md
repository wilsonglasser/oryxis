# 06 — Threat model

Assets: vault contents (host inventory, private keys, passwords, TOTP, snippets, session recordings), master password, AI API key, MCP/agent tokens, clipboard, terminal I/O.
Trust boundaries: see 01-ARCHITECTURE §Trust boundaries.

## Abuse cases (concrete, mapped to controls/gaps)

| # | Threat | STRIDE | Path | Existing control | Gap / status |
|---|---|---|---|---|---|
| T-01 | Malicious/compromised upstream dependency or git dep (iced/alacritty forks, russh) | Tamper/Elevate | build → binary | lockfile pins exact revs; git deps pinned by lockfile | inherent supply-chain residual; SBOM + lockfile review done; no runtime sandbox can fix a malicious compiler — accepted under documented threat model |
| T-02 | Hostile SSH server output compromises terminal parser | Tamper | connect → vte/alacritty parse → render | parser memory safety; privacy mode; no shell-out of terminal content | residual: parser bugs (Rust memory safety bounds it); NOT TESTED by fuzzing here |
| T-03 | ProxyCommand shell injection via malicious ssh_config | Elevation/Tamper | config → `sh -c` | per-dial consent dialog (`connect.rs:540-554`) | accepted with consent gate; F-05 |
| T-04 | Malicious portable export / PuTTY import / theme deep link | Tamper/Info | file parse → vault write | strict parse, 128 KiB cap, security-settings deny-list, category counts | retained; deny-list reviewed |
| T-05 | Same-user malware plants binary in plugin cache → external MCP clients spawn it with token (+ optional embedded master password) | Elevation | cache → launcher → spawn | user-private path; **NEW: Ed25519 signature gate at launcher copy (fail closed in release)** | F-01 fixed this session; debug builds also honor dev key |
| T-06 | Master password leaks into `~/.claude.json` on disk | Info disclosure | MCP install | explicit confirmation dialog; disclosed in-panel | F-04: retained by design (documented condition); user can decline |
| T-07 | AI provider receives terminal context and tool-call content | Info disclosure | explicit chat call | off by default; user key; user-chosen endpoint | F-02: retained carve-out; operators must leave unconfigured if undesired |
| T-08 | Font fetch MITM/swap | Tamper | HTTPS GET → cache | SHA-256 + length pin, size cap, atomic write, https-only | integrity holds even against hostile host; confidentiality irrelevant (public fonts) |
| T-09 | Stolen disk/backup → vault at rest | Info disclosure | offline attack | Argon2id calibrated KDF + AEAD, 0600 | strong; biometric keystore entry also OS-protected |
| T-10 | Local listener abuse (agent socket/pipe squat, CSRF-style localhost) | Spoof/Elevation | socket connect | 0700 dir, stale-probe, DACL, first-instance, confirm mode | robust for the local single-user model |
| T-11 | Vault sync/cloud re-introduction via legacy settings import | Tamper | portable import | deny-list retains sync_*/download_mirror/update_channel keys as blocked | hardened this session (deny-list kept deliberately) |
| T-12 | Supply-chain re-add via CI (mirror publish, plugin release signing) | Tamper | GitHub Actions | **removed**: publish-mirror, release-{aws,azure,gcp,k8s,relay} workflows deleted | reduced to app-only release paths |
| T-13 | DNS/redirect tricks on font host | Spoof | DNS → HTTPS | SNI/TLS cert validation + hash pin | pin is the backstop; no vendor fallback exists anymore |
| T-14 | Decompression/parser bombs in SFTP/imports | DoS | transfer/import | size caps on font bodies; SFTP bounded by protocol | partially controlled; NOT TESTED by fuzzing (limitation) |
| T-15 | Hostile network observer correlates user behavior | Info | any egress | post-transformation egress = SSH (necessary), AI (opt-in), fonts (opt-in feature) | reduced from 10 egress families to 5; see 04-EGRESS-MATRIX |

## Removed abuse paths (transformation outcome)

Signaling-worker TOFU registry abuse, relay inbox poisoning, sync passphrase brute-force (offline Argon2 on a *shared* secret), cloud credential storage in vault, auto-update download-and-run, mirror-substituted update metadata, plugin catalog poisoning — all deleted with their code paths (see REMOVALS.json).

## Test-case mapping

Each finding in 10-FINDINGS.md carries its validation evidence; T-05/T-11 have new/retained regression tests or grep proofs in 12-VALIDATION.md.
