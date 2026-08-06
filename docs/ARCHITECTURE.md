# Architecture

Oryxis is a Cargo workspace of 24 crates. The UI layer is an
[iced](https://iced.rs) application on the wgpu backend; everything below it
is a set of focused engines (SSH, Telnet, serial, vault, sync, terminal)
that the app composes.

```
+--------------------------------------------------------------------+
| Iced Application (wgpu, GPU-accelerated)      oryxis-app           |
| Sidebar / Tab bar / Card grid / Terminal / SFTP / AI               |
| Slide-in editors . Split panes . Modals & overlays                 |
+------------------------------+-------------------------------------+
| Connection engines           | Encrypted vault                     |
| oryxis-ssh    russh, auto-   | oryxis-vault                        |
|   auth, jump hosts, proxies, | SQLite, Argon2id,                   |
|   -L/-R/-D, SFTP, TOFU       | ChaCha20-Poly1305 per-field,        |
| oryxis-telnet RFC 854/1143   | session logs / recordings,          |
| oryxis-serial COM / tty      | .oryxis export / import             |
| oryxis-zmodem sz/rz engine   |                                     |
+--------------------------------------------------------------------+
| Cloud providers + plugin subsystem                                 |
| oryxis-cloud              provider trait (discovery + transport)   |
| oryxis-cloud-aws/-gcp/-azure/-k8s        provider implementations  |
| *-plugin                  subprocess binaries (JSON-RPC 2.0)       |
| oryxis-plugin-protocol    stdio wire contract                      |
| oryxis-plugin-signer      Ed25519 sign + SHA-256                   |
+------------------------------+-------------------------------------+
| P2P sync                     | AI / automation                     |
| oryxis-sync                  | oryxis-mcp                          |
| quinn QUIC, mDNS, STUN,      | JSON-RPC 2.0 over stdio,            |
| signaling + relay,           | list / get / exec SSH hosts         |
| Ed25519/X25519, LWW          | for AI assistants                   |
| oryxis-relay (self-host)     |                                     |
+------------------------------+-------------------------------------+
| Terminal                     | Core model types                    |
| oryxis-terminal              | oryxis-core                         |
| alacritty_terminal,          | Connection, Key, Identity,          |
| custom widget + PTY,         | ProxyIdentity, Group, Snippet,      |
| themes + custom themes       | KnownHost, PortForwardRule,         |
|                              | SessionGroup, CloudAccount, ...     |
+--------------------------------------------------------------------+
```

## Crates

| Crate | Purpose |
|-------|---------|
| `oryxis-app` | Iced app: views, themes, i18n, AI chat, SFTP browser, cloud UI, split panes, overlays, keyboard navigation |
| `oryxis-core` | Shared model types: Connection, SshKey, Identity, ProxyIdentity, Group, Snippet, KnownHost, PortForwardRule, SessionGroup, CloudAccount, custom themes, LogEntry |
| `oryxis-terminal` | Terminal widget: alacritty_terminal + custom canvas widget + PTY + themes + URL/IP/path detection |
| `oryxis-ssh` | SSH engine: auto-auth, jump hosts, SOCKS/HTTP/Command proxy, Local/Remote/Dynamic forwarding, SFTP, TOFU, RSA-SHA2 |
| `oryxis-telnet` | Native Telnet engine: RFC 854/855 option negotiation (RFC 1143 state machine), NAWS, terminal-type, charset transcoding |
| `oryxis-serial` | Serial console sessions: COM / `/dev/tty*`, configurable baud, framing, flow control, line endings, local echo |
| `oryxis-zmodem` | ZMODEM transfer engine: auto-detects `sz` / `rz` on the byte stream over SSH, Telnet and serial |
| `oryxis-archive` | Archive operations for the SFTP file browser: browse and read/write zip and tar over local and remote (ranged) reads, transport-agnostic |
| `oryxis-vault` | Encrypted vault: SQLite + Argon2id + ChaCha20-Poly1305 per-field + session logs / recordings + `.oryxis` export/import |
| `oryxis-biometric` | Biometric / OS-keyring app unlock: Windows Hello, macOS Touch ID (Keychain user presence), Linux Secret Service |
| `oryxis-sync` | P2P sync engine: QUIC (quinn) + mDNS + STUN + signaling + HTTP relay fallback + Ed25519/X25519 + LWW conflict resolution |
| `oryxis-relay` | Self-hostable signaling + relay HTTP server (axum + in-memory queues) |
| `oryxis-mcp` | MCP server binary: JSON-RPC 2.0 over stdio, exposes SSH hosts to AI assistants. Distributed as a plugin, not bundled in the OS installers |
| `oryxis-cloud` | Cloud provider abstraction: a `CloudProvider` trait split into discovery (list resources) and transport (open a channel: SSH / SSM / ECS Exec / kubectl exec) |
| `oryxis-cloud-aws` | AWS provider: named profiles, static keys, IAM Identity Center (SSO), EC2 + ECS discovery |
| `oryxis-cloud-gcp` | Google Cloud provider: Compute Engine + GKE discovery driven through the `gcloud` CLI |
| `oryxis-cloud-azure` | Azure provider: VMs + AKS discovery driven through the `az` CLI |
| `oryxis-cloud-k8s` | Kubernetes provider: kubeconfig auth, workload discovery and pod shells driven through the `kubectl` CLI |
| `oryxis-cloud-aws-plugin` | AWS provider packaged as a standalone subprocess (JSON-RPC 2.0 over stdio) |
| `oryxis-cloud-gcp-plugin` | Google Cloud provider packaged as a standalone subprocess |
| `oryxis-cloud-azure-plugin` | Azure provider packaged as a standalone subprocess |
| `oryxis-cloud-k8s-plugin` | Kubernetes provider packaged as a standalone subprocess |
| `oryxis-plugin-protocol` | Wire protocol for cloud-provider plugins: line-delimited JSON-RPC 2.0 over stdio |
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
| Cloud | AWS (EC2/ECS, SSM, EC2 Instance Connect), GCP (`gcloud`), Azure (`az`), Kubernetes (`kubectl`), as Ed25519-signed subprocess plugins |
| AI | reqwest + Anthropic / OpenAI-compatible / Gemini APIs |
| MCP | JSON-RPC 2.0 over stdio |
| P2P Sync | quinn (QUIC), mDNS, STUN, HTTP relay fallback, Ed25519/X25519, XChaCha20-Poly1305 |
| Encryption at rest | Argon2id + ChaCha20-Poly1305 |
| Storage | SQLite (rusqlite) |
| Async | Tokio |

## Design notes

- **No Electron, no webview.** The whole UI is a single native binary;
  optional capabilities (cloud providers, MCP server) are downloaded on
  demand as signed subprocess plugins so the core stays small.
- **Secrets are per-field encrypted.** Credentials never live in plaintext
  columns; structural tests enforce it.
- **The vault is the source of truth.** Sync, export and the MCP server all
  read through the same store; nothing bypasses it.
- **Plugins are sandboxed by process.** Cloud providers speak line-delimited
  JSON-RPC 2.0 over stdio and are Ed25519-signature-verified before every
  execution.
