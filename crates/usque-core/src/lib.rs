pub mod config;
pub mod connector;
pub mod diagnostics;
pub mod exit_probe;
pub mod failure;
pub mod geo_rules;
pub mod identity;
pub mod reconfigure;
pub mod redaction;
pub mod registration;
pub mod state;
pub mod storage;
pub mod update;

pub use config::{
    Account, AppConfig, AppPreferences, ConfigError, DEFAULT_PROFILE_ID, DnsMode, EndpointSettings,
    FrontendSettings, IpPolicy, LogLevel, MAX_GEO_DIRECT_COUNTRIES, ManagedEndpointIps,
    OperatingMode, PendingIdentityReplacement, Profile, ProxyAuthCredentials, ProxyDnsMode,
    ProxySettings, SHARED_NETWORK_SECRET_ID, SharedNetworkSettings, TransportPolicy,
    validate_proxy_password, validate_proxy_username,
};
pub use connector::{
    ConnectedPath, ConnectionAttempt, ConnectionOrchestrator, ConnectorError, TransportConnector,
};
pub use diagnostics::{
    DiagnosticCategory, DiagnosticCheckStatus, DiagnosticFinding, DiagnosticMode,
    DiagnosticSession, DiagnosticSessionState, DiagnosticSummary,
};
pub use exit_probe::{ExitInfo, GeoLocation, IpSbProbe, ProbeError};
pub use failure::{
    FailureAction, FailureMetadata, FailureSeverity, TransportFailure, TransportFailureCode,
    TransportStage,
};
pub use geo_rules::{
    GeoProgress, GeoRulesEntry, GeoRulesUpdate, download_geo_rules, global_geosite_status,
    list_geo_rules, record_successful_geo_update, update_all_geo_rules,
};
pub use identity::{
    ConsumerEntitlement, EndpointPin, IdentityError, IdentityMetadata, IdentityProvider,
    MasqueKeyPair, WarpIdentity, parse_manual_warp_secret,
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
