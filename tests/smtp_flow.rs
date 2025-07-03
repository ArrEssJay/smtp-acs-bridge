use acs_smtp_relay::relay::{MockMailer, Mailer};
use acs_smtp_relay::handle_connection;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Helper function to read the two-line EHLO response.
async fn read_ehlo_response(reader: &mut BufReader<io::ReadHalf<TcpStream>>) {
    let mut line_buf = String::new();
    reader.read_line(&mut line_buf).await.unwrap();
    // Assert the first line has a hyphen
    assert!(line_buf.starts_with("250-"));
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    // Assert the last line has a space
    assert!(line_buf.starts_with("250 "));
}

#[tokio::test]
async fn test_smtp_session_flow() {
    let mut mock_mailer = MockMailer::new();
    let raw_email_body = "Subject: Test\r\n\r\nHello world\r\n";
    
    mock_mailer.expect_send()
        .withf(move |data, recipients, from| {
            data == raw_email_body.as_bytes()
                && recipients == ["<to@example.com>"]
                && from.as_deref() == Some("<from@example.com>")
        })
        .times(1)
        .returning(|_, _, _| Ok(()));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mailer_arc: Arc<dyn Mailer> = Arc::new(mock_mailer);

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        handle_connection(stream, mailer_arc, 10_000_000).await;
    });

    let (read_half, mut write_half) = io::split(TcpStream::connect(addr).await.unwrap());
    let mut reader = BufReader::new(read_half);
    let mut line_buf = String::new();

    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("220"));

    write_half.write_all(b"EHLO client.example.com\r\n").await.unwrap();
    read_ehlo_response(&mut reader).await;
    
    write_half.write_all(b"MAIL FROM:<from@example.com>\r\n").await.unwrap();
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("250"));

    write_half.write_all(b"RCPT TO:<to@example.com>\r\n").await.unwrap();
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("250"));

    write_half.write_all(b"DATA\r\n").await.unwrap();
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("354"));

    write_half.write_all(raw_email_body.as_bytes()).await.unwrap();
    write_half.write_all(b".\r\n").await.unwrap();
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("250"));

    write_half.write_all(b"QUIT\r\n").await.unwrap();
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("221"));
}

#[tokio::test]
async fn test_smtp_auth_flow() {
    let mut mock_mailer = MockMailer::new();
    
    // AUTH is a no-op, so we don't expect a send.
    // This test just verifies the AUTH command is accepted.
    mock_mailer.expect_send().times(0);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mailer_arc: Arc<dyn Mailer> = Arc::new(mock_mailer);

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        handle_connection(stream, mailer_arc, 10_000_000).await;
    });

    let (read_half, mut write_half) = io::split(TcpStream::connect(addr).await.unwrap());
    let mut reader = BufReader::new(read_half);
    let mut line_buf = String::new();

    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("220"));

    write_half.write_all(b"EHLO client.example.com\r\n").await.unwrap();
    read_ehlo_response(&mut reader).await;
    
    write_half.write_all(b"AUTH PLAIN dGVzdAB0ZXN0AHRlc3Q=\r\n").await.unwrap();
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("235"));

    // Quit after successful auth to confirm the state is correct.
    write_half.write_all(b"QUIT\r\n").await.unwrap();
    line_buf.clear();
    reader.read_line(&mut line_buf).await.unwrap();
    assert!(line_buf.starts_with("221"));
}
