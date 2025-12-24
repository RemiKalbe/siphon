//! 1Password CLI backend
//!
//! Uses the `op` CLI tool to read secrets.
//! Requires 1Password CLI to be installed and authenticated.
//!
//! See: https://developer.1password.com/docs/cli

use std::process::Command;

use crate::error::SecretError;

/// Resolve a secret from 1Password using the CLI
pub fn resolve(vault: &str, item: &str, field: &str) -> Result<String, SecretError> {
    let uri = format!("op://{}/{}/{}", vault, item, field);

    let output = Command::new("op")
        .args(["read", &uri])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SecretError::backend(
                    "1password",
                    "1Password CLI ('op') not found. Install from https://1password.com/downloads/command-line/",
                )
            } else {
                SecretError::backend("1password", format!("Failed to execute 'op' CLI: {}", e))
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error_msg = stderr.trim();

        // Provide helpful error messages for common issues
        if error_msg.contains("not signed in") || error_msg.contains("session expired") {
            return Err(SecretError::backend(
                "1password",
                "Not signed in to 1Password CLI. Run 'op signin' or 'eval $(op signin)'",
            ));
        }

        if error_msg.contains("isn't a vault") || error_msg.contains("vault") {
            return Err(SecretError::NotFound(format!(
                "1Password vault '{}' not found",
                vault
            )));
        }

        if error_msg.contains("isn't an item") || error_msg.contains("item") {
            return Err(SecretError::NotFound(format!(
                "1Password item '{}/{}' not found",
                vault, item
            )));
        }

        if error_msg.contains("isn't a field") || error_msg.contains("field") {
            return Err(SecretError::NotFound(format!(
                "1Password field '{}/{}/{}' not found",
                vault, item, field
            )));
        }

        return Err(SecretError::backend("1password", error_msg));
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if value.is_empty() {
        return Err(SecretError::NotFound(format!(
            "1Password field '{}/{}/{}' is empty",
            vault, item, field
        )));
    }

    Ok(value)
}
