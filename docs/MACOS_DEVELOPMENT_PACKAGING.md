# macOS development packaging

The macOS port assembles the Flutter app, Engine, native control helper,
four Network Extension prototypes, standalone sandbox-enabled Proton CEF
component and maintenance helper into separate Apple Silicon and Intel
development bundles. This is development delivery, not full Windows feature
parity. Signed VPN activation, official Proton challenge acceptance, production
updates, managed uninstallation and hardware acceptance remain incomplete.

The production update channel and Windows/Linux release workflows are unchanged.
The generated app and manifest explicitly disable production automatic
installation. These ad-hoc disk images are not notarized and cannot activate
the Network Extensions.

## Published development preview

The fork's [dev pre-release](https://github.com/JustinL330/usque-app/releases/tag/macos-dev-v0.3.3-3289e10d)
provides macOS 13+ disk images, SHA-256 files, development manifests and SPDX file
inventories. Its pinned release tag `macos-dev-v0.3.3-3289e10d` points to dev commit
`3289e10d3dd88c9d4b6431bd0c98c677df6500fd`; the application source candidate is
`00e243f54cfe8cc6cf4665e9d30172344606c4e8`. Later download-link documentation
changes do not change those binaries.

| Architecture | Development DMG | SHA-256 |
| --- | --- | --- |
| Apple Silicon | [arm64](https://github.com/JustinL330/usque-app/releases/download/macos-dev-v0.3.3-3289e10d/usque-v0.3.3-macos-arm64.dmg) | `c55c2de4a7e0964d2f163b26efce825d6475ef800a2e5d34a73defe07c964e6d` |
| Intel | [x86_64](https://github.com/JustinL330/usque-app/releases/download/macos-dev-v0.3.3-3289e10d/usque-v0.3.3-macos-x86_64.dmg) | `03f47e9f1ac306d6e52e8b0313d68477440ab8d846b97194129b92d4b6c931c5` |

These are unofficial, ad-hoc signed, unnotarized development packages. System
VPN activation and production automatic installation remain disabled. Read the
[QA checklist and retained issues](MACOS_QA_CHECKLIST.md) before testing; a
successful cross build does not establish Intel runtime acceptance. Installers
are release attachments, not files committed to the dev source tree.

## Build one architecture

On a macOS host with the repository's pinned Flutter/Rust dependencies and both
Rust targets available, and both pinned CEF SDK archives cached as described
in the [verification component README](../apps/usque_gui/macos/ProtonVerification/README.md):

```sh
source .toolchains/env.sh
python3 tool/macos_package.py build --arch arm64
python3 tool/macos_package.py build --arch x86_64
```

Each invocation explicitly selects Flutter/Xcode and Rust architectures, sets
the macOS deployment target to 13.0, builds all native components, removes
unneeded framework slices, and checks every included Mach-O file. An Intel
build produced on Apple Silicon remains a cross build until run on Intel
hardware. Builds serialize access to Flutter's shared build directory.

The output directory is `build/macos-development/<architecture>/`:

- `Usque.app`
- `usque-v<version>-macos-arm64.dmg` or
  `usque-v<version>-macos-x86_64.dmg`
- A SHA-256 file, development manifest, and SPDX 2.3 file inventory.

The disk image contains the app, an Applications folder link and a development
status notice. The builder verifies image integrity without mounting or
installing it. `--no-dmg` builds and verifies only the app and reports no disk
image artifact.

The SPDX document is a hashed bundle file inventory. Its dependency/license
review is explicitly incomplete; it does not replace the signed production
release SBOM process.

## Assemble existing build outputs

```sh
python3 tool/macos_package.py package \
  --arch arm64 --version 0.3.3 --build-number 29 \
  --app apps/usque_gui/build/macos/Build/Products/Release/Usque.app \
  --engine target/aarch64-apple-darwin/release/usque-engine \
  --network-prototype build/macos-network-prototype/arm64/UsqueNetworkPrototype.app \
  --verification build/macos-proton-verification/arm64/UsqueProtonVerification.app \
  --maintenance build/macos-maintenance/arm64/usque-maintenance

python3 tool/macos_package.py inspect \
  --arch arm64 --app build/macos-development/arm64/Usque.app
```

Assembly copies only generated bundles. It does not change installed
applications, user configuration, Keychain items, routes, DNS or extensions.
It rejects escaping/dangling bundle symlinks, wrong architectures, newer CPU
baselines, non-macOS binaries and deployment requirements above macOS 13.

The Engine resides in `Contents/MacOS/usque-engine`; a relative compatibility
link in Resources preserves the Flutter launch path. This also satisfies the
Engine's canonical bundle lookup for `Contents/Helpers/usque-macos-control`.
Every framework, Engine, helper and provider is inspected and ad-hoc signed
after assembly. Passing this integrity check does not demonstrate VPN safety.
When CEF is included, inspection requires its browser executable, framework and
all five helper variants, required ICU/V8 data, and runnable program permissions,
with the expected identity, macOS 13 target and required sandbox flag.
Development, disabled-production, disabled-automatic-installation
and disabled-signed-prototype flags are checked explicitly.
The `build` command always includes CEF and maintenance. The lower-level
`package` command permits omitting these components for fixtures and partial
development builds, and records their absence explicitly in the manifest.

The release GUI requires a same-Team signed CEF component before transmitting
an authentication handoff. Ad-hoc integrity is insufficient for that check.
The maintenance helper has no configured production trust authority and rejects
mutating commands before mounting, staging or installing anything. Its isolated
atomic-replacement and rollback checks are described in the
[maintenance README](../tool/macos-maintenance/README.md).

Running from a disk image does not enable login startup or system proxy
changes; those operations report that the app must first be copied into
Applications. The image itself provides the drag-to-Applications layout.

## Validation

```sh
python3 -m unittest discover -s tool -p test_macos_package.py
```

The isolated fixtures exercise architecture/deployment/CPU checks, malformed
universal slices, symlink boundaries, nested CEF/helper assembly and development
metadata. Real image builds and hardware acceptance must be recorded
separately. The builder never claims Intel execution, macOS 13 hardware or
signed system VPN acceptance from a successful cross build.

Building, inspecting and these package fixtures do not launch the application,
CEF or a browser. Use those background checks while the workstation is in use.
WindowServer/CEF smoke, file-picker and permission tests are separate interactive
acceptance steps and must not be started during a background-only run.
