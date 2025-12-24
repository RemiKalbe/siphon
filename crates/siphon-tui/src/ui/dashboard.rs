//! Dashboard layout and rendering with graphs

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Bar, BarChart, BarGroup, Block, Borders, Cell, Chart, Dataset, GraphType,
        Paragraph, Row, Sparkline, Table,
    },
    Frame,
};
use std::time::Duration;

use crate::metrics::MetricsSnapshot;

/// Dashboard renderer
pub struct Dashboard;

impl Dashboard {
    /// Render the complete dashboard
    pub fn render(frame: &mut Frame, snapshot: &MetricsSnapshot) {
        // Main layout: 4 rows
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),  // Header: Tunnel Info
                Constraint::Length(10), // Middle top: Request rate + Response times
                Constraint::Length(8),  // Middle bottom: Status codes + Throughput
                Constraint::Min(8),     // Bottom: Live log
            ])
            .split(frame.area());

        // Header: Tunnel info panel
        Self::render_tunnel_info(frame, main_chunks[0], snapshot);

        // Middle top: 2-column layout
        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);

        Self::render_request_rate(frame, top_chunks[0], snapshot);
        Self::render_response_times(frame, top_chunks[1], snapshot);

        // Middle bottom: 2-column layout
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[2]);

        Self::render_status_codes(frame, bottom_chunks[0], snapshot);
        Self::render_throughput(frame, bottom_chunks[1], snapshot);

        // Bottom: Live request log
        Self::render_live_log(frame, main_chunks[3], snapshot);
    }

    fn render_tunnel_info(frame: &mut Frame, area: Rect, snapshot: &MetricsSnapshot) {
        let block = Block::default()
            .title(" Siphon - Tunnel Status ")
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if let Some(ref info) = snapshot.tunnel_info {
            let uptime = snapshot
                .uptime
                .map(format_duration)
                .unwrap_or_else(|| "N/A".to_string());

            let tunnel_type = format!("{:?}", info.tunnel_type);

            let text = vec![
                Line::from(vec![
                    Span::styled("URL: ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        &info.url,
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Subdomain: ", Style::default().fg(Color::Gray)),
                    Span::raw(&info.subdomain),
                    Span::raw("  │  "),
                    Span::styled("Uptime: ", Style::default().fg(Color::Gray)),
                    Span::raw(&uptime),
                    Span::raw("  │  "),
                    Span::styled("Type: ", Style::default().fg(Color::Gray)),
                    Span::raw(&tunnel_type),
                ]),
                Line::from(vec![
                    Span::styled("Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled("q", Style::default().fg(Color::Yellow)),
                    Span::styled(" or ", Style::default().fg(Color::DarkGray)),
                    Span::styled("Esc", Style::default().fg(Color::Yellow)),
                    Span::styled(" to quit", Style::default().fg(Color::DarkGray)),
                ]),
            ];

            let para = Paragraph::new(text);
            frame.render_widget(para, inner);
        } else {
            let text = vec![
                Line::from(Span::styled(
                    "Connecting to tunnel server...",
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled("q", Style::default().fg(Color::Yellow)),
                    Span::styled(" or ", Style::default().fg(Color::DarkGray)),
                    Span::styled("Esc", Style::default().fg(Color::Yellow)),
                    Span::styled(" to quit", Style::default().fg(Color::DarkGray)),
                ]),
            ];
            let para = Paragraph::new(text);
            frame.render_widget(para, inner);
        }
    }

    fn render_request_rate(frame: &mut Frame, area: Rect, snapshot: &MetricsSnapshot) {
        let block = Block::default()
            .title(" Request Rate (last 60s) ")
            .borders(Borders::ALL);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into sparkline and stats
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(2)])
            .split(inner);

        // Sparkline for request rate
        let data: Vec<u64> = snapshot.request_rate_history.clone();
        let max_val = data.iter().max().copied().unwrap_or(1).max(1);

        let sparkline = Sparkline::default()
            .data(&data)
            .max(max_val)
            .style(Style::default().fg(Color::Cyan));

        frame.render_widget(sparkline, chunks[0]);

        // Stats line
        let stats = Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format_number(snapshot.total_requests),
                Style::default().fg(Color::White),
            ),
            Span::raw("  │  "),
            Span::styled("Rate: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.1} req/s", snapshot.requests_per_second),
                Style::default().fg(Color::Cyan),
            ),
        ]);

        let stats_para = Paragraph::new(stats);
        frame.render_widget(stats_para, chunks[1]);
    }

    fn render_response_times(frame: &mut Frame, area: Rect, snapshot: &MetricsSnapshot) {
        let block = Block::default()
            .title(" Response Times (last 60s) ")
            .borders(Borders::ALL);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into chart and stats
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(inner);

        // Prepare data for chart
        let p50_data: Vec<(f64, f64)> = snapshot
            .response_time_p50_history
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v as f64))
            .collect();

        let p99_data: Vec<(f64, f64)> = snapshot
            .response_time_p99_history
            .iter()
            .enumerate()
            .map(|(i, &v)| (i as f64, v as f64))
            .collect();

        let max_time = snapshot
            .response_time_p99_history
            .iter()
            .max()
            .copied()
            .unwrap_or(100)
            .max(100) as f64;

        let datasets = vec![
            Dataset::default()
                .name("P50")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Green))
                .data(&p50_data),
            Dataset::default()
                .name("P99")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Yellow))
                .data(&p99_data),
        ];

        let y_labels: Vec<Line> = vec![
            Line::from("0"),
            Line::from(format!("{}ms", max_time as u64)),
        ];
        let chart = Chart::new(datasets)
            .x_axis(Axis::default().bounds([0.0, 60.0]))
            .y_axis(
                Axis::default()
                    .bounds([0.0, max_time])
                    .labels(y_labels),
            );

        frame.render_widget(chart, chunks[0]);

        // Stats
        let rt = &snapshot.response_times;
        let stats = vec![
            Line::from(vec![
                Span::styled("P50: ", Style::default().fg(Color::Green)),
                Span::raw(rt.p50.map(format_duration_ms).unwrap_or_else(|| "-".into())),
                Span::raw("  │  "),
                Span::styled("P99: ", Style::default().fg(Color::Yellow)),
                Span::raw(rt.p99.map(format_duration_ms).unwrap_or_else(|| "-".into())),
            ]),
            Line::from(vec![
                Span::styled("Min: ", Style::default().fg(Color::Gray)),
                Span::raw(rt.min.map(format_duration_ms).unwrap_or_else(|| "-".into())),
                Span::raw("  │  "),
                Span::styled("Max: ", Style::default().fg(Color::Gray)),
                Span::raw(rt.max.map(format_duration_ms).unwrap_or_else(|| "-".into())),
            ]),
        ];

        let stats_para = Paragraph::new(stats);
        frame.render_widget(stats_para, chunks[1]);
    }

    fn render_status_codes(frame: &mut Frame, area: Rect, snapshot: &MetricsSnapshot) {
        let block = Block::default()
            .title(" Status Codes ")
            .borders(Borders::ALL);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let status = &snapshot.status_distribution;

        // Create bar chart
        let bars = vec![
            Bar::default()
                .value(status.code_2xx)
                .label("2xx".into())
                .style(Style::default().fg(Color::Green)),
            Bar::default()
                .value(status.code_3xx)
                .label("3xx".into())
                .style(Style::default().fg(Color::Blue)),
            Bar::default()
                .value(status.code_4xx)
                .label("4xx".into())
                .style(Style::default().fg(Color::Yellow)),
            Bar::default()
                .value(status.code_5xx)
                .label("5xx".into())
                .style(Style::default().fg(Color::Red)),
        ];

        let bar_chart = BarChart::default()
            .data(BarGroup::default().bars(&bars))
            .bar_width(6)
            .bar_gap(2)
            .value_style(Style::default().fg(Color::White));

        frame.render_widget(bar_chart, inner);
    }

    fn render_throughput(frame: &mut Frame, area: Rect, snapshot: &MetricsSnapshot) {
        let block = Block::default()
            .title(" Throughput ")
            .borders(Borders::ALL);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into sparklines and stats
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // In sparkline
                Constraint::Length(2), // Out sparkline
                Constraint::Min(1),    // Stats
            ])
            .split(inner);

        // Bytes In sparkline
        let in_data: Vec<u64> = snapshot.bytes_in_rate_history.clone();
        let in_max = in_data.iter().max().copied().unwrap_or(1).max(1);

        let in_label = Line::from(vec![
            Span::styled("In:  ", Style::default().fg(Color::Gray)),
        ]);
        frame.render_widget(Paragraph::new(in_label), chunks[0]);

        let in_sparkline_area = Rect {
            x: chunks[0].x + 5,
            y: chunks[0].y,
            width: chunks[0].width.saturating_sub(5),
            height: chunks[0].height,
        };

        let in_sparkline = Sparkline::default()
            .data(&in_data)
            .max(in_max)
            .style(Style::default().fg(Color::Cyan));
        frame.render_widget(in_sparkline, in_sparkline_area);

        // Bytes Out sparkline
        let out_data: Vec<u64> = snapshot.bytes_out_rate_history.clone();
        let out_max = out_data.iter().max().copied().unwrap_or(1).max(1);

        let out_label = Line::from(vec![
            Span::styled("Out: ", Style::default().fg(Color::Gray)),
        ]);
        frame.render_widget(Paragraph::new(out_label), chunks[1]);

        let out_sparkline_area = Rect {
            x: chunks[1].x + 5,
            y: chunks[1].y,
            width: chunks[1].width.saturating_sub(5),
            height: chunks[1].height,
        };

        let out_sparkline = Sparkline::default()
            .data(&out_data)
            .max(out_max)
            .style(Style::default().fg(Color::Magenta));
        frame.render_widget(out_sparkline, out_sparkline_area);

        // Stats
        let stats = Line::from(vec![
            Span::styled("Total In: ", Style::default().fg(Color::Gray)),
            Span::styled(format_bytes(snapshot.bytes_in), Style::default().fg(Color::Cyan)),
            Span::raw(" │ "),
            Span::styled("Out: ", Style::default().fg(Color::Gray)),
            Span::styled(format_bytes(snapshot.bytes_out), Style::default().fg(Color::Magenta)),
            Span::raw(" │ "),
            Span::styled("Conn: ", Style::default().fg(Color::Gray)),
            Span::raw(snapshot.active_connections.to_string()),
        ]);

        let stats_para = Paragraph::new(stats);
        frame.render_widget(stats_para, chunks[2]);
    }

    fn render_live_log(frame: &mut Frame, area: Rect, snapshot: &MetricsSnapshot) {
        let block = Block::default()
            .title(" Live Requests ")
            .borders(Borders::ALL);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Table header
        let header = Row::new(vec![
            Cell::from("Time"),
            Cell::from("Method"),
            Cell::from("URI"),
            Cell::from("Status"),
            Cell::from("Duration"),
            Cell::from("Size"),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(0);

        // Table rows (most recent first)
        let rows: Vec<Row> = snapshot
            .recent_requests
            .iter()
            .rev()
            .take(inner.height.saturating_sub(1) as usize)
            .map(|req| {
                let status_style = match req.status {
                    200..=299 => Style::default().fg(Color::Green),
                    300..=399 => Style::default().fg(Color::Blue),
                    400..=499 => Style::default().fg(Color::Yellow),
                    _ => Style::default().fg(Color::Red),
                };

                Row::new(vec![
                    Cell::from(req.timestamp.format("%H:%M:%S").to_string()),
                    Cell::from(req.method.clone()),
                    Cell::from(truncate(&req.uri, 35)),
                    Cell::from(Span::styled(req.status.to_string(), status_style)),
                    Cell::from(format_duration_ms(req.duration)),
                    Cell::from(format_bytes(req.bytes as u64)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(20),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(10),
        ];

        let table = Table::new(rows, widths).header(header);

        frame.render_widget(table, inner);
    }
}

// Helper functions

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn format_duration_ms(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

fn format_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{}B", b)
    } else if b < 1024 * 1024 {
        format!("{:.1}KB", b as f64 / 1024.0)
    } else if b < 1024 * 1024 * 1024 {
        format!("{:.1}MB", b as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}GB", b as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_number(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

fn truncate(s: &str, len: usize) -> String {
    if s.len() <= len {
        s.to_string()
    } else {
        format!("{}...", &s[..len.saturating_sub(3)])
    }
}
