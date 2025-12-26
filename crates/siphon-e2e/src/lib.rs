//! End-to-end test utilities for the Siphon tunnel system
//!
//! This crate provides test harnesses and utilities for running E2E tests
//! of the siphon tunnel system without requiring external services like Cloudflare.

pub mod certificates;
pub mod harness;
pub mod mock_dns;
pub mod mock_service;
pub mod mock_tcp_service;
pub mod test_client;

pub use certificates::TestCertificates;
pub use harness::TestServer;
pub use mock_dns::MockDnsProvider;
pub use mock_service::MockHttpService;
pub use mock_tcp_service::{MockTcpService, TcpServiceMode};
pub use test_client::TestClient;
