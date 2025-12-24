use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{DnsTarget, ResolvedCloudflareConfig};

/// Cloudflare API client for DNS management
pub struct CloudflareClient {
    client: Client,
    api_token: String,
    zone_id: String,
    dns_target: DnsTarget,
    base_domain: String,
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
}
