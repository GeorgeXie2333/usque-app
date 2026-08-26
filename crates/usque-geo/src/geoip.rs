use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::IpNet;
use prost::Message;

use crate::country::CountryCode;
use crate::error::GeoError;
use crate::proto::GeoIpList;

#[derive(Clone, Debug, Default)]
struct BitTrie {
    nodes: Vec<TrieNode>,
}

#[derive(Clone, Debug, Default)]
struct TrieNode {
    child: [u32; 2],
    terminal: bool,
}

impl BitTrie {
    fn new() -> Self {
        Self {
            nodes: vec![TrieNode::default()],
        }
    }

    fn insert(&mut self, bytes: &[u8], prefix_len: u8) -> Result<(), GeoError> {
        let total_bits = bytes.len().saturating_mul(8);
        if usize::from(prefix_len) > total_bits {
            return Err(GeoError::InvalidGeoIp);
        }
        let mut idx = 0usize;
        for bit_i in 0..usize::from(prefix_len) {
            let byte = bytes[bit_i / 8];
            let bit = usize::from((byte >> (7 - (bit_i % 8))) & 1);
            let child = self.nodes[idx].child[bit];
            if child == 0 {
                let new_idx = self.nodes.len();
                self.nodes.push(TrieNode::default());
                self.nodes[idx].child[bit] =
                    u32::try_from(new_idx).map_err(|_| GeoError::InvalidGeoIp)?;
                idx = new_idx;
            } else {
                idx = child as usize;
            }
        }
        self.nodes[idx].terminal = true;
        Ok(())
    }

    fn contains(&self, bytes: &[u8]) -> bool {
        let mut idx = 0usize;
        if self.nodes[idx].terminal {
            return true;
        }
        for bit_i in 0..bytes.len().saturating_mul(8) {
            let byte = bytes[bit_i / 8];
            let bit = usize::from((byte >> (7 - (bit_i % 8))) & 1);
            let child = self.nodes[idx].child[bit];
            if child == 0 {
                return false;
            }
            idx = child as usize;
            if self.nodes[idx].terminal {
                return true;
            }
        }
        false
    }
}

/// Prefix tree of CIDRs for one country extracted from a v2ray `GeoIPList`.
#[derive(Clone, Debug)]
pub struct GeoIpSet {
    country: CountryCode,
    v4: BitTrie,
    v6: BitTrie,
    inverse: bool,
}

impl GeoIpSet {
    pub fn from_v2ray_dat(bytes: &[u8], country: &CountryCode) -> Result<Self, GeoError> {
        let list = GeoIpList::decode(bytes).map_err(|_| GeoError::InvalidGeoIp)?;
        let mut set = Self {
            country: country.clone(),
            v4: BitTrie::new(),
            v6: BitTrie::new(),
            inverse: false,
        };
        let mut found = false;
        for entry in list.entry {
            let Ok(entry_country) = CountryCode::parse(&entry.country_code) else {
                continue;
            };
            if entry_country != *country {
                continue;
            }
            if found && set.inverse != entry.inverse_match {
                return Err(GeoError::InvalidGeoIp);
            }
            set.inverse = entry.inverse_match;
            found = true;
            for cidr in entry.cidr {
                set.insert_cidr(&cidr.ip, cidr.prefix)?;
            }
        }
        if !found {
            return Err(GeoError::InvalidGeoIp);
        }
        Ok(set)
    }

    pub fn country(&self) -> &CountryCode {
        &self.country
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let hit = match ip {
            IpAddr::V4(addr) => self.v4.contains(&addr.octets()),
            IpAddr::V6(addr) => self.v6.contains(&addr.octets()),
        };
        hit ^ self.inverse
    }

    fn insert_cidr(&mut self, ip: &[u8], prefix: u32) -> Result<(), GeoError> {
        let addr = match ip.len() {
            4 => IpAddr::V4(Ipv4Addr::from(
                <[u8; 4]>::try_from(ip).map_err(|_| GeoError::InvalidGeoIp)?,
            )),
            16 => IpAddr::V6(Ipv6Addr::from(
                <[u8; 16]>::try_from(ip).map_err(|_| GeoError::InvalidGeoIp)?,
            )),
            _ => return Err(GeoError::InvalidGeoIp),
        };
        let prefix = u8::try_from(prefix).map_err(|_| GeoError::InvalidGeoIp)?;
        // Reject prefixes that are invalid for the address family.
        IpNet::new(addr, prefix).map_err(|_| GeoError::InvalidGeoIp)?;
        match addr {
            IpAddr::V4(_) => self.v4.insert(ip, prefix),
            IpAddr::V6(_) => self.v6.insert(ip, prefix),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use prost::Message;

    use super::GeoIpSet;
    use crate::country::CountryCode;
    use crate::error::GeoError;
    use crate::proto::{Cidr, GeoIp, GeoIpList};

    fn encode_cn() -> Vec<u8> {
        GeoIpList {
            entry: vec![GeoIp {
                country_code: "CN".to_owned(),
                cidr: vec![
                    Cidr {
                        ip: vec![1, 2, 3, 0],
                        prefix: 24,
                    },
                    Cidr {
                        ip: documentation_v6_prefix(),
                        prefix: 32,
                    },
                ],
                inverse_match: false,
            }],
        }
        .encode_to_vec()
    }

    fn documentation_v6_prefix() -> Vec<u8> {
        let mut bytes = vec![0; 16];
        bytes[0] = 0x20;
        bytes[1] = 0x01;
        bytes[2] = 0x0d;
        bytes[3] = 0xb8;
        bytes
    }

    #[test]
    fn hits_and_misses_v4_and_v6() {
        let cn = CountryCode::parse("CN").unwrap();
        let set = GeoIpSet::from_v2ray_dat(&encode_cn(), &cn).unwrap();
        assert!(set.contains("1.2.3.1".parse::<IpAddr>().unwrap()));
        assert!(!set.contains("1.2.4.1".parse::<IpAddr>().unwrap()));
        assert!(set.contains("2001:db8::1".parse::<IpAddr>().unwrap()));
        assert!(!set.contains("2001:db9::1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn truncated_dat_is_not_a_hit() {
        let cn = CountryCode::parse("CN").unwrap();
        assert!(matches!(
            GeoIpSet::from_v2ray_dat(&[0x0a, 0x10], &cn),
            Err(GeoError::InvalidGeoIp)
        ));
    }

    #[test]
    fn missing_country_entry_fails_closed() {
        let us = CountryCode::parse("US").unwrap();
        assert!(matches!(
            GeoIpSet::from_v2ray_dat(&encode_cn(), &us),
            Err(GeoError::InvalidGeoIp)
        ));
    }

    #[test]
    fn inverse_match_inverts_membership() {
        let list = GeoIpList {
            entry: vec![GeoIp {
                country_code: "CN".to_owned(),
                cidr: vec![Cidr {
                    ip: vec![1, 2, 3, 0],
                    prefix: 24,
                }],
                inverse_match: true,
            }],
        }
        .encode_to_vec();
        let cn = CountryCode::parse("CN").unwrap();
        let set = GeoIpSet::from_v2ray_dat(&list, &cn).unwrap();
        assert!(!set.contains("1.2.3.1".parse::<IpAddr>().unwrap()));
        assert!(set.contains("8.8.8.8".parse::<IpAddr>().unwrap()));
    }
}
