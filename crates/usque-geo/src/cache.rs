use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

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
        let cache_dir = cache_dir.as_ref();
        let mut geoip = BTreeMap::new();
        let mut geosite = BTreeMap::new();
        for country in countries {
            let geoip_path = geoip_cache_path(cache_dir, country);
            let bytes = read_required(&geoip_path, country, ArtifactKind::GeoIp)?;
            geoip.insert(country.clone(), GeoIpSet::from_v2ray_dat(&bytes, country)?);
            if let Some((bytes, geosite_path)) = read_optional_geosite(cache_dir, country)? {
                let set = load_geosite(&bytes, &geosite_path, country)?;
                geosite.insert(country.clone(), set);
            }
        }
        Ok(Self { geoip, geosite })
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
    Ok(by_country.into_values().collect())
}

pub fn geoip_cache_path(cache_dir: impl AsRef<Path>, country: &CountryCode) -> PathBuf {
    geoip_dir(cache_dir.as_ref()).join(format!("{country}.dat"))
}

pub fn geosite_cache_path(cache_dir: impl AsRef<Path>, country: &CountryCode) -> PathBuf {
    geosite_dir(cache_dir.as_ref()).join(format!("{country}.txt"))
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

pub(crate) fn cached_geosite_countries(cache_dir: &Path) -> Result<Vec<CountryCode>, GeoError> {
    list_geosite_codes(&geosite_dir(cache_dir))
}

#[cfg(test)]
mod tests {
    use super::{GeoClassifier, atomic_write, geoip_cache_path, list_cached_countries};
    use crate::country::CountryCode;
    use crate::error::GeoError;
    use crate::geoip::GeoIpSet;

    fn fixture_dat() -> &'static [u8] {
        include_bytes!("../tests/fixtures/geoip-cn.dat")
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
