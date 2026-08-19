pub mod config;
pub mod connector;
pub mod exit_probe;
pub mod identity;
pub mod reconfigure;
pub mod redaction;
pub mod registration;
pub mod state;
pub mod storage;
pub mod update;

pub use config::{
    AppConfig, AppPreferences, ConfigError, DEFAULT_PROFILE_ID, DnsMode, EndpointSettings,
    FrontendSettings, IpPolicy, LogLevel, OperatingMode, Profile, ProxyAuthCredentials,
    ProxyDnsMode, ProxySettings, TransportPolicy, validate_proxy_password, validate_proxy_username,
};
pub use connector::{
    ConnectedPath, ConnectionAttempt, ConnectionOrchestrator, ConnectorError, TransportConnector,
};
pub use exit_probe::{ExitInfo, GeoLocation, IpSbProbe, ProbeError};
pub use identity::{
    EndpointPin, IdentityError, IdentityProvider, MasqueKeyPair, WarpIdentity,
    parse_manual_warp_secret,
};
pub use reconfigure::{ReconfigureClass, classify_reconfigure};
pub use registration::{
    ConsumerRegistrationClient, EndpointPinRefresh, PreparedEndpointPinRefresh,
    REGISTRATION_API_HOST, REGISTRATION_API_PORT, RegistrationError, RegistrationOptions,
    WarpAccountStatus, ZERO_TRUST_PORT, ZERO_TRUST_SNI, ZeroTrustCallback,
    ZeroTrustRegistrationResult, ZeroTrustRegistrationStage, is_zero_trust_endpoint,
    normalize_zero_trust_team, parse_endpoint_pin_refresh_response, parse_zero_trust_callback,
    prepare_endpoint_pin_refresh, zero_trust_login_url,
};
pub use state::{
    AddressFamily, ConnectionError, ConnectionPhase, ConnectionSnapshot, ConnectionWarning,
    ErrorCode, FrontendKind, FrontendPhase, FrontendStatus, KillSwitchState, LockdownState,
    StateMachine, Statistics, Transport,
};

pub const PRODUCT_NAME: &str = "Usque";
pub const APPLICATION_ID: &str = "io.github.georgexie2333.usque";
