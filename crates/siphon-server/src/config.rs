//! Server configuration with environment variable priority
//!
//! Configuration is resolved in this order (first found wins):
//! 1. Environment variables (SIPHON_*)
//! 2. Config file (server.toml)
//! 3. Default values (where applicable)

use std::env;
use std::path::Path;

use serde::Deserialize;
use siphon_secrets::{SecretResolver, SecretUri};

/// Environment variable prefix
const ENV_PREFIX: &str = "SIPHON";

/// Server configuration (parsed from TOML, can be overridden by env)
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ServerConfig {
    /// Port for control plane (mTLS client connections)
    pub control_port: Option<u16>,

    /// Port for HTTP data plane (traffic from Cloudflare)
    pub http_port: Option<u16>,

    /// Base domain for tunnels (e.g., "tunnel.example.com")
    pub base_domain: Option<String>,

    /// Server certificate (file path, keychain://, op://, env://, or plain PEM)
    #[serde(alias = "cert_path")]
    pub cert: Option<String>,

    /// Server private key (file path, keychain://, op://, env://, or plain PEM)
    #[serde(alias = "key_path")]
    pub key: Option<String>,

    /// CA certificate for client verification (file path, keychain://, op://, env://, or plain PEM)
    #[serde(alias = "ca_cert_path")]
    pub ca_cert: Option<String>,

    /// Cloudflare configuration
    pub cloudflare: Option<CloudflareConfig>,

    /// TCP port range for TCP tunnels
    pub tcp_port_range: Option<(u16, u16)>,

    /// HTTP plane certificate for TLS (optional - enables HTTPS if set)
    pub http_cert: Option<String>,

    /// HTTP plane private key for TLS (optional - enables HTTPS if set)
    pub http_key: Option<String>,
}

/// Cloudflare API configuration
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CloudflareConfig {
    /// API token with DNS edit permissions
    pub api_token: Option<String>,

    /// Zone ID for the domain
    pub zone_id: Option<String>,

    /// Server's public IP (for A records) - mutually exclusive with server_cname
    pub server_ip: Option<String>,

    /// Server's CNAME target (for CNAME records) - use for platforms like Railway
    pub server_cname: Option<String>,
}

/// Resolved server configuration with actual secret values
#[derive(Debug)]
pub struct ResolvedServerConfig {
    pub control_port: u16,
    pub http_port: u16,
    pub base_domain: String,
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_cert_pem: String,
    pub cloudflare: ResolvedCloudflareConfig,
    pub tcp_port_range: (u16, u16),
    /// HTTP plane TLS certificate (if HTTPS is enabled)
    pub http_cert_pem: Option<String>,
    /// HTTP plane TLS private key (if HTTPS is enabled)
    pub http_key_pem: Option<String>,
}

/// DNS record target type
#[derive(Debug, Clone)]
pub enum DnsTarget {
    /// A record pointing to an IP address
    Ip(String),
    /// CNAME record pointing to a hostname
    Cname(String),
}

/// Resolved Cloudflare configuration with actual secret values
#[derive(Debug)]
pub struct ResolvedCloudflareConfig {
    pub api_token: String,
    pub zone_id: String,
    pub dns_target: DnsTarget,
}

/// Get environment variable with prefix
fn get_env(name: &str) -> Option<String> {
    env::var(format!("{}_{}", ENV_PREFIX, name)).ok()
}

/// Get environment variable as u16
fn get_env_u16(name: &str) -> Option<u16> {
    get_env(name).and_then(|v| v.parse().ok())
}

/// Auto-detect public IP address using external services
fn detect_public_ip() -> anyhow::Result<String> {
    // Try Cloudflare first (most reliable, returns structured data)
    if let Some(ip) = detect_ip_cloudflare() {
        tracing::info!("Detected public IP: {}", ip);
        return Ok(ip);
    }

    // Fallback to simple IP echo services
    let services = [
        "https://api.ipify.org",
        "https://ifconfig.me/ip",
        "https://icanhazip.com",
    ];

    for service in services {
        match ureq::get(service).call() {
            Ok(response) => {
                if let Ok(ip) = response.into_string() {
                    let ip = ip.trim().to_string();
                    if !ip.is_empty() {
                        tracing::info!("Detected public IP: {}", ip);
                        return Ok(ip);
                    }
                }
            }
            Err(e) => {
                tracing::debug!("Failed to get IP from {}: {}", service, e);
            }
        }
    }

    anyhow::bail!(
        "Could not auto-detect server IP. Set SIPHON_SERVER_IP or cloudflare.server_ip in config"
    )
}

