use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use siphon_secrets::{SecretResolver, SecretUri};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;
use tracing_subscriber::EnvFilter;

use siphon_tui::{MetricsCollector, SetupWizard, SiphonConfig, TuiApp};

mod connector;
mod forwarder;
mod tcp_forwarder;

use connector::TunnelConnection;
use siphon_protocol::TunnelType;

/// Siphon - Secure tunnel client for exposing local services
#[derive(Parser, Debug)]
#[command(name = "siphon")]
#[command(about = "Expose local services securely through a tunnel")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Tunnel server address (host:port)
    #[arg(short, long)]
    server: Option<String>,

    /// Local address to forward to (e.g., 127.0.0.1:3000)
    #[arg(short, long)]
    local: Option<String>,

    /// Requested subdomain (optional, auto-generated if not specified)
    #[arg(long)]
    subdomain: Option<String>,

    /// Client certificate (file path, keychain://, op://, env://)
    #[arg(long)]
    cert: Option<String>,

    /// Client private key (file path, keychain://, op://, env://)
    #[arg(long)]
    key: Option<String>,

    /// CA certificate (file path, keychain://, op://, env://)
    #[arg(long)]
    ca: Option<String>,

    /// Tunnel type: http or tcp
    #[arg(long)]
    tunnel_type: Option<String>,

    /// Disable TUI dashboard (run in CLI mode)
    #[arg(long)]
    no_tui: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run interactive setup wizard
    Setup,

    /// Encode a file as base64 for use in config
    Encode {
        /// Path to the file to encode (certificate, key, etc.)
        file: String,
    },
}

/// Resolved configuration from CLI args and/or config file
struct ResolvedConfig {
    server_addr: String,
    local_addr: String,
    subdomain: Option<String>,
    tunnel_type: TunnelType,
    cert: String,
    key: String,
    ca: String,
}

