# Issue #66：第一步修复后的本机复现记录

日期：2026-09-15，以下时间均为 UTC+08:00。代码基线
`0a127f1c87f5b4414ef3b698db7694b91164e53f`。

本次确认故障停在 Wintun 关闭后的接口消失确认阶段：Engine 与 Agent 的包线程
均已退出，原生 EndSession 和 CloseAdapter 已返回，但 IP Helper 仍报告旧接口
存在。尚未确定 Windows 接口对象延迟消失的底层原因，不能把这个阶段定位写成
驱动根因已经查明。

## 证据来源与操作边界

- 用户自行安装基于 HEAD 的版本，正常连接数分钟后断开并立即重连，随后报告
  问题仍出现。用户确认报错后仅打开 Bettbox，没有手动停止 Agent。
- 读取 17:14:31 导出的诊断包及本地 Engine 日志，二者均含同一 Engine run 的
  145 条事件。15:17 的旧诊断包不作为这次复现的数据。
- 已安装 Engine 和 Agent 的各 PE 节内容哈希均与本地 release 产物相同；
  Engine 包含本次新增的 `PACKET_PUMPS_JOIN_PENDING`。文件整体哈希因头部或
  签名等非节内容而不同，未将整体哈希不同解释为安装了旧代码。
- 只读读取 SCM、Kernel-PnP 和 SetupAPI 事件。普通读取被目录 ACL 拒绝后，
  通过管理员只读脚本提取三份恢复文件的白名单字段，没有启动 Agent、加载
  Wintun DLL、调用恢复接口或修改网络。
- 提取仅包含状态、原始时间、generation、匿名资源计数和类型化清理结果，
  不复制 journal 中的 plan、receipt、设备标识、地址、SID 或凭据。

## 本次时间线

| 本地时间 | 观察 | 含义 |
| --- | --- | --- |
| 17:04:34.508 | 首次 Commit 成功，generation 17 | 到手动断开前已运行约 6 分 48 秒 |
| 17:11:22.020–22.024 | Agent `WintunEndSession` 进入/返回，4ms | 原生会话结束调用已返回 |
| 17:11:22.024840–22.025349 | Engine 包任务 join 开始/完成，约 0.509ms | 修复后的实际阻塞工作已经退出，没有 join 超时 |
| 17:11:22.025656–22.025916 | Engine 数据面停止开始/完成 | 随后才发送 rollback |
| 17:11:22.047 | Agent 包线程 join 返回，`succeeded=true` | Agent 的包线程也已完成退出 |
| 17:11:22.147–22.494 | 包会话、Kill Switch、默认路由、DNS、接口配置、端点旁路各步骤记录 restored | 首次回滚只有适配器步骤未完成；这是 Agent 的恢复记录，不是独立流量泄漏测试 |
| 17:11:22.498 | 请求释放适配器，Rust 引用数为 1 | 执行实际最后引用释放 |
| 17:11:22.691 | Kernel-PnP 删除旧 Usque 软件设备，Status 0 | PnP 设备删除已发生 |
| 17:11:22.695 | `WintunCloseAdapter` 返回，196ms | 不等同于 IP Helper 接口已消失 |
| 17:11:32.731 | 首次适配器清理失败，generation 25 | interface=true、PnP=false，confirm/pending；没有 API 错误码 |
| 17:11:43.837 | 第一次自动恢复仍失败，generation 27 | 同一存在/不存在组合 |
| 17:11:58.909 | 第二次自动恢复仍失败，generation 29 | 同一存在/不存在组合 |
| 17:12:29.716 | Bettbox GUI 启动；Core 于 17:12:30.011 启动 | 用户确认这是报错后的操作 |
| 17:12:30.979 | Bettbox 的另一个 Wintun 设备开始运行 | 并非旧 Usque 设备的同一设备实例 |
| 17:12:31.313 | 第三次自动恢复成功，generation 31，耗时 2366ms | 本次未耗尽三次预算；旧适配器步骤最终完成 |
| 17:12:31.324 | Agent 启动模式从 Auto 改为 Demand | 与恢复到 Clean 的代码路径相符 |
| 17:12:31.817–34.327 | 下一次 Prepare 开始并成功，generation 43 | 发生在旧事务完成清理之后 |
| 17:12:41.992 | 新连接启动失败，`ENDPOINT_PIN_MISMATCH` | 另一个发生在传输启动中的失败；没有随后 OpenPacketSession/Commit |
| 17:12:42.760 | 该次启动回滚成功，Agent phase=Clean、generation 49 | 第二次事务清理成功 |
| 17:14:31 | 导出时 Agent 已不可用 | 包中恢复观察、历史、trace 均为 unavailable，不能当成资源不存在 |
| 17:20:25 | 只读取得 journal：Clean、generation 49、steps=[] | 当前受保护记录已经清理完成 |

