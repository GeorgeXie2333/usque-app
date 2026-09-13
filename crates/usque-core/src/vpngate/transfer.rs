use super::pool::{COUNTRIES_PATH, SERVERS_PATH};
use super::*;
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Transfer {
    skip_raw: bool,
    index: Option<(PoolIndex, String)>,
    servers: Option<Vec<u8>>,
    countries: Option<Vec<u8>>,
}

impl DirectoryDownloader {
    pub fn node_progress(&self) -> NodeProgress {
        self.node_progress.borrow().clone()
    }
    pub fn cancel_node(&self, operation: &str) {
        let progress = self.node_progress();
        if progress.operation_id == operation {
            self.node_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancel();
            if progress.stage == "complete" {
                self.store.release_prepared(&Selection {
                    server_id: progress.server_id,
                    config_sha256: progress.config_sha256,
                });
            }
        }
    }
    pub fn cancel_node_for(&self, server: &str) {
        if self.node_progress.borrow().server_id == server {
            self.node_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancel();
        }
    }
    pub fn begin_node(&self, request: &NodeRequest) {
        *self.node_cancel.lock().unwrap_or_else(|e| e.into_inner()) = CancellationToken::new();
        self.node_progress.send_replace(NodeProgress {
            operation_id: request.operation_id.clone(),
            server_id: request.server_id.clone(),
            config_sha256: request.config_sha256.clone(),
            stage: "preparing".into(),
            error: None,
        });
    }
    pub async fn node_operation(
        &self,
        request: &NodeRequest,
        primary: Arc<dyn CatalogueHttp>,
        warp: &dyn WarpCatalogueSource,
        parent: &CancellationToken,
    ) -> Result<(), DirectoryError> {
        request.validate()?;
        if !matches!(
            request.action,
            NodeAction::Prepare | NodeAction::Favorite | NodeAction::UpdateFavorite
        ) {
            return Err(DirectoryError::StaleSelection);
        }
        let cancel = {
            let mut slot = self.node_cancel.lock().unwrap_or_else(|e| e.into_inner());
            let child = parent.child_token();
            if slot.is_cancelled() {
                child.cancel();
            }
            *slot = child.clone();
            child
        };
        self.progress.send_replace(DownloadProgress::default());
        let result = async {
            let revision = if matches!(
                request.action,
                NodeAction::Favorite | NodeAction::UpdateFavorite
            ) {
                Some(self.store.favorite_revision(request)?)
            } else {
                None
            };
            self.prepare_node(&request.selection(), primary, warp, &cancel)
                .await?;
            if cancel.is_cancelled() {
                return Err(DirectoryError::Cancelled);
            }
            if let Some(revision) = revision {
                self.store.set_favorite(request, revision, &cancel)?;
                self.store.release_prepared(&request.selection());
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            self.store.release_prepared(&request.selection());
        }
        self.node_progress.send_modify(|p| {
            if p.operation_id != request.operation_id {
                return;
            }
            p.stage = match &result {
                Ok(()) => "complete",
                Err(DirectoryError::Cancelled) => "cancelled",
                Err(_) => "failed",
            }
            .into();
            p.error = result.as_ref().err().map(ToString::to_string);
        });
        self.set_stage(match &result {
            Ok(()) => DownloadStage::Complete,
            Err(DirectoryError::Cancelled) => DownloadStage::Cancelled,
            Err(_) => DownloadStage::Failed,
        });
        result
    }
    async fn prepare_node(
        &self,
        selection: &Selection,
        primary: Arc<dyn CatalogueHttp>,
        warp: &dyn WarpCatalogueSource,
        cancel: &CancellationToken,
    ) -> Result<ServerSummary, DirectoryError> {
        if let Some(summary) = self.store.prepare_local(selection, cancel)? {
            return Ok(summary);
        }
        let (pool, _, _, mut skip_raw) = self
            .store
            .load_pool()?
            .ok_or(DirectoryError::StaleSelection)?;
        let node = pool.selected(selection)?.clone();
        if node.summary()?.unsupported_reason.is_some() {
            return Err(DirectoryError::UnsupportedProfile);
        }
        let validate = |bytes: &[u8]| node.configuration(bytes);
        let mut result = self
            .resource(
                primary.as_ref(),
                Some((&pool.index.data_commit, node.config_path())),
                (false, &mut skip_raw),
                cancel,
                &validate,
            )
            .await?;
        if result.is_none() {
            let source = self.open_warp(warp, cancel).await?;
            let retry = self
                .resource(
                    source.as_ref(),
                    Some((&pool.index.data_commit, node.config_path())),
                    (true, &mut skip_raw),
                    cancel,
                    &validate,
                )
                .await;
            source.close().await;
            result = retry?;
        }
        let ((summary, wire), _) = result.ok_or(DirectoryError::Unavailable)?;
        self.store.stage_configuration(summary, wire, cancel)
    }
    pub(super) async fn run_pool(
        &self,
        primary: Arc<dyn CatalogueHttp>,
        warp: &dyn WarpCatalogueSource,
        cancel: &CancellationToken,
    ) -> Result<(), DirectoryError> {
        let mut transfer = Transfer::default();
        let mut complete = self
            .pool_round(primary.as_ref(), false, &mut transfer, cancel)
            .await?;
        if !complete {
            let source = self.open_warp(warp, cancel).await?;
            let retry = self
                .pool_round(source.as_ref(), true, &mut transfer, cancel)
                .await;
            source.close().await;
            complete = retry?;
        }
        if !complete {
            return Err(DirectoryError::Unavailable);
        }
        let (index, url) = transfer.index.ok_or(DirectoryError::Unavailable)?;
        let catalogue = PoolCatalogue::parse(
            index,
            &transfer.servers.ok_or(DirectoryError::Unavailable)?,
            &transfer.countries.ok_or(DirectoryError::Unavailable)?,
        )?;
        if cancel.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        let time = super::download::now_ms();
        self.store
            .save_pool(&catalogue, time, &url, transfer.skip_raw)?;
        self.progress.send_modify(|p| {
            p.source_url = Some(url);
            p.fetched_at_unix_ms = Some(time);
        });
        Ok(())
    }
    async fn pool_round(
        &self,
        http: &dyn CatalogueHttp,
        through_warp: bool,
        transfer: &mut Transfer,
        cancel: &CancellationToken,
    ) -> Result<bool, DirectoryError> {
        if transfer.index.is_none() {
            let previous = self.store.load_pool().ok().flatten().map(|(p, ..)| p.index);
            let validate = |bytes: &[u8]| {
                let index = PoolIndex::parse(bytes)?;
                if let Some(previous) = &previous {
                    index.not_older_than(previous)?;
                }
                Ok(index)
            };
            transfer.index = self
                .resource(
                    http,
                    None,
                    (through_warp, &mut transfer.skip_raw),
                    cancel,
                    &validate,
                )
                .await?;
        }
        let Some((index, _)) = &transfer.index else {
            return Ok(false);
        };
        for (path, target) in [
            (SERVERS_PATH, &mut transfer.servers),
            (COUNTRIES_PATH, &mut transfer.countries),
        ] {
            if target.is_none() {
                let descriptor = &index.files[path];
                *target = self
                    .resource(
                        http,
                        Some((&index.data_commit, path)),
                        (through_warp, &mut transfer.skip_raw),
                        cancel,
                        &|bytes| {
                            descriptor.verify(bytes, MAX_DIRECTORY_BYTES)?;
                            Ok(bytes.to_vec())
                        },
                    )
                    .await?
                    .map(|(data, _)| data);
            }
        }
        Ok(transfer.servers.is_some() && transfer.countries.is_some())
    }
    async fn open_warp(
        &self,
        warp: &dyn WarpCatalogueSource,
        cancel: &CancellationToken,
    ) -> Result<Arc<dyn CatalogueHttp>, DirectoryError> {
        self.set_stage(DownloadStage::PreparingWarp);
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DirectoryError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(30), warp.open(cancel)) => result.map_err(|_| DirectoryError::Timeout).and_then(|r| r),
        };
        if let Err(error) = &result {
            self.failure(DownloadStage::PreparingWarp, None, error);
        }
        result
    }
    async fn resource<T: Send, F: Fn(&[u8]) -> Result<T, DirectoryError> + Sync>(
        &self,
        http: &dyn CatalogueHttp,
        pinned: Option<(&str, &str)>,
        route: (bool, &mut bool),
        cancel: &CancellationToken,
        validate: &F,
    ) -> Result<Option<(T, String)>, DirectoryError> {
        let (through_warp, skip_raw) = route;
        let raw = pinned.map(|(sha,path)| format!("https://raw.githubusercontent.com/GeorgeXie2333/vpngate-list-mirror/{sha}/{path}")).unwrap_or_else(|| RAW_URL.into());
        let raw_stage = if through_warp {
            DownloadStage::WarpRaw
        } else {
            DownloadStage::PrimaryRaw
        };
        if !*skip_raw {
            self.set_stage(raw_stage);
            match fetch(http, &raw, cancel, validate).await {
                Ok(data) => return Ok(Some((data, raw))),
                Err(DirectoryError::Cancelled) => return Err(DirectoryError::Cancelled),
                Err(error) => {
                    if error == DirectoryError::Timeout {
                        *skip_raw = true;
                    }
                    self.failure(raw_stage, Some(raw), &error);
                }
            }
        }
        let stage = if through_warp {
            DownloadStage::WarpCdn
        } else {
            DownloadStage::PrimaryCdn
        };
        self.set_stage(stage);
        let race_cancel = cancel.child_token();
        let mut pending = FuturesUnordered::new();
        for host in CDN_HOSTS {
            let race = race_cancel.clone();
            let url = pinned
                .map(|(sha, path)| {
                    format!("https://{host}/gh/GeorgeXie2333/vpngate-list-mirror@{sha}/{path}")
                })
                .unwrap_or_else(|| format!("https://{host}{CDN_PATH}"));
            pending.push(async move {
                let result = fetch(http, &url, &race, validate).await;
                (url, result)
            });
        }
        while let Some((url, result)) = pending.next().await {
            match result {
                Ok(value) => {
                    race_cancel.cancel();
                    return Ok(Some((value, url)));
                }
                Err(DirectoryError::Cancelled) => return Err(DirectoryError::Cancelled),
                Err(error) => self.failure(stage, Some(url), &error),
            }
        }
        Ok(None)
    }
}
async fn fetch<T, F: Fn(&[u8]) -> Result<T, DirectoryError>>(
    http: &dyn CatalogueHttp,
    url: &str,
    cancel: &CancellationToken,
    validate: &F,
) -> Result<T, DirectoryError> {
    let bytes = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(DirectoryError::Cancelled),
        result = tokio::time::timeout(RESPONSE_TIMEOUT, http.get(url, cancel)) => result.map_err(|_| DirectoryError::Timeout)??,
    };
    if bytes.len() > response_limit(url) {
        return Err(DirectoryError::SizeLimit);
    }
    if cancel.is_cancelled() {
        return Err(DirectoryError::Cancelled);
    }
    validate(&bytes)
}
