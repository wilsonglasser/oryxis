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
