# Linux and WSL development / Linux 与 WSL 开发

Use the Linux checkout for editing, Rust tests, Flutter widget tests, Android
compile/static checks and an interactive UI preview. These commands use Linux
tools without reading or synchronizing a Windows checkout. Windows builds and
golden comparisons remain separate required checks in the existing CI jobs.

日常编辑、单元测试、Android 编译检查和界面预览可以全部在 Linux/WSL 内完成，
无需 Windows 仓库或 Windows 开发工具。Windows 构建和 golden 检查仍由对应 CI
执行；本地未执行时记录为 `not_run`。

## Set up the tools / 工具准备

Follow the versions and acceptance commands in [Contributing](../CONTRIBUTING.md).
Rust comes from [rust-toolchain.toml](../rust-toolchain.toml), Flutter's version
and full revision from [CI](../.github/workflows/ci.yml), and Android NDK/CMake
from the [Android helper](../tool/build_android_rust.ps1). Keep SDKs and dependency
caches outside the tracked source tree.

Install Linux Rust, Flutter, JDK, Android SDK, PowerShell 7 and the pinned static
tools. Linux preview builds need Clang, CMake, Ninja, pkg-config and GTK 3
development headers. Native editing uses Neovim 0.11+ with Rust Analyzer, Dart's
language server and Ruff; no editor plugin downloads are required. Linux Node/npm
and GitHub CLI are convenience tools.

Create the ignored `apps/usque_gui/android/local.properties` with your own
absolute Linux SDK paths:

```properties
flutter.sdk=/absolute/path/to/flutter
sdk.dir=/absolute/path/to/android-sdk
```

Put the other Linux executables on PATH. Set `JAVA_HOME`, `ANDROID_SDK_ROOT`,
`LIBCLANG_PATH` and any custom Cargo, Pub or Gradle cache paths as needed.
An optional, ignored `.toolchains/env.sh` can load these local settings. The
wrapper loads it when present and removes Windows drive entries from its own
PATH without changing WSL settings.

From the repository root, check the Flutter pin and executable locations:

```shell
bash tool/dev.sh doctor
```

The wrapper resolves Flutter from `local.properties` and rejects a version or
revision mismatch before resolving packages. It rejects Windows executables and
SDKs on mounted Windows drives. Missing tools and failed commands return errors.

Git authentication belongs to your Linux environment. GitHub CLI requires your
own interactive login before GitHub operations:

```shell
gh auth login --hostname github.com --git-protocol https
gh auth setup-git
```

Do not copy Windows credentials or put tokens in repository configuration.

## Preview the interface / 界面预览与热重载

```shell
bash tool/dev.sh preview
```

This opens **Usque UI preview**. The toolbar identifies simulated data and lets
you choose **Disconnected**, **Connecting**, **Connected**, **Reconnecting**
or **Connection error**. Connected mode generates sample traffic for the charts. Use
**Restart onboarding** to inspect first-launch screens and **Reset preview**
to restore Home with the sample account. Resize the window to inspect narrow
layouts; use the application's Settings to change theme and language. The toolbar
follows the selected language, including right-to-left text direction.

在终端按 `r` 热重载、`R` 热重启、`q` 退出。顶部可切换连接状态，重新开始首次引导，
或恢复示例账号。工具栏会跟随应用设置中的语言及文字方向。账号、偏好和设置
只保存在当前进程内，重启后恢复初始数据。

The dedicated entry point is `lib/main_preview.dart`. The Linux runner accepts
debug builds only; the entry point also rejects non-Linux and non-debug execution.
The production entry point refuses to construct a Linux engine. Windows and
Android production entry points do not import the preview.

The simulated engine has no native transport, account registration requests,
VPN session, proxy listeners or route/DNS/firewall/system-proxy changes. Update
downloads, package installation, cache cleanup, secret exports, native file
imports and GEO downloads are unavailable. Preferences and reset operations
use an in-memory store. Legal links explicitly opened by the user can still
open the system browser. Preview results do not validate real networking,
credentials, platform permissions, cleanup or update behavior.

Compile without opening a window:

```shell
bash tool/dev.sh build-preview
```

Equivalent commands from `apps/usque_gui`, using its verified Flutter SDK:

```shell
flutter pub get --enforce-lockfile
flutter build linux --debug --no-pub -t lib/main_preview.dart
flutter run -d linux --debug --no-pub -t lib/main_preview.dart
```

WSL GUI previews need a working WSLg display; headless hosts can compile and run
widget tests. Linux rendering does not replace exact Windows golden comparisons.
Do not regenerate Windows baselines on Linux. This runner does not add a
supported Linux VPN product or a Linux release artifact.

## Edit and check / 编辑与检查

```shell
bash tool/dev.sh edit apps/usque_gui/lib/main_preview.dart
bash tool/dev.sh check rust
bash tool/dev.sh check flutter
bash tool/dev.sh check android
bash tool/dev.sh check python
bash tool/dev.sh check static
```

`edit` opens native Neovim with the checked-in [configuration](../tool/neovim.lua).
Use `gd` for definition, `gr` for references, `K` for hover, Space `rn` for rename,
Space `ca` for code actions, Space `f` for formatting and Space `e` for diagnostics.
Completion appears as you type; Ctrl-Space requests it explicitly. Rust Analyzer
passes `--locked` to Cargo. Ruff provides Python diagnostics and formatting.

Scopes run the corresponding [Contributing](../CONTRIBUTING.md) commands.
`check static` includes the aggregate source checker, repository policy,
actionlint and whitespace checks. It does not replace tests or platform builds.
`check all` runs all Linux scopes; overlapping checks are intentional. Failed
commands stop the selected scope and preserve their exit status. Linux Flutter
tests exclude `golden`; the Windows golden job remains required.

For preview changes, run Flutter checks and `build-preview`. CI also compiles
the Linux debug preview. Flutter changes still require the complete Windows
Rust/Flutter matrix. Finish safe Linux work when other environments cannot run
locally and record unavailable checks as `not_run`; never substitute a Linux
pass for them.

Android checks compile or analyze code without installing an APK. These tools
do not install or start an Android emulator. Device interaction and VPN lifecycle
testing remain subject to Contributing's dedicated-device/isolation rules.

## Keep local files out of Git / 本地文件忽略规则

Commit preview sources, the Linux runner, shared tooling, tests and guides.
Keep these ignored: `.toolchains` (directory or symlink), `local.properties`,
JNI libraries, Flutter/Gradle/Rust build/cache directories, Linux Flutter
ephemeral files, Neovim logs/swap files, diagnostics and signing material.
Review `git status --short` before committing. The root `AGENTS.md` remains
local and ignored; shared instructions live in tracked documentation.
