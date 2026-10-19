use super::{
    ClientConfig, Context, Failure, Interaction, api, crypto::KeyMaterial, interaction::JobIo,
    workflow,
};
use crate::warp_wireguard::Outer;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;
use usque_core::{
    DataPlaneMode,
    chain_exit::{ChainSource, ImportError, ValidatedProfile, store::ProfileCipher},
    proton_vpn::{
        LeasePersistence, MAX_INPUT_BYTES, Record, Request, Response, TunnelCredentials,
        store::SessionStore,
    },
    vpngate::PreparedProfile,
};
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct Output {
    pub metadata: Response,
    pub sensitive_handoff: Zeroizing<Vec<u8>>,
}
struct Running {
    io: Arc<JobIo>,
    cancel: CancellationToken,
    done: Arc<AtomicBool>,
}
struct RunContext {
    underlay: Context,
    client: ClientConfig,
    accounts: AccountContext,
}
#[derive(Clone, Default)]
pub struct AccountContext {
    pub active: Vec<Uuid>,
    pub selected: Option<Uuid>,
}
pub struct Manager {
    path: PathBuf,
    cipher: Arc<dyn ProfileCipher>,
    running: Mutex<Option<Running>>,
}
impl Manager {
    pub fn new(path: PathBuf, cipher: Arc<dyn ProfileCipher>) -> Self {
        Self {
            path,
            cipher,
            running: Mutex::new(None),
        }
    }
    pub fn signal_stop(&self) {
        if let Ok(slot) = self.running.lock()
            && let Some(run) = slot.as_ref()
        {
            run.cancel.cancel();
        }
    }
    pub fn stop_blocking(&self) -> bool {
        self.signal_stop();
        let done = self
            .running
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|run| run.done.clone()));
        let deadline = Instant::now() + Duration::from_secs(45);
        while done.as_ref().is_some_and(|v| !v.load(Ordering::Acquire)) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
    pub fn command(
        self: &Arc<Self>,
        request: Request,
        input: Zeroizing<Vec<u8>>,
        context: Option<Context>,
    ) -> Result<Output, ImportError> {
        self.command_with_client(request, input, context, ClientConfig::default())
    }

    pub fn command_with_client(
        self: &Arc<Self>,
        request: Request,
        input: Zeroizing<Vec<u8>>,
        context: Option<Context>,
        client_config: ClientConfig,
    ) -> Result<Output, ImportError> {
        self.command_scoped(
            request,
            input,
            context,
            client_config,
            AccountContext::default(),
        )
    }
    pub fn command_scoped(
        self: &Arc<Self>,
        request: Request,
        input: Zeroizing<Vec<u8>>,
        context: Option<Context>,
        client_config: ClientConfig,
        accounts: AccountContext,
    ) -> Result<Output, ImportError> {
        request.validate()?;
        if input.len() > MAX_INPUT_BYTES
            || !matches!(request.action.as_str(), "login" | "totp" | "verify") && !input.is_empty()
        {
            return Err(usque_core::proton_vpn::error("invalid_sensitive_input"));
        }
        let mut slot = self
            .running
            .lock()
            .map_err(|_| usque_core::proton_vpn::error("unavailable"))?;
        let mut handoff = Zeroizing::new(Vec::new());
        let parent = self
            .path
            .parent()
            .ok_or_else(|| usque_core::proton_vpn::error("unavailable"))?;
        let sessions = SessionStore::new(parent, &*self.cipher);
        if request.mutates_account()
            && let Some(id) = request.profile_id
        {
            let related = match sessions.related_ids(id) {
                Ok(ids) => ids,
                Err(failure)
                    if failure.reason == "session_not_found"
                        && matches!(request.action.as_str(), "logout" | "forget") =>
                {
                    vec![id]
                }
                Err(failure) => return Err(failure),
            };
            if related.iter().any(|id| accounts.active.contains(id))
                || matches!(request.action.as_str(), "logout" | "forget")
                    && accounts.selected.is_some_and(|id| related.contains(&id))
            {
                return Err(usque_core::proton_vpn::error("profile_in_use"));
            }
        }
        let mut servers = Vec::new();
        let mut server_total = 0;
        let mut server_revision = None;
        if request.action == "servers" {
            let record = sessions.load(
                request
                    .profile_id
                    .ok_or_else(|| usque_core::proton_vpn::error("missing_profile"))?,
            )?;
            server_total = record.server_catalog.len();
            server_revision = Some(record.catalog_revision);
            if request.offset > server_total {
                return Err(usque_core::proton_vpn::error("invalid_request"));
            }
            servers = record
                .server_catalog
                .into_iter()
                .skip(request.offset)
                .take(usque_core::proton_vpn::CATALOG_PAGE_SIZE)
                .collect();
            return Ok(Output {
                metadata: Response {
                    servers,
                    server_offset: request.offset,
                    server_total,
                    server_revision,
                    ..Default::default()
                },
                sensitive_handoff: handoff,
            });
        } else if request.action == "choose_server" {
            if slot
                .as_ref()
                .is_some_and(|run| !run.done.load(Ordering::Acquire))
            {
                return Err(usque_core::proton_vpn::error("job_busy"));
            }
            let id = request
                .profile_id
                .ok_or_else(|| usque_core::proton_vpn::error("missing_profile"))?;
            let _lease = sessions.lease(id)?;
            let mut record = sessions.load(id)?;
            if request
                .node_id
                .as_ref()
                .is_some_and(|id| !record.server_catalog.iter().any(|node| &node.id == id))
            {
                return Err(usque_core::proton_vpn::error("invalid_node"));
            }
            record.promote();
            record.server_id = request.node_id.clone();
            sessions.save(&record)?;
            let io = Arc::new(JobIo::new());
            io.finish(Ok(id), false);
            *slot = Some(Running {
                io,
                cancel: CancellationToken::new(),
                done: Arc::new(AtomicBool::new(true)),
            });
        }
        if request.action == "forget" {
            if slot
                .as_ref()
                .is_some_and(|run| !run.done.load(Ordering::Acquire))
            {
                return Err(usque_core::proton_vpn::error("job_busy"));
            }
            workflow::forget(
                request
                    .profile_id
                    .ok_or_else(|| usque_core::proton_vpn::error("missing_profile"))?,
                self.path
                    .parent()
                    .ok_or_else(|| usque_core::proton_vpn::error("unavailable"))?,
                &*self.cipher,
            )
            .map_err(|f| usque_core::proton_vpn::error(&f.reason))?;
            let io = Arc::new(JobIo::new());
            io.finish(Ok(Uuid::nil()), false);
            *slot = Some(Running {
                io,
                cancel: CancellationToken::new(),
                done: Arc::new(AtomicBool::new(true)),
            });
        } else if request.needs_network() {
            if slot
                .as_ref()
                .is_some_and(|run| !run.done.load(Ordering::Acquire))
            {
                return Err(usque_core::proton_vpn::error("job_busy"));
            }
            let context =
                context.ok_or_else(|| usque_core::proton_vpn::error("underlay_unavailable"))?;
            if context.profile.data_plane != DataPlaneMode::ConnectIp {
                return Err(usque_core::proton_vpn::error("connect_ip_required"));
            }
            let io = Arc::new(JobIo::new());
            let cancel = CancellationToken::new();
            let done = Arc::new(AtomicBool::new(false));
            let manager = self.clone();
            let thread_io = io.clone();
            let thread_cancel = cancel.clone();
            let thread_done = done.clone();
            let thread_request = request.clone();
            let thread_accounts = accounts.clone();
            std::thread::Builder::new()
                .name("proton-control".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|_| Failure::new("unavailable"))?;
                        runtime.block_on(manager.run(
                            &thread_request,
                            input,
                            RunContext {
                                underlay: context,
                                client: client_config,
                                accounts: thread_accounts,
                            },
                            &*thread_io,
                            &thread_cancel,
                        ))
                    }))
                    .unwrap_or_else(|_| Err(Failure::new("worker_failed_remote_state_unknown")));
                    thread_io.finish(result, thread_cancel.is_cancelled());
                    thread_done.store(true, Ordering::Release);
                })
                .map_err(|_| usque_core::proton_vpn::error("unavailable"))?;
            *slot = Some(Running { io, cancel, done });
        } else if matches!(
            request.action.as_str(),
            "cancel" | "totp" | "handoff" | "verify" | "select_node"
        ) {
            let run = slot
                .as_ref()
                .ok_or_else(|| usque_core::proton_vpn::error("job_not_found"))?;
            if request.job_id
                != Some(
                    run.io
                        .snapshot()
                        .map_err(|e| usque_core::proton_vpn::error(&e.reason))?
                        .id,
                )
            {
                return Err(usque_core::proton_vpn::error("job_not_found"));
            }
            if request.action == "cancel" {
                run.cancel.cancel();
            } else {
                handoff = run
                    .io
                    .command(&request, input)
                    .map_err(|e| usque_core::proton_vpn::error(&e.reason))?;
            }
        }
        let job = slot
            .as_ref()
            .map(|run| run.io.snapshot())
            .transpose()
            .map_err(|e| usque_core::proton_vpn::error(&e.reason))?;
        if request.action == "get"
            && request
                .job_id
                .is_some_and(|id| job.as_ref().is_none_or(|j| j.id != id))
        {
            return Err(usque_core::proton_vpn::error("job_not_found"));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| usque_core::proton_vpn::error("unavailable"))?;
        // Catalogue contention or storage errors must not hide the identity of
        // an already running job. The UI still needs to poll or cancel it.
        let catalogue =
            SessionStore::new(parent, &*self.cipher)
                .records()
                .and_then(|mut records| {
                    let mut profiles = Vec::new();
                    while let Some(record) = records.pop() {
                        let mut related = vec![record];
                        let mut index = 0;
                        while index < records.len() {
                            if records[index].session.user_id == related[0].session.user_id {
                                related.push(records.remove(index));
                            } else {
                                index += 1;
                            }
                        }
                        related.sort_by_key(|record| {
                            (
                                accounts.selected != Some(record.id),
                                record.rotation_pending,
                                record.version == 1,
                                std::cmp::Reverse(record.expires_at),
                                record.id,
                            )
                        });
                        let mut summary = related[0].summary();
                        summary.related_ids = related.iter().map(|record| record.id).collect();
                        profiles.push(summary);
                    }
                    let library = usque_core::chain_exit::store::ChainProfileStore::new(
                        parent,
                        &*self.cipher,
                    )
                    .list()?;
                    for profile in &mut profiles {
                        if let Some(summary) = library.iter().find(|item| {
                            item.id == profile.id && item.source == ChainSource::ProtonVpn
                        }) {
                            profile.name.clone_from(&summary.name);
                        }
                    }
                    Ok(profiles)
                });
        let (profiles, error) = match catalogue {
            Ok(profiles) => (profiles, None),
            Err(failure) => (Vec::new(), Some(failure.reason)),
        };
        Ok(Output {
            metadata: Response {
                job,
                profiles,
                error,
                servers,
                server_offset: request.offset,
                server_total,
                server_revision,
            },
            sensitive_handoff: handoff,
        })
    }
    async fn run(
        &self,
        request: &Request,
        input: Zeroizing<Vec<u8>>,
        context: RunContext,
        io: &dyn Interaction,
        cancel: &CancellationToken,
    ) -> Result<Uuid, Failure> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| Failure::new("unavailable"))?;
        let mut outer = Box::pin(Outer::open(context.underlay, cancel)).await?;
        let mut client = api::Client::new(Arc::new(outer.network.clone()));
        client.config = context.client;
        let mut result = if request.action == "logout" {
            workflow::logout(
                &mut client,
                request
                    .profile_id
                    .ok_or_else(|| Failure::new("missing_profile"))?,
                parent,
                &*self.cipher,
                io,
                cancel,
            )
            .await
        } else {
            workflow::provision(
                &mut client,
                request,
                input,
                workflow::Storage {
                    parent,
                    cipher: self.cipher.clone(),
                    accounts: context.accounts,
                },
                io,
                cancel,
            )
            .await
        };
        if result.is_err() {
            // A refresh owns an existing encrypted account; its durable pending
            // journal decides reuse. New failed logins revoke all known handles.
            let cleanup = CancellationToken::new();
            let clean = if request.action == "refresh" {
                client.revoke_retired(&cleanup).await
            } else {
                client.revoke_all(&cleanup).await
            };
            if !clean {
                result = Err(Failure::new("cleanup_failed_remote_state_unknown"));
            }
        }
        outer.close().await;
        result
    }
}

