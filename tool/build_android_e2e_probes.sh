#!/usr/bin/env bash
# Builds the two code-free, debuggable probe apps used by
# tool/android_per_app_e2e.py. Each probe only declares INTERNET, so it gets a
# UID of its own that `run-as` can act as, even on user builds.
#
# Usage: tool/build_android_e2e_probes.sh <android-sdk> <output-dir>
#
# The signing key is generated per run, never stored, and identifies nothing
# but these disposable test apps.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <android-sdk> <output-dir>" >&2
  exit 2
fi

sdk=$1
out=$2
build_tools="$sdk/build-tools/36.0.0"
platform="$sdk/platforms/android-36/android.jar"
test -x "$build_tools/aapt2"
test -f "$platform"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

PROBE_KEY_PASSWORD=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
export PROBE_KEY_PASSWORD
keytool -genkeypair -noprompt -keystore "$work/probe.p12" -storetype PKCS12 \
  -storepass:env PROBE_KEY_PASSWORD -alias probe -keyalg RSA -keysize 2048 -validity 2 \
  -dname "CN=usque-e2e-probe" > /dev/null 2>&1

for probe in a b; do
  mkdir -p "$work/$probe"
  cat > "$work/$probe/AndroidManifest.xml" << EOF
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="io.github.georgexie2333.usque.e2e.probe$probe"
    android:versionCode="1" android:versionName="1">
    <uses-permission android:name="android.permission.INTERNET" />
    <application android:label="Usque probe ${probe^^}" android:hasCode="false" />
</manifest>
EOF
  "$build_tools/aapt2" link --manifest "$work/$probe/AndroidManifest.xml" \
    -I "$platform" --min-sdk-version 24 --target-sdk-version 36 --debug-mode \
    -o "$work/$probe/unsigned.apk"
  "$build_tools/zipalign" -f -p 4 "$work/$probe/unsigned.apk" "$work/$probe/aligned.apk"
  "$build_tools/apksigner" sign --ks "$work/probe.p12" --ks-pass env:PROBE_KEY_PASSWORD \
    --v4-signing-enabled false --out "$out/probe-$probe.apk" "$work/$probe/aligned.apk"
  "$build_tools/apksigner" verify "$out/probe-$probe.apk"
done
