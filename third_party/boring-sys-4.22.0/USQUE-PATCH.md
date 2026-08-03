# Usque patch notes

This directory contains the crates.io `boring-sys` 4.22.0 source published
from upstream commit `467cb51c987fbb0041b2f31bd56b697a956e5cea`.

Usque changes one Android cross-compilation decision in `build/main.rs`.
Upstream passes the API-unqualified Rust target (for example
`aarch64-linux-android`) through `CMAKE_C_COMPILER_TARGET`. That value
overrides the Android NDK toolchain's API-qualified target and prevents Clang
from locating Android CRT objects on Windows hosts.

The patch lets the NDK CMake toolchain own Android cross-compilation and
defines the three CMake language flag variables so `cmake-rs` cannot copy the
same API-unqualified target back from `cc-rs`. It also keeps target-specific
`CC` and `CXX` variables (needed by non-CMake dependencies such as `ring`)
from replacing the compiler selected by the Android CMake toolchain. No
BoringSSL source, bindings, public API, or Cargo feature is changed.
