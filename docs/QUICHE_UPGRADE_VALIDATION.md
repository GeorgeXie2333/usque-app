# quiche 0.30.1 migration validation

Local validation on 2026-11-12 used the `dev` working tree based on
`0451d50de7e4eec9ff5e79f43f3571e965f65d84`, plus this upgrade. The working tree
was not committed for these checks, so this record does not identify an immutable
release candidate. It does not establish results for later source changes.

## Source and compatibility

The published 0.30.1 archive was verified against crates.io SHA-256
`8ba5af7b73277657f07f47615a3440f6920bf1b747fecd7ca5e9957ff77c5b98`.
The original 0.29.3 archive was also checksum-verified before deriving the local
patch. All 93 new upstream files were extracted before applying that patch.
The retained patches and removal criteria are in
[USQUE-PATCH.md](../third_party/quiche-0.30.1/USQUE-PATCH.md).

- Retain per-path PMTUD, DATAGRAM admission limits, explicit local probe
  rejection, independent BBRv3 and separate PTO/loss-time callback accounting.
- Resolve upstream's new unchecked-controller enum value 5 by assigning the
  local BBRv3 Rust/C value 6. External C consumers of the old local enum need
  recompilation. Application configuration names and protobuf values are stable.
- Preserve upstream's early-data handshake callback fix when retaining PMTUD
  settings for future paths. Forward BBRv2's new persistent RTT-jump counter;
  BBRv3 has no corresponding detector and reports zero.
- Keep Boring 4.22.0 and the existing TLS fingerprint/verification configuration.
  Keep libc 0.2.191 and rustix 1.1.6 by replacing the published crate's temporary
  iOS-only dependency workaround with a libc 0.2.191 minimum. This avoids
  reintroducing [RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486.html).
- Accept the upstream 22,000 tracked-sent-packet limit and its fail-closed
  connection behavior; do not enable the unchecked congestion controller.

Byte comparisons after normalizing checkout line endings confirmed that the
existing BBRv3 implementation, BBRv3 tests, PTO accounting tests and BBRv3 license
were preserved. BBRv2, Reno and CUBIC algorithm files match upstream 0.30.1.

The tested root lockfile SHA-256 is
`b3d631cb3df346f84cf2a1c554c3f2c805825cca05312d1004a76e735349bba8`;
the standalone quiche lockfile SHA-256 is
`b4885dd84ab939c7c13cb99a91799847dd55bae0ece66cbaf82233d49128274c`.

## Workstation checks

Commands start at the repository root unless noted. Rust was the pinned
1.97.1 toolchain. Python commands used the verified Python 3.12.14 executable
from the local Codex runtime because the global `python` command is an app alias.
No test disabled certificate verification or weakened an isolation check.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | Passed |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | 2,339 passed; 13 existing opt-in tests ignored; 0 failures |
| `& ./tool/build_windows_rust_release.ps1 -Variant x64-v2` | Release compile passed |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | Passed |
| `& ./tool/build_android_rust.ps1 -CargoAction build -AbiFilter all` | Debug JNI builds passed for arm64-v8a, armeabi-v7a and x86_64 |
| `go install github.com/rhysd/actionlint/cmd/actionlint@914e7df21a07ef503a81201c76d2b11c789d3fca` | Pinned actionlint 1.7.12 restored |
| `actionlint -no-color` | Passed |
| `python tool/check_repository_policy.py` | Passed |
| `git diff --check` | Passed |

An initial sandboxed Windows Clippy attempt could not resolve the dependency
download host. The same helper command completed successfully after enabling
the required network/cache access; this was not counted as a source failure.

The standalone suite used the Windows helper's selected native environment,
MSVC/SDK include forwarding in `BINDGEN_EXTRA_CLANG_ARGS`, and an isolated build
directory under `target`. It used the registry Boring package, matching the
independent CI suite, with the documented Windows dev-CRT overrides:

| Command | Result |
| --- | --- |
| `cargo test --manifest-path third_party/quiche-0.30.1/Cargo.toml --locked --lib --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false` | 1,315 passed; 0 failures |
| `cargo test --manifest-path third_party/quiche-0.30.1/Cargo.toml --locked --lib --features qlog --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false recovery::gcongestion::bbr3::` | 20 passed; 0 failures |
| `cargo test --manifest-path third_party/quiche-0.30.1/Cargo.toml --locked --lib --features congestion_window_unchecked_available --config profile.dev.package.boring-sys.opt-level=1 --config profile.dev.package.boring-sys.debug=false bbr3_and_unchecked_keep_independent_recovery` | 1 passed; 0 failures |

Flutter was resolved from `android/local.properties` and verified as 3.44.7,
revision `84fc5cbb223bc12f83d65b647ff8a56caf779ffd`, matching CI.

| Directory | Command | Result |
| --- | --- | --- |
| `apps/usque_gui` | `flutter pub get --enforce-lockfile` | Passed |
| `apps/usque_gui` | `flutter build apk --debug --config-only --no-pub` | Passed |
| `apps/usque_gui/android` | `./gradlew.bat --no-daemon :app:ktlintCheck` | Passed |
| `apps/usque_gui/android` | `./gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | Passed |

## Limits and safety

Windows ARM64, Linux and macOS native builds were `not_run` on this Windows x64
host. Hosted CI was `not_run`; this work did not push a branch or trigger it.
The 13 ignored workspace tests include isolated Wintun loading, explicitly
opted-in live proxy/TLS probes, supplied-profile/CDN checks and a controlled
memory benchmark. They were not counted as passes.

Protected Windows/Android lifecycle, external leak observation and controlled
performance runs were `not_run`: the required isolated environments were not
used. They remain supplemental rather than publication prerequisites. These
deterministic checks establish neither live WARP interoperability nor throughput,
device lifecycle, platform-state restoration or leak guarantees.

The local patch retains exact-path validation for probe rejection, bounded
DATAGRAM handling and the existing cleanup behavior. The migration adds no
platform-network mutation, credential logging or diagnostic upload. No MSI or
release APK was produced or installed. Generated JNI libraries, build output
and logs remain ignored local artifacts.
