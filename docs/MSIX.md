# MSIX / Microsoft Store

The Store build is the same `oryxis.exe` the GitHub release ships, wrapped
in an MSIX package instead of an NSIS installer. Nothing about the NSIS
installers changes: `winget` keeps targeting the system setup, and the
per-user setup stays the auto-updater's target.

## Why MSIX and not the EXE submission path

The Store accepts either an MSIX package or a plain `.msi` / `.exe`
installer (policy 10.2.9). The installer path requires the binary and
every PE inside it to be signed by a CA in the Microsoft Trusted Root
Program. MSIX requires no code-signing certificate at all: Partner Center
re-signs the package with a Microsoft certificate after it passes
certification. So the Store channel needs no code-signing certificate
of its own.

Consequence for this repo: the package the workflow hands to Partner
Center is **unsigned on purpose**.

## One-time setup in Partner Center

1. Create the developer account. Registration is free for both individual
   and company account types. Pick company if the publisher name reads as
   a business entity (policy 10.14).
2. Reserve the app name (`Oryxis`).
3. Open **Product management > View app identity details** and copy three
   values into repository **secrets**, so the identity never lands in the
   source tree:

   | Partner Center field                     | Secret                        | Workflow input (override) |
   |------------------------------------------|-------------------------------|---------------------------|
   | Package/Identity/Name                    | `MSIX_IDENTITY_NAME`          | `identity_name`           |
   | Package/Identity/Publisher (`CN=<GUID>`) | `MSIX_PUBLISHER`              | `publisher`               |
   | Publisher display name                   | `MSIX_PUBLISHER_DISPLAY_NAME` | `publisher_display_name`  |

   ```
   gh secret set MSIX_IDENTITY_NAME --repo <owner>/oryxis
   gh secret set MSIX_PUBLISHER --repo <owner>/oryxis
   gh secret set MSIX_PUBLISHER_DISPLAY_NAME --repo <owner>/oryxis
   ```

   With the secrets in place the workflow runs with every input left
   empty. The inputs exist to pack under a different identity (a fork,
   or a dry run) without touching the secrets. They reach the script as
   environment variables rather than template expansions, so the values
   stay masked in the run log.

4. Fill in the submission's privacy policy URL. Policy 10.5.1 makes it
   mandatory for Win32 / Desktop Bridge products regardless of how little
   data the app touches.

## Building the package

```
Actions > "MSIX (Microsoft Store)" > Run workflow
```

Inputs are all optional when the repo variables exist; `version` defaults
to the `oryxis-app` crate version. The run produces two artifacts:

- **`oryxis-msix-store`** contains `oryxis-<version>.msixbundle` (x64 +
  arm64). This is what gets uploaded to Partner Center.
- **`oryxis-msix-sideload`** contains a copy signed with a throwaway
  self-signed certificate plus `oryxis-test-cert.cer`. Windows refuses to
  install an unsigned package, so this pair exists purely so the build
  can be tested on a real machine. Never submit it.

## Package version

The Store rejects a package whose first version field is 0, and the app
is on 0.x, so the two numbering schemes are decoupled. The package
counter lives in `resources/msix/package-version.txt`, starts at `1.0.0`
and is bumped once per submission; the workflow appends the
Store-reserved fourth field (`1.0.0` becomes `1.0.0.0`).

Bump that file in the commit that submits, so `git log` on it is the
history of what actually reached the Store, and name the app build
(`0.11.0`, ...) in the Partner Center release notes, since the Store page
shows the package number, not the app's. The run log and the job summary
print both.

The `version` input overrides the file for a one-off run.

## Publishing an update from CI

Run the workflow with **publish** checked and it submits the bundle it
just built, instead of only leaving artifacts. It uses the Microsoft
Store Developer CLI through Microsoft's own action
(`microsoft/microsoft-store-apppublisher`), which is the supported path.
Note the platform limitation: **Store updates via GitHub Actions work for
free products only**, which Oryxis is.

One-time setup, all in Partner Center and Entra ID:

1. Associate a Microsoft Entra tenant with the Partner Center account.
2. Register an application in Entra ID.
3. In Partner Center, Account settings > User management > Microsoft Entra
   applications, add that application and give it the **Manager** role.
4. Add these repository secrets:

   | Secret | Where it comes from |
   |--------|---------------------|
   | `AZURE_AD_TENANT_ID` | Entra admin center, Identity > Overview |
   | `AZURE_AD_APPLICATION_CLIENT_ID` | the app registration's Application (client) ID |
   | `AZURE_AD_APPLICATION_SECRET` | that registration's client secret (shown once) |
   | `SELLER_ID` | Partner Center, Account settings > Identifiers |
   | `MSIX_STORE_PRODUCT_ID` | the Store ID of the published app |

A publish run fails, rather than skipping, when a secret is missing: the
run was explicitly asked to submit. It also fails if the Store rejects the
version, which is what happens when `package-version.txt` was not bumped,
since the Store never accepts the same version twice.

The first submission of an app cannot go through this path; it has to be
done by hand in Partner Center (listing, age rating, capability
justification). From the second one on, the workflow covers package
updates. Store listing text is separate, see the import/export CSV flow.

## Sideload QA (do this before submitting)

On a Windows machine, unzip the sideload artifact and run
`install-sideload.ps1` from an **elevated** PowerShell. It trusts the
throwaway certificate and installs the package, and it prints the two
lines that undo both afterwards.

By hand, it is:

```powershell
Import-Certificate -FilePath .\oryxis-test-cert.cer `
  -CertStoreLocation Cert:\LocalMachine\TrustedPeople

