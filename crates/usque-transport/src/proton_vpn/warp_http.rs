//! Connection-only control traffic over the established WARP stack. This
//! handle never exposes WARP as an exit for ordinary application traffic and
//! cannot fall back to system sockets, HTTP proxies or physical DNS.
use super::api::Http;
use crate::{
    InternalNetwork,
    internal_network::{InternalHttpError, InternalRequest, InternalResponse},
};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

pub(super) struct WarpHttp {
    network: InternalNetwork,
}

impl WarpHttp {
    pub fn new(network: InternalNetwork) -> Self {
        Self { network }
    }
}

#[async_trait]
impl Http for WarpHttp {
    async fn exchange(
        &self,
        request: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        let generation = self.generation();
        if cancel.is_cancelled() {
            return Err(InternalHttpError::Cancelled);
        }
        if !self.current(generation) {
            return Err(InternalHttpError::Connect);
        }
        // Access/Lease checkpoint their captured generation before continuing
        // or authorizing traffic. Preserve a received reply so a validated,
        // known token rotation can still be saved if reconnection arrives with
        // the response; discarding it would needlessly leave the journal in an
        // unknown state. Such a save never authorizes the old lease to proceed.
        self.network.exchange_https(request, cancel).await
    }

    fn generation(&self) -> Option<u64> {
        self.network.control_generation()
    }

    fn current(&self, generation: Option<u64>) -> bool {
        generation.is_some() && self.generation() == generation
    }
}

#[cfg(test)]
#[path = "warp_http_tests.rs"]
mod tests;
