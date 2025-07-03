use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tracing::{error, info, instrument, warn};

pub mod relay;
use relay::Mailer;

/// Holds the parsed configuration from the Azure connection string.
#[derive(Debug)]
pub struct AcsConfig {
    pub endpoint: String,
    pub access_key: String,
}

/// Parses a connection string like "endpoint=...;accesskey=..." into an AcsConfig struct.
pub fn parse_connection_string(conn_str: &str) -> Result<AcsConfig> {
    let map: HashMap<_, _> = conn_str.split(';').filter_map(|s| s.split_once('=')).collect();
    let endpoint = map.get("endpoint").context("Connection string is missing 'endpoint'")?.to_string();
    let access_key = map.get("accesskey").context("Connection string is missing 'accesskey'")?.to_string();
    Ok(AcsConfig { endpoint, access_key })
}

/// Represents the state of a single SMTP transaction (one email).
#[derive(Default, Clone)]
struct Transaction {
    from: Option<String>,
    recipients: Vec<String>,
}

/// Writes a standard SMTP response line to the client stream.
async fn write_response(stream: &mut io::WriteHalf<TcpStream>, code: u16, text: &str) -> Result<()> {
    let response = format!("{} {}\r\n", code, text);
    stream.write_all(response.as_bytes()).await?;
    info!(client_response = %response.trim(), "Sent response");
    Ok(())
}

/// Handles a single, complete client TCP connection, processing one or more SMTP transactions.
#[instrument(skip_all, fields(peer_addr = %stream.peer_addr().map_or_else(|_| "unknown".to_string(), |a| a.to_string())))]
pub async fn handle_connection(stream: TcpStream, mailer: Arc<dyn Mailer>, max_email_size: usize) {
    let peer_addr = stream.peer_addr().map_or_else(|_| "unknown".to_string(), |a| a.to_string());
    info!(client_addr = %peer_addr, "New client connection");
    let (read_half, mut write_half) = io::split(stream);
    let mut reader = BufReader::new(read_half);

    let mut line = String::new();
    let mut transaction = Transaction::default();

    if write_response(&mut write_half, 220, "acs-smtp-relay ready").await.is_err() { return; }

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => { info!(client_addr = %peer_addr, "Client disconnected"); break; }
            Ok(_) => {
                let cmd = line.trim().to_uppercase();
                
                // -- HELO/EHLO --
                if cmd.starts_with("HELO") || cmd.starts_with("EHLO") {
                    transaction = Transaction::default(); // Reset on (re)-greeting
                    if write_response(&mut write_half, 250, "OK").await.is_err() { break; }
                
                // -- MAIL FROM --
                } else if cmd.starts_with("MAIL FROM:") {
                    transaction = Transaction::default(); // Start new transaction
                    transaction.from = Some(line.trim()[10..].trim().to_string());
                    if write_response(&mut write_half, 250, "OK").await.is_err() { break; }
                
                // -- RCPT TO --
                } else if cmd.starts_with("RCPT TO:") {
                    if transaction.from.is_none() {
                        if write_response(&mut write_half, 503, "Bad sequence of commands").await.is_err() { break; }
                    } else {
                        transaction.recipients.push(line.trim()[8..].trim().to_string());
                        if write_response(&mut write_half, 250, "OK").await.is_err() { break; }
                    }
                
                // -- DATA --
                } else if cmd.starts_with("DATA") {
                    if transaction.recipients.is_empty() {
                         if write_response(&mut write_half, 503, "Bad sequence of commands").await.is_err() { break; }
                         continue;
                    }
                    if write_response(&mut write_half, 354, "Start mail input; end with <CRLF>.<CRLF>").await.is_err() { break; }
                    
                    // Read the email body until the terminating line ".\r\n"
                    let mut email_data = Vec::new();
                    loop {
                        let mut data_line = String::new();
                        match reader.read_line(&mut data_line).await {
                             Ok(0) => break, // Client disconnected unexpectedly
                             Ok(_) => {
                                if email_data.len() + data_line.len() > max_email_size {
                                    error!(client_addr = %peer_addr, size = email_data.len(), max_size = max_email_size, "Email size exceeds maximum limit");
                                    let _ = write_response(&mut write_half, 552, "Requested mail action aborted: exceeded storage allocation").await;
                                    return; // Close connection
                                }
                                 if data_line == ".\r\n" { break; }
                                 // Handle dot-stuffing (a leading '.' is escaped as '..')
                                 let line_to_write = if data_line.starts_with('.') { &data_line[1..] } else { &data_line };
                                 email_data.extend_from_slice(line_to_write.as_bytes());
                             }
                             Err(e) => { error!(client_addr = %peer_addr, error = ?e, "Error reading email data"); return; }
                        }
                    }

                    // Relay the collected email data
                    info!(client_addr = %peer_addr, email_size = email_data.len(), "Received email data. Relaying...");
                    match mailer.send(&email_data, &transaction.recipients, &transaction.from).await {
                        Ok(_) => { if write_response(&mut write_half, 250, "OK: Queued for delivery").await.is_err() { break; } }
                        Err(e) => {
                            error!(client_addr = %peer_addr, error = ?e, "Failed to relay email");
                            if write_response(&mut write_half, 451, "Requested action aborted: local error in processing").await.is_err() { break; }
                        }
                    }
                    transaction = Transaction::default(); // Reset for next email
                
                // -- QUIT --
                } else if cmd.starts_with("QUIT") {
                    let _ = write_response(&mut write_half, 221, "Bye").await;
                    break;
                
                // -- RSET --
                } else if cmd.starts_with("RSET") {
                    transaction = Transaction::default();
                    if write_response(&mut write_half, 250, "OK").await.is_err() { break; };
                }
                else {
                    warn!(client_addr = %peer_addr, command = %line.trim(), "Unrecognized command");
                    if write_response(&mut write_half, 500, "Syntax error, command unrecognized").await.is_err() { break; }
                }
            }
            Err(e) => { error!(client_addr = %peer_addr, error = ?e, "Error reading from client"); break; }
        }
    }
}

