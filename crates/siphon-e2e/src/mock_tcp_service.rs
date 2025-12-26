//! Mock TCP service for E2E tests
//!
//! This module provides a mock TCP service that can echo data back,
//! record received data, and send configurable responses.

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// Behavior mode for the mock TCP service
#[derive(Clone, Debug)]
pub enum TcpServiceMode {
    /// Echo back all received data
    Echo,
    /// Send a fixed response for each connection, then close
    FixedResponse(Vec<u8>),
    /// Accumulate data and send response when connection closes
    Accumulate,
}

/// A recorded TCP connection
#[derive(Clone, Debug)]
pub struct RecordedTcpConnection {
    /// All data received on this connection
    pub received_data: Vec<u8>,
    /// Peer address
    pub peer_addr: SocketAddr,
}

/// A mock TCP service for testing
pub struct MockTcpService {
    addr: SocketAddr,
    /// Recorded connections
    connections: Arc<RwLock<Vec<RecordedTcpConnection>>>,
    /// Service mode
    mode: Arc<RwLock<TcpServiceMode>>,
    /// Shutdown channel
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl MockTcpService {
    /// Start a mock TCP service on an ephemeral port
    pub async fn start() -> Self {
        Self::start_with_mode(TcpServiceMode::Echo).await
    }

    /// Start a mock TCP service with a specific mode
    pub async fn start_with_mode(mode: TcpServiceMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind mock TCP service");
        let addr = listener.local_addr().unwrap();

        let connections: Arc<RwLock<Vec<RecordedTcpConnection>>> = Arc::new(RwLock::new(Vec::new()));
        let mode = Arc::new(RwLock::new(mode));

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        let connections_clone = connections.clone();
        let mode_clone = mode.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        tracing::debug!("Mock TCP service shutting down");
                        break;
                    }
                    result = listener.accept() => {
                        match result {
                            Ok((stream, peer_addr)) => {
                                let connections = connections_clone.clone();
                                let mode = mode_clone.read().clone();

                                tokio::spawn(async move {
                                    handle_connection(stream, peer_addr, connections, mode).await;
                                });
                            }
                            Err(e) => {
                                tracing::error!("TCP accept error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        Self {
            addr,
            connections,
            mode,
            shutdown_tx: Some(shutdown_tx),
        }
    }

    /// Get the address this service is listening on
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the address as a string
    pub fn addr_string(&self) -> String {
        self.addr.to_string()
    }

    /// Get the port
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Get all recorded connections
    pub fn get_connections(&self) -> Vec<RecordedTcpConnection> {
        self.connections.read().clone()
    }

    /// Get total bytes received across all connections
    pub fn total_bytes_received(&self) -> usize {
        self.connections
            .read()
            .iter()
            .map(|c| c.received_data.len())
            .sum()
    }

    /// Get connection count
    pub fn connection_count(&self) -> usize {
        self.connections.read().len()
    }

    /// Clear recorded connections
    pub fn clear_connections(&self) {
        self.connections.write().clear();
    }

    /// Set service mode
    pub fn set_mode(&self, mode: TcpServiceMode) {
        *self.mode.write() = mode;
    }

    /// Shutdown the service
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}

impl Drop for MockTcpService {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.try_send(());
        }
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    connections: Arc<RwLock<Vec<RecordedTcpConnection>>>,
    mode: TcpServiceMode,
) {
    let mut received_data = Vec::new();
    let mut buf = [0u8; 4096];

    match mode {
        TcpServiceMode::Echo => {
            // Echo mode: read and immediately write back
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        received_data.extend_from_slice(&buf[..n]);
                        if let Err(e) = stream.write_all(&buf[..n]).await {
                            tracing::error!("Echo write error: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Echo read error: {}", e);
                        break;
                    }
                }
            }
        }
        TcpServiceMode::FixedResponse(response) => {
            // Read some data first
            match stream.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    received_data.extend_from_slice(&buf[..n]);
                }
                _ => {}
            }

            // Send fixed response
            if let Err(e) = stream.write_all(&response).await {
                tracing::error!("Fixed response write error: {}", e);
            }
        }
        TcpServiceMode::Accumulate => {
            // Just accumulate data
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        received_data.extend_from_slice(&buf[..n]);
                    }
                    Err(e) => {
                        tracing::error!("Accumulate read error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    // Record the connection
    connections.write().push(RecordedTcpConnection {
        received_data,
        peer_addr,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn test_tcp_echo() {
        let service = MockTcpService::start().await;

        let mut stream = TcpStream::connect(service.addr()).await.unwrap();
        stream.write_all(b"Hello, TCP!").await.unwrap();

        let mut buf = [0u8; 32];
        let n = stream.read(&mut buf).await.unwrap();

        assert_eq!(&buf[..n], b"Hello, TCP!");

        // Close and wait for recording
        drop(stream);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(service.connection_count(), 1);
        assert_eq!(service.total_bytes_received(), 11);
    }

    #[tokio::test]
    async fn test_tcp_fixed_response() {
        let service =
            MockTcpService::start_with_mode(TcpServiceMode::FixedResponse(b"PONG".to_vec())).await;

        let mut stream = TcpStream::connect(service.addr()).await.unwrap();
        stream.write_all(b"PING").await.unwrap();

        let mut buf = [0u8; 32];
        let n = stream.read(&mut buf).await.unwrap();

        assert_eq!(&buf[..n], b"PONG");
    }
}