impl ResolvedConfig {
    /// Resolve configuration from CLI args, falling back to config file for connection settings
    fn resolve(cli: &Cli) -> Result<Self> {
        // Try to load config file for connection settings
        let config_file = SiphonConfig::load_default().ok();

        // Server address (from CLI or config)
        let server_addr = cli
            .server
            .clone()
            .or_else(|| config_file.as_ref().map(|c| c.server_addr.clone()))
            .context("Server address required. Use --server or run 'siphon setup'")?;

        // Local address (CLI only - required at runtime)
        let local_addr = cli
            .local
            .clone()
            .context("Local address required. Use --local (e.g., --local 127.0.0.1:3000)")?;

        // Subdomain (CLI only - optional)
        let subdomain = cli.subdomain.clone();

        // Tunnel type (CLI only - defaults to http)
        let tunnel_type_str = cli
            .tunnel_type
            .clone()
            .unwrap_or_else(|| "http".to_string());

        let tunnel_type = match tunnel_type_str.as_str() {
            "http" => TunnelType::Http,
            "tcp" => TunnelType::Tcp,
            _ => anyhow::bail!(
                "Invalid tunnel type: {}. Use 'http' or 'tcp'",
                tunnel_type_str
            ),
        };

        // Certificates (from CLI or config)
        let cert = cli
            .cert
            .clone()
            .or_else(|| config_file.as_ref().map(|c| c.cert.clone()))
            .context("Certificate required. Use --cert or run 'siphon setup'")?;

        let key = cli
            .key
            .clone()
            .or_else(|| config_file.as_ref().map(|c| c.key.clone()))
            .context("Private key required. Use --key or run 'siphon setup'")?;

        let ca = cli
            .ca
            .clone()
            .or_else(|| config_file.as_ref().map(|c| c.ca_cert.clone()))
            .context("CA certificate required. Use --ca or run 'siphon setup'")?;

        Ok(Self {
            server_addr,
            local_addr,
            subdomain,
            tunnel_type,
            cert,
            key,
            ca,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls crypto provider before any TLS operations
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cli = Cli::parse();

    // Handle subcommands
    match &cli.command {
        Some(Commands::Setup) => return run_setup(),
        Some(Commands::Encode { file }) => return run_encode(file),
        None => {}
    }

    // Resolve configuration
    let config = match ResolvedConfig::resolve(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            eprintln!(
                "\nRun 'siphon setup' to configure interactively, or provide all required options."
            );
            std::process::exit(1);
        }
    };

    // Initialize logging (only in no-tui mode, TUI has its own display)
    if cli.no_tui {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::from_default_env()
                    .add_directive("siphon=info".parse()?)
                    .add_directive("siphon_common=info".parse()?),
            )
            .init();
    }

    // Resolve secrets
    let resolver = SecretResolver::new();

    let cert_uri: SecretUri = config.cert.parse().context("Invalid cert URI")?;
    let key_uri: SecretUri = config.key.parse().context("Invalid key URI")?;
    let ca_uri: SecretUri = config.ca.parse().context("Invalid CA URI")?;

    if cli.no_tui {
        tracing::info!("Resolving secrets...");
    }

    let cert_pem = resolver
        .resolve_trimmed(&cert_uri)
        .map_err(|e| anyhow::anyhow!("Failed to resolve certificate: {}", e))?;
    let key_pem = resolver
        .resolve_trimmed(&key_uri)
        .map_err(|e| anyhow::anyhow!("Failed to resolve private key: {}", e))?;
    let ca_pem = resolver
        .resolve_trimmed(&ca_uri)
        .map_err(|e| anyhow::anyhow!("Failed to resolve CA certificate: {}", e))?;

    if cli.no_tui {
        tracing::info!("Secrets resolved successfully");
    }

    // Load TLS configuration
    let tls_config = siphon_common::load_client_config_from_pem(&cert_pem, &key_pem, &ca_pem)
        .context("Failed to load TLS configuration")?;

    let tls_connector = TlsConnector::from(Arc::new(tls_config));

    // Extract server hostname for TLS
    let server_host = config
        .server_addr
        .split(':')
        .next()
        .context("Invalid server address")?;

    let server_name = ServerName::try_from(server_host.to_string())
        .map_err(|_| anyhow::anyhow!("Invalid server hostname: {}", server_host))?;

    // Create metrics collector
    let metrics = MetricsCollector::new();

    if cli.no_tui {
        // CLI mode - run tunnel without TUI
        run_cli_mode(
            config.server_addr,
            config.local_addr,
            config.subdomain,
            config.tunnel_type,
            tls_connector,
            server_name,
            metrics,
        )
        .await
    } else {
        // TUI mode - run dashboard alongside tunnel
        run_tui_mode(
            config.server_addr,
            config.local_addr,
            config.subdomain,
            config.tunnel_type,
            tls_connector,
            server_name,
            metrics,
        )
        .await
    }
}

fn run_setup() -> Result<()> {
    let mut wizard = SetupWizard::new();

    match wizard.run()? {
        Some(_config) => {
            println!("\nSetup complete! Run 'siphon' to start the tunnel.");
            Ok(())
        }
        None => {
            println!("\nSetup cancelled.");
            Ok(())
        }
    }
}

fn run_encode(file_path: &str) -> Result<()> {
    use base64::Engine;

    let content =
        std::fs::read(file_path).with_context(|| format!("Failed to read file: {}", file_path))?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(&content);

    println!("base64://{}", encoded);

    Ok(())
}

async fn run_cli_mode(
    server_addr: String,
    local_addr: String,
    subdomain: Option<String>,
    tunnel_type: TunnelType,
    tls_connector: TlsConnector,
    server_name: ServerName<'static>,
    metrics: MetricsCollector,
) -> Result<()> {
    tracing::info!("Connecting to {} to expose {}", server_addr, local_addr);

    // Reconnection loop
    let mut shutdown = false;
    loop {
        if shutdown {
            break;
        }

        tracing::info!("Connecting to {}...", server_addr);

        tokio::select! {
            result = run_tunnel(
                &server_addr,
                &local_addr,
                subdomain.clone(),
                tunnel_type.clone(),
                tls_connector.clone(),
                server_name.clone(),
                metrics.clone(),
            ) => {
                match result {
                    Ok(_) => {
                        tracing::info!("Tunnel closed normally");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Tunnel error: {}", e);
                        tracing::info!("Reconnecting in 5 seconds...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
            _ = shutdown_signal() => {
                tracing::info!("Shutdown signal received");
                shutdown = true;
            }
        }
    }

    tracing::info!("Client shutdown complete");
    Ok(())
}

async fn run_tui_mode(
    server_addr: String,
    local_addr: String,
    subdomain: Option<String>,
    tunnel_type: TunnelType,
    tls_connector: TlsConnector,
    server_name: ServerName<'static>,
    metrics: MetricsCollector,
) -> Result<()> {
    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Clone metrics for TUI
    let tui_metrics = metrics.clone();

    // Spawn TUI in its own task
    let tui_handle = tokio::spawn(async move {
        let app = TuiApp::new(tui_metrics, shutdown_tx);
        app.run().await
    });

    // Reconnection loop with TUI
    loop {
        tokio::select! {
            result = run_tunnel(
                &server_addr,
                &local_addr,
                subdomain.clone(),
                tunnel_type.clone(),
                tls_connector.clone(),
                server_name.clone(),
                metrics.clone(),
            ) => {
                match result {
                    Ok(_) => {
                        // Tunnel closed normally
                        break;
                    }
                    Err(e) => {
                        metrics.record_error(format!("Tunnel error: {}", e));
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                // TUI requested shutdown
                break;
            }
            _ = shutdown_signal() => {
                // OS signal received
                break;
            }
        }
    }

    // Wait for TUI to finish
    let _ = tui_handle.await;

    Ok(())
}

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
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

async fn run_tunnel(
    server_addr: &str,
    local_addr: &str,
    subdomain: Option<String>,
    tunnel_type: TunnelType,
    tls_connector: TlsConnector,
    server_name: ServerName<'static>,
    _metrics: MetricsCollector,
) -> Result<()> {
    // Connect to server
    let stream = TcpStream::connect(server_addr).await?;

    // Perform TLS handshake
    let tls_stream = tls_connector.connect(server_name, stream).await?;

    // Create tunnel connection handler
    let mut connection = TunnelConnection::new(tls_stream, local_addr.to_string());

    // Request tunnel
    connection.request_tunnel(subdomain, tunnel_type).await?;

    // Run the connection (processes messages until disconnection)
    connection.run().await
}
