//! WARP endpoint discovery metadata and encrypted, resumable scan storage.
//! Pool/port data: vernette/warpscout b49ef5e8a466164a64d669951521b58944346bc8
//! (MIT; see docs/WARP_WIREGUARD_UPSTREAM.md). No upstream runtime is linked.
use crate::chain_exit::{Endpoint, ImportError, ImportSecrets, store::ProfileCipher};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::Zeroizing;

pub const PRIMARY_PORTS: [u16; 4] = [2408, 500, 1701, 4500];
pub const PORTS: [u16; 54] = [
    2408, 500, 1701, 4500, 854, 859, 864, 878, 880, 890, 891, 894, 903, 908, 928, 934, 939, 942,
    943, 945, 946, 955, 968, 987, 988, 1002, 1010, 1014, 1018, 1070, 1074, 1180, 1387, 1843, 2371,
    2506, 3138, 3476, 3581, 3854, 4177, 4198, 4233, 5279, 5956, 7103, 7152, 7156, 7281, 7559, 8319,
    8742, 8854, 8886,
];
const POOLS: [[u8; 3]; 14] = [
    [8, 6, 112],
    [8, 34, 70],
    [8, 34, 146],
    [8, 35, 211],
    [8, 39, 125],
    [8, 39, 204],
    [8, 39, 214],
    [8, 47, 69],
    [162, 159, 192],
    [162, 159, 195],
    [188, 114, 96],
    [188, 114, 97],
    [188, 114, 98],
    [188, 114, 99],
];
const MAX_OBJECT: usize = 256 * 1024;
pub const PAGE_SIZE: usize = 100;
pub const CHUNK_SIZE: usize = 64;
const IDENTITY_ID: Uuid = Uuid::from_u128(0xca6c4231_a777_4f4a_9cc1_e71dfb132b43);
const INDEX_ID: Uuid = Uuid::from_u128(0xa0c173a2_1223_4f12_bfa2_9bad9b041ee4);

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    #[default]
    Quick,
    Target,
    Full,
}

