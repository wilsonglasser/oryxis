use std::net::SocketAddr;
use std::sync::Arc;

use quinn::{Endpoint, ServerConfig, ClientConfig};

use crate::crypto;
use crate::error::SyncError;
use crate::protocol::{SyncMessage, encode_message, decode_message};

/// Create a QUIC server endpoint with self-signed TLS.
pub fn create_server_endpoint(
    device_id: &uuid::Uuid,
    listen_port: u16,
) -> Result<Endpoint, SyncError> {
    let (cert_der, key_der) = crypto::generate_tls_cert(device_id)?;

    let cert_chain = vec![rustls::pki_types::CertificateDer::from(cert_der)];
    let key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
        .map_err(|e| SyncError::Transport(format!("Invalid key: {}", e)))?;

    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| SyncError::Transport(format!("TLS config: {}", e)))?;
    server_crypto.alpn_protocols = vec![b"oryxis-sync/1".to_vec()];

    let server_config = ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
            .map_err(|e| SyncError::Transport(format!("QUIC config: {}", e)))?,
    ));

    let addr: SocketAddr = format!("0.0.0.0:{}", listen_port)
        .parse()
        .map_err(|e| SyncError::Transport(format!("Invalid addr: {}", e)))?;

    let endpoint = Endpoint::server(server_config, addr)
        .map_err(|e| SyncError::Transport(format!("Bind failed: {}", e)))?;

    tracing::info!("QUIC server listening on {}", endpoint.local_addr().unwrap());
    Ok(endpoint)
}

/// Create a QUIC client endpoint (connects to peers).
pub fn create_client_endpoint() -> Result<Endpoint, SyncError> {
    let mut client_crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipVerification))
        .with_no_client_auth();
    client_crypto.alpn_protocols = vec![b"oryxis-sync/1".to_vec()];

    let client_config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
            .map_err(|e| SyncError::Transport(format!("Client QUIC config: {}", e)))?,
    ));

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| SyncError::Transport(format!("Client bind: {}", e)))?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

/// Send a framed SyncMessage over a QUIC send stream.
pub async fn send_message(
    send: &mut quinn::SendStream,
    msg: &SyncMessage,
) -> Result<(), SyncError> {
    let frame = encode_message(msg)
        .map_err(|e| SyncError::Protocol(format!("Encode: {}", e)))?;
    send.write_all(&frame)
        .await
        .map_err(|e| SyncError::Transport(format!("Write: {}", e)))?;
    Ok(())
}

/// Receive a framed SyncMessage from a QUIC recv stream.
pub async fn recv_message(
    recv: &mut quinn::RecvStream,
) -> Result<SyncMessage, SyncError> {
    // Read 4-byte length prefix
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| SyncError::Transport(format!("Read len: {}", e)))?;
    let len = u32::from_le_bytes(len_buf) as usize;

    if len > 16 * 1024 * 1024 {
        return Err(SyncError::Protocol("Message too large".into()));
    }

    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| SyncError::Transport(format!("Read body: {}", e)))?;

    decode_message(&buf).map_err(|e| SyncError::Protocol(format!("Decode: {}", e)))
}

// Skip TLS certificate verification (we verify via Ed25519 keys at the application layer).
#[derive(Debug)]
struct SkipVerification;

impl rustls::client::danger::ServerCertVerifier for SkipVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}
