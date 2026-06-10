use std::process::Command;

fn main() {
    stamp_build_metadata();

    // Embed the app icon + VERSIONINFO resource so NVIDIA / AMD overlay software
    // classifies Oryxis as a productivity app (not a game) based on the
    // FileDescription / ProductName / Comments metadata rather than defaulting
    // to its "unknown executable → assume game" heuristic.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_manifest_file("../../resources/manifest.xml");
        res.set_icon("../../resources/logo.ico");
        res.set("FileDescription", "Oryxis SSH Client");
        res.set("ProductName", "Oryxis");
        res.set("CompanyName", "Oryxis");
        res.set("LegalCopyright", "Copyright (C) Oryxis authors");
        res.set("OriginalFilename", "oryxis.exe");
        res.set("InternalName", "oryxis");
        // Comment string includes "terminal" / "SSH" / "productivity" so heuristic
        // scanners read the app as a developer tool, not a game.
        res.set(
            "Comments",
            "SSH terminal client and productivity tool, not a game. \
             GPU-accelerated via wgpu for text rendering only.",
        );
        res.compile().expect("Failed to compile Windows resources");
    }

    // Cross-compiling to Windows from a non-Windows host: the winresource
    // block above is skipped (its `#[cfg(windows)]` evaluates the host, and
    // it needs an rc.exe we don't have). Without the SxS manifest the exe
    // fails to load, rfd statically imports comctl32 v6's `TaskDialogIndirect`,
    // which only exists in the v6 assembly the manifest pulls in. Embed at
    // least the manifest straight through the MSVC-style linker (lld-link /
    // link.exe), no resource compiler required, so the cross-built exe starts.
    #[cfg(not(windows))]
    {
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        if target_os == "windows" && target_env == "msvc" {
            let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../resources/manifest.xml");
            let manifest = manifest.canonicalize().unwrap_or(manifest);
            println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
            println!("cargo:rerun-if-changed=../../resources/manifest.xml");
        }
    }
}

/// Bake the commit SHA and release channel into the binary so the
/// auto-updater can tell what it's running. The nightly channel updates
/// by comparing the running commit against the `nightly` release's
/// target commit (version numbers don't move between nightlies), and the
/// stable channel uses the embedded channel to offer a clean stable
/// build when a user on a nightly binary switches back.
fn stamp_build_metadata() {
    // Full commit SHA: prefer a CI-provided value, then `git`, else a
    // sentinel the updater treats as "can't compare, don't nag".
    let sha = std::env::var("ORYXIS_GIT_SHA").ok().filter(|s| !s.is_empty()).or_else(|| {
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    });
    let sha = sha.unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ORYXIS_GIT_SHA={sha}");

    // Channel: only the nightly workflow sets this; everything else
    // (tagged releases, local builds) is stable.
    let channel = std::env::var("ORYXIS_BUILD_CHANNEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "stable".to_string());
    println!("cargo:rustc-env=ORYXIS_CHANNEL={channel}");

    // Re-run when HEAD moves so a rebuild restamps the SHA, and when the
    // override env vars change.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-env-changed=ORYXIS_GIT_SHA");
    println!("cargo:rerun-if-env-changed=ORYXIS_BUILD_CHANNEL");
}
