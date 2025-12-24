# Siphon

Secure tunnel client and server for exposing local services through mTLS-authenticated tunnels.

## Features

- **mTLS Authentication** - Certificate-based mutual TLS for secure client-server communication
- **HTTP & TCP Tunnels** - Support for both HTTP and raw TCP tunnel types
- **Cloudflare DNS Integration** - Automatic subdomain creation via Cloudflare API (supports Full Strict SSL)
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

Certificates support multiple formats: file path, `file://`, `base64://`, `op://` (1Password), `keychain://`.

### Server Setup

Configure via environment variables:

```bash
export SIPHON_BASE_DOMAIN="tunnel.example.com"
export SIPHON_CLOUDFLARE_API_TOKEN="your-token"
export SIPHON_CLOUDFLARE_ZONE_ID="your-zone-id"

# Certificates - multiple formats supported:
export SIPHON_CERT="file:///path/to/server.crt"
export SIPHON_KEY="file:///path/to/server.key"
export SIPHON_CA_CERT="file:///path/to/ca.crt"
# Or: base64://LS0tLS1CRUdJTi...
# Or: op://vault/item/field (1Password CLI)
# Or: keychain://service/key (OS keychain)

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

## Generating mTLS Certificates

Siphon uses mutual TLS (mTLS) for secure client-server authentication. You need:
- A Certificate Authority (CA)
- A server certificate signed by the CA
- Client certificates signed by the CA

### Using OpenSSL

```bash
# 1. Create the CA
openssl genrsa -out ca.key 4096
openssl req -new -x509 -days 3650 -key ca.key -out ca.crt \
  -subj "/CN=Siphon CA"

# 2. Create the server certificate
openssl genrsa -out server.key 4096
openssl req -new -key server.key -out server.csr \
  -subj "/CN=tunnel.example.com"
openssl x509 -req -days 365 -in server.csr -CA ca.crt -CAkey ca.key \
  -CAcreateserial -out server.crt

# 3. Create a client certificate
openssl genrsa -out client.key 4096
openssl req -new -key client.key -out client.csr \
  -subj "/CN=client1"
openssl x509 -req -days 365 -in client.csr -CA ca.crt -CAkey ca.key \
  -CAcreateserial -out client.crt
```

### Using step-ca (recommended)

[step-ca](https://smallstep.com/docs/step-ca/) provides a simpler workflow:

```bash
# Install step CLI
brew install step  # macOS
# or: https://smallstep.com/docs/step-cli/installation/

# Initialize a CA
step ca init --name "Siphon CA" --provisioner admin

# Issue server certificate
step ca certificate tunnel.example.com server.crt server.key

# Issue client certificate
step ca certificate client1 client.crt client.key
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

### Cloudflare Full (Strict) SSL

To enable HTTPS on the HTTP data plane (required for Cloudflare Full Strict mode):

```bash
export SIPHON_HTTP_CERT="file:///path/to/origin.crt"
export SIPHON_HTTP_KEY="file:///path/to/origin.key"
```

You can use a [Cloudflare Origin CA certificate](https://developers.cloudflare.com/ssl/origin-configuration/origin-ca/) (free, trusted only by Cloudflare) or any valid certificate for your domain.

## Utilities

### Encode certificates as base64

If you encounter base64 compatibility issues (different CLI tools may produce varying output), you can use the built-in encode command:

```bash
siphon encode /path/to/server.crt
# Output: base64://LS0tLS1CRUdJTi...
```

## License

MIT
