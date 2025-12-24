//! OS Keychain backend
//!
//! Supports:
//! - macOS Keychain
//! - Windows Credential Manager
//! - Linux Secret Service (via libsecret)

use crate::error::SecretError;

/// Resolve a secret from the OS keychain
pub fn resolve(service: &str, key: &str) -> Result<String, SecretError> {
    let entry = keyring::Entry::new(service, key)
        .map_err(|e| SecretError::backend("keychain", e.to_string()))?;

    entry.get_password().map_err(|e| match e {
        keyring::Error::NoEntry => {
            SecretError::NotFound(format!("No keychain entry for {}/{}", service, key))
        }
        keyring::Error::Ambiguous(creds) => SecretError::backend(
            "keychain",
            format!("Ambiguous entry: {} credentials found", creds.len()),
        ),
        keyring::Error::NoStorageAccess(inner) => {
            SecretError::AccessDenied(format!("Cannot access keychain storage: {}", inner))
        }
        _ => SecretError::backend("keychain", e.to_string()),
    })
}

/// Store a secret in the OS keychain (useful for setup)
#[allow(dead_code)]
pub fn store(service: &str, key: &str, value: &str) -> Result<(), SecretError> {
    let entry = keyring::Entry::new(service, key)
        .map_err(|e| SecretError::backend("keychain", e.to_string()))?;

    entry
        .set_password(value)
        .map_err(|e| SecretError::backend("keychain", e.to_string()))
}

/// Delete a secret from the OS keychain
#[allow(dead_code)]
pub fn delete(service: &str, key: &str) -> Result<(), SecretError> {
    let entry = keyring::Entry::new(service, key)
        .map_err(|e| SecretError::backend("keychain", e.to_string()))?;

    entry
        .delete_credential()
        .map_err(|e| SecretError::backend("keychain", e.to_string()))
}
