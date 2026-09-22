use usque_core::chain_exit::ImportError;
use usque_core::chain_exit::store::{ChainProfileStore, ProfileCipher};
use usque_core::chain_exit::{ChainSource, ImportSecrets, ValidatedProfile};
use uuid::Uuid;
use zeroize::Zeroizing;

fn password_profile(extra: &str) -> ImportSecrets {
    ImportSecrets::new(format!(
        "client\ndev tun\nproto udp\nremote 192.0.2.1 5060\nauth-user-pass\n<ca>\n{}\n</ca>\n{extra}",
        include_str!("../../usque-openvpn/tests/fixtures/ca.crt")
    ))
}

#[test]
fn proton_style_password_profile_with_five_ports_is_supported() {
    let text = ImportSecrets::new(include_str!("fixtures/proton_style_password_udp.ovpn").into());
    assert!(ValidatedProfile::parse(ChainSource::OpenvpnCustom, &text).is_ok());
}

struct FixtureCipher;
impl ProfileCipher for FixtureCipher {
    fn seal(&self, _: Uuid, value: &[u8]) -> Result<Vec<u8>, ImportError> {
        Ok(value.to_vec())
    }
    fn open(&self, _: Uuid, value: &[u8]) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        Ok(Zeroizing::new(value.to_vec()))
    }
}

#[test]
fn serialization_expansion_is_rejected_before_creating_a_record() {
    let directory = tempfile::tempdir().unwrap();
    let store = ChainProfileStore::new(directory.path(), &FixtureCipher);
    let text = password_profile(&"\n".repeat(102_400));
    assert!(text.configuration.len() < 128 * 1024);
    assert!(
        store
            .import(ChainSource::OpenvpnCustom, "oversized JSON", text)
            .is_err()
    );
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn oversized_version_one_records_remain_readable_but_cannot_be_rewritten() {
    let directory = tempfile::tempdir().unwrap();
    let store = ChainProfileStore::new(directory.path(), &FixtureCipher);
    let summary = store
        .import(ChainSource::OpenvpnCustom, "Legacy", password_profile(""))
        .unwrap();
    let path = directory
        .path()
        .join("chain-profiles")
        .join(format!("{}.profile", summary.id));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["version"] = 1.into();
    record["summary"]
        .as_object_mut()
        .unwrap()
        .remove("candidates");
    record["summary"]
        .as_object_mut()
        .unwrap()
        .remove("remote_random");
    record["secrets"]["configuration"] = password_profile(&"\n".repeat(102_400))
        .configuration
        .clone()
        .into();
    let historical = serde_json::to_vec(&record).unwrap();
    assert!((192 * 1024..256 * 1024).contains(&historical.len()));
    std::fs::write(&path, &historical).unwrap();
    assert_eq!(store.list().unwrap()[0].id, summary.id);
    assert_eq!(
        store.load(&summary.selection()).unwrap().0.candidates.len(),
        1
    );
    assert!(
        store
            .rename(summary.id, summary.edit_revision, "New name")
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), historical);
    store
        .remove(summary.id, summary.edit_revision, &[])
        .unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn endpoint_and_authentication_policies_preserve_safe_native_inputs() {
    use usque_core::chain_exit::{ClientCertificateMode, MssPolicy};
    let text = password_profile(
        "remote 192.0.2.1 5060\nremote VPN.EXAMPLE 80 udp4\nremote vpn.example 80 udp4\nremote-random\nsetenv CLIENT_CERT 0\nmssfix 0\n",
    );
    let ValidatedProfile::OpenVpn(profile) =
        ValidatedProfile::parse(ChainSource::OpenvpnCustom, &text).unwrap()
    else {
        panic!()
    };
    assert_eq!(profile.candidates.len(), 2);
    assert_eq!(profile.candidates[1].endpoint.host, "vpn.example");
    assert_eq!(profile.candidates[1].ipv6, Some(false));
    assert_eq!(profile.client_certificate, ClientCertificateMode::Disabled);
    assert_eq!(profile.mss, MssPolicy::Disabled);
    assert_eq!(
        profile
            .content
            .lines()
            .filter(|line| line.starts_with("remote "))
            .count(),
        1
    );
    assert!(!profile.content.contains("remote-random"));
    for (directive, reason) in [
        ("remote vpn.example 443 tcp", "conflicting_protocol"),
        ("setenv CLIENT_CERT 1", "conflicting_authentication"),
        ("setenv OTHER 0", "unsupported_directive"),
        ("mssfix 0 mtu", "unsupported_directive"),
        ("mssfix 575", "unsupported_directive"),
        ("remote-random\nremote-random", "duplicate_directive"),
    ] {
        assert_eq!(
            ValidatedProfile::parse(ChainSource::OpenvpnCustom, &password_profile(directive))
                .unwrap_err()
                .reason,
            reason
        );
    }
    let too_many: String = (1..=16)
        .map(|i| format!("remote vpn{i}.example 1194\n"))
        .collect();
    assert_eq!(
        ValidatedProfile::parse(ChainSource::OpenvpnCustom, &password_profile(&too_many))
            .unwrap_err()
            .reason,
        "too_many_endpoints"
    );
    for directive in [
        "mssfix 576",
        "mssfix 1420 mtu",
        "mssfix 1500 fixed",
        "mssfix 65535",
    ] {
        assert!(
            ValidatedProfile::parse(ChainSource::OpenvpnCustom, &password_profile(directive))
                .is_ok()
        );
    }
}

#[test]
fn version_one_ipv6_and_incomplete_auth_records_remain_manageable() {
    for incomplete in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let store = ChainProfileStore::new(directory.path(), &FixtureCipher);
        let mut secrets = password_profile("");
        secrets.configuration = secrets
            .configuration
            .replace("192.0.2.1", "2001:0db8:0000::1");
        let summary = store
            .import(ChainSource::OpenvpnCustom, "Legacy", secrets)
            .unwrap();
        let path = directory
            .path()
            .join("chain-profiles")
            .join(format!("{}.profile", summary.id));
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        record["version"] = 1.into();
        record["summary"]
            .as_object_mut()
            .unwrap()
            .remove("candidates");
        record["summary"]
            .as_object_mut()
            .unwrap()
            .remove("remote_random");
        record["summary"]["endpoint"]["host"] = "2001:0db8:0000::1".into();
        if incomplete {
            let text = record["secrets"]["configuration"]
                .as_str()
                .unwrap()
                .replace("auth-user-pass\n", "");
            record["secrets"]["configuration"] = text.into();
            record["summary"]["requires_auth"] = false.into();
        }
        std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(store.list().unwrap()[0].id, summary.id);
        assert_eq!(store.load(&summary.selection()).is_err(), incomplete);
        store
            .remove(summary.id, summary.edit_revision, &[])
            .unwrap();
    }
}

#[test]
#[ignore = "optional supplied public profile; set USQUE_CHAIN_OVPN_TEST_FILE"]
fn supplied_proton_file_validates_without_rewriting_its_content() {
    let path = std::env::var_os("USQUE_CHAIN_OVPN_TEST_FILE").expect("provide a test input path");
    let text = ImportSecrets::new(std::fs::read_to_string(path).expect("read test input"));
    let profile =
        ValidatedProfile::parse(ChainSource::OpenvpnCustom, &text).expect("valid profile");
    let ValidatedProfile::OpenVpn(profile) = profile else {
        panic!("OpenVPN profile required")
    };
    assert_eq!(profile.candidates.len(), 5);
    assert!(profile.remote_random);
}
