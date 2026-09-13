use super::{
    CatalogueStore, DirectoryError, MAX_CONFIG_JSON_BYTES, MAX_DIRECTORY_BYTES, MAX_INDEX_BYTES,
    NodeProgress,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

pub const RAW_URL: &str =
    "https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/main/pool/latest.json";
pub const CDN_HOSTS: [&str; 5] = [
    "cdn.jsdelivr.net",
    "fastly.jsdelivr.net",
    "gcore.jsdelivr.net",
    "testingcf.jsdelivr.net",
    "quantil.jsdelivr.net",
];
pub const CDN_PATH: &str = "/gh/GeorgeXie2333/vpngate-list-mirror@latest/pool/latest.json";
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStage {
    #[default]
    Idle,
    PrimaryRaw,
    PrimaryCdn,
    PreparingWarp,
    WarpRaw,
    WarpCdn,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageFailure {
    pub stage: DownloadStage,
    pub source_url: Option<String>,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadProgress {
    pub stage: DownloadStage,
    pub failures: Vec<StageFailure>,
    pub source_url: Option<String>,
    pub fetched_at_unix_ms: Option<u64>,
}

/// Implementations return the complete HTTP entity, bounded before allocation
/// can grow past the limit. Tunnel implementations must use an internal dialer.
#[async_trait]
pub trait CatalogueHttp: Send + Sync {
    async fn get(
        &self,
        url: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, DirectoryError>;
    /// Close an owned temporary WARP session; borrowed active sessions do nothing.
    async fn close(&self) {}
}

#[async_trait]
pub trait WarpCatalogueSource: Send + Sync {
    async fn open(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn CatalogueHttp>, DirectoryError>;
}

pub struct DirectCatalogueHttp {
    client: reqwest::Client,
}
impl DirectCatalogueHttp {
    pub fn new() -> Result<Self, DirectoryError> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(RESPONSE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| DirectoryError::Request)?;
        Ok(Self { client })
    }
}
#[async_trait]
impl CatalogueHttp for DirectCatalogueHttp {
    async fn get(
        &self,
        url: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, DirectoryError> {
        if !approved_url(url) {
            return Err(DirectoryError::Request);
        }
        let limit = response_limit(url);
        let request = async {
            // All supported sources accept identity encoding. This keeps the
            // response budget independent of compressed HTTP representations.
            let mut response = self
                .client
                .get(url)
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .send()
                .await
                .map_err(request_error)?;
            if response.status() != reqwest::StatusCode::OK {
                return Err(DirectoryError::Request);
            }
            if response.content_length().is_some_and(|n| n > limit as u64) {
                return Err(DirectoryError::SizeLimit);
            }
            if response
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            {
                return Err(DirectoryError::InvalidDirectory);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(request_error)? {
                if chunk.len() > limit.saturating_sub(bytes.len()) {
                    return Err(DirectoryError::SizeLimit);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        };
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(DirectoryError::Cancelled),
            result = request => result,
        }
    }
}
fn request_error(error: reqwest::Error) -> DirectoryError {
    if error.is_timeout() {
        DirectoryError::Timeout
    } else {
        DirectoryError::Request
    }
}

pub fn approved_url(url: &str) -> bool {
    url == RAW_URL
        || CDN_HOSTS
            .iter()
            .any(|host| url == format!("https://{host}{CDN_PATH}"))
        || pinned_path(url).is_some()
}
pub(super) fn legacy_cache_url(url: &str) -> bool {
    approved_url(url)
        || url
            == "https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/refs/heads/main/data/servers.json"
        || CDN_HOSTS.iter().any(|host| {
            url == format!(
                "https://{host}/gh/GeorgeXie2333/vpngate-list-mirror@latest/data/servers.json"
            )
        })
}

fn pinned_path(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/")
        .or_else(|| {
            CDN_HOSTS.iter().find_map(|host| {
                url.strip_prefix(&format!(
                    "https://{host}/gh/GeorgeXie2333/vpngate-list-mirror@"
                ))
            })
        })?;
    let (commit, path) = rest.split_once('/')?;
    if !super::pool::valid_commit(commit) {
        return None;
    }
    if matches!(path, "pool/servers.json" | "pool/countries.json")
        || path
            .strip_prefix("pool/configs/")
            .and_then(|s| s.strip_suffix(".json"))
            .is_some_and(super::valid_hash)
    {
        Some(path)
    } else {
        None
    }
}
pub fn response_limit(url: &str) -> usize {
    match pinned_path(url) {
        Some(path) if path.starts_with("pool/configs/") => MAX_CONFIG_JSON_BYTES,
        Some(_) => MAX_DIRECTORY_BYTES,
        None => MAX_INDEX_BYTES,
    }
}
pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

struct RefreshResult {
    generation: u64,
    result: Result<DownloadProgress, DirectoryError>,
}

/// Shared by desktop and Android. Overlapping requests reuse one refresh result;
/// replacing a cache is the final operation after complete validation.
pub struct DirectoryDownloader {
    pub(super) store: CatalogueStore,
    generation: AtomicU64,
    refresh: Mutex<RefreshResult>,
    pub(super) progress: watch::Sender<DownloadProgress>,
    cancel: std::sync::Mutex<CancellationToken>,
    pub(super) node_progress: watch::Sender<NodeProgress>,
    pub(super) node_cancel: std::sync::Mutex<CancellationToken>,
}
impl DirectoryDownloader {
    pub fn new(store: CatalogueStore) -> Self {
        let (progress, _) = watch::channel(DownloadProgress::default());
        Self {
            store,
            generation: AtomicU64::new(0),
            refresh: Mutex::new(RefreshResult {
                generation: 0,
                result: Err(DirectoryError::Unavailable),
            }),
            progress,
            cancel: std::sync::Mutex::new(CancellationToken::new()),
            node_progress: watch::channel(NodeProgress::default()).0,
            node_cancel: std::sync::Mutex::new(CancellationToken::new()),
        }
    }
    pub fn subscribe(&self) -> watch::Receiver<DownloadProgress> {
        self.progress.subscribe()
    }
    pub fn progress(&self) -> DownloadProgress {
        self.progress.borrow().clone()
    }
    pub fn cancel(&self) {
        self.cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel();
    }
    pub async fn refresh(
        &self,
        primary: Arc<dyn CatalogueHttp>,
        warp: &dyn WarpCatalogueSource,
        parent: &CancellationToken,
    ) -> Result<DownloadProgress, DirectoryError> {
        let observed = self.generation.load(Ordering::Acquire);
        let mut refresh = tokio::select! {
            _ = parent.cancelled() => return Err(DirectoryError::Cancelled),
            guard = self.refresh.lock() => guard,
        };
        if refresh.generation != observed {
            return refresh.result.clone();
        }
        let cancellation = parent.child_token();
        *self.cancel.lock().unwrap_or_else(|e| e.into_inner()) = cancellation.clone();
        self.progress.send_replace(DownloadProgress::default());
        let result = self.run_pool(primary, warp, &cancellation).await;
        self.progress.send_modify(|p| {
            p.stage = match &result {
                Ok(_) => DownloadStage::Complete,
                Err(DirectoryError::Cancelled) => DownloadStage::Cancelled,
                Err(_) => DownloadStage::Failed,
            }
        });
        let result = result.map(|()| self.progress());
        refresh.generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        refresh.result = result.clone();
        result
    }

    pub(super) fn set_stage(&self, stage: DownloadStage) {
        self.progress.send_modify(|p| p.stage = stage);
    }
    pub(super) fn failure(
        &self,
        stage: DownloadStage,
        source_url: Option<String>,
        error: &DirectoryError,
    ) {
        self.progress.send_modify(|p| {
            if p.failures.len() == 16 {
                p.failures.remove(0);
            }
            p.failures.push(StageFailure {
                stage,
                source_url,
                error: error.to_string(),
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vpngate::Catalogue;
    use crate::vpngate::pool_tests::{COMMIT, Fixture};
    use std::collections::BTreeMap;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::AtomicUsize;

    struct Reply {
        delay: Duration,
        body: Result<Vec<u8>, DirectoryError>,
    }
    #[derive(Default)]
    struct MockHttp {
        replies: BTreeMap<String, Reply>,
        calls: StdMutex<Vec<String>>,
        closed: AtomicUsize,
        active: AtomicUsize,
    }
    struct Active<'a>(&'a AtomicUsize);
    impl Drop for Active<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl CatalogueHttp for MockHttp {
        async fn get(
            &self,
            url: &str,
            cancel: &CancellationToken,
        ) -> Result<Vec<u8>, DirectoryError> {
            self.calls.lock().unwrap().push(url.into());
            self.active.fetch_add(1, Ordering::SeqCst);
            let _active = Active(&self.active);
            let Some(reply) = self.replies.get(url) else {
                return Err(DirectoryError::Request);
            };
            tokio::select! {
                _ = cancel.cancelled() => Err(DirectoryError::Cancelled),
                _ = tokio::time::sleep(reply.delay) => reply.body.clone(),
            }
        }
        async fn close(&self) {
            self.closed.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Warp {
        http: Arc<MockHttp>,
        opens: AtomicUsize,
    }
    #[async_trait]
    impl WarpCatalogueSource for Warp {
        async fn open(
            &self,
            _: &CancellationToken,
        ) -> Result<Arc<dyn CatalogueHttp>, DirectoryError> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            Ok(self.http.clone())
        }
    }
    fn reply(delay: u64, body: Result<Vec<u8>, DirectoryError>) -> Reply {
        Reply {
            delay: Duration::from_millis(delay),
            body,
        }
    }
    fn warp() -> Warp {
        Warp {
            http: Arc::new(MockHttp::default()),
            opens: AtomicUsize::new(0),
        }
    }
    fn fixture() -> Vec<u8> {
        Fixture::new().index
    }

    fn with_assets(mut replies: BTreeMap<String, Reply>) -> BTreeMap<String, Reply> {
        let fixture = Fixture::new();
        for (path, bytes) in [
            ("pool/servers.json", fixture.servers),
            ("pool/countries.json", fixture.countries),
        ] {
            replies.insert(format!("https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/{COMMIT}/{path}"), reply(1, Ok(bytes.clone())));
            for host in CDN_HOSTS {
                replies.insert(
                    format!("https://{host}/gh/GeorgeXie2333/vpngate-list-mirror@{COMMIT}/{path}"),
                    reply(1, Ok(bytes.clone())),
                );
            }
        }
        replies
    }
    #[tokio::test(start_paused = true)]
    async fn raw_success_does_not_start_cdn_or_warp() {
        let directory = tempfile::tempdir().unwrap();
        let downloader = DirectoryDownloader::new(CatalogueStore::new(directory.path()));
        let http = Arc::new(MockHttp {
            replies: with_assets([(RAW_URL.into(), reply(5, Ok(fixture())))].into()),
            ..Default::default()
        });
        let warp = warp();
        let progress = downloader
            .refresh(http.clone(), &warp, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(progress.source_url.as_deref(), Some(RAW_URL));
        assert_eq!(progress.stage, DownloadStage::Complete);
        assert_eq!(http.calls.lock().unwrap().len(), 3);
        assert_eq!(warp.opens.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn raw_timeout_then_first_complete_valid_cdn_wins_and_losers_drop() {
        let directory = tempfile::tempdir().unwrap();
        let downloader = DirectoryDownloader::new(CatalogueStore::new(directory.path()));
        let mut replies = BTreeMap::from([(RAW_URL.into(), reply(20_000, Ok(fixture())))]);
        for (i, host) in CDN_HOSTS.into_iter().enumerate() {
            replies.insert(
                format!("https://{host}{CDN_PATH}"),
                if i == 0 {
                    reply(1, Ok(b"{invalid}".to_vec()))
                } else if i == 1 {
                    reply(10, Ok(fixture()))
                } else {
                    reply(1_000, Ok(fixture()))
                },
            );
        }
        let http = Arc::new(MockHttp {
            replies: with_assets(replies),
            ..Default::default()
        });
        let warp = warp();
        let progress = downloader
            .refresh(http.clone(), &warp, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            progress.source_url,
            Some(format!("https://{}{CDN_PATH}", CDN_HOSTS[1]))
        );
        assert_eq!(http.calls.lock().unwrap().len(), 16);
        assert_eq!(http.active.load(Ordering::SeqCst), 0);
        assert_eq!(progress.failures.len(), 2);
        assert_eq!(
            progress.failures[0].error,
            DirectoryError::Timeout.to_string()
        );
        assert_eq!(warp.opens.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn only_after_all_primary_sources_fail_warp_repeats_once_and_closes() {
        let directory = tempfile::tempdir().unwrap();
        let downloader = DirectoryDownloader::new(CatalogueStore::new(directory.path()));
        let http = Arc::new(MockHttp::default());
        let warp = Warp {
            http: Arc::new(MockHttp {
                replies: with_assets([(RAW_URL.into(), reply(1, Ok(fixture())))].into()),
                ..Default::default()
            }),
            opens: AtomicUsize::new(0),
        };
        let progress = downloader
            .refresh(http.clone(), &warp, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(progress.failures.len(), 6);
        assert_eq!(http.calls.lock().unwrap().len(), 6);
        assert_eq!(warp.http.calls.lock().unwrap().len(), 3);
        assert_eq!(warp.http.closed.load(Ordering::SeqCst), 1);
        assert_eq!(warp.opens.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn total_failure_preserves_cache_and_concurrent_refreshes_share_result() {
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogueStore::new(directory.path());
        store
            .save(
                &Catalogue::parse(&super::super::tests::fixture()).unwrap(),
                42,
                RAW_URL,
            )
            .unwrap();
        let downloader = DirectoryDownloader::new(store.clone());
        let http = Arc::new(MockHttp {
            replies: [(RAW_URL.into(), reply(1, Err(DirectoryError::Request)))].into(),
            ..Default::default()
        });
        let warp = warp();
        let cancel = CancellationToken::new();
        let (first, second) = tokio::join!(
            downloader.refresh(http.clone(), &warp, &cancel),
            downloader.refresh(http.clone(), &warp, &cancel)
        );
        assert_eq!(first, Err(DirectoryError::Unavailable));
        assert_eq!(first, second);
        assert_eq!(http.calls.lock().unwrap().len(), 6);
        assert_eq!(warp.http.calls.lock().unwrap().len(), 6);
        assert_eq!(warp.http.closed.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.list(&Default::default()).unwrap().fetched_at_unix_ms,
            Some(42)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_warp_download_closes_temporary_session_and_keeps_cache() {
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogueStore::new(directory.path());
        let downloader = DirectoryDownloader::new(store.clone());
        let warp = Warp {
            http: Arc::new(MockHttp {
                replies: [(RAW_URL.into(), reply(100_000, Ok(fixture())))].into(),
                ..Default::default()
            }),
            opens: AtomicUsize::new(0),
        };
        let parent = CancellationToken::new();
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            downloader.cancel();
        };
        let (result, ()) = tokio::join!(
            downloader.refresh(Arc::new(MockHttp::default()), &warp, &parent),
            cancel
        );
        assert_eq!(result, Err(DirectoryError::Cancelled));
        assert_eq!(warp.http.closed.load(Ordering::SeqCst), 1);
        assert_eq!(warp.http.active.load(Ordering::SeqCst), 0);
        assert!(store.load().unwrap().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn raw_timeout_skips_raw_in_warp_and_lazy_configuration_but_resets_next_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogueStore::new(directory.path());
        let downloader = DirectoryDownloader::new(store.clone());
        let http = Arc::new(MockHttp {
            replies: [(RAW_URL.into(), reply(1, Err(DirectoryError::Timeout)))].into(),
            ..Default::default()
        });
        let source = Warp {
            http: Arc::new(MockHttp {
                replies: with_assets(
                    [(
                        format!("https://{}{CDN_PATH}", CDN_HOSTS[0]),
                        reply(1, Ok(fixture())),
                    )]
                    .into(),
                ),
                ..Default::default()
            }),
            opens: AtomicUsize::new(0),
        };
        downloader
            .refresh(http.clone(), &source, &CancellationToken::new())
            .await
            .unwrap();
        assert!(
            source
                .http
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|url| !url.contains("raw.githubusercontent.com"))
        );
        assert_eq!(source.http.closed.load(Ordering::SeqCst), 1);
        let server = store.list(&Default::default()).unwrap().servers.remove(0);
        let request =
            super::super::pool_tests::node_request(&server, super::super::NodeAction::Prepare);
        let f = Fixture::new();
        let replies = f
            .configs
            .iter()
            .map(|(path, bytes)| {
                (
                    format!(
                        "https://{}/gh/GeorgeXie2333/vpngate-list-mirror@{COMMIT}/{path}",
                        CDN_HOSTS[0]
                    ),
                    reply(1, Ok(bytes.clone())),
                )
            })
            .collect();
        let configs = Arc::new(MockHttp {
            replies,
            ..Default::default()
        });
        downloader.begin_node(&request);
        downloader
            .node_operation(
                &request,
                configs.clone(),
                &warp(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            configs
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|url| !url.contains("raw.githubusercontent.com"))
        );
        assert!(store.load_selection(&request.selection()).is_ok());
        downloader
            .refresh(http.clone(), &source, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            http.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|url| url.as_str() == RAW_URL)
                .count(),
            2
        );
    }

    #[tokio::test(start_paused = true)]
    async fn warp_retries_only_missing_files_at_the_already_chosen_commit() {
        let directory = tempfile::tempdir().unwrap();
        let downloader = DirectoryDownloader::new(CatalogueStore::new(directory.path()));
        let f = Fixture::new();
        let servers = format!(
            "https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/{COMMIT}/pool/servers.json"
        );
        let countries = format!(
            "https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/{COMMIT}/pool/countries.json"
        );
        let http = Arc::new(MockHttp {
            replies: [
                (RAW_URL.into(), reply(1, Ok(f.index))),
                (servers, reply(1, Ok(f.servers))),
            ]
            .into(),
            ..Default::default()
        });
        let source = Warp {
            http: Arc::new(MockHttp {
                replies: [(countries.clone(), reply(1, Ok(f.countries)))].into(),
                ..Default::default()
            }),
            opens: AtomicUsize::new(0),
        };
        downloader
            .refresh(http, &source, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(*source.http.calls.lock().unwrap(), vec![countries]);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_favorite_download_closes_warp_and_never_saves_membership() {
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogueStore::new(directory.path());
        let fixture = Fixture::new();
        store
            .save_pool(&fixture.pool(), 42, RAW_URL, false)
            .unwrap();
        let server = store.list(&Default::default()).unwrap().servers.remove(0);
        let request =
            super::super::pool_tests::node_request(&server, super::super::NodeAction::Favorite);
        let replies = fixture.configs.into_iter().map(|(path, bytes)| (format!("https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/{COMMIT}/{path}"), reply(100_000, Ok(bytes)))).collect();
        let warp = Warp {
            http: Arc::new(MockHttp {
                replies,
                ..Default::default()
            }),
            opens: AtomicUsize::new(0),
        };
        let downloader = DirectoryDownloader::new(store.clone());
        downloader.begin_node(&request);
        let parent = CancellationToken::new();
        let cancellation = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            parent.cancel();
        };
        let (result, ()) = tokio::join!(
            downloader.node_operation(&request, Arc::new(MockHttp::default()), &warp, &parent),
            cancellation
        );
        assert_eq!(result, Err(DirectoryError::Cancelled));
        assert_eq!(downloader.node_progress().stage, "cancelled");
        assert_eq!(warp.http.closed.load(Ordering::SeqCst), 1);
        assert_eq!(warp.http.active.load(Ordering::SeqCst), 0);
        assert_eq!(store.list(&Default::default()).unwrap().favorite_count, 0);
        assert!(store.load_selection(&request.selection()).is_err());
    }
}
