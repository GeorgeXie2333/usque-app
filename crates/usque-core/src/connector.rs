use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::{sleep, timeout};

use crate::config::{IpPolicy, Profile, TransportPolicy};
use crate::state::{AddressFamily, ErrorCode, Transport};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionAttempt {
    pub transport: Transport,
    pub family: AddressFamily,
    pub endpoint: SocketAddr,
    pub sni: String,
    /// Production transports must never accept an attempt with this disabled.
    pub require_endpoint_pin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectedPath {
    pub transport: Transport,
    pub family: AddressFamily,
    pub endpoint: SocketAddr,
}

#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
#[error("{message}")]
pub struct ConnectorError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[async_trait]
pub trait TransportConnector: Send + Sync {
    async fn connect(&self, attempt: ConnectionAttempt) -> Result<ConnectedPath, ConnectorError>;

    /// Refreshes the endpoint pin through the authenticated WARP enrollment API.
    ///
    /// Implementations must not learn a replacement pin from the failed TLS
    /// connection itself. The orchestrator invokes this at most once per
    /// top-level connection request.
    async fn refresh_endpoint_pin(&self) -> Result<(), ConnectorError> {
        Err(ConnectorError {
            code: ErrorCode::PinMismatch,
            message: "authenticated endpoint pin refresh is unavailable".to_owned(),
            retryable: false,
        })
    }
}

#[derive(Clone)]
pub struct ConnectionOrchestrator {
    connector: Arc<dyn TransportConnector>,
    happy_eyeballs_delay: Duration,
    attempt_timeout: Duration,
}

impl ConnectionOrchestrator {
    pub fn new(connector: Arc<dyn TransportConnector>) -> Self {
        Self {
            connector,
            happy_eyeballs_delay: Duration::from_millis(250),
            attempt_timeout: Duration::from_secs(8),
        }
    }

    pub fn with_timing(
        connector: Arc<dyn TransportConnector>,
        happy_eyeballs_delay: Duration,
        attempt_timeout: Duration,
    ) -> Self {
        Self {
            connector,
            happy_eyeballs_delay,
            attempt_timeout,
        }
    }

    pub async fn connect(&self, profile: &Profile) -> Result<ConnectedPath, OrchestratorError> {
        profile
            .validate()
            .map_err(|error| OrchestratorError::InvalidProfile(error.to_string()))?;

        let transports: &[Transport] = match profile.transport {
            TransportPolicy::Auto => &[Transport::Http3, Transport::Http2],
            TransportPolicy::Http3 => &[Transport::Http3],
            TransportPolicy::Http2 => &[Transport::Http2],
        };

        let mut failures = Vec::new();
        let mut pin_refresh_attempted = false;
        for &transport in transports {
            match self.race_address_families(profile, transport).await {
                Ok(path) => return Ok(path),
                Err(mut transport_failures) => {
                    if contains_pin_mismatch(&transport_failures) {
                        if pin_refresh_attempted {
                            return Err(OrchestratorError::EndpointPinRejected(transport_failures));
                        }
                        pin_refresh_attempted = true;
                        self.connector
                            .refresh_endpoint_pin()
                            .await
                            .map_err(OrchestratorError::EndpointPinRefreshFailed)?;

                        match self.race_address_families(profile, transport).await {
                            Ok(path) => return Ok(path),
                            Err(mut retry_failures) => {
                                if contains_pin_mismatch(&retry_failures) {
                                    return Err(OrchestratorError::EndpointPinRejected(
                                        retry_failures,
                                    ));
                                }
                                failures.append(&mut retry_failures);
                            }
                        }
                    } else {
                        failures.append(&mut transport_failures);
                    }
                }
            }
        }
        Err(OrchestratorError::AllAttemptsFailed(failures))
    }

    async fn race_address_families(
        &self,
        profile: &Profile,
        transport: Transport,
    ) -> Result<ConnectedPath, Vec<AttemptFailure>> {
        let attempts = attempts_for(profile, transport);
        match attempts.as_slice() {
            [] => Err(Vec::new()),
            [only] => self
                .run_attempt(only.clone())
                .await
                .map_err(|error| vec![AttemptFailure::new(only, error)]),
            [first, second] => self.race_two(first.clone(), second.clone()).await,
            _ => unreachable!("a profile has at most one IPv4 and one IPv6 endpoint"),
        }
    }

    async fn race_two(
        &self,
        first: ConnectionAttempt,
        second: ConnectionAttempt,
    ) -> Result<ConnectedPath, Vec<AttemptFailure>> {
        let first_future = self.run_attempt(first.clone());
        tokio::pin!(first_future);
        let delay = sleep(self.happy_eyeballs_delay);
        tokio::pin!(delay);

        tokio::select! {
            first_result = &mut first_future => {
                match first_result {
                    Ok(path) => Ok(path),
                    Err(first_error) => self
                        .run_attempt(second.clone())
                        .await
                        .map_err(|second_error| vec![
                            AttemptFailure::new(&first, first_error),
                            AttemptFailure::new(&second, second_error),
                        ]),
                }
            }
            () = &mut delay => {
                let second_future = self.run_attempt(second.clone());
                tokio::pin!(second_future);
                tokio::select! {
                    first_result = &mut first_future => {
                        match first_result {
                            Ok(path) => Ok(path),
                            Err(first_error) => second_future
                                .await
                                .map_err(|second_error| vec![
                                    AttemptFailure::new(&first, first_error),
                                    AttemptFailure::new(&second, second_error),
                                ]),
                        }
                    }
                    second_result = &mut second_future => {
                        match second_result {
                            Ok(path) => Ok(path),
                            Err(second_error) => first_future
                                .await
                                .map_err(|first_error| vec![
                                    AttemptFailure::new(&first, first_error),
                                    AttemptFailure::new(&second, second_error),
                                ]),
                        }
                    }
                }
            }
        }
    }

    async fn run_attempt(
        &self,
        attempt: ConnectionAttempt,
    ) -> Result<ConnectedPath, ConnectorError> {
        if !attempt.require_endpoint_pin {
            return Err(ConnectorError {
                code: ErrorCode::PinMismatch,
                message: "endpoint pinning cannot be disabled".to_owned(),
                retryable: false,
            });
        }
        timeout(self.attempt_timeout, self.connector.connect(attempt))
            .await
            .map_err(|_| ConnectorError {
                code: ErrorCode::EndpointUnreachable,
                message: "connection attempt timed out".to_owned(),
                retryable: true,
            })?
    }
}

fn attempts_for(profile: &Profile, transport: Transport) -> Vec<ConnectionAttempt> {
    let ipv4 = || ConnectionAttempt {
        transport,
        family: AddressFamily::Ipv4,
        endpoint: profile.endpoint.ipv4_socket(),
        sni: profile.endpoint.sni.clone(),
        require_endpoint_pin: true,
    };
    let ipv6 = || ConnectionAttempt {
        transport,
        family: AddressFamily::Ipv6,
        endpoint: profile.endpoint.ipv6_socket(),
        sni: profile.endpoint.sni.clone(),
        require_endpoint_pin: true,
    };

    match profile.ip_policy {
        IpPolicy::Auto | IpPolicy::PreferIpv6 => vec![ipv6(), ipv4()],
        IpPolicy::PreferIpv4 => vec![ipv4(), ipv6()],
        IpPolicy::Ipv4Only => vec![ipv4()],
        IpPolicy::Ipv6Only => vec![ipv6()],
    }
}

fn contains_pin_mismatch(failures: &[AttemptFailure]) -> bool {
    failures
        .iter()
        .any(|failure| failure.error.code == ErrorCode::PinMismatch)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttemptFailure {
    pub transport: Transport,
    pub family: AddressFamily,
    pub endpoint: SocketAddr,
    pub error: ConnectorError,
}

impl AttemptFailure {
    fn new(attempt: &ConnectionAttempt, error: ConnectorError) -> Self {
        Self {
            transport: attempt.transport,
            family: attempt.family,
            endpoint: attempt.endpoint,
            error,
        }
    }
}

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("profile is invalid: {0}")]
    InvalidProfile(String),
    #[error("authenticated endpoint pin refresh failed: {0}")]
    EndpointPinRefreshFailed(ConnectorError),
    #[error("the endpoint pin still mismatched after one authenticated refresh")]
    EndpointPinRejected(Vec<AttemptFailure>),
    #[error("all connection attempts failed")]
    AllAttemptsFailed(Vec<AttemptFailure>),
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;

    #[derive(Clone)]
    struct FakeOutcome {
        delay: Duration,
        result: Result<(), ConnectorError>,
    }

    struct FakeConnector {
        outcomes: Mutex<HashMap<(Transport, AddressFamily), FakeOutcome>>,
    }

    #[async_trait]
    impl TransportConnector for FakeConnector {
        async fn connect(
            &self,
            attempt: ConnectionAttempt,
        ) -> Result<ConnectedPath, ConnectorError> {
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .get(&(attempt.transport, attempt.family))
                .cloned()
                .expect("fake outcome");
            sleep(outcome.delay).await;
            outcome.result?;
            Ok(ConnectedPath {
                transport: attempt.transport,
                family: attempt.family,
                endpoint: attempt.endpoint,
            })
        }
    }

    fn failed(message: &str) -> Result<(), ConnectorError> {
        Err(ConnectorError {
            code: ErrorCode::EndpointUnreachable,
            message: message.to_owned(),
            retryable: true,
        })
    }

    #[tokio::test]
    async fn auto_falls_back_to_http2_after_http3_fails() {
        let connector = Arc::new(FakeConnector {
            outcomes: Mutex::new(HashMap::from([
                (
                    (Transport::Http3, AddressFamily::Ipv6),
                    FakeOutcome {
                        delay: Duration::ZERO,
                        result: failed("h3 v6 blocked"),
                    },
                ),
                (
                    (Transport::Http3, AddressFamily::Ipv4),
                    FakeOutcome {
                        delay: Duration::ZERO,
                        result: failed("h3 v4 blocked"),
                    },
                ),
                (
                    (Transport::Http2, AddressFamily::Ipv6),
                    FakeOutcome {
                        delay: Duration::from_millis(20),
                        result: Ok(()),
                    },
                ),
                (
                    (Transport::Http2, AddressFamily::Ipv4),
                    FakeOutcome {
                        delay: Duration::from_millis(50),
                        result: Ok(()),
                    },
                ),
            ])),
        });
        let orchestrator = ConnectionOrchestrator::with_timing(
            connector,
            Duration::from_millis(2),
            Duration::from_secs(1),
        );
        let result = orchestrator.connect(&Profile::default()).await.unwrap();
        assert_eq!(result.transport, Transport::Http2);
        assert_eq!(result.family, AddressFamily::Ipv6);
    }

    #[tokio::test]
    async fn happy_eyeballs_keeps_only_the_first_success() {
        let connector = Arc::new(FakeConnector {
            outcomes: Mutex::new(HashMap::from([
                (
                    (Transport::Http3, AddressFamily::Ipv6),
                    FakeOutcome {
                        delay: Duration::from_millis(100),
                        result: Ok(()),
                    },
                ),
                (
                    (Transport::Http3, AddressFamily::Ipv4),
                    FakeOutcome {
                        delay: Duration::from_millis(5),
                        result: Ok(()),
                    },
                ),
            ])),
        });
        let profile = Profile {
            transport: TransportPolicy::Http3,
            ..Profile::default()
        };
        let orchestrator = ConnectionOrchestrator::with_timing(
            connector,
            Duration::from_millis(2),
            Duration::from_secs(1),
        );
        let result = orchestrator.connect(&profile).await.unwrap();
        assert_eq!(result.family, AddressFamily::Ipv4);
    }

    struct RefreshableConnector {
        refreshed: AtomicBool,
        refresh_count: AtomicUsize,
        accept_after_refresh: bool,
    }

    #[async_trait]
    impl TransportConnector for RefreshableConnector {
        async fn connect(
            &self,
            attempt: ConnectionAttempt,
        ) -> Result<ConnectedPath, ConnectorError> {
            if self.refreshed.load(Ordering::SeqCst) && self.accept_after_refresh {
                return Ok(ConnectedPath {
                    transport: attempt.transport,
                    family: attempt.family,
                    endpoint: attempt.endpoint,
                });
            }
            Err(ConnectorError {
                code: ErrorCode::PinMismatch,
                message: "test pin mismatch".to_owned(),
                retryable: false,
            })
        }

        async fn refresh_endpoint_pin(&self) -> Result<(), ConnectorError> {
            self.refresh_count.fetch_add(1, Ordering::SeqCst);
            self.refreshed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn pin_mismatch_gets_one_authenticated_refresh_and_retry() {
        let connector = Arc::new(RefreshableConnector {
            refreshed: AtomicBool::new(false),
            refresh_count: AtomicUsize::new(0),
            accept_after_refresh: true,
        });
        let orchestrator = ConnectionOrchestrator::with_timing(
            connector.clone(),
            Duration::ZERO,
            Duration::from_secs(1),
        );

        assert!(orchestrator.connect(&Profile::default()).await.is_ok());
        assert_eq!(connector.refresh_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn repeated_pin_mismatch_hard_stops_without_h2_fallback() {
        let connector = Arc::new(RefreshableConnector {
            refreshed: AtomicBool::new(false),
            refresh_count: AtomicUsize::new(0),
            accept_after_refresh: false,
        });
        let orchestrator = ConnectionOrchestrator::with_timing(
            connector.clone(),
            Duration::ZERO,
            Duration::from_secs(1),
        );

        assert!(matches!(
            orchestrator.connect(&Profile::default()).await,
            Err(OrchestratorError::EndpointPinRejected(_))
        ));
        assert_eq!(connector.refresh_count.load(Ordering::SeqCst), 1);
    }
}
