use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use siphon_protocol::{ServerMessage, TunnelType};

/// Handle to a tunnel connection
pub struct TunnelHandle {
    /// Channel to send messages to this tunnel
    pub sender: mpsc::Sender<ServerMessage>,
    /// Client identifier (from certificate CN)
    #[allow(dead_code)]
    pub client_id: String,
    /// Type of tunnel
    #[allow(dead_code)]
    pub tunnel_type: TunnelType,
    /// Cloudflare DNS record ID (for cleanup)
    pub dns_record_id: Option<String>,
}

/// Routes incoming requests to appropriate tunnel connections
pub struct Router {
    /// Subdomain -> tunnel handle mapping
    routes: DashMap<String, TunnelHandle>,
    /// TCP port -> subdomain mapping (for TCP tunnels)
    tcp_ports: DashMap<u16, String>,
}

impl Router {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            routes: DashMap::new(),
            tcp_ports: DashMap::new(),
        })
    }

    /// Register a new tunnel
    pub fn register(
        &self,
        subdomain: String,
        handle: TunnelHandle,
        tcp_port: Option<u16>,
    ) -> Result<(), RouterError> {
        // Check if subdomain is already taken
        if self.routes.contains_key(&subdomain) {
            return Err(RouterError::SubdomainTaken(subdomain));
        }

        // Register TCP port if applicable
        if let Some(port) = tcp_port {
            self.tcp_ports.insert(port, subdomain.clone());
        }

        self.routes.insert(subdomain, handle);
        Ok(())
    }

    /// Unregister a tunnel
    pub fn unregister(&self, subdomain: &str) -> Option<TunnelHandle> {
        if let Some((_, handle)) = self.routes.remove(subdomain) {
            // Remove TCP port mapping if exists
            self.tcp_ports.retain(|_, v| v != subdomain);
            Some(handle)
        } else {
            None
        }
    }

    /// Get a sender for a subdomain
    pub fn get_sender(&self, subdomain: &str) -> Option<mpsc::Sender<ServerMessage>> {
        self.routes.get(subdomain).map(|h| h.sender.clone())
    }

    /// Get subdomain for a TCP port
    #[allow(dead_code)]
    pub fn get_subdomain_for_port(&self, port: u16) -> Option<String> {
        self.tcp_ports.get(&port).map(|s| s.clone())
    }

    /// Check if a subdomain is available
    pub fn is_available(&self, subdomain: &str) -> bool {
        !self.routes.contains_key(subdomain)
    }

    /// List all active subdomains
    #[allow(dead_code)]
    pub fn list_subdomains(&self) -> Vec<String> {
        self.routes.iter().map(|r| r.key().clone()).collect()
    }
}

impl Default for Router {
    fn default() -> Self {
        Self {
            routes: DashMap::new(),
            tcp_ports: DashMap::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("Subdomain already taken: {0}")]
    SubdomainTaken(String),
}
