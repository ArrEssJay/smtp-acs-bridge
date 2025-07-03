use acs_smtp_relay::relay::{AcsMailer, Mailer};
use base64::Engine;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_acs_mailer_sends_correct_request() {
    // Arrange
    let server = MockServer::start().await;
    
    // THE FIX: The expected body must include the 'html' field that mail-parser auto-generates.
    // The library wraps the plain text in a simple <p> tag.
    let expected_body = serde_json::json!({
      "senderAddress": "default@sender.com",
      "content": {
        "subject": "Test Email",
        "plainText": "One weird trick to get your emails delivered",
        "html": "<html><body>One weird trick to get your emails delivered</body></html>"
      },
      "recipients": {
        "to": [ { "address": "<to@example.com>" } ]
      }
    });

    Mock::given(method("POST")) 
        .and(path("/emails:send"))
        .and(query_param("api-version", "2023-03-31"))
        .and(body_json(expected_body.clone()))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;


    // Debugging: Log unmatched requests
    Mock::given(|_request: &wiremock::Request| true) // Match any request
    .respond_with({
        let expected_body = expected_body.clone();
        move |request: &wiremock::Request| {
            let expected_body = expected_body.clone();
            let body_str = String::from_utf8_lossy(&request.body);
            println!("\n--- UNMATCHED REQUEST RECEIVED ---");
            println!("EXPECTED BODY:\n{}", serde_json::to_string_pretty(&expected_body).unwrap());
            println!("\nACTUAL BODY:\n{}", body_str);
            println!("--- END UNMATCHED REQUEST ---\n");
            ResponseTemplate::new(404)
        }
    })
    .mount(&server)
    .await;

    let http_client = reqwest::Client::new();
    let access_key = base64::engine::general_purpose::STANDARD.encode("dummy_key");
    let mailer = AcsMailer::new(
        http_client,
        server.uri(),
        access_key,
        "default@sender.com".to_string(),
        None,
    );

    // Act
    let raw_email = concat!(
        "From: sender@example.com\r\n",
        "To: <to@example.com>\r\n",
        "Subject: Test Email\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "One weird trick to get your emails delivered"
    )
    .as_bytes();

    let recipients = vec!["<to@example.com>".to_string()];
    let from = Some("<ignored@client.com>".to_string());

    let result = mailer.send(raw_email, &recipients, &from).await;

    // Assert
    assert!(result.is_ok(), "AcsMailer::send error: {:?}", result);
    server.verify().await;
}

#[tokio::test]
async fn test_acs_mailer_sender_override() {
    // Arrange
    let server = MockServer::start().await;
    
    // THE FIX: The expected body must also be updated in this test.
    let expected_body = serde_json::json!({
      "senderAddress": "override@allowed.com",
      "content": {
        "subject": "Override Test",
        "plainText": "This is from an allowed sender.",
        "html": "<html><body><p>This is from an allowed sender.</p></body></html>"
      },
      "recipients": {
        "to": [ { "address": "<to@example.com>" } ]
      }
    });

    Mock::given(method("POST"))
        .and(path("/emails:send"))
        .and(query_param("api-version", "2023-03-31"))
        .and(body_json(expected_body.clone()))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;

    let http_client = reqwest::Client::new();
    let access_key = base64::engine::general_purpose::STANDARD.encode("dummy_key");
    let allowed_domains = Some(vec!["allowed.com".to_string()]);
    let mailer = AcsMailer::new(
        http_client,
        server.uri(),
        access_key,
        "default@sender.com".to_string(),
        allowed_domains,
    );

    // Act
    let raw_email = concat!(
        "Subject: Override Test\r\n",
        "Content-Type: text/plain\r\n",
        "\r\n",
        "This is from an allowed sender."
    )
    .as_bytes();
    let recipients = vec!["<to@example.com>".to_string()];
    let from = Some("<override@allowed.com>".to_string());
    let result = mailer.send(raw_email, &recipients, &from).await;

    // Assert
    assert!(result.is_ok(), "AcsMailer::send error: {:?}", result);
    server.verify().await;
}