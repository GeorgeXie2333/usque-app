use super::*;

fn ovpn(extra: &str) -> ImportSecrets {
    let auth = if extra.contains("auth-user-pass") {
        ""
    } else {
        "auth-user-pass\n"
    };
    ImportSecrets::new(format!(
        "client\ndev tun\nproto tcp-client\nremote vpn.example.org 443\n<ca>\nTEST\n</ca>\n{auth}{extra}"
    ))
}
fn wg(extra: &str) -> ImportSecrets {
    ImportSecrets::new(format!(
        "[Interface]\nPrivateKey = AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=\nAddress = 10.8.0.2/32, fd00::2/128\n[Peer]\nPublicKey = AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=\nEndpoint = vpn.example.org:51820\nAllowedIPs = 10.8.0.0/24, fd00::/64\n{extra}"
    ))
}
#[test]
fn labels_and_protocol_capabilities_are_explicit() {
    assert_eq!(
        ChainSource::DISPLAY_ORDER.map(ChainSource::label),
        ["OpenVPN", "WireGuard", "VPN Gate"]
    );
    assert!(!ChainProtocol::OpenvpnTcp.requires_udp());
    assert!(ChainProtocol::OpenvpnUdp.requires_udp());
    assert!(ChainProtocol::Wireguard.requires_udp());
}
#[test]
fn imported_openvpn_preserves_udp_and_certificate_constraints() {
    let mut input = ovpn("auth-user-pass\nverify-x509-name server name\n");
    input.configuration = input.configuration.replace("tcp-client", "udp");
    let ValidatedProfile::OpenVpn(p) =
        ValidatedProfile::parse(ChainSource::OpenvpnCustom, &input).unwrap()
    else {
        panic!()
    };
    assert_eq!(p.protocol, ChainProtocol::OpenvpnUdp);
    assert!(p.requires_auth);
    assert!(p.content.contains("verify-x509-name server name"));
    assert!(p.content.contains("tls-version-min 1.2"));
    assert!(p.content.contains("remote-cert-tls server"));
}
#[test]
fn imported_openvpn_rejects_execution_files_and_unsafe_directives() {
    for directive in [
        "up secret-script",
        "plugin secret-plugin",
        "ca secret.pem",
        "auth-user-pass secret.txt",
        "compress lz4",
        "tls-version-min 1.0",
        "remote-cert-tls client",
    ] {
        let error =
            ValidatedProfile::parse(ChainSource::OpenvpnCustom, &ovpn(directive)).unwrap_err();
        assert!(!error.to_string().contains("secret"));
    }
}

#[test]
fn protocol_family_and_conflicting_remote_options_are_not_lost() {
    for (name, v6) in [("tcp4", false), ("tcp6-client", true), ("udp6", true)] {
        let mut input = ovpn("");
        input.configuration = input.configuration.replace("tcp-client", name);
        let ValidatedProfile::OpenVpn(p) =
            ValidatedProfile::parse(ChainSource::OpenvpnCustom, &input).unwrap()
        else {
            panic!()
        };
        assert_eq!(p.endpoint_ipv6, Some(v6));
    }
    for text in [
        "proto tcp\nremote vpn.example 443 udp",
        "remote vpn.example 443 udp\nproto tcp",
    ] {
        let input = ImportSecrets::new(format!(
            "client\ndev tun\nauth-user-pass\n<ca>\nTEST\n</ca>\n{text}"
        ));
        assert_eq!(
            ValidatedProfile::parse(ChainSource::OpenvpnCustom, &input)
                .unwrap_err()
                .reason,
            "conflicting_protocol"
        );
    }
    let too_large = ImportSecrets::new("x".repeat(MAX_CONFIG_BYTES + 1));
    assert!(ValidatedProfile::parse(ChainSource::OpenvpnCustom, &too_large).is_err());
}

#[test]
fn schema_16_retains_gate_selection_backups_and_unrelated_favorites() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::storage::ConfigStore::new(directory.path().join("config.json"));
    let mut legacy = serde_json::to_value(crate::AppConfig::default()).unwrap();
    legacy["schema_version"] = 16.into();
    legacy["network"]["vpn_gate"] = serde_json::json!({"enabled":true,"selection":{"server_id":format!("v1:{}", "cd".repeat(32)),"config_sha256":"ab".repeat(32)}});
    let bytes = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(store.path(), &bytes).unwrap();
    let favorites = directory.path().join("favorites.json");
    std::fs::write(&favorites, b"unchanged-fixture").unwrap();
    let migrated = store.load().unwrap();
    assert_eq!(migrated.schema_version, 17);
    let chain = migrated.network.chain_exit.unwrap();
    assert!(chain.enabled);
    assert_eq!(chain.source, ChainSource::VpnGate);
    assert_eq!(
        serde_json::to_value(migrated.network.vpn_gate).unwrap(),
        legacy["network"]["vpn_gate"]
    );
    assert_eq!(std::fs::read(store.backup_path()).unwrap(), bytes);
    assert_eq!(std::fs::read(favorites).unwrap(), b"unchanged-fixture");
    legacy["network"]["vpn_gate"]["selection"]["config_sha256"] = "invalid".into();
    let bad = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(store.path(), &bad).unwrap();
    assert!(store.load().is_err());
    assert_eq!(std::fs::read(store.path()).unwrap(), bad);
}

