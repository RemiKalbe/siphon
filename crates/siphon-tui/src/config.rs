//! Siphon configuration management
//!
//! Handles loading and saving configuration to `~/.config/siphon/config.toml`

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Siphon client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiphonConfig {
    /// Tunnel server address (host:port)
    pub server_addr: String,

    /// Local address to forward to (e.g., 127.0.0.1:3000)
    pub local_addr: String,

    /// Requested subdomain (None = auto-generate)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subdomain: Option<String>,

    /// Tunnel type: "http" or "tcp"
    #[serde(default = "default_tunnel_type")]
    pub tunnel_type: String,

    /// Client certificate reference (keychain://siphon/cert, file path, etc.)
    pub cert: String,

    /// Client private key reference (keychain://siphon/key, file path, etc.)
    pub key: String,

    /// CA certificate reference (keychain://siphon/ca, file path, etc.)
    pub ca_cert: String,
}

fn default_tunnel_type() -> String {
    "http".to_string()
}

impl Default for SiphonConfig {
    fn default() -> Self {
        Self {
            server_addr: String::new(),
            local_addr: "127.0.0.1:3000".to_string(),
            subdomain: None,
            tunnel_type: default_tunnel_type(),
            cert: String::new(),
            key: String::new(),
            ca_cert: String::new(),
        }
    }
}

impl SiphonConfig {
    /// Get the default config directory path
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("siphon")
    }

    /// Get the default config file path
    pub fn default_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Check if configuration file exists
    pub fn exists() -> bool {
        Self::default_path().exists()
    }

    /// Load configuration from a specific path
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration from the default location
    pub fn load_default() -> anyhow::Result<Self> {
        Self::load(&Self::default_path())
    }

    /// Try to load configuration, returning None if it doesn't exist
    pub fn try_load_default() -> Option<Self> {
        if Self::exists() {
            Self::load_default().ok()
        } else {
            None
        }
    }

    /// Save configuration to a specific path
    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;

        tracing::info!("Configuration saved to {:?}", path);
        Ok(())
    }

    /// Save configuration to the default location
    pub fn save_default(&self) -> anyhow::Result<()> {
        self.save(&Self::default_path())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.server_addr.is_empty() {
            errors.push("Server address is required".to_string());
        }

        if self.local_addr.is_empty() {
            errors.push("Local address is required".to_string());
        }

        if self.cert.is_empty() {
            errors.push("Certificate is required".to_string());
        }

        if self.key.is_empty() {
            errors.push("Private key is required".to_string());
        }

        if self.ca_cert.is_empty() {
            errors.push("CA certificate is required".to_string());
        }

        if !["http", "tcp"].contains(&self.tunnel_type.as_str()) {
            errors.push(format!(
                "Invalid tunnel type '{}'. Must be 'http' or 'tcp'",
                self.tunnel_type
            ));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_default_config() {
        let config = SiphonConfig::default();
        assert_eq!(config.tunnel_type, "http");
        assert_eq!(config.local_addr, "127.0.0.1:3000");
    }

    #[test]
    fn test_config_validation() {
        let config = SiphonConfig::default();
        let result = config.validate();
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("Server address")));
    }

    #[test]
    fn test_config_roundtrip() {
        let config = SiphonConfig {
            server_addr: "tunnel.example.com:4443".to_string(),
            local_addr: "127.0.0.1:8080".to_string(),
            subdomain: Some("myapp".to_string()),
            tunnel_type: "http".to_string(),
            cert: "keychain://siphon/cert".to_string(),
            key: "keychain://siphon/key".to_string(),
            ca_cert: "keychain://siphon/ca".to_string(),
        };

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        config.save(&path).unwrap();

        let loaded = SiphonConfig::load(&path).unwrap();
        assert_eq!(loaded.server_addr, config.server_addr);
        assert_eq!(loaded.subdomain, config.subdomain);
    }
}
