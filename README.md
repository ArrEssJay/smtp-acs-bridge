# SMTP to Azure Communication Services Bridge

A TCP SMTP server that relays incoming email messages to the Azure Communication Services Email API.

## Overview

This application accepts SMTP connections on port 1025 and forwards email messages to Azure Communication Services using the REST API. It implements the SMTP protocol for compatibility with standard email clients and applications.

## Features

- SMTP protocol implementation (EHLO, MAIL FROM, RCPT TO, DATA)
- Azure Communication Services Email API integration
- HMAC-SHA256 authentication for Azure requests
- Configurable email size limits
- Structured logging with configurable levels
- Optional HTTP health check endpoints
- Docker container support

## Requirements

- Rust 1.77 or later
- Azure Communication Services Email resource
- Network access to Azure Communication Services endpoints

## Configuration

Configure the application using environment variables:

| Variable | Description | Required | Default |
|----------|-------------|----------|---------|
| `ACS_CONNECTION_STRING` | Azure Communication Services connection string | Yes | - |
| `ACS_SENDER_ADDRESS` | Email address for the sender field | Yes | - |
| `LISTEN_ADDR` | SMTP server bind address | No | `127.0.0.1:1025` |
| `MAX_EMAIL_SIZE` | Maximum email size in bytes | No | `25485760` |
| `ACS_ALLOWED_SENDER_DOMAINS` | Comma-separated list of allowed sender domains | No | - |
| `RUST_LOG` | Log level configuration | No | `info` |

## Installation

### From Source

```bash
git clone https://github.com/ArrEssJay/smtp-acs-bridge.git
cd smtp-acs-bridge
cargo build --release
```

### Docker

```bash
docker pull ghcr.io/arressjay/smtp-acs-bridge:latest
```

## Usage

### Local Development

```bash
export ACS_CONNECTION_STRING="endpoint=https://your-acs-resource.communication.azure.com/;accesskey=your-access-key"
export ACS_SENDER_ADDRESS="noreply@yourdomain.com"
cargo run
```

### Docker

```bash
docker run -p 1025:1025 \
  -e ACS_CONNECTION_STRING="endpoint=https://your-acs-resource.communication.azure.com/;accesskey=your-access-key" \
  -e ACS_SENDER_ADDRESS="noreply@yourdomain.com" \
  ghcr.io/arressjay/smtp-acs-bridge:latest
```

### Azure Container Instances

```bash
az container create \
  --resource-group myResourceGroup \
  --name smtp-relay \
  --image ghcr.io/arressjay/smtp-acs-bridge:latest \
  --ports 1025 \
  --environment-variables \
    ACS_CONNECTION_STRING="endpoint=https://your-acs-resource.communication.azure.com/;accesskey=your-access-key" \
    ACS_SENDER_ADDRESS="noreply@yourdomain.com"
```

## SMTP Protocol Support

The server implements these SMTP commands:

- `EHLO` - Extended Hello with authentication advertising
- `HELO` - Basic Hello
- `MAIL FROM` - Sender specification
- `RCPT TO` - Recipient specification
- `DATA` - Message data transfer
- `RSET` - Reset transaction
- `QUIT` - Close connection
- `AUTH` - Authentication (accepts any credentials)

## Testing

This project uses a combination of unit, integration, and manual tests to ensure correctness and reliability.

### Running All Automated Tests

To run the complete suite of automated tests, exactly as they are run in the CI pipeline, use the following command. This includes all unit tests and the integration tests that use mock services.

```bash
cargo test --all-features
```

### Test Categories

The tests are organized into several categories:

#### 1. Unit Tests

-   **Location:** Inside `src/` modules, within `#[cfg(test)]` blocks.
-   **Purpose:** To test individual functions and components in isolation (e.g., configuration parsing, metrics calculations).
-   **How to Run Separately:** `cargo test`

#### 2. Integration Tests

-   **Location:** The `tests/` directory.
-   **Purpose:** To test how different parts of the application work together. These require the `mocks` feature flag, which is enabled by `--all-features`.
-   **Key Tests:**
    -   `smtp_flow.rs`: Verifies the SMTP command flow (`EHLO`, `MAIL FROM`, `DATA`, etc.) is handled correctly by the server. It uses a **mock mailer** to isolate the SMTP protocol logic from the Azure API.
    -   `acs_mailer_integration.rs`: Tests the `AcsMailer` struct's ability to correctly format and sign requests for the Azure API. It uses **`wiremock`** to simulate the Azure API endpoint, ensuring our HTTP requests are correct.
    -   `lettre_e2e.rs`: A full end-to-end test that starts the relay server and uses the `lettre` SMTP client to send an email through it to a **mocked Azure API**. This is the most comprehensive automated test, validating the entire chain from SMTP client to ACS request generation.

#### 3. Manual End-to-End Test

-   **File:** `tests/send_test_email.rs`
-   **Purpose:** To perform a true end-to-end test by sending an email through a running instance of the relay to the **real Azure Communication Services API**. This test is ignored by default (`#[ignore]`) and is intended for manual validation against a live environment.

##### **How to Run the Manual Test:**

1.  **Start the relay server** in one terminal, configured with your real Azure credentials:
    ```bash
    ACS_CONNECTION_STRING="endpoint=https://...;accesskey=..." \
    ACS_SENDER_ADDRESS="DoNotReply@your-domain.com" \
    cargo run
    ```

2.  **In a second terminal, run the specific test** with the required environment variables. Note that `SMTP_USER` and `SMTP_PASS` can be dummy values as they are not validated by the relay.
    ```bash
    SMTP_USER="user" SMTP_PASS="pass" \
    RECIPIENT_EMAIL="your-test-recipient@example.com" \
    ACS_SENDER_ADDRESS="DoNotReply@your-domain.com" \
    cargo test --test send_test_email --features mocks -- --ignored --nocapture
    ```

## Health Checks

When built with `--features health-server`, the application provides HTTP endpoints:

- `GET /health` - Basic health status
- `GET /metrics` - Application metrics in JSON format
- `GET /ready` - Readiness check for container orchestration

Enable health server:
```bash
cargo build --features health-server
```

## Monitoring

The application logs structured JSON messages. Key log fields include:

- `timestamp` - ISO 8601 timestamp
- `level` - Log level (ERROR, WARN, INFO, DEBUG, TRACE)
- `message` - Log message
- `peer_addr` - Client IP address
- `email_size` - Message size in bytes
- `recipient_count` - Number of recipients

## Deployment Considerations

### Security

- Run as non-root user in production
- Use TLS termination at load balancer level
- Restrict network access to required ports
- Rotate Azure access keys regularly

### Performance

- Configure appropriate resource limits
- Monitor memory usage with large emails
- Set reasonable email size limits
- Use connection pooling for high throughput

### Azure Integration

- Store connection strings in Azure Key Vault
- Use Azure Monitor for log aggregation
- Configure auto-scaling based on connection count
- Use Azure Container Registry for image storage

## Error Handling

The application returns standard SMTP response codes:

- `220` - Service ready
- `250` - Requested action completed
- `354` - Start mail input
- `503` - Bad sequence of commands
- `552` - Message size exceeds limit
- `421` - Service not available

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass
5. Submit a pull request

## License

MIT License. See LICENSE file for details.