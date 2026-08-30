# 10 — Findings register

Status labels: CONFIRMED / PROBABLE / POSSIBLE / NOT REPRODUCED / NOT TESTED. Severity reflects actual reachability, privilege, exposure and data sensitivity in this deployment context (personal endpoint, single user).

## AEGIS-0001 — MCP launcher copied from cache without signature re-verification
- **status** CONFIRMED · **severity** HIGH · **confidence** HIGH · **category** hardening
- **evidence** pre-edit `crates/oryxis-app/src/mcp_install.rs:56-87` copied `cache::current_binary("mcp")` → `~/.oryxis/bin/oryxis-mcp` (0755) with no verification; the download path that used to verify was deleted with `plugins/download.rs`; `plugins/verify.rs` (Ed25519, baked-in prod key, dev key in debug) had zero remaining callers.
- **trigger** plugin install/refresh; pre-transformation launcher persists on disk
- **attacker preconditions** same-user code execution OR any process able to write `~/.oryxis/plugins/mcp/**`
- **risk** external MCP clients (Claude Desktop/Code, Cursor) spawn the launcher with `ORYXIS_MCP_TOKEN` and, if the user opted in, the vault master password embedded in client config — a planted binary exfiltrates both
- **fix (implemented)** `verify_cached_binary()`: manifest SHA-256 match + Ed25519 verify before any copy; release fails closed, debug honors dev key; descriptive errors. See 11-SECURITY-CHANGES.
- **residual** an already-installed launcher from before the transformation is not re-verified at boot (same-user tamper ≡ same-user code exec); rotation re-verifies.

## AEGIS-0002 — Inherited half-transformation: broken build + silent SQL column shift + INSERT arity bug
- **status** CONFIRMED (at intake) · **severity** HIGH · **confidence** HIGH · **category** vulnerability (data integrity)
- **evidence** (a) `fonts.rs:802` called deleted `net_mirror` (build broken); (b) `store/connections.rs` SELECT lists had `cloud_ref`/`customized_fields` removed while row-mapper indexes still expected them at 25/29 → every field read at index ≥25 silently mis-mapped (env_vars read as session_logging etc.); (c) INSERT had 65 column names vs 67 placeholders/values → runtime SQL error on every save.
- **fix (implemented)** direct pinned-URL fetch; NULL sentinels at retired SELECT positions (documented, index-stable); INSERT renumbered to 65/65 and verified programmatically; 257 vault tests green.
- **validation** `cargo test -p oryxis-vault` incl. connection round-trips.

## AEGIS-0003 — AI assistant egress: terminal context to third-party LLM
- **status** CONFIRMED (by design) · **severity** MEDIUM · **privacy** · **retained**
- **evidence** `ai/mod.rs` (provider catalog w/ default URLs; user override), `ai/wire.rs` SSE, `execute_command` tool prompt `ai/mod.rs:26`, key in vault.
- **controls** default-off (no provider configured until user acts), user-chosen endpoint+key, Privacy Mode redaction before context leaves, judge fail-safe requires confirmation on transport error.
- **decision** Controlled-Network carve-out retained (explicit user configuration = explicit consent). **Operating condition:** do not configure a provider on machines where this egress is unacceptable.

## AEGIS-0004 — Vault master password embedded in `~/.claude.json` (opt-in)
- **status** CONFIRMED (by design) · **severity** MEDIUM · **retained**
- **evidence** `mcp.rs:57-172`, `mcp_install.rs` post-install refresh; explicit confirmation dialog; plaintext on disk in the client's config file.
- **decision** kept (feature contract: external client needs it to unlock the vault bridge); disclosed here as a mandatory-awareness item. Operators should decline the embed and use the token-only config.

## AEGIS-0005 — ProxyCommand `sh -c` execution from ssh_config
- **status** CONFIRMED · **severity** MEDIUM · **kept with control**
- **evidence** `oryxis-ssh/src/engine/connect.rs:540-570`; per-dial consent dialog; approved-fingerprint set (`proxy_consent.rs`, vault `trusted_proxy_commands` table).
- **decision** essential ssh_config compatibility; consent gate retained.

## AEGIS-0006 — Silent boot-time font fetch
- **status** CONFIRMED · **severity** LOW · **kept (integrity-pinned)**
- **evidence** `fonts.rs` `boot_pack_tasks` downloads the *configured* terminal font when missing; commit-pinned URLs, SHA-256+length pin, size cap, https-only, atomic install, no vendor mirror (removed this session).
- **decision** kept: heals the user's chosen font; integrity does not depend on the host. Disclosed in 04-EGRESS-MATRIX (E-03).

