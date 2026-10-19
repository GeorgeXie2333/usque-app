//! Native Proton sessions use an explicit WARP underlay and platform-encrypted
//! persistence. Human verification is a one-use, generation-bound interaction.
mod api;
mod client;
mod crypto;
mod interaction;
pub(crate) mod local_agent;
mod manager;
mod nodes;
pub(crate) mod renew;
mod workflow;
pub use crate::warp_wireguard::Context;
pub use client::ClientConfig;
pub use manager::{Manager, Output, attach};
#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod nodes_tests;
#[cfg(test)]
mod workflow_tests;

use async_trait::async_trait;
use std::fmt;
use tokio_util::sync::CancellationToken;
use usque_core::proton_vpn::Node;
use zeroize::Zeroizing;

#[derive(Clone)]
pub(super) struct Failure {
    pub reason: String,
    pub code: Option<u32>,
    pub status: Option<u16>,
}
impl Failure {
    fn new(reason: &str) -> Self {
        Self {
            reason: reason.into(),
            code: None,
            status: None,
        }
    }
    fn api(reason: &str, code: u32, status: u16) -> Self {
        Self {
            reason: reason.into(),
            code: Some(code),
            status: Some(status),
        }
    }
}
impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtonFailure")
            .field("reason", &self.reason)
            .field("code", &self.code)
            .field("status", &self.status)
            .finish()
    }
}
impl From<usque_core::chain_exit::ImportError> for Failure {
    fn from(value: usque_core::chain_exit::ImportError) -> Self {
        Self::new(&value.reason)
    }
}
impl From<crypto::CryptoError> for Failure {
    fn from(_: crypto::CryptoError) -> Self {
        Self::new("cryptographic_validation_failed")
    }
}

#[async_trait]
pub(super) trait Interaction: Send + Sync {
    async fn verify(
        &self,
        challenge: Zeroizing<String>,
        methods: Vec<String>,
        cancel: &CancellationToken,
    ) -> Result<(Zeroizing<String>, String), Failure>;
    async fn totp(&self, cancel: &CancellationToken) -> Result<Zeroizing<String>, Failure>;
    async fn node(&self, nodes: Vec<Node>, cancel: &CancellationToken) -> Result<String, Failure>;
    fn phase(&self, phase: &str);
}
