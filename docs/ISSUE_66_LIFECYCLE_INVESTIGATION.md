# Issue #66：断开与清理任务生命周期排查及第一步修复

日期：2026-09-15。排查代码基线：`04842a5c9eb75273fb77c958e465a7905f86bd15`。

该基线的 Engine 存在一个停止任务的所有权缺口：外层异步任务退出后，内层
`spawn_blocking` 工作线程仍可能存活。纯内存实验可稳定展示这一行为，但它
尚未被证明是 Wintun 接口残留的原因。已有现场证据不支持 Agent 提前退出或
Wintun DLL 在普通断开时提前卸载。

以下保留排查时的证据边界；末节记录随后实施的第一步源码修复。

## 新的复现线索与证据范围

用户补充：正常连接并使用 TUN 数分钟，手动断开后重新连接，较容易出现
`WINDOWS_RECOVERY_EXHAUSTED`。这说明连续修改设置、切换 VPN Gate 并非必要
触发条件，排查应覆盖普通连接的完整生命周期。

本轮使用的诊断包仍为 2026-09-15 15:17:42 导出的版本，没有把该包当成这条
新复现路径的独立实测。读取了源码、已有诊断包和上一轮核对的本机系统事件；
本轮没有创建 TUN、调用恢复命令或修改服务、路由、DNS、WFP。

## 清理由谁持有

| 层次 | 所有权与退出条件 | 排查结果 |
| --- | --- | --- |
| Engine 断开任务 | `disconnect_locked` 将整个运行时移入后台清理任务；下一次连接等待该任务结果 | 断开按钮返回不等于底层清理完成，但新建连接仍有清理等待与 Agent 状态检查 |
| Engine 包通知线程 | 外层 Tokio 任务等待一个 `spawn_blocking` 任务 | 停止逻辑先 abort 外层，可能失去对内层真实退出的等待 |
| Agent 包收发线程 | `PacketPump` 的专用线程拥有 `WintunSession`；`stop` 与 `Drop` 调用 join | 正常返回前会结束原生会话；后续日志增强已修复关闭会话和租约 EOF 的 generation 漏传 |
| Agent 恢复工作线程 | `spawn_blocking` 闭包拥有 `Arc<BackendInner>`；恢复调用持有 journal 与 mutation gate | Engine 断开 IPC 不会直接取消正在执行的 Agent 恢复调用 |
| Wintun DLL | `WindowsResources.library` 保留 `Arc<WintunLibrary>`，与 Agent Backend 同寿命 | 普通断开仅取走 pump 和 adapter，不取走 library |
| Wintun 内部清理 | DLL 会排队执行孤儿设备清理；Windows 继续处理设备移除 | 原生调用返回不代表该队列和 Windows 网络接口状态均已完成收尾 |

主要实现：

- [Engine 断开任务](../crates/usque-engine/src/lib.rs)：`disconnect_locked`、
  `await_disconnect_cleanup`。
- [Engine Windows 运行时](../crates/usque-engine/src/windows_agent.rs)：
  `cancel_packet_pumps`、`start_packet_pumps`、`stop_tasks`。
- [Agent 包会话](../crates/usque-agent/src/windows/packet_session.rs)：
  `PacketPump::stop`、`PacketPump::join`、`run_packet_pump`。
- [Agent Backend](../crates/usque-agent/src/windows/backend.rs)：
  `WindowsResources`、`restore_step_traced`、`restore_sync`。
- [Agent 服务](../crates/usque-agent/src/windows/server.rs)：`mutate`、
  `run_automatic_recovery`、`wait_for_idle_exit_after`。

Agent 的空闲退出要求 `Clean` 且没有活动工作。`RecoveryRequired` 不满足该条件；
没有发现一个“连接使用数分钟后，自动销毁清理程序”的普通连接计时器。

## 已确认的 Engine 生命周期缺口

`start_packet_pumps` 返回给运行时的是外层 Tokio `JoinHandle`。真正执行
`WaitForMultipleObjects` 的工作在内层 `spawn_blocking` 中。

`cancel_packet_pumps` 先对外层调用 `abort`，随后 `stop_tasks` 等待的仍是外层
句柄。外层取消完成时会丢弃内层句柄，已经开始的阻塞工作不会因此被取消。
因此 `PACKET_PUMPS_JOIN_FINISHED` 不能证明底层包通知线程已经退出。

[Tokio 的 spawn_blocking 契约](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)
明确区分异步任务取消与已运行阻塞任务的退出。

纯内存实验使用以下方法，未调用 Windows 网络 API：

1. 从当前源码原样提取 `stop_tasks`。
2. 使用相同的“外层 Tokio 任务等待内层 spawn_blocking”结构。
3. 内层通过内存通道阻塞，明确通知实验它已经开始；此时外层被 abort。
4. 调用原样提取的 `stop_tasks`，检查它返回时内层是否仍存活。
5. 释放通道并等待完成确认，保证不遗留实验线程。

