//! Thread-safe metrics collection for real-time TUI dashboard

use parking_lot::RwLock;
use siphon_protocol::TunnelType;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Maximum samples to keep in time-series data (60 seconds at 1 sample/sec)
const HISTORY_SIZE: usize = 60;

/// Maximum recent requests to display in live log
const MAX_RECENT_REQUESTS: usize = 100;

/// Thread-safe metrics collector that can be updated from async tasks
#[derive(Clone)]
pub struct MetricsCollector {
    inner: Arc<RwLock<MetricsState>>,
}

/// Internal metrics state
pub struct MetricsState {
    // Tunnel info
    pub tunnel_info: Option<TunnelInfo>,
    pub connected_at: Option<Instant>,

    // Request metrics
    pub total_requests: u64,
    pub requests_in_progress: u64,
    pub status_codes: StatusCodeDistribution,

    // Response time tracking
    response_times: VecDeque<Duration>,

    // Connection metrics
    pub active_tcp_connections: u64,
    pub total_tcp_connections: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,

    // Error tracking
    pub error_count: u64,
    pub last_error: Option<String>,

    // Recent requests for live log
    pub recent_requests: VecDeque<RequestLogEntry>,

    // Time-series data for graphs (rolling windows)
    pub request_rate_history: VecDeque<u64>,
    pub response_time_p50_history: VecDeque<u64>,
    pub response_time_p99_history: VecDeque<u64>,
    pub bytes_in_rate_history: VecDeque<u64>,
    pub bytes_out_rate_history: VecDeque<u64>,

    // Counters for rate calculation (reset each second)
    requests_this_second: u64,
    bytes_in_this_second: u64,
    bytes_out_this_second: u64,
    last_tick: Instant,
}

/// Information about the established tunnel
#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub subdomain: String,
    pub url: String,
    pub port: Option<u16>,
    pub tunnel_type: TunnelType,
}

/// Distribution of HTTP status codes
#[derive(Debug, Clone, Default)]
pub struct StatusCodeDistribution {
    pub code_2xx: u64,
    pub code_3xx: u64,
    pub code_4xx: u64,
    pub code_5xx: u64,
}

/// Statistics about response times
#[derive(Debug, Clone, Default)]
pub struct ResponseTimeStats {
    pub min: Option<Duration>,
    pub max: Option<Duration>,
    pub avg: Option<Duration>,
    pub p50: Option<Duration>,
    pub p95: Option<Duration>,
    pub p99: Option<Duration>,
}

/// Entry in the live request log
#[derive(Debug, Clone)]
pub struct RequestLogEntry {
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub method: String,
    pub uri: String,
    pub status: u16,
    pub duration: Duration,
    pub bytes: usize,
}

/// Immutable snapshot of metrics for rendering
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub tunnel_info: Option<TunnelInfo>,
    pub uptime: Option<Duration>,
    pub total_requests: u64,
    pub requests_per_second: f64,
    pub status_distribution: StatusCodeDistribution,
    pub response_times: ResponseTimeStats,
    pub active_connections: u64,
    pub total_connections: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub error_count: u64,
    pub last_error: Option<String>,
    pub recent_requests: Vec<RequestLogEntry>,

    // Graph data
    pub request_rate_history: Vec<u64>,
    pub response_time_p50_history: Vec<u64>,
    pub response_time_p99_history: Vec<u64>,
    pub bytes_in_rate_history: Vec<u64>,
    pub bytes_out_rate_history: Vec<u64>,
}

