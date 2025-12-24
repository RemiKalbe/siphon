use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot};

/// Data for an HTTP response from a tunnel client
#[derive(Debug)]
pub struct HttpResponseData {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Shared registry for pending HTTP responses
/// Maps stream_id -> response sender channel
pub type ResponseRegistry = Arc<DashMap<u64, oneshot::Sender<HttpResponseData>>>;

/// Create a new response registry
pub fn new_response_registry() -> ResponseRegistry {
    Arc::new(DashMap::new())
}

/// Handle to a TCP connection's write half and associated data
pub struct TcpConnectionHandle {
    pub writer: mpsc::Sender<Vec<u8>>,
    #[allow(dead_code)]
    pub subdomain: String,
}

/// Shared registry for TCP connections
/// Maps stream_id -> TCP connection handle
pub type TcpConnectionRegistry = Arc<DashMap<u64, TcpConnectionHandle>>;

/// Create a new TCP connection registry
pub fn new_tcp_connection_registry() -> TcpConnectionRegistry {
    Arc::new(DashMap::new())
}

/// Port allocator for TCP tunnels
pub struct PortAllocator {
    start: u16,
    end: u16,
    allocated: RwLock<std::collections::HashSet<u16>>,
}

impl PortAllocator {
    pub fn new(start: u16, end: u16) -> Arc<Self> {
        Arc::new(Self {
            start,
            end,
            allocated: RwLock::new(std::collections::HashSet::new()),
        })
    }

    /// Allocate the next available port
    pub fn allocate(&self) -> Option<u16> {
        let mut allocated = self.allocated.write();
        for port in self.start..=self.end {
            if !allocated.contains(&port) {
                allocated.insert(port);
                return Some(port);
            }
        }
        None
    }

    /// Release a port back to the pool
    pub fn release(&self, port: u16) {
        let mut allocated = self.allocated.write();
        allocated.remove(&port);
    }

    /// Check if a port is allocated
    #[allow(dead_code)]
    pub fn is_allocated(&self, port: u16) -> bool {
        self.allocated.read().contains(&port)
    }
}

/// Global stream ID counter shared across all planes
pub struct StreamIdGenerator {
    counter: AtomicU64,
}

impl StreamIdGenerator {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            counter: AtomicU64::new(1),
        })
    }

    pub fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for StreamIdGenerator {
    fn default() -> Self {
        Self {
            counter: AtomicU64::new(1),
        }
    }
}
