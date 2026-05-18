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

/// Soft cap on a framed `SyncMessage` once the peer is authenticated.
/// Sized to fit a large sync manifest comfortably; a single record
/// payload is dwarfed by this even with full-fat encrypted vault rows.
pub const MAX_AUTHED_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Cap applied to frames received *before* the peer's Ed25519 identity
/// has been verified (Hello / PairingRequest / RelayHello). Without
/// this, anyone who can reach the QUIC port could announce a 16 MiB
/// length and force the server to allocate that buffer per stream
/// before any auth happens. The real legal frame here is sub-kilobyte
/// (a UUID + pubkey + signature), so 64 KiB leaves ample headroom for
/// future fields while keeping the pre-auth allocation budget small.
pub const MAX_PREAUTH_MESSAGE_BYTES: usize = 64 * 1024;

/// Receive a framed SyncMessage from a QUIC recv stream. Defaults to
/// the post-auth cap; callers that read a pre-auth frame should use
/// [`recv_message_capped`] with [`MAX_PREAUTH_MESSAGE_BYTES`] so an
/// unauthenticated peer cannot force a multi-megabyte allocation.
pub async fn recv_message(
    recv: &mut quinn::RecvStream,
) -> Result<SyncMessage, SyncError> {
    recv_message_capped(recv, MAX_AUTHED_MESSAGE_BYTES).await
}

/// Receive a framed `SyncMessage` and reject the frame if the declared
/// length exceeds `cap`. Use for any read that runs before the sender
/// has been authenticated, so a hostile or accidental large length
/// can't force a giant `vec![0; len]` allocation.
pub async fn recv_message_capped(
    recv: &mut quinn::RecvStream,
    cap: usize,
) -> Result<SyncMessage, SyncError> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| SyncError::Transport(format!("Read len: {}", e)))?;
    let len = u32::from_le_bytes(len_buf) as usize;

    if len > cap {
        return Err(SyncError::Protocol(format!(
            "Message too large: {len} > cap {cap}"
        )));
    }

    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| SyncError::Transport(format!("Read body: {}", e)))?;

    decode_message(&buf).map_err(|e| SyncError::Protocol(format!("Decode: {}", e)))
}

/// Per-session transport. Same `send` / `recv` shape regardless of
/// whether the bytes go over a QUIC stream or HTTP-relayed inboxes,
/// so `handle_sync_session` and the client-side flow don't have to
/// care which one they got.
///
/// A `SessionTransport` is one-session-at-a-time; the engine creates
/// one when a peer connects (or when a `Sync Now` opens a session),
/// runs it to completion, and drops it.
pub enum SessionTransport {
    /// Direct QUIC stream pair, the LAN / cross-NAT-direct path.
    Quic {
        send: quinn::SendStream,
        recv: quinn::RecvStream,
    },
    /// Initiator side of a relay session: we POST outbound frames
    /// directly via `RelayClient::send` and long-poll our own inbox
    /// for the peer's responses. Frames from a different sender on
    /// our inbox are dropped (we sync with one peer at a time).
    RelayClient {
        client: crate::relay::RelayClient,
        peer_id: uuid::Uuid,
        my_id: uuid::Uuid,
    },
    /// Responder side of a relay session: a background inbox listener
    /// demuxes incoming frames by sender into per-session mpsc queues;
    /// this variant pulls from one such queue while still sending via
    /// `RelayClient::send` directly.
    RelayServer {
        client: crate::relay::RelayClient,
        peer_id: uuid::Uuid,
        inbox: tokio::sync::mpsc::UnboundedReceiver<SyncMessage>,
    },
}

impl SessionTransport {
    pub async fn send(&mut self, msg: &SyncMessage) -> Result<(), SyncError> {
        match self {
            Self::Quic { send, .. } => send_message(send, msg).await,
            Self::RelayClient { client, peer_id, .. }
            | Self::RelayServer { client, peer_id, .. } => {
                client.send(*peer_id, msg).await
            }
        }
    }

    /// Block until a frame from the bound peer arrives. Relay frames
    /// from a different sender (multi-peer cross-talk on the inbox)
    /// are silently dropped; they'll get processed when their own
    /// session fires.
    pub async fn recv(&mut self) -> Result<SyncMessage, SyncError> {
        match self {
            Self::Quic { recv, .. } => recv_message(recv).await,
            Self::RelayClient { client, peer_id, my_id } => loop {
                let (from, msg) = client.recv(*my_id).await?;
                if from == *peer_id {
                    return Ok(msg);
                }
                tracing::debug!(
                    "relay: dropping frame from unexpected sender {from} (expected {peer_id})"
                );
            },
            Self::RelayServer { inbox, .. } => inbox
                .recv()
                .await
                .ok_or_else(|| SyncError::Transport("relay inbox closed".into())),
        }
    }
}

// Skip TLS certificate verification. Identity verification happens at
// the application layer via Ed25519: each peer signs the RFC 5705 TLS
// exporter with its long-term identity key during the Hello handshake
// (`engine::handle_incoming` + `engine::sync_with_peer`), and the
// receiver verifies against the pubkey stored on the `SyncPeer` row at
// pairing time. Because the exporter is bound to the specific TLS
// session, a MITM with its own cert (which this verifier would happily
// accept) cannot relay or replay a valid signature: its two TLS
// sessions on either side of the man derive different exporters.
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