## AEGIS-0007 — Supply-chain trust in maintainer forks
- **status** CONFIRMED · **severity** MEDIUM · **residual, accepted**
- **evidence** Cargo.lock git deps: `wilsonglasser/iced` (branch), `wilsonglasser/alacritty`, `wilsonglasser/serialport-rs`, `wilsonglasser/winit`, `wilsonglasser/iced_fonts`, `wilsonglasser/winrt-notification`, `1Password/arboard`, `iced-rs/cryoglyph` — all lockfile-pinned to exact revs; `oryxis-app/build.rs` runs `git` at build time.
- **decision** inherent to the upstream project's fork strategy; pinning verified; no runtime sandbox can mitigate a hostile compiler. Documented threat-model boundary.

## AEGIS-0008 — Dev plugin-signing seed committed in source
- **status** CONFIRMED · **severity** INFO · **by design**
- `oryxis_plugin_protocol::DEV_PLUGIN_SIGNING_SEED` is public by construction; `verify::active_pubkeys()` trusts it **only** under `cfg!(debug_assertions)`; prod key overridable at build time via `ORYXIS_PROD_PUBKEY_HEX` (malformed value = compile error, all-zero key = reject everything).

## AEGIS-0009 — No advisory-database scan performed
- **status** NOT TESTED · **severity** — · **limitation**
- `cargo audit` / `cargo deny` / `cargo cyclonedix` unavailable in this environment and not installed (installing unreviewed tooling would violate the mandate's tool-trust rules). Mitigation: lockfile fully pinned; SBOM generated locally from `cargo metadata`; operator runbook includes an advisory-check step.

## AEGIS-0010 — Dynamic analysis not performed (no disposable VM / packet capture here)
- **status** BLOCKED · **limitation** — see 07-DYNAMIC-TESTS.md; capped the verdict at CONDITIONAL PASS. Operator rehearsal scripts provided in `network-policy/`.

## AEGIS-0011 — Dead settings/UI residue of removed features
- **status** CONFIRMED (at intake) · **severity** LOW · **hygiene** · **fixed** — cloud auto-refresh toggle/messages/prefs/settings-index entries, download-mirror denylist prose, e2e scenarios, and ~200 dead i18n keys × 24 languages removed (see 11-SECURITY-CHANGES; final counts in 12-VALIDATION).

## Cleared classes (NOT OBSERVED)
- Telemetry/analytics/crash-upload/tracking IDs: none in first-party code (keyword sweep clean; only SSH-key "fingerprint" matches).
- Persistence (launchd/systemd/cron/RunKeys/schtasks/login items): none.
- Hard-coded secrets in source: none (fixtures/tests only).
- Inbound non-loopback listeners: none (agent socket/pipe is local + authenticated by filesystem/DACL; harness is dev-only loopback).
- Vendor endpoints in production code: none remaining (dl.oryxis.app, api.github.com, signaling — all deleted; remaining URLs are the pinned font hosts + display/test strings).

## Fleet re-verification (2026-08-30, second pass)

Four parallel read-only agents re-audited the tree. Outcome: two defects found and fixed same-session, four informational disclosures recorded. No change to the CONDITIONAL PASS decision.

Fixed:
- **TOCTOU in the MCP launcher gate** (AEGIS-0001 follow-up): `verify_cached_binary` hashed one read while `sync_launcher_from_cache` copied a fresh re-read of the path. The gate now returns the verified in-memory bytes and the launcher write uses those — verified == copied by construction. (`mcp_install.rs`, mcp_install tests green.)
- **Four CI workflows invoked the deleted `publish-mirror.yml`** (release/nightly/release-mcp/release-gif `mirror:` jobs — guaranteed job failure on any run); dead `ORYXIS_SIGNALING_*` env wiring (ci/release/nightly) and the stale `signaling-worker/**` path filter removed. All remaining workflows parse.

Informational (no live vulnerability; recorded for completeness):
- **AEGIS-0012 (INFO):** `oryxis-ssh/src/sftp_harness.rs:32` — hardcoded throwaway ed25519 host key for the in-process test SSH server (comment discloses provenance; test-only code path).
- **AEGIS-0013 (INFO):** user-clicked `OpenUrl` links to `oryxis.app` and the GitHub repo in About/Themes views — no automated fetch.
- **AEGIS-0014 (INFO):** SSH local forwards honor a rule-provided `listen_host`, `0.0.0.0` included (`oryxis-ssh/engine/forwarding.rs:398`) — intentional LAN-exposure capability, config-controlled.
- **AEGIS-0015 (INFO):** archive extraction restores source-supplied unix mode bits (`oryxis-archive/local.rs:108`) — an attacker-crafted archive can carry group/world bits; user-directed extract only.
