//! SFTP view helpers: formatting. Split out of views/sftp/mod.rs.

use super::*;
/// POSIX rwx string for a permission/mode value, e.g. "drwxr-xr-x".
/// The leading type char comes from the row's dir/symlink flags; only
/// the low 12 bits of `mode` carry the rwx + setuid/setgid/sticky bits.
pub(crate) fn format_perms(mode: Option<u32>, is_dir: bool, is_symlink: bool) -> String {
    let Some(m) = mode else {
        return "-".to_string();
    };
    let type_char = if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };
    let rwx = |bits: u32| {
        format!(
            "{}{}{}",
            if bits & 0o4 != 0 { 'r' } else { '-' },
            if bits & 0o2 != 0 { 'w' } else { '-' },
            if bits & 0o1 != 0 { 'x' } else { '-' },
        )
    };
    format!(
        "{}{}{}{}",
        type_char,
        rwx((m >> 6) & 0o7),
        rwx((m >> 3) & 0o7),
        rwx(m & 0o7),
    )
}

/// "uid:gid" owner string, with a dash when neither side is known
/// (Windows local entries, or a server that omits owner attributes).
pub(crate) fn format_owner(uid: Option<u32>, gid: Option<u32>) -> String {
    match (uid, gid) {
        (Some(u), Some(g)) => format!("{u}:{g}"),
        (Some(u), None) => u.to_string(),
        (None, Some(g)) => format!(":{g}"),
        (None, None) => "-".to_string(),
    }
}

/// Value for the Type column: folders / symlinks keep their friendly label;
/// files show the MIME type guessed from the extension (`application/
/// octet-stream` when unknown or extensionless).
pub(crate) fn format_kind(name: &str, is_dir: bool, is_symlink: bool) -> String {
    if is_symlink {
        return t("sftp_type_symlink").to_string();
    }
    if is_dir {
        return t("sftp_type_folder").to_string();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() && !stem.is_empty() => {
            mime_for_ext(&ext.to_ascii_lowercase()).to_string()
        }
        _ => "application/octet-stream".to_string(),
    }
}

/// MIME type for a (lowercased) file extension. The comprehensive
/// [`crate::mime_types`] table (embedded from mime-db) covers the long tail;
/// `dev_mime_override` wins first for source-code / dev extensions that
/// mime-db gets wrong (e.g. `.rs`, `.ts`) or doesn't list (`.go`, `.vue`).
/// Anything unknown falls back to `application/octet-stream`.
pub(crate) fn mime_for_ext(ext: &str) -> &'static str {
    dev_mime_override(ext)
        .or_else(|| crate::mime_types::lookup(ext))
        .unwrap_or("application/octet-stream")
}

/// Source-code / dev extensions where mime-db is wrong or missing. Returns
/// `None` for everything else so the embedded mime-db table answers.
pub(crate) fn dev_mime_override(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "py" | "pyw" | "pyi" => "text/x-python",
        "rb" => "text/x-ruby",
        // mime-db maps .ts to MPEG transport stream; in a code tree it's
        // overwhelmingly TypeScript.
        "ts" | "tsx" | "mts" | "cts" => "application/typescript",
        "jsx" => "text/jsx",
        "mjs" | "cjs" => "text/javascript",
        "kt" | "kts" => "text/x-kotlin",
        "swift" => "text/x-swift",
        "cs" => "text/x-csharp",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "text/x-c++",
        "vue" => "text/x-vue",
        "svelte" => "text/x-svelte",
        "astro" => "text/x-astro",
        "dockerfile" => "text/x-dockerfile",
        "env" => "text/plain",
        "ex" | "exs" => "text/x-elixir",
        "erl" => "text/x-erlang",
        "hs" => "text/x-haskell",
        "clj" | "cljs" | "cljc" => "text/x-clojure",
        "scala" | "sc" => "text/x-scala",
        "dart" => "application/dart",
        "zig" => "text/x-zig",
        "nim" => "text/x-nim",
        "proto" => "text/x-protobuf",
        "tf" | "tfvars" => "text/x-terraform",
        "gradle" => "text/x-gradle",
        _ => return None,
    })
}

/// Rough px width of `s` at the given font size, used only to decide whether
/// a Name cell is truncated (and so warrants a hover tooltip). The UI font is
/// proportional, so this is an estimate biased slightly high (~0.55em average
/// advance) to avoid attaching tooltips to names that actually fit.
pub(crate) fn approx_text_width(s: &str, size: f32) -> f32 {
    s.chars().count() as f32 * size * 0.55
}

pub(crate) fn format_modified_local(modified: Option<std::time::SystemTime>) -> String {
    let Some(t) = modified else { return String::new() };
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub(crate) fn format_modified_remote(mtime: Option<u32>) -> String {
    let Some(secs) = mtime else { return String::new() };
    match chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

pub(crate) fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[idx])
    }
}
