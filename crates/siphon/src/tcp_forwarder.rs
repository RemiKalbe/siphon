use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use siphon_protocol::ClientMessage;

/// Handle to a TCP connection
struct TcpConnectionHandle {
    writer: mpsc::Sender<Vec<u8>>,
}

/// Manages TCP connections to the local service
pub struct TcpForwarder {
    local_addr: String,
    connections: Arc<DashMap<u64, TcpConnectionHandle>>,
    response_tx: mpsc::Sender<ClientMessage>,
}

impl TcpForwarder {
    pub fn new(local_addr: String, response_tx: mpsc::Sender<ClientMessage>) -> Self {
        Self {
            local_addr,
            connections: Arc::new(DashMap::new()),
            response_tx,
        }
    }

    /// Handle a new TCP connection request from the server
    pub async fn handle_connect(&self, stream_id: u64) {
        tracing::debug!(
            "Opening TCP connection {} to {}",
            stream_id,
            self.local_addr
        );

        // Connect to local service
        let stream = match TcpStream::connect(&self.local_addr).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    "Failed to connect to local service {}: {}",
                    self.local_addr,
                    e
                );
                // Send TcpClose to indicate connection failed
                let _ = self
                    .response_tx
                    .send(ClientMessage::TcpClose { stream_id })
                    .await;
                return;
            }
        };

        let (mut read_half, mut write_half) = stream.into_split();

        // Create channel for writing to this connection
        let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(32);

        // Register the connection
        self.connections
            .insert(stream_id, TcpConnectionHandle { writer: write_tx });

        // Spawn write task
        let connections = self.connections.clone();
        let response_tx = self.response_tx.clone();
        tokio::spawn(async move {
            while let Some(data) = write_rx.recv().await {
                if let Err(e) = write_half.write_all(&data).await {
                    tracing::error!("Failed to write to local TCP stream {}: {}", stream_id, e);
                    break;
                }
            }
            // Clean up
            connections.remove(&stream_id);
            let _ = response_tx
                .send(ClientMessage::TcpClose { stream_id })
                .await;
        });

        // Spawn read task - read from local service and send to server
        let connections = self.connections.clone();
        let response_tx = self.response_tx.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            loop {
                match read_half.read(&mut buf).await {
                    Ok(0) => {
                        // EOF - connection closed
                        tracing::debug!("Local TCP connection {} closed", stream_id);
                        break;
                    }
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        if let Err(e) = response_tx
                            .send(ClientMessage::TcpData { stream_id, data })
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
            connections.remove(&stream_id);
            let _ = response_tx
                .send(ClientMessage::TcpClose { stream_id })
                .await;
        });
    }

    /// Handle incoming TCP data from the server
    pub async fn handle_data(&self, stream_id: u64, data: Vec<u8>) {
        if let Some(handle) = self.connections.get(&stream_id) {
            if let Err(e) = handle.writer.send(data).await {
                tracing::error!("Failed to forward TCP data to stream {}: {}", stream_id, e);
            }
        } else {
            tracing::warn!(
                "Received TCP data for unknown stream {} (may have been closed)",
                stream_id
            );
        }
    }

    /// Handle TCP connection close from the server
    pub fn handle_close(&self, stream_id: u64) {
        if let Some((_, handle)) = self.connections.remove(&stream_id) {
            // Dropping the sender will cause the write task to exit
            drop(handle);
            tracing::debug!("Closed TCP connection {}", stream_id);
        }
    }
}