impl Default for MetricsState {
    fn default() -> Self {
        Self {
            tunnel_info: None,
            connected_at: None,
            total_requests: 0,
            requests_in_progress: 0,
            status_codes: StatusCodeDistribution::default(),
            response_times: VecDeque::with_capacity(1000),
            active_tcp_connections: 0,
            total_tcp_connections: 0,
            bytes_in: 0,
            bytes_out: 0,
            error_count: 0,
            last_error: None,
            recent_requests: VecDeque::with_capacity(MAX_RECENT_REQUESTS),
            request_rate_history: VecDeque::with_capacity(HISTORY_SIZE),
            response_time_p50_history: VecDeque::with_capacity(HISTORY_SIZE),
            response_time_p99_history: VecDeque::with_capacity(HISTORY_SIZE),
            bytes_in_rate_history: VecDeque::with_capacity(HISTORY_SIZE),
            bytes_out_rate_history: VecDeque::with_capacity(HISTORY_SIZE),
            requests_this_second: 0,
            bytes_in_this_second: 0,
            bytes_out_this_second: 0,
            last_tick: Instant::now(),
        }
    }
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MetricsState::default())),
        }
    }

    /// Set tunnel information when connection is established
    pub fn set_tunnel_info(&self, info: TunnelInfo) {
        let mut state = self.inner.write();
        state.tunnel_info = Some(info);
        state.connected_at = Some(Instant::now());
    }

    /// Record the start of an HTTP request
    pub fn record_request_start(&self) {
        let mut state = self.inner.write();
        state.requests_in_progress += 1;
    }

    /// Record the completion of an HTTP request
    pub fn record_request_complete(
        &self,
        status: u16,
        duration: Duration,
        bytes: usize,
        method: String,
        uri: String,
    ) {
        let mut state = self.inner.write();

        // Update counters
        state.total_requests += 1;
        state.requests_in_progress = state.requests_in_progress.saturating_sub(1);
        state.requests_this_second += 1;

        // Update status distribution
        match status {
            200..=299 => state.status_codes.code_2xx += 1,
            300..=399 => state.status_codes.code_3xx += 1,
            400..=499 => state.status_codes.code_4xx += 1,
            _ => state.status_codes.code_5xx += 1,
        }

        // Store response time
        state.response_times.push_back(duration);
        if state.response_times.len() > 1000 {
            state.response_times.pop_front();
        }

        // Add to recent requests
        state.recent_requests.push_back(RequestLogEntry {
            timestamp: chrono::Local::now(),
            method,
            uri,
            status,
            duration,
            bytes,
        });
        if state.recent_requests.len() > MAX_RECENT_REQUESTS {
            state.recent_requests.pop_front();
        }
    }

    /// Record a TCP connection being established
    pub fn record_tcp_connect(&self) {
        let mut state = self.inner.write();
        state.active_tcp_connections += 1;
        state.total_tcp_connections += 1;
    }

    /// Record a TCP connection being closed
    pub fn record_tcp_disconnect(&self) {
        let mut state = self.inner.write();
        state.active_tcp_connections = state.active_tcp_connections.saturating_sub(1);
    }

    /// Record bytes received (inbound)
    pub fn record_bytes_in(&self, bytes: u64) {
        let mut state = self.inner.write();
        state.bytes_in += bytes;
        state.bytes_in_this_second += bytes;
    }

    /// Record bytes sent (outbound)
    pub fn record_bytes_out(&self, bytes: u64) {
        let mut state = self.inner.write();
        state.bytes_out += bytes;
        state.bytes_out_this_second += bytes;
    }

    /// Record an error
    pub fn record_error(&self, error: String) {
        let mut state = self.inner.write();
        state.error_count += 1;
        state.last_error = Some(error);
    }

    /// Tick the metrics collector (call once per second to update history)
    pub fn tick(&self) {
        let mut state = self.inner.write();

        // Calculate time since last tick
        let elapsed = state.last_tick.elapsed();
        if elapsed < Duration::from_millis(900) {
            return; // Too soon, skip
        }

        // Update request rate history
        let requests_this_sec = state.requests_this_second;
        state.request_rate_history.push_back(requests_this_sec);
        if state.request_rate_history.len() > HISTORY_SIZE {
            state.request_rate_history.pop_front();
        }

        // Update bytes rate history
        let bytes_in_this_sec = state.bytes_in_this_second;
        state.bytes_in_rate_history.push_back(bytes_in_this_sec);
        if state.bytes_in_rate_history.len() > HISTORY_SIZE {
            state.bytes_in_rate_history.pop_front();
        }

        let bytes_out_this_sec = state.bytes_out_this_second;
        state.bytes_out_rate_history.push_back(bytes_out_this_sec);
        if state.bytes_out_rate_history.len() > HISTORY_SIZE {
            state.bytes_out_rate_history.pop_front();
        }

        // Calculate and store response time percentiles
        let (p50, p99) = calculate_percentiles(&state.response_times);
        state
            .response_time_p50_history
            .push_back(p50.map(|d| d.as_millis() as u64).unwrap_or(0));
        if state.response_time_p50_history.len() > HISTORY_SIZE {
            state.response_time_p50_history.pop_front();
        }

        state
            .response_time_p99_history
            .push_back(p99.map(|d| d.as_millis() as u64).unwrap_or(0));
        if state.response_time_p99_history.len() > HISTORY_SIZE {
            state.response_time_p99_history.pop_front();
        }

        // Reset per-second counters
        state.requests_this_second = 0;
        state.bytes_in_this_second = 0;
        state.bytes_out_this_second = 0;
        state.last_tick = Instant::now();
    }

    /// Get an immutable snapshot of current metrics for rendering
    pub fn snapshot(&self) -> MetricsSnapshot {
        let state = self.inner.read();

        let uptime = state.connected_at.map(|t| t.elapsed());

        // Calculate requests per second (average over last 10 seconds)
        let recent_requests: u64 = state.request_rate_history.iter().rev().take(10).sum();
        let sample_count = state.request_rate_history.len().min(10) as f64;
        let requests_per_second = if sample_count > 0.0 {
            recent_requests as f64 / sample_count
        } else {
            0.0
        };

        // Calculate response time stats
        let response_times = calculate_response_time_stats(&state.response_times);

        MetricsSnapshot {
            tunnel_info: state.tunnel_info.clone(),
            uptime,
            total_requests: state.total_requests,
            requests_per_second,
            status_distribution: state.status_codes.clone(),
            response_times,
            active_connections: state.active_tcp_connections,
            total_connections: state.total_tcp_connections,
            bytes_in: state.bytes_in,
            bytes_out: state.bytes_out,
            error_count: state.error_count,
            last_error: state.last_error.clone(),
            recent_requests: state.recent_requests.iter().cloned().collect(),

            // Graph data - pad to fixed HISTORY_SIZE for consistent chart rendering
            request_rate_history: pad_history(&state.request_rate_history, HISTORY_SIZE),
            response_time_p50_history: pad_history(&state.response_time_p50_history, HISTORY_SIZE),
            response_time_p99_history: pad_history(&state.response_time_p99_history, HISTORY_SIZE),
            bytes_in_rate_history: pad_history(&state.bytes_in_rate_history, HISTORY_SIZE),
            bytes_out_rate_history: pad_history(&state.bytes_out_rate_history, HISTORY_SIZE),
        }
    }
}

