use std::path::PathBuf;

use thiserror::Error;

/// Errors that can occur during secret resolution
#[derive(Debug, Error)]
pub enum SecretError {
    /// Invalid URI format
    #[error("Invalid secret URI '{uri}': {reason}")]
    InvalidUri { uri: String, reason: String },

    /// Secret not found in backend
    #[error("Secret not found: {0}")]
    NotFound(String),

    /// Backend feature not compiled in
    #[error("Secret backend '{backend}' not available (feature not enabled)")]
    BackendDisabled { backend: String },

    /// Backend runtime error
    #[error("{backend} error: {message}")]
    BackendError { backend: String, message: String },

    /// Permission/access denied
    #[error("Access denied to secret: {0}")]
    AccessDenied(String),

    /// File IO error
    #[error("Failed to read file '{path}': {message}")]
    FileError { path: PathBuf, message: String },

    /// Environment variable error
    #[error("Environment variable '{var}' not set")]
    EnvNotSet { var: String },
}

impl SecretError {
    /// Create an invalid URI error
    pub fn invalid_uri(uri: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidUri {
            uri: uri.into(),
            reason: reason.into(),
        }
    }

    /// Create a backend error
    pub fn backend(backend: impl Into<String>, message: impl Into<String>) -> Self {
        Self::BackendError {
            backend: backend.into(),
            message: message.into(),
        }
    }

    /// Create a backend disabled error
    pub fn disabled(backend: impl Into<String>) -> Self {
        Self::BackendDisabled {
            backend: backend.into(),
        }
    }
}
