# Siphon

Secure tunnel client and server for exposing local services through mTLS-authenticated tunnels.

## Features

- **mTLS Authentication** - Certificate-based mutual TLS for secure client-server communication
- **HTTP & TCP Tunnels** - Support for both HTTP and raw TCP tunnel types
- **Cloudflare DNS Integration** - Automatic subdomain creation via Cloudflare API
- **TUI Dashboard** - Real-time metrics and monitoring with terminal UI
- **Interactive Setup** - Guided wizard for configuration with OS keychain integration
- **Cross-Platform** - Runs on Linux, macOS, and Windows

## Installation

### From crates.io

```bash
cargo install siphon        # Client
cargo install siphon-server # Server
```

### From source

```bash
git clone https://github.com/RemiKalbe/siphon.git
cd siphon
cargo build --release
```

## Quick Start

### Client Setup

Run the interactive setup wizard:

```bash
siphon setup
```

Or provide configuration directly:

```bash
siphon --server tunnel.example.com:4443 \
       --local 127.0.0.1:3000 \
       --cert ./client.crt \
       --key ./client.key \
       --ca ./ca.crt
```

### Server Setup

Configure via environment variables:

```bash
export SIPHON_BASE_DOMAIN="tunnel.example.com"
export SIPHON_CLOUDFLARE_API_TOKEN="your-token"
export SIPHON_CLOUDFLARE_ZONE_ID="your-zone-id"

# Certificates - use file:// URIs or base64://
export SIPHON_CERT="file:///path/to/server.crt"
export SIPHON_KEY="file:///path/to/server.key"
export SIPHON_CA_CERT="file:///path/to/ca.crt"

# Or use base64 for CI/CD environments:
# export SIPHON_CERT="base64://LS0tLS1CRUdJTi..."

# SIPHON_SERVER_IP is optional - auto-detected if not set
# Warning: Some cloud providers use different IPs for inbound vs outbound traffic.
# Auto-detection uses outbound requests, so it may set the wrong IP silently.
# If tunnels don't work, explicitly set this to your server's public inbound IP.

siphon-server
```

Or use Docker:

```bash
docker-compose up -d
```

## Configuration

### Client

Configuration is stored in `~/.config/siphon/config.toml`:

```toml
server_addr = "tunnel.example.com:4443"
local_addr = "127.0.0.1:3000"
subdomain = "myapp"
tunnel_type = "http"

# Secrets can reference keychain, files, or environment variables
cert = "keychain://siphon/cert"
key = "keychain://siphon/key"
ca_cert = "keychain://siphon/ca"
```

### Server

See [server.example.toml](server.example.toml) for configuration options.

## Utilities

### Encode certificates as base64

If you encounter base64 compatibility issues (different CLI tools may produce varying output), you can use the built-in encode command:

```bash
siphon encode /path/to/server.crt
# Output: base64://LS0tLS1CRUdJTi...
```

## License

MIT
