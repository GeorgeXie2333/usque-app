use std::net::IpAddr;

use usque_geo::{
    CountryCode, GeoClassifier, GeoError, GeoIpSet, GeoSiteSet, geoip_cache_path,
    geosite_cache_path, list_cached_countries,
};

const GEOIP_CN: &[u8] = include_bytes!("fixtures/geoip-cn.dat");
const GEOSITE_CN: &str = include_str!("fixtures/geosite-cn.txt");

fn cn() -> CountryCode {
    CountryCode::parse("CN").unwrap()
}

#[test]
fn country_code_rejects_invalid_values() {
    assert!(CountryCode::parse("CN").is_ok());
    assert!(CountryCode::parse("cn").is_ok());
    assert!(matches!(
        CountryCode::parse("china"),
        Err(GeoError::InvalidCountryCode(_))
    ));
    assert!(CountryCode::parse("C").is_err());
}

#[test]
fn geoip_fixture_hits_and_misses() {
    let set = GeoIpSet::from_v2ray_dat(GEOIP_CN, &cn()).unwrap();
    assert!(set.contains("1.2.3.1".parse::<IpAddr>().unwrap()));
    assert!(!set.contains("1.2.4.1".parse::<IpAddr>().unwrap()));
    assert!(set.contains("2001:db8::1".parse::<IpAddr>().unwrap()));
    assert!(!set.contains("2001:db9::1".parse::<IpAddr>().unwrap()));
}

#[test]
fn truncated_geoip_dat_fails_closed() {
    assert!(matches!(
        GeoIpSet::from_v2ray_dat(&GEOIP_CN[..10], &cn()),
        Err(GeoError::InvalidGeoIp)
    ));
}

#[test]
fn geosite_fixture_matches_suffix_and_full() {
    let set = GeoSiteSet::from_text(GEOSITE_CN, &cn()).unwrap();
    assert!(set.contains("example.cn"));
    assert!(set.contains("www.example.cn"));
    assert!(!set.contains("example.cn.evil"));
    assert!(set.contains("baidu.com"));
    assert!(!set.contains("www.baidu.com"));
}

#[test]
fn load_requires_cached_geoip() {
    let directory = tempfile::tempdir().unwrap();
    let error = GeoClassifier::load(directory.path(), &[cn()]).unwrap_err();
    assert!(matches!(error, GeoError::MissingArtifact { .. }));
}

#[test]
fn load_and_list_from_cache_layout() {
    let directory = tempfile::tempdir().unwrap();
    let cn = cn();
    std::fs::create_dir_all(geoip_cache_path(directory.path(), &cn).parent().unwrap()).unwrap();
    std::fs::create_dir_all(geosite_cache_path(directory.path(), &cn).parent().unwrap()).unwrap();
    std::fs::write(geoip_cache_path(directory.path(), &cn), GEOIP_CN).unwrap();
    std::fs::write(geosite_cache_path(directory.path(), &cn), GEOSITE_CN).unwrap();

    let listed = list_cached_countries(directory.path()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].country, cn);
    assert!(listed[0].geoip);
    assert!(listed[0].geosite);

    let classifier = GeoClassifier::load(directory.path(), std::slice::from_ref(&cn)).unwrap();
    assert!(classifier.ip_matches("1.2.3.9".parse().unwrap(), &cn));
    assert!(!classifier.ip_matches("9.9.9.9".parse().unwrap(), &cn));
    assert_eq!(classifier.lookup_ip("1.2.3.9".parse().unwrap()), Some(&cn));
    assert_eq!(classifier.lookup_ip("9.9.9.9".parse().unwrap()), None);
    assert!(classifier.host_matches("foo.example.cn", &cn));
    assert!(!classifier.host_matches("google.com", &cn));
}
