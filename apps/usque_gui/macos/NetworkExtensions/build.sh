#!/bin/bash
set -euo pipefail
extension_root=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$extension_root/../../../.." && pwd)
arch=arm64
output=""
runtime_library=""
typecheck=0
signed_poc=0
identity=-
while (($#)); do
  case "$1" in
    --arch) arch=$2; shift 2 ;;
    --output) output=$2; shift 2 ;;
    --runtime-library) runtime_library=$2; shift 2 ;;
    --typecheck) typecheck=1; shift ;;
    --signed-poc) signed_poc=1; shift ;;
    --identity) identity=$2; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$arch" in
  arm64) rust_target=aarch64-apple-darwin ;;
  x86_64) rust_target=x86_64-apple-darwin ;;
  *) echo "--arch must be arm64 or x86_64" >&2; exit 2 ;;
esac
if [[ -z "$output" ]]; then output="$repo_root/build/macos-network-prototype/$arch"; fi
[[ "$output" = /* ]] || { echo "--output must be absolute" >&2; exit 2; }
sdk=$(xcrun --sdk macosx --show-sdk-path)
module_cache="$repo_root/.toolchains/cache/clang"
mkdir -p "$module_cache" "$output"
export CLANG_MODULE_CACHE_PATH="$module_cache"
export MACOSX_DEPLOYMENT_TARGET=13.0
flags=(-swift-version 5 -sdk "$sdk" -target "$arch-apple-macos13.0" -module-cache-path "$module_cache" -import-objc-header "$extension_root/include/usque_native.h" -parse-as-library)
prefix=io.github.georgexie2333.usque
if ((signed_poc)); then flags+=(-D SIGNED_POC); prefix+=.poc; fi
common=("$extension_root/Shared/Protocol.swift" "$extension_root/Shared/PhysicalPathState.swift" "$extension_root/Shared/PhysicalNetwork.swift" "$extension_root/Shared/Identity.swift" "$extension_root/Shared/ControlChannel.swift" "$extension_root/Shared/Policy.swift")
frameworks=(-framework Foundation -framework Security -framework Network -framework NetworkExtension -framework SystemExtensions)
link_checks=(-Xlinker -fatal_warnings)
if ((typecheck)); then
  xcrun clang -fobjc-arc -fblocks -fsyntax-only -isysroot "$sdk" -target "$arch-apple-macos13.0" "$extension_root/Shared/AuthenticatedXPC.m"
  for role in PacketTunnel TransparentProxy DNSProxy ContentFilter; do
    xcrun swiftc "${flags[@]}" -typecheck -module-name "Usque$role" "${common[@]}" "$extension_root/Shared/ProviderMain.swift" "$extension_root/$role/"*.swift "${frameworks[@]}"
  done
  xcrun swiftc "${flags[@]}" -typecheck -module-name UsqueControl "${common[@]}" "$extension_root/Control/"*.swift "${frameworks[@]}"
  echo "Typechecked four isolated providers and control helper ($arch, deployment target 13.0). No extension activated."
  exit 0
fi
if [[ -z "$runtime_library" ]]; then runtime_library="$repo_root/target/$rust_target/debug/libusque_macos_runtime.a"; fi
[[ -f "$runtime_library" ]] || { echo "Missing Rust runtime library. Build usque-macos-runtime for $rust_target or pass --runtime-library." >&2; exit 2; }
[[ "$(lipo -archs "$runtime_library")" = "$arch" ]] || { echo "Rust library does not match $arch" >&2; exit 2; }
team=${USQUE_MACOS_TEAM_ID:-UNSIGNED}
if [[ "$identity" != - && ! "$team" =~ ^[A-Z0-9]{10}$ ]]; then echo "A signing identity requires USQUE_MACOS_TEAM_ID" >&2; exit 2; fi
app="$output/UsqueNetworkPrototype.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Library/SystemExtensions" "$app/Contents/Resources"
native_object="$output/AuthenticatedXPC.o"
xcrun clang -fobjc-arc -fblocks -O2 -isysroot "$sdk" -target "$arch-apple-macos13.0" -c "$extension_root/Shared/AuthenticatedXPC.m" -o "$native_object"
for role in packet transparent dns filter; do
  case "$role" in
    packet) name=UsquePacketTunnel; directory=PacketTunnel; suffix=packet-tunnel; provider_key=com.apple.networkextension.packet-tunnel; provider_class=PacketTunnelProvider; entitlement=packet-tunnel-provider-systemextension ;;
    transparent) name=UsqueTransparentProxy; directory=TransparentProxy; suffix=transparent-proxy; provider_key=com.apple.networkextension.app-proxy; provider_class=TransparentProxyProvider; entitlement=app-proxy-provider-systemextension ;;
    dns) name=UsqueDNSProxy; directory=DNSProxy; suffix=dns-proxy; provider_key=com.apple.networkextension.dns-proxy; provider_class=DNSProxyProvider; entitlement=dns-proxy-systemextension ;;
    filter) name=UsqueContentFilter; directory=ContentFilter; suffix=content-filter; provider_key=com.apple.networkextension.filter-packet; provider_class=ContentFilterProvider; entitlement=content-filter-provider-systemextension ;;
  esac
  bundle="$app/Contents/Library/SystemExtensions/$prefix.$suffix.systemextension"
  mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
  xcrun swiftc "${flags[@]}" -O -module-name "$name" "${common[@]}" "$extension_root/Shared/ProviderMain.swift" "$extension_root/$directory/"*.swift \
    "$native_object" "$runtime_library" "${frameworks[@]}" "${link_checks[@]}" -framework SystemConfiguration -lc++ -lz -lresolv -o "$bundle/Contents/MacOS/$name"
  python3 - "$bundle" "$name" "$prefix.$suffix" "$provider_key" "$provider_class" "$entitlement" "$team.$prefix.network" "$role" <<'PY'
import plistlib, sys
bundle, name, identifier, provider_key, provider_class, entitlement, group, role = sys.argv[1:]
classes = {provider_key: provider_class}
if role == 'filter': classes['com.apple.networkextension.filter-data'] = 'FlowMetadataProvider'
info = {'CFBundleExecutable': name, 'CFBundleIdentifier': identifier, 'CFBundleName': name,
        'CFBundlePackageType': 'SYSX', 'CFBundleVersion': '1', 'CFBundleShortVersionString': '0.1.0',
        'LSMinimumSystemVersion': '13.0', 'NSSystemExtensionUsageDescription': 'Usque signed network architecture experiment.',
        'NetworkExtension': {'NEMachServiceName': group + '.' + role, 'NEProviderClasses': classes}}
entitlements = {'com.apple.developer.networking.networkextension': [entitlement],
                'com.apple.security.application-groups': [group], 'com.apple.security.network.client': True}
with open(bundle+'/Contents/Info.plist', 'wb') as f: plistlib.dump(info, f)
with open(bundle+'/Contents/Resources/Entitlements.plist', 'wb') as f: plistlib.dump(entitlements, f)
PY
  signing=(--force --sign "$identity")
  if [[ "$identity" != - ]]; then signing+=(--entitlements "$bundle/Contents/Resources/Entitlements.plist" --options runtime --timestamp); fi
  codesign "${signing[@]}" "$bundle"
  [[ "$(lipo -archs "$bundle/Contents/MacOS/$name")" = "$arch" ]]
done
xcrun swiftc "${flags[@]}" -O -module-name UsqueControl "${common[@]}" "$extension_root/Control/"*.swift \
  "$native_object" "$runtime_library" "${frameworks[@]}" "${link_checks[@]}" -framework SystemConfiguration -lc++ -lz -lresolv -o "$app/Contents/MacOS/usque-macos-control"
python3 - "$app" "$prefix" "$team" "$signed_poc" <<'PY'
import plistlib, sys
app, prefix, team, poc = sys.argv[1:]
info = {'CFBundleExecutable': 'usque-macos-control', 'CFBundleIdentifier': prefix+'.macos-control',
        'CFBundleName': 'Usque Network Prototype', 'CFBundlePackageType': 'APPL', 'CFBundleVersion': '1',
        'CFBundleShortVersionString': '0.1.0', 'LSMinimumSystemVersion': '13.0', 'LSUIElement': True,
        'UsqueSignedPrototype': poc == '1', 'UsqueProductionCapable': False,
        'NSSystemExtensionUsageDescription': 'Test Usque VPN, DNS, transparent routing and independent protection.'}
entitlements = {'com.apple.developer.system-extension.install': True,
                'com.apple.developer.networking.networkextension': ['packet-tunnel-provider-systemextension','app-proxy-provider-systemextension','dns-proxy-systemextension','content-filter-provider-systemextension'],
                'com.apple.security.application-groups': [team+'.'+prefix+'.network']}
with open(app+'/Contents/Info.plist', 'wb') as f: plistlib.dump(info, f)
with open(app+'/Contents/Resources/Entitlements.plist', 'wb') as f: plistlib.dump(entitlements, f)
PY
signing=(--force --sign "$identity")
if [[ "$identity" != - ]]; then signing+=(--entitlements "$app/Contents/Resources/Entitlements.plist" --options runtime --timestamp); fi
codesign "${signing[@]}" "$app"
[[ "$(lipo -archs "$app/Contents/MacOS/usque-macos-control")" = "$arch" ]]
codesign --verify --strict --deep "$app"
echo "Built $app ($arch, macOS 13+)."
echo "Four separate provider executables; production capability remains false."
if [[ "$identity" = - ]]; then echo "AD-HOC DEVELOPMENT ONLY: compiled but cannot activate Network Extensions."; fi
