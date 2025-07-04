# syntax=docker/dockerfile:1

# Build stage
FROM rust:1.77-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to cache dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/acs_smtp_relay*

# Copy the actual source code
COPY src ./src
COPY tests ./tests

# Build the actual application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false -m -d /app smtp-relay

WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/acs-smtp-relay .

# Change ownership to non-root user
RUN chown smtp-relay:smtp-relay /app/acs-smtp-relay

# Switch to non-root user
USER smtp-relay

# Expose SMTP port
EXPOSE 1025

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD timeout 5 bash -c '</dev/tcp/localhost/1025' || exit 1

# Set the binary as entrypoint
ENTRYPOINT ["./acs-smtp-relay"]