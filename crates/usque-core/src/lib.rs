pub mod config;
pub mod connector;
pub mod exit_probe;
pub mod identity;
pub mod redaction;
pub mod registration;
pub mod state;
pub mod storage;
pub mod update;

pub use config::{
    AppConfig, AppPreferences, DEFAULT_PROFILE_ID, DnsMode, EndpointSettings, IpPolicy, LogLevel,
    OperatingMode, Profile, ProxyDnsMode, ProxySettings, TransportPolicy,
};
pub use connector::{
    ConnectedPath, ConnectionAttempt, ConnectionOrchestrator, ConnectorError, TransportConnector,
};
pub use exit_probe::{ExitInfo, GeoLocation, IpSbProbe, ProbeError};
pub use identity::{
    EndpointPin, IdentityError, MasqueKeyPair, WarpIdentity, parse_manual_warp_secret,
};
pub use registration::{
    ConsumerRegistrationClient, EndpointPinRefresh, PreparedEndpointPinRefresh,
    REGISTRATION_API_HOST, REGISTRATION_API_PORT, RegistrationError, RegistrationOptions,
    parse_endpoint_pin_refresh_response, prepare_endpoint_pin_refresh,
};
pub use state::{
    AddressFamily, ConnectionError, ConnectionPhase, ConnectionSnapshot, ConnectionWarning,
    ErrorCode, KillSwitchState, LockdownState, StateMachine, Statistics, Transport,
};

pub const PRODUCT_NAME: &str = "Usque";
pub const APPLICATION_ID: &str = "io.github.georgexie2333.usque";
