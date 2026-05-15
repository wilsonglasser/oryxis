use std::path::Path;

// Signaling for cross-network sync is optional. When both
// `ORYXIS_SIGNALING_URL` and `ORYXIS_SIGNALING_TOKEN` are set (env or
// the workspace `.env`), `SyncConfig::default()` picks them up via
// `option_env!` as the baseline so a release build can ship with the
// hosted Worker pre-configured. When they're missing the engine still
// builds and runs in LAN-only mode; the user can fill the URL in
// Settings > Sync > Advanced at runtime.
fn main() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dotenv_path = workspace_root.join(".env");

    if dotenv_path.exists() {
        if let Ok(contents) = std::fs::read_to_string(&dotenv_path) {
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
                    // CI env vars take precedence over `.env`.
                    if std::env::var(key).is_err() {
                        println!("cargo:rustc-env={}={}", key, value);
                    }
                }
            }
        }
    }

    for key in &["ORYXIS_SIGNALING_URL", "ORYXIS_SIGNALING_TOKEN"] {
        if let Ok(val) = std::env::var(key) {
            println!("cargo:rustc-env={}={}", key, val);
        }
    }

    println!("cargo:rerun-if-changed=../../.env");
    println!("cargo:rerun-if-env-changed=ORYXIS_SIGNALING_URL");
    println!("cargo:rerun-if-env-changed=ORYXIS_SIGNALING_TOKEN");
}