/// Detect IP using Cloudflare's trace endpoint
fn detect_ip_cloudflare() -> Option<String> {
    match ureq::get("https://cloudflare.com/cdn-cgi/trace").call() {
        Ok(response) => {
            if let Ok(body) = response.into_string() {
                // Parse "ip=x.x.x.x" from the response
                for line in body.lines() {
                    if let Some(ip) = line.strip_prefix("ip=") {
                        return Some(ip.to_string());
                    }
                }
            }
            None
        }
        Err(e) => {
            tracing::debug!("Failed to get IP from Cloudflare trace: {}", e);
            None
        }
    }
}

impl ServerConfig {
    /// Load configuration from a TOML file (optional)
    pub fn load(path: &str) -> Self {
        if Path::new(path).exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(config) => {
                        tracing::info!("Loaded config from {}", path);
                        return config;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse {}: {}", path, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read {}: {}", path, e);
                }
            }
        }
        Self::default()
    }

    /// Resolve configuration from environment variables first, then config file
    pub fn resolve(self) -> anyhow::Result<ResolvedServerConfig> {
        let resolver = SecretResolver::new();

        // Control port: ENV > config > default 4443
        let control_port = get_env_u16("CONTROL_PORT")
            .or(self.control_port)
            .unwrap_or(4443);

        // HTTP port: ENV > config > default 8080
        let http_port = get_env_u16("HTTP_PORT").or(self.http_port).unwrap_or(8080);

        // Base domain: ENV > config > required
        let base_domain = get_env("BASE_DOMAIN").or(self.base_domain).ok_or_else(|| {
            anyhow::anyhow!("Base domain required. Set SIPHON_BASE_DOMAIN or base_domain in config")
        })?;

        // Certificate: ENV > config > required
        let cert_source = get_env("CERT").or(self.cert).ok_or_else(|| {
            anyhow::anyhow!("Certificate required. Set SIPHON_CERT or cert in config")
        })?;

        // Key: ENV > config > required
        let key_source = get_env("KEY").or(self.key).ok_or_else(|| {
            anyhow::anyhow!("Private key required. Set SIPHON_KEY or key in config")
        })?;

        // CA cert: ENV > config > required
        let ca_cert_source = get_env("CA_CERT").or(self.ca_cert).ok_or_else(|| {
            anyhow::anyhow!("CA certificate required. Set SIPHON_CA_CERT or ca_cert in config")
        })?;

        // Cloudflare API token: ENV > config > required
        let cf_config = self.cloudflare.unwrap_or_default();
        let cf_api_token_source = get_env("CLOUDFLARE_API_TOKEN")
            .or(cf_config.api_token)
            .ok_or_else(|| anyhow::anyhow!(
                "Cloudflare API token required. Set SIPHON_CLOUDFLARE_API_TOKEN or cloudflare.api_token in config"
            ))?;

        // Cloudflare zone ID: ENV > config > required
        let cf_zone_id = get_env("CLOUDFLARE_ZONE_ID")
            .or(cf_config.zone_id)
            .ok_or_else(|| anyhow::anyhow!(
                "Cloudflare zone ID required. Set SIPHON_CLOUDFLARE_ZONE_ID or cloudflare.zone_id in config"
            ))?;

        // DNS target: CNAME or IP (mutually exclusive)
        let cf_server_ip = get_env("SERVER_IP").or(cf_config.server_ip);
        let cf_server_cname = get_env("SERVER_CNAME").or(cf_config.server_cname);

        let dns_target = match (cf_server_ip, cf_server_cname) {
            (Some(_), Some(_)) => {
                anyhow::bail!(
                    "Cannot set both SIPHON_SERVER_IP and SIPHON_SERVER_CNAME. Use one or the other."
                )
            }
            (Some(ip), None) => DnsTarget::Ip(ip),
            (None, Some(cname)) => DnsTarget::Cname(cname),
            (None, None) => {
                tracing::info!("Server IP/CNAME not configured, auto-detecting IP...");
                DnsTarget::Ip(detect_public_ip()?)
            }
        };

        // TCP port range: ENV > config > default 30000-40000
        let tcp_port_start = get_env_u16("TCP_PORT_START")
            .or(self.tcp_port_range.map(|r| r.0))
            .unwrap_or(30000);
        let tcp_port_end = get_env_u16("TCP_PORT_END")
            .or(self.tcp_port_range.map(|r| r.1))
            .unwrap_or(40000);

        // Resolve secrets
        tracing::info!("Resolving secrets...");

        let cert_uri: SecretUri = cert_source
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid certificate source: {}", e))?;
        let key_uri: SecretUri = key_source
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid key source: {}", e))?;
        let ca_cert_uri: SecretUri = ca_cert_source
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid CA certificate source: {}", e))?;
        let api_token_uri: SecretUri = cf_api_token_source
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid Cloudflare API token source: {}", e))?;

        let cert_pem = resolver
            .resolve_trimmed(&cert_uri)
            .map_err(|e| anyhow::anyhow!("Failed to resolve certificate: {}", e))?;
        let key_pem = resolver
            .resolve_trimmed(&key_uri)
            .map_err(|e| anyhow::anyhow!("Failed to resolve private key: {}", e))?;
        let ca_cert_pem = resolver
            .resolve_trimmed(&ca_cert_uri)
            .map_err(|e| anyhow::anyhow!("Failed to resolve CA certificate: {}", e))?;
        let api_token = resolver
            .resolve_trimmed(&api_token_uri)
            .map_err(|e| anyhow::anyhow!("Failed to resolve Cloudflare API token: {}", e))?;

        // HTTP plane TLS (optional)
        let http_cert_source = get_env("HTTP_CERT").or(self.http_cert);
        let http_key_source = get_env("HTTP_KEY").or(self.http_key);

        let (http_cert_pem, http_key_pem) = match (http_cert_source, http_key_source) {
            (Some(cert_src), Some(key_src)) => {
                let cert_uri: SecretUri = cert_src
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Invalid HTTP certificate source: {}", e))?;
                let key_uri: SecretUri = key_src
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Invalid HTTP key source: {}", e))?;

                let cert = resolver
                    .resolve_trimmed(&cert_uri)
                    .map_err(|e| anyhow::anyhow!("Failed to resolve HTTP certificate: {}", e))?;
                let key = resolver
                    .resolve_trimmed(&key_uri)
                    .map_err(|e| anyhow::anyhow!("Failed to resolve HTTP key: {}", e))?;

                tracing::info!("HTTP plane TLS enabled");
                (Some(cert), Some(key))
            }
            (Some(_), None) => {
                anyhow::bail!("SIPHON_HTTP_CERT is set but SIPHON_HTTP_KEY is missing")
            }
            (None, Some(_)) => {
                anyhow::bail!("SIPHON_HTTP_KEY is set but SIPHON_HTTP_CERT is missing")
            }
            (None, None) => (None, None),
        };

        tracing::info!("All secrets resolved successfully");

        Ok(ResolvedServerConfig {
            control_port,
            http_port,
            base_domain,
            cert_pem,
            key_pem,
            ca_cert_pem,
            cloudflare: ResolvedCloudflareConfig {
                api_token,
                zone_id: cf_zone_id,
                dns_target,
            },
            tcp_port_range: (tcp_port_start, tcp_port_end),
            http_cert_pem,
            http_key_pem,
        })
    }

    /// Load config file and resolve with environment variable overrides
    pub fn load_and_resolve(path: &str) -> anyhow::Result<ResolvedServerConfig> {
        let config = Self::load(path);
        config.resolve()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_prefix() {
        assert_eq!(ENV_PREFIX, "SIPHON");
    }

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert!(config.control_port.is_none());
        assert!(config.http_port.is_none());
        assert!(config.base_domain.is_none());
    }
}
