# 13 — Operations runbook (offline edition)

## Build (verified this session)

```sh
cargo build --release --locked -p oryxis-app        # the app
cargo build --release --locked -p oryxis-mcp        # the MCP plugin binary (optional)
cargo build --release --locked -p oryxis-plugin-signer  # signing tool (dev/self-hosted releases)
```

`--locked` enforces the pruned `Cargo.lock` (777 packages, all rev-pinned).
Network is needed once to populate `~/.cargo` from the pinned revisions;
after that, `--offline` works (verified for everything except a fresh
`arboard` git checkout — pre-seed the cache or allow that one fetch).

## Install / run

- Run `target/release/oryxis` directly, or use `install.sh` (inspected:
  builds + `sudo cp` binary/icon/desktop entry; no network, no scripts
  fetched).
- All state lives under `~/.oryxis/` (`ORYXIS_HOME` overrides): `vault.db`
  (0600), `fonts/`, `plugins/`, `bin/`, `agent.sock`, `runtime/`.
- Expected processes: exactly one `oryxis` (single-instance enforced);
  optionally `oryxis-gif` during GIF export; external clients may spawn
  `~/.oryxis/bin/oryxis-mcp`.
- Expected listeners: `~/.oryxis/agent.sock` (Unix) or the named pipe
  (Windows) only when the ssh-agent bridge is enabled; nothing else,
  ever. Dev builds with `--harness` add loopback TCP 6799.

## Updates (offline by design)

There is no auto-updater. To update: build the new revision, replace the
binary, done. The vault format is forward-compatible within this edition
(only removals were made; old vaults open unchanged; legacy sync tables
are dropped only if you destroy-and-recreate the vault).

### Installing the MCP plugin binary (self-built)

Release builds fail closed on unsigned plugin binaries. For a self-built
release you control the trust anchor:

1. Generate a keypair: `ORYXIS_SIGNING_KEY` env (hex) for the signer, and
   build the app with `ORYXIS_PROD_PUBKEY_HEX=<matching pubkey hex>` so the
   binary trusts your key (malformed value = compile error by design).
2. Sign: `oryxis-plugin-signer --key "$ORYXIS_SIGNING_KEY" <binary>`
   → sha256 + base64 signature.
3. Place the binary at `~/.oryxis/plugins/mcp/<version>/oryxis-mcp` and a
   `manifest.json` (see `plugins/mcp.json` for the shape) whose entry for
   `<version>` carries that sha256 + signature.
4. Boot (or toggle Settings → MCP) — the launcher
   `~/.oryxis/bin/oryxis-mcp` installs only after the signature verifies.

Debug builds: `oryxis-plugin-signer --dev` signs with the public dev seed
those builds trust.

## Backup / restore

- Vault: copy `~/.oryxis/vault.db` while the app is closed (SQLite single
  file). Or use the in-app portable export (password-encrypted, category
  checkboxes, security-sensitive settings excluded by deny-list).
- Restore = copy the file back / import the `.oryxis` export.
- Session recordings live inside the vault; nothing else is needed.

## Least-privilege deployment

- Run as your normal user; the app needs no elevation anywhere (install.sh's
  `sudo cp` is the only elevated step, and only for system-wide installs).
- No services, launch agents, or scheduled tasks are created — verified.

## Strict network policy (recommended)

Apply the rehearsal rules permanently (`security-offline/network-policy/`)
minus the font/AI holes if you don't use those features: default-deny
egress, allow only your SSH destinations. The app degrades locally:
missing fonts fall back to bundled/system faces, AI is inert without a
configured provider, MCP works offline by construction.

## Monitoring (local only)

- Debug log: Settings → Advanced → debug log → `~/.oryxis/oryxis-debug.log`
  (opt-in, size-bounded, no secrets).
- Health: the app either renders or it doesn't; there is no telemetry to
  check by design.

## Failure modes

| Symptom | Cause | Recovery |
|---|---|---|
| "launcher: no cached manifest…" in MCP panel | plugin cache empty/unsigned | follow the MCP install procedure above |
| CJK/pack font missing, toast stuck | font fetch denied | pick a bundled font, or pre-cache `~/.oryxis/fonts/` from a connected machine (verify the pinned sha256) |
| Old sync/cloud settings in imported file | deny-list keeps them out | nothing to do; they never apply |
| Vault from the pre-offline edition | legacy sync tables present | harmless; dropped on destroy-and-recreate |

## Rollback

The whole transformation is uncommitted working-tree state:
`git checkout -- .` restores upstream 4b8ef3b8 wholesale, or revert
individual paths. User data (`~/.oryxis`) was never touched by the audit.
