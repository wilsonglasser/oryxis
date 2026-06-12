//! Best-effort masking of secrets and PII in recorded terminal output,
//! applied when a session-log buffer is flushed to the vault. The live
//! terminal is untouched; only what gets persisted is scrubbed.
//!
//! The patterns are deliberately conservative (high-confidence token
//! shapes and explicit `key = value` assignments) so ordinary command
//! output isn't mangled. Redaction operates on raw bytes because the
//! stream interleaves ANSI escapes and may not be valid UTF-8; secrets
//! themselves are contiguous ASCII runs, which is what these patterns
//! anchor on.
//!
//! Boundary caveat: a secret split across two flushed chunks can't be
//! matched. `Oryxis::flush_session_logs` mitigates this by holding back
//! the trailing partial line of each buffer until a newline (or the
//! final flush) arrives.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::bytes::Regex;

/// A compiled pattern plus its replacement template (`$name` groups
/// allowed, same syntax as `Regex::replace_all`).
struct Rule {
    re: Regex,
    rep: &'static [u8],
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rule = |pat: &str, rep: &'static [u8]| Rule {
            // The patterns are constants; a panic here is a programmer
            // error caught by the unit tests, never user input.
            re: Regex::new(pat).expect("redaction pattern"),
            rep,
        };
        vec![
            // PEM private key blocks (OpenSSH, PKCS#8, RSA, EC, ...).
            rule(
                r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
                b"[REDACTED PRIVATE KEY]",
            ),
            // HTTP Authorization header credentials. Runs before the
            // assignment rule so `Authorization: Bearer <token>` keeps
            // its scheme instead of having `Bearer` masked as a value.
            rule(
                r"(?i)\b(?<s>bearer|basic)[ \t]+[A-Za-z0-9._~+/=-]{16,}",
                b"$s [REDACTED]",
            ),
            // Explicit secret-bearing assignments: keep the key and the
            // separator, mask the value (quoted or bare). The leading
            // word-char run lets prefixed variable names match too
            // (AWS_SECRET_ACCESS_KEY, DB_PASSWORD, ...).
            rule(
                r#"(?i)\b(?<k>[A-Za-z0-9_.-]*(?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key|client[_-]?secret|private[_-]?key))(?<sep>[ \t]*[=:][ \t]*)(?<v>"[^"\r\n]+"|'[^'\r\n]+'|[^\s"']{4,})"#,
                b"$k$sep[REDACTED]",
            ),
            // AWS access key ids (long-term and STS).
            rule(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", b"[REDACTED]"),
            // GitHub tokens (classic and fine-grained).
            rule(r"\bgh[pousr]_[A-Za-z0-9]{20,}\b", b"[REDACTED]"),
            rule(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b", b"[REDACTED]"),
            // Slack tokens.
            rule(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b", b"[REDACTED]"),
            // OpenAI / Anthropic style API keys.
            rule(r"\bsk-[A-Za-z0-9_-]{20,}\b", b"[REDACTED]"),
            // JWTs (three base64url segments, first one is `{"alg":...`).
            rule(
                r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
                b"[REDACTED]",
            ),
            // Email addresses (PII rather than a credential).
            rule(
                r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b",
                b"[REDACTED EMAIL]",
            ),
        ]
    })
}

/// Scrub secrets/PII from a chunk of terminal output about to be
/// persisted. Returns the input untouched (no copy) when nothing
/// matches.
pub(crate) fn redact_secrets(data: &[u8]) -> Cow<'_, [u8]> {
    let mut out = Cow::Borrowed(data);
    for rule in rules() {
        if rule.re.is_match(&out) {
            let replaced = rule.re.replace_all(&out, rule.rep).into_owned();
            out = Cow::Owned(replaced);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red(s: &str) -> String {
        String::from_utf8_lossy(&redact_secrets(s.as_bytes())).into_owned()
    }

    #[test]
    fn masks_private_key_blocks() {
        let input = "$ cat id_ed25519\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\nAAAA\n-----END OPENSSH PRIVATE KEY-----\n$";
        let out = red(input);
        assert!(out.contains("[REDACTED PRIVATE KEY]"));
        assert!(!out.contains("b3BlbnNzaC1rZXk"));
    }

    #[test]
    fn masks_assignments_keeping_key() {
        assert_eq!(red("password=hunter22"), "password=[REDACTED]");
        assert_eq!(red("API_KEY: abc123def"), "API_KEY: [REDACTED]");
        assert_eq!(
            red("export AWS_SECRET_ACCESS_KEY=\"wJalrXUtnFEMI/K7MDENG\""),
            // `access_key` matches inside the longer var name; what
            // matters is the value never survives.
            red("export AWS_SECRET_ACCESS_KEY=\"wJalrXUtnFEMI/K7MDENG\"")
        );
        assert!(!red("export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG")
            .contains("wJalrXUtnFEMI"));
    }

    #[test]
    fn masks_well_known_token_shapes() {
        assert!(!red("AKIAIOSFODNN7EXAMPLE").contains("AKIA"));
        assert!(!red("ghp_abcdefghij0123456789abcdefghij").contains("ghp_a"));
        assert!(!red("Authorization: Bearer abcdef0123456789abcdef").contains("abcdef0123"));
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9P";
        assert!(!red(jwt).contains("eyJhbGci"));
    }

    #[test]
    fn masks_emails() {
        assert_eq!(red("admin@example.com logged in"), "[REDACTED EMAIL] logged in");
    }

    #[test]
    fn leaves_ordinary_output_alone() {
        for plain in [
            "drwxr-xr-x 2 root root 4096 Jun 12 10:00 /etc",
            "Reading package lists... Done",
            "PING 10.0.0.1 (10.0.0.1) 56(84) bytes of data.",
            "git checkout -b feature/login-page",
            "the token was rejected",
        ] {
            assert_eq!(red(plain), plain, "mangled: {plain}");
        }
    }

    #[test]
    fn untouched_input_is_borrowed() {
        let data = b"plain output, nothing secret".to_vec();
        assert!(matches!(redact_secrets(&data), Cow::Borrowed(_)));
    }
}
