# Documentation / 文档导航

Choose a guide by task. Technical documents are mainly in English; the product
overview is available in [English](../README.md) and
[简体中文](../README.zh-CN.md).

按任务选择文档。开发分支可能包含尚未发布的行为；使用正式安装包时，请查阅对应
Release 的说明及标签下文档。历史验收记录只适用于其注明的提交，不代表当前版本
已经通过相同测试。

## Use Usque / 使用指南

| Document | Read it for / 用途 |
| --- | --- |
| [Installation and removal](INSTALLATION.md) | Package verification, upgrades, uninstall, recovery, and version applicability / 校验、升级、卸载、恢复与适用版本 |
| [Network Doctor](network-doctor.md) | Run checks, read results, and export a local report / 运行检查、理解结果与导出诊断 |
| [Direct DNS](encrypted-direct-dns.md) | Choose System, DoH or DoT and fill in resolver settings / 选择直连 DNS 模式与填写服务器配置 |
| [WARP via WireGuard](WARP_WIREGUARD.md) | Generate/import WARP configurations and scan endpoints / 生成、导入 WARP 配置与扫描端点 |
| [Chain proxy](CHAIN_PROXY.md) | OpenVPN, WireGuard, WARP via WireGuard, VPN Gate: import, select and apply / 导入、选用与应用 |
| [VPN Gate directory](VPN_GATE.md) | Manage the volunteer directory and favorites / 管理志愿服务器目录与收藏 |
| [Experimental L4](L4_PROXY.md) | Enable TCP proxy mode and understand its traffic limits / 启用 TCP 代理模式及了解限制 |
| [Experimental Zero Trust](ZERO_TRUST_EXPERIMENTAL.md) | Enrollment, unsupported features, and validation requirements / 实验性注册、限制与验证要求 |
| [Security policy](../SECURITY.md) | Private vulnerability reporting and supported versions / 私密漏洞报告与支持范围 |

## Develop and maintain / 开发与维护

