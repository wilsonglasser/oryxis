# Security Policy

Oryxis stores SSH credentials, so security reports get priority over
everything else.

## Reporting a vulnerability

Please do **not** open a public issue for security problems.

- **Preferred:** report privately via
  [GitHub Security Advisories](https://github.com/wilsonglasser/oryxis/security/advisories/new)
  ("Report a vulnerability" on the Security tab).
- **Alternative:** email `wilsonglasser@gmail.com` with `[SECURITY]` in the
  subject.

You can expect an acknowledgment within 72 hours. Fixes for confirmed
vulnerabilities are released as fast as severity demands, and reporters are
credited in the release notes unless they prefer otherwise.

## Supported versions

Only the latest release receives security fixes. Oryxis ships small and
often, so upgrading is always the remediation path; the auto-updater and
`winget upgrade` keep installs current.

## Security model

What Oryxis does to protect your credentials:

- **Encryption at rest.** The vault is a local SQLite file; every secret
  (passwords, private keys, TOTP secrets, API keys) is encrypted in its own
  column with ChaCha20-Poly1305, per-field salt and nonce, under a master
  key derived with Argon2id. Structural tests enforce that credentials
  never land in plaintext columns.
- **Locking.** Optional master password, biometric unlock (Windows Hello /
  Touch ID / Linux keyring), and an idle auto-lock that zeroizes the master
  key in memory.
- **Host verification.** TOFU host key pinning, verified on every
  connection.
- **Sync.** P2P payloads are end-to-end encrypted with X25519 +
  XChaCha20-Poly1305; relay servers only ever see ciphertext. Password sync
  is opt-in and off by default.
- **Plugins.** Subprocess plugins are Ed25519-signature-verified against a
  baked-in key before every execution, plus a SHA-256 manifest check at
  download time.
- **AI.** Bring-your-own-key, stored encrypted; Privacy Mode redaction runs
  before terminal context reaches any model; the auto-exec path has a
  deterministic block-list floor plus an independent judge.
- **No telemetry.** The app makes no phone-home calls. Network traffic is
  limited to what you ask for: your SSH/Telnet sessions, update checks
  against GitHub Releases, plugin downloads, your AI provider, and your
  sync peers.
- **Pure-Rust crypto path.** No C dependencies in the cryptography stack.

## Out of scope

- Vulnerabilities in servers you connect to.
- The Telnet and serial protocols being cleartext (inherent to the
  protocols; the UI says so where it matters).
- Attacks requiring an already-compromised local machine (a keylogger or
  root malware can defeat any local vault).
