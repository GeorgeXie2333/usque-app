use super::*;

#[test]
fn device_endpoints_use_host_or_address_and_the_returned_port() {
    for (value, host, port) in [
        (
            json!({"v4": "162.159.195.23:0", "ports": [4500, 2408]}),
            "162.159.195.23",
            4500,
        ),
        (
            json!({"v6": "[2606:4700:d0::1]:0"}),
            "2606:4700:d0::1",
            2408,
        ),
        (
            json!({"host": "engage.cloudflareclient.com:500", "v4": "162.159.192.1"}),
            "engage.cloudflareclient.com",
            500,
        ),
        (
            json!({"v4": "162.159.195.23", "ports": [4500, 2408]}),
            "162.159.195.23",
            4500,
        ),
        (
            json!({"v4": "", "v6": "2606:4700:d0::1", "ports": [1701]}),
            "2606:4700:d0::1",
            1701,
        ),
        (json!({"v4": "188.114.99.7:2408"}), "188.114.99.7", 2408),
    ] {
        assert_eq!(
            registered_endpoint(&value).unwrap(),
            Endpoint {
                host: host.into(),
                port
            }
        );
    }
    for value in [
        json!({}),
        json!({"v4": "127.0.0.1"}),
        json!({"v4": "162.159.192.1", "ports": [65536]}),
    ] {
        assert_eq!(
            registered_endpoint(&value).unwrap_err().reason,
            "registration_response_invalid"
        );
    }
}

struct RegistrationPeer {
    delay: Duration,
    reject: Option<(Method, InternalHttpError)>,
    malformed: bool,
    methods: Mutex<Vec<Method>>,
    keys: Mutex<Vec<String>>,
}
impl RegistrationPeer {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            reject: None,
            malformed: false,
            methods: Mutex::new(vec![]),
            keys: Mutex::new(vec![]),
        }
    }
}
#[async_trait::async_trait]
impl RegistrationHttp for RegistrationPeer {
    async fn request(
        &self,
        request: InternalRequest<'_>,
        _: &CancellationToken,
    ) -> Result<Vec<u8>, InternalHttpError> {
        self.methods.lock().unwrap().push(request.method.clone());
        assert_eq!(request.limit, 64 * 1024);
        assert!(
            request
                .headers
                .contains(&("CF-Client-Version", "a-6.38.9-5641"))
        );
        assert!(
            request
                .headers
                .contains(&("User-Agent", "1.1.1.1/6.38.9-5641 (Android 16.0.0)"))
        );
        if request.method == Method::POST {
            assert_eq!(request.url, "https://api.cloudflareclient.com/v0a5641/reg");
            let value: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(value["key_type"], "curve25519");
            assert_eq!(value["tunnel_type"], "wireguard");
            assert_eq!(value["os_version"], "16.0.0");
            assert_eq!(value["serial_number"], "");
            assert_eq!(value["model"], "PC");
            assert!(value["tos"].as_str().is_some());
            let public = value["key"].as_str().unwrap();
            assert_eq!(STANDARD.decode(public).unwrap().len(), 32);
            self.keys.lock().unwrap().push(public.into());
            assert!(
                !request
                    .headers
                    .iter()
                    .any(|(name, _)| *name == "Authorization")
            );
        } else {
            assert_eq!(request.method, Method::GET);
            assert_eq!(
                request.url,
                "https://api.cloudflareclient.com/v0a5641/reg/device-1"
            );
            assert!(
                request
                    .headers
                    .contains(&("Authorization", "Bearer fixture-token"))
            );
            assert!(request.body.is_empty());
        }
        tokio::time::sleep(self.delay).await;
        if let Some((method, failure)) = &self.reject
            && *method == request.method
        {
            return Err(*failure);
        }
        if self.malformed && request.method == Method::GET {
            return Ok(br#"{"id":"device-1","token":"must-not-escape"}"#.to_vec());
        }
        if request.method == Method::POST {
            return Ok(br#"{"id":"device-1","token":"fixture-token"}"#.to_vec());
        }
        Ok(serde_json::to_vec(&json!({
            "id": "device-1", "token": "fixture-token", "config": {
                "interface": {"addresses": {"v4": "172.16.0.2", "v6": "2606:4700:110::2"}},
                "peers": [{"public_key": STANDARD.encode([4; 32]), "endpoint": {"v4": "162.159.192.99:0", "ports": [4500, 2408]}}]
            }
        })).unwrap())
    }
}

#[tokio::test(start_paused = true)]
async fn slow_mobile_enrollment_has_its_own_budget_and_returns_matching_fresh_keys() {
    let peer = RegistrationPeer::new(Duration::from_secs(6));
    for index in 0..2 {
        let secrets = register_with(&peer, &CancellationToken::new())
            .await
            .unwrap();
        assert!(!secrets.configuration.contains("fixture-token"));
        let ValidatedProfile::WireGuard(profile) =
            ValidatedProfile::parse(ChainSource::WarpWireguard, &secrets).unwrap()
        else {
            panic!("WireGuard configuration")
        };
        let public = PublicKey::from(&StaticSecret::from(*profile.private_key));
        assert_eq!(
            STANDARD.encode(public.as_bytes()),
            peer.keys.lock().unwrap()[index]
        );
        assert_eq!(profile.endpoint.host, "162.159.192.99");
        assert_eq!(profile.endpoint.port, 4500);
        assert_eq!(profile.addresses.len(), 2);
    }
    let keys = peer.keys.lock().unwrap();
    assert_ne!(keys[0], keys[1]);
    assert_eq!(peer.methods.lock().unwrap().len(), 4);
}

#[tokio::test(start_paused = true)]
async fn enrollment_errors_retain_stage_and_status_without_tokens_or_retries() {
    for (method, failure, expected, calls) in [
        (
            Method::POST,
            InternalHttpError::HttpStatus(429),
            "registration_create_http_429",
            1,
        ),
        (
            Method::GET,
            InternalHttpError::HttpStatus(403),
            "registration_device_http_403",
            2,
        ),
        (
            Method::POST,
            InternalHttpError::Tls,
            "registration_create_tls",
            1,
        ),
        (
            Method::POST,
            InternalHttpError::Dns,
            "registration_create_dns",
            1,
        ),
    ] {
        let mut peer = RegistrationPeer::new(Duration::ZERO);
        peer.reject = Some((method, failure));
        let error = register_with(&peer, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.reason, expected);
        assert_eq!(peer.methods.lock().unwrap().len(), calls);
        assert!(!error.to_string().contains("fixture-token"));
    }
    let mut peer = RegistrationPeer::new(Duration::ZERO);
    peer.malformed = true;
    let error = register_with(&peer, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.reason, "registration_response_invalid");
    assert!(!error.to_string().contains("must-not-escape"));
    assert_eq!(peer.methods.lock().unwrap().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn enrollment_timeout_and_cancellation_remain_bounded() {
    let peer = RegistrationPeer::new(Duration::from_secs(30));
    let started = tokio::time::Instant::now();
    let error = register_with(&peer, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.reason, "registration_create_timeout");
    assert_eq!(started.elapsed(), Duration::from_secs(15));
    let cancel = CancellationToken::new();
    let started = tokio::time::Instant::now();
    let (result, ()) = tokio::join!(register_with(&peer, &cancel), async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    });
    assert_eq!(result.unwrap_err().reason, "registration_create_cancelled");
    assert_eq!(started.elapsed(), Duration::from_millis(20));
}
