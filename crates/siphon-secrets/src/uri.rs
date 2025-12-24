use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

use crate::error::SecretError;

/// Represents a secret reference that can be resolved from various backends.
///
/// Supports the following URI schemes:
/// - `keychain://service/key` - OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service)
/// - `op://vault/item/field` - 1Password CLI
/// - `env://VAR_NAME` - Environment variable
/// - `file:///path/to/file` - File content
/// - Plain string - Literal value (backwards compatible)
#[derive(Debug, Clone, PartialEq)]
pub enum SecretUri {
    /// Plain text value (no URI scheme, backwards compatible)
    Plain(String),

    /// OS Keychain: `keychain://service/key`
    Keychain { service: String, key: String },

    /// 1Password CLI: `op://vault/item/field`
    OnePassword {
        vault: String,
        item: String,
        field: String,
    },

    /// Environment variable: `env://VAR_NAME`
    Env { var_name: String },

    /// File path: `file:///path/to/file` or just a path
    File { path: PathBuf },
}

impl SecretUri {
    /// Check if this is a plain value (not a URI reference)
    pub fn is_plain(&self) -> bool {
        matches!(self, SecretUri::Plain(_))
    }

    /// Get the backend name for logging/errors
    pub fn backend_name(&self) -> &'static str {
        match self {
            SecretUri::Plain(_) => "plain",
            SecretUri::Keychain { .. } => "keychain",
            SecretUri::OnePassword { .. } => "1password",
            SecretUri::Env { .. } => "env",
            SecretUri::File { .. } => "file",
        }
    }
}

impl FromStr for SecretUri {
    type Err = SecretError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("keychain://") {
            parse_keychain_uri(s)
        } else if s.starts_with("op://") {
            parse_onepassword_uri(s)
        } else if s.starts_with("env://") {
            parse_env_uri(s)
        } else if s.starts_with("file://") {
            parse_file_uri(s)
        } else if looks_like_file_path(s) {
            // Treat bare paths as file URIs for convenience
            Ok(SecretUri::File {
                path: PathBuf::from(s),
            })
        } else {
            // Plain value (no URI scheme)
            Ok(SecretUri::Plain(s.to_string()))
        }
    }
}

/// Parse `keychain://service/key`
fn parse_keychain_uri(s: &str) -> Result<SecretUri, SecretError> {
    let rest = s.strip_prefix("keychain://").unwrap();
    let parts: Vec<&str> = rest.splitn(2, '/').collect();

    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(SecretError::invalid_uri(
            s,
            "keychain URI must be keychain://service/key",
        ));
    }

    Ok(SecretUri::Keychain {
        service: parts[0].to_string(),
        key: parts[1].to_string(),
    })
}

/// Parse `op://vault/item/field`
fn parse_onepassword_uri(s: &str) -> Result<SecretUri, SecretError> {
    let rest = s.strip_prefix("op://").unwrap();
    let parts: Vec<&str> = rest.splitn(3, '/').collect();

    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(SecretError::invalid_uri(
            s,
            "1Password URI must be op://vault/item/field",
        ));
    }

    Ok(SecretUri::OnePassword {
        vault: parts[0].to_string(),
        item: parts[1].to_string(),
        field: parts[2].to_string(),
    })
}

/// Parse `env://VAR_NAME`
fn parse_env_uri(s: &str) -> Result<SecretUri, SecretError> {
    let var_name = s.strip_prefix("env://").unwrap();

    if var_name.is_empty() {
        return Err(SecretError::invalid_uri(
            s,
            "env URI must specify a variable name",
        ));
    }

    Ok(SecretUri::Env {
        var_name: var_name.to_string(),
    })
}

/// Parse `file:///path/to/file`
fn parse_file_uri(s: &str) -> Result<SecretUri, SecretError> {
    let path = s.strip_prefix("file://").unwrap();

    if path.is_empty() {
        return Err(SecretError::invalid_uri(s, "file URI must specify a path"));
    }

    Ok(SecretUri::File {
        path: PathBuf::from(path),
    })
}

/// Check if a string looks like a file path
fn looks_like_file_path(s: &str) -> bool {
    // Unix absolute path or Windows path or relative path with extension
    s.starts_with('/')
        || s.starts_with("./")
        || s.starts_with("../")
        || (s.len() > 2 && s.chars().nth(1) == Some(':')) // Windows C:\...
        || s.contains(".pem")
        || s.contains(".crt")
        || s.contains(".key")
}

/// Custom serde deserializer for SecretUri
impl<'de> Deserialize<'de> for SecretUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        SecretUri::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_keychain_uri() {
        let uri: SecretUri = "keychain://myservice/mykey".parse().unwrap();
        assert_eq!(
            uri,
            SecretUri::Keychain {
                service: "myservice".to_string(),
                key: "mykey".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_onepassword_uri() {
        let uri: SecretUri = "op://Private/Server/api-token".parse().unwrap();
        assert_eq!(
            uri,
            SecretUri::OnePassword {
                vault: "Private".to_string(),
                item: "Server".to_string(),
                field: "api-token".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_env_uri() {
        let uri: SecretUri = "env://MY_SECRET".parse().unwrap();
        assert_eq!(
            uri,
            SecretUri::Env {
                var_name: "MY_SECRET".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_file_uri() {
        let uri: SecretUri = "file:///etc/tunnel/secret.key".parse().unwrap();
        assert_eq!(
            uri,
            SecretUri::File {
                path: PathBuf::from("/etc/tunnel/secret.key"),
            }
        );
    }

    #[test]
    fn test_parse_bare_path() {
        let uri: SecretUri = "/etc/tunnel/server.crt".parse().unwrap();
        assert_eq!(
            uri,
            SecretUri::File {
                path: PathBuf::from("/etc/tunnel/server.crt"),
            }
        );
    }

    #[test]
    fn test_parse_plain_value() {
        let uri: SecretUri = "my-secret-token".parse().unwrap();
        assert_eq!(uri, SecretUri::Plain("my-secret-token".to_string()));
    }

    #[test]
    fn test_invalid_keychain_uri() {
        let result: Result<SecretUri, _> = "keychain://onlyservice".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_onepassword_uri() {
        let result: Result<SecretUri, _> = "op://vault/item".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_env_uri() {
        let result: Result<SecretUri, _> = "env://".parse();
        assert!(result.is_err());
    }
}
