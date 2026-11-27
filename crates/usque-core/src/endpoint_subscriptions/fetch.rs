//! Direct bounded HTTPS downloader for disconnected endpoint subscription refreshes.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use futures::StreamExt;
use reqwest::{
    Client, StatusCode, Url,
    header::{ACCEPT, LOCATION, USER_AGENT},
    redirect::Policy,
};
use tokio_util::sync::CancellationToken;

use super::{
    MAX_SUBSCRIPTION_BODY_BYTES, SubscriptionError, is_usable_public_endpoint_ip,
    resolve_subscription_redirect_url, validate_subscription_url,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const USER_AGENT_VALUE: &str = concat!("Usque/", env!("CARGO_PKG_VERSION"));

/// Non-sensitive errors for bounded subscription downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionFetchError {
    InvalidUrl,
    Cancelled,
    Timeout,
    TooLarge,
    HttpStatus(u16),
    Network,
}

impl SubscriptionFetchError {
    pub const fn to_subscription_error(self) -> SubscriptionError {
        match self {
            Self::InvalidUrl => SubscriptionError::InvalidUrl,
            Self::Cancelled => SubscriptionError::Cancelled,
            Self::Timeout => SubscriptionError::Timeout,
            Self::TooLarge => SubscriptionError::TooLarge,
            Self::HttpStatus(_) => SubscriptionError::HttpError,
            Self::Network => SubscriptionError::Network,
        }
    }
}

