# ---- Builder Stage ----
FROM rust:1.88-slim-bookworm AS builder

# Update all system packages and install build-time dependencies needed for linking.
RUN apt-get update && apt-get upgrade -y && apt-get install -y libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy over your manifests to cache dependencies.
COPY Cargo.toml Cargo.lock ./

# FIX: Create a complete dummy project structure that satisfies all targets
# defined in Cargo.toml, including the integration test.
RUN mkdir -p src tests && \
    echo "pub fn lib() {}" > src/lib.rs && \
    echo "fn main() { acs_smtp_relay::lib(); }" > src/main.rs && \
    echo "#[test] fn an_empty_test() {}" > tests/smtp_flow.rs && \
    cargo test --no-run --all-features && \
    rm -rf src tests

# Copy your actual source code. This will be much faster now.
COPY src ./src
COPY tests ./tests

# Build the final release binary. This step will use the cached dependencies.
RUN cargo build --release

# ---- Runtime Stage ----
FROM gcr.io/distroless/cc-debian12

LABEL org.opencontainers.image.description="SMTP to Azure Communication Services relay. See README for details."

WORKDIR /app

# Copy the compiled binary from the builder stage.
COPY --from=builder /app/target/release/acs-smtp-relay .

# Set a non-root user for security. This user is built-in to distroless images.
USER nonroot:nonroot

# Run the binary when the container starts.
CMD ["./acs-smtp-relay"]