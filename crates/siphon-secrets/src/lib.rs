//! Secret management with multiple backend support
//!
//! This crate provides a unified interface for resolving secrets from various backends:
//!
//! - **OS Keychain** (`keychain://service/key`): macOS Keychain, Windows Credential Manager, Linux Secret Service
//! - **1Password CLI** (`op://vault/item/field`): Requires `op` CLI to be installed and authenticated
//! - **Environment variables** (`env://VAR_NAME`): Read from process environment
//! - **Files** (`file:///path` or just `/path`): Read content from filesystem
//! - **Plain values**: Any string without a URI scheme is treated as a literal value
//!
//! # Example
//!
//! ```rust,ignore
//! use siphon_secrets::{SecretUri, SecretResolver};
//!
//! // Parse a secret URI from config
//! let uri: SecretUri = "keychain://myapp/api-token".parse()?;
//!
//! // Resolve to actual value
//! let resolver = SecretResolver::new();
//! let secret = resolver.resolve(&uri)?;
//! ```
//!
//! # Features
//!
//! - `keychain` (default): Enable OS keychain support via `keyring` crate
//! - `onepassword` (default): Enable 1Password CLI support
//! - `env` (default): Enable environment variable support
//! - `file` (default): Enable file reading support

mod backends;
mod error;
mod resolver;
mod uri;

pub use error::SecretError;
pub use resolver::SecretResolver;
pub use uri::SecretUri;

// Re-export keychain utilities for setup/management
#[cfg(feature = "keychain")]
pub mod keychain {
    pub use crate::backends::keychain::{delete, resolve, store};
}