/// Listens for graceful shutdown signals (Ctrl+C, SIGTERM).
async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.expect("failed to install Ctrl+C handler"); };
    #[cfg(unix)]
    let terminate = async { signal::unix::signal(signal::unix::SignalKind::terminate()).expect("failed to install signal handler").recv().await; };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {}, }
    info!("Signal received, starting graceful shutdown.");
}

/// The main application loop. Binds to the listener and hands off connections.
pub async fn run(listener: TcpListener, mailer: Arc<dyn Mailer>, max_email_size: usize) {
    loop {
        tokio::select! {
            // Accept new connections and spawn a task to handle them.
            Ok((stream, _)) = listener.accept() => {
                let mailer_clone = mailer.clone();
                tokio::spawn(async move { handle_connection(stream, mailer_clone, max_email_size).await; });
            }
            // Await the shutdown signal.
            _ = shutdown_signal() => { info!("Shutting down server..."); break; }
            // Handle a listener error, which is fatal.
            else => { error!("TCP listener failed"); break; }
        }
    }
}

// Unit tests for logic contained within this file.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn test_email_size_limit_enforced() {
        // Dummy mailer that just returns Ok
        struct DummyMailer;
        #[async_trait::async_trait]
        impl Mailer for DummyMailer {
            async fn send(&self, _raw_email: &[u8], _recipients: &[String], _from: &Option<String>) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mailer = Arc::new(DummyMailer);
        let max_email_size = 100; // 100 bytes

        // Spawn the server
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, mailer, max_email_size).await;
        });

        // Connect as a client
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1024];
        // Read greeting
        let _ = stream.read(&mut buf).await.unwrap();
        // Send SMTP commands
        stream.write_all(b"HELO test.example.com\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"MAIL FROM:<from@example.com>\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"RCPT TO:<to@example.com>\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"DATA\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        // Send a body that exceeds the limit
        let big_body = "A".repeat(101) + "\r\n.\r\n";
        stream.write_all(big_body.as_bytes()).await.unwrap();
        let n = stream.read(&mut buf).await.unwrap();
        let response = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(response.contains("552"), "Expected 552 error, got: {}", response);
    }

    #[tokio::test]
    async fn test_mailer_send_receives_from_argument() {
        use std::sync::Mutex;
        struct DummyMailer {
            pub last_from: Arc<Mutex<Option<Option<String>>>>,
        }
        #[async_trait::async_trait]
        impl Mailer for DummyMailer {
            async fn send(&self, _raw_email: &[u8], _recipients: &[String], from: &Option<String>) -> anyhow::Result<()> {
                let mut guard = self.last_from.lock().unwrap();
                *guard = Some(from.clone());
                Ok(())
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let last_from = Arc::new(Mutex::new(None));
        let mailer = Arc::new(DummyMailer { last_from: last_from.clone() });
        let max_email_size = 1000;
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, mailer, max_email_size).await;
        });
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"HELO test.example.com\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"MAIL FROM:<from@example.com>\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"RCPT TO:<to@example.com>\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"DATA\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"Hello\r\n.\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        // Check that the DummyMailer received the correct 'from' argument
        let from_value = last_from.lock().unwrap().clone();
        assert_eq!(from_value, Some(Some("<from@example.com>".to_string())));
    }

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

    #[tokio::test]
    async fn test_client_addr_in_logs() {
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::{fmt, EnvFilter};
        use std::sync::mpsc;

        // Set up a channel to capture logs
        let (tx, rx) = mpsc::channel();
        let tx = Arc::new(Mutex::new(tx));
        struct ChannelWriter {
            tx: Arc<Mutex<mpsc::Sender<String>>>,
        }
        impl std::io::Write for ChannelWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let s = String::from_utf8_lossy(buf).to_string();
                let _ = self.tx.lock().unwrap().send(s);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let make_writer = {
            let tx = tx.clone();
            move || ChannelWriter { tx: tx.clone() }
        };
        let subscriber = fmt()
            .with_env_filter(EnvFilter::new("info"))
            .with_writer(make_writer)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        struct DummyMailer;
        #[async_trait::async_trait]
        impl Mailer for DummyMailer {
            async fn send(&self, _raw_email: &[u8], _recipients: &[String], _from: &Option<String>) -> anyhow::Result<()> {
                Ok(())
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mailer = Arc::new(DummyMailer);
        let max_email_size = 1000;
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, mailer, max_email_size).await;
        });
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"HELO test.example.com\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"QUIT\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        // Collect logs
        let logs: Vec<String> = rx.try_iter().collect();
        let found = logs.iter().any(|log| log.contains("client_addr"));
        assert!(found, "Expected client_addr in logs, got: {:?}", logs);
    }
}
