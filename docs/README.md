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
| [Network Doctor](network-doctor.md) | Standard and Deep checks, limits, and local reports / 本地诊断与结果边界 |
| [Encrypted direct DNS](encrypted-direct-dns.md) | System, DoH, DoT, configuration, and privacy / 直连 DNS 配置及隐私 |
| [Experimental Zero Trust](ZERO_TRUST_EXPERIMENTAL.md) | Enrollment, unsupported features, and validation requirements / 实验性注册、限制与验证要求 |
| [Security policy](../SECURITY.md) | Private vulnerability reporting and supported versions / 私密漏洞报告与支持范围 |

## Develop and maintain / 开发与维护

| Document | Read it for / 用途 |
| --- | --- |
| [Contributing](../CONTRIBUTING.md) | Toolchain setup and change-scoped checks / 工具链准备与检查矩阵 |
| [Repository safety contract](../AGENTS.md) | Workstation limits, isolation, and authoritative sources / 开发机安全边界与权威来源 |
| [Implementation progress](IMPLEMENTATION.md) | Source-tree milestones, not proof of a test run / 源码实现进度，不等同于测试通过 |
| [GUI development](../apps/usque_gui/README.md) | Editing workflows and native UI conventions / 界面交互与布局约定 |
| [Release process](RELEASE.md) | Candidate preparation, approval, signing, and publication / 候选包、审批、签名与发布 |
| [Code signing policy](CODE_SIGNING.md) | Official identities, key handling, and rotation / 官方签名身份、密钥管理与轮换 |
| [GitHub governance](GITHUB_GOVERNANCE.md) | Repository checks, permissions, and maintainer rules / 仓库检查、权限与维护规则 |
| [Reliability testing](RELIABILITY_TESTING.md) | Deterministic checks, isolated environments, and evidence contracts / 确定性检查、隔离环境与证据契约 |
| [Network-quality rollback](network-quality-rollback.md) | Reviewed build-only rollback and regression requirements / 构建级回滚及回归验证 |

Read the [Code of Conduct](../CODE_OF_CONDUCT.md) before participating. Changes
must preserve the safety rules and follow the contribution checklist; this index
does not replace either. Protected-runner validation is supplemental and does
not gate publication. Missing, failed, or `not_run` evidence is never a pass.

## Technical contracts / 技术契约

| Document | Read it for / 用途 |
| --- | --- |
| [Reliability invariants](reliability-invariants.md) | Append-only invariant identifiers and safety properties / 不可复用的标识与安全属性 |
| [Network-quality metrics](network-quality-metrics.md) | Availability, counters, queue bounds, RTT, and PMTU / 指标语义与资源边界 |
| [Network-quality IPC](network-quality-ipc.md) | Wire fields, compatibility, event coalescing, and UI samples / 协议字段、兼容性与采样 |
| [H3 path infrastructure](h3-path-infrastructure.md) | Socket ownership, exact generations, and migration / 路径所有权、网络代次与迁移 |
| [H3 client reliability](h3-client-reliability.md) | Receive handling, fragmentation policy, GOAWAY, and recovery / 接收、分片策略与恢复 |
| [HTTP/3 congestion control](congestion-control.md) | Algorithm selection, deferred session settings, and BBRv3 / 算法选择、延迟生效与 BBRv3 |
| [QUIC UDP receive buffer](UDP_RECEIVE_BUFFER.md) | Windows/Android default, OS readback, diagnostic fields and evidence limits / 共用接收缓冲默认值、实际回读与证据边界 |
| [Network settings](NETWORK_SETTINGS.md) | Field patches, durable saves, session application, and verification / 字段补丁、保存与会话生效契约 |
| [Direct DNS threat model](direct-dns-threat-model.md) | Scoped trust boundaries, assumptions, and review evidence / 专题信任边界、假设与审查依据 |

These documents describe contracts or scoped reviews, not measured performance
or external leak guarantees. The executable sources remain authoritative for
behavior; a source change must update affected documentation without weakening
fail-closed checks. Preserve protobuf numbers and invariant identifiers.

## Historical records / 历史记录

| Record | Scope / 范围 |
| --- | --- |
| [Implementation baseline](implementation-baseline.md) | PR-00 source, toolchain, and unavailable-lab baseline / PR-00 基线 |
| [Network-quality acceptance](network-quality-acceptance.md) | PR-01–PR-12 implementation and test matrix, with later correction notice / 阶段验收及后续更正 |
| [PMTU review and fixes](pmtu-path-fixes.md) | Candidate-specific defects, corrections, and regression results / 特定候选版本的修复记录 |
| [Receive-buffer experiments](RECEIVE_BUFFER_EXPERIMENTS.md) | Retired Android A/B builds, user observations, and evidence limits / 已结束的安卓对照试验与证据边界 |

Do not update historical test counts to look current or turn `not_run` into a
pass. Later evidence needs its own exact commit, candidate, and environment.

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
