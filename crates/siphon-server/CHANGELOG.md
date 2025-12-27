# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/RemiKalbe/siphon/compare/siphon-server-v0.1.0...siphon-server-v0.1.1) - 2025-12-27

### Feat

- *(e2e)* add end-to-end test infrastructure

### Fix

- resolve clippy warnings

## [0.1.0](https://github.com/RemiKalbe/siphon/releases/tag/siphon-server-v0.1.0) - 2025-12-26

### Added

- *(server)* add SIPHON_BIND_HOST env var for IPv6 support
- *(server)* cleanup old Origin CA certificates before creating new one
- improve TLS errors, TUI rendering, and add auto Origin CA
- *(server)* add CNAME record support for DNS
- *(server)* add TLS support for HTTP plane (Cloudflare Full Strict)
- *(client)* default to port 443 when not specified
- *(secrets)* add base64 URI scheme and simplify env vars
- use Cloudflare trace as primary IP detection
- auto-detect server IP and rename env var
- initial release of Siphon tunnel system

### Fixed

- *(server)* fix logging to respect RUST_LOG
- *(client)* track request metrics and warn on forwarding failures
- *(server)* ensure generated subdomains start with a letter
- remove println that caused TUI layout shift, fix Origin CA key type
- *(config)* use SIPHON_CLOUDFLARE_AUTO_ORIGIN_CA env var for consistency
- explicitly install rustls crypto provider at startup
- enable ring crypto provider for rustls
- resolve clippy warnings and add pre-commit fmt hook

### Other

- *(server)* use cuid2 for subdomain generation
- add logging for TLS certificate and handshake issues
- clarify that only matching base domain certs are revoked
- update README with Cloudflare API token permissions and auto Origin CA
- *(client)* simplify setup wizard and config
- add mTLS certificate generation instructions
- use port 4443 in examples, remove default port
- move certificate formats note outside code block
- use standard HTTPS port 443 in examples
- note secret formats support in client setup
- list all secret formats without prescribing use cases
- soften encode command recommendation
- add encode command to README
- add warning about cloud provider IP asymmetry
- update README with correct server IP env var
- add README.md
