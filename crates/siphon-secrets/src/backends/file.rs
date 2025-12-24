//! File backend

use std::path::Path;

use crate::error::SecretError;

/// Resolve a secret from a file
pub fn resolve(path: &Path) -> Result<String, SecretError> {
    std::fs::read_to_string(path).map_err(|e| SecretError::FileError {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_resolve_existing_file() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "secret-content").unwrap();

        let result = resolve(file.path()).unwrap();
        assert_eq!(result.trim(), "secret-content");
    }

    #[test]
    fn test_resolve_missing_file() {
        let result = resolve(Path::new("/definitely/not/a/real/path/12345"));
        assert!(matches!(result, Err(SecretError::FileError { .. })));
    }
}
