use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_util::codec::{Decoder, Encoder};
use uuid::Uuid;

use siphon_protocol::{ClientMessage, ServerMessage, TunnelCodec, TunnelType};

use crate::cloudflare::CloudflareClient;
use crate::router::{Router, TunnelHandle};
use crate::state::{HttpResponseData, ResponseRegistry, TcpConnectionRegistry};
use crate::tcp_plane::TcpPlane;

/// Control plane server that accepts tunnel client connections via mTLS
pub struct ControlPlane {
    router: Arc<Router>,
    tls_acceptor: TlsAcceptor,
    cloudflare: Arc<CloudflareClient>,
    base_domain: String,
    response_registry: ResponseRegistry,
    tcp_plane: Arc<TcpPlane>,
    tcp_registry: TcpConnectionRegistry,
}

impl ControlPlane {
    pub fn new(
        router: Arc<Router>,
        tls_acceptor: TlsAcceptor,
        cloudflare: Arc<CloudflareClient>,
        base_domain: String,
        response_registry: ResponseRegistry,
        tcp_plane: Arc<TcpPlane>,
        tcp_registry: TcpConnectionRegistry,
    ) -> Arc<Self> {
        Arc::new(Self {
            router,
            tls_acceptor,
            cloudflare,
            base_domain,
            response_registry,
            tcp_plane,
            tcp_registry,
        })
    }

    /// Start listening for tunnel client connections
    pub async fn run(self: Arc<Self>, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("Control plane listening on {}", addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let this = self.clone();

            tokio::spawn(async move {
                if let Err(e) = this.handle_connection(stream, peer_addr).await {
                    tracing::error!("Connection error from {}: {}", peer_addr, e);
                }
            });
        }
    }

