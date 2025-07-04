use crate::error::{ConfigError, SmtpRelayError};
use anyhow::Result;
use base64::Engine;
use std::collections::HashMap;
use std::net::SocketAddr;
use url::Url;

// Configuration for the SMTP relay server
#[derive(Debug, Clone)]
pub struct Config {
    pub smtp_bind_address: SocketAddr,
    pub acs_config: AcsConfig,
    pub sender_address: String,
    pub allowed_sender_domains: Option<Vec<String>>,
    pub max_message_size: usize,
    pub connection_timeout: std::time::Duration,
    pub max_concurrent_connections: Option<usize>,
}

// Azure Communication Services configuration
#[derive(Debug, Clone)]
pub struct AcsConfig {
    pub endpoint: String,
    pub access_key: String,
}

impl Config {
    // Creates a new configuration with defaults and validates all settings
    pub fn new(
        smtp_bind_address: SocketAddr,
        connection_string: &str,
        sender_address: String,
        allowed_sender_domains: Option<Vec<String>>,
    ) -> Result<Self, SmtpRelayError> {
        let acs_config = parse_connection_string(connection_string)?;

        let config = Self {
            smtp_bind_address,
            acs_config,
            sender_address,
            allowed_sender_domains,
            max_message_size: 25 * 1024 * 1024, // 25MB default
            connection_timeout: std::time::Duration::from_secs(300), // 5 minutes
            max_concurrent_connections: Some(1000),
        };

        config.validate()?;
        Ok(config)
    }

    // Validates the entire configuration
    pub fn validate(&self) -> Result<(), SmtpRelayError> {
        self.validate_smtp_config()?;
        self.validate_acs_config()?;
        self.validate_sender_address()?;
        self.validate_allowed_domains()?;
        self.validate_limits()?;
        Ok(())
    }

    fn validate_smtp_config(&self) -> Result<(), SmtpRelayError> {
        if self.smtp_bind_address.port() == 0 {
            return Err(SmtpRelayError::Config(ConfigError::InvalidPort(0)));
        }

        if self.smtp_bind_address.port() < 1024 && !is_privileged_user() {
            return Err(SmtpRelayError::Config(ConfigError::InvalidPort(
                self.smtp_bind_address.port(),
            )));
        }

        Ok(())
    }

    fn validate_acs_config(&self) -> Result<(), SmtpRelayError> {
        // Validate endpoint URL
        Url::parse(&self.acs_config.endpoint).map_err(|_| {
            SmtpRelayError::Config(ConfigError::InvalidConnectionString(
                "Invalid endpoint URL".to_string(),
            ))
        })?;

        // Validate access key format (base64 string)
        if self.acs_config.access_key.is_empty() {
            return Err(SmtpRelayError::Config(ConfigError::MissingAccessKey));
        }

        base64::engine::general_purpose::STANDARD
            .decode(&self.acs_config.access_key)
            .map_err(|_| {
                SmtpRelayError::Config(ConfigError::InvalidConnectionString(
                    "Invalid access key format".to_string(),
                ))
            })?;

        Ok(())
    }

    fn validate_sender_address(&self) -> Result<(), SmtpRelayError> {
        if !is_valid_email(&self.sender_address) {
            return Err(SmtpRelayError::Config(ConfigError::InvalidSenderAddress(
                self.sender_address.clone(),
            )));
        }
        Ok(())
    }

    fn validate_allowed_domains(&self) -> Result<(), SmtpRelayError> {
        if let Some(domains) = &self.allowed_sender_domains {
            for domain in domains {
                if !is_valid_domain(domain) {
                    return Err(SmtpRelayError::Config(ConfigError::InvalidDomain(
                        domain.clone(),
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_limits(&self) -> Result<(), SmtpRelayError> {
        if self.max_message_size == 0 {
            return Err(SmtpRelayError::Config(
                ConfigError::InvalidConnectionString(
                    "Message size limit must be greater than 0".to_string(),
                ),
            ));
        }

        if self.connection_timeout.is_zero() {
            return Err(SmtpRelayError::Config(
                ConfigError::InvalidConnectionString(
                    "Connection timeout must be greater than 0".to_string(),
                ),
            ));
        }

        Ok(())
    }
}

// Parses a connection string like "endpoint=...;accesskey=..." into an AcsConfig struct
pub fn parse_connection_string(conn_str: &str) -> Result<AcsConfig, SmtpRelayError> {
    let map: HashMap<_, _> = conn_str
        .split(';')
        .filter_map(|s| s.split_once('='))
        .collect();

    let endpoint = map
        .get("endpoint")
        .ok_or(SmtpRelayError::Config(ConfigError::MissingEndpoint))?
        .trim_end_matches('/')
        .to_string();

    let access_key = map
        .get("accesskey")
        .ok_or(SmtpRelayError::Config(ConfigError::MissingAccessKey))?
        .to_string();

    Ok(AcsConfig {
        endpoint,
        access_key,
    })
}

// Basic email address validation
fn is_valid_email(email: &str) -> bool {
    email.contains('@') && email.len() > 3 && !email.starts_with('@') && !email.ends_with('@')
}

// Basic domain validation
fn is_valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.starts_with('-')
        && !domain.ends_with('-')
}

// Check if running as privileged user (simplified)
fn is_privileged_user() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::getuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        true // Assume privileged on non-Unix systems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_parse_connection_string() {
        let conn_str = "endpoint=https://example.communication.azure.com/;accesskey=dGVzdA==";
        let config = parse_connection_string(conn_str).unwrap();
        assert_eq!(config.endpoint, "https://example.communication.azure.com");
        assert_eq!(config.access_key, "dGVzdA==");
    }

    #[test]
    fn test_parse_connection_string_missing_endpoint() {
        let conn_str = "accesskey=dGVzdA==";
        let result = parse_connection_string(conn_str);
        assert!(matches!(
            result,
            Err(SmtpRelayError::Config(ConfigError::MissingEndpoint))
        ));
    }

    #[test]
    fn test_validate_email() {
        assert!(is_valid_email("test@example.com"));
        assert!(is_valid_email("user+tag@domain.co.uk"));
        assert!(!is_valid_email("@example.com"));
        assert!(!is_valid_email("test@"));
        assert!(!is_valid_email("test"));
    }

    #[test]
    fn test_validate_domain() {
        assert!(is_valid_domain("example.com"));
        assert!(is_valid_domain("sub.example.com"));
        assert!(!is_valid_domain(".example.com"));
        assert!(!is_valid_domain("example.com."));
        assert!(!is_valid_domain("-example.com"));
    }

    #[test]
    fn test_config_validation() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2525);
        let conn_str = "endpoint=https://example.communication.azure.com/;accesskey=dGVzdEtleQ==";

        let config = Config::new(
            addr,
            conn_str,
            "test@example.com".to_string(),
            Some(vec!["example.com".to_string()]),
        );

        assert!(config.is_ok());
    }
}
