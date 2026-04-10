use std::net::SocketAddr;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::DiscoveredPeer;
use crate::error::SyncError;

const SERVICE_TYPE: &str = "_oryxis-sync._udp.local.";
const SERVICE_NAME: &str = "oryxis-sync";

/// Register this device on mDNS for LAN discovery.
pub fn register(
    device_id: &Uuid,
    port: u16,
) -> Result<ServiceDaemon, SyncError> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| SyncError::Discovery(format!("mDNS init: {}", e)))?;

    let host = "oryxis-host";
    let instance_name = format!("{}.{}", device_id, SERVICE_NAME);
    let properties: [(&str, &str); 1] = [("device_id", &device_id.to_string())];

    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &format!("{}.", host),
        "",
        port,
        &properties[..],
    )
    .map_err(|e| SyncError::Discovery(format!("mDNS service info: {}", e)))?;

    mdns.register(service)
        .map_err(|e| SyncError::Discovery(format!("mDNS register: {}", e)))?;

    tracing::info!("mDNS registered on port {}", port);
    Ok(mdns)
}

/// Browse for Oryxis peers on the local network.
/// Sends discovered peers to the channel.
pub fn browse(
    own_device_id: &Uuid,
    tx: mpsc::UnboundedSender<DiscoveredPeer>,
) -> Result<ServiceDaemon, SyncError> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| SyncError::Discovery(format!("mDNS browse init: {}", e)))?;

    let receiver = mdns.browse(SERVICE_TYPE)
        .map_err(|e| SyncError::Discovery(format!("mDNS browse: {}", e)))?;

    let own_id = *own_device_id;

    tokio::spawn(async move {
        while let Ok(event) = receiver.recv_async().await {
            if let ServiceEvent::ServiceResolved(info) = event {
                // Extract device_id from TXT record
                let device_id_str = info.get_properties()
                    .get_property_val_str("device_id")
                    .unwrap_or_default()
                    .to_string();

                if let Ok(peer_id) = Uuid::parse_str(&device_id_str) {
                    if peer_id == own_id {
                        continue; // Skip self
                    }

                    // Get first address
                    if let Some(addr) = info.get_addresses().iter().next() {
                        let socket_addr = SocketAddr::new(*addr, info.get_port());
                        tracing::info!("mDNS discovered peer {} at {}", peer_id, socket_addr);
                        let _ = tx.send(DiscoveredPeer {
                            device_id: peer_id,
                            addr: socket_addr,
                            method: super::DiscoveryMethod::Lan,
                        });
                    }
                }
            }
        }
    });

    Ok(mdns)
}
