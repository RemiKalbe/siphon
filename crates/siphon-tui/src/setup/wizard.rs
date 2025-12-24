//! Styled CLI setup wizard for configuring Siphon connection settings

use std::io::{self, Write};

use crossterm::execute;
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};

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
        let mut stdout = io::stdout();

        // Header
        println!();
        self.print_header(&mut stdout)?;
        println!();

        self.print_dim(
            &mut stdout,
            "This will configure your connection to the tunnel server.",
        )?;
        self.print_dim(
            &mut stdout,
            "Runtime options (--local, --subdomain) are provided when starting.",
        )?;
        println!();
        println!();

        // Step 1: Server address
        self.print_step(&mut stdout, 1, 4, "Server Connection")?;
        self.config.server_addr =
            self.prompt(&mut stdout, "Server address", "tunnel.example.com:4443")?;
        if self.config.server_addr.is_empty() {
            self.print_error(&mut stdout, "Server address is required.")?;
            return Ok(None);
        }

        // Add default port if not specified
        if !self.config.server_addr.contains(':') {
            self.config.server_addr.push_str(":4443");
            self.print_info(&mut stdout, &format!("Using default port: 4443"))?;
        }
        self.print_success(&mut stdout, &format!("Server: {}", self.config.server_addr))?;
        println!();

        // Step 2: Client certificate
        self.print_step(&mut stdout, 2, 4, "Client Certificate")?;
        let cert_path = self.prompt(&mut stdout, "Certificate path", "~/certs/client.crt")?;
        if cert_path.is_empty() {
            self.print_error(&mut stdout, "Certificate is required.")?;
            return Ok(None);
        }
        let cert_pem = match self.load_and_validate_cert(&cert_path, "certificate") {
            Ok(pem) => {
                self.print_success(&mut stdout, &format!("Loaded from {}", cert_path))?;
                pem
            }
            Err(e) => {
                self.print_error(&mut stdout, &e.to_string())?;
                return Ok(None);
            }
        };
        println!();

        // Step 3: Private key
        self.print_step(&mut stdout, 3, 4, "Private Key")?;
        let key_path = self.prompt(&mut stdout, "Private key path", "~/certs/client.key")?;
        if key_path.is_empty() {
            self.print_error(&mut stdout, "Private key is required.")?;
            return Ok(None);
        }
        let key_pem = match self.load_and_validate_key(&key_path) {
            Ok(pem) => {
                self.print_success(&mut stdout, &format!("Loaded from {}", key_path))?;
                pem
            }
            Err(e) => {
                self.print_error(&mut stdout, &e.to_string())?;
                return Ok(None);
            }
        };
        println!();

        // Step 4: CA certificate
        self.print_step(&mut stdout, 4, 4, "CA Certificate")?;
        let ca_path = self.prompt(&mut stdout, "CA certificate path", "~/certs/ca.crt")?;
        if ca_path.is_empty() {
            self.print_error(&mut stdout, "CA certificate is required.")?;
            return Ok(None);
        }
        let ca_pem = match self.load_and_validate_cert(&ca_path, "CA certificate") {
            Ok(pem) => {
                self.print_success(&mut stdout, &format!("Loaded from {}", ca_path))?;
                pem
            }
            Err(e) => {
                self.print_error(&mut stdout, &e.to_string())?;
                return Ok(None);
            }
        };
        println!();

        // Store in keychain
        self.print_action(&mut stdout, "Storing credentials in OS keychain...")?;
        match self.store_credentials(&cert_pem, &key_pem, &ca_pem) {
            Ok(()) => {
                self.print_success(&mut stdout, "Credentials stored securely")?;
            }
            Err(e) => {
                self.print_error(&mut stdout, &format!("Failed to store credentials: {}", e))?;
                return Ok(None);
            }
        }

        // Update config with keychain references
        self.config.cert = "keychain://siphon/cert".to_string();
        self.config.key = "keychain://siphon/key".to_string();
        self.config.ca_cert = "keychain://siphon/ca".to_string();

        // Save config
        self.print_action(&mut stdout, "Saving configuration...")?;
        match self.config.save_default() {
            Ok(()) => {
                self.print_success(&mut stdout, "Saved to ~/.config/siphon/config.toml")?;
            }
            Err(e) => {
                self.print_error(&mut stdout, &format!("Failed to save config: {}", e))?;
                return Ok(None);
            }
        }

        println!();
        self.print_complete(&mut stdout)?;

        Ok(Some(self.config.clone()))
    }

    fn print_header(&self, stdout: &mut io::Stdout) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            SetAttribute(Attribute::Bold),
            Print("◆ Siphon Setup"),
            ResetColor,
            SetAttribute(Attribute::Reset),
        )?;
        println!();
        Ok(())
    }

    fn print_step(
        &self,
        stdout: &mut io::Stdout,
        current: u8,
        total: u8,
        title: &str,
    ) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::Blue),
            Print(format!("[{}/{}] ", current, total)),
            SetForegroundColor(Color::White),
            SetAttribute(Attribute::Bold),
            Print(title),
            ResetColor,
            SetAttribute(Attribute::Reset),
        )?;
        println!();
        Ok(())
    }

    fn print_success(&self, stdout: &mut io::Stdout, message: &str) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::Green),
            Print("  ✓ "),
            ResetColor,
            Print(message),
        )?;
        println!();
        Ok(())
    }

    fn print_error(&self, stdout: &mut io::Stdout, message: &str) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::Red),
            Print("  ✗ "),
            ResetColor,
            Print(message),
        )?;
        println!();
        Ok(())
    }

    fn print_info(&self, stdout: &mut io::Stdout, message: &str) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::Yellow),
            Print("  → "),
            ResetColor,
            Print(message),
        )?;
        println!();
        Ok(())
    }

    fn print_action(&self, stdout: &mut io::Stdout, message: &str) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("  ● "),
            ResetColor,
            Print(message),
        )?;
        println!();
        Ok(())
    }

    fn print_dim(&self, stdout: &mut io::Stdout, message: &str) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::DarkGrey),
            Print(format!("  {}", message)),
            ResetColor,
        )?;
        println!();
        Ok(())
    }

    fn print_complete(&self, stdout: &mut io::Stdout) -> anyhow::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(Color::Green),
            SetAttribute(Attribute::Bold),
            Print("◆ Setup complete!"),
            ResetColor,
            SetAttribute(Attribute::Reset),
        )?;
        println!();
        println!();
        execute!(
            stdout,
            Print("  Start a tunnel with: "),
            SetForegroundColor(Color::Cyan),
            Print("siphon --local 127.0.0.1:3000"),
            ResetColor,
        )?;
        println!();
        println!();
        Ok(())
    }

    fn prompt(
        &self,
        stdout: &mut io::Stdout,
        label: &str,
        placeholder: &str,
    ) -> anyhow::Result<String> {
        execute!(
            stdout,
            SetForegroundColor(Color::White),
            Print(format!("  {} ", label)),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("({})", placeholder)),
            ResetColor,
        )?;
        println!();

        execute!(stdout, SetForegroundColor(Color::Cyan), Print("  › "),)?;
        execute!(stdout, ResetColor)?;
        stdout.flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_string())
    }

    fn store_credentials(&self, cert_pem: &str, key_pem: &str, ca_pem: &str) -> anyhow::Result<()> {
        siphon_secrets::keychain::store("siphon", "cert", cert_pem)?;
        siphon_secrets::keychain::store("siphon", "key", key_pem)?;
        siphon_secrets::keychain::store("siphon", "ca", ca_pem)?;
        Ok(())
    }

    fn load_and_validate_cert(&self, path: &str, name: &str) -> anyhow::Result<String> {
        let expanded = shellexpand::tilde(path);
        let content = std::fs::read_to_string(expanded.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))?;

        if !content.contains("-----BEGIN CERTIFICATE-----") {
            anyhow::bail!("Invalid {}: must be PEM format", name);
        }

        Ok(content)
    }

    fn load_and_validate_key(&self, path: &str) -> anyhow::Result<String> {
        let expanded = shellexpand::tilde(path);
        let content = std::fs::read_to_string(expanded.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))?;

        if !content.contains("-----BEGIN") || !content.contains("PRIVATE KEY-----") {
            anyhow::bail!("Invalid private key: must be PEM format");
        }

        Ok(content)
    }
}

impl Default for SetupWizard {
    fn default() -> Self {
        Self::new()
    }
}