CloseAdapter 返回至首次记录恢复成功相隔 **68.618 秒**。最后一次记录接口仍在
到恢复成功相隔 32.404 秒。因为期间启动了 Bettbox，这不是无外部干预的自然
清理时长。Bettbox 设备开始运行到清理成功相隔约 334ms；这是第二次现场中
出现类似关联，但仍不足以证明 Bettbox 的某一具体调用清除了旧接口。

SetupAPI 对首次关闭记录 `SUCCESS`，期间出现 `CR_INVALID_DEVNODE`；同样的
警告也出现在第二次迅速完成的清理中，因此不能单凭该警告确定根因。

## 对原有假设的修正

1. **本次不是 Engine 假 join 导致的等待缺口。** 安装版本已核对，实际工作在
   0.509ms 内完成；没有 `PACKET_PUMPS_JOIN_PENDING`。
2. **没有发现 Agent 在清理中提前退出。** 原始失败、自动恢复以及第二次事务
   使用同一个 Agent run；最后一次回滚为 Clean。当前服务为 Stopped、退出码
   0，未查询到对应应用崩溃事件，符合 Clean 后空闲退出策略，但没有独立的
   服务退出原因日志来确认精确退出时刻。
3. **不是旧 Usque TUN 尚未清理就发起下一个 Usque Prepare。** 第二次 Prepare
   晚于旧适配器 restored，保护检查没有被提前跳过。
4. **剩余问题在原生关闭后的接口收尾。** 同名/GUID/LUID 身份验证后的 IP
   Helper 表仍包含旧接口，而精确 PnP 检测已不存在。Rust 最后引用释放和原生
   调用返回不能证明 Windows 内核、NDIS、过滤驱动或原生内部工作队列已完成。
   现有记录无法在这些可能性之间进一步定因。

本机采样时存储设置为 Cubic、CONNECT-IP、TUN+HTTP+SOCKS5，VPN Gate 关闭。
这是当前设置，不是首次连接时保存的会话算法快照；不能用它追认当时算法，
也没有证据把此次清理失败归因于 BBRv3。

## 独立的端点 pin 错误

`ENDPOINT_PIN_MISMATCH` 出现在旧事务清理成功、新 Prepare 成功之后。代码把
端点固定公钥校验失败及部分 pin 刷新失败映射到这个错误码；现有日志没有
记录具体子原因、当时的实际出站路径或证书分类。不能直接断言 Bettbox 修改了
证书、账户密钥失效或发生中间人攻击。也不能通过关闭 pin 校验修复连接。

导出时连接快照为 Disconnected，error/failure 为空。这份包没有保存一份最终
`WINDOWS_RECOVERY_EXHAUSTED` 快照；应该按以上两个先后阶段理解这次失败。

## 仍需补齐的证据

- Agent 的包线程 stop/join 记录实际存在于 trace 序号 6–8，但 generation 为
  0。这确认了此前发现的 generation 漏传问题；正常导出会丢弃这些非原生、
  generation 为 0 的记录。应修复调用方传参，保留隐私校验。
- 当前接口详细状态只在采样或三次耗尽后的观察中记录；本次第三次成功，
  没有触发耗尽观察，历史只留下接口/PnP 存在性。需要在每次原生关闭及每次
  恢复尝试边界保存类型化观察，并与工作线程和 DLL 生命周期关联。
- Agent Clean 后正常退出，会让随后导出的包失去受保护历史和 trace。应保存
  故障当时的有界、脱敏证据，同时明确区分历史缓存与本次实时观察，不能让
  “Agent 当前不可用”覆盖此前已经取得的恢复证据。
- 真正区分 Windows/NDIS/过滤驱动引用及内部工作队列，需要隔离环境中的
  ETW/线程栈和驱动收尾证据；工作站没有运行这些测试或主动抓取流量。

上述现场排查只补充诊断记录，没有更改生产代码或重新运行 VPN。用户本机复现属于
现场观察，不作为 snapshot VM 或独立泄漏验证通过的证据。

## 后续日志增强

根据这次现场补齐以下记录；这批修改不宣称已经解决 Windows 接口延迟消失。

