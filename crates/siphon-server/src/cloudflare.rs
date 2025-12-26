use rcgen::{CertificateParams, KeyPair};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{DnsTarget, ResolvedCloudflareConfig};

/// Cloudflare API client for DNS and Origin CA management
pub struct CloudflareClient {
    client: Client,
    api_token: String,
    zone_id: String,
    dns_target: DnsTarget,
    base_domain: String,
}

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

#[derive(Debug, Serialize)]
struct CreateDnsRecord {
    #[serde(rename = "type")]
    record_type: String,
    name: String,
    content: String,
    ttl: u32,
    proxied: bool,
}

#[derive(Debug, Deserialize)]
struct DnsRecordResponse {
    success: bool,
    result: Option<DnsRecord>,
    errors: Vec<CloudflareApiError>,
}

#[derive(Debug, Deserialize)]
struct DnsRecord {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CloudflareApiError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct DeleteResponse {
    success: bool,
}

/// Request body for creating an Origin CA certificate
#[derive(Debug, Serialize)]
struct CreateOriginCertRequest {
    /// PEM-encoded CSR
    csr: String,
    /// Hostnames to include in the certificate
    hostnames: Vec<String>,
    /// Certificate type: "origin-rsa" or "origin-ecc"
    request_type: String,
    /// Validity period in days (7, 30, 90, 365, 730, 1095, or 5475)
    requested_validity: u32,
}

/// Response from Origin CA certificate creation
#[derive(Debug, Deserialize)]
struct OriginCertResponse {
    success: bool,
    result: Option<OriginCertResult>,
    errors: Vec<CloudflareApiError>,
}

#[derive(Debug, Deserialize)]
struct OriginCertResult {
    certificate: String,
    expires_on: String,
}

/// Response from listing Origin CA certificates
#[derive(Debug, Deserialize)]
struct ListOriginCertsResponse {
    success: bool,
    result: Option<Vec<OriginCertListItem>>,
    errors: Vec<CloudflareApiError>,
}

/// An Origin CA certificate in the list response
#[derive(Debug, Deserialize)]
struct OriginCertListItem {
    id: String,
    hostnames: Vec<String>,
    expires_on: String,
}

/// Response from revoking an Origin CA certificate
#[derive(Debug, Deserialize)]
struct RevokeOriginCertResponse {
    success: bool,
    errors: Vec<CloudflareApiError>,
}

#[derive(Debug, Error)]
pub enum CloudflareError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error: {0}")]
    Api(String),
}

impl CloudflareClient {
    pub fn new(config: &ResolvedCloudflareConfig, base_domain: &str) -> Self {
        Self {
            client: Client::new(),
            api_token: config.api_token.clone(),
            zone_id: config.zone_id.clone(),
            dns_target: config.dns_target.clone(),
            base_domain: base_domain.to_string(),
        }
    }

    /// Create a DNS record for a subdomain (A record for IP, CNAME for hostname)
    ///
    /// # Arguments
    /// * `subdomain` - The subdomain to create (e.g., "myapp")
    /// * `proxied` - Whether to proxy through Cloudflare (true for HTTP, false for TCP)
    ///
    /// # Returns
    /// The DNS record ID for later deletion
    pub async fn create_record(
        &self,
        subdomain: &str,
        proxied: bool,
    ) -> Result<String, CloudflareError> {
        let full_name = format!("{}.{}", subdomain, self.base_domain);

        let (record_type, content) = match &self.dns_target {
            DnsTarget::Ip(ip) => ("A", ip.clone()),
            DnsTarget::Cname(hostname) => ("CNAME", hostname.clone()),
        };

        tracing::info!(
            "Creating DNS {} record: {} -> {} (proxied: {})",
            record_type,
            full_name,
            content,
            proxied
        );

        let response = self
            .client
            .post(format!(
                "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
                self.zone_id
            ))
            .bearer_auth(&self.api_token)
            .json(&CreateDnsRecord {
                record_type: record_type.to_string(),
                name: full_name.clone(),
                content,
                ttl: 60, // Short TTL for dynamic records
                proxied,
            })
            .send()
            .await?;

        let result: DnsRecordResponse = response.json().await?;

        if result.success {
            let record = result
                .result
                .ok_or_else(|| CloudflareError::Api("No record in response".to_string()))?;
            tracing::info!("Created DNS record {} with ID {}", full_name, record.id);
            Ok(record.id)
        } else {
            let error_msg = result
                .errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join(", ");
            Err(CloudflareError::Api(error_msg))
        }
    }

    /// Delete a DNS record
    pub async fn delete_record(&self, record_id: &str) -> Result<(), CloudflareError> {
        tracing::info!("Deleting DNS record {}", record_id);

        let response = self
            .client
            .delete(format!(
                "https://api.cloudflare.com/client/v4/zones/{}/dns_records/{}",
                self.zone_id, record_id
            ))
            .bearer_auth(&self.api_token)
            .send()
            .await?;

        let result: DeleteResponse = response.json().await?;

        if result.success {
            tracing::info!("Deleted DNS record {}", record_id);
            Ok(())
        } else {
            Err(CloudflareError::Api(format!(
                "Failed to delete record {}",
                record_id
            )))
        }
    }

