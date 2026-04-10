use std::net::SocketAddr;

use tokio::net::UdpSocket;

use crate::error::SyncError;

/// Default public STUN servers.
const STUN_SERVERS: &[&str] = &[
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun.cloudflare.com:3478",
];

/// STUN binding request (simplified — RFC 5389 minimal).
/// Returns our public IP:port as seen by the STUN server.
pub async fn get_public_addr(local_socket: &UdpSocket) -> Result<SocketAddr, SyncError> {
    for server in STUN_SERVERS {
        match query_stun(local_socket, server).await {
            Ok(addr) => {
                tracing::info!("STUN resolved public address: {} (via {})", addr, server);
                return Ok(addr);
            }
            Err(e) => {
                tracing::warn!("STUN query to {} failed: {}", server, e);
                continue;
            }
        }
    }
    Err(SyncError::Discovery("All STUN servers failed".into()))
}

async fn query_stun(socket: &UdpSocket, server: &str) -> Result<SocketAddr, SyncError> {
    let server_addr: SocketAddr = tokio::net::lookup_host(server)
        .await
        .map_err(|e| SyncError::Discovery(format!("DNS resolve {}: {}", server, e)))?
        .next()
        .ok_or_else(|| SyncError::Discovery(format!("No address for {}", server)))?;

    // STUN Binding Request (RFC 5389)
    // Header: type(2) + length(2) + magic(4) + transaction_id(12) = 20 bytes
    let mut request = [0u8; 20];
    request[0] = 0x00; // Binding Request
    request[1] = 0x01;
    // Length = 0 (no attributes)
    request[2] = 0x00;
    request[3] = 0x00;
    // Magic Cookie
    request[4] = 0x21;
    request[5] = 0x12;
    request[6] = 0xA4;
    request[7] = 0x42;
    // Transaction ID (random 12 bytes)
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut request[8..20]);

    socket
        .send_to(&request, server_addr)
        .await
        .map_err(|e| SyncError::Discovery(format!("STUN send: {}", e)))?;

    let mut buf = [0u8; 256];
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(3), socket.recv_from(&mut buf));

    let (len, _) = timeout
        .await
        .map_err(|_| SyncError::Discovery("STUN timeout".into()))?
        .map_err(|e| SyncError::Discovery(format!("STUN recv: {}", e)))?;

    parse_stun_response(&buf[..len])
}

fn parse_stun_response(data: &[u8]) -> Result<SocketAddr, SyncError> {
    if data.len() < 20 {
        return Err(SyncError::Discovery("STUN response too short".into()));
    }

    // Check it's a Binding Response (0x0101)
    if data[0] != 0x01 || data[1] != 0x01 {
        return Err(SyncError::Discovery("Not a STUN binding response".into()));
    }

    let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let attrs = &data[20..20 + msg_len.min(data.len() - 20)];

    // Parse attributes looking for XOR-MAPPED-ADDRESS (0x0020) or MAPPED-ADDRESS (0x0001)
    let mut offset = 0;
    while offset + 4 <= attrs.len() {
        let attr_type = u16::from_be_bytes([attrs[offset], attrs[offset + 1]]);
        let attr_len = u16::from_be_bytes([attrs[offset + 2], attrs[offset + 3]]) as usize;
        let attr_data = &attrs[offset + 4..offset + 4 + attr_len.min(attrs.len() - offset - 4)];

        match attr_type {
            0x0020 => {
                // XOR-MAPPED-ADDRESS
                return parse_xor_mapped_address(attr_data);
            }
            0x0001 => {
                // MAPPED-ADDRESS (fallback)
                return parse_mapped_address(attr_data);
            }
            _ => {}
        }

        // Attributes are padded to 4-byte boundaries
        offset += 4 + ((attr_len + 3) & !3);
    }

    Err(SyncError::Discovery("No address in STUN response".into()))
}

fn parse_xor_mapped_address(data: &[u8]) -> Result<SocketAddr, SyncError> {
    if data.len() < 8 {
        return Err(SyncError::Discovery("XOR-MAPPED-ADDRESS too short".into()));
    }

    let family = data[1];
    let port = u16::from_be_bytes([data[2], data[3]]) ^ 0x2112; // XOR with magic cookie high bits

    match family {
        0x01 => {
            // IPv4
            let ip = std::net::Ipv4Addr::new(
                data[4] ^ 0x21,
                data[5] ^ 0x12,
                data[6] ^ 0xA4,
                data[7] ^ 0x42,
            );
            Ok(SocketAddr::new(std::net::IpAddr::V4(ip), port))
        }
        _ => Err(SyncError::Discovery("Unsupported address family".into())),
    }
}

fn parse_mapped_address(data: &[u8]) -> Result<SocketAddr, SyncError> {
    if data.len() < 8 {
        return Err(SyncError::Discovery("MAPPED-ADDRESS too short".into()));
    }

    let family = data[1];
    let port = u16::from_be_bytes([data[2], data[3]]);

    match family {
        0x01 => {
            let ip = std::net::Ipv4Addr::new(data[4], data[5], data[6], data[7]);
            Ok(SocketAddr::new(std::net::IpAddr::V4(ip), port))
        }
        _ => Err(SyncError::Discovery("Unsupported address family".into())),
    }
}
