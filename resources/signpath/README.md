# SignPath Authenticode signing

The Windows release builds (`.github/workflows/release.yml`, jobs
`build-windows` and `build-windows-arm`) Authenticode-sign three PE files per
architecture through the [SignPath Foundation](https://signpath.org/) free OSS
program:

1. `oryxis.exe` (the inner binary, signed *before* it is zipped or wrapped by
   the installers, so the signature propagates into the portable zip and into
   the installed binary).
2. `oryxis-setup-<arch>.exe` (system NSIS installer, the one winget targets).
3. `oryxis-user-setup-<arch>.exe` (per-user NSIS installer).

The macOS Developer ID / notarization path and the detached Ed25519 `.sig`
files (consumed by the in-app auto-updater) are unrelated and unchanged. The
Ed25519 signatures are computed in the `release` job *after* download, i.e.
over the SignPath-signed bytes, so auto-update keeps verifying the artifact a
user actually runs.

## One-time setup

### 1. SignPath console (https://app.signpath.io)

- Project slug: **`oryxis`** (already created).
- Signing policies: **`test-signing`** (self-signed test cert, available now)
  and **`release-signing`** (production cert, imported by SignPath after they
  review the setup).
- Artifact configuration: create one with slug **`exe`** and paste the XML from
  [`oryxis-exe.artifact-config.xml`](./oryxis-exe.artifact-config.xml). The
  root must be `<zip-file>` because GitHub wraps uploads in a zip.
- Make sure the `release-signing` policy **auto-approves** requests from the
  verified GitHub Actions origin, otherwise every tagged release blocks on a
  manual approval until `wait-for-completion-timeout-in-seconds` (600s) expires
  and fails the job.

### 2. GitHub repo settings

Secret (Settings -> Secrets and variables -> Actions -> Secrets):

| Secret | Value |
|--------|-------|
| `SIGNPATH_API_TOKEN` | CI user API token from SignPath (Organization -> Users -> your CI user). |

Variables (same page -> Variables). All optional; defaults shown:

| Variable | Default | Notes |
|----------|---------|-------|
| `SIGNPATH_ORGANIZATION_ID` | *(none)* | **Required.** GUID from the SignPath org URL. |
| `SIGNPATH_PROJECT_SLUG` | `oryxis` | |
| `SIGNPATH_SIGNING_POLICY_SLUG` | `release-signing` | Set to `test-signing` while validating. |
| `SIGNPATH_ARTIFACT_CONFIG_SLUG` | `exe` | Must match the artifact config slug above. |

If `SIGNPATH_API_TOKEN` is unset (forks, manual `workflow_dispatch` without
access), every signing step is skipped via `if: env.SIGNPATH_API_TOKEN != ''`
and the build ships unsigned. When the token *is* set, a SignPath failure fails
the build rather than silently shipping unsigned artifacts.

## Validating before the first real release

1. Set `SIGNPATH_ORGANIZATION_ID` + `SIGNPATH_API_TOKEN`, and
   `SIGNPATH_SIGNING_POLICY_SLUG=test-signing`.
2. Trigger `release.yml` via **workflow_dispatch** (the publishing jobs are
   gated on a tag, so a manual run only produces artifacts) and download the
   `oryxis-windows-*` artifacts.
3. Confirm signatures: `signtool verify /pa /v oryxis-setup-x86_64.exe`, or on
   the binary's Properties -> Digital Signatures tab. The test cert chains to an
   untrusted root (expected); what you are verifying is that all three exes are
   signed and the `output-artifact-directory` -> copy-back wiring landed the
   signed bytes.
4. Once SignPath imports the production certificate, flip
   `SIGNPATH_SIGNING_POLICY_SLUG` to `release-signing` and tag a release.

> The production cert is **OV** (Organization Validation): it removes the
> "Unknown Publisher" prompt but SmartScreen reputation still accrues with
> download volume. SignPath Foundation does not issue EV certs.
