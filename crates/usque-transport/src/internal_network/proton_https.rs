//! One account lease's HTTP/1 connection, scoped to the fixed Proton origin and
//! current WARP generation. A submitted request is never automatically replayed.
use super::*;
use hyper::client::conn::http1::SendRequest;
use std::time::Duration;
use tokio::sync::Mutex;

const IDLE_TTL: Duration = Duration::from_secs(30);

struct Connection {
    sender: SendRequest<Full<Bytes>>,
    driver: AbortOnDropHandle<()>,
    generation: u64,
    used_at: Instant,
}
impl Connection {
    async fn close(self) {
        self.driver.abort();
        let _ = self.driver.await;
    }
}

pub(crate) struct Session {
    network: InternalNetwork,
    generation: Option<u64>,
    idle: Mutex<Option<Connection>>,
    stopped: CancellationToken,
}

impl Session {
    pub(crate) fn new(network: InternalNetwork) -> Self {
        Self {
            generation: network.control_generation(),
            network,
            idle: Mutex::new(None),
            stopped: CancellationToken::new(),
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.stopped.cancel();
        if let Some(connection) = self.idle.lock().await.take() {
            connection.close().await;
        }
    }

    async fn connect(
        &self,
        cancel: &CancellationToken,
        ipv6: Option<bool>,
        generation: u64,
    ) -> Result<Connection, InternalHttpError> {
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        let establish = async {
            let started = Instant::now();
            let stream = self
                .network
                .connect_host("vpn-api.proton.me", 443, cancel, deadline, ipv6)
                .await?;
            tracing::debug!(
                probe_stage = "tcp",
                elapsed_us = started.elapsed().as_micros(),
                "Proton HTTPS timing"
            );
            let roots =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            let name = rustls::pki_types::ServerName::try_from("vpn-api.proton.me")
                .map_err(|_| InternalHttpError::Protocol)?;
            let started = Instant::now();
            let stream = tokio_rustls::TlsConnector::from(Arc::new(config))
                .connect(name, stream)
                .await
                .map_err(|_| InternalHttpError::Tls)?;
            tracing::debug!(
                probe_stage = "tls",
                elapsed_us = started.elapsed().as_micros(),
                "Proton HTTPS timing"
            );
            self.handshake(stream, generation).await
        };
        timeout_at(deadline, establish)
            .await
            .map_err(|_| InternalHttpError::Timeout)?
    }

    async fn handshake(
        &self,
        stream: impl HttpsIo + 'static,
        generation: u64,
    ) -> Result<Connection, InternalHttpError> {
        let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| InternalHttpError::Protocol)?;
        let cancel = self.network.cancellation.clone();
        let stop = self.stopped.clone();
        let driver = AbortOnDropHandle::new(tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => (),
                _ = stop.cancelled() => (),
                _ = connection => (),
            }
        }));
        Ok(Connection {
            sender,
            driver,
            generation,
            used_at: Instant::now(),
        })
    }

    pub(crate) async fn exchange(
        &self,
        options: InternalRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<InternalResponse, InternalHttpError> {
        let uri: http::Uri = options
            .url
            .parse()
            .map_err(|_| InternalHttpError::Protocol)?;
        if uri.scheme_str() != Some("https")
            || uri
                .authority()
                .is_none_or(|a| a.as_str() != "vpn-api.proton.me")
        {
            return Err(InternalHttpError::Protocol);
        }
        if options.limit == 0 || options.limit > MAX_DIRECTORY_BYTES {
            return Err(InternalHttpError::SizeLimit);
        }
        let mut builder = http::Request::builder()
            .method(options.method)
            .uri(uri.path_and_query().map_or("/", |v| v.as_str()))
            .header(http::header::HOST, "vpn-api.proton.me")
            .header(http::header::ACCEPT_ENCODING, "identity");
        for (name, value) in options.headers {
            let name: http::HeaderName = name.parse().map_err(|_| InternalHttpError::Protocol)?;
            if [
                http::header::HOST,
                http::header::CONNECTION,
                http::header::ACCEPT_ENCODING,
                http::header::TRANSFER_ENCODING,
                http::header::CONTENT_LENGTH,
                http::header::UPGRADE,
            ]
            .contains(&name)
            {
                return Err(InternalHttpError::Protocol);
            }
            builder = builder.header(name, value);
        }
        let request = builder
            .body(Full::new(options.body))
            .map_err(|_| InternalHttpError::Protocol)?;
        let mut idle = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(InternalHttpError::Cancelled),
            _ = self.network.cancellation.cancelled() => return Err(InternalHttpError::Cancelled),
            _ = self.stopped.cancelled() => return Err(InternalHttpError::Cancelled),
            idle = self.idle.lock() => idle,
        };
        let mut connection = idle.take();
        let Some(generation) = self
            .generation
            .filter(|generation| self.network.control_generation() == Some(*generation))
        else {
            if let Some(connection) = connection {
                connection.close().await;
            }
            return Err(InternalHttpError::Connect);
        };
        // Only readiness failures before submission may rebuild a stale socket.
        // In particular, never retry a certificate or token POST after send.
        let operation = async {
            if let Some(mut existing) = connection.take() {
                if existing.generation == generation
                    && existing.used_at.elapsed() < IDLE_TTL
                    && existing.sender.ready().await.is_ok()
                {
                    connection = Some(existing);
                } else {
                    existing.close().await;
                }
            }
            if connection.is_none() {
                connection = Some(self.connect(cancel, options.ipv6, generation).await?);
            }
            if self.network.control_generation() != Some(generation) {
                return Err(InternalHttpError::Connect);
            }
            let active = connection.as_mut().expect("connected");
            active
                .sender
                .ready()
                .await
                .map_err(|_| InternalHttpError::Protocol)?;
            let started = Instant::now();
            let mut response = active
                .sender
                .send_request(request)
                .await
                .map_err(|_| InternalHttpError::Protocol)?;
            tracing::debug!(
                probe_stage = "headers",
                elapsed_us = started.elapsed().as_micros(),
                "Proton HTTPS timing"
            );
            let reusable = response.status() != http::StatusCode::SWITCHING_PROTOCOLS
                && !response
                    .headers()
                    .get_all(http::header::CONNECTION)
                    .iter()
                    .any(|v| {
                        v.to_str().map_or(true, |v| {
                            v.split(',').any(|v| v.trim().eq_ignore_ascii_case("close"))
                        })
                    });
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(http::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.len() <= 5 && v.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|v| (1..=3600).contains(v));
            if response
                .headers()
                .get(http::header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            {
                return Err(InternalHttpError::Encoding);
            }
            if response
                .headers()
                .get(http::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|n| n > options.limit as u64)
            {
                return Err(InternalHttpError::SizeLimit);
            }
            let mut body = zeroize::Zeroizing::new(Vec::new());
            while let Some(frame) = response.body_mut().frame().await {
                let frame = frame.map_err(|_| InternalHttpError::Protocol)?;
                if let Some(data) = frame.data_ref() {
                    if data.len() > options.limit.saturating_sub(body.len()) {
                        return Err(InternalHttpError::SizeLimit);
                    }
                    body.extend_from_slice(data);
                }
            }
            Ok((
                InternalResponse {
                    status,
                    retry_after,
                    body,
                },
                reusable,
            ))
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(InternalHttpError::Cancelled),
            _ = self.network.cancellation.cancelled() => Err(InternalHttpError::Cancelled),
            _ = self.stopped.cancelled() => Err(InternalHttpError::Cancelled),
            result = tokio::time::timeout(RESPONSE_TIMEOUT, operation) => result.map_err(|_| InternalHttpError::Timeout).and_then(|v| v),
        };
        if let Some(mut connection) = connection {
            if result.as_ref().is_ok_and(|(_, reuse)| *reuse)
                && self.network.control_generation() == Some(generation)
                && !cancel.is_cancelled()
                && !self.stopped.is_cancelled()
            {
                connection.used_at = Instant::now();
                *idle = Some(connection);
            } else {
                connection.close().await;
            }
        }
        // Keep a fully received token replacement even if WARP changed with
        // the response. Access persists it before its generation checkpoint.
        result.map(|(reply, _)| reply)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stopped.cancel();
    }
}

#[cfg(test)]
#[path = "proton_https_tests.rs"]
mod tests;
