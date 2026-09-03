use std::{io, mem, ptr, sync::Arc};

use async_trait::async_trait;
use thiserror::Error;
use windows_sys::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW,
    QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW, SC_HANDLE,
    SC_MANAGER_CONNECT, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_PRESHUTDOWN_INFO,
    SERVICE_DEMAND_START, SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_PRESHUTDOWN_INFO,
    SERVICE_QUERY_CONFIG, SERVICE_START_TYPE,
};

use crate::journal::RecoveryPhase;

pub const PRESHUTDOWN_TIMEOUT_MS: u32 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStartMode {
    Auto,
    Demand,
}

impl ServiceStartMode {
    const fn windows_value(self) -> SERVICE_START_TYPE {
        match self {
            Self::Auto => SERVICE_AUTO_START,
            Self::Demand => SERVICE_DEMAND_START,
        }
    }
}

pub const fn desired_start_mode(phase: RecoveryPhase) -> ServiceStartMode {
    match phase {
        RecoveryPhase::Clean => ServiceStartMode::Demand,
        RecoveryPhase::Preparing
        | RecoveryPhase::Prepared
        | RecoveryPhase::Active
        | RecoveryPhase::Paused
        | RecoveryPhase::Recovering
        | RecoveryPhase::RecoveryRequired => ServiceStartMode::Auto,
    }
}

#[async_trait]
pub trait ServiceStartModeController: Send + Sync {
    async fn ensure_start_mode(&self, mode: ServiceStartMode) -> Result<(), ServiceConfigError>;

    async fn ensure_shutdown_timeout(&self) -> Result<(), ServiceConfigError> {
        Ok(())
    }
}

pub struct WindowsServiceStartModeController {
    service_name: Arc<str>,
}

impl WindowsServiceStartModeController {
    pub fn new(service_name: impl Into<Arc<str>>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }
}

#[async_trait]
impl ServiceStartModeController for WindowsServiceStartModeController {
    async fn ensure_start_mode(&self, mode: ServiceStartMode) -> Result<(), ServiceConfigError> {
        let service_name = Arc::clone(&self.service_name);
        tokio::task::spawn_blocking(move || ensure_start_mode_sync(&service_name, mode))
            .await
            .map_err(|error| ServiceConfigError::Task(error.to_string()))?
    }

    async fn ensure_shutdown_timeout(&self) -> Result<(), ServiceConfigError> {
        let service_name = Arc::clone(&self.service_name);
        tokio::task::spawn_blocking(move || ensure_shutdown_timeout_sync(&service_name))
            .await
            .map_err(|error| ServiceConfigError::Task(error.to_string()))?
    }
}

fn ensure_shutdown_timeout_sync(service_name: &str) -> Result<(), ServiceConfigError> {
    // SAFETY: null names select the local SCM; the returned handle is checked.
    let manager = OwnedServiceHandle::new(unsafe {
        OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT)
    })
    .map_err(ServiceConfigError::OpenManager)?;
    let name = wide(service_name);
    // SAFETY: manager and null-terminated service name remain live for the call.
    let service = OwnedServiceHandle::new(unsafe {
        OpenServiceW(
            manager.0,
            name.as_ptr(),
            SERVICE_QUERY_CONFIG | SERVICE_CHANGE_CONFIG,
        )
    })
    .map_err(ServiceConfigError::OpenService)?;
    let mut config = SERVICE_PRESHUTDOWN_INFO {
        dwPreshutdownTimeout: 0,
    };
    let mut required = 0;
    // SAFETY: config is aligned writable storage of exactly the documented type.
    if unsafe {
        QueryServiceConfig2W(
            service.0,
            SERVICE_CONFIG_PRESHUTDOWN_INFO,
            (&mut config as *mut SERVICE_PRESHUTDOWN_INFO).cast(),
            mem::size_of::<SERVICE_PRESHUTDOWN_INFO>() as u32,
            &mut required,
        )
    } == 0
    {
        return Err(ServiceConfigError::Query(io::Error::last_os_error()));
    }
    if config.dwPreshutdownTimeout != PRESHUTDOWN_TIMEOUT_MS {
        config.dwPreshutdownTimeout = PRESHUTDOWN_TIMEOUT_MS;
        // SAFETY: only this service's preshutdown setting is changed. No global
        // shutdown timeout, service dependencies, or networking is modified.
        if unsafe {
            ChangeServiceConfig2W(
                service.0,
                SERVICE_CONFIG_PRESHUTDOWN_INFO,
                (&config as *const SERVICE_PRESHUTDOWN_INFO).cast(),
            )
        } == 0
        {
            return Err(ServiceConfigError::Change(io::Error::last_os_error()));
        }
    }
    Ok(())
}

