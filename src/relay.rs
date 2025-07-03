use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use hmac::{Hmac, Mac};
use mail_parser::{Message, MessageParser};
use reqwest::{header, Client, Method};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::{info, instrument};
use url::Url;

// --- Data Structures for the ACS Email API Payload ---

#[derive(Serialize)]
pub struct AcsEmailAddress<'a> {
    address: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcsRecipients<'a> {
    to: Vec<AcsEmailAddress<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcsEmailContent {
    subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    plain_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcsEmailRequest<'a> {
    sender_address: &'a str,
    content: AcsEmailContent,
    recipients: AcsRecipients<'a>,
}

#[cfg(feature = "mocks")]
use mockall::automock;

/// A trait for sending emails, allowing for mock implementations in tests.
#[cfg_attr(feature = "mocks", automock)]
#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, raw_email: &[u8], recipients: &[String], from: &Option<String>) -> Result<()>;
}

/// A concrete Mailer implementation for Azure Communication Services.
pub struct AcsMailer {
    client: Client,
    api_endpoint: String,
    api_key: String,
    sender_address: String,
    allowed_sender_domains: Option<Vec<String>>,
}

impl AcsMailer {
    pub fn new(
        client: Client,
        endpoint: String,
        key: String,
        sender: String,
        allowed_sender_domains: Option<Vec<String>>,
    ) -> Self {
        Self {
            client,
            api_endpoint: endpoint,
            api_key: key,
            sender_address: sender,
            allowed_sender_domains,
        }
    }
}

/// Helper function to build the ACS request payload from a parsed email.
fn build_acs_request<'a>(
    parsed_email: &'a Message,
    recipients: &'a [String],
    sender_address: &'a str,
) -> Result<AcsEmailRequest<'a>> {
    if recipients.is_empty() {
        return Err(anyhow!("Cannot build message with no recipients"));
    }
    let subject = parsed_email.subject().unwrap_or("No Subject").to_string();
    let text_body = parsed_email.text_body.get(0).map(|s| s.to_string());
    let html_body = parsed_email.html_body.get(0).map(|s| s.to_string());
    if text_body.is_none() && html_body.is_none() {
        return Err(anyhow!("Email content is empty (both text and html)"));
    }
    let content = AcsEmailContent {
        subject,
        plain_text: text_body,
        html: html_body,
    };
    let recipients_struct = AcsRecipients {
        to: recipients
            .iter()
            .map(|addr| AcsEmailAddress { address: addr })
            .collect(),
    };
    Ok(AcsEmailRequest {
        sender_address,
        content,
        recipients: recipients_struct,
    })
}

#[async_trait]
impl Mailer for AcsMailer {
    #[instrument(skip_all, fields(recipient_count = recipients.len()))]
    async fn send(&self, raw_email: &[u8], recipients: &[String], from: &Option<String>) -> Result<()> {
        let sender_for_request = if let (Some(allowed_domains), Some(from_address)) =
            (&self.allowed_sender_domains, from)
        {
            // SMTP addresses are often like `<user@domain.com>`, so trim brackets.
            let trimmed_from = from_address.trim_matches(|c| c == '<' || c == '>');
            if let Some(from_domain) = trimmed_from.split('@').nth(1) {
                if allowed_domains.iter().any(|d| d == from_domain) {
                    info!(client_sender = %trimmed_from, "Using client-provided sender address based on allowed domain list.");
                    trimmed_from.to_string()
                } else {
                    tracing::warn!(client_sender = %trimmed_from, fallback_sender = %self.sender_address, "Client-provided sender domain is not in the allow-list. Using default sender.");
                    self.sender_address.clone()
                }
            } else {
                tracing::warn!(invalid_from = %from_address, "Could not parse domain from client-provided 'MAIL FROM' address. Using default sender.");
                self.sender_address.clone()
            }
        } else {
            self.sender_address.clone()
        };
        info!("Parsing raw email data.");
        let parsed_email = MessageParser::default()
            .parse(raw_email)
            .context("Failed to parse raw email")?;
        info!("Building ACS request payload.");
        let request_payload = build_acs_request(&parsed_email, recipients, &sender_for_request)?;
        let body_bytes = serde_json::to_vec(&request_payload)?;
        // --- HMAC-SHA256 Authentication ---
        const API_VERSION: &str = "2023-03-31";
        let url_path = format!("/emails:send?api-version={}", API_VERSION);
        let full_url = format!("{}{}", self.api_endpoint, url_path);
        let parsed_url = Url::parse(&full_url)?;
        let host = parsed_url.host_str().context("Endpoint URL has no host")?;
        // 1. Get timestamp
        let timestamp = Utc::now().to_rfc2822();
        // 2. Hash the body
        let mut hasher = Sha256::new();
        hasher.update(&body_bytes);
        let content_hash = B64.encode(hasher.finalize());
        // 3. Construct the string to sign
        let string_to_sign = format!(
            "{}\n{}\n{};host:{};x-ms-content-sha256:{}",
            Method::POST.as_str(),
            url_path,
            timestamp,
            host,
            content_hash
        );
        info!(string_to_sign = %string_to_sign, "Generated string-to-sign for HMAC");
        // 4. Create the signature
        let decoded_key = B64.decode(&self.api_key).context("Failed to decode API key")?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&decoded_key)?;
        mac.update(string_to_sign.as_bytes());
        let signature = B64.encode(mac.finalize().into_bytes());
        // 5. Build the Authorization header
        let auth_header = format!(
            "HMAC-SHA256 SignedHeaders=x-ms-date;host;x-ms-content-sha256&Signature={}",
            signature
        );
        info!(url = %full_url, "Sending signed request to ACS API.");
        let response = self
            .client
            .post(&full_url)
            .header("x-ms-date", timestamp)
            .header("x-ms-content-sha256", content_hash)
            .header(header::AUTHORIZATION, auth_header)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body_bytes)
            .send()
            .await
            .context("Failed to send HTTP request to ACS")?;
        info!(status = %response.status(), "Received response from ACS");
        response
            .error_for_status()
            .context("ACS API returned an error status")?;
        info!("Successfully relayed email to ACS.");
        Ok(())
    }
}