# CLAUDE.md

Internal notes for Claude (and any other agent) working on this repo.

## What this is

A Rust-native SSH client built on iced. Workspace of 7 crates:

| Crate | Role |
|-------|------|
| `oryxis-core` | Pure model types (`Connection`, `Identity`, `ProxyIdentity`, `Group`, `SshKey`, etc.) |
| `oryxis-ssh` | russh-based SSH engine — connect, jump hosts, SOCKS/HTTP/Command proxies, SFTP |
| `oryxis-vault` | Encrypted SQLite vault (Argon2id + ChaCha20Poly1305) + portable export/import |
| `oryxis-sync` | P2P sync (QUIC + mDNS + STUN + Ed25519/X25519 LWW) |
| `oryxis-terminal` | Embedded alacritty terminal + custom widget |
| `oryxis-mcp` | MCP server binary (JSON-RPC over stdio) |
| `oryxis-app` | Iced UI, dispatcher, views, AI chat |

## Build / test gates

```bash
cargo check --workspace
cargo test --workspace --lib --bins
cargo clippy --workspace --all-targets -- -D warnings   # CI gate
```

`cargo fmt --all` reformats every file (including ones unrelated to your
edit). Don't run it blindly. Format only the files you touched, or skip it
entirely and match the file's existing style.

## Architectural conventions

### Vault & encryption

- One SQLite file. Schema-versioned via `ALTER TABLE` migrations in
  `store.rs::create_tables`.
- Secrets (passwords, private keys) live in their own `BLOB` columns,
  encrypted per-field with the master key. Plaintext columns (JSON,
  text fields) **must not** carry credentials — the test
  `proxy_password_does_not_leak_into_proxy_column` enforces this for
  proxies.
- API for password fields follows a tri-state model:
  - `None` → preserve the existing column value
  - `Some("")` → clear it
  - `Some(pw)` → encrypt + store

### `Connection.proxy` resolution

A connection can express its proxy in two ways:

1. **Inline** — `Connection.proxy: Option<ProxyConfig>` (host/port/user
   in JSON; password in the encrypted `proxy_password` column).
2. **Identity reference** — `Connection.proxy_identity_id: Option<Uuid>`
   pointing at a `proxy_identities` row.

`Vault::resolve_proxy(&Connection)` returns the effective `ProxyConfig`
with password hydrated. **Identity wins over inline** when both are
set. A dangling identity (id no longer exists) resolves to `None` with
a warning — never an error, so a deleted proxy doesn't break every
host that referenced it.

The SSH engine consumes `Connection.proxy` only — callers
(`dispatch_ssh.rs`, `mcp/handlers.rs`) collapse the resolved value
into `conn.proxy` just before handing the connection off.

### Jump hosts + proxies

`engine::connect_via_jump_hosts` honors the **first** jump's proxy when
dialing the bastion. Subsequent hops travel inside the SSH tunnel, so
their proxy fields don't apply. Per-jump proxies are passed in
`ConnectionResolver.proxies: HashMap<Uuid, ProxyConfig>`, populated by
the caller via `Vault::resolve_proxy` for each id in `jump_chain`.

### SSH config import

`ssh_config.rs` parses `~/.ssh/config`. `ProxyCommand` maps directly to
`ProxyType::Command(cmd)`. `ProxyJump` is alias-resolved in a second
pass (`link_proxy_jumps`) once every imported host has been assigned
its UUID. Unresolved aliases are recorded in `Connection.notes` rather
than failing the import.

### Sync

`oryxis-sync` is opt-in P2P over QUIC. Manifest entries cover all
syncable entity types (`EntityType::Connection / SshKey / Identity /
Group / Snippet / KnownHost / ProxyIdentity`).

Wire payloads for connection / identity / proxy-identity use wrapper
structs (`SyncConnection`, `SyncIdentity`, `SyncProxyIdentity`) that
flatten the inner model and add `#[serde(default)]` `password` fields.
Forward + backward compatibility is automatic: older peers send bare
JSON which still deserializes; older peers receive new JSON and ignore
the unknown fields.

**Password sync is opt-in** via the `sync_passwords` setting (Settings
→ Sync toggle). When off, password fields are omitted from the wire
payload (`#[serde(skip_serializing_if = "Option::is_none")]`).

### i18n

All user-facing strings go through `crate::i18n::t("key")`. The English
table in `i18n::en` always returns a value (`_ => "???"` fallback);
the other 10 languages return `Option<&'static str>` and fall back to
English on `None`. New keys must be added to **all 11** language
functions.

The 11 languages today: English, Português (BR), Spanish, French,
German, Italian, Chinese, Japanese, Russian, Persian (`fa`), Arabic
(`ar`). Persian and Arabic flip `Language::is_rtl()`.

