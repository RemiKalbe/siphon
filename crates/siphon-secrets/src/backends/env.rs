//! Environment variable backend

use crate::error::SecretError;

/// Resolve a secret from an environment variable
pub fn resolve(var_name: &str) -> Result<String, SecretError> {
    std::env::var(var_name).map_err(|_| SecretError::EnvNotSet {
        var: var_name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_existing_var() {
        std::env::set_var("TEST_SECRET_VAR", "test-value");
        let result = resolve("TEST_SECRET_VAR").unwrap();
        assert_eq!(result, "test-value");
        std::env::remove_var("TEST_SECRET_VAR");
    }

    #[test]
    fn test_resolve_missing_var() {
        let result = resolve("DEFINITELY_NOT_SET_12345");
        assert!(matches!(result, Err(SecretError::EnvNotSet { .. })));
    }
}