    async fn handle_connection(
        self: Arc<Self>,
        stream: TcpStream,
        peer_addr: SocketAddr,
    ) -> Result<()> {
        tracing::info!("New connection from {}", peer_addr);

        // Perform TLS handshake with client cert verification
        let tls_stream = self.tls_acceptor.accept(stream).await?;
        tracing::info!("TLS handshake complete with {}", peer_addr);

        // Extract client identity from certificate
        let client_id = extract_client_id(&tls_stream);
        tracing::info!("Client identified as: {}", client_id);

        // Split the stream for reading and writing
        let (read_half, write_half) = tokio::io::split(tls_stream);

        // Create channels for communication
        let (tx, mut rx) = mpsc::channel::<ServerMessage>(32);

        // Read loop: process incoming messages from client
        let router = self.router.clone();
        let cloudflare = self.cloudflare.clone();
        let base_domain = self.base_domain.clone();
        let client_id_clone = client_id.clone();
        let response_registry = self.response_registry.clone();
        let tcp_plane = self.tcp_plane.clone();
        let _tcp_registry = self.tcp_registry.clone();

        let mut codec = TunnelCodec::<ClientMessage>::new();
        let mut read_buf = BytesMut::with_capacity(8192);

        // State for this connection
        let mut assigned_subdomain: Option<String> = None;
        let mut assigned_tcp_port: Option<u16> = None;

        // Spawn write task
        let write_handle = tokio::spawn(async move {
            let mut write_half = write_half;
            let mut codec = TunnelCodec::<ServerMessage>::new();
            let mut write_buf = BytesMut::with_capacity(8192);

            while let Some(msg) = rx.recv().await {
                write_buf.clear();
                if let Err(e) = codec.encode(msg, &mut write_buf) {
                    tracing::error!("Failed to encode message: {}", e);
                    break;
                }
                if let Err(e) = write_half.write_all(&write_buf).await {
                    tracing::error!("Failed to write message: {}", e);
                    break;
                }
            }
        });

        // Read loop
        let mut read_half = read_half;
        loop {
            // Read more data
            match read_half.read_buf(&mut read_buf).await {
                Ok(0) => {
                    tracing::info!("Client {} disconnected", peer_addr);
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Read error: {}", e);
                    break;
                }
            };

            // Try to decode messages
            loop {
                match codec.decode(&mut read_buf) {
                    Ok(Some(msg)) => {
                        match msg {
                            ClientMessage::RequestTunnel {
                                subdomain,
                                tunnel_type,
                                local_port,
                            } => {
                                tracing::info!(
                                    "Tunnel request from {}: subdomain={:?}, type={:?}, local_port={}",
                                    client_id_clone,
                                    subdomain,
                                    tunnel_type,
                                    local_port
                                );

                                // Generate or validate subdomain
                                let subdomain = subdomain.unwrap_or_else(|| {
                                    // Generate random subdomain (ensure first char is a letter)
                                    let id = Uuid::new_v4().to_string();
                                    let first = id.chars().next().unwrap();
                                    let prefix = if first.is_ascii_digit() {
                                        // Map 0-9 to a-j
                                        char::from(b'a' + first.to_digit(10).unwrap() as u8)
                                    } else {
                                        first
                                    };
                                    format!("{}{}", prefix, &id[1..8])
                                });

                                // Validate subdomain format
                                if !is_valid_subdomain(&subdomain) {
                                    let _ = tx
                                        .send(ServerMessage::TunnelDenied {
                                            reason: "Invalid subdomain format".to_string(),
                                        })
                                        .await;
                                    continue;
                                }

                                // Check availability
                                if !router.is_available(&subdomain) {
                                    let _ = tx
                                        .send(ServerMessage::TunnelDenied {
                                            reason: "Subdomain already in use".to_string(),
                                        })
                                        .await;
                                    continue;
                                }

                                // For TCP tunnels, allocate a port first
                                let tcp_port = if tunnel_type == TunnelType::Tcp {
                                    match tcp_plane
                                        .clone()
                                        .allocate_and_listen(subdomain.clone())
                                        .await
                                    {
                                        Ok(port) => Some(port),
                                        Err(e) => {
                                            tracing::error!("Failed to allocate TCP port: {}", e);
                                            let _ = tx
                                                .send(ServerMessage::TunnelDenied {
                                                    reason: format!(
                                                        "TCP port allocation failed: {}",
                                                        e
                                                    ),
                                                })
                                                .await;
                                            continue;
                                        }
                                    }
                                } else {
                                    None
                                };

                                // Create DNS record
                                let proxied = tunnel_type == TunnelType::Http;
                                match cloudflare.create_record(&subdomain, proxied).await {
                                    Ok(record_id) => {
                                        // Create tunnel handle
                                        let handle = TunnelHandle {
                                            sender: tx.clone(),
                                            client_id: client_id_clone.clone(),
                                            tunnel_type: tunnel_type.clone(),
                                            dns_record_id: Some(record_id),
                                        };

                                        // Register the tunnel
                                        if let Err(e) =
                                            router.register(subdomain.clone(), handle, tcp_port)
                                        {
                                            tracing::error!("Failed to register tunnel: {}", e);
                                            // Release TCP port if allocated
                                            if let Some(port) = tcp_port {
                                                tcp_plane.release_port(port);
                                            }
                                            let _ = tx
                                                .send(ServerMessage::TunnelDenied {
                                                    reason: format!("Registration failed: {}", e),
                                                })
                                                .await;
                                            continue;
                                        }

                                        assigned_subdomain = Some(subdomain.clone());
                                        assigned_tcp_port = tcp_port;

                                        let (full_url, response_port) = if tunnel_type
                                            == TunnelType::Http
                                        {
                                            (format!("https://{}.{}", subdomain, base_domain), None)
                                        } else {
                                            (format!("{}.{}", subdomain, base_domain), tcp_port)
                                        };

                                        tracing::info!(
                                            "Tunnel established: {} -> {} (port: {:?})",
                                            full_url,
                                            local_port,
                                            response_port
                                        );

                                        let _ = tx
                                            .send(ServerMessage::TunnelEstablished {
                                                subdomain: subdomain.clone(),
                                                url: full_url,
                                                port: response_port,
                                            })
                                            .await;
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to create DNS record: {}", e);
                                        // Release TCP port if allocated
                                        if let Some(port) = tcp_port {
                                            tcp_plane.release_port(port);
                                        }
                                        let _ = tx
                                            .send(ServerMessage::TunnelDenied {
                                                reason: format!("DNS error: {}", e),
                                            })
                                            .await;
                                    }
                                }
                            }
                            ClientMessage::HttpResponse {
                                stream_id,
                                status,
                                headers,
                                body,
                            } => {
                                // Forward response to the waiting HTTP handler
                                tracing::debug!(
                                    "Received HTTP response for stream {}: status={}",
                                    stream_id,
                                    status
                                );

                                // Look up the pending response in the shared registry
                                if let Some((_, sender)) = response_registry.remove(&stream_id) {
                                    let response = HttpResponseData {
                                        status,
                                        headers,
                                        body,
                                    };
                                    if sender.send(response).is_err() {
                                        tracing::warn!(
                                            "Failed to send response for stream {} (receiver dropped)",
                                            stream_id
                                        );
                                    }
                                } else {
                                    tracing::warn!(
                                        "No pending request for stream {} (may have timed out)",
                                        stream_id
                                    );
                                }
                            }
                            ClientMessage::TcpData { stream_id, data } => {
                                tracing::debug!(
                                    "Received TCP data for stream {}: {} bytes",
                                    stream_id,
                                    data.len()
                                );
                                // Forward to TCP plane
                                if let Some(writer) = tcp_plane.get_writer(stream_id) {
                                    if let Err(e) = writer.send(data).await {
                                        tracing::error!(
                                            "Failed to forward TCP data to stream {}: {}",
                                            stream_id,
                                            e
                                        );
                                    }
                                } else {
                                    tracing::warn!(
                                        "No TCP connection for stream {} (may have been closed)",
                                        stream_id
                                    );
                                }
                            }
                            ClientMessage::TcpClose { stream_id } => {
                                tracing::debug!("TCP connection {} closed by client", stream_id);
                                // Close the TCP connection
                                tcp_plane.close_connection(stream_id);
                            }
                            ClientMessage::Ping { timestamp } => {
                                let _ = tx.send(ServerMessage::Pong { timestamp }).await;
                            }
                        }
                    }
                    Ok(None) => break, // Need more data
                    Err(e) => {
                        tracing::error!("Decode error: {}", e);
                        break;
                    }
                }
            }
        }

        // Cleanup
        tracing::info!("Cleaning up connection for {}", client_id);

        // Unregister tunnel
        if let Some(subdomain) = &assigned_subdomain {
            if let Some(handle) = router.unregister(subdomain) {
                // Delete DNS record
                if let Some(record_id) = handle.dns_record_id {
                    if let Err(e) = cloudflare.delete_record(&record_id).await {
                        tracing::error!("Failed to delete DNS record: {}", e);
                    }
                }
            }
        }

        // Release TCP port if allocated
        if let Some(port) = assigned_tcp_port {
            tcp_plane.release_port(port);
        }

        write_handle.abort();
        Ok(())
    }
}

