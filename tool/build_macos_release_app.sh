#!/usr/bin/env bash
# Prepare the exact Flutter/Rust source and build the ordinary proxy preview.
set -euo pipefail
arch=${1:?expected native architecture}
test "$(uname -s)" = Darwin
test "$(uname -m)" = "$arch"
sdk="$RUNNER_TEMP/flutter"
git init -q "$sdk"
git -C "$sdk" fetch --depth 1 https://github.com/flutter/flutter.git "refs/tags/$FLUTTER_VERSION"
git -C "$sdk" checkout -q --detach FETCH_HEAD
test "$(git -C "$sdk" rev-parse HEAD)" = "$FLUTTER_COMMIT"
export PATH="$sdk/bin:$PATH"
RUSTUP_TOOLCHAIN=$(python3 -c 'import tomllib; print(tomllib.load(open("rust-toolchain.toml","rb"))["toolchain"]["channel"])')
export RUSTUP_TOOLCHAIN
rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal
target="$arch-apple-darwin"
if [[ "$arch" == arm64 ]]; then target=aarch64-apple-darwin; fi
rustup target add "$target"
cargo fetch --locked --target "$target"
flutter precache --macos
(cd apps/usque_gui && flutter pub get --enforce-lockfile)
python3 tool/macos_package.py build --arch "$arch" --no-dmg --output "$RUNNER_TEMP/mac-app"
