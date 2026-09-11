fn main() {
    let target = std::env::var("TARGET").expect("Cargo target");
    let mut build = cmake::Config::new("native");
    // Match Rust's release CRT even for a debug Rust build, as with BoringSSL.
    build.profile("Release");
    if target.contains("msvc") {
        // Keep Ninja's header parser and cl.exe diagnostics in the same
        // language on localized Visual Studio installations.
        build.env("VSLANG", "1033");
        build.define("CMAKE_CL_SHOWINCLUDES_PREFIX", "Note: including file: ");
    }
    build.define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");
    build.define(
        "USQUE_INTEROP_TEST",
        if std::env::var_os("CARGO_FEATURE_INTEROP_TEST").is_some() {
            "ON"
        } else {
            "OFF"
        },
    );
    if target.contains("android") {
        let ndk = std::env::var("ANDROID_NDK_HOME")
            .or_else(|_| std::env::var("ANDROID_NDK_ROOT"))
            .expect("use tool/build_android_rust.ps1 with the pinned NDK");
        build.define(
            "CMAKE_TOOLCHAIN_FILE",
            format!("{ndk}/build/cmake/android.toolchain.cmake"),
        );
        let abi = if target.starts_with("aarch64") {
            "arm64-v8a"
        } else if target.starts_with("armv7") {
            "armeabi-v7a"
        } else {
            "x86_64"
        };
        build.define("ANDROID_ABI", abi);
        build.define("ANDROID_PLATFORM", "android-24");
        build.define("ANDROID_STL", "c++_static");
    }
    let out = build.build();
    println!("cargo:rustc-link-search=native={}/lib", out.display());
    for lib in [
        "usque_openvpn",
        "mbedtls",
        "mbedx509",
        "mbedcrypto",
        "everest",
        "p256m",
    ] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    if target.contains("windows") {
        for lib in ["ws2_32", "bcrypt", "crypt32", "advapi32", "iphlpapi"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    } else if target.contains("android") {
        println!("cargo:rustc-link-lib=c++_static");
        println!("cargo:rustc-link-lib=c++abi");
        println!("cargo:rustc-link-lib=log");
    } else if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=pthread");
    }
    println!("cargo:rerun-if-changed=native");
    for path in [
        "openvpn3-3.11.7",
        "mbedtls-3.6.7",
        "asio-1.30.2",
        "lz4-1.10.0",
        "xxhash-0.8.3",
    ] {
        println!("cargo:rerun-if-changed=../../third_party/{path}");
    }
}