    /// Create an Origin CA certificate for the base domain
    ///
    /// This generates a private key and CSR locally, then requests a certificate
    /// from Cloudflare's Origin CA. The certificate is valid for HTTPS connections
    /// from Cloudflare to this origin server (Full Strict mode).
    ///
    /// # Arguments
    /// * `validity_days` - Certificate validity in days (default: 365)
    ///
    /// # Returns
    /// An OriginCertificate containing the certificate and private key in PEM format
    pub async fn create_origin_certificate(
        &self,
        validity_days: u32,
    ) -> Result<OriginCertificate, CloudflareError> {
        tracing::info!(
            "Creating Origin CA certificate for *.{} (valid for {} days)",
            self.base_domain,
            validity_days
        );

        // Generate a new key pair
        let key_pair = KeyPair::generate().map_err(|e| {
            CloudflareError::Api(format!("Failed to generate key pair: {}", e))
        })?;

        // Create certificate parameters for CSR
        let mut params = CertificateParams::default();
        params.distinguished_name = rcgen::DistinguishedName::new();

        // Generate CSR
        let csr = params
            .serialize_request(&key_pair)
            .map_err(|e| CloudflareError::Api(format!("Failed to generate CSR: {}", e)))?;

        let csr_pem = csr.pem().map_err(|e| {
            CloudflareError::Api(format!("Failed to encode CSR as PEM: {}", e))
        })?;

        // Hostnames: wildcard + base domain
        let hostnames = vec![
            format!("*.{}", self.base_domain),
            self.base_domain.clone(),
        ];

        tracing::debug!("Requesting Origin CA certificate for hostnames: {:?}", hostnames);

        // Request certificate from Cloudflare Origin CA
        // Use origin-ecc since rcgen generates ECDSA keys by default
        let response = self
            .client
            .post("https://api.cloudflare.com/client/v4/certificates")
            .bearer_auth(&self.api_token)
            .json(&CreateOriginCertRequest {
                csr: csr_pem,
                hostnames,
                request_type: "origin-ecc".to_string(),
                requested_validity: validity_days,
            })
            .send()
            .await?;

        let result: OriginCertResponse = response.json().await?;

        if result.success {
            let cert_result = result
                .result
                .ok_or_else(|| CloudflareError::Api("No certificate in response".to_string()))?;

            tracing::info!(
                "Created Origin CA certificate for *.{}, expires: {}",
                self.base_domain,
                cert_result.expires_on
            );

            Ok(OriginCertificate {
                certificate: cert_result.certificate,
                private_key: key_pair.serialize_pem(),
                expires_on: cert_result.expires_on,
            })
        } else {
            let error_msg = result
                .errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join(", ");
            Err(CloudflareError::Api(format!(
                "Failed to create Origin CA certificate: {}",
                error_msg
            )))
        }
    }

    /// List all Origin CA certificates for the zone
    async fn list_origin_certificates(&self) -> Result<Vec<OriginCertListItem>, CloudflareError> {
        let response = self
            .client
            .get(format!(
                "https://api.cloudflare.com/client/v4/certificates?zone_id={}",
                self.zone_id
            ))
            .bearer_auth(&self.api_token)
            .send()
            .await?;

        let result: ListOriginCertsResponse = response.json().await?;

        if result.success {
            Ok(result.result.unwrap_or_default())
        } else {
            let error_msg = result
                .errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join(", ");
            Err(CloudflareError::Api(format!(
                "Failed to list Origin CA certificates: {}",
                error_msg
            )))
        }
    }

    /// Revoke an Origin CA certificate by its ID
    async fn revoke_origin_certificate(&self, cert_id: &str) -> Result<(), CloudflareError> {
        tracing::info!("Revoking Origin CA certificate {}", cert_id);

        let response = self
            .client
            .delete(format!(
                "https://api.cloudflare.com/client/v4/certificates/{}",
                cert_id
            ))
            .bearer_auth(&self.api_token)
            .send()
            .await?;

        let result: RevokeOriginCertResponse = response.json().await?;

        if result.success {
            tracing::info!("Revoked Origin CA certificate {}", cert_id);
            Ok(())
        } else {
            let error_msg = result
                .errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join(", ");
            Err(CloudflareError::Api(format!(
                "Failed to revoke certificate {}: {}",
                cert_id, error_msg
            )))
        }
    }

    /// Clean up old Origin CA certificates for this domain
    ///
    /// This revokes any existing Origin CA certificates that match our base domain
    /// (either *.base_domain or base_domain). Should be called before creating
    /// a new certificate to avoid accumulating old ones.
    pub async fn cleanup_old_origin_certificates(&self) -> Result<u32, CloudflareError> {
        let wildcard = format!("*.{}", self.base_domain);
        let certs = self.list_origin_certificates().await?;

        let mut revoked = 0;
        for cert in certs {
            // Check if this certificate is for our domain
            let matches = cert.hostnames.iter().any(|h| {
                h == &self.base_domain || h == &wildcard
            });

            if matches {
                tracing::info!(
                    "Found old Origin CA certificate {} for {:?}, expires {}",
                    cert.id,
                    cert.hostnames,
                    cert.expires_on
                );

                if let Err(e) = self.revoke_origin_certificate(&cert.id).await {
                    tracing::warn!("Failed to revoke certificate {}: {}", cert.id, e);
                } else {
                    revoked += 1;
                }
            }
        }

        if revoked > 0 {
            tracing::info!("Revoked {} old Origin CA certificate(s)", revoked);
        }

        Ok(revoked)
    }
}