### RTL layout

`crate::i18n::is_rtl_layout()` resolves the user's `LayoutDirection`
setting (`Auto` defers to `Language::is_rtl()`; explicit
`LeftToRight` / `RightToLeft` overrides). Use this signal — never
match on language directly — when writing direction-aware code.

Two `widgets` helpers cover the common cases:

- `widgets::dir_row(items)` builds a `Row` whose children are
  reversed under RTL. Use anywhere the *physical* placement of
  widgets should mirror — sidebar/content split, leading/trailing
  icon pairs, toolbar action buttons. Don't use `iced::widget::row!`
  for these — the macro can't be reversed after construction.
- `widgets::dir_align_x()` returns `Horizontal::Right` under RTL
  and `Horizontal::Left` otherwise. Apply to `Column::align_x()` /
  `Container::align_x()` when a `Length::Fill` child should hug the
  *leading* edge instead of the physical left edge. Note that the
  parent column / container also needs `Length::Fill` width — without
  slack to align inside, the alignment has no effect.

For the keychain split-button "+ ADD ▼" pattern, the rounded outer
corners need to swap sides under RTL too — compute `Radius` from
`is_rtl_layout()` rather than hard-coding LTR corner positions.

iced doesn't auto-flip text alignment in `Length::Fill` containers,
icon glyphs, or scrollbar position. The first two are handled by
`dir_align_x()` (alignment) and `panel_right_*` icons (the sidebar
collapse glyph swaps from `panel_left_close/open`). Scrollbar
position remains physical-right and isn't fixable from the iced
0.13/0.14 API.

### Iced patterns specific to the wilsonglasser fork

- `pick_list(selected, options, mapper).on_select(callback)` — the
  fork's API is 4-step (mapper closure converts `&T` → `String` for
  display; `on_select` is a separate chained call). Don't try the
  upstream 3-arg form.
- For typed enum pickers (e.g. `ProxyKind`), implement `Display` so
  the mapper can be a simple `|k| k.to_string()`. When the rendering
  needs a runtime list lookup (e.g. resolving `Identity(Uuid)` to a
  user label), capture the list in the mapper closure.

## Settings table

Live in the SQLite `settings` table — accessed via
`vault.get_setting("key")` / `vault.set_setting("key", value)`. Values
are `String`. Booleans use `"true"` / `"false"`. The vault opens
without unlocking for settings reads, so the lock screen can hydrate
theme + language before the master password is entered.

Boot logic in `boot.rs::load_data_from_vault` reads settings into
`Oryxis` state once. Mutations go through dispatch handlers that both
update in-memory state and persist via `set_setting`.

Notable settings:

- `sync_enabled`, `sync_mode`, `sync_passwords`, `sync_device_name`,
  `sync_signaling_url`, `sync_relay_url`, `sync_listen_port`
- `mcp_server_enabled`, `mcp_server_port`
- `language`, `app_theme`, `terminal_theme`
- `ai_provider`, `ai_model`, `ai_api_key` (the API key is encrypted
  per-field inside the value via `set_user_password` machinery)

## When adding a new model entity

1. Add the type to `oryxis-core/src/models/<name>.rs` and re-export
   from `models.rs`.
2. Add a SQLite table to `store::create_tables` (`CREATE TABLE IF NOT
   EXISTS <name>s`).
3. Add CRUD methods to `oryxis-vault/src/store.rs`:
   `save_*`, `list_*`, `delete_*`, plus a password getter / setter if
   any field is encrypted.
4. If sync should cover it: add `EntityType::<Name>` to
   `oryxis-sync/src/protocol.rs`, plus arms in
   `engine::build_manifest`, `collect_records`, `apply_records`. If
   it has a password, add a `Sync<Name>` wrapper next to the existing
   ones and respect the `sync_passwords` setting.
5. If portable export should cover it: add `Export<Name>` to
   `portable.rs`, include in `ExportPayload`, populate during export,
   apply during import.
6. UI: dispatcher (`dispatch_<area>.rs`), view, messages enum, app
   state fields, boot defaults, i18n keys × all 11 languages.

## When in doubt

- Keep CRUD APIs consistent with the `identities` family — same
  signatures, same behaviors (preserve-vs-clear semantics, cascade
  NULL on delete).
- Match the file's existing style by hand. Don't rely on rustfmt for
  a clean diff.
- Test passwords don't leak: structural tests > documentation.
- See `feedback_*` files in `~/.claude/projects/-home-wilson-oryxis/memory/`
  for user preferences (no Co-Authored-By, comments in English,
  i18n discipline, split big files, integration tests outside repo
  tree, etc.).
