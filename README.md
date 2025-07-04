# Rust SMTP to Azure ACS Email Relay

[![Docker Build & Push](https://github.com/ArrEssJay/smtp-acs-bridge/actions/workflows/docker.yml/badge.svg)](https://github.com/ArrEssJay/smtp-acs-bridge/actions/workflows/docker.yml)

This service relays emails received via SMTP to the Azure Communication Services (ACS) Email REST API. It is designed to allow applications that can only send email via SMTP to integrate with the modern Azure API.

## Key Features

- **High-Performance SMTP Server**: Built with Rust and Tokio for maximum performance and reliability
- **Production-Ready**: Custom error types, comprehensive configuration validation, and structured logging
- **Metrics & Monitoring**: Built-in metrics collection with periodic logging and optional health endpoints
- **Security**: Proper HMAC-SHA256 request signing for ACS API authentication
- **RFC Compliance**: Strict SMTP protocol compliance including dot-stuffing and command sequence validation
- **Containerized**: Multi-stage Dockerfile with distroless base image for minimal attack surface
- **Kubernetes Ready**: Generic manifest template included for easy deployment
- **Graceful Shutdown**: Handles SIGTERM and SIGINT properly for safe container operations
- **Configurable Limits**: Email size limits, connection timeouts, and concurrent connection controls

## How It Works

1. An application connects to this service on its exposed port (e.g., 1025)
2. The application sends SMTP commands (`EHLO`, `MAIL FROM`, `RCPT TO`, `DATA`)
3. The service accepts the email's raw data with proper RFC compliance
4. It parses the necessary components (subject, body) from the raw email data
5. It constructs and signs an HTTP request for the Azure Communication Services Email API
6. It sends the request to Azure, relaying the SMTP email over HTTPS
7. Metrics are collected and logged periodically for monitoring purposes

## Configuration

The service is configured entirely through environment variables. All configuration is validated at startup to ensure proper operation.

| Variable                     | Required | Description                                                                                                                                                    | Default | Example                                       |
|------------------------------|----------|----------------------------------------------------------------------------------------------------------------------------------------------------------------|---------|-----------------------------------------------|
| `ACS_CONNECTION_STRING`      | **Yes**  | The connection string for your Azure Communication Services resource.                                                                                | N/A     | `endpoint=https://...;accesskey=...`          |
| `ACS_SENDER_ADDRESS`         | **Yes**  | The verified "MailFrom" address in your ACS Email domain. This is used as the default sender.                                                          | N/A     | `DoNotReply@your-verified-domain.com`         |
| `LISTEN_ADDR`                | No       | The IP and port the SMTP server should listen on.                                                                                    | `0.0.0.0:1025` | `0.0.0.0:1025`                                |
| `RUST_LOG`                   | No       | The logging level. See [EnvFilter docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) for syntax.                    | `info` | `info`, `acs_smtp_relay=debug,warn` |
| `MAX_EMAIL_SIZE`             | No       | The maximum allowed size (in bytes) for an email's `DATA` section. Emails exceeding this size are rejected with a 552 error.           | `25485760` (25MB) | `10485760` (10MB)                             |
| `ACS_ALLOWED_SENDER_DOMAINS` | No       | A comma-separated list of domains. If set, the SMTP `MAIL FROM` address will be used as the sender if its domain matches one in this list. Otherwise, the default `ACS_SENDER_ADDRESS` is used. | None | `example.com,notify.example.com`              |

### Configuration Validation

The service validates all configuration at startup, including:
- SMTP bind address and port accessibility
- ACS endpoint URL format and access key validity
- Email address format validation
- Domain format validation for allowed sender domains
- Resource limits (email size, timeouts)

### Security Notes

- The service uses HMAC-SHA256 for Azure API authentication
- Email addresses and domains are validated for basic format compliance
- Privileged port binding (< 1024) requires appropriate permissions
- All configuration values are logged (except sensitive access keys)

## Monitoring & Metrics

The service includes built-in metrics collection and monitoring capabilities:

### Built-in Metrics
- **Connection Metrics**: Total connections, active connections
- **Email Metrics**: Total sent, failed, success rate
- **Performance Metrics**: Average response times
- **System Metrics**: Uptime, version information

### Metrics Logging
Metrics are automatically logged every 5 minutes in JSON format for easy integration with log aggregation systems.

### Optional Health Check Server
Enable the health-server feature for HTTP health endpoints:

```bash
cargo build --features health-server
```

When enabled, the following endpoints are available:
- `/health` - Basic health status
- `/metrics` - Detailed metrics in JSON format  
- `/ready` - Readiness probe for Kubernetes

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

### Prerequisites
- Install the Rust toolchain (1.70+ recommended)
- All dependencies are managed through Cargo

### Building
```bash
# Standard build
cargo build

# Build with all features (including health server and mocks)
cargo build --all-features

# Release build for production
cargo build --release
```

### Testing
The project has comprehensive test coverage including unit, integration, and protocol tests:

```bash
# Run all tests (requires mocks feature)
cargo test --features mocks

# Run tests with all features
cargo test --all-features

# Run specific test suites
cargo test --test smtp_flow --features mocks
cargo test --test lettre_e2e --features mocks
```

### Development Features
- **Mock Support**: Enable with `--features mocks` for testing without real Azure resources
- **Health Server**: Enable with `--features health-server` for HTTP monitoring endpoints

## SMTP Protocol Compliance

This service implements strict RFC compliance for robust email handling:

### Supported SMTP Commands
- **HELO/EHLO**: Connection initialization with capability advertisement
- **MAIL FROM**: Sender specification with address validation
- **RCPT TO**: Recipient specification (multiple recipients supported)
- **DATA**: Email content transfer with proper dot-stuffing handling
- **AUTH**: No-op authentication for client compatibility (any credentials accepted)
- **RSET**: Transaction reset
- **QUIT**: Clean connection termination

### Protocol Features
- **Dot-stuffing**: Proper handling of lines beginning with dots (RFC 5321)
- **Command Sequencing**: Strict validation of SMTP command order
- **Size Limits**: Configurable email size limits with 552 error responses
- **Connection Management**: Graceful handling of client disconnections
- **Error Handling**: Appropriate SMTP error codes for various failure scenarios

### Authentication Compatibility
Some SMTP clients require the server to advertise and accept the `AUTH` command, even if authentication is not enforced. This service implements a no-op AUTH handler for compatibility:

- The server advertises `AUTH PLAIN LOGIN` in response to `EHLO`
- Any `AUTH` command is accepted and immediately replied to with a success message
- No credentials are checked; authentication is a no-op. Security should be enforced at the network or deployment level

This allows clients that require authentication (such as Microsoft Autodiscover Service or some mail clients) to connect and relay mail successfully.

## Manual Integration Testing

To test the relay against the real Azure Communication Services API, you can use the manual integration test.

### Prerequisites
- A valid Azure Communication Services resource with Email capabilities
- A verified sender address configured in your ACS resource
- The relay server built with mocks feature: `cargo build --features mocks`

### Running the Test

**Terminal 1 - Start the SMTP relay server:**
```bash
# Replace with your real ACS connection string and verified sender address
ACS_CONNECTION_STRING="endpoint=https://your-resource.communication.azure.com/;accesskey=your-access-key" \
ACS_SENDER_ADDRESS="DoNotReply@your-verified-domain.com" \
cargo run
```

**Terminal 2 - Run the integration test:**
```bash
# The SMTP_USER and SMTP_PASS can be any value (authentication is no-op)
# Replace RECIPIENT_EMAIL with a real email address you can check
SMTP_USER="testuser" \
SMTP_PASS="testpass" \
RECIPIENT_EMAIL="you@example.com" \
ACS_SENDER_ADDRESS="DoNotReply@your-verified-domain.com" \
cargo test --test send_test_email --features mocks -- --nocapture
```

### Expected Behavior
- The test will connect to the SMTP server on localhost:1025
- It will send a test email through the relay to Azure Communication Services
- You should receive the test email at the specified recipient address
- Check server logs for detailed request/response information

## License

This project is licensed under the MIT License. See the `Cargo.toml` file for details.
