# Packaging

Package manager manifests for distributing Oryxis. These are versioned here;
publishing to each registry is a separate manual step (see below).

When cutting a new release, bump the version and `sha256`/`hash` fields in
each manifest to match the new release artifacts.

## AUR (Arch Linux) — `aur/`

`oryxis-bin` installs the prebuilt `oryxis-linux-<arch>.tar.gz` release
artifact. The AUR is its own git server (one repo per package), so this is
published with a push to `aur.archlinux.org`, not through GitHub.

One-time setup:

1. Create an account at https://aur.archlinux.org/register
2. Add your public SSH key under My Account -> SSH Public Key

Publish (run from a checkout of the AUR repo, not this repo):

```bash
git clone ssh://aur@aur.archlinux.org/oryxis-bin.git
cp /path/to/oryxis/packaging/aur/{PKGBUILD,.SRCINFO} oryxis-bin/
cd oryxis-bin
git add PKGBUILD .SRCINFO
git commit -m "Update to 0.8.0"
git push
```

`.SRCINFO` is kept in sync with `PKGBUILD` by hand here. On an Arch machine
it is normally regenerated with `makepkg --printsrcinfo > .SRCINFO`.

## Scoop (Windows) — `scoop/`

`oryxis.json` installs the `oryxis-windows-<arch>.zip` release artifact.
Two ways to ship it:

- **Personal bucket (no review):** add an `oryxis.json` to any git repo with a
  `bucket/` folder. Users install with:

  ```
  scoop bucket add oryxis https://github.com/wilsonglasser/oryxis
  scoop install oryxis
  ```

  (Requires the manifest to live under a `bucket/` directory in the bucket
  repo. This repo keeps it under `packaging/scoop/` for versioning; copy it
  into a `bucket/` folder of whichever repo serves as the bucket.)

- **Official `extras` bucket (discoverable):** open a PR to
  `ScoopInstaller/Extras` adding `bucket/oryxis.json`. Users then install with
  `scoop install extras/oryxis` without adding a custom bucket. This is the
  better path for discovery.

## Flathub (Linux) — `flatpak/`

App ID `app.oryxis.Oryxis` (3-segment rDNS under the oryxis.app domain). The
folder is self-contained: manifest, the generated cargo sources, and the
desktop + metainfo files (the v0.8.0 tag predates the latter two, so they ship
beside the manifest for now).

Regenerate `cargo-sources.json` whenever `Cargo.lock` changes:

```bash
pip install aiohttp tomlkit
python flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
```

(`flatpak-cargo-generator.py` lives in flatpak/flatpak-builder-tools.)

Publish:

1. Fork `flathub/flathub`, branch `app.oryxis.Oryxis`.
2. Copy the four files from `flatpak/` to the repo root.
3. Open a PR against the `master` branch. The Flathub buildbot compiles the
   app in the sandbox; iterate until it goes green, then a reviewer merges and
   `flathub/app.oryxis.Oryxis` is created.
4. After publishing, claim the "Verified" badge from the oryxis.app domain
   (DNS TXT or `.well-known`).

Local test (needs flatpak + flatpak-builder):

```bash
flatpak install flathub org.freedesktop.Sdk//24.08 org.freedesktop.Platform//24.08 \
  org.freedesktop.Sdk.Extension.rust-stable//24.08
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/app.oryxis.Oryxis.yml
```
