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
</p>

---

## Download

Pre-built binaries are available on the [Releases](https://github.com/wilsonglasser/oryxis/releases/latest) page:

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

## Features

### SSH & Connectivity
- **Smart auto-authentication** — Automatically tries key, agent, password, and keyboard-interactive in order. No need to pick an auth method.
- **Full SSH pipeline** — Direct connections, SOCKS4/5 proxy, HTTP CONNECT proxy, ProxyCommand, and jump host chaining via [russh 0.60](https://github.com/warp-tech/russh).
- **RSA SHA-2 support** — Modern rsa-sha2-256/512 signing. Works with servers that reject legacy ssh-rsa.
- **Connection progress** — Step-by-step indicator (transport, auth, session) with detailed error messages.
- **TOFU host key verification** — Server fingerprints saved on first connect, rejected if key changes.

### Terminal
- **Embedded emulator** — Powered by [alacritty_terminal 0.26](https://github.com/alacritty/alacritty) with 256-color, truecolor, mouse selection, and scrollback.
- **Syntax highlighting** — IPs (magenta), URLs (blue), and file paths (cyan) detected and colored automatically.
- **Bold-to-bright colors** — Bold text uses vivid bright ANSI variants, like Termius.
- **6 terminal color themes** — Oryxis Dark, Hacker Green, Dracula, Solarized Dark, Monokai, Nord.
- **Configurable font size** — Adjust terminal text size (10-24px) from Settings.

### Identity System
- **Reusable credentials** — Create Identities (username + password + key) and link them to multiple hosts.
- **Identity picker** — Select an identity in the host editor to auto-fill credentials.
- **Keychain view** — Keys and Identities side by side, with search, edit, and context menus.

### Themes
- **4 global themes** — Oryxis Dark, Oryxis Light, Dracula, Nord. Changes the entire UI instantly.
- **Teal accent** (#229991) — Inspired by the Oryxis logo, used throughout the interface.
- **Theme cards** — Visual preview in Settings with color bars.

### Vault & Security
- **No password by default** — Opens instantly. Enable a master password in Settings if desired.
- **Argon2id + ChaCha20Poly1305** — Industry-standard key derivation and encryption.
- **Per-field encryption** — Each password and private key encrypted with unique salt + nonce.
- **Re-encryption on password change** — All secrets re-encrypted when enabling/disabling master password.
- **No telemetry** — No data leaves your machine.

### UI / UX
- **Native GPU-accelerated UI** — Built with [Iced 0.14](https://iced.rs) (wgpu backend).
- **Termius-inspired design** — Card grid, slide-in editors, sidebar navigation.
- **Folder organization** — Group hosts into folders with breadcrumb navigation.
- **Search** — Filter hosts by name/hostname and keys by label.
- **Empty states** — Centered onboarding screens for Hosts, Keys, and Snippets.
- **Multi-tab sessions** — SSH and local shell sessions in tabs.
- **Snippets** — Save and execute commands with one click.
- **Settings with sidebar** — Terminal, Theme, Shortcuts, Security, and About sections.

## Architecture

```
+----- Iced Application (wgpu) --------------------------------+
|                                                               |
|  Sidebar -- Navigation (Hosts, Keys, Snippets, etc.)          |
|  Tab Bar -- Open terminal sessions                            |
|  Content -- Grid cards or terminal canvas                     |
|  Panels  -- Slide-in editors (New Host, Add Key, Identity)    |
|                                                               |
+---------------------------------------------------------------+
|  oryxis-ssh              |  oryxis-vault                      |
|  (russh 0.60 + jump      |  (SQLite + Argon2id +              |
|   hosts + SOCKS + HTTP    |   ChaCha20Poly1305 +               |
|   proxy + ProxyCommand    |   Identity CRUD)                   |
|   + auto-auth)            |                                    |
+---------------------------------------------------------------+
|  oryxis-terminal          |  oryxis-core                       |
|  (alacritty_terminal 0.26 |  (Connection, Key, Group,          |
|   + canvas + PTY +        |   Snippet, Identity, KnownHost,    |
|   syntax highlight)       |   LogEntry)                        |
+---------------------------------------------------------------+
```

| Crate | Purpose |
|-------|---------|
| `oryxis-app` | Iced application, views, state, themes, settings |
| `oryxis-core` | Shared types — Connection, SshKey, Identity, Group, Snippet, KnownHost, LogEntry |
| `oryxis-terminal` | Terminal widget (alacritty_terminal + canvas + PTY + selection + syntax highlight + themes) |
| `oryxis-ssh` | SSH engine — auto-auth, jump hosts, SOCKS/HTTP proxy, ProxyCommand, TOFU, RSA-SHA2 |
| `oryxis-vault` | Encrypted vault — SQLite + Argon2id + ChaCha20Poly1305 + Identity CRUD |
| `oryxis-sync` | P2P sync engine — planned |

## Tech Stack

| Layer | Technology |
|-------|-----------|
| UI | Iced 0.14 (wgpu, GPU-accelerated) |
| Icons | Bootstrap Icons (iced_fonts) |
| Terminal | alacritty_terminal 0.26 |
| SSH | russh 0.60 (async, pure Rust, RSA-SHA2) |
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

# Debug
cargo run

# Release (optimized)
cargo build --release
./target/release/oryxis

# Run tests
cargo test --workspace
```

## Usage

1. **First launch** — App opens directly (no password needed by default)
2. **Add hosts** — Click `+ HOST`, fill in hostname and credentials
3. **Connect** — Click a host card to open an SSH session
4. **Identities** — Create reusable credential bundles in the Keychain
5. **Keys** — Import SSH keys from file via the Keychain view
6. **Snippets** — Save frequently used commands for quick execution
7. **Themes** — Switch global theme in Settings (Oryxis Dark, Light, Dracula, Nord)
8. **Security** — Enable vault master password in Settings > Security

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+C` | Copy from terminal |
| `Ctrl+Shift+V` | Paste to terminal |
| `Ctrl+Shift+W` | Close tab |
| `Ctrl+1...9` | Switch to tab 1-9 |
| `Ctrl+L` | Open local terminal |
| `Ctrl+N` | New host |
| Mouse drag | Select text in terminal |
| Mouse wheel | Scroll terminal history |

### Authentication Methods

| Method | Description |
|--------|------------|
| Auto (default) | Tries key, agent, password, keyboard-interactive in order |
| Password | Stored encrypted in vault |
| Key | Select an imported SSH key |
| Agent | Uses running ssh-agent (Unix socket or Windows named pipe) |
| Interactive | Keyboard-interactive (2FA/TOTP) |

## Security

- **Vault encryption** — Argon2id KDF (memory-hard) + ChaCha20Poly1305 AEAD
- **Zero plaintext storage** — Passwords and private keys always encrypted at rest
- **Per-field encryption** — Each secret has unique 32-byte salt + 12-byte nonce
- **Optional master password** — Disabled by default for convenience, enable in Settings
- **Re-encryption** — All secrets re-encrypted when master password changes
- **TOFU** — Server fingerprints verified on every connection
- **Memory safety** — Pure Rust, no C dependencies in crypto path
- **No telemetry** — No data leaves your machine

## Roadmap

| Version | Status | Scope |
|---------|--------|-------|
| **v0.1** | **Released** | SSH, vault, keys, identities, snippets, themes, settings, 5-platform release |
| **v0.2** | Planned | Port forwarding UI, SFTP file transfer, split panes |
| **v0.3** | Planned | P2P sync (iroh), session recording, custom themes |

## Contributing

Contributions, ideas, and feedback are welcome. Open an issue to discuss before submitting large PRs.

## License

[AGPL-3.0](LICENSE) — Free and open-source forever.

---

<p align="center">
  Built with Rust, for people who live in the terminal.
</p>
