//! Reusable TUI widget components
//!
//! This module contains helper widgets and components used across the TUI.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render a centered message in an area
pub fn render_centered_message(frame: &mut Frame, area: Rect, message: &str, style: Style) {
    let para = Paragraph::new(message)
        .style(style)
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(para, area);
}

/// Render an error message with red styling
pub fn render_error(frame: &mut Frame, area: Rect, error: &str) {
    let block = Block::default()
        .title(" Error ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let para = Paragraph::new(error).style(Style::default().fg(Color::Red));
    frame.render_widget(para, inner);
}

/// Render a loading spinner/message
pub fn render_loading(frame: &mut Frame, area: Rect, message: &str) {
    let block = Block::default()
        .title(" Loading ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let para = Paragraph::new(message)
        .style(Style::default().fg(Color::Yellow))
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(para, inner);
}

/// Create a styled key hint (e.g., "[q] Quit")
pub fn key_hint(key: &str, action: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("[", Style::default().fg(Color::DarkGray)),
        Span::styled(key.to_string(), Style::default().fg(Color::Yellow)),
        Span::styled("] ", Style::default().fg(Color::DarkGray)),
        Span::styled(action.to_string(), Style::default().fg(Color::Gray)),
    ])
}

/// Create multiple key hints on a single line
pub fn key_hints(hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled("[", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(key.to_string(), Style::default().fg(Color::Yellow)));
        spans.push(Span::styled("] ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(action.to_string(), Style::default().fg(Color::Gray)));
    }
    Line::from(spans)
}

/// Status indicator colors
pub fn status_color(status: u16) -> Color {
    match status {
        200..=299 => Color::Green,
        300..=399 => Color::Blue,
        400..=499 => Color::Yellow,
        _ => Color::Red,
    }
}

/// Method colors for HTTP methods
pub fn method_color(method: &str) -> Color {
    match method.to_uppercase().as_str() {
        "GET" => Color::Green,
        "POST" => Color::Blue,
        "PUT" => Color::Yellow,
        "DELETE" => Color::Red,
        "PATCH" => Color::Magenta,
        _ => Color::Gray,
    }
}