Get-AuthenticodeSignature .\oryxis-<pkg>-app<app>-test-signed.msixbundle |
  Format-List Status, StatusMessage     # must say Valid BEFORE installing

Add-AppxPackage .\oryxis-<pkg>-app<app>-test-signed.msixbundle
```

The certificate store is where this goes wrong. It has to be the LOCAL
MACHINE store, and either **Trusted People** or **Trusted Root
Certification Authorities** (a self-signed certificate is its own root).
Importing through the double-click wizard usually lands it in the current
user's store, or in `Intermediate Certification Authorities`, and neither
grants any trust: the install then fails with `0x800B010A` ("the root
certificate could not be verified"), which says nothing about the store
being wrong.

Then verify, in this order, the things the package changes:

1. The app launches and the vault lands in `%USERPROFILE%\.oryxis`.
2. **Taskbar button groups correctly** and the JumpList shows recent
   hosts. This proves the AUMID gate works: a packaged process must keep
   the package identity, so `jumplist::tag_window` and the explicit
   `SetAppID` are skipped (`crate::packaged::is_packaged`).
3. **Settings > About has no update panel.** Inside a package the Store
   services the app; `WindowsApps` is read-only, so running the NSIS
   installer would only lay down a second, unpackaged copy. The panel and
   its keynav rows are gone, and the boot check is refused in
   `dispatch_update.rs`.
4. `oryxis` resolves from a fresh shell (the execution alias replaces the
   PATH entry the NSIS installers write; MSIX cannot edit PATH).
5. The ssh-agent named pipe is reachable and a plugin download succeeds
   (`~/.oryxis/plugins`), both of which write outside the read-only
   package root.

Finish with the **Windows App Certification Kit** against the installed
package. It catches manifest and asset problems before certification
does.

Uninstall with `Remove-AppxPackage` (or Settings > Apps) and remove the
test certificate from Trusted People when done.

## Restricted capability justification

Partner Center asks why the package declares `runFullTrust` before it will
accept the submission. It is the standard capability for a packaged Win32
app, but the answer has to name concrete uses or the reviewer comes back
asking. What was submitted:

> Oryxis is a native Win32 desktop application written in Rust and
> packaged with the Desktop Bridge. runFullTrust is the standard
> capability for any packaged full-trust Win32 application and is
> required for it to run as an MSIX package at all.
>
> The product is an SSH, SFTP, Telnet and serial client, and it depends
> on full-trust APIs that are unavailable inside the UWP sandbox:
>
> - Outbound TCP connections to arbitrary hosts and ports chosen by the
>   user (SSH, SFTP, Telnet), plus loopback listening sockets for the
>   port forwarding feature, which other local applications connect
>   through.
> - Serial port (COM) access for console connections to network
>   appliances.
> - Windows named pipes: the app implements the standard ssh-agent
>   protocol and serves it on `\\.\pipe\oryxis-ssh-agent`, secured with a
>   per-user DACL, so that tools like git, VS Code and WSL can
>   authenticate using keys held in the app's encrypted vault.
> - Reading and writing files outside the package: the encrypted vault in
>   `%USERPROFILE%\.oryxis`, and the user's existing AWS and Kubernetes
>   credential files when they enable those optional integrations.
> - Launching helper processes: the operating system's remote desktop
>   client when the user opens an RDP or VNC session over an SSH tunnel,
>   and the optional first-party plugin executables the user chooses to
>   install.
>
> The application does not install drivers or NT services, does not use
> undocumented or unsupported APIs, and collects no user data.

The closing sentence is deliberate: full trust makes a reviewer check
policy 10.2.4 (dependency on non-Microsoft drivers or NT services), so
the answer rules it out up front.

## Notes for certification (submission form)

Two things are worth pre-empting in the certification notes, because a
reviewer meeting them cold costs a rejection cycle:

- **First run.** The app opens on vault creation (master password). No
  account, no server, no credentials needed to exercise the UI.
- **Plugins.** Cloud providers, the MCP server and the GIF exporter are
  optional first-party components downloaded on demand into
  `~/.oryxis/plugins`, each pinned by SHA-256 and verified against an
  Ed25519 signature, and always user-initiated. This is the
  add-on/extension case policy 10.1.5 permits; mention the plugin-based
  cloud providers in the listing description as well, so the described
  functionality covers them (policy 10.2.2).

## Package contents

The layout is deliberately minimal: `oryxis.exe`, `Assets\` and the
generated `AppxManifest.xml`. The app reads nothing from beside its
executable in release builds (the plugin dev lookup is
`#[cfg(debug_assertions)]`), and fonts are either embedded or downloaded
into `~/.oryxis`.

The layout also gets a generated `resources.pri`. The manifest refers to
`Assets\Square44x44Logo.png`, a name no file carries: the resource index
is what resolves it to the right `.scale-*` / `.targetsize-*` variant per
DPI. Packing without it fails, and "fixing" that by dropping plain
base-name copies in would make Windows ignore every scaled and unplated
asset. The workflow strips the `<packaging>` section from the generated
priconfig so all variants stay in this package rather than being split
into resource packages that are never produced.

Assets in `resources/msix/Assets` are rendered from `resources/logo.svg`:
`Square44x44Logo`, `Square150x150Logo` and `StoreLogo` at scales
100/125/150/200/400, plus `Square44x44Logo` targetsize 16/24/32/48/256 in
both plated and `altform-unplated` variants. Regenerate them with
`rsvg-convert -w N -h N` if the logo changes; wrong pixel dimensions pass
packing and fail the certification kit.
