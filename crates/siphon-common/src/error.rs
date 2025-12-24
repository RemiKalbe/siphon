use thiserror::Error;

/// Common errors for tunnel operations
#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("TLS error: {0}")]
    Tls(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Certificate error: {0}")]
    Certificate(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Protocol error: {0}")]
    Protocol(String),
}
