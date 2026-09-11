use super::{Catalogue, CatalogueStore, DirectoryError, MAX_DIRECTORY_BYTES};
use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

pub const RAW_URL: &str = "https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/refs/heads/main/data/servers.json";
pub const CDN_HOSTS: [&str; 5] = [
    "cdn.jsdelivr.net",
    "fastly.jsdelivr.net",
    "gcore.jsdelivr.net",
    "testingcf.jsdelivr.net",
    "quantil.jsdelivr.net",
];
pub const CDN_PATH: &str = "/gh/GeorgeXie2333/vpngate-list-mirror@latest/data/servers.json";
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
            if response
                .content_length()
                .is_some_and(|n| n > MAX_DIRECTORY_BYTES as u64)
            {
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
                if chunk.len() > MAX_DIRECTORY_BYTES.saturating_sub(bytes.len()) {
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
}

struct RefreshResult {
    generation: u64,
    result: Result<DownloadProgress, DirectoryError>,
}

/// Shared by desktop and Android. Overlapping requests reuse one refresh result;
/// replacing a cache is the final operation after complete validation.
pub struct DirectoryDownloader {
    store: CatalogueStore,
    generation: AtomicU64,
    refresh: Mutex<RefreshResult>,
    progress: watch::Sender<DownloadProgress>,
    cancel: std::sync::Mutex<CancellationToken>,
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
        let result = self.run(primary, warp, &cancellation).await;
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

    async fn run(
        &self,
        primary: Arc<dyn CatalogueHttp>,
        warp: &dyn WarpCatalogueSource,
        cancellation: &CancellationToken,
    ) -> Result<(), DirectoryError> {
        let mut winner = self.attempt(primary.as_ref(), false, cancellation).await?;
        if winner.is_none() {
            self.set_stage(DownloadStage::PreparingWarp);
            let source = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(DirectoryError::Cancelled),
                result = tokio::time::timeout(Duration::from_secs(30), warp.open(cancellation)) =>
                    result.map_err(|_| DirectoryError::Timeout).and_then(|v| v),
            };
            let source = match source {
                Ok(source) => source,
                Err(error) => {
                    self.failure(DownloadStage::PreparingWarp, None, &error);
                    return Err(error);
                }
            };
            let result = self.attempt(source.as_ref(), true, cancellation).await;
            source.close().await;
            winner = result?;
        }
        let (catalogue, url) = winner.ok_or(DirectoryError::Unavailable)?;
        if cancellation.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        let fetched_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        self.store.save(&catalogue, fetched_at_unix_ms, &url)?;
        self.progress.send_modify(|p| {
            p.source_url = Some(url);
            p.fetched_at_unix_ms = Some(fetched_at_unix_ms);
        });
        Ok(())
    }

    async fn attempt(
        &self,
        http: &dyn CatalogueHttp,
        through_warp: bool,
        cancellation: &CancellationToken,
    ) -> Result<Option<(Catalogue, String)>, DirectoryError> {
        let raw_stage = if through_warp {
            DownloadStage::WarpRaw
        } else {
            DownloadStage::PrimaryRaw
        };
        self.set_stage(raw_stage);
        match fetch_validated(http, RAW_URL, cancellation).await {
            Ok(catalogue) => return Ok(Some((catalogue, RAW_URL.to_owned()))),
            Err(DirectoryError::Cancelled) => return Err(DirectoryError::Cancelled),
            Err(error) => self.failure(raw_stage, Some(RAW_URL.to_owned()), &error),
        }
        let cdn_stage = if through_warp {
            DownloadStage::WarpCdn
        } else {
            DownloadStage::PrimaryCdn
        };
        self.set_stage(cdn_stage);
        let race_cancel = cancellation.child_token();
        let mut pending = FuturesUnordered::new();
        for host in CDN_HOSTS {
            let cancel = race_cancel.clone();
            pending.push(async move {
                let url = format!("https://{host}{CDN_PATH}");
                let result = fetch_validated(http, &url, &cancel).await;
                (url, result)
            });
        }
        while let Some((url, result)) = pending.next().await {
            match result {
                Ok(catalogue) => {
                    race_cancel.cancel();
                    return Ok(Some((catalogue, url)));
                }
                Err(DirectoryError::Cancelled) => return Err(DirectoryError::Cancelled),
                Err(error) => self.failure(cdn_stage, Some(url), &error),
            }
        }
        Ok(None)
    }
    fn set_stage(&self, stage: DownloadStage) {
        self.progress.send_modify(|p| p.stage = stage);
    }
    fn failure(&self, stage: DownloadStage, source_url: Option<String>, error: &DirectoryError) {
        self.progress.send_modify(|p| {
            p.failures.push(StageFailure {
                stage,
                source_url,
                error: error.to_string(),
            })
        });
    }
}

async fn fetch_validated(
    http: &dyn CatalogueHttp,
    url: &str,
    cancellation: &CancellationToken,
) -> Result<Catalogue, DirectoryError> {
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(DirectoryError::Cancelled),
        result = tokio::time::timeout(RESPONSE_TIMEOUT, http.get(url, cancellation)) =>
            result.map_err(|_| DirectoryError::Timeout)??,
    };
    if cancellation.is_cancelled() {
        return Err(DirectoryError::Cancelled);
    }
    Catalogue::parse(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        super::super::tests::fixture()
    }

    #[tokio::test(start_paused = true)]
    async fn raw_success_does_not_start_cdn_or_warp() {
        let directory = tempfile::tempdir().unwrap();
        let downloader = DirectoryDownloader::new(CatalogueStore::new(directory.path()));
        let http = Arc::new(MockHttp {
            replies: [(RAW_URL.into(), reply(5, Ok(fixture())))].into(),
            ..Default::default()
        });
        let warp = warp();
        let progress = downloader
            .refresh(http.clone(), &warp, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(progress.source_url.as_deref(), Some(RAW_URL));
        assert_eq!(progress.stage, DownloadStage::Complete);
        assert_eq!(http.calls.lock().unwrap().len(), 1);
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
            replies,
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
        assert_eq!(http.calls.lock().unwrap().len(), 6);
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
                replies: [(RAW_URL.into(), reply(1, Ok(fixture())))].into(),
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
        assert_eq!(
            warp.http.calls.lock().unwrap().as_slice(),
            &[RAW_URL.to_string()]
        );
        assert_eq!(warp.http.closed.load(Ordering::SeqCst), 1);
        assert_eq!(warp.opens.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn total_failure_preserves_cache_and_concurrent_refreshes_share_result() {
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogueStore::new(directory.path());
        store
            .save(&Catalogue::parse(&fixture()).unwrap(), 42, RAW_URL)
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
}
