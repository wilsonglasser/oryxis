# 00 — Scope, feasibility, protection plan

Date: 2026-08-30 · Agent: AEGIS OFFLINE · Mode: audit + authorized implementation (working copy only)

## Target

| Field | Value |
|---|---|
| Application | **Oryxis** — native Rust (iced/wgpu) desktop SSH client (Termius alternative) with encrypted local vault |
| Source | Local working copy at `/Users/srv/Documents/Projects/oryxis` |
| Base revision | `4b8ef3b8e323f21d5665ce00e3fe43a75a18a7e4` (origin/main) |
| Working tree | **DIRTY — inherited mid-transformation** (105 paths; ~20,988 lines already deleted: app-side cloud providers, sync engines, updater, plugin downloader, vendor mirror routing). This audit *preserves and completes* that work; it does not revert it. |
| Platforms | Linux, macOS, Windows (source-level audit + build on darwin/arm64; no cross-builds run) |
| Provenance note | Repo is a clone of `wilsonglasser/oryxis` (AGPL-3.0); git user `rmilea`. No release artifacts audited — source tree only. |

## Derived mission (conservative assumptions, no user answers available)

Required core capabilities (KEEP):
1. SSH connect/run (keys, agent, jump hosts, ProxyCommand with per-dial consent), SFTP dual-pane + console, mosh, telnet, serial, zmodem.
2. Local encrypted vault (SQLite + ChaCha20Poly1305, Argon2id) for hosts/keys/snippets/history; biometric unlock (OS keystore); portable export/import.
3. Local ssh-agent bridge (Unix socket / Windows named pipe), port forwarding, Wake-on-LAN, RDP/VNC launcher, local terminal, GIF export plugin, themes, i18n, importers (PuTTY etc.).
4. Local MCP bridge (`oryxis-mcp`, stdio, spawned by external clients).

Explicitly removable (consistent with the inherited transformation — REMOVE):
- Cloud-provider discovery/import (AWS/Azure/GCP/K8s) and their plugin subprocess model.
- Device sync (P2P QUIC engine, WebRTC-style signaling, relay server, Cloudflare worker, git/WebDAV/SFTP/folder transports) and the vault sync API surface.
- Auto-updater / release lookups / vendor download mirror (`dl.oryxis.app`, `dl-cn.oryxis.app`).
- Remote plugin catalog + plugin binary downloads.

Retained user-initiated network (CONTROLLED carve-outs — disclose, do not remove):
- **AI assistant**: user-configured provider + vault-stored API key, invoked explicitly from a session. Default-off (no provider configured until the user acts). Terminal context egress is real and disclosed; operators who do not want it simply never configure a provider.
- **Pinned font fetches**: CJK + Nerd Font pack faces from commit-pinned `raw.githubusercontent.com` URLs, SHA-256 + length verified, https-only, size-capped, atomic install. Triggered by the user's language/font selection (and boot heal of the *configured* font). The vendor mirror layer for these fetches is already deleted; this audit re-points them to the canonical URL only.

## Offline profile decision

**Controlled Network** (not Strict Offline): the product's essential purpose is dialing user-configured remote hosts. Per policy §1: SSH/SFTP/mosh/telnet/WoL/RDP-VNC are user-initiated connections to user-configured targets — the minimum essential controlled portion. Everything else (telemetry, updates, cloud, sync, catalog fetches) is strict: removed. AI + fonts are controlled carve-outs, both integrity-pinned or user-keyed, both disclosed in 04-EGRESS-MATRIX.csv.

Assurance target: `high` (not air-gapped; dynamic/network-denial rehearsal is limited — see 07-DYNAMIC-TESTS.md).

## Authorization & boundaries

- Implementation authorized **inside the supplied working copy only**. No commits, pushes, PRs, releases, or external messages (none requested).
- No destructive user-data operations. The vault format is preserved; schema changes are removal-only (dead sync tables/APIs) with old files still readable.
- No real credentials, no production endpoints dialed, no third-party active testing. Build/test only; `cargo --offline` preferred.
- Execution of repository-provided tooling: `install.sh` (inspected: builds + `sudo cp` — NOT executed), `build.rs` files (inspected: `oryxis-app` runs `git describe` for a version stamp; `oryxis-sync`'s dies with the crate), CI workflows (inspected, not run), `scripts/keygen-dev.sh` (inspected, not executed). No package lifecycle scripts run beyond ordinary `cargo check/test` of first-party code.

## Assets, adversaries, acceptance

Assets: vault contents (host inventories, private keys, passwords, snippets, session logs), vault master password, AI API key, MCP/agent tokens, clipboard, terminal I/O.
Adversaries: malicious/compromised dependency or upstream commit; hostile network (TLS interception, DNS poisoning); malicious imported file (portable export, theme deep link, ssh_config, PuTTY import); same-user malware (boundary: detection/hardening only — not fully defensible on a personal machine); stolen disk/backup (vault at rest).
Acceptance gates: §5 of the mandate; condensed — (1) build + tests green offline, (2) zero non-essential egress reachable in code, (3) every retained egress row disclosed + integrity-pinned or user-keyed, (4) removals proven by reference/lockfile/binary-content checks, (5) MCP launcher fails closed on unsigned binaries.
