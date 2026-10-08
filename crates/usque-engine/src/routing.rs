use usque_core::{RoutingAction, RoutingMatch, RoutingRule, RoutingSettings};
use usque_ipc::v1;

use crate::ControlServiceError;

pub(crate) fn from_proto(
    source: Option<v1::RoutingSettings>,
) -> Result<RoutingSettings, ControlServiceError> {
    let Some(source) = source else {
        return Ok(RoutingSettings::default());
    };
    let invalid = || ControlServiceError::InvalidConfiguration("ROUTING_RULE_INVALID".into());
    let rules = source
        .rules
        .into_iter()
        .map(|rule| {
            Ok(RoutingRule {
                id: rule.id.parse().map_err(|_| invalid())?,
                kind: match rule.kind.as_str() {
                    "domain" => RoutingMatch::Domain,
                    "cidr" => RoutingMatch::Cidr,
                    _ => return Err(invalid()),
                },
                target: rule.target,
                action: match rule.action.as_str() {
                    "direct" => RoutingAction::Direct,
                    "reject" => RoutingAction::Reject,
                    "proxy" => RoutingAction::Proxy,
                    _ => return Err(invalid()),
                },
            })
        })
        .collect::<Result<Vec<_>, ControlServiceError>>()?;
    RoutingSettings {
        rules,
        ads_enabled: source.ads_enabled,
    }
    .normalized()
    .map_err(ControlServiceError::configuration)
}

pub(crate) fn to_proto(source: &RoutingSettings) -> v1::RoutingSettings {
    v1::RoutingSettings {
        rules: source
            .rules
            .iter()
            .map(|rule| v1::RoutingRule {
                id: rule.id.to_string(),
                kind: match rule.kind {
                    RoutingMatch::Domain => "domain",
                    RoutingMatch::Cidr => "cidr",
                }
                .into(),
                target: rule.target.clone(),
                action: match rule.action {
                    RoutingAction::Direct => "direct",
                    RoutingAction::Reject => "reject",
                    RoutingAction::Proxy => "proxy",
                }
                .into(),
            })
            .collect(),
        ads_enabled: source.ads_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_unknown_action() {
        let wire = v1::RoutingSettings {
            rules: vec![v1::RoutingRule {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "domain".into(),
                target: "ads.test".into(),
                action: "reject".into(),
            }],
            ads_enabled: true,
        };
        let settings = from_proto(Some(wire.clone())).unwrap();
        assert_eq!(to_proto(&settings), wire);
        let mut bad = wire;
        bad.rules[0].action = "unexpected".into();
        assert!(from_proto(Some(bad)).is_err());
    }
}