/// Bounded direct HTTPS GET with strict public-IP SSRF checks on every hop,
/// `no_proxy()`, manual redirect validation (at most `max_redirects`), and a
/// strict response byte/time ceiling. Never logs subscription URLs or contents.
pub async fn fetch_subscription_direct(
    url: &str,
    max_bytes: usize,
    max_redirects: usize,
    total_timeout: Duration,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, SubscriptionFetchError> {
    if max_bytes == 0 || max_bytes > MAX_SUBSCRIPTION_BODY_BYTES {
        return Err(SubscriptionFetchError::TooLarge);
    }
    let mut current_url =
        validate_subscription_url(url).map_err(|_| SubscriptionFetchError::InvalidUrl)?;
    let deadline = Instant::now() + total_timeout;

    for hop in 0..=max_redirects {
        if cancel.is_cancelled() {
            return Err(SubscriptionFetchError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(SubscriptionFetchError::Timeout);
        }

        let parsed = Url::parse(&current_url).map_err(|_| SubscriptionFetchError::InvalidUrl)?;
        let raw_host = parsed
            .host_str()
            .ok_or(SubscriptionFetchError::InvalidUrl)?;
        let port = parsed.port_or_known_default().unwrap_or(443);
        let (is_ip_literal, resolved) =
            resolve_public_target(raw_host, port, remaining, cancel).await?;

        let mut builder = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(CONNECT_TIMEOUT.min(remaining))
            .timeout(remaining);
        if !is_ip_literal {
            builder = builder.resolve_to_addrs(raw_host, &resolved);
        }
        let client = builder
            .build()
            .map_err(|_| SubscriptionFetchError::Network)?;

        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(SubscriptionFetchError::Cancelled),
            res = tokio::time::timeout(
                remaining,
                client
                    .get(current_url.as_str())
                    .header(USER_AGENT, USER_AGENT_VALUE)
                    .header(ACCEPT, "text/plain, */*;q=0.5")
                    .send(),
            ) => match res {
                Ok(Ok(response)) => response,
                Ok(Err(error)) if error.is_timeout() => return Err(SubscriptionFetchError::Timeout),
                Ok(Err(_)) => return Err(SubscriptionFetchError::Network),
                Err(_) => return Err(SubscriptionFetchError::Timeout),
            },
        };

        let status = response.status();
        if status.is_redirection()
            || matches!(
                status,
                StatusCode::MOVED_PERMANENTLY
                    | StatusCode::FOUND
                    | StatusCode::SEE_OTHER
                    | StatusCode::TEMPORARY_REDIRECT
                    | StatusCode::PERMANENT_REDIRECT
            )
        {
            if hop == max_redirects {
                return Err(SubscriptionFetchError::HttpStatus(status.as_u16()));
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(SubscriptionFetchError::Network)?;
            current_url = resolve_subscription_redirect_url(&current_url, location)
                .map_err(|_| SubscriptionFetchError::InvalidUrl)?;
            continue;
        }

        if !status.is_success() {
            return Err(SubscriptionFetchError::HttpStatus(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64)
        {
            return Err(SubscriptionFetchError::TooLarge);
        }

        let mut bytes = Vec::with_capacity(
            response
                .content_length()
                .and_then(|length| usize::try_from(length).ok())
                .unwrap_or(0)
                .min(max_bytes),
        );
        let mut stream = response.bytes_stream();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SubscriptionFetchError::Timeout);
            }
            let chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(SubscriptionFetchError::Cancelled),
                res = tokio::time::timeout(remaining, stream.next()) => match res {
                    Ok(Some(Ok(chunk))) => chunk,
                    Ok(Some(Err(error))) if error.is_timeout() => {
                        return Err(SubscriptionFetchError::Timeout);
                    }
                    Ok(Some(Err(_))) => return Err(SubscriptionFetchError::Network),
                    Ok(None) => return Ok(bytes),
                    Err(_) => return Err(SubscriptionFetchError::Timeout),
                },
            };
            if bytes.len().saturating_add(chunk.len()) > max_bytes {
                return Err(SubscriptionFetchError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
    }

    Err(SubscriptionFetchError::Network)
}

async fn resolve_public_target(
    raw_host: &str,
    port: u16,
    remaining: Duration,
    cancel: &CancellationToken,
) -> Result<(bool, Vec<SocketAddr>), SubscriptionFetchError> {
    if let Some(bracketed) = raw_host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        && let Ok(v6) = bracketed.parse::<Ipv6Addr>()
    {
        return if is_usable_public_endpoint_ip(IpAddr::V6(v6)) {
            Ok((true, vec![SocketAddr::new(IpAddr::V6(v6), port)]))
        } else {
            Err(SubscriptionFetchError::InvalidUrl)
        };
    }
    if let Ok(v4) = raw_host.parse::<Ipv4Addr>() {
        return if is_usable_public_endpoint_ip(IpAddr::V4(v4)) {
            Ok((true, vec![SocketAddr::new(IpAddr::V4(v4), port)]))
        } else {
            Err(SubscriptionFetchError::InvalidUrl)
        };
    }

    let lookup_budget = CONNECT_TIMEOUT.min(remaining);
    let lookup_host = raw_host.to_owned();
    let addrs = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(SubscriptionFetchError::Cancelled),
        res = tokio::time::timeout(lookup_budget, tokio::net::lookup_host((lookup_host.as_str(), port))) => match res {
            Ok(Ok(iter)) => iter.collect::<Vec<_>>(),
            Ok(Err(_)) => return Err(SubscriptionFetchError::Network),
            Err(_) => return Err(SubscriptionFetchError::Timeout),
        },
    };
    let mut public_addrs = Vec::new();
    for addr in addrs {
        if is_usable_public_endpoint_ip(addr.ip()) && !public_addrs.contains(&addr) {
            public_addrs.push(addr);
        }
    }
    if public_addrs.is_empty() {
        return Err(SubscriptionFetchError::InvalidUrl);
    }
    Ok((false, public_addrs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn direct_fetch_rejects_local_private_and_non_https_urls() {
        let cancel = CancellationToken::new();
        for bad in [
            "http://example.com/sub.txt",
            "https://127.0.0.1/sub.txt",
            "https://[::1]/sub.txt",
            "https://192.168.1.1/sub.txt",
            "https://localhost/sub.txt",
            "https://user:pass@example.com/sub.txt",
        ] {
            let err = fetch_subscription_direct(
                bad,
                MAX_SUBSCRIPTION_BODY_BYTES,
                3,
                Duration::from_secs(5),
                &cancel,
            )
            .await
            .unwrap_err();
            assert_eq!(err, SubscriptionFetchError::InvalidUrl, "url={bad}");
        }
    }

    #[tokio::test]
    async fn direct_fetch_respects_cancellation_and_size_bounds() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = fetch_subscription_direct(
            "https://example.com/sub.txt",
            MAX_SUBSCRIPTION_BODY_BYTES,
            3,
            Duration::from_secs(5),
            &cancel,
        )
        .await
        .unwrap_err();
        assert_eq!(err, SubscriptionFetchError::Cancelled);

        let uncanceled = CancellationToken::new();
        let too_big = fetch_subscription_direct(
            "https://example.com/sub.txt",
            MAX_SUBSCRIPTION_BODY_BYTES + 1,
            3,
            Duration::from_secs(5),
            &uncanceled,
        )
        .await
        .unwrap_err();
        assert_eq!(too_big, SubscriptionFetchError::TooLarge);
    }
}
