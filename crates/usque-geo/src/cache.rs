use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::country::CountryCode;
use crate::error::{ArtifactKind, GeoError};
use crate::geoip::GeoIpSet;
use crate::geosite::GeoSiteSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedCountry {
    pub country: CountryCode,
    pub geoip: bool,
    pub geosite: bool,
}

#[derive(Debug, Clone)]
pub struct GeoClassifier {
    geoip: BTreeMap<CountryCode, GeoIpSet>,
    geosite: BTreeMap<CountryCode, GeoSiteSet>,
}

impl GeoClassifier {
    pub fn load(cache_dir: impl AsRef<Path>, countries: &[CountryCode]) -> Result<Self, GeoError> {
        ClassifierSources::read(cache_dir.as_ref(), countries)?.build()
    }

    /// Reuse one validated classifier when the selected artifact contents match.
    /// Files are still read and hashed on every call: timestamps and lengths
    /// cannot establish that cached rules remain valid after an in-place edit.
    /// Only successful parses are cached, and no configuration or DNS decision
    /// is cached. The single entry bounds retained memory across selections.
    pub fn load_cached(
        cache_dir: impl AsRef<Path>,
        countries: &[CountryCode],
    ) -> Result<Arc<Self>, GeoError> {
        static CACHE: Mutex<ClassifierCache> = Mutex::new(ClassifierCache { entry: None });
        let sources = ClassifierSources::read(cache_dir.as_ref(), countries)?;
        CACHE
            .lock()
            .map_err(|_| io::Error::other("GEO classifier cache is unavailable"))?
            .load(sources)
    }

    pub fn ip_matches(&self, ip: IpAddr, country: &CountryCode) -> bool {
        self.geoip.get(country).is_some_and(|set| set.contains(ip))
    }

    pub fn host_matches(&self, host: &str, country: &CountryCode) -> bool {
        self.geosite
            .get(country)
            .is_some_and(|set| set.contains(host))
    }

    pub fn lookup_ip(&self, ip: IpAddr) -> Option<&CountryCode> {
        self.geoip
            .iter()
            .find_map(|(country, set)| set.contains(ip).then_some(country))
    }

    pub fn has_geosite(&self, country: &CountryCode) -> bool {
        self.geosite.contains_key(country)
    }
}

struct CountrySources {
    country: CountryCode,
    geoip: Vec<u8>,
    geosite: Option<(Vec<u8>, PathBuf)>,
}

struct ClassifierSources {
    global_geosite: Option<Vec<u8>>,
    countries: Vec<CountrySources>,
}

