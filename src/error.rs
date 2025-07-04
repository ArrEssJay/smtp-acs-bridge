use std::fmt;

// Custom error types for the SMTP-to-ACS relay
#[derive(Debug)]
pub enum SmtpRelayError {
    // Configuration errors
    Config(ConfigError),
    // SMTP protocol errors
    Smtp(SmtpError),
    // Azure Communication Services API errors
    Acs(AcsError),
    // Email parsing/validation errors
    Email(EmailError),
    // Network/IO errors
    Network(NetworkError),
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidConnectionString(String),
    MissingEndpoint,
    MissingAccessKey,
    InvalidSenderAddress(String),
    InvalidDomain(String),
    InvalidPort(u16),
}

#[derive(Debug)]
pub enum SmtpError {
    InvalidCommand(String),
    InvalidSequence(String),
    MessageTooLarge(usize, usize), // actual, max
    InvalidAddress(String),
    MissingFrom,
    NoRecipients,
    DataCorrupted,
}

#[derive(Debug)]
pub enum AcsError {
    ApiRequest(String),
    AuthenticationFailed,
    Unauthorized,
    RateLimited,
    ServiceUnavailable,
    InvalidResponse(String),
}

#[derive(Debug)]
pub enum EmailError {
    ParseFailed(String),
    MissingSubject,
    MissingContent,
    InvalidEncoding(String),
    UnsupportedContentType(String),
}

#[derive(Debug)]
pub enum NetworkError {
    ConnectionLost,
    Timeout,
    DnsResolution(String),
    TlsHandshake(String),
}

impl fmt::Display for SmtpRelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmtpRelayError::Config(e) => write!(f, "Configuration error: {e}"),
            SmtpRelayError::Smtp(e) => write!(f, "SMTP protocol error: {e}"),
            SmtpRelayError::Acs(e) => write!(f, "Azure Communication Services error: {e}"),
            SmtpRelayError::Email(e) => write!(f, "Email processing error: {e}"),
            SmtpRelayError::Network(e) => write!(f, "Network error: {e}"),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingEndpoint => write!(f, "Missing endpoint in connection string"),
            ConfigError::MissingAccessKey => write!(f, "Missing access key in connection string"),
            ConfigError::InvalidConnectionString(s) => write!(f, "Invalid connection string: {s}"),
            ConfigError::InvalidSenderAddress(addr) => write!(f, "Invalid sender address: {addr}"),
            ConfigError::InvalidDomain(domain) => write!(f, "Invalid domain: {domain}"),
            ConfigError::InvalidPort(port) => write!(f, "Invalid port: {port}"),
        }
    }
}

impl fmt::Display for SmtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmtpError::InvalidCommand(cmd) => write!(f, "Invalid SMTP command: {cmd}"),
            SmtpError::InvalidSequence(seq) => write!(f, "Invalid command sequence: {seq}"),
            SmtpError::MessageTooLarge(actual, max) => {
                write!(f, "Message too large: {actual} bytes (max: {max})")
            }
            SmtpError::InvalidAddress(addr) => write!(f, "Invalid email address: {addr}"),
            SmtpError::MissingFrom => write!(f, "Missing MAIL FROM command"),
            SmtpError::NoRecipients => write!(f, "No recipients specified"),
            SmtpError::DataCorrupted => write!(f, "DATA section corrupted"),
        }
    }
}

impl fmt::Display for AcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcsError::AuthenticationFailed => write!(f, "Authentication failed (401)"),
            AcsError::Unauthorized => write!(f, "Unauthorized (403)"),
            AcsError::RateLimited => write!(f, "Rate limited (429)"),
            AcsError::ServiceUnavailable => write!(f, "Service unavailable (5xx)"),
            AcsError::ApiRequest(msg) => write!(f, "API request failed: {msg}"),
            AcsError::InvalidResponse(resp) => write!(f, "Invalid response from ACS: {resp}"),
        }
    }
}

impl fmt::Display for EmailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmailError::ParseFailed(msg) => write!(f, "Failed to parse email: {msg}"),
            EmailError::MissingSubject => write!(f, "Missing subject in email"),
            EmailError::MissingContent => write!(f, "Missing content in email"),
            EmailError::InvalidEncoding(enc) => write!(f, "Invalid encoding: {enc}"),
            EmailError::UnsupportedContentType(ct) => write!(f, "Unsupported content type: {ct}"),
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::ConnectionLost => write!(f, "Connection lost"),
            NetworkError::Timeout => write!(f, "Network timeout"),
            NetworkError::DnsResolution(host) => write!(f, "DNS resolution failed for: {host}"),
            NetworkError::TlsHandshake(msg) => write!(f, "TLS handshake failed: {msg}"),
        }
    }
}

impl std::error::Error for SmtpRelayError {}
impl std::error::Error for ConfigError {}
impl std::error::Error for SmtpError {}
impl std::error::Error for AcsError {}
impl std::error::Error for EmailError {}
impl std::error::Error for NetworkError {}

// Convenient conversion from anyhow::Error
impl From<anyhow::Error> for SmtpRelayError {
    fn from(_err: anyhow::Error) -> Self {
        SmtpRelayError::Network(NetworkError::ConnectionLost)
    }
}

// HTTP status code mapping for ACS errors
impl AcsError {
    pub fn from_status_code(status: u16, body: &str) -> Self {
        match status {
            401 => AcsError::AuthenticationFailed,
            403 => AcsError::Unauthorized,
            429 => AcsError::RateLimited,
            502..=504 => AcsError::ServiceUnavailable,
            _ => AcsError::ApiRequest(format!("HTTP {status}: {body}")),
        }
    }
}
