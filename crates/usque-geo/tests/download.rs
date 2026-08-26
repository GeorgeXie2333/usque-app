use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use usque_geo::{
    ALLOWED_HOSTS, ArtifactKind, CountryCode, FetchedBody, GeoDownloader, GeoError, HttpFetch,
    MAX_GEOIP_BYTES, UpdateStatus, geoip_cache_path, geosite_cache_path,
};

const GEOIP_CN: &[u8] = include_bytes!("fixtures/geoip-cn.dat");
const GEOIP_CN_SUM: &str = include_str!("fixtures/geoip-cn.dat.sha256sum");

fn cn() -> CountryCode {
    CountryCode::parse("CN").unwrap()
}

fn us() -> CountryCode {
    CountryCode::parse("US").unwrap()
}

fn geoip_object(country: &str) -> String {
    if country.eq_ignore_ascii_case("cn") {
        format!("{country}.dat")
    } else {
        format!("dat/{country}.dat")
    }
}

fn geoip_dat(host: &str, country: &str) -> String {
    format!(
        "https://{host}/gh/v2fly/geoip@release/{}",
        geoip_object(country)
    )
}

fn geoip_sum(host: &str, country: &str) -> String {
    format!(
        "https://{host}/gh/v2fly/geoip@release/{}.sha256sum",
        geoip_object(country)
    )
}

fn geosite_cn(host: &str) -> String {
    format!("https://{host}/gh/v2fly/domain-list-community@master/data/cn")
}

fn geosite_geo_cn(host: &str) -> String {
    format!("https://{host}/gh/v2fly/domain-list-community@master/data/geolocation-cn")
}

enum MockRoute {
    Ok(Vec<u8>),
    Status(u16),
    TooLarge,
}

struct MockFetch {
    routes: HashMap<String, MockRoute>,
    hits: Mutex<Vec<String>>,
}

impl MockFetch {
    fn new() -> Self {
        Self {
            routes: HashMap::new(),
            hits: Mutex::new(Vec::new()),
        }
    }

    fn with_cn_geoip(mut self) -> Self {
        for host in ALLOWED_HOSTS {
            self.routes
                .insert(geoip_dat(host, "cn"), MockRoute::Ok(GEOIP_CN.to_vec()));
            self.routes.insert(
                geoip_sum(host, "cn"),
                MockRoute::Ok(GEOIP_CN_SUM.as_bytes().to_vec()),
            );
        }
        self
    }

    fn with_cn_geosite(mut self) -> Self {
        for host in ALLOWED_HOSTS {
            self.routes.insert(
                geosite_cn(host),
                MockRoute::Ok(b"domain:example.cn\n".to_vec()),
            );
            self.routes.insert(
                geosite_geo_cn(host),
                MockRoute::Ok(b"full:baidu.com\n".to_vec()),
            );
        }
        self
    }

    fn insert(&mut self, url: String, route: MockRoute) {
        self.routes.insert(url, route);
    }

    fn hits(&self) -> Vec<String> {
        self.hits.lock().expect("hits").clone()
    }

    fn serve(&self, url: &str, max_bytes: usize) -> Result<FetchedBody, GeoError> {
        self.hits.lock().expect("hits").push(url.to_owned());
        match self.routes.get(url) {
            Some(MockRoute::Ok(body)) => {
                if body.len() > max_bytes {
                    return Err(GeoError::PayloadTooLarge(max_bytes));
                }
                Ok(FetchedBody {
                    status: 200,
                    url: url.to_owned(),
                    body: Bytes::from(body.clone()),
                })
            }
            Some(MockRoute::Status(status)) => Ok(FetchedBody {
                status: *status,
                url: url.to_owned(),
                body: Bytes::new(),
            }),
            Some(MockRoute::TooLarge) => Err(GeoError::PayloadTooLarge(max_bytes)),
            None => Ok(FetchedBody {
                status: 404,
                url: url.to_owned(),
                body: Bytes::new(),
            }),
        }
    }
}

impl HttpFetch for MockFetch {
    async fn get_capped(&self, url: &str, max_bytes: usize) -> Result<FetchedBody, GeoError> {
        self.serve(url, max_bytes)
    }
}

struct SharedFetch(Arc<MockFetch>);

impl HttpFetch for SharedFetch {
    async fn get_capped(&self, url: &str, max_bytes: usize) -> Result<FetchedBody, GeoError> {
        self.0.serve(url, max_bytes)
    }
}

#[tokio::test]
async fn downloads_verified_geoip_into_cache_layout() {
    let directory = tempfile::tempdir().unwrap();
    let downloader = GeoDownloader::new(MockFetch::new().with_cn_geoip(), directory.path());
    downloader.download_geoip(&cn()).await.unwrap();
    let cached = std::fs::read(geoip_cache_path(directory.path(), &cn())).unwrap();
    assert_eq!(cached, GEOIP_CN);
}

