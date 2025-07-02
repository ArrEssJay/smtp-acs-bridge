# ---- Builder Stage ----
# Use a specific, recent, and slim base image for security and reproducibility.
FROM rust:1.88-slim-bookworm AS builder

# Update all system packages and install build-time dependencies needed for linking.
RUN apt-get update && apt-get upgrade -y && apt-get install -y libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy over your manifests to cache dependencies.
COPY Cargo.toml Cargo.lock ./

# FIX: Create a minimal, valid project structure to cache dependencies.
# The dummy main.rs MUST use crate::lib() to correctly reference its own library.
# The crate name acs-smtp-relay is converted to acs_smtp_relay in code,
# but using the crate keyword is the canonical and correct way.
RUN mkdir src && \
    echo "pub fn lib() {}" > src/lib.rs && \
    echo "fn main() { crate::lib(); }" > src/main.rs && \
    cargo build --release && \
    rm -rf src/

# Copy your actual source code.
COPY src ./src

# Build the binary in release mode for performance.
RUN cargo build --release

# ---- Runtime Stage ----
# Use a matching distroless image for a smaller footprint and better security.
FROM gcr.io/distroless/cc-debian12

WORKDIR /app

# Copy the compiled binary from the builder stage.
COPY --from=builder /app/target/release/acs-smtp-relay .

# Set a non-root user for security. This user is built-in to distroless images.
USER nonroot:nonroot

# Run the binary when the container starts.
CMD ["./acs-smtp-relay"]