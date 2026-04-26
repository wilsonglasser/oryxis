fn main() {
    // SENTRY_DSN is read at compile time via option_env! in main.rs; tell cargo
    // to rebuild when it changes so release builds pick up the secret.
    println!("cargo:rerun-if-env-changed=SENTRY_DSN");

    // Embed the app icon + VERSIONINFO resource so NVIDIA / AMD overlay software
    // classifies Oryxis as a productivity app (not a game) based on the
    // FileDescription / ProductName / Comments metadata rather than defaulting
    // to its "unknown executable → assume game" heuristic.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
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
            "SSH terminal client and productivity tool — not a game. \
             GPU-accelerated via wgpu for text rendering only.",
        );
        res.compile().expect("Failed to compile Windows resources");
    }
}
