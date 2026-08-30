# 11 — Security changes implemented

All changes live uncommitted in the working tree (rollback = git). Grouped by slice; evidence file refs are to the post-change state.

## S1 — Compile-integrity repairs to the inherited half-transformation
1. `fonts.rs` fetch: deleted `net_mirror::candidates` loop → single direct GET of the commit-pinned canonical URL. https-only, 15 s/90 s timeouts, SHA-256+length pin, +64 KiB body cap, atomic tmp→fsync→rename→dir-fsync all preserved. The vendor mirror layer (dl.oryxis.app / dl-cn.oryxis.app) stays dead.
2. `oryxis-vault` sync/cloud API removal: tombstone calls deleted from 9 store modules; `mod sync` + `SyncPeerRow` + `Tombstone` + `derive_sync_secret` + `SYNC_SECRET_SALT` removed; schema no longer creates `sync_peers`/`sync_metadata` (legacy vaults: dropped by `destroy_and_recreate`; their encrypted settings still rotated by key on password change); `convert_all_fields` no longer walks the nonexistent table (was a password-change breaker on fresh vaults); typed sync settings accessors removed (no remaining callers).
3. `store/connections.rs` plumbing completed: INSERT renumbered to 65 columns/placeholders/values (was 65 vs 67 — runtime SQL error on every save); SELECT lists carry documented `NULL /*retired*/` sentinels at the two retired positions so every remaining positional read stays provably aligned (was a silent column shift for all fields read at index ≥ 25); `cloud_ref`/`customized_fields` value params and field reads deleted.
4. `portable.rs`: cloud-profile export/import machinery removed end-to-end (payload field, struct, category, selection flags, counters, dependency filtering, import loop). Old export files containing `cloud_profiles` still import cleanly (serde ignores unknown fields; the cloud data is dropped).
5. `views/settings/about.rs` unclosed `impl` fixed; dangling `mod cloud_accounts;` / `mod cloud;` declarations removed.

## S2 — App-side feature removal completed (~90 files, delegated slice verified)
`View::Cloud`, `CloudMessage`, `Modal::CloudImportConfirm`, `PinnedTabSpec::{EcsExec,KubectlExec}`, SSM keepalive machinery, cloud settings variants + prefs + settings-index category, dashboard dynamic-group tiles, toolbar/menus/keynav rows, host-panel cloud transport row, editor cloud presets and `customized_fields` tracking, boot `migrate_legacy_cloud_layout`, Windows-only `update::is_per_user_install` logging hook. 222 dead i18n keys × 23 languages (5,106 lines) removed after per-key liveness verification.

## S3 — Repository amputation
11 orphaned crates (cloud ×9, sync, relay), `signaling-worker/` (Cloudflare Worker + wrangler config), `SELF_HOSTING.md`, 6 vendor-mirror/component CI workflows, 3 dead e2e scenarios. Workspace manifest + `async-trait` dep pruned; lockfile −1,181 lines / 57 packages (834 → 777). Docs truth-pass: README, SECURITY.md, docs/FEATURES.md, docs/ARCHITECTURE.md.

## S4 — MCP launcher trust boundary (the one new control)
`mcp_install::verify_cached_binary(source, version)`:
- reads the cached `manifest.json`, finds the version entry, hashes the cached binary (SHA-256), matches it against the entry's binaries by digest, then Ed25519-verifies via `plugins::verify` (production key always; dev seed key only under `cfg!(debug_assertions)`);
- `sync_launcher_from_cache` runs the gate **before** any copy to `~/.oryxis/bin/oryxis-mcp` — release builds fail closed on missing manifest, digest mismatch, or bad signature, each with an actionable error string;
- wired live: boot attempts a refresh (debug-level log on the ordinary empty cache), and `ToggleMcpServer` reports the failure into the MCP panel's status line;
- dead `post_install_refresh` + `mcp_config_installed` deleted; `cache::current_binary` trust-model doc updated to name the boundary crossings.

## S5 — Kept-by-design (disclosed, not changed)
AI assistant (default-off, user provider/key/endpoint), opt-in master-password embed in `~/.claude.json`, ProxyCommand consent gate, boot font heal, WoL. See 10-FINDINGS for the conditions each carries.

## Measurements
- Workspace crates: 25 → 14; crates.io/git packages: 834 → 777.
- First-party Rust lines removed this session (on top of the inherited ~21k): ~7,900 (5,106 i18n + ~1,600 vault/tests + ~1,200 app cloud residue incl. dead code) plus 11 crate directories (~750 KB) and the signaling worker.
- Network egress families: 10 → 5 (SSH family, opt-in AI, pinned fonts, WoL, dev-only harness).
- New runtime listeners: none. New permissions: none. New dependencies: none (sha2 already present).
