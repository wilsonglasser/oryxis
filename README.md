<p align="center">
  <img src="resources/logo.svg" width="120" alt="Oryxis logo">
</p>

<h1 align="center">Oryxis</h1>

<p align="center">
  A modern SSH client built entirely in Rust — fast, encrypted, native.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/platform-linux-blue" alt="Platform">
  <img src="https://img.shields.io/badge/version-0.1.0-green" alt="Version">
</p>

---

## What is Oryxis?

Oryxis is an open-source alternative to [Termius](https://termius.com/) — a desktop SSH client with a modern UI, an encrypted vault for credentials, and a Termius-inspired design. No Electron, no webview, no cloud servers. Just a single native binary.

### Why?

Most SSH clients are either powerful but ugly (PuTTY), pretty but Electron-heavy (Termius, Tabby), or terminal-only (OpenSSH). Oryxis aims to be all three: **beautiful, fast, and native**.

## Features

- **Native GPU-accelerated UI** — Built with [Iced](https://iced.rs) (wgpu backend). Termius-inspired dark theme with grid card layouts.
- **Embedded terminal emulator** — Powered by [alacritty_terminal](https://github.com/alacritty/alacritty) with 256-color, truecolor, copy/paste, mouse selection, and scrollback.
- **Encrypted vault** — Master password with Argon2id key derivation. Passwords and private keys encrypted with ChaCha20Poly1305. SQLite storage.
- **Full SSH pipeline** — Direct connections, SOCKS4/5 proxy, HTTP CONNECT proxy, ProxyCommand, and jump host chaining via [russh](https://github.com/warp-tech/russh).
- **Key management** — Import SSH keys from file (native file picker) or paste PEM. Keys stored encrypted in the vault.
- **Snippets** — Save and execute commands with one click on the active terminal session.
- **TOFU host key verification** — Server fingerprints saved on first connect, alerts on key changes.
- **Known hosts & history** — Full connection log with timestamps, host key registry.
- **Multi-tab sessions** — Multiple SSH/local shell sessions in tabs, switchable from the top bar.
- **Single binary** — `cargo build --release` and you're done.

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
| File picker | rfd |
| Async | Tokio |

## Building

### Prerequisites

- Rust 1.90+
- Linux with X11 or Wayland
- System dependencies:

```bash
sudo apt install -y build-essential pkg-config libssl-dev libgtk-3-dev libwayland-dev libxkbcommon-dev
```

### Build & Run

```bash
git clone https://github.com/wilsonglasser/oryxis.git
cd oryxis

# Debug
cargo run

# Release
cargo build --release
./target/release/oryxis

# Hot reload (requires cargo-watch)
cargo install cargo-watch
cargo watch -x run
```

## Usage

1. **First launch** — Set a master password for the vault
2. **Add hosts** — Click `+ HOST`, fill in hostname/credentials, save
3. **Connect** — Click a host card to open an SSH session in a new tab
4. **Keys** — Import SSH keys from file via the Keychain view
5. **Snippets** — Save commands and execute them on the active session
6. **Jump hosts** — Select another host as "Jump Host" in the host editor

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+C` | Copy selected text |
| `Ctrl+Shift+V` | Paste from clipboard |
| Mouse drag | Select text in terminal |
| Mouse wheel | Scroll through terminal history |

## Roadmap

| Version | Status | Scope |
|---------|--------|-------|
| **v0.1** | Done | SSH connections, vault, key management, snippets, TOFU, multi-tab |
| **v0.2** | Planned | Port forwarding UI, SFTP file transfer, split panes |
| **v0.3** | Planned | P2P folder sharing (iroh), CRDT sync, team roles |
| **v0.4** | Planned | Session recording, custom themes, biometric unlock |

## Contributing

Contributions, ideas, and feedback are welcome. Open an issue to discuss before submitting large PRs.

## License

MIT

---

<p align="center">
  Built with Rust, for people who live in the terminal.
</p>
