#!/usr/bin/env bash
set -euo pipefail
# This opt-in prevents accidentally running a cloud installer on a workstation.
# It is not evidence of isolation and never authorizes privileged networking.
fail() { printf 'CODEX_CLOUD_ERROR: %s\n' "$*" >&2; exit 1; }
[[ $# -eq 0 ]] || fail 'Usage: bash tool/setup_codex_cloud.sh (no arguments)'
[[ "${USQUE_CODEX_CLOUD:-}" == 1 ]] || fail 'Set USQUE_CODEX_CLOUD=1 in the cloud environment.'
[[ "$(uname -s)" == Linux ]] || fail 'This installer requires Linux.'
[[ "$(uname -m)" == x86_64 ]] || fail 'This installer requires amd64.'
usque_tools=$(realpath -m -- "${USQUE_CLOUD_TOOLS_DIR:-/workspace/.usque-cloud}")
[[ "$usque_tools" == /workspace/* && "$usque_tools" != /workspace/usque-app && "$usque_tools" != /workspace/usque-app/* ]] || fail 'Keep cloud tools outside the repository, under /workspace.'
export USQUE_CLOUD_TOOLS_DIR="$usque_tools"
# shellcheck source=/dev/null
source /etc/os-release
[[ "$ID" == debian && "${VERSION_CODENAME:-}" == trixie ]] || fail 'Use the reviewed Debian 13 (trixie) cloud image.'
[[ -f /workspace/usque-app/AGENTS.md ]] || fail 'The shared root AGENTS.md must be present before setup.'
# Debian amd64, unprivileged; base image supplies Python 3.11+, Git, curl,
# apt-get, dpkg-deb, tar, unzip and Debian archive signing keys.
cd /workspace/usque-app
usque_root=$PWD
usque_tools=${USQUE_CLOUD_TOOLS_DIR:-/workspace/.usque-cloud}
export USQUE_CLOUD_TOOLS_DIR="$usque_tools"
mkdir -p "$usque_tools"
python3 - "$usque_root" "$usque_tools" <<'PY'
import json, pathlib, re, shlex, sys, tomllib
root, tools = map(pathlib.Path, sys.argv[1:])
rust = tomllib.loads((root/'rust-toolchain.toml').read_text())['toolchain']
ci = (root/'.github/workflows/ci.yml').read_text()
ruff = tomllib.loads((root/'pyproject.toml').read_text())['tool']['ruff']['required-version']
if not ruff.startswith('=='):
 raise SystemExit('Ruff must be exactly pinned')
values = {
 'usque_rust_version': rust['channel'],
 'usque_rust_components': ','.join(rust.get('components', [])),
 'usque_rust_targets': ','.join(rust.get('targets', [])),
 'usque_flutter_version': re.search(r'^  FLUTTER_VERSION: ([\d.]+)$', ci, re.M)[1],
 'usque_flutter_commit': re.search(r'^  FLUTTER_COMMIT: ([a-f0-9]{40})$', ci, re.M)[1],
 'usque_ruff_version': ruff[2:]
}
(tools/'pins.sh').write_text(''.join(f'{k}={shlex.quote(v)}\n' for k,v in values.items()))
PY
declare usque_rust_version usque_ruff_version usque_flutter_version usque_flutter_commit
# shellcheck source=/dev/null
source "$usque_tools/pins.sh"
# Download the complete dependency closure, even when packages happen to be
# installed on this machine. apt validates signed Release metadata and hashes.
mkdir -p "$usque_tools/apt/"{lists/partial,archives/partial,conf} "$usque_tools/native"
: > "$usque_tools/apt/status"
cat > "$usque_tools/apt/sources.list" <<'SOURCES'
deb [signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] https://deb.debian.org/debian trixie main
SOURCES
usque_apt=(-o APT::Update::Error-Mode=any -o Debug::NoLocking=true -o Dir::Etc::main=-
 -o Dir::Etc::parts="$usque_tools/apt/conf"
 -o Dir::Etc::sourcelist="$usque_tools/apt/sources.list" -o Dir::Etc::sourceparts=-
 -o Dir::State::status="$usque_tools/apt/status"
 -o Dir::State::lists="$usque_tools/apt/lists"
 -o Dir::Cache::archives="$usque_tools/apt/archives"
 -o Dir::Cache::pkgcache="$usque_tools/apt/pkgcache.bin"
 -o Dir::Cache::srcpkgcache="$usque_tools/apt/srcpkgcache.bin")
/usr/bin/apt-get "${usque_apt[@]}" update
/usr/bin/apt-get "${usque_apt[@]}" --download-only -y install \
 clang cmake ninja-build pkg-config g++ libgtk-3-dev libclang-dev libssl-dev \
 xvfb xauth unzip xz-utils
for usque_deb in "$usque_tools"/apt/archives/*.deb; do
 dpkg-deb -x "$usque_deb" "$usque_tools/native"
done
# Relocate absolute symlinks inside our extracted prefix only.
python3 - "$usque_tools/native" <<'PY'
import os, pathlib, sys
root=pathlib.Path(sys.argv[1])
for base, dirs, files in os.walk(root):
 for name in dirs+files:
  p=pathlib.Path(base)/name
  if p.is_symlink():
   target=os.readlink(p)
   if target.startswith(('/usr/', '/lib/')) and (root/target.lstrip('/')).exists():
    p.unlink(); p.symlink_to(os.path.relpath(root/target.lstrip('/'),p.parent))
PY
export CARGO_HOME="$usque_tools/cargo" RUSTUP_HOME="$usque_tools/rustup"
if [[ ! -x "$CARGO_HOME/bin/rustup" ]]; then
 usque_rustup_url=https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init
 curl --fail --show-error --location "$usque_rustup_url" -o "$usque_tools/rustup-init"
 curl --fail --show-error --location "$usque_rustup_url.sha256" -o "$usque_tools/rustup-init.sha256"
 (cd "$usque_tools" && sha256sum --check rustup-init.sha256)
 chmod +x "$usque_tools/rustup-init"
 "$usque_tools/rustup-init" -y --no-modify-path --default-toolchain none
fi
usque_rust_args=(--profile minimal)
[[ -z "$usque_rust_components" ]] || usque_rust_args+=(--component "$usque_rust_components")
[[ -z "$usque_rust_targets" ]] || usque_rust_args+=(--target "$usque_rust_targets")
"$CARGO_HOME/bin/rustup" toolchain install "$usque_rust_version" "${usque_rust_args[@]}"
if [[ ! -x "$usque_tools/venv/bin/python" ]]; then python3 -m venv "$usque_tools/venv"; fi
PIP_CACHE_DIR="$usque_tools/cache/pip" "$usque_tools/venv/bin/pip" install "ruff==$usque_ruff_version"
# Official upstream source, verified against the full CI revision. Never reset
# an existing SDK checkout or silently replace a user-supplied SDK.
if [[ ! -e "$usque_tools/flutter" ]]; then
 git clone --depth 1 --branch "$usque_flutter_version" https://github.com/flutter/flutter.git "$usque_tools/flutter"
fi
test "$(git -C "$usque_tools/flutter" rev-parse HEAD)" = "$usque_flutter_commit"
git -C "$usque_tools/flutter" diff --quiet HEAD --
# Record a generated environment without replacing pre-existing user content.
python3 - "$usque_root" "$usque_tools" "$usque_rust_version" <<'PY'
import pathlib, shlex, sys
root,tools=map(pathlib.Path,sys.argv[1:3]); version=sys.argv[3]
q=lambda p:shlex.quote(str(p))
text=f'''# Generated by the reviewed Codex cloud installer; HOME stays unchanged.
export USQUE_CLOUD_TOOLS_DIR={q(tools)}
export CARGO_HOME={q(tools/'cargo')}
export RUSTUP_HOME={q(tools/'rustup')}
export RUSTUP_TOOLCHAIN={q(version)}
export PUB_CACHE={q(tools/'pub')}
export XDG_CONFIG_HOME={q(tools/'config')}
export XDG_CACHE_HOME={q(tools/'cache')}
export ANALYZER_STATE_LOCATION_OVERRIDE={q(tools/'analyzer')}
export PIP_CACHE_DIR={q(tools/'cache/pip')}
export CI=true
export FLUTTER_SUPPRESS_ANALYTICS=true
export PATH={q(tools/'cargo/bin')}:{q(tools/'venv/bin')}:{q(tools/'native/usr/bin')}:{q(tools/'flutter/bin')}:$PATH
export LIBCLANG_PATH={q(tools/'native/usr/lib/x86_64-linux-gnu')}
export LD_LIBRARY_PATH={q(tools/'native/usr/lib/x86_64-linux-gnu')}:{q(tools/'native/usr/lib/llvm-19/lib')}${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}
export CARGO_BUILD_JOBS=4
# PKG_CONFIG_SYSROOT_DIR is deliberately scoped to preview commands, never global.
'''
env=root/'.toolchains/codex-cloud.env.sh'; env.parent.mkdir(exist_ok=True)
if env.exists() and env.read_text()!=text:
 raise SystemExit(f'Preserve existing user configuration: review {env} before changing it')
env.write_text(text)
properties=root/'apps/usque_gui/android/local.properties'
if properties.exists():
 matches=[line[12:] for line in properties.read_text().splitlines() if line.startswith('flutter.sdk=')]
 if matches!=[str(tools/'flutter')]:
  raise SystemExit(f'Preserve existing SDK paths: review {properties}; expected flutter.sdk={tools}/flutter')
else:
 properties.write_text(f'flutter.sdk={tools}/flutter\n')
PY
# shellcheck source=/dev/null
source "$usque_root/.toolchains/codex-cloud.env.sh"
flutter --suppress-analytics precache --linux
flutter --version --machine > "$usque_tools/flutter-version.json"
python3 - "$usque_tools/flutter-version.json" "$usque_flutter_version" "$usque_flutter_commit" <<'PY'
import json,sys
v=json.load(open(sys.argv[1]))
if (v['frameworkVersion'],v['frameworkRevision'])!=tuple(sys.argv[2:]):
 raise SystemExit('Flutter version/revision does not match CI')
print('Verified Flutter',v['frameworkVersion'],v['frameworkRevision'])
PY
cargo fetch --locked
cd "$usque_root/apps/usque_gui"
flutter pub get --enforce-lockfile
cd "$usque_root"
rustc --version
ruff --version
clang --version
cmake --version
python3 - <<'PY'
import ctypes,os
ctypes.CDLL(os.path.join(os.environ['LIBCLANG_PATH'],'libclang-19.so.19'))
print('libclang load verified')
PY
PKG_CONFIG_SYSROOT_DIR="$usque_tools/native" \
PKG_CONFIG_LIBDIR="$usque_tools/native/usr/lib/x86_64-linux-gnu/pkgconfig:$usque_tools/native/usr/share/pkgconfig" \
pkg-config --modversion gtk+-3.0
# This setup prepares Linux development only; no device/VPN/signing operations.
printf 'CODEX_CLOUD_DEPENDENCIES_OK\n'
