use std::sync::{Arc, Mutex};
use usque_core::{EndpointPin, IdentityProvider, MasqueKeyPair, WarpIdentity};
use usque_macos_runtime::{SessionKey, credential_rpc::*, credentials::CredentialBroker};
use uuid::Uuid;

fn identity(token: &str) -> WarpIdentity {
    let key = MasqueKeyPair::generate();
    let pin = EndpointPin::from_spki_der(&key.public_spki_der().unwrap()).unwrap();
    WarpIdentity::from_secure_records(
        key,
        pin,
        "device".into(),
        token.into(),
        None,
        IdentityProvider::Consumer,
        None,
        "172.16.0.2".parse().unwrap(),
        "2606:4700:110:8888::2".parse().unwrap(),
    )
    .unwrap()
}
fn key() -> SessionKey {
    SessionKey {
        session_id: Uuid::new_v4(),
        config_revision: 4,
        network_generation: 9,
    }
}
struct Transport {
    corrupt: bool,
    requests: Mutex<Vec<CredentialRequest>>,
}
impl CredentialTransport for Transport {
    fn exchange(&self, request: &CredentialRequest) -> Result<CredentialResponse, String> {
        request.validate_digest()?;
        self.requests.lock().unwrap().push(request.clone());
        Ok(CredentialResponse {
            callback_id: request.callback_id,
            broker: BrokerResponse {
                accepted: true,
                session: Some(request.broker.session().clone()),
                profile_id: Some(request.broker.profile_id()),
                revision: Some(Uuid::new_v4()),
                request_digest: Some(if self.corrupt {
                    "wrong".into()
                } else {
                    request.request_digest.clone()
                }),
                ..Default::default()
            },
        })
    }
}
#[tokio::test]
async fn rpc_ack_binds_to_exact_secret_payload_and_session_before_commit() {
    for corrupt in [false, true] {
        let transport = Arc::new(Transport {
            corrupt,
            requests: Mutex::new(vec![]),
        });
        let broker = RpcCredentialBroker::new(transport.clone());
        let session = key();
        let profile = Uuid::new_v4();
        let previous = identity("previous-private-token");
        let next = identity("next-private-token");
        let result = broker
            .persist_pin(&session, profile, &previous, &next)
            .await;
        assert_eq!(result.is_err(), corrupt);
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!format!("{:?}", requests[0]).contains("private-token"));
        let mut mutated = requests[0].clone();
        if let BrokerRequest::PersistPin { next_identity, .. } = &mut mutated.broker {
            next_identity.push(' ');
        }
        assert!(mutated.validate_digest().is_err());
    }
}