/// Pad history data to a fixed size with leading zeros for consistent chart rendering
fn pad_history(data: &VecDeque<u64>, size: usize) -> Vec<u64> {
    let current_len = data.len();
    if current_len >= size {
        // Already at or exceeds target size, just convert
        data.iter().copied().collect()
    } else {
        // Pad with zeros at the beginning, so newest data is on the right
        let mut result = vec![0u64; size - current_len];
        result.extend(data.iter().copied());
        result
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate response time statistics from samples
fn calculate_response_time_stats(samples: &VecDeque<Duration>) -> ResponseTimeStats {
    if samples.is_empty() {
        return ResponseTimeStats::default();
    }

    let mut sorted: Vec<Duration> = samples.iter().copied().collect();
    sorted.sort();

    let min = sorted.first().copied();
    let max = sorted.last().copied();

    let sum: Duration = sorted.iter().sum();
    let avg = Some(sum / sorted.len() as u32);

    let p50 = percentile(&sorted, 50);
    let p95 = percentile(&sorted, 95);
    let p99 = percentile(&sorted, 99);

    ResponseTimeStats {
        min,
        max,
        avg,
        p50,
        p95,
        p99,
    }
}

/// Calculate a specific percentile from sorted samples
fn percentile(sorted: &[Duration], p: usize) -> Option<Duration> {
    if sorted.is_empty() {
        return None;
    }
    let idx = (sorted.len() * p / 100).min(sorted.len() - 1);
    Some(sorted[idx])
}

/// Calculate P50 and P99 from samples
fn calculate_percentiles(samples: &VecDeque<Duration>) -> (Option<Duration>, Option<Duration>) {
    if samples.is_empty() {
        return (None, None);
    }

    let mut sorted: Vec<Duration> = samples.iter().copied().collect();
    sorted.sort();

    (percentile(&sorted, 50), percentile(&sorted, 99))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector() {
        let metrics = MetricsCollector::new();

        metrics.record_request_start();
        metrics.record_request_complete(
            200,
            Duration::from_millis(50),
            1024,
            "GET".into(),
            "/api/test".into(),
        );

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_requests, 1);
        assert_eq!(snapshot.status_distribution.code_2xx, 1);
    }

    #[test]
    fn test_status_code_distribution() {
        let metrics = MetricsCollector::new();

        for status in [200, 201, 301, 404, 500] {
            metrics.record_request_complete(
                status,
                Duration::from_millis(10),
                100,
                "GET".into(),
                "/".into(),
            );
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.status_distribution.code_2xx, 2);
        assert_eq!(snapshot.status_distribution.code_3xx, 1);
        assert_eq!(snapshot.status_distribution.code_4xx, 1);
        assert_eq!(snapshot.status_distribution.code_5xx, 1);
    }

    #[test]
    fn test_response_time_percentiles() {
        let metrics = MetricsCollector::new();

        // Add various response times
        for ms in [10, 20, 30, 40, 50, 60, 70, 80, 90, 100] {
            metrics.record_request_complete(
                200,
                Duration::from_millis(ms),
                100,
                "GET".into(),
                "/".into(),
            );
        }

        let snapshot = metrics.snapshot();
        assert!(snapshot.response_times.p50.is_some());
        assert!(snapshot.response_times.p95.is_some());
        assert!(snapshot.response_times.p99.is_some());
    }

    #[test]
    fn test_tcp_connection_tracking() {
        let metrics = MetricsCollector::new();

        metrics.record_tcp_connect();
        metrics.record_tcp_connect();
        assert_eq!(metrics.snapshot().active_connections, 2);

        metrics.record_tcp_disconnect();
        assert_eq!(metrics.snapshot().active_connections, 1);
    }
}
