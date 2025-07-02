# Rust SMTP to Azure ACS Relay (Production Ready)

This service acts as a simple bridge between applications that send email via SMTP (like `matrix-authentication-service`) and the Azure Communication Services (ACS) Email REST API.

This version is hardened for production use with:
- Structured JSON logging via `tracing`.
- Graceful shutdown on `SIGTERM`/`SIGINT`.
- High-availability and security-hardened Kubernetes manifests.

## Prerequisites

-   [Rust](https://www.rust-lang.org/tools/install)
-   [Docker](https://www.docker.com/get-started/)
-   [kubectl](https://kubernetes.io/docs/tasks/tools/) connected to your cluster

## Configuration

The service is configured entirely through environment variables:

| Variable                | Description                                                                                              | Example                                                                  |
| ----------------------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `ACS_CONNECTION_STRING` | The connection string for your Azure Communication Services resource. **(Required)**                       | `endpoint=https://...;accesskey=...`                                     |
| `ACS_SENDER_ADDRESS`    | The verified "MailFrom" address in your ACS Email domain. **(Required)**                                 | `DoNotReply@your-verified-domain.com`                                    |
| `LISTEN_ADDR`           | The IP and port the SMTP server should listen on.                                                        | `0.0.0.0:1025` (Default)                                                 |
| `RUST_LOG`              | The logging level for the `tracing` subscriber.                                                          | `info` (Default), `acs_smtp_relay=debug,warn`                            |

## How to Use

### 1. Build the Docker Image

From the root of the project directory, run the build command. **Remember to replace `yourregistry.azurecr.io` with your own container registry.**

```bash
docker build -t yourregistry.azurecr.io/acs-smtp-relay:latest .
```

### 2. Push the Image

```bash
docker push yourregistry.azurecr.io/acs-smtp-relay:latest
```

### 3. Deploy to Kubernetes

First, edit the k8s/acs-relay.yaml file to:
- Update the ACS_CONNECTION_STRING in the Secret object with your real value.
- Update the image field in the Deployment to point to the image you just pushed.
- Ensure the namespace field matches your Matrix deployment namespace.

Then, apply the manifest to your cluster.

```bash
kubectl apply -f k8s/acs-relay.yaml
```

### 4. Configure Matrix Authentication Service (MAS)

In your MAS Helm chart values.yaml, configure the email settings to point to your new service:

```yaml
mas:
  config:
    email:
      transport: smtp
      from: '"Your Matrix Server" <noreply@your.matrix.host>'
      smtp:
        # Use the Kubernetes internal DNS name for the service
        host: "acs-smtp-relay-svc.matrix.svc.cluster.local" # Adjust namespace if needed
        port: 25
        # 'plain' mode disables TLS and STARTTLS, which is appropriate for in-cluster traffic.
        mode: plain
```

Deploy the changes to MAS, and it will now send all its emails through your new highly-available relay.
