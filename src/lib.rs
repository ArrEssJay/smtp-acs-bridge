use anyhow::Result;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tracing::{error, info, instrument, warn, Span};

pub mod config;
pub mod error;
#[cfg(feature = "health-server")]
pub mod health;
pub mod metrics;
pub mod relay;

pub use config::{parse_connection_string, AcsConfig, Config};
pub use error::SmtpRelayError;
pub use metrics::MetricsCollector;
use relay::Mailer;

// Represents the state of a single SMTP transaction (one email).
#[derive(Default, Clone)]
struct Transaction {
    from: Option<String>,
    recipients: Vec<String>,
}

// Writes a standard SMTP response line to the client stream.
async fn write_response(
    stream: &mut io::WriteHalf<TcpStream>,
    code: u16,
    text: &str,
) -> Result<()> {
    let response = format!("{} {}\r\n", code, text);
    stream.write_all(response.as_bytes()).await?;
    info!(client_response = %response.trim(), "Sent response");
    Ok(())
}

// Handles a single, complete client TCP connection, processing one or more SMTP transactions.
#[instrument(skip_all, name = "handle_connection")]
pub async fn handle_connection(
    stream: TcpStream,
    mailer: Arc<dyn Mailer>,
    max_email_size: usize,
    server_name: String,
) {
    info!("handle_connection: START");
    let peer_addr = stream
        .peer_addr()
        .map_or_else(|_| "unknown".to_string(), |a| a.to_string());
    // Manually add the peer_addr to the current span for better logging context.
    Span::current().record("peer_addr", peer_addr.as_str());

    info!(client_addr = %peer_addr, "New client connection");
    let (read_half, mut write_half) = io::split(stream);
    let mut reader = BufReader::new(read_half);

    if write_response(&mut write_half, 220, "acs-smtp-relay ready")
        .await
        .is_err()
    {
        info!("handle_connection: END (failed to send greeting)");
        return;
    }

    let mut line = String::new();
    let mut transaction = Transaction::default();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                info!(client_addr = %peer_addr, "Client disconnected");
                info!("handle_connection: END (client disconnected)");
                return;
            }
            Ok(_) => {
                let cmd = line.trim().to_uppercase();

                if cmd.starts_with("HELO") {
                    transaction = Transaction::default();
                    if write_response(&mut write_half, 250, &server_name)
                        .await
                        .is_err()
                    {
                        return;
                    }
                } else if cmd.starts_with("EHLO") {
                    transaction = Transaction::default();
                    let ehlo_response = format!("250-{}\r\n250 AUTH PLAIN LOGIN\r\n", server_name);
                    if write_half
                        .write_all(ehlo_response.as_bytes())
                        .await
                        .is_err()
                    {
                        return;
                    }
                    info!(client_response = %ehlo_response.trim(), "Sent EHLO response");
                } else if cmd.starts_with("AUTH") {
                    if cmd == "AUTH PLAIN" {
                        if write_response(&mut write_half, 334, "").await.is_err() {
                            return;
                        }
                        if reader.read_line(&mut line).await.is_err() {
                            return;
                        };
                    }
                    if write_response(&mut write_half, 235, "2.7.0 Authentication successful")
                        .await
                        .is_err()
                    {
                        return;
                    }
                } else if cmd.starts_with("MAIL FROM:") {
                    transaction = Transaction::default();
                    transaction.from = Some(line.trim()[10..].trim().to_string());
                    if write_response(&mut write_half, 250, "OK").await.is_err() {
                        return;
                    }
                } else if cmd.starts_with("RCPT TO:") {
                    if transaction.from.is_none() {
                        if write_response(&mut write_half, 503, "Bad sequence of commands")
                            .await
                            .is_err()
                        {
                            return;
                        }
                    } else {
                        transaction
                            .recipients
                            .push(line.trim()[8..].trim().to_string());
                        if write_response(&mut write_half, 250, "OK").await.is_err() {
                            return;
                        }
                    }
                } else if cmd.starts_with("DATA") {
                    if transaction.recipients.is_empty() {
                        if write_response(&mut write_half, 503, "Bad sequence of commands")
                            .await
                            .is_err()
                        {
                            return;
                        }
                        continue;
                    }
                    if write_response(
                        &mut write_half,
                        354,
                        "Start mail input; end with <CRLF>.<CRLF>",
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }

                    let mut email_data = Vec::new();
                    loop {
                        let mut data_line = String::new();
                        match reader.read_line(&mut data_line).await {
                            Ok(0) => {
                                info!(client_addr = %peer_addr, "Client disconnected during DATA");
                                return;
                            }
                            Ok(_) => {
                                if email_data.len() + data_line.len() > max_email_size {
                                    error!(client_addr = %peer_addr, size = email_data.len(), max_size = max_email_size, "Email size exceeds maximum limit");
                                    let _ = write_response(&mut write_half, 552, "Requested mail action aborted: exceeded storage allocation").await;
                                    return;
                                }
                                if data_line == ".\r\n" {
                                    break;
                                }
                                let line_to_write =
                                    if let Some(stripped) = data_line.strip_prefix('.') {
                                        stripped
                                    } else {
                                        &data_line
                                    };
                                email_data.extend_from_slice(line_to_write.as_bytes());
                            }
                            Err(e) => {
                                error!(client_addr = %peer_addr, error = ?e, "Error reading email data");
                                return;
                            }
                        }
                    }

                    info!(client_addr = %peer_addr, email_size = email_data.len(), "Received email data. Relaying...");
                    match mailer
                        .send(&email_data, &transaction.recipients, &transaction.from)
                        .await
                    {
                        Ok(_) => {
                            if write_response(&mut write_half, 250, "OK: Queued for delivery")
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(e) => {
                            error!(client_addr = %peer_addr, error = ?e, "Failed to relay email");
                            if write_response(
                                &mut write_half,
                                451,
                                "Requested action aborted: local error in processing",
                            )
                            .await
                            .is_err()
                            {
                                return;
                            }
                        }
                    }
                    transaction = Transaction::default();
                } else if cmd.starts_with("QUIT") {
                    let _ = write_response(&mut write_half, 221, "Bye").await;
                    return;
                } else if cmd.starts_with("RSET") {
                    transaction = Transaction::default();
                    if write_response(&mut write_half, 250, "OK").await.is_err() {
                        return;
                    }
                } else {
                    warn!(client_addr = %peer_addr, command = %line.trim(), "Unrecognized command");
                    if write_response(&mut write_half, 500, "Syntax error, command unrecognized")
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            Err(e) => {
                error!(client_addr = %peer_addr, error = ?e, "Error reading from client");
                info!("handle_connection: END (error)");
                return;
            }
        }
    }
    // info!("handle_connection: END (loop exit)"); // unreachable
}

