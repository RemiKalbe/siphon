//! Simple CLI setup wizard for configuring Siphon connection settings

use std::io::{self, Write};

use crate::config::SiphonConfig;

/// Setup wizard for interactive configuration
pub struct SetupWizard {
    config: SiphonConfig,
}

impl SetupWizard {
    /// Create a new setup wizard
    pub fn new() -> Self {
        Self {
            config: SiphonConfig::default(),
        }
    }

    /// Run the setup wizard
    pub fn run(&mut self) -> anyhow::Result<Option<SiphonConfig>> {
        println!();
        println!("Siphon Setup");
        println!("============");
        println!();
        println!("This will configure your connection to the tunnel server.");
        println!(
            "Runtime options (local address, subdomain) are provided when starting the tunnel."
        );
        println!();

        // Server address
        self.config.server_addr = self.prompt("Server address (e.g., tunnel.example.com:4443)")?;
        if self.config.server_addr.is_empty() {
            println!("Server address is required.");
            return Ok(None);
        }

        // Add default port if not specified
        if !self.config.server_addr.contains(':') {
            self.config.server_addr.push_str(":4443");
            println!("  Using default port: {}", self.config.server_addr);
        }

        println!();

        // Certificate
        let cert_path = self.prompt("Client certificate path")?;
        if cert_path.is_empty() {
            println!("Certificate is required.");
            return Ok(None);
        }
        let cert_pem = self.load_and_validate_cert(&cert_path, "certificate")?;

        // Private key
        let key_path = self.prompt("Private key path")?;
        if key_path.is_empty() {
            println!("Private key is required.");
            return Ok(None);
        }
        let key_pem = self.load_and_validate_key(&key_path)?;

        // CA certificate
        let ca_path = self.prompt("CA certificate path")?;
        if ca_path.is_empty() {
            println!("CA certificate is required.");
            return Ok(None);
        }
        let ca_pem = self.load_and_validate_cert(&ca_path, "CA certificate")?;

        println!();

        // Store in keychain
        println!("Storing credentials in OS keychain...");
        siphon_secrets::keychain::store("siphon", "cert", &cert_pem)?;
        siphon_secrets::keychain::store("siphon", "key", &key_pem)?;
        siphon_secrets::keychain::store("siphon", "ca", &ca_pem)?;
        println!("  Credentials stored successfully.");

        // Update config with keychain references
        self.config.cert = "keychain://siphon/cert".to_string();
        self.config.key = "keychain://siphon/key".to_string();
        self.config.ca_cert = "keychain://siphon/ca".to_string();

        // Save config
        println!();
        println!("Saving configuration...");
        self.config.save_default()?;
        println!("  Saved to: ~/.config/siphon/config.toml");

        println!();
        println!("Setup complete!");
        println!();
        println!("Start a tunnel with:");
        println!("  siphon --local 127.0.0.1:3000");
        println!();

        Ok(Some(self.config.clone()))
    }

    fn prompt(&self, message: &str) -> anyhow::Result<String> {
        print!("{}: ", message);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_string())
    }

    fn load_and_validate_cert(&self, path: &str, name: &str) -> anyhow::Result<String> {
        let expanded = shellexpand::tilde(path);
        let content = std::fs::read_to_string(expanded.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))?;

        if !content.contains("-----BEGIN CERTIFICATE-----") {
            anyhow::bail!("Invalid {}: must be PEM format", name);
        }

        println!("  Loaded {} from {}", name, path);
        Ok(content)
    }

    fn load_and_validate_key(&self, path: &str) -> anyhow::Result<String> {
        let expanded = shellexpand::tilde(path);
        let content = std::fs::read_to_string(expanded.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))?;

        if !content.contains("-----BEGIN") || !content.contains("PRIVATE KEY-----") {
            anyhow::bail!("Invalid private key: must be PEM format");
        }

        println!("  Loaded private key from {}", path);
        Ok(content)
    }
}

impl Default for SetupWizard {
    fn default() -> Self {
        Self::new()
    }
}
