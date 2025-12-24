//! Styled CLI setup wizard for configuring Siphon connection settings

use std::borrow::Cow;
use std::io;

use crossterm::cursor::MoveUp;
use crossterm::execute;
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Config, Editor, Helper};

use crate::config::SiphonConfig;

/// Path completer helper for rustyline
struct PathHelper {
    completer: FilenameCompleter,
}

impl PathHelper {
    fn new() -> Self {
        Self {
            completer: FilenameCompleter::new(),
        }
    }
}

impl Completer for PathHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        self.completer.complete(line, pos, ctx)
    }
}

impl Hinter for PathHelper {
    type Hint = String;
}

impl Highlighter for PathHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Borrowed(prompt)
    }
}

impl Validator for PathHelper {}

impl Helper for PathHelper {}

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

        // Create rustyline editors
        let config = Config::builder().auto_add_history(false).build();
        let mut text_editor: Editor<(), DefaultHistory> = Editor::with_config(config.clone())?;
        let mut path_editor: Editor<PathHelper, DefaultHistory> = Editor::with_config(config)?;
        path_editor.set_helper(Some(PathHelper::new()));

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
        let server_addr = self.prompt_text(
            &mut stdout,
            &mut text_editor,
            "Server address",
            "tunnel.example.com:4443",
        )?;
        let server_addr = match server_addr {
            Some(addr) => addr,
            None => return Ok(None),
        };

        if server_addr.is_empty() {
            self.print_error(&mut stdout, "Server address is required.")?;
            return Ok(None);
        }

        // Add default port if not specified
        self.config.server_addr = if server_addr.contains(':') {
            server_addr
        } else {
            format!("{}:4443", server_addr)
        };

        self.clear_prompt_lines(&mut stdout, 2)?;
        self.print_success(&mut stdout, &format!("Server: {}", self.config.server_addr))?;
        println!();

        // Step 2: Client certificate
        self.print_step(&mut stdout, 2, 4, "Client Certificate")?;
        let cert_path = self.prompt_path(
            &mut stdout,
            &mut path_editor,
            "Certificate path",
            "~/certs/client.crt",
        )?;
        let cert_path = match cert_path {
            Some(path) => path,
            None => return Ok(None),
        };

        if cert_path.is_empty() {
            self.print_error(&mut stdout, "Certificate is required.")?;
            return Ok(None);
        }

        let cert_pem = match self.load_and_validate_cert(&cert_path, "certificate") {
            Ok(pem) => pem,
            Err(e) => {
                self.print_error(&mut stdout, &e.to_string())?;
                return Ok(None);
            }
        };

        self.clear_prompt_lines(&mut stdout, 2)?;
        self.print_success(&mut stdout, &format!("Certificate: {}", cert_path))?;
        println!();

        // Step 3: Private key
        self.print_step(&mut stdout, 3, 4, "Private Key")?;
        let key_path = self.prompt_path(
            &mut stdout,
            &mut path_editor,
            "Private key path",
            "~/certs/client.key",
        )?;
        let key_path = match key_path {
            Some(path) => path,
            None => return Ok(None),
        };

        if key_path.is_empty() {
            self.print_error(&mut stdout, "Private key is required.")?;
            return Ok(None);
        }

        let key_pem = match self.load_and_validate_key(&key_path) {
            Ok(pem) => pem,
            Err(e) => {
                self.print_error(&mut stdout, &e.to_string())?;
                return Ok(None);
            }
        };

        self.clear_prompt_lines(&mut stdout, 2)?;
        self.print_success(&mut stdout, &format!("Private key: {}", key_path))?;
        println!();

        // Step 4: CA certificate
        self.print_step(&mut stdout, 4, 4, "CA Certificate")?;
        let ca_path = self.prompt_path(
            &mut stdout,
            &mut path_editor,
            "CA certificate path",
            "~/certs/ca.crt",
        )?;
        let ca_path = match ca_path {
            Some(path) => path,
            None => return Ok(None),
        };

        if ca_path.is_empty() {
            self.print_error(&mut stdout, "CA certificate is required.")?;
            return Ok(None);
        }

        let ca_pem = match self.load_and_validate_cert(&ca_path, "CA certificate") {
            Ok(pem) => pem,
            Err(e) => {
                self.print_error(&mut stdout, &e.to_string())?;
                return Ok(None);
            }
        };

        self.clear_prompt_lines(&mut stdout, 2)?;
        self.print_success(&mut stdout, &format!("CA certificate: {}", ca_path))?;
        println!();

        // Store in keychain
        self.print_action(&mut stdout, "Storing credentials in OS keychain...")?;
        match self.store_credentials(&cert_pem, &key_pem, &ca_pem) {
            Ok(()) => {
                self.clear_prompt_lines(&mut stdout, 1)?;
                self.print_success(&mut stdout, "Credentials stored in keychain")?;
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
                self.clear_prompt_lines(&mut stdout, 1)?;
                self.print_success(&mut stdout, "Config saved to ~/.config/siphon/config.toml")?;
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

    fn clear_prompt_lines(&self, stdout: &mut io::Stdout, lines: u16) -> anyhow::Result<()> {
        for _ in 0..lines {
            execute!(stdout, MoveUp(1), Clear(ClearType::CurrentLine))?;
        }
        Ok(())
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

    fn prompt_text(
        &self,
        stdout: &mut io::Stdout,
        editor: &mut Editor<(), DefaultHistory>,
        label: &str,
        placeholder: &str,
    ) -> anyhow::Result<Option<String>> {
        execute!(
            stdout,
            SetForegroundColor(Color::White),
            Print(format!("  {} ", label)),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("({})", placeholder)),
            ResetColor,
        )?;
        println!();

        // Build colored prompt
        let prompt = format!("\x1b[36m  › \x1b[0m");

        match editor.readline(&prompt) {
            Ok(line) => Ok(Some(line.trim().to_string())),
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn prompt_path(
        &self,
        stdout: &mut io::Stdout,
        editor: &mut Editor<PathHelper, DefaultHistory>,
        label: &str,
        placeholder: &str,
    ) -> anyhow::Result<Option<String>> {
        execute!(
            stdout,
            SetForegroundColor(Color::White),
            Print(format!("  {} ", label)),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("({}) ", placeholder)),
            SetForegroundColor(Color::DarkGrey),
            Print("[Tab to complete]"),
            ResetColor,
        )?;
        println!();

        // Build colored prompt
        let prompt = format!("\x1b[36m  › \x1b[0m");

        match editor.readline(&prompt) {
            Ok(line) => Ok(Some(line.trim().to_string())),
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => Ok(None),
            Err(e) => Err(e.into()),
        }
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
