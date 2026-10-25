//! Ordinary OS sockets and DNS: no WARP bootstrap, socket protection, interface
//! binding or VPN bypass. The operating system and configured HTTP proxies own
//! routing. Network errors never trigger a separate physical/WARP fallback.
use super::{
    Failure,
    api::{Http, HttpFailure},
};
use crate::internal_network::{InternalHttpError, InternalRequest, InternalResponse};
use async_trait::async_trait;
use reqwest::{Client, ClientBuilder, Url, header};
use tokio_util::sync::CancellationToken;
use usque_core::{
    proton_vpn::MAX_RESPONSE_BYTES,
    vpngate::{CONNECT_TIMEOUT, RESPONSE_TIMEOUT},
};
use zeroize::Zeroizing;

pub(super) struct SystemHttp {
    client: Client,
}

fn builder() -> ClientBuilder {
    Client::builder()
        .https_only(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(RESPONSE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        // Session mutations have their own durable uncertainty journal. The
        // HTTP library must never replay them behind the caller's back.
        .retry(reqwest::retry::never())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .http1_only()
        .pool_max_idle_per_host(0)
}

impl SystemHttp {
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            client: builder()
                .build()
                .map_err(|_| Failure::new("api_transport_failed"))?,
        })
    }
}

fn failure(error: reqwest::Error) -> InternalHttpError {
    if error.is_timeout() {
        return InternalHttpError::Timeout;
    }
    let mut source = std::error::Error::source(&error);
    let mut interrupted = false;
    while let Some(cause) = source {
        if cause.is::<rustls::Error>() {
            return InternalHttpError::Tls;
        }
        // Reqwest wraps pre-header EOF as Request and truncated bodies as
        // Decode. Inspect the transport cause without treating malformed HTTP
        // or content decoding failures as a reason to switch networks.
        interrupted |= cause
            .downcast_ref::<hyper::Error>()
            .is_some_and(|e| e.is_incomplete_message() || e.is_closed())
            || cause.downcast_ref::<std::io::Error>().is_some_and(|e| {
                matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::NotConnected
                )
            });
        // io::Error::source can skip the contained rustls::Error itself and
        // expose only its inner certificate error. Inspect that wrapper too.
        source = cause
            .downcast_ref::<std::io::Error>()
            .and_then(|error| error.get_ref().map(|inner| inner as &dyn std::error::Error))
            .or_else(|| cause.source());
    }
    if error.is_connect() || interrupted {
        InternalHttpError::Connect
    } else {
        InternalHttpError::Protocol
    }
}

#[async_trait]
impl Http for SystemHttp {
    async fn exchange(
        &self,
        options: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        self.exchange_delivery(options, cancel)
            .await
            .map_err(|error| error.kind)
    }
    async fn exchange_delivery(
        &self,
        options: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, HttpFailure> {
        let InternalRequest {
            url,
            method,
            headers,
            body,
            limit,
            ipv6: _, // Family selection follows the system resolver/routing.
        } = options;
        if limit == 0 || limit > MAX_RESPONSE_BYTES {
            return Err(InternalHttpError::SizeLimit.into());
        }
        let url = Url::parse(url).map_err(|_| InternalHttpError::Protocol)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(InternalHttpError::Protocol.into());
        }
        let request = async {
            let mut request = self
                .client
                .request(method, url)
                .header(header::ACCEPT_ENCODING, "identity")
                .header(header::CONNECTION, "close");
            for (name, value) in headers {
                let mut value = header::HeaderValue::from_str(value)
                    .map_err(|_| InternalHttpError::Protocol)?;
                // Tokens and device identity must not appear in HTTP Debug
                // output. No request, response or library error is logged here.
                value.set_sensitive(true);
                request = request.header(name, value);
            }
            let mut response = request.body(body).send().await.map_err(|error| {
                let not_submitted = error.is_connect();
                HttpFailure {
                    kind: failure(error),
                    not_submitted,
                    reason: None,
                }
            })?;
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .filter(|value| value.len() <= 5 && value.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|value| (1..=3600).contains(value));
            if response
                .headers()
                .get(header::CONTENT_ENCODING)
                .is_some_and(|value| value != "identity")
            {
                return Err(InternalHttpError::Encoding.into());
            }
            if response
                .content_length()
                .is_some_and(|size| size > limit as u64)
            {
                return Err(InternalHttpError::SizeLimit.into());
            }
            let mut bytes = Zeroizing::new(Vec::new());
            while let Some(chunk) = response.chunk().await.map_err(failure)? {
                if chunk.len() > limit.saturating_sub(bytes.len()) {
                    return Err(InternalHttpError::SizeLimit.into());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(InternalResponse {
                status,
                retry_after,
                body: bytes,
            })
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(InternalHttpError::Cancelled.into()),
            result = request => result,
        }
    }

    // WARP reconnects do not invalidate a system-network job or its puzzle.
    // OS network changes are handled by the current request's ordinary socket
    // errors. Cancellation, job/session binding and expiry remain mandatory.
    fn generation(&self) -> Option<u64> {
        None
    }
    fn current(&self, generation: Option<u64>) -> bool {
        generation.is_none()
    }
}

#[cfg(test)]
#[path = "system_http_tests.rs"]
mod tests;