impl ClassifierSources {
    fn read(cache_dir: &Path, countries: &[CountryCode]) -> Result<Self, GeoError> {
        let global_geosite = read_optional_global_geosite(cache_dir)?;
        let countries = countries
            .iter()
            .map(|country| {
                Ok(CountrySources {
                    country: country.clone(),
                    geoip: read_required(
                        &geoip_cache_path(cache_dir, country),
                        country,
                        ArtifactKind::GeoIp,
                    )?,
                    geosite: if global_geosite.is_none() {
                        read_optional_geosite(cache_dir, country)?
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<_, GeoError>>()?;
        Ok(Self {
            global_geosite,
            countries,
        })
    }

    fn fingerprint(&self) -> [u8; 32] {
        fn artifact(hash: &mut Sha256, kind: u8, bytes: &[u8]) {
            hash.update([kind]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        let mut hash = Sha256::new();
        if let Some(bytes) = &self.global_geosite {
            artifact(&mut hash, 1, bytes);
        }
        for source in &self.countries {
            artifact(&mut hash, 2, source.country.as_str().as_bytes());
            artifact(&mut hash, 3, &source.geoip);
            if let Some((bytes, path)) = &source.geosite {
                let kind = if path.extension().is_some_and(|ext| ext == "bin") {
                    4
                } else {
                    5
                };
                artifact(&mut hash, kind, bytes);
            }
        }
        hash.finalize().into()
    }

    fn build(self) -> Result<GeoClassifier, GeoError> {
        let mut geoip = BTreeMap::new();
        let mut geosite = BTreeMap::new();
        let countries = self
            .countries
            .iter()
            .map(|source| source.country.clone())
            .collect::<Vec<_>>();
        let mut global_sets = if let Some(bytes) = self.global_geosite.as_deref() {
            Some(GeoSiteSet::from_validated_v2ray_dat_many(
                bytes, &countries,
            )?)
        } else {
            None
        };
        for source in self.countries {
            let country = source.country;
            geoip.insert(
                country.clone(),
                GeoIpSet::from_v2ray_dat(&source.geoip, &country)?,
            );
            if let Some(sets) = global_sets.as_mut() {
                if let Some(set) = sets.remove(&country) {
                    geosite.insert(country, set);
                }
            } else if let Some((bytes, path)) = source.geosite {
                let set = load_geosite(&bytes, &path, &country)?;
                geosite.insert(country, set);
            }
        }
        Ok(GeoClassifier { geoip, geosite })
    }
}

struct CachedClassifier {
    fingerprint: [u8; 32],
    classifier: Arc<GeoClassifier>,
}

#[derive(Default)]
struct ClassifierCache {
    entry: Option<CachedClassifier>,
}

impl ClassifierCache {
    fn load(&mut self, sources: ClassifierSources) -> Result<Arc<GeoClassifier>, GeoError> {
        let fingerprint = sources.fingerprint();
        if let Some(entry) = &self.entry
            && entry.fingerprint == fingerprint
        {
            return Ok(Arc::clone(&entry.classifier));
        }
        let classifier = Arc::new(sources.build()?);
        self.entry = Some(CachedClassifier {
            fingerprint,
            classifier: Arc::clone(&classifier),
        });
        Ok(classifier)
    }
}

pub fn list_cached_countries(cache_dir: impl AsRef<Path>) -> Result<Vec<CachedCountry>, GeoError> {
    let cache_dir = cache_dir.as_ref();
    let mut by_country: BTreeMap<CountryCode, CachedCountry> = BTreeMap::new();
    for country in list_codes_with_extension(&geoip_dir(cache_dir), "dat")? {
        by_country
            .entry(country.clone())
            .or_insert_with(|| CachedCountry {
                country,
                geoip: false,
                geosite: false,
            })
            .geoip = true;
    }
    for country in list_geosite_codes(&geosite_dir(cache_dir))? {
        by_country
            .entry(country.clone())
            .or_insert_with(|| CachedCountry {
                country,
                geoip: false,
                geosite: false,
            })
            .geosite = true;
    }
    if let Some(bytes) = read_optional_global_geosite(cache_dir)? {
        GeoSiteSet::validate_v2ray_dat(&bytes)?;
        let countries = by_country
            .values()
            .map(|cached| cached.country.clone())
            .collect::<Vec<_>>();
        let sets = GeoSiteSet::from_v2ray_dat_many(&bytes, &countries)?;
        for cached in by_country.values_mut() {
            cached.geosite = sets.contains_key(&cached.country);
        }
    }
    Ok(by_country.into_values().collect())
}

pub fn geoip_cache_path(cache_dir: impl AsRef<Path>, country: &CountryCode) -> PathBuf {
    geoip_dir(cache_dir.as_ref()).join(format!("{country}.dat"))
}

pub(crate) fn global_geoip_cache_path(cache_dir: &Path) -> PathBuf {
    geoip_dir(cache_dir).join("geoip.dat")
}

pub fn geosite_cache_path(cache_dir: impl AsRef<Path>, country: &CountryCode) -> PathBuf {
    geosite_dir(cache_dir.as_ref()).join(format!("{country}.txt"))
}

/// Global v2fly domain-list-community protobuf cache.
pub fn global_geosite_cache_path(cache_dir: impl AsRef<Path>) -> PathBuf {
    geosite_dir(cache_dir.as_ref()).join("dlc.dat")
}

pub fn has_global_geosite(cache_dir: impl AsRef<Path>) -> bool {
    global_geosite_cache_path(cache_dir).is_file()
}

pub(crate) fn geosite_bin_path(cache_dir: &Path, country: &CountryCode) -> PathBuf {
    geosite_dir(cache_dir).join(format!("{country}.bin"))
}

pub(crate) fn geoip_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("geo").join("geoip")
}

pub(crate) fn geosite_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("geo").join("geosite")
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), GeoError> {
    let parent = path
        .parent()
        .ok_or_else(|| GeoError::MissingParent(path.to_path_buf()))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    replace_file(temporary.path(), path)?;
    let _ = temporary.keep();
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: source and destination are null-terminated wide paths that outlive
    // the synchronous MoveFileExW call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn read_required(
    path: &Path,
    country: &CountryCode,
    kind: ArtifactKind,
) -> Result<Vec<u8>, GeoError> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(GeoError::MissingArtifact {
            country: country.clone(),
            kind,
        }),
        Err(error) => Err(error.into()),
    }
}

fn read_optional_geosite(
    cache_dir: &Path,
    country: &CountryCode,
) -> Result<Option<(Vec<u8>, PathBuf)>, GeoError> {
    let txt = geosite_cache_path(cache_dir, country);
    match fs::read(&txt) {
        Ok(bytes) => return Ok(Some((bytes, txt))),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
        Err(_) => {}
    }
    let bin = geosite_bin_path(cache_dir, country);
    match fs::read(&bin) {
        Ok(bytes) => Ok(Some((bytes, bin))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_optional_global_geosite(cache_dir: &Path) -> Result<Option<Vec<u8>>, GeoError> {
    match fs::read(global_geosite_cache_path(cache_dir)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn load_geosite(bytes: &[u8], path: &Path, country: &CountryCode) -> Result<GeoSiteSet, GeoError> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("bin") => GeoSiteSet::from_v2ray_dat(bytes, country),
        _ => GeoSiteSet::from_text_bytes(bytes, country),
    }
}

fn list_codes_with_extension(dir: &Path, extension: &str) -> Result<Vec<CountryCode>, GeoError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut countries = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case(extension) {
            continue;
        }
        if let Ok(country) = CountryCode::parse(stem) {
            countries.push(country);
        }
    }
    Ok(countries)
}

fn list_geosite_codes(dir: &Path) -> Result<Vec<CountryCode>, GeoError> {
    let mut countries = list_codes_with_extension(dir, "txt")?;
    countries.extend(list_codes_with_extension(dir, "bin")?);
    countries.sort();
    countries.dedup();
    Ok(countries)
}

pub(crate) fn cached_geoip_countries(cache_dir: &Path) -> Result<Vec<CountryCode>, GeoError> {
    let mut countries = list_codes_with_extension(&geoip_dir(cache_dir), "dat")?;
    countries.sort();
    countries.dedup();
    Ok(countries)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use prost::Message;

    use super::{
        ClassifierCache, ClassifierSources, GeoClassifier, atomic_write, geoip_cache_path,
        geosite_cache_path, global_geosite_cache_path, list_cached_countries,
    };
    use crate::country::CountryCode;
    use crate::error::GeoError;
    use crate::geoip::GeoIpSet;

    fn fixture_dat() -> &'static [u8] {
        include_bytes!("../tests/fixtures/geoip-cn.dat")
    }

    // Use a local cache so unrelated parallel tests cannot evict this entry.
    fn cached(
        cache: &mut ClassifierCache,
        directory: &std::path::Path,
        countries: &[CountryCode],
    ) -> Result<Arc<GeoClassifier>, GeoError> {
        cache.load(ClassifierSources::read(directory, countries)?)
    }

    #[test]
    fn cached_classifier_reuses_only_identical_contents_and_country_selection() {
        let directory = tempfile::tempdir().unwrap();
        let cn = CountryCode::parse("CN").unwrap();
        let us = CountryCode::parse("US").unwrap();
        let mut list = crate::proto::GeoIpList::decode(fixture_dat()).unwrap();
        let mut entry = list.entry[0].clone();
        entry.country_code = "US".into();
        entry.cidr[0].ip = vec![20, 0, 0, 0];
        entry.cidr[0].prefix = 8;
        list.entry.push(entry);
        let bytes = list.encode_to_vec();
        for country in [&cn, &us] {
            atomic_write(&geoip_cache_path(directory.path(), country), &bytes).unwrap();
        }
        let mut cache = ClassifierCache::default();
        let countries = std::slice::from_ref(&cn);
        let first = cached(&mut cache, directory.path(), countries).unwrap();
        let again = cached(&mut cache, directory.path(), countries).unwrap();
        assert!(Arc::ptr_eq(&first, &again));

        // A valid same-size edit, even with the old mtime, must replace the trie.
        let path = geoip_cache_path(directory.path(), &cn);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        list.entry[0].cidr[0].ip = vec![30, 0, 0, 0];
        list.entry[0].cidr[0].prefix = 8;
        let replacement = list.encode_to_vec();
        assert_eq!(replacement.len(), bytes.len());
        std::fs::write(&path, replacement).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let changed = cached(&mut cache, directory.path(), countries).unwrap();
        assert!(!Arc::ptr_eq(&first, &changed));
        assert!(changed.ip_matches("30.1.2.3".parse().unwrap(), &cn));
        assert!(!first.ip_matches("30.1.2.3".parse().unwrap(), &cn));

        let selected = cached(&mut cache, directory.path(), std::slice::from_ref(&us)).unwrap();
        assert!(!Arc::ptr_eq(&changed, &selected));
        assert!(selected.ip_matches("20.1.2.3".parse().unwrap(), &us));
        assert!(!selected.ip_matches("30.1.2.3".parse().unwrap(), &cn));

        std::fs::write(&path, vec![0; bytes.len()]).unwrap();
        assert!(cached(&mut cache, directory.path(), countries).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            cached(&mut cache, directory.path(), countries),
            Err(GeoError::MissingArtifact { .. })
        ));
    }

    #[test]
    fn cached_classifier_tracks_optional_rules_and_global_precedence() {
        let directory = tempfile::tempdir().unwrap();
        let cn = CountryCode::parse("CN").unwrap();
        let countries = std::slice::from_ref(&cn);
        atomic_write(&geoip_cache_path(directory.path(), &cn), fixture_dat()).unwrap();
        let mut cache = ClassifierCache::default();
        let ip_only = cached(&mut cache, directory.path(), countries).unwrap();
        assert!(!ip_only.has_geosite(&cn));
        let local = geosite_cache_path(directory.path(), &cn);
        atomic_write(&local, b"domain:before.cn\n").unwrap();
        let first = cached(&mut cache, directory.path(), countries).unwrap();
        assert!(first.host_matches("before.cn", &cn));
        atomic_write(&local, b"domain:afterx.cn\n").unwrap();
        let changed = cached(&mut cache, directory.path(), countries).unwrap();
        assert!(!changed.host_matches("before.cn", &cn));
        assert!(changed.host_matches("afterx.cn", &cn));

        let global = global_geosite_cache_path(directory.path());
        let list = crate::proto::GeoSiteList {
            entry: vec![crate::proto::GeoSite {
                country_code: "CN".into(),
                domain: vec![crate::proto::Domain {
                    r#type: crate::proto::DOMAIN_DOMAIN,
                    value: "global.cn".into(),
                }],
            }],
        };
        atomic_write(&global, &list.encode_to_vec()).unwrap();
        let with_global = cached(&mut cache, directory.path(), countries).unwrap();
        assert!(with_global.host_matches("global.cn", &cn));
        assert!(!with_global.host_matches("afterx.cn", &cn));
        atomic_write(&global, b"invalid").unwrap();
        assert!(cached(&mut cache, directory.path(), countries).is_err());
        atomic_write(
            &global,
            &crate::proto::GeoSiteList { entry: vec![] }.encode_to_vec(),
        )
        .unwrap();
        assert!(cached(&mut cache, directory.path(), countries).is_err());
        std::fs::remove_file(&global).unwrap();
        let fallback = cached(&mut cache, directory.path(), countries).unwrap();
        assert!(fallback.host_matches("afterx.cn", &cn));
        std::fs::remove_file(&local).unwrap();
        assert!(
            !cached(&mut cache, directory.path(), countries)
                .unwrap()
                .has_geosite(&cn)
        );
    }

    #[test]
    fn missing_geoip_file_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let cn = CountryCode::parse("CN").unwrap();
        assert!(matches!(
            GeoClassifier::load(directory.path(), std::slice::from_ref(&cn)),
            Err(GeoError::MissingArtifact { .. })
        ));
    }

    #[test]
    fn lists_countries_from_filenames() {
        let directory = tempfile::tempdir().unwrap();
        let cn = CountryCode::parse("CN").unwrap();
        atomic_write(&geoip_cache_path(directory.path(), &cn), fixture_dat()).unwrap();
        atomic_write(
            &super::geosite_cache_path(directory.path(), &cn),
            include_bytes!("../tests/fixtures/geosite-cn.txt"),
        )
        .unwrap();
        let listed = list_cached_countries(directory.path()).unwrap();
        assert_eq!(
            listed,
            vec![super::CachedCountry {
                country: cn.clone(),
                geoip: true,
                geosite: true,
            }]
        );
        let classifier = GeoClassifier::load(directory.path(), std::slice::from_ref(&cn)).unwrap();
        assert!(classifier.ip_matches("1.2.3.4".parse().unwrap(), &cn));
        assert!(classifier.host_matches("foo.example.cn", &cn));
        assert!(GeoIpSet::from_v2ray_dat(fixture_dat(), &cn).is_ok());
    }

    #[test]
    fn atomic_write_replaces_without_unlinking_first() {
        let directory = tempfile::tempdir().unwrap();
        let cn = CountryCode::parse("CN").unwrap();
        let path = geoip_cache_path(directory.path(), &cn);
        atomic_write(&path, b"stale").unwrap();
        atomic_write(&path, fixture_dat()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), fixture_dat());
    }
}
