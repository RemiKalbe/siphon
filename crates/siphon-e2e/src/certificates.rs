//! Test certificate generation using rcgen
//!
//! This module provides utilities for generating test certificates at runtime
//! for mTLS testing without requiring pre-generated certificate files.

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A complete set of test certificates for mTLS
#[derive(Clone)]
pub struct TestCertificates {
    /// CA certificate PEM
    pub ca_cert_pem: String,
    /// CA private key PEM
    pub ca_key_pem: String,

    /// Server certificate PEM
    pub server_cert_pem: String,
    /// Server private key PEM
    pub server_key_pem: String,

    /// Client certificate PEM
    pub client_cert_pem: String,
    /// Client private key PEM
    pub client_key_pem: String,
}

impl TestCertificates {
    /// Generate a complete certificate chain for testing
    ///
    /// Creates:
    /// - A self-signed CA
    /// - A server certificate signed by the CA (with localhost SAN)
    /// - A client certificate signed by the CA
    pub fn generate() -> Self {
        // 1. Generate CA
        let ca_key = KeyPair::generate().expect("Failed to generate CA key");
        // Serialize CA key PEM before it gets consumed by Issuer
        let ca_key_pem = ca_key.serialize_pem();

        let mut ca_params = CertificateParams::default();
        ca_params.distinguished_name = {
            let mut dn = DistinguishedName::new();
            dn.push(DnType::CommonName, "Siphon Test CA");
            dn.push(DnType::OrganizationName, "Siphon E2E Tests");
            dn
        };
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];

        let ca_cert = ca_params
            .clone()
            .self_signed(&ca_key)
            .expect("Failed to create CA cert");

        // Create an issuer from the CA for signing other certs (consumes ca_key)
        let ca_issuer = Issuer::new(ca_params, ca_key);

        // 2. Generate Server Certificate
        let server_key = KeyPair::generate().expect("Failed to generate server key");
        let mut server_params = CertificateParams::default();
        server_params.distinguished_name = {
            let mut dn = DistinguishedName::new();
            dn.push(DnType::CommonName, "localhost");
            dn
        };
        server_params.subject_alt_names = vec![
            rcgen::SanType::DnsName("localhost".try_into().unwrap()),
            rcgen::SanType::IpAddress(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            rcgen::SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ];
        server_params.key_usages = vec![
            rcgen::KeyUsagePurpose::DigitalSignature,
            rcgen::KeyUsagePurpose::KeyEncipherment,
        ];
        server_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];

        let server_cert = server_params
            .signed_by(&server_key, &ca_issuer)
            .expect("Failed to create server cert");

        // 3. Generate Client Certificate
        let client_key = KeyPair::generate().expect("Failed to generate client key");
        let mut client_params = CertificateParams::default();
        client_params.distinguished_name = {
            let mut dn = DistinguishedName::new();
            dn.push(DnType::CommonName, "test-client");
            dn
        };
        client_params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        client_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];

        let client_cert = client_params
            .signed_by(&client_key, &ca_issuer)
            .expect("Failed to create client cert");

        Self {
            ca_cert_pem: ca_cert.pem(),
            ca_key_pem,
            server_cert_pem: server_cert.pem(),
            server_key_pem: server_key.serialize_pem(),
            client_cert_pem: client_cert.pem(),
            client_key_pem: client_key.serialize_pem(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_certificates() {
        let certs = TestCertificates::generate();

        // Verify PEM format
        assert!(certs.ca_cert_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(certs
            .server_cert_pem
            .contains("-----BEGIN CERTIFICATE-----"));
        assert!(certs
            .client_cert_pem
            .contains("-----BEGIN CERTIFICATE-----"));
        assert!(certs.ca_key_pem.contains("-----BEGIN PRIVATE KEY-----"));
        assert!(certs.server_key_pem.contains("-----BEGIN PRIVATE KEY-----"));
        assert!(certs.client_key_pem.contains("-----BEGIN PRIVATE KEY-----"));
    }
}
