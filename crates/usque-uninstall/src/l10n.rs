//! Uninstall confirmation copy keyed by Windows UI locale.

include!(concat!(env!("OUT_DIR"), "/setup_strings.rs"));

pub fn setup_locale(name: &str) -> &'static str {
    let normalized = name.replace('_', "-").to_ascii_lowercase();
    if normalized.starts_with("zh-hk") || normalized.starts_with("zh-mo") {
        return "zh-HK";
    }
    if normalized.starts_with("zh-tw") || normalized.starts_with("zh-hant") {
        return "zh-TW";
    }
    match normalized.split('-').next().unwrap_or("") {
        "zh" => "zh-CN",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "de" => "de-DE",
        "es" => "es-ES",
        "fr" => "fr-FR",
        "pt" => "pt-BR",
        "nl" => "nl-NL",
        "it" => "it-IT",
        "pl" => "pl-PL",
        "ru" => "ru-RU",
        "uk" => "uk-UA",
        "tr" => "tr-TR",
        "ar" => "ar-SA",
        "fa" => "fa-IR",
        "id" | "in" => "id-ID",
        "vi" => "vi-VN",
        "th" => "th-TH",
        _ => "en-US",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_regions_and_fallback_remain_distinct() {
        assert_eq!(setup_locale("zh-HK"), "zh-HK");
        assert_eq!(setup_locale("zh-TW"), "zh-TW");
        assert_eq!(setup_locale("zh-Hans-CN"), "zh-CN");
        assert_eq!(setup_locale("pt"), "pt-BR");
        assert_eq!(setup_locale("unknown"), "en-US");
    }

    #[test]
    fn essential_screen_copy_is_embedded_for_english_and_chinese() {
        for locale in ["en-US", "zh-CN"] {
            for key in [
                "uninstall_title",
                "uninstall_body",
                "uninstall_delete_data",
                "uninstall_delete_action",
                "uninstall_partial_data",
                "uninstall_registration_failed",
                "uninstall_reboot",
                "uninstall_permission_denied",
                "uninstall_preview",
            ] {
                assert_ne!(setup_text(locale, key), "Usque", "{locale}: {key}");
            }
        }
    }
}
