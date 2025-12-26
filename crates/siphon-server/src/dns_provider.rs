//! DNS provider abstraction for tunnel DNS management
//!
//! This trait allows for different DNS backends (Cloudflare, mock for testing, etc.)

use async_trait::async_trait;
use thiserror::Error;

/// Origin CA certificate and private key
#[derive(Debug, Clone)]
pub struct OriginCertificate {
    /// PEM-encoded certificate
    pub certificate: String,
    /// PEM-encoded private key
    pub private_key: String,
    /// Certificate expiration date
    pub expires_on: String,
}

/// Errors from DNS provider operations
#[derive(Debug, Error)]
pub enum DnsError {
    #[error("HTTP request failed: {0}")]
    Request(String),

    #[error("API error: {0}")]
    Api(String),
}

/// Trait for DNS and certificate management providers
///
/// This abstraction allows the server to work with different DNS backends,
/// such as Cloudflare for production or a mock implementation for testing.
#[async_trait]
pub trait DnsProvider: Send + Sync {
    /// Create a DNS record for a subdomain
    ///
    /// # Arguments
    /// * `subdomain` - The subdomain to create (e.g., "myapp")
    /// * `proxied` - Whether to proxy through the provider (true for HTTP, false for TCP)
    ///
    /// # Returns
    /// The DNS record ID for later deletion
    async fn create_record(&self, subdomain: &str, proxied: bool) -> Result<String, DnsError>;

    /// Delete a DNS record by its ID
    async fn delete_record(&self, record_id: &str) -> Result<(), DnsError>;

    /// Create an origin certificate for HTTPS
    ///
    /// # Arguments
    /// * `validity_days` - Certificate validity in days
    ///
    /// # Returns
    /// An OriginCertificate if the provider supports it, None otherwise
    async fn create_origin_certificate(
        &self,
        validity_days: u32,
    ) -> Result<Option<OriginCertificate>, DnsError>;

    /// Clean up old origin certificates for this domain
    ///
    /// # Returns
    /// The number of certificates revoked
    async fn cleanup_old_origin_certificates(&self) -> Result<u32, DnsError>;
}
