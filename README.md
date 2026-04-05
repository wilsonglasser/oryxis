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
  <img src="https://img.shields.io/github/license/wilsonglasser/oryxis" alt="License">
</p>

---

## Download

Pre-built binaries are available on the [Releases](https://github.com/wilsonglasser/oryxis/releases/latest) page:

| Platform | Architecture | Download |
|----------|-------------|----------|
| Linux | x86_64 | [`oryxis-linux-x86_64.tar.gz`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-linux-x86_64.tar.gz) |
| Linux | ARM64 | [`oryxis-linux-aarch64.tar.gz`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-linux-aarch64.tar.gz) |
| macOS | Intel | [`oryxis-macos-x86_64.tar.gz`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-macos-x86_64.tar.gz) |
| macOS | Apple Silicon | [`oryxis-macos-aarch64.tar.gz`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-macos-aarch64.tar.gz) |
| Windows | x86_64 | [`oryxis-windows-x86_64.zip`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-windows-x86_64.zip) |
| Windows | ARM64 | [`oryxis-windows-aarch64.zip`](https://github.com/wilsonglasser/oryxis/releases/latest/download/oryxis-windows-aarch64.zip) |

---

## What is Oryxis?

Oryxis is an open-source alternative to [Termius](https://termius.com/) — a desktop SSH client with a modern UI, an encrypted vault for credentials, and a Termius-inspired design. No Electron, no webview, no cloud servers. Just a single native binary.

### Why?

Most SSH clients are either powerful but ugly (PuTTY), pretty but Electron-heavy (Termius, Tabby), or terminal-only (OpenSSH). Oryxis aims to be all three: **beautiful, fast, and native**.

## Features

- **Native GPU-accelerated UI** — Built with [Iced](https://iced.rs) (wgpu backend). Termius-inspired dark theme with grid card layouts.
- **Embedded terminal emulator** — Powered by [alacritty_terminal](https://github.com/alacritty/alacritty) with 256-color, truecolor, copy/paste, mouse selection, and 10k line scrollback.
- **Encrypted vault** — Master password with Argon2id key derivation. Passwords and private keys encrypted with ChaCha20Poly1305. SQLite storage.
- **Full SSH pipeline** — Direct connections, SOCKS4/5 proxy, HTTP CONNECT proxy, ProxyCommand, and jump host chaining via [russh](https://github.com/warp-tech/russh).
- **Key management** — Import SSH keys from file (native OS file picker) or paste PEM. Keys stored encrypted in the vault.
- **Snippets** — Save and execute commands with one click on the active terminal session.
- **TOFU host key verification** — Server fingerprints saved on first connect, connection rejected if key changes.
- **Known hosts & history** — Full connection activity log with timestamps. Host key registry with delete/re-trust.
- **Multi-tab sessions** — Multiple SSH and local shell sessions in tabs with a top tab bar.
- **Cross-platform** — Linux, macOS, and Windows. Single native binary per platform.

## Screenshots

```
┌──────────┬─────────────────────────────────────────┐
│ >_ Hosts │ [Hosts]  [prod-web]  [staging-db]       │
│ K  Keys  ├─────────────────────────────────────────┤
│ {} Snips │                                         │
│ ☐ Known  │  ┌──────────┐ ┌──────────┐ ┌────────┐  │
│ ⏱ History│  │ >_ prod  │ │ >_ stag  │ │ >_ dev │  │
│ ⚙ Config │  │ root@... │ │ deploy@..│ │ user@..│  │
│          │  └──────────┘ └──────────┘ └────────┘  │
│          │                                         │
│ + Local  │  ▸ Production                           │
│          │  ┌──────────┐ ┌──────────┐              │
└──────────┴─────────────────────────────────────────┘
```

## Architecture

```
┌─ Iced Application (wgpu) ────────────────────────────┐
│                                                      │
│  Sidebar ─ Navigation (Hosts, Keys, Snippets, etc.)  │
│  Tab Bar ─ Open terminal sessions                    │
│  Content ─ Grid cards or terminal canvas             │
│  Panels  ─ Slide-in editors (New Host, Add Key)      │
│                                                      │
├──────────────────────────────────────────────────────┤
│  oryxis-ssh            │  oryxis-vault               │
│  (russh + jump hosts   │  (SQLite + Argon2id +       │
│   + SOCKS + HTTP proxy │   ChaCha20Poly1305)         │
│   + ProxyCommand)      │                             │
├──────────────────────────────────────────────────────┤
│  oryxis-terminal       │  oryxis-core                │
│  (alacritty_terminal   │  (Connection, Key, Group,   │
│   + canvas + PTY)      │   Snippet, KnownHost, Log)  │
└──────────────────────────────────────────────────────┘
```

| Crate | Purpose |
|-------|---------|
| `oryxis-app` | Iced application, views, state, Termius-style UI |
| `oryxis-core` | Shared types — Connection, SshKey, Group, Snippet, KnownHost, LogEntry |
| `oryxis-terminal` | Terminal widget (alacritty_terminal + Iced canvas + PTY + selection + scrollback) |
| `oryxis-ssh` | SSH engine — direct, jump hosts, SOCKS/HTTP proxy, ProxyCommand, TOFU |
| `oryxis-vault` | Encrypted vault — SQLite + Argon2id + ChaCha20Poly1305 |
| `oryxis-sync` | P2P sync engine (iroh wrapper) — planned |

## Tech Stack

| Layer | Technology |
|-------|-----------|
| UI | Iced 0.14 (wgpu, GPU-accelerated) |
| Icons | Bootstrap Icons (iced_fonts) |
| Terminal | alacritty_terminal 0.25 |
| SSH | russh 0.48 (async, pure Rust) |
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

# Release (optimized, ~28MB binary)
cargo build --release
./target/release/oryxis

# Hot reload during development
cargo install cargo-watch
cargo watch -x run

# Run tests (44 tests)
cargo test --workspace
```

### Install on Linux

```bash
./install.sh
```

Installs binary to `/usr/local/bin`, icon and `.desktop` file for the application menu.

## Usage

1. **First launch** — Set a master password to encrypt your vault
2. **Add hosts** — Click `+ HOST`, fill in hostname, credentials, and optional jump host
3. **Connect** — Click a host card to open an SSH session in a new tab
4. **Keys** — Import SSH keys from file via the Keychain view (native file picker)
5. **Snippets** — Save frequently used commands, click to execute on the active session
6. **Jump hosts** — Create the bastion host first, then select it as "Jump Host" on the target

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+C` | Copy selected text |
| `Ctrl+Shift+V` | Paste from clipboard |
| Mouse drag | Select text in terminal |
| Mouse wheel | Scroll through terminal history (10k lines) |

### SSH Connection Types

| Type | How to configure |
|------|-----------------|
| Direct | Just hostname + port |
| Jump host | Select another saved host as "Jump Host" |
| SOCKS5 proxy | Set proxy type to SOCKS5 in connection settings |
| SOCKS4 proxy | Set proxy type to SOCKS4 |
| HTTP CONNECT | Set proxy type to HTTP |
| ProxyCommand | Set proxy type to Command with your command |

### Authentication Methods

| Method | Description |
|--------|------------|
| Password | Stored encrypted in vault |
| Key | Select an imported SSH key |
| Agent | Uses running ssh-agent |
| Interactive | Keyboard-interactive (2FA/TOTP) |

## Security

- **Vault encryption** — Argon2id KDF (memory-hard) + ChaCha20Poly1305 AEAD
- **Zero plaintext storage** — Passwords and private keys never stored unencrypted
- **TOFU** — Server fingerprints verified on every connection, changed keys rejected
- **Memory safety** — Pure Rust, no C dependencies in crypto path
- **No telemetry** — No data leaves your machine

## Roadmap

| Version | Status | Scope |
|---------|--------|-------|
| **v0.1** | **Released** | SSH connections, vault, key management, snippets, TOFU, multi-tab, 6-platform release |
| **v0.2** | Planned | Port forwarding UI, SFTP file transfer, split panes |
| **v0.3** | Planned | P2P folder sharing (iroh), CRDT sync, team roles |
| **v0.4** | Planned | Session recording, custom themes, biometric unlock |

## Contributing

Contributions, ideas, and feedback are welcome. Open an issue to discuss before submitting large PRs.

```bash
# Development setup
cargo install cargo-watch
cargo watch -x "test --workspace"  # Run tests on save
```

## License

[AGPL-3.0](LICENSE) — Free and open-source forever. Anyone can use, modify, and distribute. Modified versions made available over a network must share source code.

---

<p align="center">
  Built with Rust, for people who live in the terminal.
</p>
