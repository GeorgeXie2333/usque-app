# Usque workspace build notes

## Safety boundary

On this development host, never install a generated MSI, start Windows VPN mode, create a TUN/Wintun session, or run commands that apply WFP, routes, DNS, or system-proxy mutations.

`usque-agent --validate-only` and MSI table/static verification are safe. Do not run `--recover-state` or `--emergency-remove-kill-switch` on this host merely to test a build.

Windows VPN and uninstall recovery need a snapshot-enabled isolated VM with another way in. SOCKS5/HTTP tests are allowed here.

Install and uninstall behavior for end users is in `docs/INSTALLATION.md`. Do not run `usque-uninstall.exe` against a real ProductCode on this host, and do not run the engine `--purge-user-data`.

## Pinned local toolchains

- Rust: `1.97.1`; always use `--locked`.
- Flutter and Android SDK paths come from `apps/usque_gui/android/local.properties`. Do not assume `flutter` is on `PATH`.
- Android CMake/Ninja: use the SDK copy below `<sdk.dir>\cmake\3.22.1\bin` (or the newest installed SDK CMake directory containing both executables).
- Windows C++ tools: locate Visual Studio Build Tools with `vswhere.exe` and import `VC\Auxiliary\Build\vcvars64.bat`.

## Windows x64-v2 build

Do not invoke a plain release Cargo build in a fresh shell. With Visual Studio 18 Build Tools, the `cmake` crate may auto-select `Visual Studio 18 2026`, while the installed CMake 3.22 only understands generators through Visual Studio 17. The resulting BoringSSL build fails before compiling.

Use the checked-in helper, which imports the MSVC x64 environment and forces the Ninja generator. Use the same helper for full-workspace tests and Clippy because they also compile BoringSSL.

The helper must invoke the `vcvars64.bat` wrapper with no `-arch` or `-host_arch` arguments; those switches are invalid for the installed Visual Studio 18 wrapper and can leave `INCLUDE` unset while appearing to return success. The helper also locates `libclang.dll`, places its dependency directory on `PATH`, and the vendored `boring-sys` build forwards the imported MSVC/Windows SDK include directories to bindgen. Do not remove any of these steps merely because a cached BoringSSL binding makes one local build pass.

```powershell
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
```

If a previous failed BoringSSL configure left a generator cache, clean only that generated package and retry; do not delete the entire `target` tree:

```powershell
cargo clean -p boring-sys --release --target x86_64-pc-windows-msvc
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
```

When Flutter/Dart or Windows runner code changed, rebuild the GUI using the exact SDK from `local.properties`:

```powershell
$flutterSdk = ((Get-Content .\apps\usque_gui\android\local.properties |
    Where-Object { $_.StartsWith('flutter.sdk=') } |
    Select-Object -First 1).Substring('flutter.sdk='.Length)).Replace('\:', ':').Replace('\\', '\')
Push-Location .\apps\usque_gui
& "$flutterSdk\bin\flutter.bat" pub get
& "$flutterSdk\bin\flutter.bat" analyze
& "$flutterSdk\bin\flutter.bat" test
& "$flutterSdk\bin\flutter.bat" build windows --release
Pop-Location
```

Package the local validation MSI only after Rust tests/Clippy and Flutter checks pass:

```powershell
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& .\tool\build_windows_local_validation.ps1 -Variant x64-v2 -Version "0.1.3"
```

The packaging script creates one temporary self-signed identity, signs the copied payload and MSI, validates the MSI tables, then removes the private key and temporary trust entries. Certificate-store access may need an approved elevated run. Never install the produced MSI automatically.

WiX, ARP hiding, `usque-uninstall.exe`, and `USQUE_REMOVE_USER_DATA` are documented in `docs/RELEASE.md`. Do not replace the custom installer UI with stock `WixUI_InstallDir`.

## Android arm64-v8a build

Use the pinned NDK/CMake routing in the existing helper, then call Flutter by its explicit SDK path:

```powershell
& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction build -Profile release
$flutterSdk = ((Get-Content .\apps\usque_gui\android\local.properties |
    Where-Object { $_.StartsWith('flutter.sdk=') } |
    Select-Object -First 1).Substring('flutter.sdk='.Length)).Replace('\:', ':').Replace('\\', '\')
Push-Location .\apps\usque_gui
& "$flutterSdk\bin\flutter.bat" build apk --release --split-per-abi --target-platform android-arm64
Pop-Location
```

Verify the APK contains only `lib/arm64-v8a`, has no `kernel_blob.bin` or Vulkan validation layer, and uses the expected signing certificate before delivery.
