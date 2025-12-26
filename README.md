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

Run the setup wizard to configure server connection:

```bash
siphon setup
```

Then start a tunnel:

```bash
siphon --local 127.0.0.1:3000
```

Or provide all options directly:

```bash
siphon --server tunnel.example.com:4443 \
       --local 127.0.0.1:3000 \
       --cert ./client.crt \
       --key ./client.key \
       --ca ./ca.crt
```

Options:
- `--local` (required): Local address to forward (e.g., `127.0.0.1:3000`)
- `--subdomain`: Request a specific subdomain (optional, auto-generated if not set)
- `--tunnel-type`: `http` (default) or `tcp`

Certificates support multiple formats: file path, `file://`, `base64://`, `op://` (1Password), `keychain://`.

### Server Setup

Configure via environment variables:

```bash
export SIPHON_BASE_DOMAIN="tunnel.example.com"
export SIPHON_CLOUDFLARE_ZONE_ID="your-zone-id"

# Cloudflare API token - create at https://dash.cloudflare.com/profile/api-tokens
# Required permission: Zone.DNS (Edit)
export SIPHON_CLOUDFLARE_API_TOKEN="your-token"

# Certificates - multiple formats supported:
export SIPHON_CERT="file:///path/to/server.crt"
export SIPHON_KEY="file:///path/to/server.key"
export SIPHON_CA_CERT="file:///path/to/ca.crt"
# Or: base64://LS0tLS1CRUdJTi...
# Or: op://vault/item/field (1Password CLI)
# Or: keychain://service/key (OS keychain)

# DNS target (optional - auto-detects IP if neither is set)
# For VPS with static IP:
#   export SIPHON_SERVER_IP="1.2.3.4"
# For platforms like Railway/Render/Fly.io that provide hostnames:
#   export SIPHON_SERVER_CNAME="myapp.up.railway.app"
#
# Note: Auto-detection uses outbound requests, which may return the wrong IP
# on some cloud providers. If tunnels don't work, set one of these explicitly.

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

## Configuration

### Client

Connection settings are stored in `~/.config/siphon/config.toml`:

```toml
server_addr = "tunnel.example.com:4443"

# Secrets can reference keychain, files, or environment variables
cert = "keychain://siphon/cert"
key = "keychain://siphon/key"
ca_cert = "keychain://siphon/ca"
```

Runtime options (`--local`, `--subdomain`, `--tunnel-type`) are provided when starting the tunnel.

### Server

See [server.example.toml](server.example.toml) for configuration options.

### Cloudflare Full (Strict) SSL

To enable HTTPS on the HTTP data plane (required for Cloudflare Full Strict mode), you have two options:

#### Option 1: Automatic Origin CA (Recommended)

The server can automatically generate and manage Cloudflare Origin CA certificates:

```bash
export SIPHON_CLOUDFLARE_AUTO_ORIGIN_CA="true"
```

This requires an additional API token permission: **Zone.SSL and Certificates (Edit)**

On startup, the server will:
1. Revoke any existing Origin CA certificates for your domain
2. Generate a new ECDSA key and CSR
3. Request a certificate from Cloudflare's Origin CA (valid for 1 year)
4. Use it for HTTPS on the HTTP data plane

#### Option 2: Manual Certificates

Provide your own certificates:

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
