use acs_smtp_relay::relay::{AcsMailer, Mailer};
use acs_smtp_relay::{parse_connection_string, run};
use anyhow::{Context, Result};
use std::env;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing::subscriber::set_global_default(
        fmt::Subscriber::builder().with_env_filter(EnvFilter::from_default_env()).json().finish(),
    ).context("Failed to set global logger")?;

    let connection_string = env::var("ACS_CONNECTION_STRING").context("ACS_CONNECTION_STRING must be set")?;
    let sender_address = env::var("ACS_SENDER_ADDRESS").context("ACS_SENDER_ADDRESS must be set")?;
    let listen_addr = env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:1025".to_string());
    let max_email_size = env::var("MAX_EMAIL_SIZE")
        .unwrap_or_else(|_| "10485760".to_string()) // Default to 10MB
        .parse::<usize>()
        .context("Failed to parse MAX_EMAIL_SIZE as an integer")?;

    let allowed_sender_domains = env::var("ACS_ALLOWED_SENDER_DOMAINS")
        .ok()
        .map(|s| s.split(',').map(|d| d.trim().to_string()).collect());

    let acs_config = parse_connection_string(&connection_string)?;

    let http_client = reqwest::Client::new();
    let mailer: Arc<dyn Mailer> = Arc::new(AcsMailer::new(
        http_client,
        acs_config.endpoint,
        acs_config.access_key,
        sender_address,
        allowed_sender_domains,
    ));

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!(%listen_addr, max_email_size_bytes = max_email_size, "Minimal SMTP relay listening for connections");

    run(listener, mailer, max_email_size).await;

    tracing::info!("Server has shut down gracefully.");
    Ok(())
}