结果：**32/32 次停止函数在内层工作退出前返回**。这是人为控制退出顺序的
所有权实验，不是 VPN 故障复现率，也不证明现场线程一直阻塞。

该线程持有 `PacketSessionMapping`，即共享内存映射和事件句柄，**并不直接持有
Agent 的 Wintun 适配器或原生会话句柄**。因此当前只能确认停止等待不完整和
日志语义过强，不能把它直接等同于旧网卡残留的根因。

## 现场证据能排除什么

已核对的旧现场中：

- Agent 从 15:14:03 启动后持续存活，三次自动恢复与随后观察使用同一个
  Agent run。没有发生“关闭函数尚未执行，Agent 就退出”的现象。
- 最后一次 `WintunEndSession` 已返回；适配器释放时 Usque 内部引用计数为 1。
- `WintunCloseAdapter` 于 15:15:22.460 返回，耗时 43ms；SetupAPI 记录设备删除
  成功。后续恢复仍看到同一身份的接口存在、PnP 不存在。
- 15:17:07 用户在报错后打开 Bettbox；其新 TUN 启动后，15:17:08.824 的采样
  首次确认旧 Usque 接口消失。该间隔不能用作未经干预的自然清理时长。

固定版本的 [Wintun session.c](https://github.com/WireGuard/wintun/blob/0.14.1/api/session.c)
显示 EndSession 会关闭会话设备句柄并释放会话内存；
[adapter.c](https://github.com/WireGuard/wintun/blob/0.14.1/api/adapter.c)
显示 CloseAdapter 还会排队执行孤儿设备清理。
[SwDeviceClose 的文档](https://learn.microsoft.com/en-us/windows/win32/api/swdevice/nf-swdevice-swdeviceclose)
说明软件设备的移除可以在函数返回后继续。

现有 trace 没有原生内部清理队列的开始/完成证据，也没有故障当时的线程栈及
驱动内部引用证据。仍不能确定接口残留究竟来自何处；不能直接归因于 BBRv3、
Bettbox、Wintun 驱动缺陷或 Windows 缺陷。

## 建议的修复和验证顺序

1. 让运行时明确拥有底层包通知工作及其完成信号。先发退出信号，再等待真实
   退出；超时应报告未完成并保留待收尾工作，不能把外层 abort 当成真实 join。
2. 修复 Agent 关闭会话、释放租约路径漏传 trace generation 的问题，补齐包
   线程退出记录。将退出请求、外层任务退出、阻塞工作返回分开记录。
3. 补充按连接关联的 Backend 恢复工作开始/结束、DLL 生命周期和服务退出原因
   证据。仅增加类型化事件与匿名关联标识，继续禁止导出原始 journal、设备
   GUID/LUID、地址、SID 或凭据；不把原生 API 返回伪装成 DLL 内部队列完成。
4. 在隔离环境分别覆盖短连接、使用数分钟后断开、持续流量后断开，以及是否
   启动另一 Wintun 客户端的对照。采样线程是否退出、设备句柄是否释放及接口
   消失时间；当前工作站不执行这些场景。

继续保持接口与 PnP 双重确认、身份/generation 检查、取消后不自动连接以及
每请求有限重试预算。没有证据支持直接放宽 `Clean` 条件或以重启 Agent 代替
正确收尾。

## 修复前的验证记录

| 检查 | 结果 |
| --- | --- |
| Windows helper：`-Variant x64-v2 -CargoAction test -Package usque-engine`，默认并行 | lib：175 通过、1 失败、3 忽略；失败为恢复错误日志计数断言，观察到 0 条而预期 1 条；该轮未运行 main 测试 |
| 上述失败用例单独运行 | 1 通过；不能据此覆盖首次并行失败 |
| 同一 helper，设置 `RUST_TEST_THREADS=1` | lib：176 通过、3 忽略；main：2 通过；helper 输出成功。Cargo 操作使用 `--locked` |
| 纯内存生命周期实验 | 32/32 次观察到提前返回；全部工作最终释放并确认退出 |
| 真实 TUN、服务启停、网络恢复、驱动/内核跟踪 | `not_run` |

默认并行执行的日志计数失败仍是需要跟进的测试稳定性问题。没有将串行通过
写成默认并行 gate 通过。

实验复用当前已构建的 Tokio 库，没有更改依赖或 lockfile。临时源码和可执行
文件位于 `target` 下的独立目录，实验结束后检查绝对路径边界并删除。上述
实验阶段没有修改运行逻辑。

## 第一步修复：明确持有并等待实际工作任务

本批仅修改 Engine 的任务停止与等待所有权，保留 Agent 恢复标准、重试预算、
协议和日志导出的现有结构。Agent trace generation 漏传及更多原生生命周期
证据留在后续批次。

- `spawn_packet_waiter` 直接返回执行原生等待的 `spawn_blocking` 句柄。取消
  仍先发出共享 shutdown 事件，随后取消异步转发；不再通过外层任务的退出
  推断阻塞工作已结束。
- `stop_tasks` 借用运行时持有的句柄，逐个确认实际退出后才移除。停止等待
  被取消时，尚未完成的句柄仍在原来的集合中。
- 5 秒为停止等待的报告阈值。超过阈值记录 `PACKET_PUMPS_JOIN_PENDING`，继续
  等待同一任务；这不是清理成功或强制终止的期限。所有任务退出后，普通断开
  才会记录 `PACKET_PUMPS_JOIN_FINISHED` 并继续后续清理。
- `await_disconnect_cleanup` 等待期间将后台清理句柄保留在服务中，避免一次
  请求被取消后，下一次 Connect/Retry 误认为没有清理工作。
- Connect/Retry 的前台等待响应连接取消；必要的后台清理继续执行。取消的
  请求不会在清理完成后自行恢复连接，清理错误也不会被丢弃。

普通断开、VPN Gate 切换、TUN 分离和 Agent 重附着共用任务停止逻辑。若原生
工作一直不能退出，该次清理会继续保持未完成，后续连接仍须等待；本批没有
通过丢弃任务、重启 Agent 或放宽接口/PnP 确认条件来绕过等待。

新增日志只包含固定事件码与固定描述，不新增设备标识、地址、凭据或原始
panic 内容。`PACKET_PUMP_TASK_PANICKED` 表示任务已经异常退出，不代表 Windows
网络资源已恢复。

### 正式回归覆盖

新增 7 个测试，使用匿名共享内存、Windows 事件、内存通道、模拟运行时和脚本化
Agent IPC；不加载 Wintun DLL、不创建 TUN、不调用本机 Agent。

1. 实际原生等待返回后，人为延迟工作退出；abort 不能提前完成 join，映射必须
   由实际工作释放。
2. 超过 5 秒报告阈值后，工作仍被持有；取消等待后可以继续等待同一任务。
3. 空闲及通知通道饱和时，shutdown 事件都能唤醒原生等待。
4. 取消一次后台清理等待后，下一次请求仍能找到并等待该清理。
5. Connect 和 Retry 在清理完成前不创建模拟会话，重复 Connect 保留同一会话。
6. 等待清理时取消重复连接请求，不丢失后台清理，也不在清理结束后自动连接。
7. 等待被取消之后，原清理任务最终返回的错误仍阻止新连接。

实现修复前，正式用例已复现“abort 后提前 join”和“取消等待后丢失清理句柄”
两个缺陷。初始映射测试夹具的容量配置错误已修正；它不作为产品缺陷证据。

### 本批验证

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all --check` | 通过 |
| Windows helper：`-Variant x64-v2 -CargoAction test -Package usque-engine` | 默认并行：lib 183 通过、3 忽略；main 2 通过 |
| Windows helper：`-Variant x64-v2 -CargoAction clippy` | 全工作区通过 |
| `tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | 通过 |
| Windows helper：`-Variant x64-v2` | release 编译通过；没有安装或运行产物 |
| 工作区安全测试集 | 默认并行：1071 通过、3 忽略、2 显式过滤 |
| `python tool/check_repository_policy.py`、`git diff --check` | 通过 |
| 实机 TUN、Wintun DLL 加载、服务启停及网络恢复 | `not_run` |

工作区测试显式排除两个加载官方 Wintun DLL 的现有用例：
`windows::wintun::tests::pinned_official_library_loads_all_required_exports_without_installing_driver`
和 `windows::backend::tests::opening_backend_only_verifies_and_loads_the_dependency`。
固定版本的 [DllMain](https://github.com/WireGuard/wintun/blob/0.14.1/api/main.c)
会调用 [AdapterCleanupLegacyDevices](https://github.com/WireGuard/wintun/blob/0.14.1/api/adapter_win7.h)，
其中包含旧设备删除操作；因此本机没有运行未过滤的全工作区测试命令。这两个
用例属于 `not_run`，不能将安全测试子集写成完整 gate 通过。

安全测试在 Windows release helper 初始化的同一 PowerShell 会话中执行；保留
MSVC/CMake/Ninja 环境，并临时设置 `RUSTFLAGS=-C target-cpu=x86-64-v2`，结束后
恢复原值。命令为：

```powershell
cargo test --workspace --all-targets --locked -- `
    --skip windows::wintun::tests::pinned_official_library_loads_all_required_exports_without_installing_driver `
    --skip windows::backend::tests::opening_backend_only_verifies_and_loads_the_dependency
```

Cargo 编译、Clippy 和测试均使用 `--locked`。本批没有 Flutter、protobuf、
Android Kotlin、打包或发布变更，对应检查不适用。修复后 Engine 默认并行测试
中的原日志计数用例通过，但没有专门修复或宣称消除了先前观察到的不稳定性。

本批修复已确认的任务所有权缺陷；尚不能据此宣称真实 Wintun 接口残留的根因
已经消除。使用数分钟后断开重连的现场场景仍为 `not_run`。
