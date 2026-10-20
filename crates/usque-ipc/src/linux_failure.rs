//! Typed, bounded Linux evidence shared by the Agent and Engine. Free-form
//! error text is deliberately not part of the public evidence projection.
use crate::agent_v1::{
    LinuxCommandAdapter as Adapter, LinuxDnsBackend as Backend, LinuxDnsProbeFailure,
    LinuxFailureDetail, LinuxFailureReason as Reason, LinuxFailureStage as Stage,
};
use std::{error::Error, fmt, io};

pub fn valid(detail: &LinuxFailureDetail) -> bool {
    valid_fields(detail)
        && detail
            .cleanup_detail
            .as_ref()
            .is_none_or(|cleanup| cleanup.cleanup_detail.is_none() && valid_fields(cleanup))
}
fn valid_fields(detail: &LinuxFailureDetail) -> bool {
    fn numbers(errno: Option<i32>, exit: Option<i32>) -> bool {
        errno.is_none_or(|n| (1..=4095).contains(&n)) && exit.is_none_or(|n| (1..=255).contains(&n))
    }
    fn reason(value: i32) -> bool {
        Reason::try_from(value).is_ok_and(|v| v != Reason::Unspecified)
    }
    Stage::try_from(detail.stage).is_ok_and(|s| s != Stage::Unspecified)
        && reason(detail.reason)
        && Backend::try_from(detail.backend).is_ok()
        && Adapter::try_from(detail.adapter).is_ok()
        && numbers(detail.os_errno, detail.exit_code)
        && detail.dns_probe_failures.len() <= 2
        && detail
            .dns_probe_failures
            .iter()
            .enumerate()
            .all(|(i, probe)| {
                Backend::try_from(probe.backend).is_ok_and(|b| b != Backend::Unspecified)
                    && reason(probe.reason)
                    && Adapter::try_from(probe.adapter).is_ok()
                    && numbers(probe.os_errno, probe.exit_code)
                    && (i == 0 || detail.dns_probe_failures[i - 1].backend < probe.backend)
            })
}

#[derive(Debug)]
struct EvidenceError {
    detail: LinuxFailureDetail,
    source: io::Error,
}
impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Linux {:?}: {:?}",
            Stage::try_from(self.detail.stage).ok(),
            Reason::try_from(self.detail.reason).ok()
        )
    }
}
impl Error for EvidenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