- 关闭包会话和租约 EOF 传入当前 journal generation，包线程退出记录能够通过
  正常导出的校验。正式回归测试先在旧实现失败，修改后通过。
- 原生 CloseAdapter 返回后的第一轮检查、每次适配器清理尝试的开始与结果、
  至多八次状态变化及最后一次观察进入 trace。保留接口运行/管理/媒体状态、
  PnP 存在性、身份校验、API 和数字错误码。复用清理本身的检查，没有额外
  加载 Wintun、增加 Windows 查询或创建采样线程。
- 新阶段枚举追加为 18/19；已有协议版本、字段号码和恢复事件文件格式保持
  不变。返回成功只表示该次适配器步骤确认消失，不表示 journal 已保存 Clean。
- Engine 在恢复相关边界用单个读者获取已运行 Agent 的诊断响应，合并短时间内
  的通知，保留一次延迟补采；空闲时不循环查询，也不启动已停止的服务。
- 脱敏历史以有版本、原子替换、有大小上限的文件保存在用户日志目录。后台文件
  写入使用单个在途的独立线程，等待退出有上限；模拟写入卡住不会拖住 Tokio
  runtime 退出。缓存写锁按目录隔离，忙时跳过该次写入。损坏、
  不支持的版本、重解析项以及超限缓存不会阻止导出；较旧的并发采集不会覆盖
  较新的记录，临时读不到 trace 不会擦除之前取得的事件。
- 诊断 ZIP 在实时证据不可用时使用 `cached_evidence`，注明此前响应的采集时间，
  旧观察字段为 `observation_at_capture`。顶层仍如实表示 Agent 当前不可用。
  缓存从不参与连接许可、恢复次数、取消、适配器删除或 Clean 判定。
- Agent 和 Engine 均按字段白名单处理；缓存无 journal/receipt、设备标识、
  SID、地址、凭据及任意原生错误文本。缓存文件最多 192 KiB、32 条历史事件、
  128 条 trace，完整导出继续小于 256 KiB；清理本地数据同时删除此缓存。

验证覆盖实际记录生成、后台通知合并、脚本化 Agent IPC 结束后再次导出 ZIP、
缓存损坏/超限/未知字段/敏感字段注入、旧 Agent、乱序采集，以及日志队列满时
清理仍按原有规则执行。所有新增执行测试均使用模拟资源或临时文件。

### 检查记录

基线为本文开头的 `0a127f1`，验证对象是包含日志增强的工作区源码。

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all --check` | passed |
| Windows helper：`-Variant x64-v2 -CargoAction clippy` | passed，完整 workspace/all-targets，locked |
| 同一 helper 初始化的环境中运行完整安全 Rust 测试 | passed：1084 项通过，3 项既有 ignored，2 项明确排除，详见下方 |
| Windows helper：`-Variant x64-v2 -CargoAction test -Package usque-engine` | passed：192 个库测试及 2 个主程序测试通过，3 个既有 ignored |
| `tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | passed，locked；不生成或安装 APK |
| `tool/build_windows_rust_release.ps1 -Variant x64-v2` | passed，locked；Agent、Engine、update、uninstall 四个 release 二进制仅编译 |
| Buf 1.72.0 lint、format | passed |
| Buf breaking，按 CI 使用 PR #62 的 main 基线 `5a03d1b91528c1556f5160fe74b976f9fcf0f0ca` | passed |
| `tool/check_repository_policy.py`（已验证的 Python 3.12.14 完整路径调用）、`git diff --check` | passed |

完整安全 Rust 测试命令在 Windows helper 初始化的同一 PowerShell 会话执行，
显式保持 `RUSTFLAGS=-C target-cpu=x86-64-v2`：

```powershell
cargo test --workspace --all-targets --locked -- `
  --skip windows::wintun::tests::pinned_official_library_loads_all_required_exports_without_installing_driver `
  --skip windows::backend::tests::opening_backend_only_verifies_and_loads_the_dependency
```

两项排除测试会加载真实 Wintun DLL，其初始化可能清理旧设备，因此本机不执行。
未删改这些测试，也未把完整未过滤的 helper test 标为通过。Windows VPN、长时间
使用后断开重连、Bettbox 共存、崩溃恢复、外部泄漏观察均为 `not_run`；遵循用户
继续暂停本机实测的要求。Flutter、Kotlin、安装器、发布流程及 Go 冻结数据未改动；
此次 Agent IPC 枚举以 Rust wire fixture 和 Buf 验证，不改变 Go 网络互操作协议。
