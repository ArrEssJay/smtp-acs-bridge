# syntax=docker/dockerfile:1

# Build stage
FROM rust:1.88-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./

# Create minimal Cargo.toml for dependency caching (remove test declarations)
RUN sed '/^\[\[test\]\]/,/^$/d' Cargo.toml > Cargo.minimal.toml && \
    mv Cargo.minimal.toml Cargo.toml

# Create dummy source and fetch dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo fetch && \
    rm -rf src

# Copy the real Cargo.toml and source
COPY Cargo.toml ./
COPY src ./src
COPY tests ./tests

# Build with optimizations
RUN cargo build --release --locked --features health-server && \
    strip target/release/acs-smtp-relay

# Runtime stage
FROM gcr.io/distroless/cc-debian12

# Metadata for Azure Container Registry
LABEL org.opencontainers.image.source="https://github.com/ArrEssJay/smtp-acs-bridge/"
LABEL org.opencontainers.image.description="SMTP to Azure Communication Services relay"
LABEL org.opencontainers.image.vendor="Rowan Jones"
LABEL org.opencontainers.image.title="SMTP-ACS-Bridge"
LABEL org.opencontainers.image.licenses="MIT"

WORKDIR /app

# Copy the optimized binary
COPY --from=builder /app/target/release/acs-smtp-relay ./acs-smtp-relay

# Use non-root user (required for Azure security policies)
USER nonroot:nonroot

# Environment variables for Azure deployment
ENV RUST_LOG=info
ENV LISTEN_ADDR=0.0.0.0:1025

# Expose SMTP port
EXPOSE 1025

# Azure-compatible entrypoint
ENTRYPOINT ["./acs-smtp-relay"]