| Document | Read it for / 用途 |
| --- | --- |
| [Contributing](../CONTRIBUTING.md) | Toolchain setup and change-scoped checks / 工具链准备与检查矩阵 |
| [Development-machine safety](../CONTRIBUTING.md#development-machines) | Workstation limits, isolation, and evidence requirements / 开发机安全边界与验证要求 |
| [Implementation progress](IMPLEMENTATION.md) | Source-tree milestones, not proof of a test run / 源码实现进度，不等同于测试通过 |
| [GUI development](../apps/usque_gui/README.md) | Editing workflows and native UI conventions / 界面交互与布局约定 |
| [Country flags](COUNTRY_FLAGS.md) | Bundled assets, attribution and update checks / 内置旗帜资源、来源与更新检查 |
| [Release process](RELEASE.md) | Candidate preparation, approval, signing, and publication / 候选包、审批、签名与发布 |
| [Code signing policy](CODE_SIGNING.md) | Official identities, key handling, and rotation / 官方签名身份、密钥管理与轮换 |
| [GitHub governance](GITHUB_GOVERNANCE.md) | Repository checks, permissions, and maintainer rules / 仓库检查、权限与维护规则 |
| [Reliability testing](RELIABILITY_TESTING.md) | Deterministic checks, isolated environments, and result requirements / 确定性检查、隔离环境与结果要求 |
| [Network-quality rollback](network-quality-rollback.md) | Reviewed build-only rollback and regression requirements / 构建级回滚及回归验证 |

Read the [Code of Conduct](../CODE_OF_CONDUCT.md) before participating and follow
[Contributing](../CONTRIBUTING.md) for safety rules and required checks.
Publication and optional protected-runner validation are explained in
[Release process](RELEASE.md#runner-isolation-boundary).

## Technical reference / 技术规范

| Document | Read it for / 用途 |
| --- | --- |
| [Reliability invariants](reliability-invariants.md) | Append-only invariant identifiers and safety properties / 不可复用的标识与安全属性 |
| [Network-quality metrics](network-quality-metrics.md) | Availability, counters, queue bounds, RTT, and PMTU / 指标语义与资源边界 |
| [Network-quality IPC](network-quality-ipc.md) | Wire fields, compatibility, event coalescing, and UI samples / 协议字段、兼容性与采样 |
| [H3 path infrastructure](h3-path-infrastructure.md) | Socket ownership, exact generations, and migration / 路径所有权、网络代次与迁移 |
| [H3 client reliability](h3-client-reliability.md) | Receive handling, fragmentation policy, GOAWAY, and recovery / 接收、分片策略与恢复 |
| [HTTP/3 congestion control](congestion-control.md) | Algorithm selection, deferred session settings, and BBRv3 / 算法选择、延迟生效与 BBRv3 |
| [QUIC UDP receive buffer](UDP_RECEIVE_BUFFER.md) | Windows/Android default, OS readback, diagnostic fields and evidence limits / 共用接收缓冲默认值、实际回读与证据边界 |
| [Network settings](NETWORK_SETTINGS.md) | Field updates, saved settings and active-session changes / 字段修改、设置保存与会话生效规则 |
| [Windows lifecycle](windows-lifecycle.md) | Service recovery, upgrade ordering and quiet uninstall / 服务恢复、升级顺序与静默卸载 |
| [Direct DNS threat model](direct-dns-threat-model.md) | Scoped trust boundaries, assumptions, and review evidence / 专题信任边界、假设与审查依据 |

Use these pages when changing an implementation. Keep behavior descriptions in
sync with executable sources, preserve protobuf numbers and invariant identifiers,
and retain fail-closed checks. Scoped reviews identify the version they examined;
they do not establish performance or leak results for later versions.

Common terms in these references:

| Term | Meaning / 含义 |
| --- | --- |
| Runtime Profile | The connection's account identity and effective settings; shared network preferences are not per-account settings / 连接使用的账号身份和设置副本；共享网络设置不属于单个账号 |
| Data plane | The mechanism carrying application traffic, such as CONNECT-IP or L4 / 承载应用流量的方式，例如 CONNECT-IP 或 L4 |
| Generation / epoch | A version of network, session or process state, used to reject results from an older state / 网络、会话或进程状态的版本号，用于拒绝旧状态返回的结果 |
| Lease | Tracked ownership of a resource or permission that must be released or recovered / 需要释放或恢复的资源使用权 |
| Fail closed | Refuse an operation or traffic when its safety conditions cannot be confirmed / 无法确认安全条件时拒绝操作或流量 |

## Historical records / 历史记录

| Record | Scope / 范围 |
| --- | --- |
| [Implementation baseline](implementation-baseline.md) | PR-00 source, toolchain, and unavailable-lab baseline / PR-00 基线 |
| [MASQUE performance candidates](MASQUE_PERFORMANCE_VALIDATION.md) | Staged source candidates, checks, and unmeasured device results / 分阶段候选、检查与待测设备结果 |
| [Network-quality acceptance](network-quality-acceptance.md) | PR-01–PR-12 implementation and test matrix, with later correction notice / 阶段验收及后续更正 |
| [PMTU review and fixes](pmtu-path-fixes.md) | Candidate-specific defects, corrections, and regression results / 特定候选版本的修复记录 |
| [Receive-buffer experiments](RECEIVE_BUFFER_EXPERIMENTS.md) | Retired Android A/B builds and production-default validation / 已结束的安卓对照试验与默认值验证记录 |
| [Initial L4 validation](L4_VALIDATION.md) | Initial implementation checks and unavailable environments / 初始实现检查与未运行项目 |
| [L4 backpressure fix](L4_BACKPRESSURE_FIX.md) | Reproduced stalls, stop handling and regression results / 阻塞复现、停止处理与回归结果 |
| [L4 download optimization](L4_DOWNLOAD_OPTIMIZATION.md) | Copy/allocation changes and their original measurements / 拷贝、分配优化及当时的检查记录 |
| [VPN Gate validation](VPN_GATE_VALIDATION.md) | Local implementation and follow-up checks / 本地实现与后续修复检查 |
| [Chain proxy validation](CHAIN_PROXY_VALIDATION.md) | Native builds, protocol/UI tests and size comparisons / 原生编译、协议与界面测试、体积对照 |
| [Chain proxy fixes](CHAIN_PROXY_FIX_VALIDATION.md) | Import, DNS, queues, authentication, multiple endpoints and candidate-specific regressions / 导入、DNS、队列、认证、多端点修复与回归 |
| [Chain DNS follow-up](CHAIN_DNS_VALIDATION.md) | DNS receive cancellation, TCP alternatives, and explicitly authorized SOCKS-only measurements / DNS 接收竞态、TCP 备用及无 TUN 代理实测 |
| [WARP WireGuard validation](WARP_WIREGUARD_VALIDATION.md) | Discovery, storage, platform checks and artifact sizes / 扫描、存储、平台检查与产物体积 |
| [WARP WireGuard registration fix](WARP_WIREGUARD_REGISTRATION_FIX.md) | wgcf registration compatibility, endpoint parsing, disconnected tasks and regression results / 注册兼容、端点解析、未连接任务与回归验证 |
| [WARP single-port scan](WARP_WIREGUARD_SCAN_VALIDATION.md) | One port per IP, saved-job compatibility, overlapping HTTPS and validation limits / 每 IP 单端口、任务兼容、并行 HTTPS 与验证边界 |

Historical results apply only to the recorded candidate and environment. Some
records identify a baseline plus uncommitted work rather than a reproducible
source snapshot; those limits are stated in the record. Keep the original test
counts and `not_run` statuses. Record later runs separately with their exact
candidate and environment.

## Upstream references / 上游资料

The [sanitized fixtures](../oracle/fixtures/README.md) explain the interoperability
inputs. The [Go README](../oracle/go/README.md) and
[research notes](../oracle/go/RESEARCH.md) belong to the frozen upstream client,
not this GUI product; their CLI commands and platform claims do not define
Usque app behavior.

Third-party source and license notices remain with their components. Local
changes are recorded separately for
[quiche](../third_party/quiche-0.29.3/USQUE-PATCH.md),
[boring-sys](../third_party/boring-sys-4.22.0/USQUE-PATCH.md),
[smoltcp integration](../third_party/ts_netstack_smoltcp_core/PATCHES.md), and
[Wintun provenance](../third_party/wintun-0.14.1/SOURCE.md).
Do not rewrite frozen upstream documents as product instructions.
