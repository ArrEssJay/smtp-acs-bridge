# ---- Builder Stage ----
# Use the latest stable Rust version to mitigate known vulnerabilities.
FROM rust:1 as builder

# Update all system packages and install build-time dependencies needed for linking.
RUN apt-get update && apt-get upgrade -y && apt-get install -y libssl-dev pkg-config && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy over your manifests and cache dependencies.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src/ && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src/

# Copy your actual source code.
COPY src ./src

# Build the binary in release mode for performance.
RUN cargo build --release

# ---- Runtime Stage ----
# Use a matching, more recent distroless image (debian12 for bookworm).
FROM gcr.io/distroless/cc-debian12

WORKDIR /app

# Copy the compiled binary from the builder stage.
COPY --from=builder /app/target/release/acs-smtp-relay .

# Set a non-root user for security. This user is built-in to distroless images.
USER nonroot:nonroot

# Run the binary when the container starts.
CMD ["./acs-smtp-relay"]