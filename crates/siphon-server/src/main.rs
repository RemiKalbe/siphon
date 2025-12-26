use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tokio_rustls::TlsAcceptor;
use tracing_subscriber::EnvFilter;

mod cloudflare;
mod config;
mod control_plane;
mod http_plane;
mod router;
mod state;
mod tcp_plane;

use cloudflare::CloudflareClient;
use config::ServerConfig;
use control_plane::ControlPlane;
use http_plane::HttpPlane;
use router::Router;
use state::{new_response_registry, new_tcp_connection_registry, PortAllocator, StreamIdGenerator};
use tcp_plane::TcpPlane;

/// Tunnel server - accepts tunnel connections and routes traffic
#[derive(Parser, Debug)]
#[command(name = "siphon-server")]
#[command(about = "Self-hosted reverse proxy tunnel server")]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "server.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install crypto provider before any TLS operations
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("siphon_server=info".parse()?)
                .add_directive("siphon_common=info".parse()?),
        )
        .init();

    let args = Args::parse();
    tracing::info!("Starting tunnel server with config: {}", args.config);

    // Load and resolve configuration (resolves all secrets)
    let config = ServerConfig::load_and_resolve(&args.config)
        .with_context(|| format!("Failed to load config from {}", args.config))?;

    tracing::info!("Base domain: {}", config.base_domain);
    tracing::info!("Control plane port: {}", config.control_port);
    tracing::info!("HTTP plane port: {}", config.http_port);

    // Load TLS configuration from resolved PEM content
    let tls_config = siphon_common::load_server_config_from_pem(
        &config.cert_pem,
        &config.key_pem,
        &config.ca_cert_pem,
    )
    .context("Failed to load TLS configuration")?;

    let tls_acceptor = TlsAcceptor::from(Arc::new(tls_config));

    // Create shared state
    let router = Router::new();
    let cloudflare = Arc::new(CloudflareClient::new(
        &config.cloudflare,
        &config.base_domain,
    ));
    let response_registry = new_response_registry();
    let tcp_registry = new_tcp_connection_registry();
    let port_allocator = PortAllocator::new(config.tcp_port_range.0, config.tcp_port_range.1);
    let stream_id_gen = StreamIdGenerator::new();

    tracing::info!(
        "TCP port range: {}-{}",
        config.tcp_port_range.0,
        config.tcp_port_range.1
    );

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
        cloudflare.clone(),
        config.base_domain.clone(),
        response_registry.clone(),
        tcp_plane,
        tcp_registry,
    );

    // Load HTTP plane TLS config if provided (for Cloudflare Full Strict mode)
    // Priority: manual certs > auto Origin CA > no TLS
    let http_tls_acceptor =
        if let (Some(cert), Some(key)) = (&config.http_cert_pem, &config.http_key_pem) {
            tracing::info!("HTTP plane TLS: using provided certificates");
            let http_tls_config = siphon_common::load_server_config_no_client_auth(cert, key)
                .context("Failed to load HTTP plane TLS configuration")?;
            Some(TlsAcceptor::from(Arc::new(http_tls_config)))
        } else if config.cloudflare.auto_origin_ca {
            tracing::info!("HTTP plane TLS: generating Cloudflare Origin CA certificate...");

            // Clean up old certificates first
            if let Err(e) = cloudflare.cleanup_old_origin_certificates().await {
                tracing::warn!("Failed to cleanup old Origin CA certificates: {}", e);
            }

            // Generate Origin CA certificate
            let origin_cert = cloudflare
                .create_origin_certificate(365) // 1 year validity
                .await
                .context("Failed to create Origin CA certificate")?;

            tracing::info!(
                "Origin CA certificate created, expires: {}",
                origin_cert.expires_on
            );

            let http_tls_config = siphon_common::load_server_config_no_client_auth(
                &origin_cert.certificate,
                &origin_cert.private_key,
            )
            .context("Failed to load Origin CA TLS configuration")?;
            Some(TlsAcceptor::from(Arc::new(http_tls_config)))
        } else {
            tracing::info!("HTTP plane TLS: disabled (plain HTTP)");
            None
        };

    let http_plane = HttpPlane::new(
        router.clone(),
        config.base_domain.clone(),
        response_registry,
        http_tls_acceptor,
    );

    // Start servers
    let control_addr: SocketAddr = format!("0.0.0.0:{}", config.control_port).parse()?;
    let http_addr: SocketAddr = format!("0.0.0.0:{}", config.http_port).parse()?;

    tracing::info!("Starting control plane on {}", control_addr);
    tracing::info!("Starting HTTP plane on {}", http_addr);

    // Run both planes concurrently with graceful shutdown
    tokio::select! {
        result = control_plane.run(control_addr) => {
            tracing::error!("Control plane stopped: {:?}", result);
        }
        result = http_plane.run(http_addr) => {
            tracing::error!("HTTP plane stopped: {:?}", result);
        }
        _ = shutdown_signal() => {
            tracing::info!("Shutdown signal received, cleaning up...");
        }
    }

    tracing::info!("Server shutdown complete");
    Ok(())
}

/// Wait for shutdown signals (SIGTERM, SIGINT)
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM");
        }
    }
}
