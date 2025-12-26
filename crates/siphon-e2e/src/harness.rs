//! Test server harness for E2E tests
//!
//! This module provides a test harness that starts a complete siphon server
//! with mocked DNS for testing purposes.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_rustls::TlsAcceptor;

use siphon_server::{
    new_response_registry, new_tcp_connection_registry, ControlPlane, HttpPlane, PortAllocator,
    Router, StreamIdGenerator, TcpPlane,
};

use crate::certificates::TestCertificates;
use crate::mock_dns::MockDnsProvider;

/// Global counter for allocating unique port ranges to each test server
static PORT_RANGE_COUNTER: AtomicU16 = AtomicU16::new(0);

/// Number of ports per test server
const PORTS_PER_SERVER: u16 = 10;

/// Base port for TCP plane allocations
const BASE_TCP_PORT: u16 = 51000;

/// A running test server instance
pub struct TestServer {
    /// Control plane address (mTLS)
    pub control_addr: SocketAddr,
    /// HTTP plane address
    pub http_addr: SocketAddr,
    /// Base domain for the test server
    pub base_domain: String,
    /// Mock DNS provider for assertions
    pub dns_provider: Arc<MockDnsProvider>,
    /// Certificate set used
    pub certs: Arc<TestCertificates>,
    /// Shutdown signal sender
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl TestServer {
    /// Start a test server with mock DNS and generated certificates
    pub async fn start() -> Self {
        let certs = Arc::new(TestCertificates::generate());
        let base_domain = "test.example.com".to_string();

        // Build TLS config for control plane (with client auth)
        let tls_config = siphon_common::load_server_config_from_pem(
            &certs.server_cert_pem,
            &certs.server_key_pem,
            &certs.ca_cert_pem,
        )
        .expect("Failed to load server TLS config");

        let tls_acceptor = TlsAcceptor::from(Arc::new(tls_config));

        // Create shared state
        let router = Router::new();
        let dns_provider = MockDnsProvider::new();
        let response_registry = new_response_registry();
        let tcp_registry = new_tcp_connection_registry();

        // Allocate a unique port range for this test server
        let range_index = PORT_RANGE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let start_port = BASE_TCP_PORT + (range_index * PORTS_PER_SERVER);
        let end_port = start_port + PORTS_PER_SERVER;
        let port_allocator = PortAllocator::new(start_port, end_port);

        let stream_id_gen = StreamIdGenerator::new();

        // Create planes
        let tcp_plane = TcpPlane::new(
            router.clone(),
            port_allocator,
            tcp_registry.clone(),
            stream_id_gen,
        );

        let control_plane = ControlPlane::new(
            router.clone(),
            tls_acceptor,
            dns_provider.clone(),
            base_domain.clone(),
            response_registry.clone(),
            tcp_plane,
            tcp_registry,
        );

        // HTTP plane without TLS for simplicity in tests
        let http_plane = HttpPlane::new(
            router.clone(),
            base_domain.clone(),
            response_registry,
            None, // No TLS for HTTP plane in tests
        );

        // Bind to ephemeral ports
        let control_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind control plane");
        let http_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind HTTP plane");

        let control_addr = control_listener.local_addr().unwrap();
        let http_addr = http_listener.local_addr().unwrap();

        // Shutdown channel
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        // Spawn control plane
        let control_plane_clone = control_plane.clone();
        tokio::spawn(async move {
            tokio::select! {
                result = control_plane_clone.run_with_listener(control_listener) => {
                    if let Err(e) = result {
                        tracing::error!("Control plane error: {}", e);
                    }
                }
                _ = &mut shutdown_rx => {
                    tracing::debug!("Control plane shutting down");
                }
            }
        });

        // Spawn HTTP plane
        let http_plane_clone = http_plane.clone();
        tokio::spawn(async move {
            if let Err(e) = http_plane_clone.run_with_listener(http_listener).await {
                tracing::error!("HTTP plane error: {}", e);
            }
        });

        // Give the servers a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        Self {
            control_addr,
            http_addr,
            base_domain,
            dns_provider,
            certs,
            shutdown_tx: Some(shutdown_tx),
        }
    }

    /// Get the client TLS config for connecting to this server
    pub fn client_tls_config(&self) -> rustls::ClientConfig {
        siphon_common::load_client_config_from_pem(
            &self.certs.client_cert_pem,
            &self.certs.client_key_pem,
            &self.certs.ca_cert_pem,
        )
        .expect("Failed to load client TLS config")
    }

    /// Get the full URL for a subdomain (e.g., "https://myapp.test.example.com")
    pub fn url_for(&self, subdomain: &str) -> String {
        format!("https://{}.{}", subdomain, self.base_domain)
    }

    /// Get the Host header value for a subdomain
    pub fn host_for(&self, subdomain: &str) -> String {
        format!("{}.{}", subdomain, self.base_domain)
    }

    /// Shutdown the test server
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}
