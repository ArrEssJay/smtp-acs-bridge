use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use hmac::{Hmac, Mac};
use mail_parser::{Message, MessageParser};
use reqwest::{header, Client, Method};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::{info, instrument, warn};
use url::Url;

// --- Data Structures for the ACS Email API Payload ---

#[derive(Serialize, Debug)]
pub struct AcsEmailAddress<'a> {
    address: &'a str,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AcsRecipients<'a> {
    to: Vec<AcsEmailAddress<'a>>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AcsEmailContent {
    subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    plain_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
}

#[derive(Serialize, Debug)]
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
    async fn send(&self, raw_email: &[u8], recipients: &[String], from: &Option<String>)
        -> Result<()>;
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

    /// Generates the necessary headers for HMAC-SHA256 authentication with the ACS API.
    fn sign_request(
        &self,
        method: &Method,
        url_path: &str,
        body_bytes: &[u8],
    ) -> Result<(String, String, String)> {
        let full_url = format!("{}{}", self.api_endpoint, url_path);
        let parsed_url = Url::parse(&full_url)?;
        let host = parsed_url.host_str().context("Endpoint URL has no host")?;

        let timestamp = Utc::now().to_rfc2822();

        let mut hasher = Sha256::new();
        hasher.update(body_bytes);
        let content_hash = B64.encode(hasher.finalize());

        let string_to_sign = format!(
            "{}\n{}\n{};host:{};x-ms-content-sha256:{}",
            method.as_str(),
            url_path,
            timestamp,
            host,
            &content_hash
        );
        info!(string_to_sign = %string_to_sign, "Generated string-to-sign for HMAC");

        let decoded_key = B64.decode(&self.api_key).context("Failed to decode API key")?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&decoded_key)?;
        mac.update(string_to_sign.as_bytes());
        let signature = B64.encode(mac.finalize().into_bytes());

        let auth_header = format!(
            "HMAC-SHA256 SignedHeaders=x-ms-date;host;x-ms-content-sha256&Signature={}",
            signature
        );
        Ok((timestamp, content_hash, auth_header))
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

    let text_body = parsed_email.body_text(0).map(|s| s.trim().to_string());
    let html_body = parsed_email.body_html(0).map(|s| s.trim().to_string());

    let text_body_empty = text_body.as_ref().map_or(true, |s| s.is_empty());
    let html_body_empty = html_body.as_ref().map_or(true, |s| {
        let normalized = s.replace(char::is_whitespace, "");
        normalized.is_empty() || normalized == "<html><body></body></html>"
    });

    if text_body_empty && html_body_empty {
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
    async fn send(
        &self,
        raw_email: &[u8],
        recipients: &[String],
        from: &Option<String>,
    ) -> Result<()> {
        let sender_for_request = if let (Some(allowed_domains), Some(from_address)) =
            (&self.allowed_sender_domains, from)
        {
            let trimmed_from = from_address.trim_matches(|c| c == '<' || c == '>');
            if let Some(from_domain) = trimmed_from.split('@').nth(1) {
                if allowed_domains.iter().any(|d| d == from_domain) {
                    info!(client_sender = %trimmed_from, "Using client-provided sender address");
                    trimmed_from.to_string()
                } else {
                    warn!(client_sender = %trimmed_from, fallback_sender = %self.sender_address, "Sender not in allow-list, using default");
                    self.sender_address.clone()
                }
            } else {
                warn!(invalid_from = %from_address, "Could not parse domain from MAIL FROM, using default");
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

        const API_VERSION: &str = "2023-03-31";
        let url_path = format!("/emails:send?api-version={}", API_VERSION);
        let (timestamp, content_hash, auth_header) =
            self.sign_request(&Method::POST, &url_path, &body_bytes)?;

        info!(url = %self.api_endpoint, sender = %sender_for_request, "Sending signed request to ACS API.");
        let response = self
            .client
            .post(format!("{}{}", self.api_endpoint, url_path))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_acs_request_rejects_empty_email() {
        let empty_message = MessageParser::new().parse(b"Subject: Empty\r\n\r\n").unwrap();
        let recipients = vec!["to@example.com".to_string()];
        let result = build_acs_request(&empty_message, &recipients, "sender@example.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Email content is empty"));
    }
}