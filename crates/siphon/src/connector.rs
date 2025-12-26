use anyhow::Result;
use bytes::BytesMut;
use siphon_tui::metrics::{MetricsCollector, TunnelInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_util::codec::{Decoder, Encoder};

use siphon_protocol::{ClientMessage, ServerMessage, TunnelCodec, TunnelType};

use crate::forwarder::HttpForwarder;
use crate::tcp_forwarder::TcpForwarder;

/// Manages the connection to the tunnel server
pub struct TunnelConnection {
    tls_stream: TlsStream<TcpStream>,
    local_addr: String,
    metrics: MetricsCollector,
    tunnel_type: TunnelType,
}

impl TunnelConnection {
    pub fn new(
        tls_stream: TlsStream<TcpStream>,
        local_addr: String,
        metrics: MetricsCollector,
        tunnel_type: TunnelType,
    ) -> Self {
        Self {
            tls_stream,
            local_addr,
            metrics,
            tunnel_type,
        }
    }

    /// Request a tunnel from the server
    pub async fn request_tunnel(
        &mut self,
        subdomain: Option<String>,
        tunnel_type: TunnelType,
    ) -> Result<()> {
        // Parse local port from address
        let local_port: u16 = self
            .local_addr
            .split(':')
            .next_back()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let msg = ClientMessage::RequestTunnel {
            subdomain,
            tunnel_type,
            local_port,
        };

        // Encode and send
        let mut codec = TunnelCodec::<ClientMessage>::new();
        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf)?;

        self.tls_stream.write_all(&buf).await?;
        self.tls_stream.flush().await?;

        tracing::debug!("Sent tunnel request");
        Ok(())
    }

    /// Run the tunnel connection, processing messages until disconnection
    pub async fn run(self) -> Result<()> {
        let local_addr = self.local_addr.clone();
        let metrics = self.metrics.clone();
        let tunnel_type = self.tunnel_type.clone();
        let (read_half, write_half) = tokio::io::split(self.tls_stream);

        // Channel for sending responses back to server
        let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<ClientMessage>(32);

        // Spawn write task
        let write_handle = tokio::spawn(async move {
            let mut write_half = write_half;
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

            // Send TLS close_notify for graceful shutdown
            if let Err(e) = write_half.shutdown().await {
                tracing::debug!("TLS shutdown: {}", e);
            }
        });

        // Read loop
        let mut read_half = read_half;
        let mut codec = TunnelCodec::<ServerMessage>::new();
        let mut read_buf = BytesMut::with_capacity(8192);
        let http_forwarder = HttpForwarder::new(local_addr.clone());
        let tcp_forwarder = TcpForwarder::new(local_addr, response_tx.clone());

        loop {
            // Read more data
            match read_half.read_buf(&mut read_buf).await {
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

            // Try to decode messages
            loop {
                match codec.decode(&mut read_buf) {
                    Ok(Some(msg)) => {
                        match msg {
                            ServerMessage::TunnelEstablished {
                                subdomain,
                                url,
                                port,
                            } => {
                                tracing::info!("Tunnel established: {} -> {}", url, http_forwarder.local_addr());
                                if let Some(p) = port {
                                    tracing::debug!("  TCP Port: {}", p);
                                }

                                // Update metrics with tunnel info for TUI
                                metrics.set_tunnel_info(TunnelInfo {
                                    subdomain: subdomain.clone(),
                                    url: url.clone(),
                                    port,
                                    tunnel_type: tunnel_type.clone(),
                                });
                            }
                            ServerMessage::TunnelDenied { reason } => {
                                tracing::error!("Tunnel denied: {}", reason);
                                anyhow::bail!("Tunnel denied: {}", reason);
                            }
                            ServerMessage::HttpRequest {
                                stream_id,
                                method,
                                uri,
                                headers,
                                body,
                            } => {
                                tracing::debug!("HTTP request {}: {} {}", stream_id, method, uri);

                                // Forward request to local service
                                let tx = response_tx.clone();
                                let fwd = http_forwarder.clone();

                                tokio::spawn(async move {
                                    match fwd.forward_http(method, uri, headers, body).await {
                                        Ok((status, resp_headers, resp_body)) => {
                                            let msg = ClientMessage::HttpResponse {
                                                stream_id,
                                                status,
                                                headers: resp_headers,
                                                body: resp_body,
                                            };
                                            let _ = tx.send(msg).await;
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to forward request {}: {}",
                                                stream_id,
                                                e
                                            );
                                            // Send error response
                                            let msg = ClientMessage::HttpResponse {
                                                stream_id,
                                                status: 502,
                                                headers: vec![],
                                                body: format!("Forwarding error: {}", e)
                                                    .into_bytes(),
                                            };
                                            let _ = tx.send(msg).await;
                                        }
                                    }
                                });
                            }
                            ServerMessage::TcpConnect { stream_id } => {
                                tracing::debug!("TCP connect: {}", stream_id);
                                tcp_forwarder.handle_connect(stream_id).await;
                            }
                            ServerMessage::TcpData { stream_id, data } => {
                                tracing::debug!("TCP data {}: {} bytes", stream_id, data.len());
                                tcp_forwarder.handle_data(stream_id, data).await;
                            }
                            ServerMessage::TcpClose { stream_id } => {
                                tracing::debug!("TCP close: {}", stream_id);
                                tcp_forwarder.handle_close(stream_id);
                            }
                            ServerMessage::Pong { timestamp } => {
                                tracing::debug!("Pong: {}", timestamp);
                            }
                        }
                    }
                    Ok(None) => break, // Need more data
                    Err(e) => {
                        tracing::error!("Decode error: {}", e);
                        anyhow::bail!("Protocol decode error: {}", e);
                    }
                }
            }
        }

        // Drop the sender to signal the write task to shutdown gracefully
        drop(response_tx);

        // Wait for the write task to complete (sends TLS close_notify)
        let _ = write_handle.await;

        Ok(())
    }
}
