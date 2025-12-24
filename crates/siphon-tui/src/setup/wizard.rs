//! Interactive setup wizard for configuring Siphon

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io;

use crate::config::SiphonConfig;

/// Wizard step enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Welcome,
    ServerAddress,
    LocalAddress,
    Subdomain,
    TunnelType,
    CertificateInput,
    KeyInput,
    CaCertInput,
    Review,
    Saving,
    Complete,
}

/// Setup wizard for interactive configuration
pub struct SetupWizard {
    step: WizardStep,
    config: SiphonConfig,

    // Temporary storage for secrets (not saved to config file)
    cert_pem: String,
    key_pem: String,
    ca_pem: String,

    // Form state
    current_input: String,
    cursor_position: usize,
    error_message: Option<String>,
    tunnel_type_index: usize,

    // Multiline input state
    multiline_mode: bool,
    multiline_buffer: Vec<String>,
}

impl SetupWizard {
    /// Create a new setup wizard
    pub fn new() -> Self {
        Self {
            step: WizardStep::Welcome,
            config: SiphonConfig::default(),
            cert_pem: String::new(),
            key_pem: String::new(),
            ca_pem: String::new(),
            current_input: String::new(),
            cursor_position: 0,
            error_message: None,
            tunnel_type_index: 0,
            multiline_mode: false,
            multiline_buffer: Vec::new(),
        }
    }

    /// Run the setup wizard
    pub fn run(&mut self) -> anyhow::Result<Option<SiphonConfig>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        terminal.clear()?;

