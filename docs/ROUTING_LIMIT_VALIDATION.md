# Routing rule limit validation — 2026-11-25

This record covers raising the custom routing limit from 256 to 4096 rules per
kind. Workstation checks ran on a Linux (WSL) development machine against the
working tree later committed as `0767dbcf` and `ca09f56a` on top of `dev` at
`0d0ee869`. Emulator acceptance ran on GitHub-hosted runners at `087f7931`,
which adds only harness commits. It is not release-candidate or
protected-runner evidence. See [Routing](ROUTING.md) for the behavior and
[Contributing](../CONTRIBUTING.md) for the required checks.

## Scope

- Core: `MAX_ROUTING_RULES_PER_KIND` 4096 and `MAX_ROUTING_TARGET_BYTES`
  384 KiB over the normalized targets; configuration schema 29 with a no-op
  migration. `AppConfig::validate` normalizes the device-wide rules once
  instead of once per account.
- Matching: `RoutingSnapshot` indexes domains by exact suffix and CIDRs by
  prefix length (one hash table per length), replacing linear scans.
- Capabilities: `max_routing_rules_per_kind = 58` and
  `max_routing_target_bytes = 59` on desktop; the same JSON keys on Android,
  forwarded to Flutter only when in range. A GUI without them keeps 256 per
  kind and no budget.
- Desktop transport: profile lists omit per-account `routing`; the rules
  travel once in `shared_network_profile`, which the GUI already hydrates
  every account from. `UpsertProfile` is only sent with a hydrated profile.
- Android transport: profiles and settings results over 32 Ki characters
  cross the UI/`:vpn` boundary as single-use files in `noBackupFilesDir`,
  verified by SHA-256 and deleted after one read. Profile limits rise to
  2 MiB (Rust and Kotlin) and settings results to 8 Mi characters. The native
  account catalog is unchanged because Kotlin connect paths read rules from it
  in-process.
- Flutter: validation checks the reported limits and finds overlaps through
  parent domains and shorter prefixes instead of all pairs; the settings page
  caches validation and compares drafts by value. Lists over 50 rules gain a
  search field and show 100 rules at a time; a rule that blocks saving stays
  visible. Pastes are bounded by the reported limits.

## Measured sizes for the largest rule set

`largest_routing_settings()` fills both kinds and spends the whole target
budget (4096 IPv6 /128 networks of 43 bytes and 4096 domains of 53 bytes).

| Payload | Size | Bound |
| --- | --- | --- |
| One JSON copy of the rules | under 1,250,000 bytes (test bound) | — |
| Android profile JSON | 1,140,087 bytes | 2 MiB |
| Android settings result (stored, applied, shared profiles) | about 3.42 MB | 8 Mi characters |
| Desktop `NetworkSettingsState`, `ProfileList` with 128 accounts, `Profile`, `UpsertProfile` | each encodes | 4 MiB frame |
| Android recovery policy with two largest profiles | under 3 MiB (test bound) | 4 MiB |

## Commands and results