/// Persist the enumeration rule so a saved cursor never changes meaning after
/// an upgrade. Legacy jobs remain readable but cannot resume with the new rule.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanPlan {
    #[default]
    LegacyPorts,
    SinglePortV1,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub mode: ScanMode,
    #[serde(default)]
    pub ipv6: bool,
    #[serde(default)]
    pub target: Option<IpAddr>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cursor: usize,
    #[serde(default)]
    pub country: Option<String>,
}
impl Request {
    pub fn parse(text: &str) -> Result<Self, ImportError> {
        if text.len() > 4096 {
            return Err(error("invalid_request"));
        }
        let value: Self = serde_json::from_str(text).map_err(|_| error("invalid_request"))?;
        if !matches!(
            value.action.as_str(),
            "generate" | "start" | "pause" | "resume" | "cancel" | "get"
        ) || value.name.chars().count() > 64
            || value.name.chars().any(char::is_control)
            || value.cursor > 200_000
            || value
                .country
                .as_ref()
                .is_some_and(|c| c.len() != 2 || !c.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(error("invalid_request"));
        }
        Ok(value)
    }
    pub fn needs_network(&self) -> bool {
        matches!(self.action.as_str(), "generate" | "start" | "resume")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Observation {
    pub exit_ip: Option<IpAddr>,
    pub country: Option<String>,
    pub colo: Option<String>,
    pub response_ms: Option<u64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExitObservation {
    pub checked_at: String,
    pub ipv4: Option<Observation>,
    pub ipv6: Option<Observation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub index: usize,
    pub endpoint: Endpoint,
    pub checked_at: String,
    pub ipv4: Option<Observation>,
    pub ipv6: Option<Observation>,
    pub failure: Option<String>,
}
impl ProbeResult {
    pub fn matches_country(&self, country: Option<&str>) -> bool {
        country.is_none_or(|c| {
            [&self.ipv4, &self.ipv6]
                .into_iter()
                .flatten()
                .any(|o| o.country.as_deref() == Some(c))
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub kind: String,
    pub mode: ScanMode,
    #[serde(default)]
    pub plan: ScanPlan,
    pub ipv6: bool,
    pub state: String,
    pub context: String,
    pub outer: Option<String>,
    pub created_at: String,
    pub addresses: Vec<IpAddr>,
    pub next: usize,
    pub chunks: usize,
    pub working: usize,
    pub countries: BTreeSet<String>,
    pub failure: Option<String>,
    pub profile_id: Option<Uuid>,
}
impl Job {
    pub fn new(request: &Request, context: String) -> Result<Self, ImportError> {
        let addresses = if request.action == "generate" {
            vec![]
        } else {
            targets(request)?
        };
        Ok(Self {
            id: Uuid::new_v4(),
            kind: request.action.clone(),
            mode: request.mode,
            plan: ScanPlan::SinglePortV1,
            ipv6: request.ipv6,
            state: "running".into(),
            context,
            outer: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            addresses,
            next: 0,
            chunks: 0,
            working: 0,
            countries: BTreeSet::new(),
            failure: None,
            profile_id: None,
        })
    }
    fn legacy_ports(&self) -> &[u16] {
        if self.mode == ScanMode::Quick {
            &PRIMARY_PORTS
        } else {
            &PORTS
        }
    }
    pub fn total(&self) -> usize {
        match self.plan {
            ScanPlan::SinglePortV1 => self.addresses.len(),
            ScanPlan::LegacyPorts => self.addresses.len() * self.legacy_ports().len(),
        }
    }
    pub fn endpoint(&self, index: usize) -> Option<Endpoint> {
        let (address, port) = match self.plan {
            // V1's order is part of the saved plan. Do not change it when
            // updating pool data: a resumed job must use the same IP/port.
            ScanPlan::SinglePortV1 => (index, [2408, 500, 1701, 4500][index % 4]),
            ScanPlan::LegacyPorts => {
                let ports = self.legacy_ports();
                (index / ports.len(), ports[index % ports.len()])
            }
        };
        self.addresses.get(address).map(|ip| Endpoint {
            host: ip.to_string(),
            port,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            id: self.id,
            kind: self.kind.clone(),
            state: self.state.clone(),
            mode: self.mode,
            ipv6: self.ipv6,
            completed: self.next,
            total: self.total(),
            working: self.working,
            countries: self.countries.iter().cloned().collect(),
            created_at: self.created_at.clone(),
            failure: self.failure.clone(),
            profile_id: self.profile_id,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub id: Uuid,
    pub kind: String,
    pub state: String,
    pub mode: ScanMode,
    pub ipv6: bool,
    pub completed: usize,
    pub total: usize,
    pub working: usize,
    pub countries: Vec<String>,
    pub created_at: String,
    pub failure: Option<String>,
    pub profile_id: Option<Uuid>,
}
#[derive(Default, Serialize)]
pub struct Response {
    pub job: Option<Snapshot>,
    pub history: Vec<Snapshot>,
    pub results: Vec<ProbeResult>,
    pub next_cursor: Option<usize>,
    pub error: Option<String>,
}

fn targets(request: &Request) -> Result<Vec<IpAddr>, ImportError> {
    if request.mode == ScanMode::Target {
        let ip = request.target.ok_or_else(|| error("invalid_endpoint"))?;
        Endpoint::parse(&ip.to_string(), "2408", 0)?;
        return Ok(vec![ip]);
    }
    if request.ipv6 && request.mode == ScanMode::Full {
        return Err(error("ipv6_full_scan_unavailable"));
    }
    if request.ipv6 {
        return Ok([0xd0u16, 0xd1]
            .into_iter()
            .flat_map(|pool| {
                (0..5).map(move |_| {
                    let mut bytes = *Uuid::new_v4().as_bytes();
                    bytes[..8].copy_from_slice(&[
                        0x26,
                        0x06,
                        0x47,
                        0,
                        (pool >> 8) as u8,
                        pool as u8,
                        0,
                        0,
                    ]);
                    IpAddr::V6(bytes.into())
                })
            })
            .collect());
    }
    let mut result = Vec::new();
    for pool in POOLS {
        let octets: Vec<u8> = if request.mode == ScanMode::Full {
            (0..=255).collect()
        } else {
            let mut chosen = BTreeSet::new();
            while chosen.len() < 5 {
                chosen.insert(Uuid::new_v4().as_bytes()[0]);
            }
            chosen.into_iter().collect()
        };
        result.extend(
            octets
                .into_iter()
                .map(|last| IpAddr::V4([pool[0], pool[1], pool[2], last].into())),
        );
    }
    Ok(result)
}

pub struct Store<'a> {
    directory: PathBuf,
    cipher: &'a dyn ProfileCipher,
}
impl<'a> Store<'a> {
    pub fn new(parent: &Path, cipher: &'a dyn ProfileCipher) -> Self {
        Self {
            directory: parent.join("warp-wireguard"),
            cipher,
        }
    }
    fn directory(&self) -> Result<(), ImportError> {
        if self.directory.exists() {
            let meta = fs::symlink_metadata(&self.directory)
                .map_err(|_| error("secure_storage_failed"))?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(error("secure_storage_failed"));
            }
        } else {
            fs::create_dir_all(&self.directory).map_err(|_| error("secure_storage_failed"))?;
        }
        Ok(())
    }
    fn write<T: Serialize>(&self, id: Uuid, value: &T) -> Result<(), ImportError> {
        self.directory()?;
        let plain =
            Zeroizing::new(serde_json::to_vec(value).map_err(|_| error("secure_storage_failed"))?);
        if plain.len() > MAX_OBJECT - 4096 {
            return Err(error("size_limit"));
        }
        let encrypted = self.cipher.seal(id, &plain)?;
        if encrypted.len() > MAX_OBJECT {
            return Err(error("size_limit"));
        }
        let mut temp = tempfile::Builder::new()
            .prefix(".warp-")
            .tempfile_in(&self.directory)
            .map_err(|_| error("secure_storage_failed"))?;
        temp.write_all(&encrypted)
            .map_err(|_| error("secure_storage_failed"))?;
        temp.as_file()
            .sync_all()
            .map_err(|_| error("secure_storage_failed"))?;
        temp.persist(self.directory.join(format!("{id}.sealed")))
            .map_err(|_| error("secure_storage_failed"))?;
        Ok(())
    }
    fn read<T: DeserializeOwned>(&self, id: Uuid) -> Result<Option<T>, ImportError> {
        self.directory()?;
        let path = self.directory.join(format!("{id}.sealed"));
        if !path.exists() {
            return Ok(None);
        }
        let meta = fs::symlink_metadata(&path).map_err(|_| error("secure_storage_failed"))?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_OBJECT as u64 {
            return Err(error("secure_storage_failed"));
        }
        let mut data = Vec::new();
        fs::File::open(path)
            .map_err(|_| error("secure_storage_failed"))?
            .take(MAX_OBJECT as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|_| error("secure_storage_failed"))?;
        if data.len() > MAX_OBJECT {
            return Err(error("size_limit"));
        }
        let plain = self.cipher.open(id, &data)?;
        if plain.len() > MAX_OBJECT {
            return Err(error("size_limit"));
        }
        serde_json::from_slice(&plain)
            .map(Some)
            .map_err(|_| error("secure_storage_failed"))
    }
    pub fn identity(&self) -> Result<Option<ImportSecrets>, ImportError> {
        self.read(IDENTITY_ID)
    }
    pub fn save_identity(&self, secrets: &ImportSecrets) -> Result<(), ImportError> {
        self.write(IDENTITY_ID, secrets)
    }
    pub fn jobs(&self) -> Result<Vec<Uuid>, ImportError> {
        Ok(self.read(INDEX_ID)?.unwrap_or_default())
    }
    pub fn save(&self, job: &Job) -> Result<(), ImportError> {
        self.write(job.id, job)
    }
    pub fn insert(&self, job: &Job) -> Result<(), ImportError> {
        let mut jobs = self.jobs()?;
        // Keep metadata bounded; older encrypted results remain until clear-all.
        jobs.insert(0, job.id);
        jobs.truncate(16);
        self.save(job)?;
        self.write(INDEX_ID, &jobs)
    }
    pub fn job(&self, id: Uuid) -> Result<Job, ImportError> {
        let job: Job = self.read(id)?.ok_or_else(|| error("job_not_found"))?;
        if job.id != id
            || job.addresses.len() > 3584
            || job.next > job.total()
            || job.chunks > 200_000
        {
            return Err(error("secure_storage_failed"));
        }
        Ok(job)
    }
    pub fn checkpoint(
        &self,
        job: &mut Job,
        rows: &mut Vec<ProbeResult>,
    ) -> Result<(), ImportError> {
        if !rows.is_empty() {
            let mut committed = job.clone();
            let mut offset = 0;
            while offset < rows.len() {
                let first = rows[offset].index;
                let chunk = first / CHUNK_SIZE;
                let mut records: Vec<ProbeResult> =
                    self.read(chunk_id(job.id, chunk))?.unwrap_or_default();
                // A prior interrupted write may contain uncommitted rows. Keep
                // only earlier attempts; retrying the cursor replaces the tail.
                records.retain(|row| row.index < first);
                while offset < rows.len() && rows[offset].index / CHUNK_SIZE == chunk {
                    records.push(rows[offset].clone());
                    offset += 1;
                }
                if records.len() > CHUNK_SIZE {
                    return Err(error("secure_storage_failed"));
                }
                self.write_chunk(job.id, chunk, &records)?;
                committed.chunks = committed.chunks.max(chunk + 1);
            }
            self.save(&committed)?;
            *job = committed;
            rows.clear();
            return Ok(());
        }
        self.save(job)
    }
    fn write_chunk(
        &self,
        job: Uuid,
        chunk: usize,
        rows: &[ProbeResult],
    ) -> Result<(), ImportError> {
        self.write(chunk_id(job, chunk), &rows)?;
        let countries: BTreeSet<_> = rows
            .iter()
            .flat_map(|row| [&row.ipv4, &row.ipv6].into_iter().flatten())
            .filter_map(|observation| observation.country.as_deref())
            .collect();
        for country in countries {
            let id = country_index_id(job, country);
            let mut bits = self.country_index(id)?.unwrap_or_default();
            bits.resize(bits.len().max(chunk / 8 + 1), 0);
            bits[chunk / 8] |= 1 << (chunk % 8);
            self.write(id, &STANDARD.encode(bits))?;
        }
        Ok(())
    }
    pub fn page(
        &self,
        job: &Job,
        cursor: usize,
        country: Option<&str>,
    ) -> Result<(Vec<ProbeResult>, Option<usize>), ImportError> {
        let mut rows = Vec::new();
        let index = country
            .map(|country| self.country_index(country_index_id(job.id, country)))
            .transpose()?
            .flatten();
        for chunk in (cursor / CHUNK_SIZE)..job.chunks {
            if index.as_ref().is_some_and(|bits| {
                bits.get(chunk / 8)
                    .is_none_or(|byte| byte & (1 << (chunk % 8)) == 0)
            }) {
                continue;
            }
            let records: Vec<ProbeResult> = self
                .read(chunk_id(job.id, chunk))?
                .ok_or_else(|| error("secure_storage_failed"))?;
            if records.len() > CHUNK_SIZE {
                return Err(error("secure_storage_failed"));
            }
            for row in records {
                if row.index >= cursor
                    && row.index < job.next
                    && row.failure.is_none()
                    && row.matches_country(country)
                {
                    let next = row.index + 1;
                    rows.push(row);
                    if rows.len() == PAGE_SIZE {
                        return Ok((rows, Some(next)));
                    }
                }
            }
        }
        Ok((rows, None))
    }
    fn country_index(&self, id: Uuid) -> Result<Option<Vec<u8>>, ImportError> {
        self.read::<String>(id)?
            .map(|text| {
                let bytes = STANDARD
                    .decode(text)
                    .map_err(|_| error("secure_storage_failed"))?;
                if bytes.len() > 25_000 {
                    return Err(error("size_limit"));
                }
                Ok(bytes)
            })
            .transpose()
    }
}
fn country_index_id(id: Uuid, country: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"Usque/warp-scan/country/v1");
    hash.update(id.as_bytes());
    hash.update(country);
    Uuid::from_bytes(hash.finalize()[..16].try_into().expect("SHA256 length"))
}
fn chunk_id(id: Uuid, chunk: usize) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"Usque/warp-scan/chunk/v1");
    hash.update(id.as_bytes());
    hash.update((chunk as u64).to_be_bytes());
    Uuid::from_bytes(hash.finalize()[..16].try_into().expect("SHA256 length"))
}
pub fn clear(parent: &Path) -> Result<(), ImportError> {
    let directory = parent.join("warp-wireguard");
    if !directory.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(&directory).map_err(|_| error("secure_storage_failed"))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(error("secure_storage_failed"));
    }
    for entry in fs::read_dir(directory).map_err(|_| error("secure_storage_failed"))? {
        let entry = entry.map_err(|_| error("secure_storage_failed"))?;
        if entry
            .file_type()
            .map_err(|_| error("secure_storage_failed"))?
            .is_file()
            && entry.file_name().to_str().is_some_and(|s| {
                s.starts_with(".warp-")
                    || s.strip_suffix(".sealed")
                        .is_some_and(|s| Uuid::parse_str(s).is_ok())
            })
        {
            fs::remove_file(entry.path()).map_err(|_| error("secure_storage_failed"))?;
        }
    }
    Ok(())
}
pub fn error(reason: &str) -> ImportError {
    ImportError {
        line: 0,
        field: "warp_wireguard".into(),
        reason: reason.into(),
    }
}
pub fn context(profile: &crate::Profile) -> Result<String, ImportError> {
    use sha2::{Digest, Sha256};
    // Local listeners, inner exits and display names do not change the scan's
    // outer network. They may pause a job, but must not invalidate its cursor.
    let outer = serde_json::json!({
        "account": profile.id,
        "endpoint": profile.endpoint,
        "transport": profile.transport,
        "ip_policy": profile.ip_policy,
        "data_plane": profile.data_plane,
        "congestion_control": profile.congestion_control,
        "mtu": profile.mtu,
        "dns_servers": profile.dns_servers,
    });
    let bytes = Zeroizing::new(serde_json::to_vec(&outer).map_err(|_| error("invalid_request"))?);
    Ok(Sha256::digest(&*bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_scan_visits_each_ip_once_on_one_common_port() {
        let req = Request::parse(r#"{"action":"start","mode":"full"}"#).unwrap();
        let job = Job::new(&req, String::new()).unwrap();
        assert_eq!(job.total(), 3584);
        assert_eq!(PORTS.into_iter().collect::<BTreeSet<_>>().len(), 54);
        let mut addresses = BTreeSet::new();
        for index in 0..job.total() {
            let endpoint = job.endpoint(index).unwrap();
            assert!(addresses.insert(endpoint.host));
            assert_eq!(endpoint.port, PRIMARY_PORTS[index % PRIMARY_PORTS.len()]);
        }
        assert_eq!(job.endpoint(job.total()), None);
        let req = Request::parse(r#"{"action":"start","mode":"full","ipv6":true}"#).unwrap();
        assert!(Job::new(&req, String::new()).is_err());
    }
    #[test]
    fn quick_targets_and_invalid_input() {
        for (ipv6, total) in [(false, 70), (true, 10)] {
            let req = Request::parse(&format!(r#"{{"action":"start","ipv6":{ipv6}}}"#)).unwrap();
            let job = Job::new(&req, String::new()).unwrap();
            assert_eq!(job.total(), total);
            let addresses: BTreeSet<_> = (0..job.total())
                .map(|index| job.endpoint(index).unwrap().host)
                .collect();
            assert_eq!(addresses.len(), total);
        }
        assert!(Request::parse(r#"{"action":"start","country":"anything"}"#).is_err());
        let req =
            Request::parse(r#"{"action":"start","mode":"target","target":"127.0.0.1"}"#).unwrap();
        assert!(Job::new(&req, String::new()).is_err());
    }

    #[test]
    fn single_address_scan_uses_only_the_primary_port() {
        for ip in ["162.159.192.1", "2606:4700:d0::1"] {
            let req = Request::parse(&format!(
                r#"{{"action":"start","mode":"target","target":"{ip}"}}"#
            ))
            .unwrap();
            let job = Job::new(&req, String::new()).unwrap();
            assert_eq!(job.total(), 1);
            assert_eq!(
                job.endpoint(0),
                Some(Endpoint::parse(ip, "2408", 0).unwrap())
            );
            assert_eq!(job.endpoint(1), None);
        }
    }

    #[test]
    fn legacy_job_cursors_keep_their_original_port_mapping() {
        let req = Request::parse(r#"{"action":"start","mode":"full"}"#).unwrap();
        let mut value = serde_json::to_value(Job::new(&req, String::new()).unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("plan");
        let job: Job = serde_json::from_value(value).unwrap();
        assert_eq!(job.plan, ScanPlan::LegacyPorts);
        assert_eq!(job.total(), 193536);
        assert_eq!(
            job.endpoint(0).unwrap().host,
            job.endpoint(53).unwrap().host
        );
        assert_ne!(
            job.endpoint(53).unwrap().host,
            job.endpoint(54).unwrap().host
        );
        assert_eq!(job.endpoint(53).unwrap().port, PORTS[53]);
    }
}
