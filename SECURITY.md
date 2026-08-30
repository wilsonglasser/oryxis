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

Only the latest release receives security fixes. This offline edition
has no auto-updater; re-build from source or replace the binary
manually to pick up fixes.

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
- **Plugins.** Local-only plugin cache; a plugin binary must pass its
  Ed25519 signature check (against the baked-in key) and the manifest's
  SHA-256 before it is copied to the stable launcher path external
  clients spawn. Release builds fail closed on unsigned binaries.
- **AI.** Bring-your-own-key, stored encrypted; Privacy Mode redaction runs
  before terminal context reaches any model; the auto-exec path has a
  deterministic block-list floor plus an independent judge.
- **No telemetry.** The app makes no phone-home calls, no update checks,
  and no catalog fetches. Network traffic is limited to what you ask
  for: your SSH/Telnet/mosh sessions, your AI provider (only if you
  configure one), and commit-pinned font downloads for languages or
  font packs you select.
- **Pure-Rust crypto path.** No C dependencies in the cryptography stack.

## Code signing

Windows binaries and installers are Authenticode-signed in CI by
[SignPath.io](https://about.signpath.io) with a certificate from the
[SignPath Foundation](https://signpath.org). The private key never leaves
SignPath's hardware security module.

## Out of scope

- Vulnerabilities in servers you connect to.
- The Telnet and serial protocols being cleartext (inherent to the
  protocols; the UI says so where it matters).
- Attacks requiring an already-compromised local machine (a keylogger or
  root malware can defeat any local vault).