        let result = self.run_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    fn run_loop<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> anyhow::Result<Option<SiphonConfig>> {
        loop {
            terminal.draw(|f| self.render(f))?;

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Global escape
                if key.code == KeyCode::Esc && !self.multiline_mode {
                    return Ok(None); // Cancelled
                }

                self.handle_input(key.code, key.modifiers)?;

                if self.step == WizardStep::Complete {
                    return Ok(Some(self.config.clone()));
                }
            }
        }
    }

    fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(10),   // Content
                Constraint::Length(3), // Help
            ])
            .split(frame.area());

        // Title bar
        self.render_title(frame, chunks[0]);

        // Main content based on step
        match self.step {
            WizardStep::Welcome => self.render_welcome(frame, chunks[1]),
            WizardStep::ServerAddress => {
                self.render_text_input(frame, chunks[1], "Server Address",
                    "Enter the tunnel server address (e.g., tunnel.example.com:4443)")
            }
            WizardStep::LocalAddress => {
                self.render_text_input(frame, chunks[1], "Local Address",
                    "Enter the local service address to forward to (e.g., 127.0.0.1:3000)")
            }
            WizardStep::Subdomain => {
                self.render_text_input(frame, chunks[1], "Subdomain (optional)",
                    "Enter a subdomain or leave blank for auto-generation")
            }
            WizardStep::TunnelType => self.render_tunnel_type_select(frame, chunks[1]),
            WizardStep::CertificateInput => {
                self.render_file_input(frame, chunks[1], "Client Certificate",
                    "Enter file path to your certificate PEM file")
            }
            WizardStep::KeyInput => {
                self.render_file_input(frame, chunks[1], "Private Key",
                    "Enter file path to your private key PEM file")
            }
            WizardStep::CaCertInput => {
                self.render_file_input(frame, chunks[1], "CA Certificate",
                    "Enter file path to the CA certificate PEM file")
            }
            WizardStep::Review => self.render_review(frame, chunks[1]),
            WizardStep::Saving => self.render_saving(frame, chunks[1]),
            WizardStep::Complete => self.render_complete(frame, chunks[1]),
        }

        // Help bar
        self.render_help(frame, chunks[2]);
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let step_num = match self.step {
            WizardStep::Welcome => 0,
            WizardStep::ServerAddress => 1,
            WizardStep::LocalAddress => 2,
            WizardStep::Subdomain => 3,
            WizardStep::TunnelType => 4,
            WizardStep::CertificateInput => 5,
            WizardStep::KeyInput => 6,
            WizardStep::CaCertInput => 7,
            WizardStep::Review => 8,
            WizardStep::Saving | WizardStep::Complete => 9,
        };

        let title = format!(" Siphon Setup Wizard - Step {}/9 ", step_num);
        let block = Block::default()
            .title(title)
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        frame.render_widget(block, area);
    }

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Welcome to Siphon Setup!",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("This wizard will help you configure your Siphon tunnel client."),
            Line::from(""),
            Line::from("You will need:"),
            Line::from("  • Tunnel server address"),
            Line::from("  • Local service address to expose"),
            Line::from("  • TLS certificates (client cert, key, and CA cert)"),
            Line::from(""),
            Line::from("Your secrets will be stored securely in the OS keychain."),
            Line::from("Configuration will be saved to ~/.config/siphon/config.toml"),
            Line::from(""),
            Line::from(Span::styled(
                "Press Enter to begin...",
                Style::default().fg(Color::Yellow),
            )),
        ];

        let para = Paragraph::new(text);
        frame.render_widget(para, inner);
    }

    fn render_text_input(&self, frame: &mut Frame, area: Rect, title: &str, hint: &str) {
        let block = Block::default().borders(Borders::ALL).title(format!(" {} ", title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // Hint
                Constraint::Length(3), // Input
                Constraint::Length(2), // Error
                Constraint::Min(0),    // Rest
            ])
            .split(inner);

        // Hint
        let hint_para = Paragraph::new(hint).style(Style::default().fg(Color::Gray));
        frame.render_widget(hint_para, chunks[0]);

        // Input field
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let input = Paragraph::new(self.current_input.as_str())
            .block(input_block);
        frame.render_widget(input, chunks[1]);

        // Show cursor
        frame.set_cursor_position((
            chunks[1].x + 1 + self.cursor_position as u16,
            chunks[1].y + 1,
        ));

        // Error message
        if let Some(ref err) = self.error_message {
            let error = Paragraph::new(err.as_str()).style(Style::default().fg(Color::Red));
            frame.render_widget(error, chunks[2]);
        }
    }

    fn render_file_input(&self, frame: &mut Frame, area: Rect, title: &str, hint: &str) {
        // Same as text_input but with different styling
        self.render_text_input(frame, area, title, hint);
    }

    fn render_tunnel_type_select(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tunnel Type ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = vec!["HTTP", "TCP"]
            .iter()
            .enumerate()
            .map(|(i, &item)| {
                let style = if i == self.tunnel_type_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let prefix = if i == self.tunnel_type_index {
                    "▶ "
                } else {
                    "  "
                };
                ListItem::new(format!("{}{}", prefix, item)).style(style)
            })
            .collect();

        let hint = Paragraph::new("Use ↑/↓ to select, Enter to confirm")
            .style(Style::default().fg(Color::Gray));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(4)])
            .split(inner);

        frame.render_widget(hint, chunks[0]);

        let list = List::new(items);
        frame.render_widget(list, chunks[1]);
    }

    fn render_review(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Review Configuration ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("Server Address: ", Style::default().fg(Color::Gray)),
                Span::raw(&self.config.server_addr),
            ]),
            Line::from(vec![
                Span::styled("Local Address:  ", Style::default().fg(Color::Gray)),
                Span::raw(&self.config.local_addr),
            ]),
            Line::from(vec![
                Span::styled("Subdomain:      ", Style::default().fg(Color::Gray)),
                Span::raw(
                    self.config
                        .subdomain
                        .as_deref()
                        .unwrap_or("(auto-generate)"),
                ),
            ]),
            Line::from(vec![
                Span::styled("Tunnel Type:    ", Style::default().fg(Color::Gray)),
                Span::raw(&self.config.tunnel_type),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Certificate:    ", Style::default().fg(Color::Gray)),
                Span::styled("✓ Loaded", Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled("Private Key:    ", Style::default().fg(Color::Gray)),
                Span::styled("✓ Loaded", Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled("CA Certificate: ", Style::default().fg(Color::Gray)),
                Span::styled("✓ Loaded", Style::default().fg(Color::Green)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Secrets will be stored in OS keychain.",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(Span::styled(
                "Config will be saved to ~/.config/siphon/config.toml",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Gray)),
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::styled(" to save, or ", Style::default().fg(Color::Gray)),
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::styled(" to cancel", Style::default().fg(Color::Gray)),
            ]),
        ];

        let para = Paragraph::new(text);
        frame.render_widget(para, inner);
    }

    fn render_saving(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Saving configuration...",
                Style::default().fg(Color::Yellow),
            )),
        ];

        let para = Paragraph::new(text);
        frame.render_widget(para, inner);
    }

    fn render_complete(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                "✓ Configuration saved successfully!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Your configuration has been saved to:"),
            Line::from(Span::styled(
                "  ~/.config/siphon/config.toml",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from("Your secrets have been stored in the OS keychain."),
            Line::from(""),
            Line::from("You can now run 'siphon' to start the tunnel."),
            Line::from(""),
            Line::from(Span::styled(
                "Press Enter to exit...",
                Style::default().fg(Color::Yellow),
            )),
        ];

        let para = Paragraph::new(text);
        frame.render_widget(para, inner);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let help_text = match self.step {
            WizardStep::Welcome => "[Enter] Continue  [Esc] Cancel",
            WizardStep::TunnelType => "[↑/↓] Select  [Enter] Confirm  [Esc] Cancel",
            WizardStep::Review => "[Enter] Save  [Esc] Cancel",
            WizardStep::Complete => "[Enter] Exit",
            _ => "[Enter] Next  [Esc] Cancel",
        };

        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let para = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(para, inner);
    }

    fn handle_input(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> anyhow::Result<()> {
        self.error_message = None;

        match self.step {
            WizardStep::Welcome => {
                if key == KeyCode::Enter {
                    self.step = WizardStep::ServerAddress;
                    self.current_input.clear();
                }
            }
            WizardStep::ServerAddress => {
                if self.handle_text_input(key) {
                    if self.current_input.is_empty() {
                        self.error_message = Some("Server address is required".to_string());
                    } else {
                        self.config.server_addr = self.current_input.clone();
                        self.current_input.clear();
                        self.cursor_position = 0;
                        self.step = WizardStep::LocalAddress;
                        self.current_input = self.config.local_addr.clone();
                        self.cursor_position = self.current_input.len();
                    }
                }
            }
            WizardStep::LocalAddress => {
                if self.handle_text_input(key) {
                    if self.current_input.is_empty() {
                        self.error_message = Some("Local address is required".to_string());
                    } else {
                        self.config.local_addr = self.current_input.clone();
                        self.current_input.clear();
                        self.cursor_position = 0;
                        self.step = WizardStep::Subdomain;
                    }
                }
            }
            WizardStep::Subdomain => {
                if self.handle_text_input(key) {
                    self.config.subdomain = if self.current_input.is_empty() {
                        None
                    } else {
                        Some(self.current_input.clone())
                    };
                    self.current_input.clear();
                    self.cursor_position = 0;
                    self.step = WizardStep::TunnelType;
                }
            }
            WizardStep::TunnelType => match key {
                KeyCode::Up => {
                    self.tunnel_type_index = self.tunnel_type_index.saturating_sub(1);
                }
                KeyCode::Down => {
                    self.tunnel_type_index = (self.tunnel_type_index + 1).min(1);
                }
                KeyCode::Enter => {
                    self.config.tunnel_type = if self.tunnel_type_index == 0 {
                        "http".to_string()
                    } else {
                        "tcp".to_string()
                    };
                    self.step = WizardStep::CertificateInput;
                }
                _ => {}
            },
            WizardStep::CertificateInput => {
                if self.handle_text_input(key) {
                    match self.load_file_content(&self.current_input.clone()) {
                        Ok(content) => {
                            if !content.contains("-----BEGIN CERTIFICATE-----") {
                                self.error_message =
                                    Some("Invalid certificate: must be PEM format".to_string());
                            } else {
                                self.cert_pem = content;
                                self.current_input.clear();
                                self.cursor_position = 0;
                                self.step = WizardStep::KeyInput;
                            }
                        }
                        Err(e) => {
                            self.error_message = Some(format!("Failed to read file: {}", e));
                        }
                    }
                }
            }
            WizardStep::KeyInput => {
                if self.handle_text_input(key) {
                    match self.load_file_content(&self.current_input.clone()) {
                        Ok(content) => {
                            if !content.contains("-----BEGIN")
                                || !content.contains("PRIVATE KEY-----")
                            {
                                self.error_message =
                                    Some("Invalid key: must be PEM format".to_string());
                            } else {
                                self.key_pem = content;
                                self.current_input.clear();
                                self.cursor_position = 0;
                                self.step = WizardStep::CaCertInput;
                            }
                        }
                        Err(e) => {
                            self.error_message = Some(format!("Failed to read file: {}", e));
                        }
                    }
                }
            }
            WizardStep::CaCertInput => {
                if self.handle_text_input(key) {
                    match self.load_file_content(&self.current_input.clone()) {
                        Ok(content) => {
                            if !content.contains("-----BEGIN CERTIFICATE-----") {
                                self.error_message =
                                    Some("Invalid CA certificate: must be PEM format".to_string());
                            } else {
                                self.ca_pem = content;
                                self.current_input.clear();
                                self.cursor_position = 0;
                                self.step = WizardStep::Review;
                            }
                        }
                        Err(e) => {
                            self.error_message = Some(format!("Failed to read file: {}", e));
                        }
                    }
                }
            }
            WizardStep::Review => {
                if key == KeyCode::Enter {
                    self.step = WizardStep::Saving;
                    self.save_config()?;
                    self.step = WizardStep::Complete;
                }
            }
            WizardStep::Complete => {
                if key == KeyCode::Enter {
                    // Will exit the loop
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Handle text input, returns true if Enter was pressed
    fn handle_text_input(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Enter => true,
            KeyCode::Char(c) => {
                self.current_input.insert(self.cursor_position, c);
                self.cursor_position += 1;
                false
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                    self.current_input.remove(self.cursor_position);
                }
                false
            }
            KeyCode::Delete => {
                if self.cursor_position < self.current_input.len() {
                    self.current_input.remove(self.cursor_position);
                }
                false
            }
            KeyCode::Left => {
                self.cursor_position = self.cursor_position.saturating_sub(1);
                false
            }
            KeyCode::Right => {
                self.cursor_position = (self.cursor_position + 1).min(self.current_input.len());
                false
            }
            KeyCode::Home => {
                self.cursor_position = 0;
                false
            }
            KeyCode::End => {
                self.cursor_position = self.current_input.len();
                false
            }
            _ => false,
        }
    }

    fn load_file_content(&self, path: &str) -> anyhow::Result<String> {
        let expanded = shellexpand::tilde(path);
        let content = std::fs::read_to_string(expanded.as_ref())?;
        Ok(content)
    }

    fn save_config(&mut self) -> anyhow::Result<()> {
        // Store secrets in keychain
        siphon_secrets::keychain::store("siphon", "cert", &self.cert_pem)?;
        siphon_secrets::keychain::store("siphon", "key", &self.key_pem)?;
        siphon_secrets::keychain::store("siphon", "ca", &self.ca_pem)?;

        // Update config with keychain references
        self.config.cert = "keychain://siphon/cert".to_string();
        self.config.key = "keychain://siphon/key".to_string();
        self.config.ca_cert = "keychain://siphon/ca".to_string();

        // Save config file
        self.config.save_default()?;

        Ok(())
    }
}

impl Default for SetupWizard {
    fn default() -> Self {
        Self::new()
    }
}
