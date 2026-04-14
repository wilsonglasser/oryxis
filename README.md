<p align="center">
  <img src="resources/logo.svg" width="120" alt="Oryxis logo">
</p>

<h1 align="center">Oryxis</h1>

<p align="center">
  A modern SSH client built entirely in Rust — fast, encrypted, native.
</p>

<p align="center">
  <a href="https://github.com/wilsonglasser/oryxis/actions/workflows/ci.yml"><img src="https://github.com/wilsonglasser/oryxis/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/wilsonglasser/oryxis/releases/latest"><img src="https://img.shields.io/github/v/release/wilsonglasser/oryxis?color=green" alt="Release"></a>
  <img src="https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-blue" alt="Platforms">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License"></a>
  <a href="https://oryxis.app"><img src="https://img.shields.io/badge/website-oryxis.app-3CBBB1" alt="Website"></a>
  <a href="https://ko-fi.com/wilsonglasser"><img src="https://img.shields.io/badge/Ko--fi-Support%20me-ff5e5b?logo=ko-fi&logoColor=white" alt="Ko-fi"></a>
  <a href="https://buymeacoffee.com/wilsonglasser"><img src="https://img.shields.io/badge/Buy%20Me%20a%20Coffee-donate-yellow?logo=buymeacoffee&logoColor=black" alt="Buy Me a Coffee"></a>
</p>

<p align="center">
  🌐 English · Português · Español · Français · Deutsch · Italiano · 中文 · 日本語 · Русский
</p>

---

## Download

**Windows (winget):**

```powershell
winget install WilsonGlasser.Oryxis
```

