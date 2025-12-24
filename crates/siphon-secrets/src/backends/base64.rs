//! Base64 decoding backend

use base64::Engine;

use crate::error::SecretError;

/// Resolve a secret from base64-encoded data
pub fn resolve(data: &str) -> Result<String, SecretError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| SecretError::backend("base64", format!("decode error: {}", e)))?;

    String::from_utf8(bytes)
        .map_err(|e| SecretError::backend("base64", format!("invalid UTF-8: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_valid_base64() {
        // "Hello World" in base64
        let result = resolve("SGVsbG8gV29ybGQ=").unwrap();
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_decode_pem_certificate() {
        // A mock PEM header
        let pem = "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----";
        let encoded = base64::engine::general_purpose::STANDARD.encode(pem);
        let result = resolve(&encoded).unwrap();
        assert_eq!(result, pem);
    }

    #[test]
    fn test_decode_invalid_base64() {
        let result = resolve("not-valid-base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_invalid_utf8() {
        // Valid base64 but decodes to invalid UTF-8 bytes
        let invalid_utf8 = base64::engine::general_purpose::STANDARD.encode([0xFF, 0xFE]);
        let result = resolve(&invalid_utf8);
        assert!(result.is_err());
    }
}
