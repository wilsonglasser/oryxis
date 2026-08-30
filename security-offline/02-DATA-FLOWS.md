# 02 — Data flows

## Inbound (user-initiated, core)

- SSH/SFTP/mosh/telnet/serial connect → user-configured host:port. Credentials from vault (decrypted at use), agent, or interactive prompt. Server output parsed by terminal engine (untrusted-rendering boundary).
- Wake-on-LAN: UDP broadcast 255.255.255.255:9, magic packet with host MAC (`wol.rs:67`). Link-local only.
- ssh-agent protocol on local socket/pipe: list/read/signed keys; per-signature confirmation mode available; keys decrypted only at sign time.

## Outbound (complete list, post-transformation)

| # | Flow | Trigger | Destination | Integrity/confidentiality |
|---|---|---|---|---|
| 1 | SSH family (shell, SFTP, forwards, jumps, ProxyCommand) | explicit connect | user-configured hosts | russh TLS-class SSH crypto; host-key known_hosts policy |
| 2 | AI chat/judge | explicit user invocation with configured provider+key | user-chosen provider (default URLs per provider catalog) | provider TLS; API key from vault; terminal context egress — disclosed |
| 3 | Font fetch | CJK language selected / pack face picked / boot heal of the configured font | `raw.githubusercontent.com` at commit-pinned URLs only | https-only, SHA-256+length pinned, size-capped, atomic install |
| 4 | WoL broadcast | user click | 255.255.255.255:9 | no payload beyond MAC |
| 5 | Harness daemon (dev only) | `--harness` feature | 127.0.0.1:6799 | sandboxed $HOME, no real secrets |

Removed flows: release/update metadata (api.github.com via net_mirror), plugin catalog + binary downloads, vendor mirror (dl.oryxis.app / dl-cn.oryxis.app), sync engine (QUIC P2P, signaling worker, relay), git/WebDAV/SFTP-folder sync transports, cloud provider APIs (AWS/Azure/GCP/K8s discovery), profile auto-refresh.

## Local-only data paths

- Vault CRUD → SQLite with per-field AEAD; master password in memory while unlocked (necessity: agent signing, biometric re-unlock), zeroized secrets on drop.
- Portable export → password-encrypted file; security-sensitive settings deny-listed (`is_portable_setting`); AI key re-encrypted per target vault.
- Session recordings/logs → inside vault, sealed with content key.
- MCP install → writes `~/.claude.json` (or WSL variant) with token and, only after explicit confirmation, the vault master password (finding F-04).
- Clipboard: terminal copy/paste, MCP JSON copy. Notifications: local sound/tray only.
