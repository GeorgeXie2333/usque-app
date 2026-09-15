# Issue #66 生命周期证据与验证

日期：2026-09-15。分支 `dev`，开始时 HEAD
`376e8ddbaa20d2825ea39e5861a3829be23a253d`，工作区无修改。保留已有恢复入口、
VPN Gate 和取消连接修复。本轮增加诊断记录，不改变清理成功标准，不将当前
补丁视为 Windows 延迟移除的根因修复。

## 补充的证据

第一批提交 `d782fbe` 在 Agent 记录以下边界：

- PacketPump 停止请求、等待线程退出、等待返回及耗时。
- 实际执行 `WintunEndSession` / `WintunCloseAdapter` 的进入与返回。
- 释放适配器前的引用数，以及最后一个引用真正释放时的原生调用。
- 原生 Wintun 日志的固定分类和级别；异步回调不能关联事务时记录为未关联。
- 三次自动恢复耗尽后的有限只读观察，记录状态变化及首次确认接口/PnP 均消失。

接口/PnP 采样增加接口运行/管理/媒体状态，以及 devnode 状态和问题码。
`CONFIGRET` 与 Win32 错误码独立，未知、错误和身份冲突均不能解释为“不存在”。
两种原生关闭函数均返回 VOID；“已返回”不等于 Windows 已完成移除。

第二批把这些记录接入既有 `windows-recovery.json` 的独立 `trace` 区域。
`current_observation`、最多 32 条步骤历史 `history` 与最多 128 条生命周期
记录分别保存，不用新采样时间覆盖历史时间。协议版本仍为 3，扩展为
`RecoveryDiagnostics.trace = 4`，已有 `PlatformState.recovery_diagnostics = 18`
和旧事件文件字段保持兼容。没有新增连接命令或 UI 操作。

## 如何获取后续记录

这些记录需要运行包含本轮修改的 Agent；旧版本无法补记过去的调用边界。
本轮仅交付源码和编译验证，没有安装或替换本机正在运行的程序。

新版发生故障时，可通过现有“导出诊断包”保存证据，无需先重试、停止服务或
重启电脑。只读观察在恢复耗尽后自动开始，约每五秒采样，最长十分钟；结束时
保留消失、超时或取消记录。导出访问已运行的 Agent；服务不可用或旧 Agent
缺少扩展时仍能导出，并明确标记证据不可用。

Agent 的本地文件为 `C:\ProgramData\Usque\agent\recovery-trace-v1.jsonl`，
沿用受保护 journal 目录的访问边界。导出包只包含重建后的白名单字段，不复制
整个 journal、原生错误文本、设备名称、GUID/LUID、SID、地址或凭据。

用 `agent_run_id` 区分进程，用 `resource_id` 关联同一资源的进入/返回记录；
它们是临时随机数/计数器，不是 Windows 的进程或设备标识。
`event_sequence`、原始时间和 `monotonic_ms` 帮助关联并发记录。未关联的原生
回调使用 generation/resource 0，不能据此认定属于某一台适配器。

## 限制和安全约束

- 新文件上限 1 MiB，单条上限 4096 字节；后台写入队列上限 128 条。
  队列满或写盘失败只计数，不阻塞网络清理。突然终止进程仍可能丢失未落盘记录。
- Agent 返回最近至多 128 条有效记录；Engine 再次校验并按 128 KiB 的 trace
  事件 JSON 预算截断，完整恢复摘要小于 256 KiB。缺失、损坏、超限和截断分别标记。
  顶层丢弃/失败计数属于当前 writer，事件里的计数属于原始 Agent 运行。
- 采样共用一个在途许可，调用超时后直到原生函数实际返回才释放许可。Agent
  采样预算为 1.8 秒，服务响应和 Engine 等待上限分别为 1.9 秒、2 秒。
- 观察到消失只记录证据，不保存 Clean、不重置三次预算、不连接 VPN。
  操作/generation/revision 变化或关闭服务会终止观察；重复查询不重新开启窗口。
