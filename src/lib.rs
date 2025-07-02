use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tracing::{error, info, instrument, warn};

pub mod relay;
use relay::Mailer;

#[derive(Debug)]
pub struct AcsConfig {
    pub endpoint: String,
    pub access_key: String,
}

pub fn parse_connection_string(conn_str: &str) -> Result<AcsConfig> {
    let map: HashMap<_, _> = conn_str.split(';').filter_map(|s| s.split_once('=')).collect();
    let endpoint = map.get("endpoint").context("Connection string is missing 'endpoint'")?.to_string();
    let access_key = map.get("accesskey").context("Connection string is missing 'accesskey'")?.to_string();
    Ok(AcsConfig { endpoint, access_key })
}

#[derive(Default, Clone)]
struct Transaction {
    from: Option<String>,
    recipients: Vec<String>,
}

async fn write_response(stream: &mut io::WriteHalf<TcpStream>, code: u16, text: &str) -> Result<()> {
    let response = format!("{} {}\r\n", code, text);
    stream.write_all(response.as_bytes()).await?;
    info!(client_response = %response.trim(), "Sent response");
    Ok(())
}

#[instrument(skip_all, fields(peer_addr = %stream.peer_addr().map_or_else(|_| "unknown".to_string(), |a| a.to_string())))]
pub async fn handle_connection(stream: TcpStream, mailer: Arc<dyn Mailer>) {
    info!("New client connection");
    let (read_half, mut write_half) = io::split(stream);
    let mut reader = BufReader::new(read_half);
    if write_response(&mut write_half, 220, "acs-smtp-relay ready").await.is_err() { return; }
    let mut line = String::new();
    let mut transaction = Transaction::default();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => { info!("Client disconnected"); break; }
            Ok(_) => {
                let cmd = line.trim().to_uppercase();
                if cmd.starts_with("HELO") || cmd.starts_with("EHLO") {
                    transaction = Transaction::default();
                    if write_response(&mut write_half, 250, "OK").await.is_err() { break; }
                } else if cmd.starts_with("MAIL FROM:") {
                    transaction = Transaction::default();
                    transaction.from = Some(line.trim()[10..].trim().to_string());
                    if write_response(&mut write_half, 250, "OK").await.is_err() { break; }
                } else if cmd.starts_with("RCPT TO:") {
                    if transaction.from.is_none() {
                        if write_response(&mut write_half, 503, "Bad sequence of commands").await.is_err() { break; }
                    } else {
                        transaction.recipients.push(line.trim()[8..].trim().to_string());
                        if write_response(&mut write_half, 250, "OK").await.is_err() { break; }
                    }
                } else if cmd.starts_with("DATA") {
                    if transaction.recipients.is_empty() {
                         if write_response(&mut write_half, 503, "Bad sequence of commands").await.is_err() { break; }
                         continue;
                    }
                    if write_response(&mut write_half, 354, "Start mail input; end with <CRLF>.<CRLF>").await.is_err() { break; }
                    let mut email_data = Vec::new();
                    loop {
                        let mut data_line = String::new();
                        match reader.read_line(&mut data_line).await {
                             Ok(0) => break,
                             Ok(_) => {
                                 if data_line == ".\r\n" { break; }
                                 let line_to_write = if data_line.starts_with('.') { &data_line[1..] } else { &data_line };
                                 email_data.extend_from_slice(line_to_write.as_bytes());
                             }
                             Err(e) => { error!(error = ?e, "Error reading email data"); return; }
                        }
                    }
                    info!("Received {} bytes of email data. Relaying...", email_data.len());
                    match mailer.send(&email_data, &transaction.recipients).await {
                        Ok(_) => { if write_response(&mut write_half, 250, "OK: Queued for delivery").await.is_err() { break; } }
                        Err(e) => {
                            error!(error = ?e, "Failed to relay email");
                            if write_response(&mut write_half, 451, "Requested action aborted: local error in processing").await.is_err() { break; }
                        }
                    }
                    transaction = Transaction::default();
                } else if cmd.starts_with("QUIT") {
                    let _ = write_response(&mut write_half, 221, "Bye").await;
                    break;
                } else if cmd.starts_with("RSET") {
                    transaction = Transaction::default();
                    if write_response(&mut write_half, 250, "OK").await.is_err() { break; };
                }
                else {
                    warn!(command = %line.trim(), "Unrecognized command");
                    if write_response(&mut write_half, 500, "Syntax error, command unrecognized").await.is_err() { break; }
                }
            }
            Err(e) => { error!(error = ?e, "Error reading from client"); break; }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.expect("failed to install Ctrl+C handler"); };
    #[cfg(unix)]
    let terminate = async { signal::unix::signal(signal::unix::SignalKind::terminate()).expect("failed to install signal handler").recv().await; };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {}, }
    info!("Signal received, starting graceful shutdown.");
}

pub async fn run(listener: TcpListener, mailer: Arc<dyn Mailer>) {
    loop {
        tokio::select! {
            Ok((stream, _)) = listener.accept() => {
                let mailer_clone = mailer.clone();
                tokio::spawn(async move { handle_connection(stream, mailer_clone).await; });
            }
            _ = shutdown_signal() => { info!("Shutting down server..."); break; }
            else => { error!("TCP listener failed"); break; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_parse_connection_string_success() {
        let conn_str = "endpoint=https://example.com;accesskey=12345";
        let config = parse_connection_string(conn_str).unwrap();
        assert_eq!(config.endpoint, "https://example.com");
        assert_eq!(config.access_key, "12345");
    }
    #[test]
    fn test_parse_connection_string_missing_endpoint() {
        let conn_str = "accesskey=12345";
        let result = parse_connection_string(conn_str);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing 'endpoint'"));
    }
    #[test]
    fn test_parse_connection_string_missing_key() {
        let conn_str = "endpoint=https://example.com;";
        let result = parse_connection_string(conn_str);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing 'accesskey'"));
    }
}
