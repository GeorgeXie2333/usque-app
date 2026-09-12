# Issue #66 修复与验证记录

日期：2026-09-12。范围：恢复入口修复、只读诊断和回归验证。

## 源码基线

- 分支：`codex/vpngate-exit`。
- 开始时 HEAD：`b2051fdbdc2b8bffed6e1e1ee15001fdce08911a`，工作区无修改。
- 本次验证针对该 HEAD 上的修复补丁，在提交前完成，原有 VPN Gate 提交保留。
- 当前分支没有 PR；Buf 使用已核实的远端目标 `main` 提交
  `80ef61ad66b9f65f92f2a326e1ecf1f01f058096`，采用 CI 的比较方式。
- 本记录是本次源码补丁的本地验证记录，不是正式发行候选包或隔离环境证据。

## 原因与实现

旧 Engine 仅在 Retry 且上次错误为恢复错误时重启已耗尽的恢复。
主连接入口及刚重启、没有错误快照的 Engine 无法触发该流程，可能不断返回
Agent 已缓存的终止错误。错误同时从状态更新和 RPC 返回处记录，进一步造成
重复日志。它们不能证明每条错误对应一次新的 Windows 检测。

现在需要 Agent 的 Connect/Retry 共用实时预检。Waiting/Running 建立恢复观察，
Exhausted 使用当前 operation ID 和 generation 最多重启一次，Blocked 保留错误，
Clean 继续连接。后台连接不刷新自动恢复预算。取消后的清理可以完成，但不能
启动新连接或恢复过期意图。VPN Gate 原地重试保留既有分支。

适配器清理成功标准未修改。现有 Agent 收尾仍要求适配器是唯一未完成步骤、
接口与 PnP 均确认不存在、调用者/会话/操作/generation 验证通过，并成功保存
Clean。未知检测、身份冲突、其他未完成步骤及保存失败均不能放行。

`PlatformState.recovery_diagnostics = 18` 为可选扩展，协议版本仍为 3。
新增 `windows-recovery.json`（schema 1），分别保存本次采样、最多 32 条历史有效
事件及自动恢复状态。历史保留原始时间与 generation；当前采样超时、忙碌或
generation 变化不解释为不存在。Agent 单个采样工作线程在超时后继续持有许可，
直至系统调用返回；Engine 最多等待两秒。

诊断只读访问正在运行的 Agent，不打开 Wintun、不启动服务、不执行恢复。
两端按字段白名单重建输出；没有完整 journal、原始错误文本、适配器名称、
GUID/LUID、SID、地址或凭据。旧 Agent 缺少扩展仍可导出，标记
`extension_unavailable`。恢复错误由状态更新统一记录，区分新请求与历史终止结果。

## 回归覆盖

| 场景 | 验证方式与结果 |
| --- | --- |
| Connect、Retry、Engine 重启后的首次连接 | 脚本化命名管道 Agent + Engine 模拟数据面，恢复后到 Connected；重复 Connect 不创建第二个事务 |
| Waiting/Running、耗尽预算、后台连接 | 验证不重复发恢复命令、每个请求最多一次 Restart、后台不重置预算 |
| 取消和迟到响应 | 暂停 Restart 响应后取消，Clean/Waiting 的迟到响应均不创建连接或观察意图 |
| 账户切换、重附着、VPN Gate 切换 | 既有意图失效、精确事务重附着和 VPN Gate 热/冷重试回归通过 |
| 残留、未知检测、身份冲突、错误 generation、其他步骤、保存失败 | 模拟 Backend 与既有恢复/适配器测试验证 fail-closed；新增 IPC 过期响应检查 |
| 当前采样与历史分离 | 并发更改 journal generation 后丢弃当前存在性结果，保留历史原始时间/步骤/耗时 |
| 超时和单个在途采样 | 使用受控阻塞模拟 Backend；超时后第二个采样返回 Busy，工作线程释放后才可重采样 |
| 日志缺失、损坏、超限、未知字段 | 有明确历史状态、最多保留 32 条有效记录、丢弃未知字段 |
| 兼容与隐私 | field 18 wire snapshot、旧读者跳过扩展、旧 Agent 只读 IPC/导出、敏感字段注入、未知枚举和输出大小测试 |
| Flutter 真实主按钮和控制器状态流 | 主按钮恢复后 Connected；Connect/Retry 跟随 Reconnecting 到 Connected；取消拒绝迟到结果 |
| 重复日志 | 通过实际 Connect RPC 验证一次请求只记录一次恢复失败，并记录新的恢复请求 |

## 执行的检查

所有下列检查均通过。Rust 工具链为仓库固定的 1.97.1，Cargo 操作由 helper
使用 `--locked`。Flutter 3.44.7 / Dart 3.12.2，已核实 Flutter commit 为
`84fc5cbb223bc12f83d65b647ff8a56caf779ffd`。

从仓库根目录执行：

```powershell
cargo fmt --all --check
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test
& .\tool\build_windows_rust_release.ps1 -Variant x64-v2
```

Rust workspace/all-targets：1010 项通过、0 失败、3 忽略。忽略项是原有的
`live_http_proxy_connect_and_forward_without_tun`、
`live_socks5_connects_and_relays_http_without_tun` 和
`live_saved_vpngate_handshake_without_tun`，需要已注册的 Windows 凭据及显式
`USQUE_LIVE_CONFIG`，本轮未执行。Windows x64-v2 release 编译通过。

从 `apps/usque_gui` 执行，flutter/dart 均使用 `android/local.properties` 指向的
固定 SDK 的 `bin` 下命令：

```powershell
flutter --version --machine
flutter pub get --enforce-lockfile
dart format --output=none --set-exit-if-changed lib test
flutter analyze --no-pub
flutter test --no-pub
& ../../tool/prepare_windows_plugin_junctions.ps1 -FlutterProject .
flutter build windows --release --no-pub
```

Flutter 447 项全部通过（包含 Windows golden 测试），130 个 Dart 文件格式检查
通过，analyze 无问题，Windows release 编译通过。

从仓库根目录执行，Buf 1.72.0：

```powershell
buf lint
buf format --exit-code --diff
buf breaking --against ".git#ref=80ef61ad66b9f65f92f2a326e1ecf1f01f058096" --against-config buf.yaml
python tool/check_repository_policy.py
git diff --check
```

仓库策略检查实际使用已核实的 Python 3.12.14 可执行文件。
Buf 和 Flutter 需要在允许读取各自工具缓存的环境执行；未更改工具版本、锁文件或
检查标准。Agent 本地 IPC 扩展没有改变 MASQUE/CONNECT-IP 互操作协议，冻结的
`oracle/go` 未修改。未修改 Kotlin、Python、PowerShell 或工作流代码。

## 已知限制与未执行项

- 最初 Windows 清理为何超出观察期限仍待新证据定位。本轮修复入口并增加证据，
  没有宣称消除底层系统延迟，也没有放宽十秒观察与双重消失判定。
- 快照 VM 中的退出重连、进程退出、高负载清理、Wintun 共存与外部泄漏观察均为
  `not_run`：本轮未提供具备快照及独立管理通道的隔离 Windows 环境。
- 没有制作或安装 MSI/APK，没有启动实际 VPN/TUN，没有执行发布或访问发行签名。
- 上述隔离验证是补充证据，不作为发布前提；`not_run` 不代表通过。
