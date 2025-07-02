# Rust SMTP to Azure ACS Email Relay

![alt text](https://github.com/your-username/acs-smtp-relay/actions/workflows/rust.yml/badge.svg)

![alt text](https://github.com/your-username/acs-smtp-relay/actions/workflows/docker.yml/badge.svg)

This service relays emails received via SMTP to the Azure Communication Services (ACS) Email REST API. It is designed to allow applications that can only send email via SMTP to integrate with the modern Azure API.

The project includes:

- A minimal, high-performance SMTP server built with Rust and Tokio.
- A stateless design suitable for containerized, high-availability deployments.
- Structured JSON logging with tracing.
- Graceful shutdown handling for SIGTERM and SIGINT.
- A multi-stage Dockerfile with a distroless base image.
- A generic Kubernetes manifest template.
- Correct HMAC-SHA256 request signing for the ACS API.

## How It Works

- An application connects to this service on its exposed port (e.g., 25).
- The application sends SMTP commands (EHLO, MAIL FROM, RCPT TO, DATA).
- The service accepts the email's raw data.
- It parses the necessary components (subject, body) from the raw data.
- It constructs and signs an HTTP request for the Azure Communication Services Email API.
- It sends the request to Azure, relaying the SMTP email over HTTPS.

## Configuration

The service is configured entirely through environment variables.

| Variable                | Description                                                      | Example                                        |
|-------------------------|------------------------------------------------------------------|------------------------------------------------|
| `ACS_CONNECTION_STRING` | Required. The connection string for your Azure Communication Services resource.         | `endpoint=https://...;accesskey=...`           |
| `ACS_SENDER_ADDRESS`    | Required. The verified "MailFrom" address in your ACS Email domain.  | `DoNotReply@your-verified-domain.com`          |
| `LISTEN_ADDR`           | The IP and port the SMTP server should listen on. Defaults to 0.0.0.0:1025.               | `0.0.0.0:1025`                                 |
| `RUST_LOG`              | The logging level. See EnvFilter docs for syntax.                     | `info` (default), `acs_smtp_relay=debug,warn`  |

## Deployment

### 1. Using Pre-built Docker Images

The recommended way to deploy is using the pre-built container images from GitHub Container Registry (GHCR).

The following image tags are available:

- `ghcr.io/your-username/acs-smtp-relay:v1.0.0` (Replace with the specific version you need)
- `ghcr.io/your-username/acs-smtp-relay:latest` (Tracks the main branch)

### 2. Deploying to Kubernetes (Production)

The included manifest is a generic template for deployment.

**Step 1: Create a Kubernetes Namespace and Secret**

Create a dedicated namespace (e.g., your-namespace) if you don't have one, and securely create the secret for your Azure connection string.

```bash
# 1. Create the namespace
kubectl create namespace your-namespace

# 2. Create the secret, replacing the connection string with your own
kubectl create secret generic acs-relay-secrets \
  --from-literal=ACS_CONNECTION_STRING='endpoint=https://your-acs.communication.azure.com;accesskey=your-key' \
  -n your-namespace
```

**Step 2: Configure and Apply the Manifest**

Edit the manifest file at `k8s/acs-relay.yaml`:

- Replace `<YOUR_CONTAINER_IMAGE_PATH>` with the image path from GHCR (e.g., `ghcr.io/your-username/acs-smtp-relay:v1.0.0`).
- Replace `<YOUR_SENDER_ADDRESS>` with your verified "MailFrom" address from Azure.

Then, apply the configured manifest to your cluster:

```bash
kubectl apply -f k8s/acs-relay.yaml -n your-namespace
```

This will create the Deployment, Service, and PodDisruptionBudget in the specified namespace.

## Development & CI/CD

This project uses GitHub Actions for continuous integration and delivery.

- **CI (rust.yml):** On every push and pull request, the workflow checks formatting, lints the code, and runs the full test suite.
- **Docker Build (docker.yml):** On every push to the main branch, a new container image is built and pushed to GHCR, tagged with the commit SHA. When a version tag (e.g., v1.0.1) is pushed, it's also tagged with the version number and latest.
- **Release Drafter (release-drafter.yml):** This action automatically drafts release notes from merged pull requests, simplifying the release process.

## Local Development

- **Prerequisites:** Install the Rust toolchain.
- **Run Tests:** The project has a full suite of unit and integration tests. Run them with:

```bash
cargo test --all-features
```

The `--all-features` flag is required to enable the mocks feature used by the integration tests.

- **Run Locally:** You can also run the service directly with cargo:

```bash
# Set environment variables first
export ACS_CONNECTION_STRING="..."
export ACS_SENDER_ADDRESS="..."

cargo run
```

## License

This project is licensed under the MIT License. See the Cargo.toml file for details.
