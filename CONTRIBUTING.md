# Contributing to Oryxis

Thanks for considering a contribution. Oryxis is a solo-maintained project
that ships small and often; the guidelines below keep reviews fast.

## Before you start

- **Bugs:** open an issue with the
  [bug template](https://github.com/wilsonglasser/oryxis/issues/new/choose).
  Settings > Advanced > "Copy environment info" gives you the environment
  block; the opt-in debug log in the same section often captures the
  failure.
- **Features / large changes:** open an issue (or a
  [Discussion](https://github.com/wilsonglasser/oryxis/discussions)) first.
  The [roadmap](README.md#roadmap) is deliberate; agreeing on scope before
  code avoids wasted work on both sides.
- **Language:** issues and discussions written in Chinese are welcome
  (中文 issue 也欢迎); the maintainer reads them (translated when needed)
  and replies in English or Chinese as able. Code, commit messages and
  code comments remain English.
- **Security issues:** never a public issue, see [SECURITY.md](SECURITY.md).

## Development setup

Prerequisites are the same as
[building from source](README.md#building-from-source): Rust stable via
rustup, plus the platform packages listed there.

```bash
git clone https://github.com/wilsonglasser/oryxis.git
cd oryxis
cargo run
```

`docs/ARCHITECTURE.md` maps the 24-crate workspace; most UI work lands in
`crates/oryxis-app`, engine work in `oryxis-ssh` / `oryxis-vault` /
`oryxis-sync` / `oryxis-terminal`.

## Quality gates

CI enforces all three; run them before pushing:

```bash
cargo check --workspace
cargo test --workspace --lib --bins
cargo clippy --workspace --all-targets -- -D warnings
```

## Project conventions

A few rules that are easy to miss and will come up in review:

- **Formatting.** Do not run `cargo fmt --all`; it reformats unrelated
  files. Match the existing style of the file you're editing (or format
  only the files you touched).
- **i18n is not optional.** Every user-facing string goes through
  `crate::i18n::t("key")`, and every new key must be added to **all 23**
  language modules in `crates/oryxis-app/src/i18n/`. English is the
  fallback table; the other languages return `Option` and fall back to it.
- **RTL awareness.** Horizontal arrangements that should mirror under RTL
  use `widgets::dir_row(...)` (not the `row!` macro) and
  `widgets::dir_align_x()`; never branch on the language directly, use
  `i18n::is_rtl_layout()`.
- **Keyboard navigation.** Every new interactive surface (view, modal,
  menu, row list) must be wired into the keynav framework
  (`crates/oryxis-app/src/keynav/`) in the same change. A mouse-only
  surface is an incomplete feature here.
- **Button feedback.** Every clickable control shows hover and press
  feedback (branch the style closure on `button::Status`); icon-only
  controls also get a tooltip.
- **Secrets stay encrypted.** Credentials live in dedicated encrypted
  columns, never in plaintext JSON/text columns. If you add a secret
  field, add a structural leak test like
  `proxy_password_does_not_leak_into_proxy_column`.
- **No stubs.** No `// TODO` gaps, placeholder UI, or half-wired features;
  if a trade-off exists, surface it in the PR description instead of
  silently taking the shortcut.
- **Comments in English**, regardless of the language of the discussion.
- **Tests with real credentials live outside the repo.** Integration tests
  that need live servers or cloud accounts must not land in the tree.

## Contributing a theme

Themes are the one contribution that needs no Rust and no local setup.
Export yours from Oryxis, add three attribution lines, and drop the file
into `themes/terminal/` or `themes/ui/` through GitHub's web editor; the
index the gallery reads is regenerated automatically on merge.

Full instructions, the field list and what gets accepted are in
[`themes/README.md`](themes/README.md). Everything below is for code.

## Pull requests

- Keep PRs focused; unrelated refactors belong in their own PR.
- Describe what changed and why; link the issue.
- Screenshots or a short capture for UI changes help a lot.
- New features need i18n keys, keynav wiring and (where it applies) tests
  in the same PR; those aren't follow-ups.

## License

By contributing you agree that your contributions are licensed under
[AGPL-3.0-or-later](LICENSE), the project license.
