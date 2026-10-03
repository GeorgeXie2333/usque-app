# Saving and applying network settings

Settings can be saved without changing the current connection. The API reports
saving and application separately, so the GUI can show whether a change is saved,
active, waiting for a later connection, or awaiting confirmation.

## Automatic endpoints / 自动选择端点

Open **Settings → Advanced network settings → Endpoint selection**, choose
**Automatic** or **Custom**, then **Apply changes**. New installations and staged
network resets use Automatic selection. Schema 20 preserves existing addresses
in Custom mode. Switching the picker keeps custom drafts; automatic saves retain
the previously saved address pair. Save new address drafts while Custom is
selected. Port and SNI remain editable in both modes; L4 retains its
identity-derived SNI. Zero Trust uses the
registration-owned addresses and has no Consumer pool picker.

Custom accepts numeric IPv4/IPv6 addresses without a Consumer or organization
prefix restriction. Only the active Custom pair is checked for VPN DNS conflicts;
Automatic ignores the saved custom pair, including organization-range addresses.
Enabling the VPN output from an Automatic proxy-only connection reconnects the
underlay so its sockets acquire the new VPN operation's exact protection leases.
Startup retries temporary capability-query failures before automatic connection.
The endpoint picker refreshes when capabilities arrive without replacing drafts.

打开 **设置 → 高级网络设置 → 端点选择**，选择 **自动选择** 或 **自定义**，再点击
**应用修改**。新安装和恢复默认使用自动选择；升级保留已有自定义端点。切换模式
会保留手动地址草稿；自动模式应用修改时保留原来已保存的地址。要保存新的手动
地址，请在自定义模式应用。端口和 SNI 在两种模式都可修改，组织账号仍使用注册地址。

自定义端点可填写任意合法 IPv4/IPv6 地址，不限制所属网段。自动模式忽略已保存的
自定义地址；仅对当前生效的自定义端点检查 VPN DNS 冲突。
从自动端点的代理连接开启系统 VPN 时会重新连接，以取得新 VPN 会话的端点保护。
启动时能力查询暂时失败会自动重试。
能力信息返回后端点选择控件立即刷新，并保留未应用的草稿。

| Transport | Free IPv4 | Plus IPv4 | Free IPv6 | Plus IPv6 |
| --- | --- | --- | --- | --- |
| H3 | `162.159.198.1`, `.2`; `162.159.199.1`, `.2` | `162.159.199.1`, `.2` | `2606:4700:103::1`, `::2`; `2606:4700:104::1`, `::2` | `2606:4700:104::1`, `::2` |
| H2 | `162.159.198.0/24`, `162.159.199.0/24` | `162.159.199.0/24` | `2606:4700:103::/48`, `2606:4700:104::/48` | `2606:4700:104::/48` |