- 只能用最后一次存在与首次确认消失限定消失时间范围。没有返回记录可能是
  调用未返回，也可能是记录丢失，须结合计数和后续状态判断。
- 正常卸载仅在 journal 已 Clean 后清除两个白名单证据文件，拒绝目录/重解析点，
  不递归删除其他文件。

实机 VPN 测试继续暂停。Wintun 延迟移除、关闭应用后的实际网络恢复、高负载、
进程退出及其他 Wintun 共存场景均为 `not_run`。后续实测需要有快照和独立管理
通道的 Windows 隔离环境；这些补充验证不作为发布前提。

## 验证记录

Cargo 操作通过仓库 helper 使用 `--locked`，Rust 1.97.1，Buf 1.72.0。
当前 `dev` 没有开放 PR，使用只读查询核实的目标 `main` 提交
`80ef61ad66b9f65f92f2a326e1ecf1f01f058096`，按 CI 方式执行 breaking check。

第一批已通过 Agent Clippy、Agent 161 项测试和 IPC 30 项测试，以及 Rust
格式、Buf lint/format/breaking、仓库策略和 `git diff --check`。
第二批 Engine 专项测试为 178 项通过、3 项凭据相关实测忽略。最终检查如下，
命令从仓库根目录执行：

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| Rust 格式 | `cargo fmt --all --check` | 通过 |
| Windows 全工作区 Clippy | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction clippy` | 通过 |
| Windows 全工作区测试 | `./tool/build_windows_rust_release.ps1 -Variant x64-v2 -CargoAction test` | 1066 通过、0 失败、3 忽略 |
| Windows Rust release 编译 | `./tool/build_windows_rust_release.ps1 -Variant x64-v2` | 通过；仅编译 |
| Android Rust Clippy | `./tool/build_android_rust.ps1 -AbiFilter arm64-v8a -CargoAction clippy` | 通过；固定 NDK/CMake，无 JNI 复制 |
| Buf lint | `buf lint` | 通过 |
| Buf 格式 | `buf format --exit-code --diff` | 通过 |
| Buf 兼容 | `buf breaking --against ".git#ref=80ef61ad66b9f65f92f2a326e1ecf1f01f058096" --against-config buf.yaml` | 通过 |
| 仓库策略 | `python tool/check_repository_policy.py` | 通过 |
| 空白检查 | `git diff --check` | 通过 |

Python 命令使用已验证的 Python 3.10+ 可执行文件：
`C:/Users/George/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe`。
Buf 使用 `C:/Users/George/go/bin/buf.exe`，版本为 1.72.0。
编译过程中 quiche 链接器仍打印创建 import library 的提示，所有 gate 正常完成，
没有增加警告抑制。没有生成 MSI、安装/替换程序或启动 VPN。

测试覆盖实际资源 Drop 的原生函数替身、阻塞调用不伪造返回、满队列和写盘失败、
延迟消失不修改 journal、不重复开启观察窗口、generation 变化取消观察、单个
在途采样、未知/矛盾字段、缺失/损坏/超限文件、旧 protobuf 读者和旧 Agent、
原始时间/计数保留、敏感字段注入，以及经过脚本化命名管道到最终 ZIP 的完整
只读导出路径。该路径断言只发送 `InspectPlatformState`。

原生日志分类对照固定版本的
[Wintun adapter.c](https://github.com/WireGuard/wintun/blob/0.14.1/api/adapter.c)
与 [logger.c](https://github.com/WireGuard/wintun/blob/0.14.1/api/logger.c)
核实。没有修改第三方原生源码。

本轮只修改 Rust、Agent 内部 protobuf 和说明文档，Flutter/Dart、Kotlin、
Windows GUI runner、构建脚本及数据面协议均未修改；相应 Flutter/Kotlin/Go
套件不属于本轮改动范围，未重复执行。Agent 私有 IPC 没有 Go-oracle 对端，
新增兼容性证据为固定 wire 字节和旧消息解码测试，保留冻结的 Go 参考不变。