struct StoredLease {
    id: Uuid,
    parent: PathBuf,
    cipher: Arc<dyn ProfileCipher>,
}
pub(super) fn persistence(
    parent: &Path,
    cipher: Arc<dyn ProfileCipher>,
    id: Uuid,
) -> Arc<dyn LeasePersistence> {
    Arc::new(StoredLease {
        id,
        parent: parent.into(),
        cipher,
    })
}
impl LeasePersistence for StoredLease {
    fn load(&self) -> Result<Record, ImportError> {
        SessionStore::new(&self.parent, &*self.cipher).load(self.id)
    }
    fn save(&self, record: &Record) -> Result<(), ImportError> {
        if record.id != self.id {
            return Err(usque_core::proton_vpn::error("profile_mismatch"));
        }
        SessionStore::new(&self.parent, &*self.cipher).save(record)
    }
    fn lock(&self) -> Result<std::fs::File, ImportError> {
        SessionStore::new(&self.parent, &*self.cipher).lease(self.id)
    }
}
/// Attach only the encrypted lease matching this immutable selected profile.
/// No credential is copied into ordinary settings or status metadata.
pub fn attach(
    parent: &Path,
    cipher: Arc<dyn ProfileCipher>,
    prepared: &mut PreparedProfile,
) -> Result<(), ImportError> {
    let Some(summary) = prepared.summary.as_ref() else {
        return Ok(());
    };
    if summary.source != ChainSource::ProtonVpn {
        return Ok(());
    }
    let persistence = Arc::new(StoredLease {
        id: summary.id,
        parent: parent.into(),
        cipher,
    });
    let record = persistence.load()?;
    if record.revision != summary.revision || record.node.endpoint != summary.endpoint {
        return Err(usque_core::proton_vpn::error("reauthentication_required"));
    }
    let keys = KeyMaterial::from_seed(
        record
            .ed25519_seed
            .as_slice()
            .try_into()
            .map_err(|_| usque_core::proton_vpn::error("invalid_saved_key"))?,
    )
    .map_err(|_| usque_core::proton_vpn::error("invalid_saved_key"))?;
    let Some(ValidatedProfile::WireGuard(config)) = prepared.custom.as_deref() else {
        return Err(usque_core::proton_vpn::error("invalid_saved_profile"));
    };
    if !bool::from(
        config
            .private_key
            .as_slice()
            .ct_eq(keys.wireguard_private()),
    ) || STANDARD.encode(config.public_key) != record.node.public_key
    {
        return Err(usque_core::proton_vpn::error("saved_key_mismatch"));
    }
    prepared.proton = Some(Arc::new(TunnelCredentials {
        certificate_chain: record.certificate_chain.clone(),
        private_key_pkcs8: keys.pkcs8_der(),
        server_name: record.node.domain.clone(),
        expires_at: record.expires_at,
        label: record.node.label.clone(),
    }));
    prepared.proton_persistence = Some(persistence);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct UnusedCipher;
    impl ProfileCipher for UnusedCipher {
        fn seal(&self, _: Uuid, _: &[u8]) -> Result<Vec<u8>, ImportError> {
            unreachable!()
        }
        fn open(&self, _: Uuid, _: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
            unreachable!()
        }
    }
    #[test]
    fn catalogue_failure_preserves_the_running_job_for_polling_and_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("proton-vpn"), b"not a directory").unwrap();
        let manager = Arc::new(Manager::new(
            directory.path().join("profiles-v2.json"),
            Arc::new(UnusedCipher),
        ));
        let io = Arc::new(JobIo::new());
        let id = io.snapshot().unwrap().id;
        let cancel = CancellationToken::new();
        *manager.running.lock().unwrap() = Some(Running {
            io,
            cancel: cancel.clone(),
            done: Arc::new(AtomicBool::new(false)),
        });
        let get = Request::parse(&format!(r#"{{"action":"get","job_id":"{id}"}}"#)).unwrap();
        let output = manager
            .command(get, Zeroizing::new(Vec::new()), None)
            .unwrap();
        let job = output.metadata.job.unwrap();
        assert_eq!(job.id, id);
        assert_eq!(job.state, "running");
        assert_eq!(
            output.metadata.error.as_deref(),
            Some("secure_storage_failed")
        );
        let request = Request::parse(&format!(r#"{{"action":"cancel","job_id":"{id}"}}"#)).unwrap();
        manager
            .command(request, Zeroizing::new(Vec::new()), None)
            .unwrap();
        assert!(cancel.is_cancelled());
    }
}