Pre-built binaries are also available on the [Releases](https://github.com/wilsonglasser/oryxis/releases/latest) page:

| Platform | Architecture | Download |
|----------|-------------|----------|
| Linux | x86_64 | [`oryxis-linux-x86_64.tar.gz`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-linux-x86_64.tar.gz) |
| Linux | ARM64 | [`oryxis-linux-aarch64.tar.gz`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-linux-aarch64.tar.gz) |
| macOS | Apple Silicon | [`oryxis-macos-aarch64.tar.gz`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-macos-aarch64.tar.gz) |
| Windows | x86_64 | [`oryxis-setup-x86_64.exe`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-setup-x86_64.exe) (installer) |
| Windows | x86_64 | [`oryxis-windows-x86_64.zip`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-windows-x86_64.zip) (portable) |
| Windows | ARM64 | [`oryxis-windows-aarch64.zip`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-windows-aarch64.zip) (portable) |

---

## What is Oryxis?

Oryxis is an open-source alternative to [Termius](https://termius.com/) — a desktop SSH client with a modern UI, an encrypted vault for credentials, and a Termius-inspired design. No Electron, no webview, no cloud servers. Just a single native binary.

### Why?

Most SSH clients are either powerful but ugly (PuTTY), pretty but Electron-heavy (Termius, Tabby), or terminal-only (OpenSSH). Oryxis aims to be all three: **beautiful, fast, and native**.

## Screenshots

<p align="center">
  <img src="resources/screen_1.png" width="720" alt="Host editor with credentials and folder navigation">
</p>
<p align="center">
  <em>Host editor — folders, search, credentials with key selection</em>
</p>

<p align="center">
  <img src="resources/screen_2.png" width="720" alt="Terminal with AI Chat sidebar">
</p>
<p align="center">
  <em>Terminal session with AI Chat sidebar — ask questions, execute commands</em>
</p>

## Features

### SSH & Connectivity
- **Smart auto-authentication** — Automatically tries key, agent, password, and keyboard-interactive in order.
- **Full SSH pipeline** — Direct, SOCKS4/5, HTTP CONNECT, ProxyCommand, jump host chaining, and local port forwarding via [russh 0.60](https://github.com/warp-tech/russh).
- **RSA SHA-2 support** — Modern rsa-sha2-256/512 signing.
- **Connection progress** — Step-by-step indicator with detailed error messages.
- **TOFU host key verification** — Fingerprints saved on first connect, rejected if key changes.

### Terminal
- **Embedded emulator** — [alacritty_terminal 0.26](https://github.com/alacritty/alacritty) with 256-color, truecolor, mouse selection, scrollback.
- **Syntax highlighting** — IPs (magenta), URLs (blue), file paths (cyan) auto-detected.
- **Bold-to-bright colors** — Bold text uses vivid bright ANSI variants.
- **6 terminal themes** — Oryxis Dark, Hacker Green, Dracula, Solarized Dark, Monokai, Nord.
- **Configurable font size** — 10-24px, adjustable in Settings.
- **Session recording** — Full terminal output saved to vault, viewable in History.

### AI Chat Assistant
- **Integrated AI sidebar** — Collapsible chat panel per terminal session.
- **Bash tool execution** — AI can run commands in the active terminal and analyze output.
- **Smart output capture** — Polls terminal until output stabilizes (no fixed timeouts).
- **Multiple providers** — Anthropic (Claude), OpenAI (GPT), Google Gemini, or custom OpenAI-compatible endpoints.
- **Terminal context** — AI receives the last ~50 lines of terminal output for context.
- **Custom system prompt** — Add additional instructions in Settings.

### Identity System
- **Reusable credentials** — Create Identities (username + password + key) linked to multiple hosts.
- **Autocomplete** — Type in username field to see matching identities, click to link.
- **Keychain view** — Keys and Identities side by side with search, edit, context menus.

### Themes & Internationalization
- **4 global themes** — Oryxis Dark, Oryxis Light, Dracula, Nord. Changes entire UI instantly.
- **9 languages** — English, Portugues (Brasil), Espanol, Francais, Deutsch, Italiano, 中文, 日本語, Русский.
- **Floating overlay menus** — Context menus float over content with click-outside-to-dismiss.

### Vault & Security
- **No password by default** — Opens instantly. Enable master password in Settings.
- **Argon2id + ChaCha20Poly1305** — Industry-standard encryption.
- **Per-field encryption** — Each secret has unique 32-byte salt + 12-byte nonce.
- **Re-encryption** — All secrets re-encrypted when password changes.
- **Vault reset** — "Forgot password?" option to destroy and recreate vault.
- **No telemetry** — No data leaves your machine.

### Export / Import
- **Single encrypted file** — Export your entire vault as a `.oryxis` file protected with a password.
- **Selective export** — Choose whether to include SSH private keys or only host configurations.
- **Smart merge** — Import merges by UUID, updating only records that are newer (LWW).

### MCP Server
- **AI integration** — Expose your SSH hosts to AI assistants (Claude Code, etc.) via the [Model Context Protocol](https://modelcontextprotocol.io/).
- **5 tools** — `list_hosts`, `get_host`, `ssh_execute`, `list_groups`, `list_keys`.
- **Per-host control** — Toggle MCP exposure per connection in the host editor.
- **Disabled by default** — Enable in Settings > Security.
- **Non-interactive SSH exec** — Execute commands and get stdout/stderr/exit_code without PTY.

### P2P Sync
- **Decentralized** — Sync vault data between devices over QUIC (quinn), no cloud dependency.
- **LAN discovery** — Automatic peer discovery via mDNS on the local network.
- **Internet discovery** — Lightweight signaling server (Cloudflare Workers) for NAT traversal with STUN.
- **Pairing** — 6-digit code for initial device introduction, then Ed25519 key authentication.
- **E2E encrypted** — Sync payloads encrypted with shared secret (X25519 + ChaCha20Poly1305).
- **Auto or manual** — Configurable sync mode with adjustable interval.
- **Optional relay** — User-configurable relay URL for symmetric NAT environments.

### UI / UX
- **Native GPU-accelerated UI** — [Iced 0.14](https://iced.rs) (wgpu backend).
- **Termius-inspired design** — Card grid, slide-in editors, sidebar navigation.
- **Folder organization** — Group hosts into folders with breadcrumb navigation.
- **Search** — Filter hosts and keys by name.
- **Empty states** — Centered onboarding screens.
- **Multi-tab sessions** — SSH and local shell sessions in tabs.
- **Snippets** — Save and execute commands with one click.
- **Settings sidebar** — Terminal, AI, Theme, Shortcuts, Security, Sync, About sections.

## Architecture

```
+----- Iced Application (wgpu) ---------------------------------+
|                                                               |
|  Sidebar -- Navigation (Hosts, Keys, Snippets, etc.)          |
|  Tab Bar -- Open terminal sessions                            |
|  Content -- Grid cards / terminal canvas / AI chat sidebar    |
|  Panels  -- Slide-in editors (Host, Key, Identity)            |
|  Overlay -- Floating context menus                            |
|                                                               |
+---------------------------------------------------------------+
|  oryxis-ssh               |  oryxis-vault                     |
|  (russh 0.60 + auto-auth  |  (SQLite + Argon2id +             |
|   + jump hosts + proxy    |   ChaCha20Poly1305 +              |
|   + RSA-SHA2 + exec)      |   Export/Import .oryxis)          |
+---------------------------+-----------------------------------+
|  oryxis-sync              |  oryxis-mcp                       |
|  (quinn QUIC + mDNS +     |  (JSON-RPC 2.0 stdio,             |
|   STUN + Ed25519/X25519   |  list/get/exec SSH hosts          |
|   + LWW conflict)         |   for AI assistants)              |
+---------------------------+-----------------------------------+
|  oryxis-terminal          |  oryxis-core                      |
|  (alacritty_terminal 0.26 |  (Connection, Key, Identity,      |
|   + syntax highlight      |   Group, Snippet, KnownHost,      |
|   + 6 themes + recording) |   LogEntry)                       |
+---------------------------------------------------------------+
```

| Crate | Purpose |
|-------|---------|
| `oryxis-app` | Iced app, views, themes, i18n, AI chat, overlay system |
| `oryxis-core` | Shared types — Connection, SshKey, Identity, Group, Snippet, KnownHost, LogEntry |
| `oryxis-terminal` | Terminal widget (alacritty + canvas + PTY + syntax highlight + 6 themes) |
| `oryxis-ssh` | SSH engine — auto-auth, jump hosts, SOCKS/HTTP proxy, ProxyCommand, TOFU, RSA-SHA2 |
| `oryxis-vault` | Encrypted vault — SQLite + Argon2id + ChaCha20Poly1305 + Identity + Session logs + Export/Import |
| `oryxis-sync` | P2P sync engine — QUIC (quinn) + mDNS + STUN + signaling + Ed25519/X25519 + LWW conflict resolution |
| `oryxis-mcp` | MCP server binary — JSON-RPC 2.0 over stdio, exposes SSH hosts to AI assistants |

## Tech Stack

| Layer | Technology |
|-------|-----------|
| UI | Iced 0.14 (wgpu, GPU-accelerated) |
| Icons | Bootstrap Icons (iced_fonts) |
| Terminal | alacritty_terminal 0.26 |
| SSH | russh 0.60 (async, pure Rust, RSA-SHA2) |
| AI | reqwest + Anthropic/OpenAI/Gemini APIs |
| MCP | JSON-RPC 2.0 over stdio |
| P2P Sync | quinn (QUIC), mDNS, STUN, Ed25519/X25519 |
| Encryption | Argon2id + ChaCha20Poly1305 |
| Storage | SQLite (rusqlite) |
| Clipboard | arboard |
| File picker | rfd (native OS dialog) |
| Async | Tokio |

## Building from Source

### Prerequisites

- Rust 1.90+ (install via [rustup](https://rustup.rs/))

**Linux:**
```bash
sudo apt install -y build-essential pkg-config libssl-dev libgtk-3-dev libwayland-dev libxkbcommon-dev
```

**macOS:** Xcode Command Line Tools (`xcode-select --install`)

**Windows:** Visual Studio Build Tools with C++ workload

### Build & Run

```bash
git clone https://github.com/wilsonglasser/oryxis.git
cd oryxis
cargo run            # Debug
cargo build --release # Release
cargo test --workspace
```

## Usage

1. **First launch** — Choose to set a master password or continue without one
2. **Add hosts** — Click `+ HOST`, fill in hostname and credentials
3. **Identities** — Create reusable credential bundles in the Keychain
4. **Connect** — Click a host card to open an SSH session
5. **AI Chat** — Enable in Settings > AI, click chat bubble in terminal to ask questions
6. **Export/Import** — Settings > Security to export vault or import from another device
7. **MCP Server** — Enable in Settings > Security, configure in your AI client
8. **P2P Sync** — Settings > Sync to pair devices and sync vault data
9. **Themes** — Switch in Settings (Oryxis Dark, Light, Dracula, Nord)
10. **Language** — Change in Settings > Theme (9 languages available)

### MCP Server Setup

The MCP server (`oryxis-mcp`) exposes your SSH hosts to AI assistants like Claude Code.

1. Enable MCP in Settings > Security
2. Add to your Claude Code config (`~/.claude.json`):

```json
{
  "mcpServers": {
    "oryxis": {
      "command": "oryxis-mcp",
      "env": {
        "ORYXIS_VAULT_PASSWORD": "your-vault-password"
      }
    }
  }
}
```

If your vault has no password, omit the `env` field.

### Signaling Server (for P2P Sync over the internet)

P2P Sync uses a lightweight signaling server on Cloudflare Workers for device discovery over the internet. LAN sync works without it (via mDNS).

To deploy your own signaling server:

```bash
cd signaling-worker
npm install -g wrangler
wrangler login
wrangler kv namespace create SYNC_KV
# Copy the ID from the output into wrangler.jsonc
wrangler secret put SIGNALING_TOKEN
# Enter your token (same value as ORYXIS_SIGNALING_TOKEN in .env)
wrangler deploy
```

Then set your Worker URL in Settings > Sync > Advanced > Signaling Server.

The signaling server only stores `device_id -> IP:port` with a 5-minute TTL. It never sees encryption keys or vault data. All requests require a Bearer token to prevent unauthorized access.

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+C` | Copy from terminal |
| `Ctrl+Shift+V` | Paste to terminal |
| `Ctrl+Shift+W` | Close tab |
| `Ctrl+1...9` | Switch to tab 1-9 |
| `Ctrl+L` | Open local terminal |
| `Ctrl+N` | New host |

## Security

- **Argon2id + ChaCha20Poly1305** — Memory-hard KDF + AEAD encryption
- **Per-field encryption** — Unique 32-byte salt + 12-byte nonce per secret
- **Optional master password** — Disabled by default, enable in Settings
- **TOFU** — Server fingerprints verified on every connection
- **Pure Rust** — No C dependencies in crypto path
- **No telemetry** — No data leaves your machine
- **AI keys encrypted** — API keys stored encrypted in vault
## Roadmap

| Version | Status | Scope |
|---------|--------|-------|
| **v0.1** | **Released** | SSH, vault, keys, identities, themes, i18n, AI chat, session recording |
| **v0.2** | **In Progress** | Export/Import, MCP server, P2P sync, port forwarding |
| **v0.3** | Planned | SFTP, split panes, custom themes, biometric unlock |

## Contributing

Contributions welcome. Open an issue to discuss before submitting large PRs.

## License

[AGPL-3.0](LICENSE) — Free and open-source forever.

---

<p align="center">
  Built with Rust, for people who live in the terminal.
</p>
