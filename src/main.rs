use acs_smtp_relay::relay::{AcsMailer, Mailer};
use acs_smtp_relay::{metrics, run, Config, MetricsCollector};
use anyhow::{Context, Result};
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing::subscriber::set_global_default(
        fmt::Subscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .json()
            .finish(),
    )
    .context("Failed to set global logger")?;

    let connection_string =
        env::var("ACS_CONNECTION_STRING").context("ACS_CONNECTION_STRING must be set")?;
    let sender_address =
        env::var("ACS_SENDER_ADDRESS").context("ACS_SENDER_ADDRESS must be set")?;
    let listen_addr = env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:1025".to_string());
    let max_email_size = env::var("MAX_EMAIL_SIZE")
        .unwrap_or_else(|_| "25485760".to_string()) // Default to 25MB
        .parse::<usize>()
        .context("Failed to parse MAX_EMAIL_SIZE as an integer")?;

    let allowed_sender_domains = env::var("ACS_ALLOWED_SENDER_DOMAINS")
        .ok()
        .map(|s| s.split(',').map(|d| d.trim().to_string()).collect());

    // Parse listen address
    let smtp_bind_address: SocketAddr = listen_addr
        .parse()
        .context("Failed to parse LISTEN_ADDR as a socket address")?;

    // Create and validate configuration
    let mut config = Config::new(
        smtp_bind_address,
        &connection_string,
        sender_address,
        allowed_sender_domains,
    )
    .map_err(|e| anyhow::anyhow!("Configuration error: {}", e))?;

    // Override with environment variables if provided
    config.max_message_size = max_email_size;

    // Re-validate after modifications
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("Configuration validation failed: {}", e))?;

    // Create HTTP client with connection pooling
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .timeout(Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    let mailer: Arc<dyn Mailer> = Arc::new(AcsMailer::new(
        http_client,
        config.acs_config.endpoint.clone(),
        config.acs_config.access_key.clone(),
        config.sender_address.clone(),
        config.allowed_sender_domains.clone(),
    ));

    // Set up metrics collection
    let metrics_collector = MetricsCollector::new();

    // Start metrics logging every 5 minutes
    metrics::start_metrics_logger(metrics_collector.clone(), Duration::from_secs(300));

    let listener = TcpListener::bind(config.smtp_bind_address).await?;
    // Get the actual address the listener is bound to.
    let actual_addr = listener.local_addr()?;
    tracing::info!(
        listen_addr = %actual_addr,
        max_email_size_bytes = config.max_message_size,
        connection_timeout_secs = config.connection_timeout.as_secs(),
        max_concurrent_connections = ?config.max_concurrent_connections,
        "SMTP-to-ACS relay listening for connections"
    );

    // In production, pass None for the shutdown signal (uses Ctrl+C/SIGTERM)
    run(
        listener,
        mailer,
        config.max_message_size,
        actual_addr.ip().to_string(),
    )
    .await;

    tracing::info!("Server has shut down gracefully.");
    Ok(())
}
