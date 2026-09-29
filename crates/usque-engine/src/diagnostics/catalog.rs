use std::sync::Arc;

use usque_core::{DiagnosticCategory as Category, DiagnosticMode as Mode};

use super::checks::{DiagnosticCheck, PassiveCheck, PassiveCheckKind as Kind};

pub(crate) fn diagnostic_catalog() -> Vec<Arc<dyn DiagnosticCheck>> {
    usque_core::diagnostics_contract_generated::CHECK_DEFINITIONS
        .iter()
        .map(|definition| match definition.kind {
            "H3Probe" => {
                Arc::new(super::probes::DeepCheck { h3: true }) as Arc<dyn DiagnosticCheck>
            }
            "DnsProbe" => {
                Arc::new(super::probes::DeepCheck { h3: false }) as Arc<dyn DiagnosticCheck>
            }
            kind => check(
                definition.id,
                match definition.category {
                    "LocalComponent" => Category::LocalComponent,
                    "PhysicalNetwork" => Category::PhysicalNetwork,
                    "Transport" => Category::Transport,
                    "Tunnel" => Category::Tunnel,
                    "Protection" => Category::Protection,
                    "Recovery" => Category::Recovery,
                    _ => unreachable!("validated diagnostic category"),
                },
                definition.dependencies,
                if definition.mode == "deep" {
                    Mode::Deep
                } else {
                    Mode::Standard
                },
                definition.resource_group,
                passive_kind(kind),
            ),
        })
        .collect()
}

fn passive_kind(name: &str) -> Kind {
    match name {
        "ControlChannel" => Kind::ControlChannel,
        "EventStream" => Kind::EventStream,
        "Capabilities" => Kind::Capabilities,
        "Configuration" => Kind::Configuration,
        "SecureStorage" => Kind::SecureStorage,
        "SocksPort" => Kind::SocksPort,
        "HttpPort" => Kind::HttpPort,
        "SystemProxy" => Kind::SystemProxy,
        "PhysicalNetwork" => Kind::PhysicalNetwork,
        "Ipv4Route" => Kind::Ipv4Route,
        "Ipv6Route" => Kind::Ipv6Route,
        "PhysicalDns" => Kind::PhysicalDns,
        "NetworkGeneration" => Kind::NetworkGeneration,
        "H3Connect" => Kind::H3Connect,
        "H3Datagram" => Kind::H3Datagram,
        "H2Tcp" => Kind::H2Tcp,
        "H2Tls" => Kind::H2Tls,
        "H2Connect" => Kind::H2Connect,
        "EndpointPin" => Kind::EndpointPin,
        "FallbackPolicy" => Kind::FallbackPolicy,
        "AddressAssignment" => Kind::AddressAssignment,
        "TunnelRoutes" => Kind::TunnelRoutes,
        "TunnelDns" => Kind::TunnelDns,
        "FirstPacket" => Kind::FirstPacket,
        "Ipv4Egress" => Kind::Ipv4Egress,
        "Ipv6Egress" => Kind::Ipv6Egress,
        "KillSwitch" => Kind::KillSwitch,
        "DnsPath" => Kind::DnsPath,
        "RouteOwnership" => Kind::RouteOwnership,
        "RecoveryJournal" => Kind::RecoveryJournal,
        "QualityRtt" => Kind::QualityRtt,
        "QualityLoss" => Kind::QualityLoss,
        "QualityQueues" => Kind::QualityQueues,
        "QualityPmtu" => Kind::QualityPmtu,
        "MigrationCapability" => Kind::MigrationCapability,
        "EncryptedDnsConfiguration" => Kind::EncryptedDnsConfiguration,
        "EncryptedDnsRuntime" => Kind::EncryptedDnsRuntime,
        _ => unreachable!("validated diagnostic check kind"),
    }
}

fn check(
    id: &'static str,
    category: Category,
    dependencies: &'static [&'static str],
    minimum_mode: Mode,
    resource_group: &'static str,
    kind: Kind,
) -> Arc<dyn DiagnosticCheck> {
    Arc::new(PassiveCheck::new(
        id,
        category,
        dependencies,
        minimum_mode,
        resource_group,
        kind,
    ))
}
