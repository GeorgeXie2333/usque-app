# Codex cloud development / Codex 云端开发

The Usque cloud environment supports Linux Rust checks, Flutter analysis and
widget tests, a debug UI preview with a fake engine, and Python tooling.
[Contributing](../CONTRIBUTING.md) remains the acceptance matrix. Dependency
installation success does not mean all tests or platform gates passed.

## Environment configuration / 环境配置

Open **Settings > Codex Cloud > Environments**, select the `usque-app`
environment for `GeorgeXie2333/usque-app`, and edit its configuration. Keep
**Who can use** set to **Only me**. The setup needs no API keys, WARP account,
release certificates, signing keys, private-network access or production secrets.

The reviewed installer targets the Codex **Debian 13 (trixie), amd64** image and
the checkout at `/workspace/usque-app`. The base image must supply Python 3.11+
with venv, Git, curl, apt-get, dpkg-deb, tar, unzip and Debian archive signing
keys. It does not use root or sudo. It rejects other platforms instead of
installing Debian libraries into an incompatible environment.

Set environment variable `USQUE_CODEX_CLOUD=1`, then use this **Install script**:

```bash
cd /workspace/usque-app
bash tool/setup_codex_cloud.sh
```

The selected remote branch must contain both the script and the shared root
`AGENTS.md`. Local uncommitted files are not available to a remote checkout.
Before merging the repository configuration, the environment can store the
complete reviewed installer inline; keep it synchronized with
[the repository script](../tool/setup_codex_cloud.sh).

The opt-in variable prevents accidental workstation installation. It is not
proof of isolation and never authorizes networking or device tests.

Use **Package managers** and the additional Flutter download host
`storage.googleapis.com`. `pub.dev` access was available in the reviewed
environment without another domain entry. If a download is blocked, inspect
the actual denied hostname and allow only the required destination. Do not
disable TLS or package verification or broaden access to unrestricted.

Save changes and **Publish** or **Republish** after validation. Saving a draft
alone does not update the filesystem used by new tasks. See the
[official cloud environment guide](https://learn.chatgpt.com/docs/environments/cloud-environments).

## Reproducible installation / 可重复安装

The installer reads Rust's version, components and targets from
`rust-toolchain.toml`, Flutter's version and full revision from CI, and Ruff
from `pyproject.toml`. It verifies the official rustup download checksum and
Flutter revision, resolves Cargo with `--locked`, and resolves Pub with
`--enforce-lockfile`.

Native packages use Debian's signed repository metadata and package hashes.
Their complete dependency closure is extracted into a private prefix, without
changing the host package database or relying on already-installed headers.
The installer relocates absolute symlinks only inside that extracted prefix.

Tools and caches live under `/workspace/.usque-cloud`.
`USQUE_CLOUD_TOOLS_DIR` may select another directory under `/workspace`, outside
the checkout. The script preserves `HOME` and `CODEX_HOME`; XDG configuration,
Pub, pip, Rust and analyzer caches use their own writable directories.
`CI=true` and Flutter's analytics suppression avoid first-run analytics writes.

It creates ignored `.toolchains/codex-cloud.env.sh` and Android
`local.properties`. Existing conflicting configuration is rejected rather than
overwritten. Repeating the same installer is supported. If toolchain pins
change, review the old generated environment and SDK path files before choosing
a fresh cache and reinstalling; do not silently reset an SDK or regenerate locks.

Keep `AGENTS.md`, scripts, tests and documentation in Git. Keep `.env`, SDK
paths, private account files, keys, caches, diagnostics, JNI libraries and build
artifacts ignored. Do not upload all ignored files to make cloud setup work.

## Startup and checks / 启动与检查

Start each shell at the repository root and load the cloud environment:

```bash
cd /workspace/usque-app
source .toolchains/codex-cloud.env.sh
```

The **Start skill** should require reading `AGENTS.md`, `CONTRIBUTING.md` and
this guide before work. It should describe the fake-engine preview, the scoped
checks below, the dedicated cache paths, and unavailable platform gates. Keep
historical test counts out of reusable startup instructions; they apply only
to the source and environment that were tested.

Run the applicable commands from Contributing directly. The cloud source file
is separate from `.toolchains/env.sh`, which the general Linux development
wrapper may load for another machine's configuration.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
ruff check tool
ruff format --check tool
python3 -m unittest discover -s tool -p 'test_*.py' -v
python3 tool/check_repository_policy.py
git diff --check

cd apps/usque_gui
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub --exclude-tags golden
```

For the Linux preview build, scope the private GTK prefix to this command only:

```bash
PKG_CONFIG_SYSROOT_DIR="$USQUE_CLOUD_TOOLS_DIR/native" \
PKG_CONFIG_LIBDIR="$USQUE_CLOUD_TOOLS_DIR/native/usr/lib/x86_64-linux-gnu/pkgconfig:$USQUE_CLOUD_TOOLS_DIR/native/usr/share/pkgconfig" \
flutter build linux --debug --no-pub -t lib/main_preview.dart
```

Do not export `PKG_CONFIG_SYSROOT_DIR` globally; it changes how unrelated builds
resolve system headers and libraries. An interactive preview requires a display.
The reviewed cloud session used an independent D-Bus session, X11 and software
rendering. This is a simulated UI, with no production Linux engine or VPN.

## Validation boundaries / 验证边界

- Windows builds, native runner tests and exact Windows goldens remain separate
  required checks. Do not regenerate Windows baselines on Linux.
- Android SDK/NDK, JDK and PowerShell are not part of this base setup. Android
  compile/static checks require the pinned toolchains and commands from
  Contributing. No APK installation or release signing occurs here.
- Buf, actionlint, Go and PowerShell/PSScriptAnalyzer must be prepared when the
  corresponding change scope requires them. The general `tool/dev.sh doctor`
  and `check all` are not base-cloud readiness checks.
- Cloud hosting alone is not a protected test environment. Do not create a
  TUN/Wintun session or change host routes, DNS, firewall or system proxy.
  Installer, update, uninstall, device and protected-runner tests retain their
  isolation requirements and are recorded `not_run` when unavailable.
- A repeated install and a clean tool-prefix test do not prove that a new task
  restores the published snapshot. Report those evidence scopes separately.

发布后，新任务选择 **Work in > Cloud** 和 `usque-app`。环境升级需重新发布；
已有任务仍保留自己的状态。初始化完成只代表依赖就绪，验证结果以实际命令为准。
