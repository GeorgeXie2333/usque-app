use std::env;

use winresource::{VersionInfo, WindowsResource};

const STABLE_ORDINAL: u64 = 99;

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let (packed_version, display_version, prerelease) = mapped_windows_version();
    let mut resource = WindowsResource::new();
    resource
        .set_language(0x0409)
        .set("CompanyName", "Usque contributors")
        .set("FileDescription", "Usque privileged network state agent")
        .set("FileVersion", &display_version)
        .set("InternalName", "usque-agent")
        .set("LegalCopyright", "Copyright (C) 2026 Usque contributors")
        .set("OriginalFilename", "usque-agent.exe")
        .set("ProductName", "Usque")
        .set("ProductVersion", &display_version)
        .set_version_info(VersionInfo::FILEVERSION, packed_version)
        .set_version_info(VersionInfo::PRODUCTVERSION, packed_version);
    if prerelease {
        resource.set_version_info(VersionInfo::FILEFLAGS, VersionInfo::VS_FF_PRERELEASE);
    }
    resource
        .compile()
        .expect("compile the Windows Agent version resource");
}

fn mapped_windows_version() -> (u64, String, bool) {
    let major = version_number("CARGO_PKG_VERSION_MAJOR");
    let minor = version_number("CARGO_PKG_VERSION_MINOR");
    let patch = version_number("CARGO_PKG_VERSION_PATCH");
    assert!(
        major <= 255 && minor <= 255,
        "Windows MSI major and minor versions must be at most 255"
    );

    let prerelease = env::var("CARGO_PKG_VERSION_PRE").unwrap_or_default();
    let ordinal = if prerelease.is_empty() {
        STABLE_ORDINAL
    } else {
        prerelease
            .strip_prefix("beta.")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| (1..=99).contains(value))
            .unwrap_or_else(|| {
                panic!(
                    "Windows Agent versions support only beta ordinals between 1 and 99: {prerelease}"
                )
            })
    };
    let mapped_build = patch
        .checked_mul(100)
        .and_then(|value| value.checked_add(ordinal))
        .filter(|value| *value <= u16::MAX as u64)
        .expect("mapped Windows Agent build version must fit in 16 bits");
    let packed = (major << 48) | (minor << 32) | (mapped_build << 16);
    (
        packed,
        format!("{major}.{minor}.{mapped_build}.0"),
        !prerelease.is_empty(),
    )
}

fn version_number(name: &str) -> u64 {
    env::var(name)
        .unwrap_or_else(|_| panic!("Cargo did not provide {name}"))
        .parse()
        .unwrap_or_else(|_| panic!("Cargo provided an invalid {name}"))
}