#[tokio::test]
async fn downloads_cn_geosite_text_lists() {
    let directory = tempfile::tempdir().unwrap();
    let downloader = GeoDownloader::new(MockFetch::new().with_cn_geosite(), directory.path());
    downloader.download_geosite(&cn()).await.unwrap();
    let cached = std::fs::read_to_string(geosite_cache_path(directory.path(), &cn())).unwrap();
    assert!(cached.contains("domain:example.cn"));
    assert!(cached.contains("full:baidu.com"));
}

#[tokio::test]
async fn checksum_mismatch_does_not_write() {
    let directory = tempfile::tempdir().unwrap();
    let mut fetch = MockFetch::new();
    for host in ALLOWED_HOSTS {
        fetch.insert(
            geoip_sum(host, "cn"),
            MockRoute::Ok(
                b"0000000000000000000000000000000000000000000000000000000000000000  cn.dat\n"
                    .to_vec(),
            ),
        );
        fetch.insert(geoip_dat(host, "cn"), MockRoute::Ok(GEOIP_CN.to_vec()));
    }
    let downloader = GeoDownloader::new(fetch, directory.path());
    let error = downloader.download_geoip(&cn()).await.unwrap_err();
    assert!(matches!(error, GeoError::ChecksumMismatch));
    assert!(!geoip_cache_path(directory.path(), &cn()).exists());
}

#[tokio::test]
async fn oversize_body_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let mut fetch = MockFetch::new().with_cn_geoip();
    for host in ALLOWED_HOSTS {
        fetch.insert(geoip_dat(host, "cn"), MockRoute::TooLarge);
    }
    let downloader = GeoDownloader::new(fetch, directory.path());
    let error = downloader.download_geoip(&cn()).await.unwrap_err();
    assert!(matches!(error, GeoError::PayloadTooLarge(MAX_GEOIP_BYTES)));
}

#[tokio::test]
async fn http_404_tries_fallbacks_then_fails() {
    let directory = tempfile::tempdir().unwrap();
    let downloader = GeoDownloader::new(MockFetch::new(), directory.path());
    let error = downloader.download_geoip(&cn()).await.unwrap_err();
    assert!(matches!(error, GeoError::GeoIpNotFound(_)));
}

#[tokio::test]
async fn missing_non_cn_geoip_is_a_clear_404() {
    let directory = tempfile::tempdir().unwrap();
    let downloader = GeoDownloader::new(MockFetch::new(), directory.path());
    let error = downloader.download_geoip(&us()).await.unwrap_err();
    assert!(matches!(error, GeoError::GeoIpNotFound(_)));
    assert!(error.to_string().contains("US"));
}

#[tokio::test]
async fn sha256_match_skips_dat_download() {
    let directory = tempfile::tempdir().unwrap();
    let path = geoip_cache_path(directory.path(), &cn());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, GEOIP_CN).unwrap();
    let fetch = Arc::new(MockFetch::new().with_cn_geoip());
    let downloader = GeoDownloader::new(SharedFetch(Arc::clone(&fetch)), directory.path());
    let results = downloader.update_cached().await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].kind, ArtifactKind::GeoIp);
    assert_eq!(results[0].status, UpdateStatus::UpToDate);
    let hits = fetch.hits();
    assert!(
        hits.iter().any(|url| url.ends_with("cn.dat.sha256sum")),
        "checksum must be fetched: {hits:?}"
    );
    assert!(
        hits.iter().all(|url| !url.ends_with("/cn.dat")),
        "dat must not be fetched when digest matches: {hits:?}"
    );
}

#[tokio::test]
async fn partial_update_failure_keeps_old_file() {
    let directory = tempfile::tempdir().unwrap();
    let cn_path = geoip_cache_path(directory.path(), &cn());
    let us_path = geoip_cache_path(directory.path(), &us());
    std::fs::create_dir_all(cn_path.parent().unwrap()).unwrap();
    std::fs::write(&cn_path, b"stale-cn").unwrap();
    std::fs::write(&us_path, b"stale-us").unwrap();

    let mut fetch = MockFetch::new().with_cn_geoip();
    for host in ALLOWED_HOSTS {
        fetch.insert(geoip_sum(host, "us"), MockRoute::Status(404));
        fetch.insert(geoip_dat(host, "us"), MockRoute::Status(404));
    }
    let downloader = GeoDownloader::new(fetch, directory.path());
    let results = downloader.update_cached().await;
    let cn_result = results
        .iter()
        .find(|result| result.country == cn() && result.kind == ArtifactKind::GeoIp)
        .unwrap();
    let us_result = results
        .iter()
        .find(|result| result.country == us() && result.kind == ArtifactKind::GeoIp)
        .unwrap();
    assert_eq!(cn_result.status, UpdateStatus::Updated);
    assert!(matches!(us_result.status, UpdateStatus::Failed { .. }));
    assert_eq!(std::fs::read(&cn_path).unwrap(), GEOIP_CN);
    assert_eq!(std::fs::read(&us_path).unwrap(), b"stale-us");
}

#[tokio::test]
async fn geosite_is_cn_only() {
    let directory = tempfile::tempdir().unwrap();
    let downloader = GeoDownloader::new(MockFetch::new(), directory.path());
    assert!(matches!(
        downloader.download_geosite(&us()).await,
        Err(GeoError::UnsupportedGeoSite(_))
    ));
}
