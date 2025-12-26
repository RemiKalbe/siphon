# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/RemiKalbe/siphon/releases/tag/siphon-tui-v0.1.0) - 2025-12-26

### Added

- *(tui)* add 'c' key to copy tunnel URL to clipboard
- improve TLS errors, TUI rendering, and add auto Origin CA
- *(setup)* add rustyline for better input experience
- initial release of Siphon tunnel system

### Fixed

- *(tui)* clear frame on resize to prevent rendering artifacts
- *(config)* use ~/.config/siphon on all platforms
- *(setup)* fall back to base64 config when keychain fails
- *(setup)* add verification steps for keychain and config save
- publish all crates to crates.io
- resolve clippy warnings and add pre-commit fmt hook

### Other

- *(server)* use cuid2 for subdomain generation
- *(setup)* remove Tab to complete hint
- *(setup)* add styled inline CLI using crossterm
- *(client)* simplify setup wizard and config
