# Siphon Server Dockerfile
# Multi-stage build for minimal image size

# Build stage
FROM rust:1.83-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release binary
RUN cargo build --release --package siphon-server

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -s /bin/false siphon

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /build/target/release/siphon-server /usr/local/bin/siphon-server

# Create directories for certificates (optional, can mount volumes)
RUN mkdir -p /app/certs && chown siphon:siphon /app/certs

# Switch to non-root user
USER siphon

# Default ports
# 4443 - Control plane (mTLS client connections)
# 8080 - HTTP plane (traffic from Cloudflare)
# 30000-40000 - TCP tunnel port range
EXPOSE 4443 8080

# Environment variables (all can be overridden)
# Required:
#   SIPHON_BASE_DOMAIN          - Base domain for tunnels (e.g., tunnel.example.com)
#   SIPHON_CERT or SIPHON_CERT_FILE        - Server certificate (PEM content or file path)
#   SIPHON_KEY or SIPHON_KEY_FILE          - Server private key (PEM content or file path)
#   SIPHON_CA_CERT or SIPHON_CA_CERT_FILE  - CA certificate (PEM content or file path)
#   SIPHON_CLOUDFLARE_API_TOKEN - Cloudflare API token
#   SIPHON_CLOUDFLARE_ZONE_ID   - Cloudflare zone ID
#   SIPHON_CLOUDFLARE_SERVER_IP - Server's public IP for DNS records
#
# Optional (with defaults):
#   SIPHON_CONTROL_PORT         - Control plane port (default: 4443)
#   SIPHON_HTTP_PORT            - HTTP plane port (default: 8080)
#   SIPHON_TCP_PORT_START       - TCP port range start (default: 30000)
#   SIPHON_TCP_PORT_END         - TCP port range end (default: 40000)

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD nc -z localhost ${SIPHON_CONTROL_PORT:-4443} || exit 1

ENTRYPOINT ["siphon-server"]

# Default: no config file (use environment variables)
CMD ["--config", "/dev/null"]
