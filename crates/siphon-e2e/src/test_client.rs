//! Test client for E2E tests
//!
//! A simplified tunnel client that speaks the protocol without TUI dependencies.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bytes::BytesMut;
use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tokio_util::codec::{Decoder, Encoder};

use siphon_protocol::{ClientMessage, ServerMessage, TunnelCodec, TunnelType};

use crate::harness::TestServer;

/// A test tunnel client
pub struct TestClient {
    /// Handle to the spawned client task
    _handle: tokio::task::JoinHandle<Result<()>>,
    /// Sender to signal shutdown
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// The established subdomain (if tunnel was established)
    pub subdomain: Option<String>,
    /// The established URL
    pub url: Option<String>,
    /// The allocated TCP port (for TCP tunnels)
    pub tcp_port: Option<u16>,
}

impl TestClient {
    /// Connect to the test server and establish a tunnel
    pub async fn connect(
        server: &TestServer,
        local_addr: &str,
        subdomain: Option<String>,
        tunnel_type: TunnelType,
    ) -> Result<Self> {
        let tls_config = server.client_tls_config();
        let connector = TlsConnector::from(Arc::new(tls_config));

        // Connect to control plane
        let tcp_stream = TcpStream::connect(server.control_addr).await?;
        let server_name = "localhost".try_into()?;
        let tls_stream = connector.connect(server_name, tcp_stream).await?;

        let (subdomain_result, url_result, tcp_port, handle, shutdown_tx) =
            run_client(tls_stream, local_addr.to_string(), subdomain, tunnel_type).await?;

        Ok(Self {
            _handle: handle,
            shutdown_tx: Some(shutdown_tx),
            subdomain: subdomain_result,
            url: url_result,
            tcp_port,
        })
    }

    /// Shutdown the client
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            // Best effort shutdown
            let _ = tx.try_send(());
        }
    }
}

/// Run the client, returning tunnel info once established
async fn run_client(
    tls_stream: TlsStream<TcpStream>,
    local_addr: String,
    subdomain: Option<String>,
    tunnel_type: TunnelType,
) -> Result<(
    Option<String>,
    Option<String>,
    Option<u16>,
    tokio::task::JoinHandle<Result<()>>,
    mpsc::Sender<()>,
)> {
    let (mut read_half, mut write_half) = tokio::io::split(tls_stream);

    // Parse local port
    let local_port: u16 = local_addr
        .split(':')
        .next_back()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Send tunnel request
    let msg = ClientMessage::RequestTunnel {
        subdomain,
        tunnel_type,
        local_port,
    };

    let mut codec = TunnelCodec::<ClientMessage>::new();
    let mut buf = BytesMut::new();
    codec.encode(msg, &mut buf)?;
    write_half.write_all(&buf).await?;
    write_half.flush().await?;

    // Read response
    let mut read_codec = TunnelCodec::<ServerMessage>::new();
    let mut read_buf = BytesMut::with_capacity(8192);

    // Wait for tunnel established message
    let (subdomain_result, url_result, tcp_port) = loop {
        match read_half.read_buf(&mut read_buf).await {
            Ok(0) => anyhow::bail!("Server disconnected before tunnel established"),
            Ok(_) => {}
            Err(e) => anyhow::bail!("Read error: {}", e),
        }

        if let Some(msg) = read_codec.decode(&mut read_buf)? {
            match msg {
                ServerMessage::TunnelEstablished {
                    subdomain,
                    url,
                    port,
                } => {
                    tracing::debug!("Tunnel established: {} -> {}", url, local_addr);
                    break (Some(subdomain), Some(url), port);
                }
                ServerMessage::TunnelDenied { reason } => {
                    anyhow::bail!("Tunnel denied: {}", reason);
                }
                _ => {
                    // Unexpected message before tunnel established
                    tracing::warn!("Unexpected message before tunnel established: {:?}", msg);
                }
            }
        }
    };

    // Create channels for communication
    let (response_tx, mut response_rx) = mpsc::channel::<ClientMessage>(32);
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // TCP connection state - maps stream_id to writer channel
    let tcp_connections: Arc<RwLock<HashMap<u64, mpsc::Sender<Vec<u8>>>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // Spawn the main client loop
    let tcp_conns = tcp_connections.clone();
    let handle = tokio::spawn(async move {
        let http_client = reqwest::Client::new();
        let local_addr = local_addr.clone();

        // Spawn write task
        let write_handle = tokio::spawn(async move {
            let mut codec = TunnelCodec::<ClientMessage>::new();
            let mut write_buf = BytesMut::with_capacity(8192);

            while let Some(msg) = response_rx.recv().await {
                write_buf.clear();
                if let Err(e) = codec.encode(msg, &mut write_buf) {
                    tracing::error!("Failed to encode message: {}", e);
                    break;
                }
                if let Err(e) = write_half.write_all(&write_buf).await {
                    tracing::error!("Failed to write message: {}", e);
                    break;
                }
                if let Err(e) = write_half.flush().await {
                    tracing::error!("Failed to flush: {}", e);
                    break;
                }
            }

            if let Err(e) = write_half.shutdown().await {
                tracing::debug!("TLS shutdown: {}", e);
            }
        });

        // Read loop
        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::debug!("Client shutdown requested");
                    break;
                }
                result = read_half.read_buf(&mut read_buf) => {
                    match result {
                        Ok(0) => {
                            tracing::info!("Server disconnected");
                            break;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!("Read error: {}", e);
                            break;
                        }
                    }

                    // Process messages
                    loop {
                        match read_codec.decode(&mut read_buf) {
                            Ok(Some(msg)) => {
                                handle_message(msg, &http_client, &local_addr, &response_tx, &tcp_conns).await;
                            }
                            Ok(None) => break,
                            Err(e) => {
                                tracing::error!("Decode error: {}", e);
                                return Err(anyhow::anyhow!("Protocol decode error: {}", e));
                            }
                        }
                    }
                }
            }
        }

        drop(response_tx);
        let _ = write_handle.await;
        Ok(())
    });

    Ok((subdomain_result, url_result, tcp_port, handle, shutdown_tx))
}