pub struct NoopServiceStartModeController;

#[async_trait]
impl ServiceStartModeController for NoopServiceStartModeController {
    async fn ensure_start_mode(&self, _mode: ServiceStartMode) -> Result<(), ServiceConfigError> {
        Ok(())
    }
}

fn ensure_start_mode_sync(
    service_name: &str,
    mode: ServiceStartMode,
) -> Result<(), ServiceConfigError> {
    // SAFETY: null machine and database names select the local active SCM
    // database, and the returned handle is checked before use.
    let manager = OwnedServiceHandle::new(unsafe {
        OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT)
    })
    .map_err(ServiceConfigError::OpenManager)?;
    let wide_name = wide(service_name);
    // SAFETY: wide_name is null-terminated and manager is a live SCM handle.
    let service = OwnedServiceHandle::new(unsafe {
        OpenServiceW(
            manager.0,
            wide_name.as_ptr(),
            SERVICE_QUERY_CONFIG | SERVICE_CHANGE_CONFIG,
        )
    })
    .map_err(ServiceConfigError::OpenService)?;

    let current = query_start_mode(service.0)?;
    if current == mode.windows_value() {
        return Ok(());
    }

    // SAFETY: the service handle is valid and every pointer is null because
    // only the start type is being changed.
    if unsafe {
        ChangeServiceConfigW(
            service.0,
            SERVICE_NO_CHANGE,
            mode.windows_value(),
            SERVICE_NO_CHANGE as SERVICE_ERROR,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
        )
    } == 0
    {
        Err(ServiceConfigError::Change(io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn query_start_mode(service: SC_HANDLE) -> Result<SERVICE_START_TYPE, ServiceConfigError> {
    let mut required = 0_u32;
    // The first call intentionally discovers the variable-size buffer.
    // SAFETY: the null buffer and zero length are the documented size-query
    // form, and required points to writable storage.
    unsafe {
        QueryServiceConfigW(service, ptr::null_mut(), 0, &mut required);
    }
    if required < mem::size_of::<QUERY_SERVICE_CONFIGW>() as u32 {
        return Err(ServiceConfigError::Query(io::Error::last_os_error()));
    }
    let words = (required as usize).div_ceil(mem::size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    // SAFETY: usize storage provides sufficient alignment and the byte length
    // is the exact size requested by QueryServiceConfigW.
    if unsafe { QueryServiceConfigW(service, buffer.as_mut_ptr().cast(), required, &mut required) }
        == 0
    {
        return Err(ServiceConfigError::Query(io::Error::last_os_error()));
    }
    // SAFETY: the successful API call initialized a QUERY_SERVICE_CONFIGW at
    // the beginning of the aligned buffer.
    Ok(unsafe { &*buffer.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() }.dwStartType)
}

struct OwnedServiceHandle(SC_HANDLE);

impl OwnedServiceHandle {
    fn new(handle: SC_HANDLE) -> io::Result<Self> {
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for OwnedServiceHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper uniquely owns the SCM handle.
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Debug, Error)]
pub enum ServiceConfigError {
    #[error("could not open the Service Control Manager: {0}")]
    OpenManager(#[source] io::Error),
    #[error("could not open the Usque Agent service configuration: {0}")]
    OpenService(#[source] io::Error),
    #[error("could not query the Usque Agent start type: {0}")]
    Query(#[source] io::Error),
    #[error("could not change the Usque Agent start type: {0}")]
    Change(#[source] io::Error),
    #[error("service configuration task failed: {0}")]
    Task(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_clean_journal_uses_demand_start() {
        assert_eq!(
            desired_start_mode(RecoveryPhase::Clean),
            ServiceStartMode::Demand
        );
        for phase in [
            RecoveryPhase::Preparing,
            RecoveryPhase::Prepared,
            RecoveryPhase::Active,
            RecoveryPhase::Paused,
            RecoveryPhase::Recovering,
            RecoveryPhase::RecoveryRequired,
        ] {
            assert_eq!(desired_start_mode(phase), ServiceStartMode::Auto);
        }
    }
}
