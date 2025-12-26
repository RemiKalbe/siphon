//! Main TUI application with event loop

use arboard::Clipboard;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, Terminal};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

use super::dashboard::Dashboard;
use crate::metrics::MetricsCollector;

/// Main TUI application
pub struct TuiApp {
    metrics: MetricsCollector,
    shutdown_tx: mpsc::Sender<()>,
}

impl TuiApp {
    /// Create a new TUI application
    pub fn new(metrics: MetricsCollector, shutdown_tx: mpsc::Sender<()>) -> Self {
        Self {
            metrics,
            shutdown_tx,
        }
    }

    /// Run the TUI event loop (blocking)
    pub async fn run(self) -> io::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Clear terminal
        terminal.clear()?;

        let result = self.run_loop(&mut terminal).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    async fn run_loop(
        &self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        let tick_rate = Duration::from_millis(100);
        let mut last_tick = std::time::Instant::now();
        let mut clipboard = Clipboard::new().ok();
        let mut copy_feedback: Option<(std::time::Instant, bool)> = None;

        loop {
            // Tick metrics for time-series updates (once per second)
            if last_tick.elapsed() >= Duration::from_secs(1) {
                self.metrics.tick();
                last_tick = std::time::Instant::now();
            }

            // Clear copy feedback after 2 seconds
            if let Some((time, _)) = copy_feedback {
                if time.elapsed() > Duration::from_secs(2) {
                    copy_feedback = None;
                }
            }

            // Draw UI
            let snapshot = self.metrics.snapshot();
            let feedback = copy_feedback.map(|(_, success)| success);
            terminal.draw(|f| Dashboard::render(f, &snapshot, feedback))?;

            // Handle events with timeout
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if crossterm::event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Esc => {
                                    let _ = self.shutdown_tx.send(()).await;
                                    return Ok(());
                                }
                                KeyCode::Char('c')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    let _ = self.shutdown_tx.send(()).await;
                                    return Ok(());
                                }
                                KeyCode::Char('c') => {
                                    // Copy tunnel URL to clipboard
                                    if let Some(ref info) = snapshot.tunnel_info {
                                        if let Some(ref mut cb) = clipboard {
                                            let success = cb.set_text(info.url.clone()).is_ok();
                                            copy_feedback =
                                                Some((std::time::Instant::now(), success));
                                        } else {
                                            copy_feedback =
                                                Some((std::time::Instant::now(), false));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Event::Resize(_, _) => {
                        // Force full redraw on resize
                        terminal.clear()?;
                    }
                    _ => {}
                }
            }
        }
    }
}
