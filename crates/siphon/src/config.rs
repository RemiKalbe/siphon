use serde::Deserialize;
use siphon_secrets::{SecretResolver, SecretUri};

/// Client configuration (parsed from TOML)
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ClientConfig {
    /// Tunnel server address (host:port)
    pub server_addr: String,

    /// Client certificate (file path, keychain://, op://, env://, or plain PEM)
    #[serde(alias = "cert_path")]
    pub cert: SecretUri,

    /// Client private key (file path, keychain://, op://, env://, or plain PEM)
    #[serde(alias = "key_path")]
    pub key: SecretUri,

    /// CA certificate (file path, keychain://, op://, env://, or plain PEM)
    #[serde(alias = "ca_cert_path")]
    pub ca_cert: SecretUri,

    /// Requested subdomain (None = auto-generate)
    pub subdomain: Option<String>,

    /// Local service address to forward to
    pub local_addr: String,

    /// Tunnel type: "http" or "tcp"
    #[serde(default = "default_tunnel_type")]
    pub tunnel_type: String,
}

/// Resolved client configuration with actual secret values
#[derive(Debug)]
pub struct ResolvedClientConfig {
    pub server_addr: String,
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_cert_pem: String,
    pub subdomain: Option<String>,
    pub local_addr: String,
    pub tunnel_type: String,
}

fn default_tunnel_type() -> String {
    "http".to_string()
}

#[allow(dead_code)]
impl ClientConfig {
    /// Load configuration from a TOML file
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ClientConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Resolve all secrets and return a resolved configuration
    pub fn resolve(&self) -> anyhow::Result<ResolvedClientConfig> {
        let resolver = SecretResolver::new();

        tracing::info!("Resolving secrets from configuration...");

        let cert_pem = resolver.resolve_trimmed(&self.cert).map_err(|e| {
            anyhow::anyhow!("Failed to resolve certificate: {}", e)
        })?;

        let key_pem = resolver.resolve_trimmed(&self.key).map_err(|e| {
            anyhow::anyhow!("Failed to resolve private key: {}", e)
        })?;

        let ca_cert_pem = resolver.resolve_trimmed(&self.ca_cert).map_err(|e| {
            anyhow::anyhow!("Failed to resolve CA certificate: {}", e)
        })?;

        tracing::info!("All secrets resolved successfully");

        Ok(ResolvedClientConfig {
            server_addr: self.server_addr.clone(),
            cert_pem,
            key_pem,
            ca_cert_pem,
            subdomain: self.subdomain.clone(),
            local_addr: self.local_addr.clone(),
            tunnel_type: self.tunnel_type.clone(),
        })
    }

    /// Load and resolve configuration from a TOML file
    pub fn load_and_resolve(path: &str) -> anyhow::Result<ResolvedClientConfig> {
        let config = Self::load(path)?;
        config.resolve()
    }
}