// Listens for graceful shutdown signals (Ctrl+C, SIGTERM).
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    info!("Signal received, starting graceful shutdown.");
}

// The main application loop. Binds to the listener and hands off connections.
pub async fn run(
    listener: TcpListener,
    mailer: Arc<dyn Mailer>,
    max_email_size: usize,
    server_name: String,
) {
    println!(
        "run: START - server listening on {:?}",
        listener.local_addr()
    );
    info!(
        "run: START - server listening on {:?}",
        listener.local_addr()
    );
    loop {
        tokio::select! {
            Ok((stream, addr)) = listener.accept() => {
                info!("run: Accepted connection from {}", addr);
                let mailer_clone = mailer.clone();
                let server_name_clone = server_name.clone();
                tokio::spawn(async move {
                    info!("run: Spawning handle_connection for {}", addr);
                    handle_connection(stream, mailer_clone, max_email_size, server_name_clone).await;
                    info!("run: handle_connection for {} returned", addr);
                });
            }
            _ = shutdown_signal() => { info!("Shutting down server..."); break; }
            else => { error!("TCP listener failed"); break; }
        }
    }
    println!("run: END - server loop exited");
    info!("run: END - server loop exited (after shutdown)");
    // If you want to ensure all spawned tasks are finished, you could track JoinHandles here.
    // For now, the server loop is exited and the listener is dropped.
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
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        struct MockMailer;
        #[async_trait::async_trait]
        impl Mailer for MockMailer {
            async fn send(
                &self,
                _raw_email: &[u8],
                _recipients: &[String],
                _from: &Option<String>,
            ) -> anyhow::Result<()> {
                panic!("send should not be called when email size exceeds limit");
            }
        }

        let mailer = Arc::new(MockMailer);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let max_email_size = 100;
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, mailer, max_email_size, "acs.local".to_string()).await;
        });
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();
        stream
            .write_all(b"EHLO test.example.com\r\n")
            .await
            .unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream
            .write_all(b"MAIL FROM:<from@example.com>\r\n")
            .await
            .unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream
            .write_all(b"RCPT TO:<to@example.com>\r\n")
            .await
            .unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"DATA\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        // Send a body that exceeds the max_email_size
        let big_body = vec![b'a'; 200];
        stream.write_all(&big_body).await.unwrap();
        stream.write_all(b".\r\n").await.unwrap();
        let n = stream.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert!(
            response.contains("552"),
            "Expected 552 error, got: {}",
            response
        );
    }

    #[tokio::test]
    async fn test_mailer_send_receives_from_argument() {
        use std::sync::Mutex;
        struct DummyMailer {
            pub last_from: Arc<Mutex<Option<Option<String>>>>,
        }
        #[async_trait::async_trait]
        impl Mailer for DummyMailer {
            async fn send(
                &self,
                _raw_email: &[u8],
                _recipients: &[String],
                from: &Option<String>,
            ) -> anyhow::Result<()> {
                let mut guard = self.last_from.lock().unwrap();
                *guard = Some(from.clone());
                Ok(())
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let last_from = Arc::new(Mutex::new(None));
        let mailer = Arc::new(DummyMailer {
            last_from: last_from.clone(),
        });
        let max_email_size = 1000;
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, mailer, max_email_size, "acs.local".to_string()).await;
        });
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();
        stream
            .write_all(b"HELO test.example.com\r\n")
            .await
            .unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream
            .write_all(b"MAIL FROM:<from@example.com>\r\n")
            .await
            .unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream
            .write_all(b"RCPT TO:<to@example.com>\r\n")
            .await
            .unwrap();
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
        let config = config::parse_connection_string(conn_str).unwrap();
        assert_eq!(config.endpoint, "https://example.com");
        assert_eq!(config.access_key, "12345");
    }
    #[test]
    fn test_parse_connection_string_missing_endpoint() {
        let conn_str = "accesskey=12345";
        let result = config::parse_connection_string(conn_str);
        assert!(result.is_err());
    }
    #[test]
    fn test_parse_connection_string_missing_key() {
        let conn_str = "endpoint=https://example.com;";
        let result = config::parse_connection_string(conn_str);
        assert!(result.is_err());
    }
    #[test]
    fn test_parse_connection_string_trims_trailing_slash() {
        let conn_str = "endpoint=https://example.com/;accesskey=12345";
        let config = config::parse_connection_string(conn_str).unwrap();
        assert_eq!(config.endpoint, "https://example.com");
        assert_eq!(config.access_key, "12345");
    }

    #[tokio::test]
    async fn test_client_addr_in_logs() {
        use std::sync::mpsc;
        use std::sync::{Arc, Mutex};
        use tracing_subscriber::{fmt, EnvFilter};

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
            async fn send(
                &self,
                _raw_email: &[u8],
                _recipients: &[String],
                _from: &Option<String>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mailer = Arc::new(DummyMailer);
        let max_email_size = 1000;
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, mailer, max_email_size, "acs.local".to_string()).await;
        });
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();
        stream
            .write_all(b"HELO test.example.com\r\n")
            .await
            .unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"QUIT\r\n").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        // Collect logs
        let logs: Vec<String> = rx.try_iter().collect();
        let found = logs.iter().any(|log| log.contains("client_addr"));
        assert!(found, "Expected client_addr in logs, got: {:?}", logs);
    }
}
