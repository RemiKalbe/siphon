use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pemfile::{certs, private_key};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::sync::Arc;

use crate::TunnelError;

/// Load certificates from a PEM file
fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, TunnelError> {
    let file = File::open(path).map_err(|e| {
        TunnelError::Certificate(format!("Failed to open cert file {:?}: {}", path, e))
    })?;
    let mut reader = BufReader::new(file);
    certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TunnelError::Certificate(format!("Failed to parse certificates: {}", e)))
}

/// Load a private key from a PEM file
fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TunnelError> {
    let file = File::open(path).map_err(|e| {
        TunnelError::Certificate(format!("Failed to open key file {:?}: {}", path, e))
    })?;
    let mut reader = BufReader::new(file);
    private_key(&mut reader)
        .map_err(|e| TunnelError::Certificate(format!("Failed to parse private key: {}", e)))?
        .ok_or_else(|| TunnelError::Certificate("No private key found in file".to_string()))
}

/// Load a root certificate store from a CA file
fn load_root_store(ca_path: &Path) -> Result<RootCertStore, TunnelError> {
    let ca_certs = load_certs(ca_path)?;
    let mut root_store = RootCertStore::empty();
    for cert in ca_certs {
        root_store.add(cert).map_err(|e| {
            TunnelError::Certificate(format!("Failed to add CA certificate: {}", e))
        })?;
    }
    Ok(root_store)
}

// ============================================================================
// PEM content loading functions (for secret-resolved content)
// ============================================================================

/// Load certificates from PEM content string
pub fn load_certs_from_pem(pem_content: &str) -> Result<Vec<CertificateDer<'static>>, TunnelError> {
    let mut cursor = Cursor::new(pem_content.as_bytes());
    certs(&mut cursor)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TunnelError::Certificate(format!("Failed to parse certificates: {}", e)))
}

/// Load a private key from PEM content string
pub fn load_private_key_from_pem(pem_content: &str) -> Result<PrivateKeyDer<'static>, TunnelError> {
    let mut cursor = Cursor::new(pem_content.as_bytes());
    private_key(&mut cursor)
        .map_err(|e| TunnelError::Certificate(format!("Failed to parse private key: {}", e)))?
        .ok_or_else(|| TunnelError::Certificate("No private key found in PEM content".to_string()))
}

/// Load a root certificate store from PEM content string
fn load_root_store_from_pem(pem_content: &str) -> Result<RootCertStore, TunnelError> {
    let ca_certs = load_certs_from_pem(pem_content)?;
    let mut root_store = RootCertStore::empty();
    for cert in ca_certs {
        root_store.add(cert).map_err(|e| {
            TunnelError::Certificate(format!("Failed to add CA certificate: {}", e))
        })?;
    }
    Ok(root_store)
}

/// Load server TLS config with mTLS (client certificate verification)
///
/// # Arguments
/// * `cert_path` - Path to server certificate PEM file
/// * `key_path` - Path to server private key PEM file
/// * `ca_path` - Path to CA certificate for verifying client certificates
pub fn load_server_config(
    cert_path: &Path,
    key_path: &Path,
    ca_path: &Path,
) -> Result<ServerConfig, TunnelError> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;
    let root_store = load_root_store(ca_path)?;

    // Require client certificates
    let client_verifier = WebPkiClientVerifier::builder(Arc::new(root_store))
        .build()
        .map_err(|e| TunnelError::Tls(format!("Failed to build client verifier: {}", e)))?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certs, key)
        .map_err(|e| TunnelError::Tls(format!("Failed to build server config: {}", e)))?;

    Ok(config)
}

/// Load client TLS config with mTLS (present client certificate)
///
/// # Arguments
/// * `cert_path` - Path to client certificate PEM file
/// * `key_path` - Path to client private key PEM file
/// * `ca_path` - Path to CA certificate for verifying server certificate
pub fn load_client_config(
    cert_path: &Path,
    key_path: &Path,
    ca_path: &Path,
) -> Result<ClientConfig, TunnelError> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;
    let root_store = load_root_store(ca_path)?;

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(certs, key)
        .map_err(|e| TunnelError::Tls(format!("Failed to build client config: {}", e)))?;

    Ok(config)
}

/// Load server TLS config from PEM content strings with mTLS
///
/// # Arguments
/// * `cert_pem` - Server certificate PEM content
/// * `key_pem` - Server private key PEM content
/// * `ca_pem` - CA certificate PEM content for verifying client certificates
pub fn load_server_config_from_pem(
    cert_pem: &str,
    key_pem: &str,
    ca_pem: &str,
) -> Result<ServerConfig, TunnelError> {
    let certs = load_certs_from_pem(cert_pem)?;
    let key = load_private_key_from_pem(key_pem)?;
    let root_store = load_root_store_from_pem(ca_pem)?;

    // Require client certificates
    let client_verifier = WebPkiClientVerifier::builder(Arc::new(root_store))
        .build()
        .map_err(|e| TunnelError::Tls(format!("Failed to build client verifier: {}", e)))?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(certs, key)
        .map_err(|e| TunnelError::Tls(format!("Failed to build server config: {}", e)))?;

    Ok(config)
}

/// Load client TLS config from PEM content strings with mTLS
///
/// # Arguments
/// * `cert_pem` - Client certificate PEM content
/// * `key_pem` - Client private key PEM content
/// * `ca_pem` - CA certificate PEM content for verifying server certificate
pub fn load_client_config_from_pem(
    cert_pem: &str,
    key_pem: &str,
    ca_pem: &str,
) -> Result<ClientConfig, TunnelError> {
    let certs = load_certs_from_pem(cert_pem)?;
    let key = load_private_key_from_pem(key_pem)?;
    let root_store = load_root_store_from_pem(ca_pem)?;

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(certs, key)
        .map_err(|e| TunnelError::Tls(format!("Failed to build client config: {}", e)))?;

    Ok(config)
}

/// Extract the Common Name (CN) from a certificate
#[allow(dead_code)]
pub fn extract_cn(cert: &rustls::pki_types::CertificateDer<'_>) -> Option<String> {
    // Parse the certificate using x509-parser would be ideal here,
    // but for simplicity we'll just note this is where CN extraction would go
    // In a real implementation, add x509-parser to dependencies

    // For now, return a placeholder - this should be implemented properly
    // when we add certificate parsing
    let _ = cert;
    None
}
