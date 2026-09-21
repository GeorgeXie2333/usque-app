use proptest::prelude::*;
use usque_core::chain_exit::{ChainSource, ImportSecrets, ValidatedProfile};

proptest! {
    #[test]
    fn arbitrary_import_text_and_credentials_never_panic_or_echo_secrets(
        input in prop::collection::vec(any::<char>(), 0..2048),
        credentials in prop::collection::vec(any::<char>(), 0..256),
    ) {
        let sentinel = "CHAIN_IMPORT_PRIVATE_SENTINEL";
        let text: String = input.into_iter().collect();
        let credential: String = credentials.into_iter().collect();
        let mut secrets = ImportSecrets::new(format!("{text}\n{sentinel}"));
        secrets.username = format!("{credential}{sentinel}");
        secrets.password = format!("{credential}{sentinel}");
        secrets.private_key_password = format!("{credential}{sentinel}");
        for source in [ChainSource::OpenvpnCustom, ChainSource::WireguardCustom] {
            let result = ValidatedProfile::parse(source, &secrets);
            let debug = format!("{result:?}");
            prop_assert!(!debug.contains(sentinel));
            if let Err(error) = result {
                prop_assert!(!error.to_string().contains(sentinel));
                prop_assert!(error.to_string().len() < 256);
            }
        }
    }
}
