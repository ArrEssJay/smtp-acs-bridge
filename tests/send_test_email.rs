// **MANUAL INTEGRATION TEST - Requires Running Server**
//
// This test sends an email via a running smtp-acs-bridge instance.
// It is designed for manual, true end-to-end testing against the real Azure ACS.
//
// **EXPECTED BEHAVIOR:** This test will FAIL if run without a server running.
// The failure "Connection refused" is normal and expected when no server is listening.
//
// # Manual Test Usage
// 1. In Terminal 1, run the `smtp-acs-bridge` with real Azure credentials:
//    ```bash
//    ACS_CONNECTION_STRING="endpoint=https://your-acs.communication.azure.com/;accesskey=..." \
//    ACS_SENDER_ADDRESS="DoNotReply@your-domain.com" \
//    cargo run
//    ```
//
// 2. In Terminal 2, run this test:
//    ```bash
//    SMTP_USER="user" SMTP_PASS="pass" \
//    RECIPIENT_EMAIL="you@example.com" \
//    ACS_SENDER_ADDRESS="DoNotReply@your-domain.com" \
//    cargo test --test send_test_email -- --nocapture
//    ```

use anyhow::{Context, Result};
use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, Message,
    SmtpTransport, Transport,
};
use std::env;

#[tokio::test]
#[ignore] // This test requires manual setup and a running server
async fn send_test_email() -> Result<()> {
    let smtp_host = env::var("SMTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let smtp_port_str = env::var("SMTP_PORT").unwrap_or_else(|_| "1025".to_string());
    let smtp_port = smtp_port_str.parse::<u16>()?;

    let smtp_user = env::var("SMTP_USER").context("SMTP_USER must be set")?;
    let smtp_pass = env::var("SMTP_PASS").context("SMTP_PASS must be set")?;
    let from_email = env::var("ACS_SENDER_ADDRESS")
        .context("ACS_SENDER_ADDRESS must be set (e.g., DoNotReply@your-domain.com)")?;
    let to_email = env::var("RECIPIENT_EMAIL").context("RECIPIENT_EMAIL must be set")?;

    println!("Building email...");
    let email = Message::builder()
        .from(from_email.parse()?)
        .to(to_email.parse()?)
        .subject("E2E Test from smtp-acs-bridge test tool")
        .header(ContentType::TEXT_PLAIN)
        .body("This is a test message.".to_string())?;

    let creds = Credentials::new(smtp_user, smtp_pass);

    println!("Connecting to {}:{}...", smtp_host, smtp_port);
    let mailer = SmtpTransport::builder_dangerous(&smtp_host)
        .port(smtp_port)
        .credentials(creds)
        .build();

    println!("Sending email...");
    let send_result = tokio::task::spawn_blocking(move || mailer.send(&email)).await??;

    // Now we can properly assert success.
    assert_eq!(
        send_result.code().severity,
        lettre::transport::smtp::response::Severity::PositiveCompletion
    );
    println!("Email sent successfully: {:?}", send_result);

    Ok(())
}
