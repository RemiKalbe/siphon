//! Secret resolution dispatcher

use crate::error::SecretError;
use crate::uri::SecretUri;

/// Resolves secrets from various backends based on URI scheme
#[derive(Debug, Default)]
pub struct SecretResolver {
    _private: (), // Prevent construction without ::new()
}

impl SecretResolver {
    /// Create a new secret resolver
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Resolve a SecretUri to its actual value
    pub fn resolve(&self, uri: &SecretUri) -> Result<String, SecretError> {
        tracing::debug!(backend = uri.backend_name(), "Resolving secret");

        match uri {
            SecretUri::Plain(value) => Ok(value.clone()),

            #[cfg(feature = "env")]
            SecretUri::Env { var_name } => crate::backends::env::resolve(var_name),

            #[cfg(not(feature = "env"))]
            SecretUri::Env { .. } => Err(SecretError::disabled("env")),

            #[cfg(feature = "file")]
            SecretUri::File { path } => crate::backends::file::resolve(path),

            #[cfg(not(feature = "file"))]
            SecretUri::File { .. } => Err(SecretError::disabled("file")),

            #[cfg(feature = "keychain")]
            SecretUri::Keychain { service, key } => {
                crate::backends::keychain::resolve(service, key)
            }

            #[cfg(not(feature = "keychain"))]
            SecretUri::Keychain { .. } => Err(SecretError::disabled("keychain")),

            #[cfg(feature = "onepassword")]
            SecretUri::OnePassword { vault, item, field } => {
                crate::backends::onepassword::resolve(vault, item, field)
            }

            #[cfg(not(feature = "onepassword"))]
            SecretUri::OnePassword { .. } => Err(SecretError::disabled("1password")),
        }
    }

    /// Resolve a SecretUri, trimming whitespace from the result
    pub fn resolve_trimmed(&self, uri: &SecretUri) -> Result<String, SecretError> {
        self.resolve(uri).map(|s| s.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_plain() {
        let resolver = SecretResolver::new();
        let uri = SecretUri::Plain("my-secret".to_string());
        let result = resolver.resolve(&uri).unwrap();
        assert_eq!(result, "my-secret");
    }

    #[test]
    #[cfg(feature = "env")]
    fn test_resolve_env() {
        std::env::set_var("TEST_RESOLVER_SECRET", "env-secret-value");
        let resolver = SecretResolver::new();
        let uri = SecretUri::Env {
            var_name: "TEST_RESOLVER_SECRET".to_string(),
        };
        let result = resolver.resolve(&uri).unwrap();
        assert_eq!(result, "env-secret-value");
        std::env::remove_var("TEST_RESOLVER_SECRET");
    }
}