pub fn detail(error: &io::Error) -> Option<LinuxFailureDetail> {
    let mut current: Option<&(dyn Error + 'static)> = error.get_ref().map(|e| e as _);
    while let Some(error) = current {
        if let Some(evidence) = error.downcast_ref::<EvidenceError>() {
            return Some(evidence.detail.clone());
        }
        current = if let Some(io) = error.downcast_ref::<io::Error>() {
            io.get_ref().map(|e| e as _)
        } else {
            error.source()
        };
    }
    None
}

pub fn wrap(stage: Stage, error: io::Error) -> io::Error {
    let mut evidence = detail(&error).unwrap_or_else(|| LinuxFailureDetail {
        reason: match error.kind() {
            io::ErrorKind::UnexpectedEof if stage == Stage::AgentHandshake => {
                Reason::HandshakeIncomplete
            }
            io::ErrorKind::TimedOut => Reason::Timeout,
            io::ErrorKind::InvalidData => Reason::MalformedResponse,
            _ => Reason::Io,
        } as i32,
        os_errno: error.raw_os_error().filter(|n| (1..=4095).contains(n)),
        ..Default::default()
    });
    // The most specific inner stage wins (e.g. DNS inside PrepareTunnel).
    if evidence.stage == 0 {
        evidence.stage = stage as i32;
    }
    with_detail(evidence, error)
}

pub fn with_cleanup(primary: io::Error, cleanup: io::Error) -> io::Error {
    let primary = wrap(Stage::Preflight, primary);
    let cleanup = wrap(Stage::Recovery, cleanup);
    let mut evidence = detail(&primary).expect("wrapped primary");
    let mut cleanup = detail(&cleanup).expect("wrapped cleanup");
    cleanup.cleanup_detail = None;
    evidence.cleanup_detail = Some(Box::new(cleanup));
    with_detail(evidence, primary)
}

pub fn with_detail(detail: LinuxFailureDetail, source: io::Error) -> io::Error {
    io::Error::new(source.kind(), EvidenceError { detail, source })
}

pub fn classified(reason: Reason, message: &'static str) -> io::Error {
    with_detail(
        LinuxFailureDetail {
            reason: reason as i32,
            ..Default::default()
        },
        io::Error::new(io::ErrorKind::Unsupported, message),
    )
}

pub fn probe(backend: Backend, error: &io::Error) -> LinuxDnsProbeFailure {
    let d = detail(error).unwrap_or_default();
    LinuxDnsProbeFailure {
        backend: backend as i32,
        reason: if d.reason == 0 {
            Reason::Io as i32
        } else {
            d.reason
        },
        adapter: d.adapter,
        os_errno: d
            .os_errno
            .or_else(|| error.raw_os_error().filter(|n| (1..=4095).contains(n))),
        exit_code: d.exit_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_invalid_probe_evidence_is_rejected_without_parsing_messages() {
        let probe = |backend| LinuxDnsProbeFailure {
            backend,
            reason: Reason::Io as i32,
            ..Default::default()
        };
        let good = LinuxFailureDetail {
            stage: Stage::DnsProbe as i32,
            reason: Reason::NoDnsBackend as i32,
            dns_probe_failures: vec![probe(1), probe(2)],
            ..Default::default()
        };
        assert!(valid(&good));
        for probes in [
            vec![probe(2), probe(1)],
            vec![probe(1), probe(1)],
            vec![probe(1), probe(2), probe(2)],
            vec![probe(99)],
        ] {
            assert!(!valid(&LinuxFailureDetail {
                dns_probe_failures: probes,
                ..good.clone()
            }));
        }
        for n in [-1, 0, 4096, i32::MAX] {
            assert!(!valid(&LinuxFailureDetail {
                os_errno: Some(n),
                ..good.clone()
            }));
        }
        for n in [-1, 0, 256, i32::MAX] {
            assert!(!valid(&LinuxFailureDetail {
                exit_code: Some(n),
                ..good.clone()
            }));
        }
        let wrapped = wrap(
            Stage::DnsApply,
            io::Error::other("secret token /private 192.0.2.1"),
        );
        let json = format!("{:?}", detail(&wrapped));
        assert!(!json.contains("secret"));
        assert!(!json.contains("192.0.2.1"));
        assert!(!json.contains("private"));
    }

    use prost::Message;
    #[test]
    fn detail_is_append_only_bounded_and_optional_on_protocol_v3() {
        let old = [10, 1, b'x', 18, 1, b'y', 24, 1];
        let mut error = crate::agent_v1::AgentError::decode(old.as_slice()).unwrap();
        assert!(error.linux_detail.is_none());
        let d = LinuxFailureDetail {
            stage: Stage::DnsProbe as i32,
            reason: Reason::NoDnsBackend as i32,
            ..Default::default()
        };
        error.linux_detail = Some(Box::new(d.clone()));
        assert!(error.encode_to_vec().starts_with(&old));
        assert!(valid(&d));
        for invalid in [
            LinuxFailureDetail {
                os_errno: Some(4096),
                ..d.clone()
            },
            LinuxFailureDetail {
                stage: 999,
                ..d.clone()
            },
            LinuxFailureDetail {
                exit_code: Some(0),
                ..d.clone()
            },
        ] {
            assert!(!valid(&invalid));
        }
        let e = wrap(Stage::DnsApply, io::Error::from_raw_os_error(13));
        let d = detail(&wrap(Stage::Preflight, e)).unwrap();
        assert_eq!(d.stage, Stage::DnsApply as i32);
        assert_eq!(d.os_errno, Some(13));
    }
}
