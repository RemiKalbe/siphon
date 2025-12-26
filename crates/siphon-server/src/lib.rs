//! Siphon tunnel server library
//!
//! This library provides the core components for running a siphon tunnel server.
//! It can be used to embed a tunnel server in other applications or for testing.

mod cloudflare;
mod config;
mod control_plane;
mod dns_provider;
mod http_plane;
mod router;
mod state;
mod tcp_plane;

// Re-export public types
pub use cloudflare::CloudflareClient;
pub use config::{ResolvedCloudflareConfig, ServerConfig};
pub use control_plane::ControlPlane;
pub use dns_provider::{DnsError, DnsProvider, OriginCertificate};
pub use http_plane::HttpPlane;
pub use router::Router;
pub use state::{
    new_response_registry, new_tcp_connection_registry, PortAllocator, ResponseRegistry,
    StreamIdGenerator, TcpConnectionRegistry,
};
pub use tcp_plane::TcpPlane;