Pool eligibility uses authenticated Consumer entitlement, not the raw API
`warp_plus` boolean. Unknown legacy entitlement uses the Plus-compatible common
pool. Custom mode permits manual overrides. Automatic candidates exclude tunnel
DNS addresses and unavailable endpoint families. The correct IPv6 prefix is
`2606:4700`; H2 range support is based on
[published experimental samples](https://github.com/vernette/warpscout/blob/master/masque.go#L65-L82),
not a guarantee that every address accepts every account.

H3 races all admitted addresses, preserving the preferred-family 250 ms head
start and existing 8-second startup timeout/compatibility retry. The Auto IP
policy retains its existing IPv6 preference; IPv4/IPv6-only policies strictly
filter the other family. H2 enumerates
all 256 IPv4 addresses per admitted /24 and samples 256 unique IPv6 addresses
per /48, including `::1` and `::2`. Subsequent rounds resample IPv6. Known hosts
come first; remaining addresses are shuffled. Each H2 batch admits at most ten
attempts, normally five per family, with one absolute two-second deadline from
batch start. In dual-stack batches the preferred family starts immediately and
the other starts after 250 ms, or sooner if the preferred attempts all fail.
Early failures advance immediately. A fully exhausted Free
dual-stack round has 103 batches, about 206 seconds before cleanup overhead.

The first authenticated, pinned, accepted CONNECT-IP connection wins and is
reused. L4 instead requires authenticated QUIC, H3 ALPN and peer SETTINGS;
business CONNECT requests start only after winner promotion. Losers close their
sockets before releasing protection leases. Terminal protection, identity and
pin errors keep their fail-closed handling; authenticated pin refresh remains
limited to one retry and a 60-second automatic-mode refresh budget.

Automatic startup shares a profile-derived bounded deadline across transport,
Engine, Android and IPC. The maximum underlay budget is 520 seconds; chaining
adds 180 seconds, and response delivery adds 15 seconds. Ordinary save and
status acknowledgements retain their shorter limits. Disconnect cancels startup
without replaying a timed-out mutation.

Endpoint selection uses appended EndpointSettings field 5, capability field 42
and field mask `endpoint.selection`. Absent legacy wire fields mean Custom;
unknown explicit values are rejected. Selection changes use controlled cold
reconnect. Managed-account writes preserve the shared Consumer selection.

Windows journals a canonical pool descriptor and authorizes each candidate
through an exact Engine/process/interface/generation-scoped lease. Prefixes are
validation data only: no prefix route or firewall permit is installed. Two
representative host routes retain physical-network observation. Prepared WFP
metadata is journal-owned; rollback releases dynamic leases before restoring
that metadata. Endpoint leases cannot fall through to generic direct egress.

L4 retains its two admitted/draining session slots, with at most eight startup
actors sharing one reserved slot. Each unpromoted actor admits at most 256 KiB
of cumulative UDP input before QUIC parsing. This bounds admitted startup input,
not total heap usage; existing bounded UDP queues and parser overhead remain.
Only the winner is promoted after loser shutdown and generation revalidation.

Workstation unit/widget tests and compile-only builds do not establish native
VPN cleanup or leaks. IPv6 protocol measurements and protected-environment
validation must be reported separately; unavailable environments are `not_run`.

## Entry points and ownership

- Flutter owns form drafts. NetworkSettingsController serializes submissions,
  accepts confirmed saved values, and filters replies by process epoch and
  sequence. Account writes share this submission order.
- Rust owns field validation, normalization, the durable configuration, and
  application planning. The planner compares against the session profile.
- Desktop and Android hosts own execution, session identity, cancellation,
  runtime confirmation, and platform cleanup.

The [control protocol](../proto/usque/v1/control.proto) appends request fields
41 (save) and 42 (state), response field 22, event field 24, and capability
field 25. Old fields retain their wire numbers and shapes. An engine without
the new capability cannot accept saves from the new GUI.

Each save carries an operation UUID, account UUID, non-secret profile values,
and an explicit field mask. The account must be the current edit context.
The command cannot create or rename accounts, change identity, or write
credentials. Unknown fields and managed Zero Trust endpoint edits are rejected.
Rust disables system proxy when HTTP is disabled.

Proxy listener lists retain every address and port through desktop protobuf,
Android JSON, edit-mask comparison and unrelated settings changes. Configurations
that do not fit one IPv4 and one IPv6 address with a shared port use a multiline
editor. Each line is an IP socket address; IPv6 uses brackets. The same native
limits and duplicate checks still apply. **Reset network defaults** in Advanced
settings stages the listener and DNS defaults as well as the visible transport
fields; **Apply changes** submits the complete changed-field mask and preserves
listener credentials.

代理页会完整保留每个监听地址及其端口。已有的多地址或不同端口配置按行显示，
IPv6 地址使用方括号，例如 `[::1]:1080`。高级设置的“恢复网络默认值”会暂存
监听和 DNS 默认值，点击“应用修改”后才保存；该操作保留已设置的代理用户名和密码。

Per-app policy saves return an operation-specific persistence result. The picker
keeps its draft on failure and prevents edits or duplicate submission while the
save is pending. A saved policy alone is not a runtime acknowledgement from the
Android VPN service.

Shared listener credentials use their own native transaction, negotiated by
capability field 33 (`shared_proxy_auth_application`). The GUI never follows
that transaction with a full-profile write. The native owner saves the password
before enabling the username, applies only the credential change to a running
session, and waits for the listener result. An application failure returns
`PROXY_AUTH_APPLY_FAILED` and stops the old listeners. A disconnected session
stays disconnected. A persistence error across the vault and configuration is
not a success: the connection is retired and the user must save again.

Android stores one encrypted shared password and serializes its migration,
write and runtime read with a stable file lock. Account deletion retains the
shared record; clearing credentials or all data removes it. Legacy passwords
are migrated only when they agree. Conflicts retain the old records and require
an explicit replacement. JNI reconfigure and TUN attachment receive a fresh
secret buffer, clear it after use, and never substitute the previous password.

代理凭据的“保存用户名和密码”操作会等待原生监听更新。若凭据已保存但未能生效，
应用会显示错误并停止旧连接；重新连接会读取已保存的凭据。清空用户名和密码会
移除共享凭据，删除单个账户不会移除它。

The result includes process epoch, sequence, operation ID, session ID, saved
profile, optional confirmed session profile, deferred fields, a sanitized error
code, and runtime status. The optional persisted flag means:

| persisted | Meaning |
| --- | --- |
| true | The save was acknowledged independently of runtime application. |
| absent | No durable acknowledgement is available for this operation. |

A structured rejection is a failed save. The GUI retains the draft. An
uncertain commit or lost reply is queried, never automatically replayed. If
the operation cannot be confirmed, the draft remains dirty. Applied, failed,
deferred, and unknown runtime states do not undo an acknowledged save.

The GUI tracks a failed status query separately from an unconfirmed save.
A valid query or event restores status visibility, including a query returning
the same epoch and sequence. Older sequences and retired epochs cannot clear
the warning or replace state. An uncertain save remains unknown until its
own operation ID receives a durable acknowledgement; an unrelated successful
save does not confirm it. A query failure arriving after a newer authoritative
reply cannot put the interface back into an unknown state.

Further explicit saves remain available while earlier operations are unknown.
The GUI tracks each operation independently: starting or rejecting another
save does not clear earlier warnings, and a successful save acknowledges only
itself. The latest save may succeed while the global warning remains. An
accepted durable acknowledgement is retained for its in-flight request even
if another snapshot arrives before that request times out.

## Account metadata and shared confirmations

Account renames use the appended RenameProfile request (46), advertised by
account_metadata_mutations (32). Identity provisioning for an existing account
does not upsert a network snapshot. Older engines without the rename capability
are rejected instead of falling back to a full-profile write.

ProfileList field 4 and NetworkSettingsState field 11 carry a non-secret shared
network profile without registration-owned endpoint overlays. Account removal
does not invalidate an already acknowledged shared-network save. Account mutation
arguments remain immutable while queued; readback reapplies later optimistic
metadata operations instead of overwriting them. Product creation defaults do not
override proto3 false values during decoding.

## Application rules

A hot change can update the running connection. A cold change requires a
controlled reconnect. A deferred change is saved for a later manual connection.
The shared planner decides which category each submitted field belongs to.

Schema 15 adds `data_plane` independently of the saved CONNECT-IP transport
policy. [Experimental L4](L4_PROXY.md) always uses HTTP/3, preserves the saved
H3/H2 preference and SNI, and is excluded from Auto. Data-plane switches and
L4 TUN toggles are cold changes. Old configurations remain CONNECT-IP; unknown
explicit modes are rejected. L4 capability fields are appended, never inferred
from generic HTTP/3 support.

| Situation | Result |
| --- | --- |
| Disconnected, connecting, reconnecting, disconnecting, error, or ordinary executor-busy saves | Save; wait for a manual connection. |
| Confirmed Windows session; system-proxy switch or HTTP-off proxy cleanup while its executor is busy | Save; coalesce the latest proxy switch into one bounded, session-scoped follow-up. |
| Stable; congestion control only | Save; keep the session algorithm. |
| Stable; auto-connect only | Save; no runtime action. |
| Stable; hot fields | Save; run the existing frontend/platform operation. |
| Stable; cold fields | Save; one controlled reconnect. |
| Mixed congestion control and runtime edits | Apply the requested runtime fields; keep congestion control deferred. |
| Previously deferred settings exist | Leave them deferred unless explicitly submitted or required by a dependency. |
| Partial platform failure | Retain saved settings; invalidate unconfirmed runtime configuration. |
| Platform restoration pending | Report applying; never infer applied from request acceptance. |

The [shared planner](../crates/usque-core/src/network_settings.rs) owns these
rules. The [desktop adapter](../crates/usque-engine/src/network_settings.rs)
reserves an immediately available lifecycle executor before the short save
transaction. Ordinary busy-executor saves do not acquire an application queue
entry. The Windows system-proxy switch is a narrow exception: one worker waits
up to 30 seconds for the executor and applies only the latest proxy flag to the
same confirmed account and session. It never pulls in unrelated deferred
settings. Closing HTTP also prevents a pending switch from enabling the proxy;
when the executor is busy, this follow-up applies only the dependent proxy
cleanup while the HTTP setting remains subject to the normal deferred rule.
If a short runtime/state read is busy, the captured confirmed session permits
reservation, but execution must recheck its connection phase and identity.
Disconnect intent, account/session replacement, failed runtime confirmation,
or unhealthy transport cancels this follow-up. A timeout remains deferred;
an application failure remains a failure. Runtime execution rechecks session
identity and disconnect intent.

Disabling HTTP clears its dependent system-proxy flag through the frontend hot
update, preserving the other outputs and their underlying session. Combining
that edit with a cold transport or routing change still requires reconnecting.

Automatic recovery uses the captured session profile. Manual connect and Retry
read saved settings. A cold application failure can attempt one runtime
restoration using the previous session, without changing the durable file.
Android keeps the confirmed recovery profile separate from an in-progress
settings target and confirms the target after native and platform completion.

Android startup preferences, recovery records and per-app policy use separate
versioned files. Every read and field patch takes the same stable file lock;
writers sync a temporary file and atomically replace the data file. Old
preferences are imported once, and clearing persists an empty record to prevent
re-import. Per-app notifications retain the latest revision across service
binding. Boot reads the current active account and its current auto-connect
flag from the native catalogue after unlock, within a bounded receiver task.
The last connected profile remains exclusively a recovery input.

Connection and retry requests retain the intent that queued them. A later
disconnect invalidates unsent requests, including those waiting for process
startup, profile validation or Android service binding. Transport failures may
replay read-only queries, but never a mutation whose reply could have been lost.
Automatic startup waits for the saved catalogue, status and required capability
information; a failed catalogue load is retried. Identity completion reconnects
only while the original account and user intent still match, and closing its
dialog cancels that continuation. Windows event-pipe EOF is reported to Dart so
status polling remains available even when quality sampling is paused.

Clear All Data retires pending GUI reads and mutations before the native wipe.
It resets shared network settings, diagnostics, timeline, quality history and
update state, then opens a fresh event subscription. A failed wipe reloads
authoritative state and remains a failure. Windows resets the settings source
epoch and diagnostic session; a diagnostic worker is bound to its original
session ID so it cannot finish a later session. Android drains older settings
writes before acknowledging the stop, resets native settings and diagnostic
state, and releases the service binding after the local wipe.

The updater holds file ownership through download, verification, publication
and cleanup. A late publication after reset is discarded before a later download
can reuse the same path. Stream, HTTP and file cleanup run independently; cleanup
errors cannot keep the operation busy or replace the primary failure. Native
package path, size, digest and signature validation are unchanged.

Android reserves a fresh application token and session generation before
dispatching persistence. Its lifecycle is idle, persisting, reconfiguring,
awaiting observation, then idle. Snapshots cannot finish an application while
persistence or the runtime reply is pending. Terminal paths release the whole
reservation, and late or duplicate callbacks cannot finish a newer save. Only
that application's controlled cold reconnect carries its token into the new
generation; unrelated session changes, disconnect, and destruction retire it.
Cancelling application does not withdraw an already durable save acknowledgement.

## Persistence and safety

[ConfigStore](../crates/usque-core/src/storage.rs) provides a short
read/modify/validate/atomic-save transaction under a stable sidecar file lock.
The lock is on the sidecar inode, not on the replaced JSON inode. Android's
legacy profile commands also use that lock; GEO downloads run outside it.
Desktop account commits preserve the latest network settings. Credential I/O,
network requests, runtime shutdown, and TUN operations stay outside the store
transaction.

Settings operation tracking does not add a configuration schema version; the
current schema is 18. Epochs, sequences, operation IDs, and application
state are in memory and do not create a durable operation log. Passwords are
removed from published profiles. This change does not relax Kill Switch, TUN retention,
Agent journal, privileged cleanup, or isolated-runner requirements.

## Verification

Shared policy cases are exercised in the core planner, desktop adapter, and
Android adapter tests. They cover transitional/busy saves, mixed deferred
fields, invalid masks, dependency normalization, concurrent persistence, and
partial runtime failure. Flutter and Binder tests cover acknowledgement
ordering, duplicate replies, lost replies, draft retention, and capability
gating. IPC tests pin the appended wire numbers and reject malformed framing.

Run the applicable matrix in [CONTRIBUTING](../CONTRIBUTING.md). The two
congestion-control visual fixtures use a confirmed saved/applied state pair;
their save bar now displays the unified deferred message in English and
Simplified Chinese, including the 200% TV text fixture. Existing failure
artifacts are not baseline inputs.

Real VPN lifecycle, WFP, route/DNS/system-proxy restoration, connected
uninstall, crash recovery, Android device lifecycle, and leak observation
require the isolated environments in [Contributing](../CONTRIBUTING.md#development-machines). Workstation
unit and compile checks do not establish those results. No MSI or release
APK is part of this change.

### Review-fix validation, 2026-09-11

The operation-tracking fixes add Flutter cases for consecutive unknown saves,
independent acknowledgement, query warnings, stale replies, and an early
acknowledgement surviving a newer snapshot. Pure Kotlin tracker cases cover
snapshot interleaving, terminal cleanup, duplicate callbacks, cancellation,
and controlled session migration without starting JNI or a VPN service.
Test execution and protected-environment validation for these fixes are
`not_run`, as requested. The historical results below do not validate these fixes.

### Workstation validation, 2026-09-08

The following commands completed successfully on Windows using Rust 1.97.1,
Flutter 3.44.7 (84fc5cbb223bc12f83d65b647ff8a56caf779ffd), the pinned Android
NDK/CMake, and Buf 1.72.0. Commands shown as `flutter` and `dart` used the SDK
resolved from Android local.properties, not a global installation.

From the repository root:

```powershell
cargo fmt --all --check
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
& .\tool\build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy
buf lint
buf format --exit-code --diff
buf breaking --against '.git#ref=5a03d1b91528c1556f5160fe74b976f9fcf0f0ca' --against-config buf.yaml
```

The Buf reference is the inspected origin/main baseline for this worktree;
CI must still check the eventual PR target SHA. An intermediate Rust run
failed when an existing DNS loopback test could not bind a TCP port (Windows
10013); the complete helper test rerun passed without changing that test.

From apps/usque_gui:

```powershell
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub --reporter expanded
flutter build apk --debug --config-only --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
```

All 405 Flutter tests passed, including exact Windows bitmap comparisons.
The six review-fix regressions cover query/event recovery, stale replies,
out-of-order query failures, operation-specific durable confirmation, and a
confirmation event arriving before the save response times out.
The Windows GUI release build completed successfully without launching it.
The changed phone-light and TV-dark congestion fixtures were rendered and
visually reviewed in English and Chinese. The new save bar interaction test
covers both themes, both languages, 200% text, and keyboard focus/activation.

From apps/usque_gui/android:

```powershell
.\gradlew.bat --no-daemon :app:ktlintCheck :app:testDebugUnitTest :app:lintDebug
```

The Gradle checks also compiled debug JNI libraries for all three configured
ABIs. Those generated files remain outside version control. No release APK
was built or installed.

The frozen oracle passed `go mod verify` and `go test ./...` from oracle/go,
and `python tool/verify_oracle_archive.py` from the root (verified Python
3.12.14). The archived source was unchanged.
`python tool/check_repository_policy.py` and `git diff --check` also passed.

| Protected environment | Result |
| --- | --- |
| usque-snapshot-vm | not_run: no isolated snapshot/management channel supplied |
| usque-android-device | not_run: no dedicated device or isolated emulator supplied |
| usque-network-observer | not_run: no isolated observer supplied |
| usque-performance-lab | not_run: no controlled performance runner supplied |

These missing environments provide no lifecycle, cleanup, leak, or performance
pass. They do not block the compile-only workstation checks or change the
repository's optional protected-runner publication policy.

## Disable application QUIC / 禁用应用 QUIC

Open **Settings → Advanced network settings → Routing & protection**, change **Disable QUIC**,
then choose **Apply changes**. The default is off and the setting is shared by
all accounts. Resetting advanced settings turns it off in the draft. Older
engines without the capability disable the control and show an update notice.

打开 **设置 → 高级网络设置 → 路由与保护**，调整 **禁用 QUIC** 后点击 **应用修改**。
默认关闭，所有账号共用；重置高级设置会在草稿中关闭它。旧引擎不支持时，开关
不可操作并提示更新。稳定连接中单独应用此设置无需重连，现有 GEO 直连连接保持
可用；连接中、断开或执行器忙时，界面会显示已保存并延后生效。

HTTP/SOCKS5 chain exits apply this policy automatically at the final frontend.
While either exit is enabled, the control displays the effective enabled state
and is read-only with **Managed by this connection**. This does not overwrite
the manual preference or a pending manual draft; leaving these exits restores
the preference. Other chain protocols retain the manual policy.

HTTP/SOCKS5 链式出口会自动在最终出口启用此策略。启用这两类出口时，开关显示
已启用并只读，说明为“由当前连接自动管理”。原有手动偏好及未保存草稿均保留，
退出这两类出口后恢复；其他链式协议继续遵循手动设置。

The rule uses the same UDP destination-port 443 match as
[Bettbox](https://github.com/appshubcc/Bettbox/blob/deef948c7291aca75a5bc59bfc00badcb3907630/lib/state.dart#L972),
with Usque's GEO direct routing taking precedence. Direct UDP/443 remains
available for every configured GEO country. If a direct attempt falls back to
the tunnel, that fallback is filtered. Non-QUIC UDP on port 443 is also blocked;
QUIC on other ports is outside this setting. TCP/443, other UDP, and Usque's
own HTTP/3 transport are unaffected. Traffic excluded from Usque's capture is
outside the setting's scope.

此功能按 UDP 目标端口 443 匹配，并优先保留 GEO 直连。所有已配置地区的直连
QUIC 均不受拦截；直连失败后回退到隧道的 UDP/443 会被拦截。同端口的其他 UDP
协议也会受影响，其他端口的 QUIC 不在范围内。Usque 自身的 HTTP/3 传输不受影响。

Schema 16 adds `disable_quic`, defaulting to false when absent. The control
protocol appends `Profile.disable_quic` at field 21 and
`Capabilities.application_quic_blocking` at field 31. Android carries the same
fields over its existing JSON/Binder boundary.

`HotTrafficPolicy` changes a shared atomic policy in the current application
runtime. Existing TUN flows and SOCKS5 associations read it for subsequent
tunnel UDP/443 packets in both directions; direct replies are explicitly
exempt. Fragment associations retain their port classification, and unknown
non-initial fragments remain unattributed and are dropped. This does not
retract packets already handed to the transport before the update.

The pure policy update preserves session identity, listeners, TUN attachment,
GEO sockets, and platform leases. Other edits in the same submission keep
their existing application classification. Only successful application updates
the confirmed recovery profile. VPN Gate applies the policy at the final
application frontend; L4 retains its existing UDP limitations. There are no
new packet logs, DNS probes, firewall rules, or persistent traffic records.

The stream TUN bridge shares one 32-errors-per-second ICMP budget across its
packet pump and all UDP workers. Rejections, unsupported associations and
definite relay failures can provide feedback without waiting for a silent UDP
timeout. Error quotes are bounded and charged to the existing admission budget;
normal cancellation discards them without generating errors. A blocked QUIC
source does not allocate a UDP worker or consume a shared TCP slot, and blocked
destinations do not consume UDP target slots. This does not infer end-to-end UDP
availability from a successful send or add a blackhole probe.

Regression tests use fake engines, in-memory packet channels, and loopback
UDP endpoints. Real VPN lifecycle and external leak validation still require
the isolated environments described above.

### QUIC workstation validation, 2026-09-17

These results describe the uncommitted QUIC working tree based on
`a89caa1b7b3085594995f0c8481258cf8e0a7870`; they do not identify an immutable
release candidate. Rust 1.97.1, Flutter 3.44.7 at the CI-pinned commit, Buf
1.72.0, and the pinned Android NDK/CMake were used. The local Flutter SDK
selection was temporarily changed for validation and restored afterward.

From the repository root:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | passed |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | 1,140 passed; 5 existing ignored tests |
| `& .\tool\build_windows_rust_release.ps1 -Variant x64-v2` | passed; compile only |
| `& ./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed |
| `buf lint` | passed |
| `buf format --exit-code --diff` | passed |
| `buf breaking --against '.git#ref=a89caa1b7b3085594995f0c8481258cf8e0a7870' --against-config buf.yaml` | passed; CI still checks its PR target |
| `python tool/check_repository_policy.py` | passed |
| `git diff --check` | passed |
| `pwsh -NoProfile -File tool/check_source.ps1` | failed: local software restriction policy blocked PSScriptAnalyzer 1.25.0 format-data import |

The aggregate ran in the helper-initialized native environment. Its Rust,
Dart, Flutter, Kotlin and Ruff stages passed before the module-import failure.
PSScriptAnalyzer did not run; no local policy or check was weakened. Buf was
run separately and passed.

From `apps/usque_gui`, using the verified SDK's `flutter` and `dart`:

| Command | Result |
| --- | --- |
| `flutter pub get --enforce-lockfile` | passed |
| `dart format --output=none --set-exit-if-changed lib test` | passed |
| `flutter analyze --no-pub` | passed |
| `flutter test --no-pub` | 524 passed |
| `flutter test --no-pub --tags golden` | 42 passed, including the final QUIC phone/TV fixtures |
| `& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .` | passed |
| `flutter build windows --release --no-pub` | passed; compile only |
| `flutter build apk --debug --config-only --no-pub` | passed; configuration only |

The two new QUIC goldens were generated on Windows with the pinned SDK and
visually reviewed, including Chinese dark mode at 200% text. Existing screenshot
baselines were not regenerated. The widget cases cover Enter/Space activation,
the exact `disable_quic` edit mask, failed-save draft retention and unsupported
engines. Transport cases cover both address families, live GEO sockets,
fallback filtering, existing tunnel flows, and reused fragment identifiers.

From `apps/usque_gui/android`:

| Command | Result |
| --- | --- |
| `.\gradlew.bat --no-daemon :app:ktlintCheck` | passed |
| `.\gradlew.bat --no-daemon :app:testDebugUnitTest :app:lintDebug` | 200 unit tests passed; lint passed; debug JNI compiled for all three ABIs |

Snapshot-VM, dedicated Android-device, external network-observer and
performance-lab validation: **not_run** (no isolated environments supplied).
No MSI, release APK, installation, live VPN session, or publication was part
of this validation.

## Custom VPN bypass settings

`bypass_domains` is a shared network field, defaults to an empty list, and uses
canonical IDNA domain suffixes. Schema 19 preserves earlier CIDRs and country
selections. Profile protobuf field 23 carries domains; capability field 41
(`custom_bypass`) gates custom target editing in Flutter. Existing CIDRs retain
field 10 (`split_exclusions`) and Android JSON key `bypass_cidrs`.

The page patches country selection, address exclusions and domains together;
validation failure saves none of them. Core validates domain syntax and bounds,
normalizes domain/network duplicates and retains VPN DNS exclusion checks.

Domain names may contain up to 253 Unicode code points before IDNA normalization,
excluding one optional trailing dot. Core also enforces the normalized ASCII DNS
name and label bounds. Android checks the same input length separately from its
CIDR parser, so valid long names, Unicode names and trailing-dot input can reach
the shared validator during connection and reconfiguration. Its early parser
does not implement a separate IDNA policy or change the existing CIDR limit.

Domain changes require the existing cold-reconfigure path. Split DNS activates
for country or custom domain rules, independently of the GEO cache. The direct
policy also evaluates explicit address rules at HTTP/SOCKS5 IP targets. The
Windows exact-target leases and Android protected sockets are unchanged; failed
GEO loading cannot be masked by a custom-rule hit. DNS hints remain bounded,
non-persistent, TTL-limited and scoped to the current network generation.

No new logs contain custom domains or target addresses. Workstation fake-engine,
loopback and compile tests do not establish VPN cleanup or leak prevention;
those require the isolated environments in CONTRIBUTING.md.

## Protected Windows operation replacement

The internal Agent capability `protected_tunnel_replacement` supports changing
an operation while its applied Kill Switch is enabled. `ReplaceTunnel` checks
the authenticated owner, device lease and journal generation, then installs a
persistent WFP guard before restoring the old operation and preparing the new
one. The guard has its own provider and sublayer. Journal schema 5 keeps its
write-ahead record outside ordinary connection cleanup steps; schemas 2–4
migrate without inventing a replacement record.

The bridge preserves the intersection of the old and requested static LAN/CIDR
bypasses. Retargeting a failed replacement can only narrow that bridge's existing
exceptions. Exact Engine bootstrap targets remain available, including bounded
automatic endpoint leases mirrored into both WFP sublayers. The bridge grants
no general application, business-flow or TUN-interface exemption. Saved bypass
and Kill Switch preferences are unchanged; the complete requested policy takes
effect after the target is committed.

`CommitTunnel` verifies target packet attachment and native network protection
before removing the bridge. Failed preparation and ordinary rollback, recovery
or lease expiry cannot remove it. The Engine must keep new business admission
closed until commit succeeds or an authenticated state query confirms completion.
`AbortReplacement` is the explicit-disconnect path. Both commit and abort retain
enough journal state to recover a crash between native guard removal and the
completion write. A changed retry uses a fresh target operation and an exact
current generation; a matching retry accepts refreshed process/device ownership.

Malformed or unsupported journals never trigger automatic WFP removal: the old
operation may still be the only protection before the bridge installation.
Explicit elevated recovery and emergency removal remain the release boundary.
An older Agent must reject the new capability/schema rather than silently use
an unprotected disconnect/reconnect sequence.

Fake-backend lifecycle and pure WFP-rule tests exercise ordering, ownership,
retargeting, readback and persistence failures without changing host networking.
Native filter arbitration, retaining an automatic endpoint's normal permit while
commit removes its guard mirror, atomic retarget rollback, and independently
observed IPv4/IPv6 traffic require isolated Windows and network-observer runners;
these checks are **not_run** on the development machine.

The Engine retains a failed HTTP/SOCKS VPN owner while its applied Kill Switch
is on. It stops final admission and packet forwarding without calling ordinary
disconnect or dropping the Agent lease. Retry rebuilds a stopped WARP runtime;
changing accounts or settings that need a new transaction uses the protected
replacement capability. Live HTTP/SOCKS VPN sessions also use this handoff with
Kill Switch off: that preference controls terminal-failure cleanup, not a gap
between two running connections. A saved preference alone cannot release the old policy.
Explicit Disconnect remains the release action, and terminal failures with an
applied disabled Kill Switch may restore ordinary networking after safe cleanup.
Selecting a proxy-only account explicitly removes VPN capture; its owned native
cleanup must finish before that account's local proxy startup can proceed.

Protected startup reuses verified numeric registration API candidates, or
refreshes the fixed control host through a still-running WARP network. It never
temporarily enables system DNS to make a guarded retry succeed. Unknown RPC
outcomes are reconciled against the authenticated Agent replacement status;
older Agents without this capability keep their existing protection and return
an update-required error rather than disconnecting first.

Explicit Disconnect keeps its exact runtime cleanup owner until native cleanup
is confirmed. A failed cleanup remains retryable through Disconnect and prevents
a new session from bypassing unfinished restoration. A lost replacement-abort
reply is accepted as complete only after the authenticated Agent reports that
same target aborted and all ordinary state clean; unrelated operations and
unknown state never authorize removal of another session's protection.
