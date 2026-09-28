//! Structured final proxy configuration; credentials remain in the sealed record.
use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyAuthMode {
    #[default]
    None,
    UsernamePassword,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyExitConfiguration {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub auth_mode: ProxyAuthMode,
    #[serde(default)]
    pub dns_servers: Vec<IpAddr>,
}

#[derive(Clone)]
pub struct ProxyProfile {
    pub protocol: ChainProtocol,
    pub endpoint: Endpoint,
    pub auth_mode: ProxyAuthMode,
    pub dns_servers: Vec<IpAddr>,
}

pub(super) fn parse(
    source: ChainSource,
    secrets: &ImportSecrets,
) -> Result<ProxyProfile, ImportError> {
    let config = secrets
        .proxy
        .as_ref()
        .ok_or_else(|| ImportError::new(0, "proxy", "missing_configuration"))?;
    if !secrets.configuration.is_empty() || !secrets.private_key_password.is_empty() {
        return Err(ImportError::new(0, "configuration", "source_mismatch"));
    }
    let endpoint = Endpoint::parse(&config.host, &config.port.to_string(), 0)?;
    if config.dns_servers.len() > 8
        || config.dns_servers.iter().enumerate().any(|(i, ip)| {
            config.dns_servers[..i].contains(ip) || crate::config::invalid_vpn_dns_address(*ip)
        })
    {
        return Err(ImportError::new(0, "dns_servers", "invalid_dns"));
    }
    validate_credentials(source, config.auth_mode, secrets, false)?;
    Ok(ProxyProfile {
        protocol: if source == ChainSource::HttpProxy {
            ChainProtocol::HttpConnect
        } else {
            ChainProtocol::Socks5
        },
        endpoint,
        auth_mode: config.auth_mode,
        dns_servers: config.dns_servers.clone(),
    })
}

pub(super) fn validate_credentials(
    source: ChainSource,
    mode: ProxyAuthMode,
    secrets: &ImportSecrets,
    required: bool,
) -> Result<(), ImportError> {
    if mode == ProxyAuthMode::None {
        if !secrets.username.is_empty() || !secrets.password.is_empty() {
            return Err(ImportError::new(0, "credentials", "unexpected_credentials"));
        }
        return Ok(());
    }
    if required && (secrets.username.is_empty() || secrets.password.is_empty()) {
        return Err(ImportError::new(0, "credentials", "missing_credentials"));
    }
    if secrets
        .username
        .chars()
        .chain(secrets.password.chars())
        .any(char::is_control)
        || (source == ChainSource::HttpProxy && secrets.username.contains(':'))
        || (source == ChainSource::Socks5Proxy
            && (secrets.username.len() > 255 || secrets.password.len() > 255))
    {
        return Err(ImportError::new(0, "credentials", "invalid_credential"));
    }
    Ok(())
}
