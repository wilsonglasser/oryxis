# Architecture

Oryxis is a Cargo workspace of 14 crates. The UI layer is an
[iced](https://iced.rs) application on the wgpu backend; everything below it
is a set of focused engines (SSH, Telnet, serial, vault, terminal)
that the app composes. This is the offline edition: cloud providers,
device sync, the auto-updater, and the plugin download catalog are
removed.

```
+--------------------------------------------------------------------+
| Iced Application (wgpu, GPU-accelerated)      oryxis-app           |
| Sidebar / Tab bar / Card grid / Terminal / SFTP / AI               |
| Slide-in editors . Split panes . Modals & overlays                 |
+------------------------------+-------------------------------------+
| Connection engines           | Encrypted vault                     |
| oryxis-ssh     russh, auto-  | oryxis-vault                        |
|   auth, jump hosts, proxies, | SQLite, Argon2id,                   |
|   -L/-R/-D, SFTP, TOFU       | ChaCha20-Poly1305 per-field,        |
| oryxis-telnet  RFC 854/1143, | session logs / recordings,          |
|   TLS, raw TCP               | .oryxis export / import             |
| oryxis-serial  COM / tty     |                                     |
| oryxis-mosh    mosh handover |                                     |
|   + UDP session over SSH     |                                     |
| oryxis-zmodem  sz/rz engine  |                                     |
| oryxis-archive tar/zip over  |                                     |
|   SFTP + local codecs        |                                     |
+--------------------------------------------------------------------+
| Plugin subsystem (local-only)             | AI / automation        |
| oryxis-plugin-protocol  stdio wire        | oryxis-mcp             |
| oryxis-plugin-signer    Ed25519 sign      | JSON-RPC 2.0 over      |
|                         + SHA-256         | stdio, list / get /    |
| cache under ~/.oryxis, verified,          | exec SSH hosts         |
| never fetched                             | for AI assistants      |
+-------------------------------------------+-----------------------+
| Terminal                     | Core model types                    |
| oryxis-terminal              | oryxis-core                         |
| alacritty_terminal,          | Connection, Key, Identity,          |
| custom widget + PTY,         | ProxyIdentity, Group, Snippet,      |
| themes + custom themes       | KnownHost, PortForwardRule,         |
|                              | SessionGroup, ...                   |
+--------------------------------------------------------------------+
```

## Crates

| Crate | Purpose |
|-------|---------|
| `oryxis-app` | Iced app: views, themes, i18n, AI chat, SFTP browser, split panes, overlays, keyboard navigation |
| `oryxis-core` | Shared model types: Connection, SshKey, Identity, ProxyIdentity, Group, Snippet, KnownHost, PortForwardRule, SessionGroup, custom themes, LogEntry |
| `oryxis-terminal` | Terminal widget: alacritty_terminal + custom canvas widget + PTY + themes + URL/IP/path detection |
| `oryxis-ssh` | SSH engine: auto-auth, jump hosts, SOCKS/HTTP/Command proxy, Local/Remote/Dynamic forwarding, SFTP, TOFU, RSA-SHA2 |
| `oryxis-mosh` | mosh carried over an SSH host: the `mosh-server` handover (remote command synthesis with shell quoting, announcement parsing) and the UDP session that follows, driving the `mosh-rs` crate and publishing escape bytes so the pane reads it like any other transport |
| `oryxis-telnet` | Native Telnet engine: RFC 854/855 option negotiation (RFC 1143 state machine), NAWS, terminal-type, charset transcoding, TLS (`telnets`), plus the raw-TCP mode console servers expose serial lines on |
| `oryxis-serial` | Serial console sessions: COM / `/dev/tty*`, configurable baud, framing, flow control, line endings, local echo |
| `oryxis-zmodem` | ZMODEM transfer engine: auto-detects `sz` / `rz` on the byte stream over SSH, Telnet and serial |
| `oryxis-archive` | SFTP archive operations: remote `tar` / `unzip` / `zip` command synthesis with safe quoting (POSIX + Windows), local zip / tar.gz codecs, zip central-directory browsing over ranged reads |
| `oryxis-vault` | Encrypted vault: SQLite + Argon2id + ChaCha20-Poly1305 per-field + session logs / recordings + `.oryxis` export/import |
| `oryxis-biometric` | Biometric / OS-keyring app unlock: Windows Hello, macOS Touch ID (Keychain user presence), Linux Secret Service |
| `oryxis-mcp` | MCP server binary: JSON-RPC 2.0 over stdio, exposes SSH hosts to AI assistants. Distributed as a plugin, not bundled in the OS installers |
| `oryxis-plugin-protocol` | Plugin wire contract: line-delimited JSON-RPC 2.0 over stdio, plus the shared dev signing seed |
| `oryxis-plugin-signer` | CLI that signs a plugin binary with the Ed25519 key and computes the SHA-256 the manifest needs |

## Tech stack

| Layer | Technology |
|-------|-----------|
| UI | Iced (wilsonglasser fork, branch `oryxis`, wgpu GPU-accelerated, software-renderer fallback) |
| Icons | Lucide + Codicon (iced_fonts) + brand SVG icons |
| Fonts | Noto Sans (UI, CJK / Hebrew / Thai / Devanagari coverage) + SauceCodePro / Symbols Nerd Font (terminal) |
| Terminal | alacritty_terminal |
| SSH | russh (async, pure Rust, RSA-SHA2) |
| Telnet / Serial / ZMODEM | native Rust engines (`oryxis-telnet`, `oryxis-serial`, `oryxis-zmodem`) |
| AI | reqwest + Anthropic / OpenAI-compatible / Gemini APIs |
| MCP | JSON-RPC 2.0 over stdio |
| Encryption at rest | Argon2id + ChaCha20-Poly1305 |
| Storage | SQLite (rusqlite) |
| Async | Tokio |

## Design notes

- **No Electron, no webview.** The whole UI is a single native binary;
  the MCP server ships as a signed local plugin binary, nothing is
  downloaded on demand.
- **Secrets are per-field encrypted.** Credentials never live in plaintext
  columns; structural tests enforce it.
- **The vault is the source of truth.** Export and the MCP server both
  read through the same store; nothing bypasses it.
- **Plugins are verified, then trusted on disk.** A plugin binary must
  pass its Ed25519 signature check before it is copied to the stable
  launcher path; the cache lives under `~/.oryxis` (user-private).
