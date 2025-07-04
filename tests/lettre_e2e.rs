use acs_smtp_relay::{config::parse_connection_string, relay::AcsMailer, run};
use base64::Engine;
use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, Message,
    SmtpTransport, Transport,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Integration test that verifies the end-to-end flow of sending an email

#[tokio::test]
async fn test_lettre_sends_email_through_bridge_to_mock_acs() -> anyhow::Result<()> {
    // --- 1. Set up the Mock Azure ACS API ---
    let acs_server = MockServer::start().await;
    let expected_body = serde_json::json!({
      "senderAddress": "DoNotReply@test.com",
      "content": {
        "subject": "Lettre E2E Test",
        "plainText": "Hello from Lettre!",
        "html": "<html><body>Hello from Lettre!<br/></body></html>"
      },
      "recipients": {
        "to": [ { "address": "<test@example.com>" } ]
      }
    });

    Mock::given(method("POST"))
        .and(path("/emails:send"))
        .and(body_json(expected_body))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&acs_server)
        .await;

    // --- 2. Start our smtp-acs-bridge application ---
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let bridge_addr = listener.local_addr()?;
    let bridge_port = bridge_addr.port();

    let access_key = base64::engine::general_purpose::STANDARD.encode("dummy_key");
    let conn_str = format!("endpoint={};accesskey={}", acs_server.uri(), access_key);
    let sender_address = "DoNotReply@test.com".to_string();

    let acs_config = parse_connection_string(&conn_str)?;
    let http_client = reqwest::Client::new();
    let mailer = Arc::new(AcsMailer::new(
        http_client,
        acs_config.endpoint,
        acs_config.access_key,
        sender_address.clone(),
        None,
    ));

    let server_handle = tokio::spawn(async move {
        run(listener, mailer, 10_000_000, bridge_addr.ip().to_string()).await;
    });

    // Give the server a moment to start up.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // --- 3. Use Lettre to send an email ---
    let email = Message::builder()
        .from(sender_address.parse()?)
        .to("test@example.com".parse()?)
        .subject("Lettre E2E Test")
        .header(ContentType::TEXT_PLAIN)
        .body(String::from("Hello from Lettre!"))?;

    let creds = Credentials::new("user".to_string(), "pass".to_string());
    let smtp_client = SmtpTransport::builder_dangerous("127.0.0.1")
        .port(bridge_port)
        .credentials(creds)
        .build();

    // spawn_blocking is used because the `lettre` SMTP transport is blocking.
    let send_result = tokio::task::spawn_blocking(move || smtp_client.send(&email)).await??;
    assert_eq!(
        send_result.code().severity,
        lettre::transport::smtp::response::Severity::PositiveCompletion
    );

    // --- 4. Verify mock and cleanly shut down the server task ---
    acs_server.verify().await;
    server_handle.abort();

    Ok(())
}
