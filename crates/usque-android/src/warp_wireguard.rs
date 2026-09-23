use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};
use usque_core::{
    storage::ConfigStore,
    warp_wireguard::{Request, Response},
};
use usque_transport::warp_wireguard::{Context, Manager};
type ManagerSlot = Option<(std::path::PathBuf, Arc<Manager>)>;
static MANAGER: OnceLock<Mutex<ManagerSlot>> = OnceLock::new();
pub(crate) fn signal_stop() {
    if let Some(slot) = MANAGER.get()
        && let Ok(slot) = slot.lock()
        && let Some((_, manager)) = &*slot
    {
        manager.signal_stop();
    }
}
pub(crate) fn stop() -> bool {
    let manager = MANAGER
        .get()
        .and_then(|s| s.lock().ok())
        .and_then(|s| s.as_ref().map(|(_, m)| m.clone()));
    manager.is_none_or(|m| m.stop_blocking())
}
pub(crate) fn command(
    path: &Path,
    request: Request,
    fetch: Option<crate::vpngate::FetchContext>,
) -> Result<String, String> {
    let manager = {
        let mut slot = MANAGER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "WARP_SCAN_UNAVAILABLE")?;
        if slot.is_none() {
            *slot = Some((
                path.into(),
                Arc::new(Manager::new(
                    path.into(),
                    crate::chain_exit::profile_cipher()?,
                )),
            ));
        }
        let (current, manager) = slot.as_ref().ok_or("WARP_SCAN_UNAVAILABLE")?;
        if current != path {
            return Err("WARP_SCAN_UNAVAILABLE".into());
        }
        manager.clone()
    };
    let context = if request.needs_network() {
        let fetch = fetch.ok_or("WARP_SCAN_UNAVAILABLE")?;
        Some(Context {
            profile: ConfigStore::new(path)
                .load()
                .map_err(|_| "WARP_SCAN_UNAVAILABLE")?
                .active_profile()
                .ok_or("WARP_IDENTITY_REQUIRED")?,
            existing: fetch.networks.map(|(_, warp)| warp),
            identity: fetch.identity,
            protector: fetch.protector,
            refresher: fetch.refresher,
            allow_physical: fetch.allow_physical,
        })
    } else {
        None
    };
    let response = manager
        .command(request, context)
        .unwrap_or_else(|e| Response {
            error: Some(e.reason),
            ..Default::default()
        });
    serde_json::to_string(&response).map_err(|_| "WARP_SCAN_UNAVAILABLE".into())
}
