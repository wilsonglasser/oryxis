//! Certificate inspection for the HTTP check.
//!
//! **The verifier in this file accepts every chain, and nothing outside
//! this file may use it.** That is the point: an expired or self-signed
//! certificate is exactly what the user opened the panel to look at, and
//! a verifier that refused it would report "handshake failed" instead of
//! showing them the certificate that is wrong. The connection this opens
//! sends nothing and reads nothing: it completes the handshake, copies
//! the chain the server presented, and closes.
//!
//! Whether the chain is TRUSTED is answered somewhere else entirely, by
//! whether the ordinary request in `net_tools::http` succeeded, which is
//! the same trust store the user's browser and the rest of the app use.
//! Keeping the two apart is what keeps this file from becoming a way to
//! make an untrusted connection look fine.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use x509_parser::prelude::FromDer;

use crate::i18n::t;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// What the panel reports about one certificate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CertSummary {
    pub subject: String,
    pub issuer: String,
    pub sans: Vec<String>,
    pub not_before: String,
    pub not_after: String,
    /// Days until expiry, negative once expired.
    pub days_left: i64,
    /// How many certificates the server sent. A chain of one is a
    /// frequent misconfiguration (the intermediate is missing), which is
    /// why the count is reported rather than just the leaf.
    pub chain_len: usize,
}

impl CertSummary {
    /// Whether the certificate is inside its validity window right now.
    pub(crate) fn expired(&self) -> bool {
        self.days_left < 0
    }
}

/// Complete a TLS handshake with `host:port` and describe the leaf
/// certificate. Errors are the handshake's own (name resolution, a
/// refused port, a server that speaks no TLS); a certificate that is
/// invalid is a successful inspection, not an error.
pub(crate) async fn inspect(host: &str, port: u16) -> Result<CertSummary, String> {
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| format!("{}: {host}", t("net_err_bad_host")))?;
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
    let verifier = Arc::new(CaptureVerifier {
        provider: provider.clone(),
        chain: Mutex::new(Vec::new()),
    });
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    // The certificate does not depend on the protocol negotiated, and
    // offering h2 here would only add a way for the handshake to fail.
    config.alpn_protocols.clear();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let work = async {
        let tcp = tokio::net::TcpStream::connect((host, port))
            .await
            .map_err(|e| e.to_string())?;
        connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    };
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, work).await {
        Ok(res) => res?,
        Err(_) => return Err(t("net_err_timeout").to_string()),
    }

    let chain = verifier.chain.lock().map_err(|_| t("net_err_tls").to_string())?.clone();
    let leaf = chain.first().ok_or_else(|| t("net_err_no_cert").to_string())?;
    summarize(leaf, chain.len())
}

/// Turn the leaf's DER into the summary the card renders.
pub(crate) fn summarize(der: &[u8], chain_len: usize) -> Result<CertSummary, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    summarize_at(der, chain_len, now)
}

/// [`summarize`] with the clock passed in, so the expiry arithmetic is
/// testable against a fixture whose dates cannot be chosen to be in the
/// past on the day the test runs.
pub(crate) fn summarize_at(
    der: &[u8],
    chain_len: usize,
    now: i64,
) -> Result<CertSummary, String> {
    let (_, cert) = x509_parser::certificate::X509Certificate::from_der(der)
        .map_err(|e| format!("{}: {e}", t("net_err_bad_cert")))?;
    let validity = cert.validity();
    let sans = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|ext| ext.value.general_names.iter().map(render_general_name).collect::<Vec<_>>())
        .unwrap_or_default();
    // `time_to_expiration` is None once the certificate is past its
    // notAfter, which is precisely the case the card most needs a number
    // for, so the remaining days are computed from the timestamps.
    let days_left = (validity.not_after.timestamp() - now) / 86_400;
    Ok(CertSummary {
        subject: cert.subject().to_string(),
        issuer: cert.issuer().to_string(),
        sans,
        not_before: validity.not_before.to_string(),
        not_after: validity.not_after.to_string(),
        days_left,
        chain_len,
    })
}