/// Extract client ID from TLS connection (certificate CN)
fn extract_client_id<S>(tls_stream: &tokio_rustls::server::TlsStream<S>) -> String {
    // In a full implementation, we would extract the CN from the client certificate
    // For now, generate a unique ID
    let (_, server_conn) = tls_stream.get_ref();

    if let Some(certs) = server_conn.peer_certificates() {
        if let Some(cert) = certs.first() {
            // Hash the certificate for a stable ID
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            cert.as_ref().hash(&mut hasher);
            return format!("client-{:016x}", hasher.finish());
        }
    }

    format!("unknown-{}", Uuid::new_v4())
}

/// Validate subdomain format (alphanumeric and hyphens only)
fn is_valid_subdomain(subdomain: &str) -> bool {
    if subdomain.is_empty() || subdomain.len() > 63 {
        return false;
    }

    // Must start and end with alphanumeric
    let chars: Vec<char> = subdomain.chars().collect();
    if !chars.first().map(|c| c.is_alphanumeric()).unwrap_or(false) {
        return false;
    }
    if !chars.last().map(|c| c.is_alphanumeric()).unwrap_or(false) {
        return false;
    }

    // Only alphanumeric and hyphens
    subdomain.chars().all(|c| c.is_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_subdomains() {
        assert!(is_valid_subdomain("myapp"));
        assert!(is_valid_subdomain("my-app"));
        assert!(is_valid_subdomain("my-app-123"));
        assert!(is_valid_subdomain("a"));
        assert!(is_valid_subdomain("123"));
    }

    #[test]
    fn test_invalid_subdomains() {
        assert!(!is_valid_subdomain(""));
        assert!(!is_valid_subdomain("-myapp"));
        assert!(!is_valid_subdomain("myapp-"));
        assert!(!is_valid_subdomain("my_app"));
        assert!(!is_valid_subdomain("my.app"));
        assert!(!is_valid_subdomain(&"a".repeat(64)));
    }
}
