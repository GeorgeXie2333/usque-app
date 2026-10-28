//! One rule implementation shared by all native providers and the data plane.
use crate::protocol::SessionConfiguration;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read, path::PathBuf};
use std::{net::IpAddr, sync::Arc};
use usque_geo::CountryCode;
use usque_transport::{GeoDirectPolicy, GeoRoute};

pub struct PolicyEvaluator {
    policy: Arc<GeoDirectPolicy>,
}
impl SessionConfiguration {
    pub fn snapshot(profile: usque_core::Profile, geo_cache_dir: PathBuf) -> Result<Self, String> {
        let mut value = Self {
            profile,
            geo_cache_dir,
            policy_digest: None,
        };
        value.policy_digest = Some(catalog_digest(&value)?);
        Ok(value)
    }
}
fn catalog_digest(configuration: &SessionConfiguration) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&configuration.profile).map_err(|_| "invalid_routing_profile")?);
    let mut paths = BTreeSet::new();
    if !configuration.profile.geo_direct_countries.is_empty()
        || configuration.profile.routing.ads_enabled
    {
        paths.insert(PathBuf::from("geo/geosite/dlc.dat"));
    }
    if configuration.profile.routing.ads_enabled {
        paths.insert(PathBuf::from("geo/geosite/ads-last-good.dat"));
    }
    for code in &configuration.profile.geo_direct_countries {
        let country = CountryCode::parse(code).map_err(|_| "invalid_country")?;
        for (directory, extension) in [("geoip", "dat"), ("geosite", "txt"), ("geosite", "bin")] {
            paths.insert(PathBuf::from(format!(
                "geo/{directory}/{country}.{extension}"
            )));
        }
    }
    for relative in paths {
        let name = relative.to_string_lossy();
        hash.update((name.len() as u64).to_be_bytes());
        hash.update(name.as_bytes());
        let path = configuration.geo_cache_dir.join(relative);
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hash.update([0]);
                continue;
            }
            Err(_) => return Err("policy_artifact_unavailable".into()),
        };
        let size = file
            .metadata()
            .map_err(|_| "policy_artifact_unavailable")?
            .len();
        if size > 128 * 1024 * 1024 {
            return Err("policy_artifact_too_large".into());
        }
        hash.update([1]);
        hash.update(size.to_be_bytes());
        let mut buffer = [0; 65536];
        let mut read = 0u64;
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|_| "policy_artifact_unavailable")?;
            if n == 0 {
                break;
            }
            read += n as u64;
            if read > size {
                return Err("policy_artifact_changed".into());
            }
            hash.update(&buffer[..n]);
        }
        if read != size {
            return Err("policy_artifact_changed".into());
        }
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
impl PolicyEvaluator {
    pub fn load(configuration: &SessionConfiguration) -> Result<Self, String> {
        let digest = catalog_digest(configuration)?;
        if configuration.policy_digest.as_deref() != Some(digest.as_str()) {
            return Err("policy_snapshot_mismatch".into());
        }
        let profile = &configuration.profile;
        let countries = profile
            .geo_direct_countries
            .iter()
            .map(|country| CountryCode::parse(country))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "invalid_country")?;
        let policy = GeoDirectPolicy::load(&configuration.geo_cache_dir, countries)
            .map_err(|_| "geo_catalog_unavailable")?
            .with_custom_rules(profile)
            .map_err(|_| "invalid_routing_rules")?
            .with_frozen_ads(&configuration.geo_cache_dir);
        if catalog_digest(configuration)? != digest {
            return Err("policy_artifact_changed".into());
        }
        Ok(Self::new(Arc::new(policy)))
    }
    pub fn new(policy: Arc<GeoDirectPolicy>) -> Self {
        Self { policy }
    }
    pub fn shared_policy(&self) -> Arc<GeoDirectPolicy> {
        self.policy.clone()
    }
    pub fn revision(&self) -> u64 {
        self.policy.routing_revision()
    }
    pub fn route_host(&self, host: &str) -> GeoRoute {
        self.policy.freeze_routing().route_host(host)
    }
    pub fn route_ip(&self, ip: IpAddr) -> GeoRoute {
        self.policy.freeze_routing().route_ip(ip)
    }
    pub fn route_flow(&self, host: Option<&str>, ip: Option<IpAddr>) -> GeoRoute {
        let policy = self.policy.freeze_routing();
        match (host, ip) {
            (Some(host), Some(ip)) if host.parse::<IpAddr>().is_err() => {
                policy.resolved_route(host, ip)
            }
            (_, Some(ip)) => policy.route_ip(ip),
            (Some(host), None) => policy.route_host(host),
            (None, None) => GeoRoute::Reject,
        }
    }
}
