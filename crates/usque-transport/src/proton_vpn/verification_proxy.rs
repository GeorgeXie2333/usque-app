//! Short-lived authenticated loopback CONNECT. TLS remains end-to-end in the
//! browser; this listener can only reach the explicitly supported HV origins.
use super::Failure;
use crate::InternalNetwork;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
    time::{Instant, timeout},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub(crate) const HOSTS: &[&str] = &[
    "verify.proton.me",
    "verify-api.proton.me",
    "account.proton.me",
];

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub(super) struct Descriptor {
    pub port: u16,
    pub username: String,
    pub password: String,
    pub nonce: String,
}

pub(super) struct VerificationProxy {
    descriptor: Descriptor,
    cancel: CancellationToken,
    task: AbortOnDropHandle<()>,
}
impl VerificationProxy {
    pub async fn start(
        network: InternalNetwork,
        generation: u64,
        cancel: CancellationToken,
    ) -> Result<Self, Failure> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| Failure::new("verification_proxy_unavailable"))?;
        let descriptor = Descriptor {
            port: listener
                .local_addr()
                .map_err(|_| Failure::new("verification_proxy_unavailable"))?
                .port(),
            username: Uuid::new_v4().simple().to_string(),
            password: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
            nonce: Uuid::new_v4().simple().to_string(),
        };
        let config = Arc::new(descriptor.clone());
        let stop = cancel.clone();
        let task = AbortOnDropHandle::new(tokio::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(300);
            let seen_connect = Arc::new(AtomicBool::new(false));
            let mut tasks = JoinSet::new();
            let mut health = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! { biased;
                    _ = stop.cancelled() => break,
                    _ = tokio::time::sleep_until(deadline) => break,
                    _ = health.tick() => if network.control_generation() != Some(generation) { break; },
                    _ = tasks.join_next(), if !tasks.is_empty() => (),
                    accepted = listener.accept() => match accepted {
                        Ok((stream, address)) if address.ip().is_loopback() && tasks.len() < 32 => {
                            let (network, config, seen, token) = (network.clone(), config.clone(), seen_connect.clone(), stop.child_token());
                            tasks.spawn(async move { tokio::select! { biased; _ = token.cancelled() => (), _ = serve(stream, network, (config, generation), seen, deadline, &token) => () } });
                        }
                        Ok(_) => (),
                        Err(_) => break,
                    }
                }
            }
            stop.cancel();
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }));
        Ok(Self {
            descriptor,
            cancel,
            task,
        })
    }
    pub fn descriptor(&self) -> Descriptor {
        self.descriptor.clone()
    }
    pub async fn close(&mut self) {
        self.cancel.cancel();
        let _ = (&mut self.task).await;
    }
}
impl Drop for VerificationProxy {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn reply(
    stream: &mut TcpStream,
    status: &str,
    extra: &str,
    body: &str,
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nCache-Control: no-store\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n{extra}\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await
}

struct Request {
    method: String,
    target: String,
    authorization: Zeroizing<String>,
}
fn parse(bytes: &[u8]) -> Option<Request> {
    if !bytes.is_ascii() || bytes.len() > 16384 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.strip_suffix("\r\n\r\n")?.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);
    if first.next().is_some() || !matches!(version, "HTTP/1.1" | "HTTP/1.0") || target.len() > 512 {
        return None;
    }
    let mut authorization = None;
    let mut host = None;
    for (index, line) in lines.enumerate() {
        if index >= 64 || line.starts_with([' ', '\t']) {
            return None;
        }
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return None;
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("proxy-authorization") {
            if authorization
                .replace(Zeroizing::new(value.to_owned()))
                .is_some()
            {
                return None;
            }
        } else if name.eq_ignore_ascii_case("host") {
            if host.replace(value).is_some() {
                return None;
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("content-length") && value != "0"
        {
            return None;
        }
    }
    if method == "CONNECT" && host.is_some_and(|h| h != target) {
        return None;
    }
    Some(Request {
        method: method.into(),
        target: target.into(),
        authorization: authorization.unwrap_or_default(),
    })
}

async fn serve(
    mut stream: TcpStream,
    network: InternalNetwork,
    lease: (Arc<Descriptor>, u64),
    seen: Arc<AtomicBool>,
    deadline: Instant,
    cancel: &CancellationToken,
) -> std::io::Result<()> {
    let (config, generation) = lease;
    let mut bytes = Zeroizing::new(Vec::new());
    let header = timeout(Duration::from_secs(5), async {
        // Read exactly the header, so a pipelined TLS ClientHello cannot be lost.
        while bytes.len() < 16384 {
            bytes.push(stream.read_u8().await?);
            if bytes.ends_with(b"\r\n\r\n") {
                return Ok::<_, std::io::Error>(());
            }
        }
        Err(std::io::ErrorKind::InvalidData.into())
    })
    .await;
    if !matches!(header, Ok(Ok(()))) {
        return Ok(());
    }
    let Some(request) = parse(&bytes) else {
        return reply(&mut stream, "400 Bad Request", "", "").await;
    };
    let expected = Zeroizing::new(format!(
        "Basic {}",
        STANDARD
            .encode(Zeroizing::new(format!("{}:{}", config.username, config.password)).as_bytes())
    ));
    if !bool::from(request.authorization.as_bytes().ct_eq(expected.as_bytes())) {
        return reply(
            &mut stream,
            "407 Proxy Authentication Required",
            &format!("Proxy-Authenticate: Basic realm=\"{}\"\r\n", config.nonce),
            "",
        )
        .await;
    }
    if network.control_generation() != Some(generation) {
        return reply(&mut stream, "502 Underlay Changed", "", "").await;
    }
    let probe_host = format!("{}.usque-proton-proxy.invalid", config.nonce);
    if request.method == "CONNECT" && request.target == format!("{probe_host}:443") {
        seen.store(true, Ordering::Release);
        return reply(&mut stream, "502 Probe Complete", "", "").await;
    }
    if request.method == "GET" && request.target == format!("http://{probe_host}/probe") {
        return reply(
            &mut stream,
            "302 Found",
            &format!("Location: https://{probe_host}/\r\n"),
            "",
        )
        .await;
    }
    if request.method == "GET"
        && request.target == format!("http://{probe_host}/ready")
        && seen.load(Ordering::Acquire)
    {
        return reply(
            &mut stream,
            "200 OK",
            "",
            &format!(
                "<!doctype html><script>location.replace('usque-proxy-ready://{}/{}')</script>",
                config.nonce,
                Sha256::digest(config.password.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
        )
        .await;
    }
    let Some(host) = request
        .target
        .strip_suffix(":443")
        .filter(|host| HOSTS.contains(host))
    else {
        return reply(&mut stream, "403 Forbidden", "", "").await;
    };
    if request.method != "CONNECT" || !seen.load(Ordering::Acquire) {
        return reply(&mut stream, "403 Forbidden", "", "").await;
    }
    let remote = network
        .connect_proton_verification(
            host,
            cancel,
            (Instant::now() + Duration::from_secs(5)).min(deadline),
        )
        .await;
    let Ok(mut remote) = remote else {
        return reply(&mut stream, "502 Bad Gateway", "", "").await;
    };
    // DNS or dialing may span a reconnect. Do not acknowledge CONNECT or let
    // the browser send its TLS/challenge bytes on a replacement generation.
    if network.control_generation() != Some(generation) {
        return reply(&mut stream, "502 Underlay Changed", "", "").await;
    }
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    let (activity, mut last) = tokio::sync::watch::channel(Instant::now());
    let idle = async {
        loop {
            let expires = *last.borrow_and_update() + Duration::from_secs(30);
            tokio::select! {
                _ = tokio::time::sleep_until(expires) => break,
                changed = last.changed() => if changed.is_err() { break; },
            }
        }
    };
    let (mut client_read, mut client_write) = stream.split();
    let (mut remote_read, mut remote_write) = tokio::io::split(&mut remote);
    let relay = async {
        tokio::try_join!(
            forward(&mut client_read, &mut remote_write, &activity),
            forward(&mut remote_read, &mut client_write, &activity)
        )
    };
    tokio::select! {
        _ = cancel.cancelled() => (),
        _ = tokio::time::sleep_until(deadline) => (),
        _ = idle => (),
        _ = relay => (),
    }
    Ok(())
}

async fn forward<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    activity: &tokio::sync::watch::Sender<Instant>,
) -> std::io::Result<()> {
    let mut buffer = Zeroizing::new(vec![0u8; 64 * 1024]);
    loop {
        let size = reader.read(&mut buffer).await?;
        if size == 0 {
            return writer.shutdown().await;
        }
        activity.send_replace(Instant::now());
        writer.write_all(&buffer[..size]).await?;
        activity.send_replace(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_smuggling_and_ambiguous_credentials() {
        for input in [
            "CONNECT verify.proton.me:443 HTTP/1.1\r\nHost: evil:443\r\n\r\n",
            "CONNECT verify.proton.me:443 HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "CONNECT verify.proton.me:443 HTTP/1.1\r\nProxy-Authorization: a\r\nProxy-Authorization: b\r\n\r\n",
            "CONNECT verify.proton.me:443 HTTP/1.1\r\n folded: yes\r\n\r\n",
        ] {
            assert!(parse(input.as_bytes()).is_none());
        }
        assert!(parse(b"CONNECT verify.proton.me:443 HTTP/1.1\r\nHost: verify.proton.me:443\r\nProxy-Authorization: Basic abc\r\n\r\n").is_some());
    }
}
