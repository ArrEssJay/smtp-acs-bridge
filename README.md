# Rust SMTP to Azure ACS Email Relay

[![Docker Build & Push](https://github.com/ArrEssJay/smtp-acs-bridge/actions/workflows/docker.yml/badge.svg)](https://github.com/ArrEssJay/smtp-acs-bridge/actions/workflows/docker.yml)

This service relays emails received via SMTP to the Azure Communication Services (ACS) Email REST API. It is designed to allow applications that can only send email via SMTP to integrate with the modern Azure API.

The project includes:

- A minimal, high-performance SMTP server built with Rust and Tokio.
- A stateless design suitable for containerized, high-availability deployments.
- Structured JSON logging with `tracing`.
- Graceful shutdown handling for `SIGTERM` and `SIGINT`.
- A multi-stage Dockerfile with a distroless base image.
- A generic Kubernetes manifest template.
- Correct HMAC-SHA256 request signing for the ACS API.

## How It Works

1. An application connects to this service on its exposed port (e.g., 25).
2. The application sends SMTP commands (`EHLO`, `MAIL FROM`, `RCPT TO`, `DATA`).
3. The service accepts the email's raw data.
4. It parses the necessary components (subject, body) from the raw data.
5. It constructs and signs an HTTP request for the Azure Communication Services Email API.
6. It sends the request to Azure, relaying the SMTP email over HTTPS.

## Configuration

The service is configured entirely through environment variables.

| Variable                     | Description                                                                                                                                                    | Example                                       |
|------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------|
| `ACS_CONNECTION_STRING`      | Required. The connection string for your Azure Communication Services resource.                                                                                | `endpoint=https://...;accesskey=...`          |
| `ACS_SENDER_ADDRESS`         | Required. The verified "MailFrom" address in your ACS Email domain. This is used as the default sender.                                                          | `DoNotReply@your-verified-domain.com`         |
| `LISTEN_ADDR`                | The IP and port the SMTP server should listen on. Defaults to `0.0.0.0:1025`.                                                                                    | `0.0.0.0:1025`                                |
| `RUST_LOG`                   | The logging level. See [EnvFilter docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) for syntax.                    | `info` (Default), `acs_smtp_relay=debug,warn` |
| `MAX_EMAIL_SIZE`             | Optional. The maximum allowed size (in bytes) for an email's `DATA` section. Defaults to 10MB. Emails exceeding this size are rejected with a 552 error.           | `10485760` (10MB)                             |
| `ACS_ALLOWED_SENDER_DOMAINS` | Optional. A comma-separated list of domains. If set, the SMTP `MAIL FROM` address will be used as the sender if its domain matches one in this list. Otherwise, the default `ACS_SENDER_ADDRESS` is used. | `example.com,notify.example.com`              |

---

## Container Images

The following image tags are available:

- `ghcr.io/arressjay/smtp-acs-bridge:v1.1.0` (For a specific version)
- `ghcr.io/arressjay/smtp-acs-bridge:main` (Tracks the `main` branch)

### 2. Deploying to Kubernetes (Production)

See the provided manifest in `k8s/acs-relay.yaml` for a production-ready deployment.

Edit the manifest file at `k8s/acs-relay.yaml`:

- Replace `<YOUR_CONTAINER_IMAGE_PATH>` with the image path from GHCR (e.g., `ghcr.io/arressjay/smtp-acs-bridge:v1.1.0`).
- Replace `<YOUR_SENDER_ADDRESS>` with your verified "MailFrom" address from Azure.

Then, apply the configured manifest to your cluster:

```bash
kubectl apply -f k8s/acs-relay.yaml -n my-namespace
```

## CI/CD

This project uses GitHub Actions for continuous integration and delivery.

- **CI:** On every push and pull request, the workflow checks formatting, lints the code, and runs the full test suite.
- **Docker Build:** On every push to the `main` branch, a new multi-arch container image is built and pushed to GHCR. When a version tag (e.g., `v1.1.0`) is pushed, it's also tagged with the version number.
- **Release Drafter:** This action automatically drafts release notes from merged pull requests, simplifying the release process.

## Local Development

- **Prerequisites:** Install the Rust toolchain.
- **Run Tests:** The project has unit, integration, and protocol tests. Run them with:

```bash
cargo test --all-features
```

## SMTP Authentication Compatibility

Some SMTP clients require the server to advertise and accept the `AUTH` command, even if authentication is not enforced. This service implements a no-op AUTH handler for compatibility:

- The server advertises `AUTH PLAIN LOGIN` in response to `EHLO`.
- Any `AUTH` command is accepted and immediately replied to with a success message.
- No credentials are checked; authentication is a no-op. Security should be enforced at the network or deployment level.

This allows clients that require authentication (such as Microsoft Autodiscover Service or some mail clients) to connect and relay mail successfully.

## License

This project is licensed under the MIT License. See the `Cargo.toml` file for details.
