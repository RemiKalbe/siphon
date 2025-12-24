use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use siphon_protocol::ServerMessage;

use crate::router::Router;
use crate::state::{PortAllocator, StreamIdGenerator, TcpConnectionHandle, TcpConnectionRegistry};

/// TCP data plane for direct TCP tunnel connections
pub struct TcpPlane {
    router: Arc<Router>,
    port_allocator: Arc<PortAllocator>,
    tcp_registry: TcpConnectionRegistry,
    stream_id_gen: Arc<StreamIdGenerator>,
}

impl TcpPlane {
    pub fn new(
        router: Arc<Router>,
        port_allocator: Arc<PortAllocator>,
        tcp_registry: TcpConnectionRegistry,
        stream_id_gen: Arc<StreamIdGenerator>,
    ) -> Arc<Self> {
        Arc::new(Self {
            router,
            port_allocator,
            tcp_registry,
            stream_id_gen,
        })
    }

    /// Allocate a port and start listening for TCP connections
    pub async fn allocate_and_listen(
        self: Arc<Self>,
        subdomain: String,
    ) -> Result<u16> {
        let port = self
            .port_allocator
            .allocate()
            .ok_or_else(|| anyhow::anyhow!("No available ports"))?;

        let addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;
        let listener = TcpListener::bind(addr).await?;

        tracing::info!("TCP plane listening on {} for subdomain {}", addr, subdomain);

        let this = self.clone();
        let subdomain_clone = subdomain.clone();

        // Spawn listener task
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        tracing::info!(
                            "TCP connection from {} for subdomain {}",
                            peer_addr,
                            subdomain_clone
                        );
                        let this = this.clone();
                        let subdomain = subdomain_clone.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.handle_tcp_connection(stream, subdomain).await {
                                tracing::error!("TCP connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("TCP accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(port)
    }

    /// Handle an incoming TCP connection
    async fn handle_tcp_connection(
        self: Arc<Self>,
        stream: TcpStream,
        subdomain: String,
    ) -> Result<()> {
        let stream_id = self.stream_id_gen.next();
        tracing::debug!("New TCP stream {} for subdomain {}", stream_id, subdomain);

        // Get sender for this subdomain
        let tunnel_sender = match self.router.get_sender(&subdomain) {
            Some(s) => s,
            None => {
                tracing::warn!("No tunnel for subdomain: {}", subdomain);
                return Ok(());
            }
        };

        // Split the stream
        let (mut read_half, mut write_half) = stream.into_split();

        // Create channel for writing data back to this TCP connection
        let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(32);

        // Register this connection
        self.tcp_registry.insert(
            stream_id,
            TcpConnectionHandle {
                writer: write_tx,
                subdomain: subdomain.clone(),
            },
        );

        // Send TcpConnect to client
        if let Err(e) = tunnel_sender.send(ServerMessage::TcpConnect { stream_id }).await {
            tracing::error!("Failed to send TcpConnect: {}", e);
            self.tcp_registry.remove(&stream_id);
            return Ok(());
        }

        // Spawn write task (receives data from tunnel client, writes to TCP)
        let tcp_registry = self.tcp_registry.clone();
        let tunnel_sender_clone = tunnel_sender.clone();
        let write_task = tokio::spawn(async move {
            while let Some(data) = write_rx.recv().await {
                if let Err(e) = write_half.write_all(&data).await {
                    tracing::error!("Failed to write to TCP stream {}: {}", stream_id, e);
                    break;
                }
            }
            // Connection closed, send TcpClose
            let _ = tunnel_sender_clone
                .send(ServerMessage::TcpClose { stream_id })
                .await;
            tcp_registry.remove(&stream_id);
        });

        // Read from TCP, send to tunnel
        let mut buf = vec![0u8; 8192];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) => {
                    // EOF
                    tracing::debug!("TCP stream {} closed by remote", stream_id);
                    break;
                }
                Ok(n) => {
                    let data = buf[..n].to_vec();
                    if let Err(e) = tunnel_sender
                        .send(ServerMessage::TcpData { stream_id, data })
                        .await
                    {
                        tracing::error!("Failed to send TcpData: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("TCP read error on stream {}: {}", stream_id, e);
                    break;
                }
            }
        }

        // Clean up
        self.tcp_registry.remove(&stream_id);
        write_task.abort();

        // Send TcpClose
        let _ = tunnel_sender
            .send(ServerMessage::TcpClose { stream_id })
            .await;

        Ok(())
    }

    /// Release a port when tunnel is closed
    pub fn release_port(&self, port: u16) {
        self.port_allocator.release(port);
    }

    /// Get write channel for a stream
    pub fn get_writer(&self, stream_id: u64) -> Option<mpsc::Sender<Vec<u8>>> {
        self.tcp_registry.get(&stream_id).map(|h| h.writer.clone())
    }

    /// Close a TCP connection
    pub fn close_connection(&self, stream_id: u64) {
        if let Some((_, handle)) = self.tcp_registry.remove(&stream_id) {
            // Dropping the sender will cause the write task to exit
            drop(handle);
        }
    }
}
