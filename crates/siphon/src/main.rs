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
                        if let Some(tls_diagnostic) = analyze_tls_error(&e) {
                            display_tls_error(tls_diagnostic.as_ref());
                            return Err(e);
                        }
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
                        if let Some(tls_diagnostic) = analyze_tls_error(&e) {
                            metrics.record_error(format!("Fatal: {}", tls_diagnostic));
                            // Give TUI a moment to display the error, then exit
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            display_tls_error(tls_diagnostic.as_ref());
                            break;
                        }
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

/// SAN mismatch diagnostic with detailed information
#[derive(Debug, miette::Diagnostic)]
#[diagnostic(
    code(siphon::tls::san_mismatch),
    severity(error),
    url("https://github.com/remikalbe/siphon#certificate-setup")
)]
struct SanMismatchDiagnostic {
    expected: String,
    presented: Vec<String>,

    #[help]
    help: String,
}

impl std::fmt::Display for SanMismatchDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Certificate hostname mismatch")?;
        writeln!(f)?;
        writeln!(f, "  Expected hostname: {}", self.expected)?;
        writeln!(f, "  Certificate is valid for:")?;
        if self.presented.is_empty() {
            writeln!(f, "    (no SANs found in certificate)")?;
        } else {
            for name in &self.presented {
                writeln!(f, "    - {}", name)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for SanMismatchDiagnostic {}

/// Certificate expired diagnostic
#[derive(Debug, miette::Diagnostic, thiserror::Error)]
#[error("Certificate has expired")]
#[diagnostic(code(siphon::tls::expired), severity(error))]
struct ExpiredCertDiagnostic {
    #[help]
    help: String,
}

/// Unknown issuer diagnostic
#[derive(Debug, miette::Diagnostic, thiserror::Error)]
#[error("Certificate issuer not trusted")]
#[diagnostic(code(siphon::tls::unknown_issuer), severity(error))]
struct UnknownIssuerDiagnostic {
    #[help]
    help: String,
}

/// Generic TLS diagnostic for other errors
#[derive(Debug, miette::Diagnostic, thiserror::Error)]
#[error("{message}")]
#[diagnostic(code(siphon::tls::error), severity(error))]
struct GenericTlsDiagnostic {
    message: String,
    #[help]
    help: String,
}

/// Analyze an error and extract detailed TLS/certificate information if applicable
fn analyze_tls_error(error: &anyhow::Error) -> Option<Box<dyn miette::Diagnostic + Send + Sync>> {
    // Check the error chain for rustls errors
    for cause in error.chain() {
        if let Some(rustls_err) = cause.downcast_ref::<rustls::Error>() {
            return Some(analyze_rustls_error(rustls_err));
        }
    }

    // Fallback: check error string for TLS-related patterns
    let error_debug = format!("{:?}", error);
    let error_display = format!("{}", error);

    if error_debug.contains("InvalidCertificate")
        || error_debug.contains("CertificateError")
        || error_debug.contains("AlertReceived")
        || error_debug.contains("HandshakeFailure")
    {
        return Some(Box::new(GenericTlsDiagnostic {
            message: format!("TLS handshake failed: {}", error_display),
            help: "Check that your certificate matches the server's expectations.".to_string(),
        }));
    }

    None
}

/// Extract detailed information from a rustls::Error
fn analyze_rustls_error(err: &rustls::Error) -> Box<dyn miette::Diagnostic + Send + Sync> {
    use rustls::Error;

    match err {
        Error::InvalidCertificate(cert_err) => analyze_certificate_error(cert_err),
        Error::NoCertificatesPresented => Box::new(GenericTlsDiagnostic {
            message: "No client certificate was presented".to_string(),
            help: "Ensure your certificate file path is correct and the file exists.".to_string(),
        }),
        Error::AlertReceived(alert) => Box::new(GenericTlsDiagnostic {
            message: format!("Server rejected connection with TLS alert: {:?}", alert),
            help: "The server doesn't trust your certificate. Check that it was signed by the correct CA.".to_string(),
        }),
        Error::InvalidCertRevocationList(crl_err) => Box::new(GenericTlsDiagnostic {
            message: format!("Invalid certificate revocation list: {:?}", crl_err),
            help: "The CRL file is malformed or corrupted.".to_string(),
        }),
        Error::DecryptError => Box::new(GenericTlsDiagnostic {
            message: "TLS decryption failed".to_string(),
            help: "The TLS session was corrupted. This may indicate a network issue or misconfigured proxy.".to_string(),
        }),
        Error::EncryptError => Box::new(GenericTlsDiagnostic {
            message: "TLS encryption failed".to_string(),
            help: "Failed to encrypt TLS message. This may indicate a configuration issue.".to_string(),
        }),
        Error::PeerIncompatible(reason) => Box::new(GenericTlsDiagnostic {
            message: format!("Server is incompatible: {:?}", reason),
            help: "The server doesn't support the required TLS version or features.".to_string(),
        }),
        Error::PeerMisbehaved(reason) => Box::new(GenericTlsDiagnostic {
            message: format!("Server protocol violation: {:?}", reason),
            help: "The server sent invalid TLS data. This may indicate a misconfigured server or MITM attack.".to_string(),
        }),
        Error::InvalidMessage(reason) => Box::new(GenericTlsDiagnostic {
            message: format!("Invalid TLS message: {:?}", reason),
            help: "The server sent malformed TLS data.".to_string(),
        }),
        Error::UnsupportedNameType => Box::new(GenericTlsDiagnostic {
            message: "Unsupported server name type".to_string(),
            help: "The server name format is not supported. Use a DNS hostname.".to_string(),
        }),
        Error::FailedToGetCurrentTime => Box::new(GenericTlsDiagnostic {
            message: "Failed to get system time".to_string(),
            help: "Certificate validation requires accurate system time. Check your system clock.".to_string(),
        }),
        Error::FailedToGetRandomBytes => Box::new(GenericTlsDiagnostic {
            message: "Failed to generate random bytes".to_string(),
            help: "System random number generator failed. This is a system-level issue.".to_string(),
        }),
        Error::General(msg) => Box::new(GenericTlsDiagnostic {
            message: format!("TLS error: {}", msg),
            help: "An unexpected TLS error occurred.".to_string(),
        }),
        _ => Box::new(GenericTlsDiagnostic {
            message: format!("TLS error: {}", err),
            help: "Check your TLS configuration and certificates.".to_string(),
        }),
    }
}

/// Extract detailed information from a CertificateError
fn analyze_certificate_error(
    err: &rustls::CertificateError,
) -> Box<dyn miette::Diagnostic + Send + Sync> {
    use rustls::CertificateError;

    match err {
        CertificateError::NotValidForNameContext { expected, presented } => {
            use rustls::pki_types::ServerName;

            let expected_str = match expected {
                ServerName::DnsName(name) => name.as_ref().to_string(),
                ServerName::IpAddress(ip) => format!("{:?}", ip),
                _ => format!("{:?}", expected),
            };

            Box::new(SanMismatchDiagnostic {
                expected: expected_str,
                presented: presented.iter().map(|s| s.to_string()).collect(),
                help: "Regenerate your certificate with a SAN that includes the server hostname."
                    .to_string(),
            })
        }
        CertificateError::NotValidForName => Box::new(GenericTlsDiagnostic {
            message: "Certificate hostname mismatch".to_string(),
            help: "The certificate's Subject Alternative Names (SANs) must include the server hostname.".to_string(),
        }),
        CertificateError::ExpiredContext { time, not_after } => Box::new(ExpiredCertDiagnostic {
            help: format!(
                "Certificate expired at {:?} (current time: {:?}). Renew the certificate.",
                not_after, time
            ),
        }),
        CertificateError::Expired => Box::new(ExpiredCertDiagnostic {
            help: "Renew the certificate to fix this issue.".to_string(),
        }),
        CertificateError::NotValidYetContext { time, not_before } => {
            Box::new(GenericTlsDiagnostic {
                message: "Certificate is not yet valid".to_string(),
                help: format!(
                    "Certificate valid from {:?} (current time: {:?}). Check your system clock.",
                    not_before, time
                ),
            })
        }
        CertificateError::NotValidYet => Box::new(GenericTlsDiagnostic {
            message: "Certificate is not yet valid".to_string(),
            help: "The certificate's notBefore date is in the future. Check your system clock."
                .to_string(),
        }),
        CertificateError::Revoked => Box::new(GenericTlsDiagnostic {
            message: "Certificate has been revoked".to_string(),
            help: "This certificate has been revoked and cannot be used. Generate a new certificate.".to_string(),
        }),
        CertificateError::UnknownIssuer => Box::new(UnknownIssuerDiagnostic {
            help: "The certificate was not signed by a trusted CA. Ensure you're using the correct CA certificate with --ca.".to_string(),
        }),
        CertificateError::BadSignature => Box::new(GenericTlsDiagnostic {
            message: "Certificate signature is invalid".to_string(),
            help: "The certificate may be corrupted or was not signed by the expected CA."
                .to_string(),
        }),
        CertificateError::BadEncoding => Box::new(GenericTlsDiagnostic {
            message: "Certificate encoding is invalid".to_string(),
            help: "Ensure the certificate file is valid PEM format.".to_string(),
        }),
        CertificateError::UnhandledCriticalExtension => Box::new(GenericTlsDiagnostic {
            message: "Certificate has unhandled critical extension".to_string(),
            help: "The certificate contains a critical X.509 extension that is not supported.".to_string(),
        }),
        CertificateError::UnknownRevocationStatus => Box::new(GenericTlsDiagnostic {
            message: "Certificate revocation status unknown".to_string(),
            help: "Could not determine if the certificate has been revoked. Check OCSP/CRL availability.".to_string(),
        }),
        CertificateError::ExpiredRevocationList => Box::new(GenericTlsDiagnostic {
            message: "Certificate revocation list has expired".to_string(),
            help: "The CRL used to check revocation status has expired. Update the CRL.".to_string(),
        }),
        CertificateError::InvalidPurpose => Box::new(GenericTlsDiagnostic {
            message: "Certificate purpose is invalid".to_string(),
            help: "The certificate's Extended Key Usage doesn't allow this use. Check the certificate was generated for TLS client authentication.".to_string(),
        }),
        CertificateError::ApplicationVerificationFailure => Box::new(GenericTlsDiagnostic {
            message: "Application-level certificate verification failed".to_string(),
            help: "The certificate was rejected by custom verification logic.".to_string(),
        }),
        _ => Box::new(GenericTlsDiagnostic {
            message: format!("Certificate validation failed: {:?}", err),
            help: "Check your certificate configuration.".to_string(),
        }),
    }
}

/// Display a TLS error using miette's pretty printing
fn display_tls_error(diagnostic: &dyn miette::Diagnostic) {
    use std::fmt::Write;

    // Build a formatted error message
    let mut output = String::new();

    // Header
    writeln!(output).unwrap();
    writeln!(output, "  × TLS Connection Failed").unwrap();
    writeln!(output).unwrap();

    // Error code if present
    if let Some(code) = diagnostic.code() {
        writeln!(output, "  Error: {}", code).unwrap();
    }

    // Main message
    writeln!(output, "  {}", diagnostic).unwrap();

    // Help text if present
    if let Some(help) = diagnostic.help() {
        writeln!(output).unwrap();
        writeln!(output, "  help: {}", help).unwrap();
    }

    // URL if present
    if let Some(url) = diagnostic.url() {
        writeln!(output).unwrap();
        writeln!(output, "  docs: {}", url).unwrap();
    }

    writeln!(output).unwrap();
    writeln!(output, "  This error cannot be resolved by reconnecting.").unwrap();

    eprintln!("{}", output);
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
    metrics: MetricsCollector,
) -> Result<()> {
    // Connect to server
    let stream = TcpStream::connect(server_addr).await?;

    // Perform TLS handshake
    let tls_stream = tls_connector.connect(server_name, stream).await?;

    // Create tunnel connection handler
    let mut connection = TunnelConnection::new(
        tls_stream,
        local_addr.to_string(),
        metrics,
        tunnel_type.clone(),
    );

    // Request tunnel
    connection.request_tunnel(subdomain, tunnel_type).await?;

    // Run the connection (processes messages until disconnection)
    connection.run().await
}