#[test]
fn imported_library_is_shared_and_legacy_writers_cannot_clear_its_selection() {
    let mut config = crate::AppConfig::default();
    let id = Uuid::new_v4();
    config.insert_account(id, "first".into(), None).unwrap();
    let mut next = config.runtime_profile(id).unwrap();
    next.chain_exit = Some(ChainExitSettings {
        enabled: true,
        source: ChainSource::WireguardCustom,
        profile_id: Some(Uuid::new_v4()),
        revision: Some(Uuid::new_v4()),
    });
    let saved = config.upsert_runtime_profile(next).unwrap();
    let other = Uuid::new_v4();
    config.insert_account(other, "second".into(), None).unwrap();
    assert_eq!(
        config.runtime_profile(other).unwrap().chain_exit,
        saved.chain_exit
    );
    let mut old = saved.clone();
    old.chain_exit = None;
    assert!(config.upsert_runtime_profile(old.clone()).is_err());
    let patch = crate::network_settings::NetworkSettingsPatch {
        operation_id: Uuid::new_v4(),
        account_id: id,
        values: old,
        changed_fields: vec!["vpn_gate".into()],
    };
    assert!(crate::network_settings::merge_patch(&mut config, &patch).is_err());
    assert_eq!(
        config.runtime_profile(id).unwrap().chain_exit,
        saved.chain_exit
    );
}

#[test]
fn l4_saves_udp_imports_but_only_tcp_can_be_enabled() {
    let directory = tempfile::tempdir().unwrap();
    let store = store::ChainProfileStore::new(directory.path(), &TestCipher);
    for (source, input, tcp) in [
        (ChainSource::OpenvpnCustom, ovpn(""), true),
        (
            ChainSource::OpenvpnCustom,
            ovpn("").configuration.replace("tcp-client", "udp").into(),
            false,
        ),
        (ChainSource::WireguardCustom, wg(""), false),
    ] {
        let summary = store.import(source, "fixture", input).unwrap();
        let mut profile = crate::Profile {
            data_plane: crate::DataPlaneMode::L4Proxy,
            chain_exit: Some(summary.selection()),
            ..Default::default()
        };
        assert_eq!(
            prepare_selection(directory.path(), &profile, &TestCipher).is_ok(),
            tcp
        );
        profile.disable_chain();
        assert!(
            prepare_selection(directory.path(), &profile, &TestCipher)
                .unwrap()
                .is_none()
        );
    }
    store::clear_library(directory.path()).unwrap();
    assert!(store.list().unwrap().is_empty());
}
#[test]
fn wireguard_restricts_both_address_families_and_redacts_secrets() {
    let input = wg("PersistentKeepalive = 25");
    let ValidatedProfile::WireGuard(p) =
        ValidatedProfile::parse(ChainSource::WireguardCustom, &input).unwrap()
    else {
        panic!()
    };
    assert_eq!(p.keepalive, Some(25));
    assert_eq!(p.mtu, 1280);
    assert!(p.allows("10.8.0.3".parse().unwrap()));
    assert!(p.allows("fd00::3".parse().unwrap()));
    assert!(!p.allows("1.1.1.1".parse().unwrap()));
    assert!(!format!("{input:?}").contains("AQEBA"));
}
#[test]
fn wireguard_rejects_hooks_multiple_peers_and_invalid_keys() {
    for input in [
        wg("PostUp = secret-command"),
        wg("[Peer]\nPublicKey = secret"),
        wg("AllowedIPs = 0.0.0.0/0"),
        wg("")
            .configuration
            .replace("AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=", "secret")
            .into(),
    ] {
        let error = ValidatedProfile::parse(ChainSource::WireguardCustom, &input).unwrap_err();
        assert!(!error.to_string().contains("secret"));
    }
}
impl From<String> for ImportSecrets {
    fn from(configuration: String) -> Self {
        Self::new(configuration)
    }
}

