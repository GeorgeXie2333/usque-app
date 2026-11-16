# macOS development packaging

The default build now assembles the Flutter GUI, ordinary-user Engine, root
Agent and administrator maintenance tool. It uses the [Agent/utun route](MACOS_TUN_AGENT.md)
and requires no Apple Developer subscription. Network Extension prototypes remain
independent and are included only when explicitly supplied to the low-level
`package --network-prototype` command.

The native arm64 and x86_64 development bundles target macOS 13+, are ad-hoc
signed and are not notarized. Production TUN stays disabled pending real network
acceptance. Root service installation/update/removal requires administrator
authorization; production automatic installation and root auto-updates remain
disabled. Existing account login and system authentication remain available;
macOS Proton guest registration and its embedded browser remain removed.

## Earlier published development preview

The earlier [dev pre-release](https://github.com/JustinL330/usque-app/releases/tag/macos-dev-v0.3.3-compact-12636756)
provides macOS 13+ disk images, SHA-256 files, development manifests and SPDX file
inventories. Its pinned release tag `macos-dev-v0.3.3-compact-12636756` points to dev commit
`12636756a527267e029f34173c9e3e035ddaacfc`; the reused GUI, Engine and CEF source candidate is
`00e243f54cfe8cc6cf4665e9d30172344606c4e8`. Native network programs were rebuilt with the updated linker configuration. Later download-link documentation
changes do not change those binaries.

| Architecture | Development DMG | SHA-256 |
| --- | --- | --- |
| Apple Silicon | [arm64](https://github.com/JustinL330/usque-app/releases/download/macos-dev-v0.3.3-compact-12636756/usque-v0.3.3-macos-arm64.dmg) | `7ed984597567d538431320879503269ff7d763633aefc50950fe0d02ae692f86` |
| Intel | [x86_64](https://github.com/JustinL330/usque-app/releases/download/macos-dev-v0.3.3-compact-12636756/usque-v0.3.3-macos-x86_64.dmg) | `65abfe884a47727a956b9fab64087764d317f27ed89e64e842210cffe1e612ac` |

These packages predate removal of macOS Proton guest registration and the embedded
browser. They still contain CEF and do not represent the current source. No new
browser-free package size or release is claimed here.

These are unofficial, ad-hoc signed, unnotarized development packages. System
VPN activation and production automatic installation remain disabled. Read the
[QA checklist and retained issues](MACOS_QA_CHECKLIST.md) before testing; a
successful cross build does not establish Intel runtime acceptance. Installers
are release attachments, not files committed to the dev source tree.

Measured download reductions are 46.63% (arm64) and 43.17% (Intel). See the
[package size explanation and evidence](MACOS_PACKAGE_SIZE.md).

## Build one architecture

On a macOS host with the repository's pinned Flutter/Rust dependencies and both
Rust targets available:

```sh
source .toolchains/env.sh
python3 tool/macos_package.py build --arch arm64
python3 tool/macos_package.py build --arch x86_64
```

For a local TUN test package, add `--test-tun` to each build command. This
compiles matching Engine and Agent binaries and writes a `*-tun-test.dmg` to
`build/macos-tun-test/<architecture>/`. After copying the app to Applications,
install the background service in Settings. Usque reopens automatically after
the first installation. After updating an existing service, reopen Usque manually. TUN
becomes available when the Agent passes preflight and has no pending recovery.

Each invocation explicitly selects Flutter/Xcode and Rust architectures, sets
the macOS deployment target to 13.0, builds all native components, removes
unneeded framework slices, and checks every included Mach-O file. Network
prototype providers, when explicitly supplied, remain separate experimental components. Staged Mach-O files
lose only debug/local symbols, with global exports checked before and after,
then receive fresh ad-hoc signatures. An Intel
build produced on Apple Silicon remains a cross build until run on Intel
hardware. Builds serialize access to Flutter's shared build directory.

The output directory is `build/macos-development/<architecture>/`:

- `Usque.app`
- `usque-v<version>-macos-arm64.dmg` or
  `usque-v<version>-macos-x86_64.dmg`
- A SHA-256 file, development manifest, and SPDX 2.3 file inventory.

The disk image contains the app, an Applications folder link and a development
status notice. It uses macOS's LZMA-compressed `ULMO` format, supported since
macOS 10.15 and within this port's macOS 13 minimum. The builder verifies image integrity without mounting or
installing it. `--no-dmg` builds and verifies only the app and reports no disk
image artifact.

The build does not download, build or bundle CEF. App assembly and inspection
reject stale `UsqueProtonVerification` helpers and Chromium frameworks copied
from older GUI output. Rebuild the GUI before packaging if this check fails.
Flutter application languages remain available.

The SPDX document is a hashed bundle file inventory. Its dependency/license
review is explicitly incomplete; it does not replace the signed production
release SBOM process.

## Assemble existing build outputs

```sh
python3 tool/macos_package.py package \
  --arch arm64 --version 0.3.4 --build-number 30 \
  --app apps/usque_gui/build/macos/Build/Products/Release/Usque.app \
  --engine target/aarch64-apple-darwin/release/usque-engine \
  --agent target/aarch64-apple-darwin/release/usque-agent \
  --agent-maintenance build/macos-agent/arm64/usque-agent-maintenance

python3 tool/macos_package.py inspect \
  --arch arm64 --app build/macos-development/arm64/Usque.app
```

Assembly copies only generated bundles. It does not change installed
applications, user configuration, Keychain items, routes, DNS or extensions.
It rejects escaping/dangling bundle symlinks, wrong architectures, newer CPU
baselines, non-macOS binaries and deployment requirements above macOS 13.

The Engine resides in `Contents/MacOS/usque-engine`; a relative Resources link
preserves the bundle launch path. Agent and maintenance binaries are under
`Contents/Helpers`. Agent-route packages require both helpers. The installed
service pins a root-owned Engine copy; the GUI uses that copy after installation
and must restart after replacement. Engine/Agent receive hardened ad-hoc
signatures without account-related entitlements. Every included Mach-O slice is
inspected before/after stripping and signing, and the final bundle passes strict
code-signature validation. Passing integrity checks does not demonstrate TUN safety.

`UsqueAgentServiceIncluded` and `UsqueNetworkPrototypeIncluded` identify the
selected route in bundle metadata and the generated manifest. The historical
`tool/macos-maintenance` updater is separate from the new
`tool/macos-agent` administrator service maintenance tool. See the Agent document
for installation, rollback, recovery and removal behavior.

Running from a disk image does not enable login startup or system proxy
changes; those operations report that the app must first be copied into
Applications. The image itself provides the drag-to-Applications layout.

## Validation

```sh
python3 -m unittest discover -s tool -p test_macos_package.py
```

The isolated fixtures exercise architecture/deployment/CPU checks, malformed
universal slices, symlink boundaries, maintenance helper assembly, stale browser
artifact rejection and development metadata. Real image builds and hardware acceptance must be recorded
separately. The builder never claims Intel execution, macOS 13 hardware or
signed system VPN acceptance from a successful cross build.

Building, inspecting and these package fixtures do not launch the application
or a browser. Use those background checks while the workstation is in use.
WindowServer, file-picker and permission tests are separate interactive
acceptance steps and must not be started during a background-only run.
