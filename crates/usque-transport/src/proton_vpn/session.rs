//! Saved-session recovery. A durable journal precedes the only refresh request;
//! a known replacement is saved before any subsequent protected operation.
use super::{
    Failure, Interaction,
    api::{Client, Http, Json},
    crypto::{Certificate, KeyMaterial},
    nodes, workflow,
};
use http::Method;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use usque_core::proton_vpn::{LeasePersistence, Record, RevocationHandle, SelectedNode};
use zeroize::Zeroize;

pub(super) struct Access {
    persistence: Arc<dyn LeasePersistence>,
    http: Arc<dyn Http>,
    generation: Option<u64>,
    recovered: bool,
}
impl Access {
    pub fn new(persistence: Arc<dyn LeasePersistence>, client: &Client) -> Self {
        Self {
            persistence,
            http: client.network(),
            generation: client.generation(),
            recovered: false,
        }
    }
    fn check(&self, cancel: &CancellationToken) -> Result<(), Failure> {
        if cancel.is_cancelled() {
            return Err(Failure::new("cancelled"));
        }
        if !self.http.current(self.generation) {
            return Err(Failure::new("underlay_changed"));
        }
        Ok(())
    }
    pub async fn save(&self, record: &Record, cancel: &CancellationToken) -> Result<(), Failure> {
        self.check(cancel)?;
        let started = tokio::time::Instant::now();
        let persistence = self.persistence.clone();
        let http = self.http.clone();
        let generation = self.generation;
        let task_cancel = cancel.clone();
        let record = record.clone();
        // Await this write even on cancellation. Never detach a later rotation.
        tokio::task::spawn_blocking(move || {
            if task_cancel.is_cancelled() {
                return Err(Failure::new("cancelled"));
            }
            if !http.current(generation) {
                return Err(Failure::new("underlay_changed"));
            }
            persistence.save(&record).map_err(Failure::from)
        })
        .await
        .map_err(|_| Failure::new("secure_storage_failed"))??;
        tracing::debug!(
            probe_stage = "account_save",
            elapsed_us = started.elapsed().as_micros(),
            "Proton connection preparation timing"
        );
        self.check(cancel)
    }
    async fn recover(
        &mut self,
        client: &mut Client,
        record: &mut Record,
        interaction: &dyn Interaction,
        cancel: &CancellationToken,
    ) -> Result<(), Failure> {
        self.check(cancel)?;
        if self.recovered || record.rotation_pending || client.uncertain {
            return Err(Failure::new("reauthentication_required"));
        }
        self.recovered = true;
        record.rotation_pending = true;
        self.save(record, cancel).await?;
        if let Err(failure) = workflow::refresh_session(client, interaction, cancel).await {
            // A malformed success can still return a usable revocation handle.
            // Keep it solely for explicit cleanup, never for protected requests.
            if (client.session.uid != record.session.uid
                || client.session.access_token != record.session.access_token)
                && let Some(handle) =
                    RevocationHandle::capture(&client.session, &client.config.identity)
            {
                record.retired_sessions.retain(|old| old.uid != handle.uid);
                record.retired_sessions.push(handle);
                self.persist_known(record).await?;
            }
            client.retired.clear();
            return Err(failure);
        }
        client.session.validate()?;
        if client.session.user_id != record.session.user_id {
            return Err(Failure::new("protected_user_mismatch"));
        }
        if record.session.uid != client.session.uid {
            record.connection_cache = None;
            record.server_id = record.selected_server().map(str::to_owned);
            record.promote();
            record.certificate_chain.zeroize();
            record.expires_at = 0;
            record.refresh_at = 0;
        }
        record.session = client.session.clone();
        for retired in &client.retired {
            if !record
                .retired_sessions
                .iter()
                .any(|old| old.uid == retired.uid)
                && let Some(handle) = RevocationHandle::capture(retired, &client.config.identity)
            {
                record.retired_sessions.push(handle);
            }
        }
        record.rotation_pending = false;
        // The caller still holds this account's file lease. Persist a fully
        // validated replacement even if cancellation or an underlay change
        // arrived with the response. That local write cannot authorize traffic;
        // the checkpoint below still stops every further protected operation.
        self.persist_known(record).await?;
        client.retired.clear();
        self.check(cancel)?;
        self.cleanup(client, record, cancel).await
    }
    async fn persist_known(&self, record: &Record) -> Result<(), Failure> {
        let persistence = self.persistence.clone();
        let known = record.clone();
        tokio::task::spawn_blocking(move || persistence.save(&known).map_err(Failure::from))
            .await
            .map_err(|_| Failure::new("secure_storage_failed"))?
    }
    pub async fn cleanup(
        &self,
        client: &mut Client,
        record: &mut Record,
        cancel: &CancellationToken,
    ) -> Result<(), Failure> {
        while let Some(handle) = record.retired_sessions.first().cloned() {
            self.check(cancel)?;
            let mut cleanup = Client::new(client.network());
            cleanup.config.identity = handle.api_identity.clone();
            cleanup.session = handle.session();
            if !cleanup.revoke_all(cancel).await {
                return Err(Failure::new("cleanup_failed_remote_state_unknown"));
            }
            record.retired_sessions.remove(0);
            self.persist_known(record).await?;
            self.check(cancel)?;
        }
        Ok(())
    }
    async fn get(
        &mut self,
        client: &mut Client,
        record: &mut Record,
        path: &str,
        interaction: &dyn Interaction,
        cancel: &CancellationToken,
    ) -> Result<Json, Failure> {
        self.check(cancel)?;
        match client
            .request(Method::GET, path, &Value::Null, interaction, cancel)
            .await
        {
            Err(failure) if failure.status == Some(401) && !client.uncertain && !self.recovered => {
                self.recover(client, record, interaction, cancel).await?;
                client
                    .request(Method::GET, path, &Value::Null, interaction, cancel)
                    .await
            }
            result => result,
        }
    }
    pub async fn permissions(
        &mut self,
        client: &mut Client,
        record: &mut Record,
        interaction: &dyn Interaction,
        cancel: &CancellationToken,
    ) -> Result<(), Failure> {
        self.check(cancel)?;
        let result = workflow::permissions(
            client,
            interaction,
            cancel,
            Some(&record.session.user_id),
            record.identity == "guest",
        )
        .await;
        let old_tier = record.max_tier;
        record.max_tier = match result {
            Err(failure) if failure.status == Some(401) && !client.uncertain && !self.recovered => {
                self.recover(client, record, interaction, cancel).await?;
                workflow::permissions(
                    client,
                    interaction,
                    cancel,
                    Some(&record.session.user_id),
                    record.identity == "guest",
                )
                .await?
            }
            result => result?,
        };
        if old_tier != record.max_tier {
            record.connection_cache = None;
            record.server_id = record.selected_server().map(str::to_owned);
            record.promote();
            record.certificate_chain.zeroize();
            record.expires_at = 0;
            record.refresh_at = 0;
        }
        record.server_id = record.selected_server().map(str::to_owned);
        record.promote();
        record
            .connection_cache
            .get_or_insert_default()
            .permissions_at = super::interaction::now();
        if old_tier != record.max_tier {
            self.save(record, cancel).await?;
        }
        Ok(())
    }
    pub async fn directory(
        &mut self,
        client: &mut Client,
        record: &mut Record,
        interaction: &dyn Interaction,
        cancel: &CancellationToken,
    ) -> Result<Vec<SelectedNode>, Failure> {
        interaction.phase("directory");
        self.check(cancel)?;
        let now = super::interaction::now();
        let config_cached = record
            .connection_cache
            .as_ref()
            .is_some_and(|cache| cache.config_fresh(now));
        let ports = if config_cached {
            record
                .connection_cache
                .as_ref()
                .expect("fresh configuration")
                .udp_ports
                .clone()
        } else {
            let config = self
                .get(client, record, "/vpn/v2/clientconfig", interaction, cancel)
                .await?;
            let ports = nodes::default_ports(&config.0)?;
            record
                .connection_cache
                .get_or_insert_default()
                .set_config(ports.clone(), super::interaction::now());
            ports
        };
        let validator = record
            .connection_cache
            .as_ref()
            .and_then(|cache| cache.directory_validator(now))
            .map(str::to_owned);
        let response = client
            .directory(validator.as_deref(), interaction, cancel)
            .await;
        let (directory, modified) = match response {
            Err(failure) if failure.status == Some(401) && !client.uncertain && !self.recovered => {
                self.recover(client, record, interaction, cancel).await?;
                // A validator belongs to the old authenticated snapshot. Never
                // use a 304 across a recovered session, even with the same UID.
                client.directory(None, interaction, cancel).await?
            }
            result => result?,
        };
        self.check(cancel)?;
        let Some(directory) = directory else {
            let cache = record
                .connection_cache
                .as_mut()
                .ok_or_else(|| Failure::new("invalid_server_directory"))?;
            if validator.is_none() || cache.directory_validator(now) != validator.as_deref() {
                return Err(Failure::new("invalid_server_directory"));
            }
            cache.directory_at = super::interaction::now();
            tracing::debug!(
                probe_stage = "directory_cache",
                config_cached,
                not_modified = true,
                "Proton connection preparation timing"
            );
            return Ok(cache.peers.clone());
        };
        let started = tokio::time::Instant::now();
        let config = serde_json::json!({"DefaultPorts":{"WireGuard":{"UDP":ports}}});
        let nodes = nodes::collect_for_tier(&directory.0, &config, record.max_tier)?;
        tracing::debug!(
            probe_stage = "directory_parse",
            elapsed_us = started.elapsed().as_micros(),
            config_cached,
            not_modified = false,
            "Proton connection preparation timing"
        );
        record.server_catalog = nodes.iter().map(|node| node.metadata.clone()).collect();
        record.catalog_revision = uuid::Uuid::new_v4();
        let selected = record.selected_server().map(str::to_owned);
        record.server_id = selected.clone();
        record.promote();
        let cache = record.connection_cache.get_or_insert_default();
        cache.set_directory(&nodes, selected.as_deref(), super::interaction::now());
        if cache.directory_complete {
            cache.directory_last_modified = modified;
        }
        Ok(nodes)
    }
    pub async fn certificate(
        &mut self,
        client: &mut Client,
        record: &mut Record,
        keys: &KeyMaterial,
        interaction: &dyn Interaction,
        cancel: &CancellationToken,
    ) -> Result<Certificate, Failure> {
        self.check(cancel)?;
        match workflow::certificate(client, keys, interaction, cancel).await {
            Err(failure) if failure.status == Some(401) && !client.uncertain && !self.recovered => {
                self.recover(client, record, interaction, cancel).await?;
                workflow::certificate(client, keys, interaction, cancel).await
            }
            result => result,
        }
    }
}