/// One subject-alternative name as the certificate means it. The
/// parser's own `Display` wraps each entry in its variant
/// (`DNSName(example.com)`), which is the kind of detail that makes a
/// card read as debug output rather than as the names the certificate
/// covers.
fn render_general_name(name: &x509_parser::extensions::GeneralName<'_>) -> String {
    use x509_parser::extensions::GeneralName;
    match name {
        GeneralName::DNSName(s) | GeneralName::RFC822Name(s) | GeneralName::URI(s) => {
            (*s).to_string()
        }
        // An IP SAN is raw bytes: 4 for v4, 16 for v6, and anything else
        // is a certificate nobody should be trusting anyway, so it falls
        // through to the parser's own rendering rather than being
        // guessed at.
        GeneralName::IPAddress(bytes) => match bytes.len() {
            4 => std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]).to_string(),
            16 => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(bytes);
                std::net::Ipv6Addr::from(octets).to_string()
            }
            _ => format!("{name}"),
        },
        other => format!("{other}"),
    }
}

/// Accepts everything and remembers what it was given. See the module
/// docs: this exists so a broken certificate can be DISPLAYED, and the
/// connection it permits carries no application data.
#[derive(Debug)]
struct CaptureVerifier {
    provider: Arc<rustls::crypto::CryptoProvider>,
    chain: Mutex<Vec<Vec<u8>>>,
}

impl ServerCertVerifier for CaptureVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if let Ok(mut chain) = self.chain.lock() {
            chain.clear();
            chain.push(end_entity.to_vec());
            chain.extend(intermediates.iter().map(|c| c.to_vec()));
        }
        Ok(ServerCertVerified::assertion())
    }

    /// Signature checks are NOT waived: they prove the peer holds the
    /// key for the certificate it just presented, which is what makes
    /// the captured chain that server's chain rather than anyone's.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A self-signed certificate generated once with openssl (its key
    /// was discarded on the spot: nothing here signs anything), so the
    /// summary is read from a real DER rather than a mock. Valid from
    /// 2026-08-29 to 2036-08-26.
    const SELF_SIGNED: &[u8] = include_bytes!("testdata/selfsigned.der");
    /// 2030-01-01, inside the fixture's window.
    const DURING: i64 = 1_893_456_000;
    /// 2040-01-01, past its notAfter.
    const AFTER: i64 = 2_208_988_800;

    #[test]
    fn summary_reads_the_certificate() {
        let s = summarize_at(SELF_SIGNED, 1, DURING).expect("parse");
        assert!(s.subject.contains("oryxis.test"), "subject: {}", s.subject);
        assert!(s.issuer.contains("oryxis.test"), "self-signed: issuer == subject");
        assert!(!s.not_before.is_empty());
        assert!(!s.not_after.is_empty());
        assert_eq!(s.chain_len, 1);
    }

    #[test]
    fn subject_alternative_names_are_listed() {
        let s = summarize_at(SELF_SIGNED, 1, DURING).expect("parse");
        // Bare names, not the parser's `DNSName(...)` wrapper: the card
        // shows what the certificate covers, not how it was decoded.
        assert_eq!(s.sans, vec!["oryxis.test".to_string(), "www.oryxis.test".to_string()]);
    }

    #[test]
    fn expiry_is_reported_from_both_sides_of_the_window() {
        let valid = summarize_at(SELF_SIGNED, 1, DURING).expect("parse");
        assert!(!valid.expired());
        assert!(valid.days_left > 0);

        // Past notAfter the count goes negative rather than saturating
        // at zero, so the card can say how long it has been expired.
        let stale = summarize_at(SELF_SIGNED, 1, AFTER).expect("parse");
        assert!(stale.expired(), "days_left = {}", stale.days_left);
        assert!(stale.days_left < 0);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(summarize(&[0x30, 0x00], 1).is_err());
        assert!(summarize(b"not a certificate", 1).is_err());
        assert!(summarize(&[], 1).is_err());
    }
}
