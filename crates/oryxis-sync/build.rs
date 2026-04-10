use std::path::Path;

fn main() {
    // Load .env from workspace root if it exists (for local builds)
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
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
                    // Only set if not already in environment (CI env vars take precedence)
                    if std::env::var(key).is_err() {
                        println!("cargo:rustc-env={}={}", key, value);
                    }
                }
            }
        }
    }

    // Pass through env vars from CI/system environment
    for key in &["ORYXIS_SIGNALING_URL", "ORYXIS_SIGNALING_TOKEN"] {
        if let Ok(val) = std::env::var(key) {
            println!("cargo:rustc-env={}={}", key, val);
        }
    }

    // Fail if neither .env nor env vars provide the required values
    let has_url = std::env::var("ORYXIS_SIGNALING_URL").is_ok() || dotenv_has_key(&dotenv_path, "ORYXIS_SIGNALING_URL");
    let has_token = std::env::var("ORYXIS_SIGNALING_TOKEN").is_ok() || dotenv_has_key(&dotenv_path, "ORYXIS_SIGNALING_TOKEN");

    if !has_url {
        panic!("ORYXIS_SIGNALING_URL not set. Define it in .env or as an environment variable.");
    }
    if !has_token {
        panic!("ORYXIS_SIGNALING_TOKEN not set. Define it in .env or as an environment variable.");
    }

    println!("cargo:rerun-if-changed=../../.env");
    println!("cargo:rerun-if-env-changed=ORYXIS_SIGNALING_URL");
    println!("cargo:rerun-if-env-changed=ORYXIS_SIGNALING_TOKEN");
}

fn dotenv_has_key(path: &Path, key: &str) -> bool {
    if let Ok(contents) = std::fs::read_to_string(path) {
        contents.lines().any(|line| {
            let line = line.trim();
            !line.starts_with('#') && line.split_once('=').map(|(k, _)| k.trim() == key).unwrap_or(false)
        })
    } else {
        false
    }
}
