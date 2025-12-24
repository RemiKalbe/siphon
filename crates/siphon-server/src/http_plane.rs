use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use siphon_protocol::ServerMessage;

use crate::router::Router;
use crate::state::ResponseRegistry;

/// HTTP data plane that receives traffic from Cloudflare
pub struct HttpPlane {
    router: Arc<Router>,
    base_domain: String,
    stream_id_counter: AtomicU64,
    /// Shared registry for pending responses
    response_registry: ResponseRegistry,
}

impl HttpPlane {
    pub fn new(
        router: Arc<Router>,
        base_domain: String,
        response_registry: ResponseRegistry,
    ) -> Arc<Self> {
        Arc::new(Self {
            router,
            base_domain,
            stream_id_counter: AtomicU64::new(1),
            response_registry,
        })
    }

    fn next_stream_id(&self) -> u64 {
        self.stream_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Start listening for HTTP traffic from Cloudflare
    pub async fn run(self: Arc<Self>, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("HTTP plane listening on {}", addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let this = self.clone();

            tokio::spawn(async move {
                let io = TokioIo::new(stream);

                let service = service_fn(move |req| {
                    let this = this.clone();
                    async move { this.handle_request(req).await }
                });

                if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                    tracing::error!("HTTP connection error from {}: {}", peer_addr, e);
                }
            });
        }
    }

    async fn handle_request(
        self: Arc<Self>,
        req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        // Extract subdomain from Host header
        let subdomain = match self.extract_subdomain(&req) {
            Some(s) => s,
            None => {
                tracing::warn!("Request without valid subdomain");
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from("Invalid or missing subdomain")))
                    .unwrap());
            }
        };

        tracing::debug!("Request for subdomain: {}", subdomain);

        // Find the tunnel for this subdomain
        let sender = match self.router.get_sender(&subdomain) {
            Some(s) => s,
            None => {
                tracing::warn!("No tunnel for subdomain: {}", subdomain);
                return Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::from(format!(
                        "Tunnel not found for: {}",
                        subdomain
                    ))))
                    .unwrap());
            }
        };

        // Generate stream ID
        let stream_id = self.next_stream_id();

        // Convert request to protocol message
        let method = req.method().to_string();
        let uri = req.uri().to_string();

        let headers: Vec<(String, String)> = req
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        // Collect body
        let body = match req.into_body().collect().await {
            Ok(collected) => collected.to_bytes().to_vec(),
            Err(e) => {
                tracing::error!("Failed to read request body: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::from("Failed to read request body")))
                    .unwrap());
            }
        };

        // Create response channel
        let (response_tx, response_rx) = oneshot::channel();

        // Register pending response in shared registry
        self.response_registry.insert(stream_id, response_tx);

        // Send request to tunnel
        let msg = ServerMessage::HttpRequest {
            stream_id,
            method,
            uri,
            headers,
            body,
        };

        if let Err(e) = sender.send(msg).await {
            tracing::error!("Failed to send request to tunnel: {}", e);
            // Clean up pending response
            self.response_registry.remove(&stream_id);

            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from("Tunnel connection lost")))
                .unwrap());
        }

        // Wait for response with timeout
        let timeout = Duration::from_secs(30);
        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(response_data)) => {
                // Build HTTP response
                let mut builder = Response::builder().status(response_data.status);

                for (name, value) in response_data.headers {
                    builder = builder.header(name, value);
                }

                Ok(builder
                    .body(Full::new(Bytes::from(response_data.body)))
                    .unwrap())
            }
            Ok(Err(_)) => {
                // Channel closed (tunnel disconnected)
                tracing::error!("Tunnel disconnected while waiting for response");
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Full::new(Bytes::from("Tunnel disconnected")))
                    .unwrap())
            }
            Err(_) => {
                // Timeout
                tracing::error!("Timeout waiting for tunnel response");
                // Clean up pending response
                self.response_registry.remove(&stream_id);

                Ok(Response::builder()
                    .status(StatusCode::GATEWAY_TIMEOUT)
                    .body(Full::new(Bytes::from("Tunnel response timeout")))
                    .unwrap())
            }
        }
    }

    /// Extract subdomain from Host header
    fn extract_subdomain(&self, req: &Request<Incoming>) -> Option<String> {
        let host = req.headers().get("host")?.to_str().ok()?;

        // Remove port if present
        let host = host.split(':').next()?;

        // Check if it ends with our base domain
        if !host.ends_with(&self.base_domain) {
            return None;
        }

        // Extract subdomain
        let subdomain_part = host.strip_suffix(&format!(".{}", self.base_domain))?;

        // Return only the first part (in case of multi-level subdomain)
        Some(subdomain_part.split('.').next()?.to_string())
    }
}