Commands start at the repository root. Flutter is the pinned 3.44.7
(`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`); Rust is 1.97.1.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo test --workspace --all-targets --locked` | Passed: 2,236 tests |
| `buf lint` and `buf format --exit-code --diff` | Passed |
| `bash tool/dev.sh check static` | Passed |
| `bash tool/dev.sh check flutter` | Passed: analyze clean; 1,747 tests, 10 skipped by existing conditions; 40 Linux golden tests |
| `bash tool/dev.sh check android` | Passed: JNI clippy (arm64-v8a), ktlint, 618 Kotlin unit tests, Android lint |
| Windows golden comparison (`flutter test --no-pub --tags golden`, Windows SDK 3.44.7 from a temporary copy, no `--update-goldens`) | Passed: 106 tests, 47 skipped by existing conditions; no baseline changed |
| `python tool/check_repository_policy.py`, `git diff --check` | Passed |

Notable tests: the property test comparing the index with the reference
specificity rules (a reversed prefix order makes it fail); 8192-rule build and
20,000 lookups; per-kind and byte-budget boundaries; desktop frame sizes;
Android profile, request and settings result sizes; Kotlin handoff round trip,
tampering, path traversal, stale sweep and recovery-store size; Dart overlap
notices matching the pairwise definition over 200 random lists; a full rule
set validating in under five seconds; capability decoding on both platforms;
editor search, paging, error visibility and a 600-line paste.

## Hosted emulator acceptance

Run [45567942499](https://github.com/GeorgeXie2333/usque-app/actions/runs/45567942499)
at `087f7931` passed on both devices with no connection retries. After the
per-app phases recorded in
[ANDROID_PER_APP_VALIDATION.md](ANDROID_PER_APP_VALIDATION.md), which passed
again, `tool/android_per_app_e2e.py` stops the debug app, stores 8192 rules
(393,180 target bytes, configuration 1,140,453 bytes) through `run-as`, and
reconnects. The last rule is `DIRECT www.cloudflare.com`; every other rule
rejects a documentation address or a `.test` name.

| Phase | Phone (API 34 `google_apis`) | TV (API 36 `android-tv`) |
| --- | --- | --- |
| 5: connect with the largest rule set; probe inside the VPN | `www.cloudflare.com` `warp=off` (DIRECT), `cloudflare.com` `warp=on` | Same |
| 6: search the editor, delete the DIRECT rule, Apply changes | `www.cloudflare.com` `warp=on`; no handoff file left | Same |

Phase 5 shows the complete list reached the VPN process through the Intent
handoff. Phase 6 sends the settings request and receives the settings result
and events through the Messenger handoff, applies the edited rules live, and
leaves no file behind. Traces use IPv4: the emulators advertise an IPv6
default route but hosted runners have no IPv6 egress. The TV run uses taps for
the editor; D-pad use of the editor is covered by widget tests.

Earlier runs on this branch, each fixed in the harness only:

| Run | Commit | Result and cause |
| --- | --- | --- |
| [45559897787](https://github.com/GeorgeXie2333/usque-app/actions/runs/45559897787) | `2ec5b1d6` | Failed: `1.1.1.1` redirects the trace to HTTPS; the phone search field has no label; TV typing sent blind deletes |
| [45563293705](https://github.com/GeorgeXie2333/usque-app/actions/runs/45563293705) | `72d558cb` | Failed: the save bar says Apply changes; the direct trace timed out over IPv6 |
| [45565927700](https://github.com/GeorgeXie2333/usque-app/actions/runs/45565927700) | `59b51191` | Phone passed; TV per-app phase read a busy screen as a finished edit |

## Not run

- Physical Android devices (`usque-android-device`): the boot receiver and
  Always-on restarts with the largest rule set, low-memory TV hardware and a
  physical remote. The emulator acceptance covers the connect, save and
  settings-event handoffs only.
- Windows Rust clippy and tests (`tool/build_windows_rust_release.ps1`):
  not authorized for this task.
- `usque-performance-lab`: memory and CPU with 8192 rules on low-end Android
  TV and desktop, including the settings result size on each event.
- `usque-network-observer`: DIRECT/REJECT behavior with a maximum rule set.
- macOS native validation.

## 中文摘要

自定义分流规则上限由每类 256 条提高到 4096 条，规范化目标总长度不超过 384 KiB，配置
schema 升至 29。规则匹配改为查表；桌面账户列表只携带一份规则；Android 大于 32 Ki 字符
的配置和设置结果改用一次性、SHA-256 校验的私有文件跨进程传递。上表列出已运行的检查和
结果。GitHub 托管模拟器（手机 API 34、TV API 36）运行 45567942499 通过：写入 8192 条
规则后连接，末尾的 DIRECT 规则生效、其余流量走 WARP；在编辑器中搜索、删除并应用后恢复
WARP，且无残留交接文件。实机（开机自启、Always-on、低内存 TV、遥控器）、Windows Rust、
性能实验室、网络观测和 macOS 验证未运行。