#[cfg(windows)]
#[test]
fn current_user_dpapi_round_trips_and_rejects_a_different_object_identity() {
    use store::ProfileCipher;
    let cipher = store::WindowsProfileCipher;
    let id = Uuid::new_v4();
    let ciphertext = cipher.seal(id, b"synthetic-private-fixture").unwrap();
    assert!(!ciphertext.windows(7).any(|w| w == b"private"));
    assert_eq!(
        &*cipher.open(id, &ciphertext).unwrap(),
        b"synthetic-private-fixture"
    );
    assert!(cipher.open(Uuid::new_v4(), &ciphertext).is_err());
}

// Deterministic codec double: checks the store's binding/transaction contract,
// not the security of Windows DPAPI or Android Keystore.
struct TestCipher;
impl store::ProfileCipher for TestCipher {
    fn seal(&self, id: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        use sha2::{Digest, Sha256};
        let mut output = id.as_bytes().to_vec();
        output.extend_from_slice(&Sha256::digest(value));
        output.extend(value.iter().map(|byte| byte ^ 0xa5));
        Ok(output)
    }
    fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        use sha2::{Digest, Sha256};
        if value.len() < 48 || &value[..16] != id.as_bytes() {
            return Err(ImportError::new(0, "storage", "invalid_ciphertext"));
        }
        let plaintext = Zeroizing::new(value[48..].iter().map(|b| b ^ 0xa5).collect::<Vec<_>>());
        if Sha256::digest(&plaintext)[..] != value[16..48] {
            return Err(ImportError::new(0, "storage", "invalid_ciphertext"));
        }
        Ok(plaintext)
    }
}

#[test]
fn encrypted_store_retains_selection_and_rejects_stale_mutations_and_tampering() {
    let directory = tempfile::tempdir().unwrap();
    let store = store::ChainProfileStore::new(directory.path(), &TestCipher);
    let mut secrets = ovpn("auth-user-pass");
    secrets.username = "fixture-user".into();
    secrets.password = "private-password-sentinel".into();
    let imported = store
        .import(ChainSource::OpenvpnCustom, "My exit", secrets)
        .unwrap();
    let path = directory
        .path()
        .join("chain-profiles")
        .join(format!("{}.profile", imported.id));
    let ciphertext = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&ciphertext).contains("private-password-sentinel"));
    assert!(
        store
            .remove(imported.id, imported.edit_revision, &[imported.id])
            .is_err()
    );
    let renamed = store
        .rename(imported.id, imported.edit_revision, "Updated exit")
        .unwrap();
    assert_ne!(renamed.edit_revision, imported.edit_revision);
    assert_eq!(renamed.revision, imported.revision);
    assert!(
        store
            .rename(imported.id, imported.edit_revision, "Stale")
            .is_err()
    );
    let mut credentials = ImportSecrets::default();
    credentials.username = "user-2".into();
    credentials.password = "new-sentinel".into();
    let updated = store
        .update_credentials(imported.id, renamed.edit_revision, credentials)
        .unwrap();
    let (_, _, current) = store.load(&imported.selection()).unwrap();
    assert_eq!(current.password, "new-sentinel");
    assert!(
        store
            .remove(imported.id, renamed.edit_revision, &[])
            .is_err()
    );
    let mut damaged = std::fs::read(&path).unwrap();
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    std::fs::write(&path, damaged).unwrap();
    assert!(store.load(&updated.selection()).is_err());
    assert!(
        store
            .rename(imported.id, updated.edit_revision, "Lost")
            .is_err()
    );
}

#[test]
fn aborted_cipher_write_preserves_previous_object_and_collects_only_orphan_temps() {
    struct FailingCipher;
    impl store::ProfileCipher for FailingCipher {
        fn seal(&self, _: Uuid, _: &[u8]) -> Result<Vec<u8>, ImportError> {
            Err(ImportError::new(0, "storage", "write_failed"))
        }
        fn open(&self, id: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
            store::ProfileCipher::open(&TestCipher, id, value)
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let store = store::ChainProfileStore::new(directory.path(), &TestCipher);
    let profile = store
        .import(ChainSource::OpenvpnCustom, "Original", ovpn(""))
        .unwrap();
    assert!(
        store::ChainProfileStore::new(directory.path(), &FailingCipher)
            .rename(profile.id, profile.edit_revision, "Failed")
            .is_err()
    );
    let orphan = directory.path().join("chain-profiles/.chain-orphan");
    std::fs::write(&orphan, b"encrypted orphan").unwrap();
    let other = directory.path().join("chain-profiles/keep.txt");
    std::fs::write(&other, b"unrelated").unwrap();
    assert_eq!(store.list().unwrap()[0].name, "Original");
    assert!(!orphan.exists());
    assert!(other.exists());
}
