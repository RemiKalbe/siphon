//! Mock DNS provider for E2E tests
//!
//! This module provides a mock implementation of the DnsProvider trait
//! that can be used in tests without making real DNS API calls.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;

use siphon_server::{DnsError, DnsProvider, OriginCertificate};

/// Mock DNS provider that tracks operations without making real API calls
pub struct MockDnsProvider {
    /// Tracks created records: record_id -> subdomain
    records: DashMap<String, String>,
    /// Counter for generating unique record IDs
    record_counter: AtomicU64,
    /// Whether to simulate failures on create
    fail_create: AtomicBool,
    /// Whether to simulate failures on delete
    fail_delete: AtomicBool,
}

impl MockDnsProvider {
    /// Create a new mock DNS provider
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            records: DashMap::new(),
            record_counter: AtomicU64::new(1),
            fail_create: AtomicBool::new(false),
            fail_delete: AtomicBool::new(false),
        })
    }

    /// Get all created records (for test assertions)
    pub fn get_records(&self) -> Vec<(String, String)> {
        self.records
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    /// Check if a record exists for the given subdomain
    pub fn has_record(&self, subdomain: &str) -> bool {
        self.records.iter().any(|r| r.value() == subdomain)
    }

    /// Get the number of active records
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Configure mock to fail on next create operation
    pub fn set_fail_create(&self, fail: bool) {
        self.fail_create.store(fail, Ordering::SeqCst);
    }

    /// Configure mock to fail on next delete operation
    pub fn set_fail_delete(&self, fail: bool) {
        self.fail_delete.store(fail, Ordering::SeqCst);
    }

    /// Clear all records (useful between tests)
    pub fn clear(&self) {
        self.records.clear();
    }
}

impl Default for MockDnsProvider {
    fn default() -> Self {
        Self {
            records: DashMap::new(),
            record_counter: AtomicU64::new(1),
            fail_create: AtomicBool::new(false),
            fail_delete: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl DnsProvider for MockDnsProvider {
    async fn create_record(&self, subdomain: &str, _proxied: bool) -> Result<String, DnsError> {
        if self.fail_create.load(Ordering::SeqCst) {
            return Err(DnsError::Api("Simulated create failure".into()));
        }

        let record_id = format!(
            "mock-record-{}",
            self.record_counter.fetch_add(1, Ordering::Relaxed)
        );
        self.records
            .insert(record_id.clone(), subdomain.to_string());
        tracing::debug!(
            "MockDnsProvider: created record {} for {}",
            record_id,
            subdomain
        );
        Ok(record_id)
    }

    async fn delete_record(&self, record_id: &str) -> Result<(), DnsError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(DnsError::Api("Simulated delete failure".into()));
        }

        self.records.remove(record_id);
        tracing::debug!("MockDnsProvider: deleted record {}", record_id);
        Ok(())
    }

    async fn create_origin_certificate(
        &self,
        _validity_days: u32,
    ) -> Result<Option<OriginCertificate>, DnsError> {
        // Tests don't need real Origin CA certificates
        // Return None to indicate no certificate was created
        Ok(None)
    }

    async fn cleanup_old_origin_certificates(&self) -> Result<u32, DnsError> {
        // No-op for mock
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_delete_record() {
        let provider = MockDnsProvider::new();

        // Create a record
        let record_id = provider.create_record("myapp", true).await.unwrap();
        assert!(record_id.starts_with("mock-record-"));
        assert!(provider.has_record("myapp"));
        assert_eq!(provider.record_count(), 1);

        // Delete the record
        provider.delete_record(&record_id).await.unwrap();
        assert!(!provider.has_record("myapp"));
        assert_eq!(provider.record_count(), 0);
    }

    #[tokio::test]
    async fn test_failure_simulation() {
        let provider = MockDnsProvider::new();

        // Simulate create failure
        provider.set_fail_create(true);
        let result = provider.create_record("failing", true).await;
        assert!(result.is_err());

        // Reset and verify it works again
        provider.set_fail_create(false);
        let record_id = provider.create_record("working", true).await.unwrap();
        assert!(provider.has_record("working"));

        // Simulate delete failure
        provider.set_fail_delete(true);
        let result = provider.delete_record(&record_id).await;
        assert!(result.is_err());
    }
}