/// Handle a server message
async fn handle_message(
    msg: ServerMessage,
    http_client: &reqwest::Client,
    local_addr: &str,
    response_tx: &mpsc::Sender<ClientMessage>,
    tcp_connections: &Arc<RwLock<HashMap<u64, mpsc::Sender<Vec<u8>>>>>,
) {
    match msg {
        ServerMessage::HttpRequest {
            stream_id,
            method,
            uri,
            headers,
            body,
        } => {
            tracing::debug!("HTTP request {}: {} {}", stream_id, method, uri);

            // Forward to local service
            let url = format!("http://{}{}", local_addr, uri);
            let method = match method.as_str() {
                "GET" => reqwest::Method::GET,
                "POST" => reqwest::Method::POST,
                "PUT" => reqwest::Method::PUT,
                "DELETE" => reqwest::Method::DELETE,
                "PATCH" => reqwest::Method::PATCH,
                "HEAD" => reqwest::Method::HEAD,
                "OPTIONS" => reqwest::Method::OPTIONS,
                _ => reqwest::Method::GET,
            };

            let mut req = http_client.request(method, &url);

            // Add headers
            for (name, value) in headers {
                if name.to_lowercase() != "host" {
                    req = req.header(&name, &value);
                }
            }

            // Add body
            if !body.is_empty() {
                req = req.body(body);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let resp_headers: Vec<(String, String)> = resp
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect();
                    let resp_body = resp.bytes().await.unwrap_or_default().to_vec();

                    let msg = ClientMessage::HttpResponse {
                        stream_id,
                        status,
                        headers: resp_headers,
                        body: resp_body,
                    };
                    let _ = response_tx.send(msg).await;
                }
                Err(e) => {
                    tracing::warn!("Failed to forward request: {}", e);
                    let msg = ClientMessage::HttpResponse {
                        stream_id,
                        status: 502,
                        headers: vec![],
                        body: format!("Forwarding error: {}", e).into_bytes(),
                    };
                    let _ = response_tx.send(msg).await;
                }
            }
        }
        ServerMessage::TcpConnect { stream_id } => {
            tracing::debug!("TCP connect {}", stream_id);

            // Connect to local service
            let local_addr = local_addr.to_string();
            let response_tx = response_tx.clone();
            let tcp_connections = tcp_connections.clone();

            tokio::spawn(async move {
                match TcpStream::connect(&local_addr).await {
                    Ok(stream) => {
                        let (mut read_half, mut write_half) = stream.into_split();

                        // Channel for writing data to local service
                        let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(32);

                        // Register the connection
                        tcp_connections.write().insert(stream_id, write_tx);

                        // Spawn write task (receives data from tunnel, writes to local)
                        let tcp_conns = tcp_connections.clone();
                        tokio::spawn(async move {
                            while let Some(data) = write_rx.recv().await {
                                if let Err(e) = write_half.write_all(&data).await {
                                    tracing::error!(
                                        "Failed to write to local TCP {}: {}",
                                        stream_id,
                                        e
                                    );
                                    break;
                                }
                            }
                            tcp_conns.write().remove(&stream_id);
                        });

                        // Read from local, send to tunnel
                        let mut buf = vec![0u8; 8192];
                        loop {
                            match read_half.read(&mut buf).await {
                                Ok(0) => {
                                    tracing::debug!("Local TCP {} closed", stream_id);
                                    break;
                                }
                                Ok(n) => {
                                    let data = buf[..n].to_vec();
                                    let msg = ClientMessage::TcpData { stream_id, data };
                                    if response_tx.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Local TCP read error {}: {}", stream_id, e);
                                    break;
                                }
                            }
                        }

                        // Send close
                        let _ = response_tx
                            .send(ClientMessage::TcpClose { stream_id })
                            .await;
                        tcp_connections.write().remove(&stream_id);
                    }
                    Err(e) => {
                        tracing::error!("Failed to connect to local service: {}", e);
                        // Send close to indicate connection failed
                        let _ = response_tx
                            .send(ClientMessage::TcpClose { stream_id })
                            .await;
                    }
                }
            });
        }
        ServerMessage::TcpData { stream_id, data } => {
            tracing::debug!("TCP data {}: {} bytes", stream_id, data.len());

            // Forward data to local connection
            // Clone the sender to avoid holding the lock across await
            let writer = tcp_connections.read().get(&stream_id).cloned();
            if let Some(writer) = writer {
                let _ = writer.send(data).await;
            }
        }
        ServerMessage::TcpClose { stream_id } => {
            tracing::debug!("TCP close {}", stream_id);
            tcp_connections.write().remove(&stream_id);
        }
        ServerMessage::Pong { .. } => {}
        ServerMessage::TunnelEstablished { .. } | ServerMessage::TunnelDenied { .. } => {
            // These should only come once at the start
        }
    }
}
