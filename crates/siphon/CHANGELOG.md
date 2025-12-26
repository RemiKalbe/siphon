# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/RemiKalbe/siphon/releases/tag/siphon-v0.1.0) - 2025-12-26

### Added

- improve TLS errors, TUI rendering, and add auto Origin CA
- *(server)* add CNAME record support for DNS
- *(server)* add TLS support for HTTP plane (Cloudflare Full Strict)
- *(client)* default to port 443 when not specified
- *(cli)* add encode command for base64 conversion
- *(secrets)* add base64 URI scheme and simplify env vars
- initial release of Siphon tunnel system

### Fixed

- *(server)* fix logging to respect RUST_LOG
- *(client)* track request metrics and warn on forwarding failures
- remove println that caused TUI layout shift, fix Origin CA key type
- *(client)* send TLS close_notify on graceful shutdown
- *(client)* update TUI with tunnel info when established
- *(client)* install rustls crypto provider at startup
- enable ring crypto provider for rustls
- resolve clippy warnings and add pre-commit fmt hook

### Other

- *(server)* use cuid2 for subdomain generation
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
