use super::{Endpoint, ImportError};
use base64::Engine;
use ipnet::IpNet;
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct WireGuardProfile {
    pub private_key: Zeroizing<[u8; 32]>,
    pub public_key: [u8; 32],
    pub preshared_key: Option<Zeroizing<[u8; 32]>>,
    pub endpoint: Endpoint,
    pub addresses: Vec<IpNet>,
    pub dns_servers: Vec<IpAddr>,
    pub allowed_ips: Vec<IpNet>,
    pub mtu: u16,
    pub keepalive: Option<u16>,
}
impl WireGuardProfile {
    pub fn allows(&self, ip: IpAddr) -> bool {
        self.allowed_ips.iter().any(|net| net.contains(&ip))
    }
}
pub(super) fn parse(text: &str) -> Result<WireGuardProfile, ImportError> {
    let mut section = "";
    let mut interface = false;
    let mut peer = false;
    let mut values: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
    for (i, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = i + 1;
        let raw = raw.split('#').next().unwrap_or_default().trim();
        if raw.is_empty() {
            continue;
        }
        if raw.starts_with('[') {
            match raw {
                "[Interface]" if !interface && !peer => {
                    interface = true;
                    section = "interface";
                }
                "[Peer]" if interface && !peer => {
                    peer = true;
                    section = "peer";
                }
                _ => {
                    return Err(ImportError::new(
                        line,
                        "section",
                        "unsupported_or_duplicate_section",
                    ));
                }
            }
            continue;
        }
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| ImportError::new(line, "field", "invalid_syntax"))?;
        let (key, value) = (key.trim(), value.trim());
        let supported = match section {
            "interface" => matches!(key, "PrivateKey" | "Address" | "DNS" | "MTU"),
            "peer" => matches!(
                key,
                "PublicKey" | "PresharedKey" | "Endpoint" | "AllowedIPs" | "PersistentKeepalive"
            ),
            _ => false,
        };
        if !supported || value.is_empty() || values.insert(key, (value, line)).is_some() {
            return Err(ImportError::new(
                line,
                "field",
                "unsupported_or_duplicate_field",
            ));
        }
    }
    let get = |key: &'static str| -> Result<(&str, usize), ImportError> {
        values
            .get(key)
            .copied()
            .ok_or_else(|| ImportError::new(0, key, "missing_field"))
    };
    let key = |field: &'static str| -> Result<Zeroizing<[u8; 32]>, ImportError> {
        let (value, line) = get(field)?;
        let data = Zeroizing::new(
            base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|_| ImportError::new(line, field, "invalid_key"))?,
        );
        let bytes: [u8; 32] = data
            .as_slice()
            .try_into()
            .map_err(|_| ImportError::new(line, field, "invalid_key"))?;
        if bytes == [0; 32] {
            return Err(ImportError::new(line, field, "invalid_key"));
        }
        Ok(Zeroizing::new(bytes))
    };
    let nets = |field: &'static str| -> Result<Vec<IpNet>, ImportError> {
        let (value, line) = get(field)?;
        let nets: Vec<_> = value
            .split(',')
            .map(|part| {
                part.trim()
                    .parse::<IpNet>()
                    .map_err(|_| ImportError::new(line, field, "invalid_network"))
            })
            .collect::<Result<_, _>>()?;
        if nets.is_empty() || nets.len() > 256 {
            return Err(ImportError::new(line, field, "size_limit"));
        }
        Ok(nets)
    };
    let addresses = nets("Address")?;
    if addresses
        .iter()
        .any(|n| n.addr().is_unspecified() || n.addr().is_multicast() || n.addr().is_loopback())
        || addresses.iter().filter(|n| n.addr().is_ipv4()).count() > 1
        || addresses.iter().filter(|n| n.addr().is_ipv6()).count() > 1
    {
        return Err(ImportError::new(0, "Address", "unsupported_addresses"));
    }
    let allowed_ips = nets("AllowedIPs")?;
    let dns_servers = if let Some((value, line)) = values.get("DNS") {
        let ips: Vec<IpAddr> = value
            .split(',')
            .map(|s| {
                s.trim()
                    .parse()
                    .map_err(|_| ImportError::new(*line, "DNS", "invalid_address"))
            })
            .collect::<Result<_, _>>()?;
        if ips.len() > 8
            || ips
                .iter()
                .any(|ip| ip.is_unspecified() || ip.is_multicast() || ip.is_loopback())
        {
            return Err(ImportError::new(*line, "DNS", "invalid_address"));
        }
        ips
    } else {
        vec![]
    };
    let (endpoint, line) = get("Endpoint")?;
    let endpoint = if let Ok(address) = endpoint.parse::<SocketAddr>() {
        Endpoint::parse(&address.ip().to_string(), &address.port().to_string(), line)?
    } else {
        let (host, port) = endpoint
            .rsplit_once(':')
            .ok_or_else(|| ImportError::new(line, "Endpoint", "invalid_endpoint"))?;
        if host.contains(':') {
            return Err(ImportError::new(line, "Endpoint", "invalid_endpoint"));
        }
        Endpoint::parse(host, port, line)?
    };
    let mtu = if let Some((value, line)) = values.get("MTU") {
        value
            .parse::<u16>()
            .ok()
            .filter(|v| (1280..=9000).contains(v))
            .ok_or_else(|| ImportError::new(*line, "MTU", "invalid_mtu"))?
    } else {
        1280
    };
    let keepalive = values
        .get("PersistentKeepalive")
        .map(|(value, line)| {
            value
                .parse::<u16>()
                .map_err(|_| ImportError::new(*line, "PersistentKeepalive", "invalid_interval"))
        })
        .transpose()?
        .filter(|v| *v != 0);
    Ok(WireGuardProfile {
        private_key: key("PrivateKey")?,
        public_key: *key("PublicKey")?,
        preshared_key: values
            .contains_key("PresharedKey")
            .then(|| key("PresharedKey"))
            .transpose()?,
        endpoint,
        addresses,
        dns_servers,
        allowed_ips,
        mtu,
        keepalive,
    })
}
