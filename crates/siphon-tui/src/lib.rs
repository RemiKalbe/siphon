//! Siphon TUI - Dashboard and setup wizard for Siphon tunnel client
//!
//! This crate provides:
//! - Real-time metrics dashboard with graphs
//! - Interactive setup wizard for configuration

pub mod config;
pub mod metrics;
pub mod setup;
pub mod ui;

pub use config::SiphonConfig;
pub use metrics::{MetricsCollector, MetricsSnapshot, TunnelInfo};
pub use setup::SetupWizard;
pub use ui::TuiApp;
