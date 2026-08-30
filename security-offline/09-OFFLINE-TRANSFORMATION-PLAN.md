# 09 — Offline transformation plan (and what was inherited)

The working tree arrived mid-transformation (base 4b8ef3b8, ~21k lines already deleted, build broken in 3 places). This audit completed it rather than restarting it. Slices:

## A. Repair the inherited half-cuts (compile integrity)
- `fonts.rs:802` called deleted `net_mirror::candidates` → single direct GET of the pinned canonical URL (mirror layer stays dead; SHA-256 pin intact).
- `oryxis-vault`: 38 errors — tombstone calls in every store module, `mod sync;`, `SyncPeerRow`/`Tombstone`/`derive_sync_secret`, sync table creation, sync settings accessors, `cloud_ref`/`customized_fields`/`cloud_query` fields, cloud-profile export/import machinery, and a live SQL bug (INSERT had 65 columns vs 67 placeholders/values; SELECT lists had dropped 2 columns while read indexes still expected them → silent column shift). All removed/fixed; SELECT keeps documented NULL sentinels at the retired positions so remaining indexes stay provably aligned; 65/65 INSERT arity verified; round-trip tests green.
- `views/settings/about.rs`: unclosed `impl` (update-section deletion took the brace); `views/mod.rs` + `grid/mod.rs` dangling module decls.

## B. Dead settings and i18n (delegated slice, in progress at time of writing)
- Remove `SettingCloudAutoRefresh*` messages, toggle arms, prefs fields, settings-index `S::Cloud` entries, `search_cloud_accounts`.
- Prune dead i18n keys (`sync_*`, `update_*`, `download_mirror*`, `cloud_*`, `settings_cloud_*`) from all 24 language files after per-key reference verification.

## C. Repository amputation (DONE)
- Deleted: `crates/oryxis-cloud{-aws,-aws-plugin,-azure,-azure-plugin,-gcp,-gcp-plugin,-k8s,-k8s-plugin}`, `crates/oryxis-sync`, `crates/oryxis-relay`, `signaling-worker/`, `SELF_HOSTING.md`, `.github/workflows/{release-aws,release-azure,release-gcp,release-k8s,release-relay,publish-mirror}.yml`, e2e `cloud-open-plugins.ice` / `download-mirror.ice` / `sync-passphrase-reopen.ice`.
- Workspace manifest pruned (members + workspace.dependencies); `async-trait` dep dropped (no remaining users).
- Docs truth-pass: README (sync row, feature bullets, screenshots, roadmap, security summary), docs/FEATURES.md (cloud/sync sections, plugin subsystem), docs/ARCHITECTURE.md (crate map), SECURITY.md (model claims).

## D. Vault sync API removal (DONE)
- Covered in A: schema stops creating `sync_peers`/`sync_metadata`; `destroy_and_recreate` still drops them for legacy vaults; `convert_all_fields` no longer walks the dead table (was a password-change breaker on fresh vaults) but still rotates legacy encrypted settings keys (`sync_sftp_passphrase`, `sync_webdav_password`, `sync_device_identity`) found in old vaults; portable deny-list deliberately retains sync/mirror/update keys so foreign exports cannot re-introduce them.

## E. e2e leftovers (DONE) — removed with C.

## F. MCP launcher signature gate (DONE — see 11-SECURITY-CHANGES)
- `sync_launcher_from_cache` now reads the cached `manifest.json`, matches the binary by SHA-256, verifies the Ed25519 signature via `plugins::verify` (prod key; dev key honored in debug builds), and fails closed with a clear local error before any copy.

## Validation gates (Phase 10)
1. `cargo check --workspace` zero errors; `cargo test --workspace` green.
2. Reference greps: `net_mirror|CloudMessage|View::Cloud|oryxis-sync|oryxis-relay|oryxis-cloud|dl.oryxis.app|api.github.com|signaling` → zero hits in production code.
3. Lockfile diff: cloud/sync/relay dep trees pruned.
4. SBOM regenerated; advisory scan noted (see 12-VALIDATION).

Rollback: the entire transformation is uncommitted working-tree state; `git checkout -- .` (or per-path) restores upstream. No user data touched; vault format remains readable (additive removals only; legacy tables dropped only on explicit destroy).
