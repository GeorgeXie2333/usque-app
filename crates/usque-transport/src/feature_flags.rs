//! Internal build decisions, never deserialized from profiles, IPC or env vars.
//! Changing a production default requires rebuild, review and the ordinary CI
//! gates. Test/lab constructors inject a copy; live instances are immutable.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkFeatureFlags {
    pub h2_tuned_flow_control: bool,
    pub network_quality_metrics: bool,
    pub udp_batch_io: bool,
    pub automatic_pmtu: bool,
    pub quic_migration: bool,
}

pub const PRODUCTION_NETWORK_FEATURES: NetworkFeatureFlags = NetworkFeatureFlags {
    h2_tuned_flow_control: true,
    network_quality_metrics: true,
    udp_batch_io: true,
    automatic_pmtu: true,
    quic_migration: true,
};

// This is only an emergency capability rollback. Normal encryption selection
// remains explicit per Profile; false rejects saved encrypted configurations.
pub const ENCRYPTED_DIRECT_DNS_ENABLED: bool = true;

impl Default for NetworkFeatureFlags {
    fn default() -> Self {
        PRODUCTION_NETWORK_FEATURES
    }
}
