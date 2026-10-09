# v0.3.1 release readiness review / 发布准备审查

Review date: **2026-10-09 (Asia/Singapore)**. Scope: documentation preparation
and read-only release audit. **Version coordination and final-candidate gates
remain open; this review does not certify v0.3.1 for publication.**

审查日期：**2026-10-09（新加坡时间）**。范围为文档更新与只读发布审查。
**版本同步与最终候选门槛仍待完成，本记录不证明 v0.3.1 已可发布。**

## Source and baseline / 源码与基线

- Reviewed source: `740d611db305afe84eb04130fe82c984212530b4`, local branch
  `dev`; the worktree was clean before this documentation patch.
- Baseline: published `v0.3.0`, peeled tag commit
  `274bc3c2a51f78d141972ea78d0828b7d43f8f8e`; 13 later commits.
- This review includes uncommitted documentation changes. It is not a final
  merged source identity, package manifest or approved signing run. Revalidate
  the final commit after version coordination and merge.
- Read-only GitHub queries on the review date reported
  [v0.3.0](https://github.com/GeorgeXie2333/usque-app/releases/tag/v0.3.0) as the
  latest published release, published at 2026-10-07 19:01:26 Asia/Singapore,
  with 18 named assets. Listing those assets does not verify their bytes,
  signatures, SBOMs or attestations.
- Remote `main` was still the baseline commit. The Actions query for the
  reviewed `dev` SHA returned zero runs. That SHA cannot satisfy the release
  gate's current-main and exact-SHA successful `ci.yml` push-run requirements.

上述结果仅针对注明的源码和审查日期。未创建或移动标签、提交或推送改动、读取签名
私钥、构建安装包、批准签名或发布。远端状态须在最终发布前重新核对。

## Documentation review / 文档审查

The English-first bilingual release template now describes `v0.3.0..HEAD`:

| Change | Documentation and limits / 文档与边界 |
| --- | --- |
| DIRECT/REJECT/PROXY and Ads | [Routing](ROUTING.md): normalized domain/CIDR rules, specificity, conflicts, local refusal, Ads default-off and unavailable-catalog behavior / 规则规范化、优先级、冲突、本地拒绝、Ads 默认关闭及缺库行为 |
| LAN across outputs | VPN/TUN, HTTP forwarding/CONNECT and supported SOCKS5 TCP/UDP; explicit rules, platform exclusions and listener exposure retain their separate boundaries / 各输出访问与显式规则、平台排除、监听暴露边界 |
| H3 PMTU startup | [H3 reliability](h3-client-reliability.md): ordinary traffic continues during discovery; oversized probe rejection retains ordinary queued traffic; no measured performance claim / 探测期间普通流量继续、探测包拒绝不丢弃普通队列、不宣称实测性能提升 |
| DNS editing | [WARP DNS](WARP_DNS.md) and [Direct DNS](encrypted-direct-dns.md): complete DoH URL, editable Cloudflare drafts, preserved saved values; WARP bootstrap optional, Direct bootstrap required / 完整 DoH 地址、可编辑默认草稿、保留已有值及不同引导 IP 要求 |
| Proxy DNS and navigation | [Chain proxy](CHAIN_PROXY.md), [VPN Gate](VPN_GATE.md): removed local-proxy DNS controls retain old settings internally; Add proxy chain DNS still exists; VPN Gate has one shared chain editor / 本地代理 DNS 控件移除但保留旧设置，链出口 DNS 仍可设置，VPN Gate 统一编辑入口 |
| Android controls and observation | [Installation](INSTALLATION.md#quick-settings-control), [Network Doctor](network-doctor.md): tile remains recoverable; hidden UI polling pauses without stopping VPN or proving energy savings / 磁贴可恢复、隐藏界面暂停轮询但不停止 VPN、不宣称实测省电 |
| Android icons | [GUI reference](../apps/usque_gui/README.md): opaque brand-orange adaptive background, separate monochrome themed icon / 不透明橙色自适应背景与独立主题图标 |

The six root READMEs, installation examples, implementation progress, release
process, signing policy, governance, reliability reference and index distinguish planned
v0.3.1 from the unchanged executable v0.3.0 contract. Previous v0.3.0 features
are not advertised as new v0.3.1 work. The historical
[v0.3.0 readiness record](RELEASE_V0.3.0_READINESS.md) keeps its original
date, source, test counts and unavailable checks.

六语言 README 与发布相关文档区分“v0.3.1 计划”和“当前 v0.3.0 执行契约”。安装
示例使用计划包名并要求替换为实际下载版本；历史记录不改写为本次通过证据。

## Upgrade and privacy review / 升级与隐私审查

The v0.3.0 tag defines configuration schema **23**; reviewed source defines
**24**. Legacy custom domains and CIDRs migrate to DIRECT, preserving countries.
CIDRs no longer create physical bypass routes; the application data plane owns
custom routing. **v0.3.0 rejects migrated schema-24 data**, as do older clients.
Reinstalling an old package is not reverse migration. Prepare a recoverable
pre-upgrade backup if needed; do not lower schema numbers, overwrite migration
backups or delete recovery records to force a downgrade.

v0.3.0 的配置 schema 为 **23**，当前源码为 **24**。旧域名和 CIDR 转为 DIRECT，
国家选择保留；自定义 CIDR 不再安装物理绕过路由。**v0.3.0 无法读取迁移后数据**，
重装旧包不会逆转迁移。需要回退时应提前安排可恢复备份，不得改 schema 或删除恢复记录。

Agent protocol **3**, recovery journal **5**, sanitized recovery export **2**,
VPN record **3** and HTTP/SOCKS5 record **5** remain unchanged in the reviewed
source; they do not prove platform upgrade or downgrade compatibility.
The DoH URL editor retains the stored name/port/path representation. Removal of
local proxy DNS controls preserves saved DNS settings and local-DNS warnings;
it is not a silent migration to a different privacy policy.

Ads without a valid catalog reports unavailable and permits connections;
custom rules remain active. Encrypted app DNS can hide domains, platform
exclusions can bypass the Engine, and explicit DIRECT rules can allow LAN even
when automatic LAN access is off. These limits are stated in the user guides.
Do not advertise a system-wide firewall, external leak proof, throughput gain
or battery saving based on source inspection or fake-engine tests.

缺少有效 Ads 库时会提示不可用并允许连接，自定义规则仍生效。应用加密 DNS、平台
排除以及显式 DIRECT 有各自边界。源码审查、模拟 Engine 测试不证明全系统防火墙、
外部无泄漏、吞吐提升或省电。本次仅修改文档，未改变清理、拒绝不安全操作或隐私策略。

## Remaining release gates / 发布待办

| Requirement | Review status / 审查状态 | Acceptance / 完成条件 |
| --- | --- | --- |
| Version synchronization | **Open**: Cargo 0.3.0, Flutter 0.3.0+24, all registered catalogs and workflows still target v0.3.0 / 待同步 | Separate change to 0.3.1 / 0.3.1+25, all first-party lock entries, 21 catalogs, CI and Release; pass the exact v0.3.1 version command |
| Platform version ordering | **Planned**, no actual v0.3.1 package / 仅计划 | Android base 25, derived 1025/2025/4025/25; MSI 0.3.199 and Agent PE 0.3.199.0; inspect actual packages and supported update paths |
| Final current-main SHA | **Open**: dev SHA differs from remote main / 尚未合入 | Merge final source and docs, require tag SHA equal current main; never reuse v0.3.0 |
| Required hosted checks | `not_run` for the final v0.3.1 candidate / 最终候选未运行 | Successful PR Check / gate, CI / gate and Build / gate; successful ci.yml push run for exact tagged SHA |
| Live tag/environment protections | **Open**: reviewers observed, tag restriction not established / 审批配置可见，标签权限未确认 | Maintainer verifies tag creation permissions, current main rules and environment deployment/approval settings before tagging |
| Signing identity and Android registration | `not_run` / 未核验 | Same pre-1.0 identities, validated public DER certificate SHA-256 variables and Registered application-ID/certificate pair; approved release-signing run |
| Signed packages and candidate integrity | `not_run`; no v0.3.1 staged manifest / 无候选清单 | Eight exact primary packages; identity/architecture/ABI, MSI/EXE checks, APK 16 KiB/ZIP alignment, extraction/compression, no shipped symbols/kernel_blob.bin/Vulkan validation layers, manifest sizes/hashes, eight SBOMs and attestations |
| Flutter symbol retention | `not_run` / 未归档 | Three matching Windows/Android symbol artifacts with source, version and binary identities, retained separately from the 18 public release assets |
| Publication | `not_run`, no approval inferred / 未审批 | Approved release-publish rechecks the same immutable staged bytes; final checksums, manifest, SBOMs, provenance and bilingual rendered notes |
| Optional protected validation | `not_run` / 未运行 | Windows snapshot VM, dedicated Android, independent network observer and performance lab only; exact-candidate sanitized evidence if available; optional and non-blocking |

### Live protection observations / 线上保护配置观察

Both `release-signing` and `release-publish` reported one required reviewer,
self-review prevention disabled, administrator bypass enabled and
`deployment_branch_policy: null`. The ruleset list returned one active branch
ruleset, **Protect main**, with strict `PR Check / gate`, `CI / gate`,
`Build / gate`, pull requests, resolved review threads, squash/linear history,
no deletion and no force pushes. Its required approving-review count was zero
and `require_code_owner_review` was true. This last value should be reconciled
with the sole-maintainer conditional wording in [governance](GITHUB_GOVERNANCE.md).
No active tag ruleset was returned; this query alone does not establish all
legacy tag permissions or maintainer-only creation. No setting was changed.

两个环境均有一名必需审查者，但允许自行审批及管理员绕过，未报告环境分支/标签限制。
main 规则集有三项严格检查，审批人数为零，但代码所有者审查开关为 true，需与
单维护者政策核对。未返回活动标签规则集，不等于已证明维护者专属标签权限；本次未改配置。

### Order of work / 执行顺序

1. Coordinate version metadata as specified in [Preparing v0.3.1](RELEASE.md#preparing-v031),
   then recheck release prose, version arithmetic and schema facts.
2. Run every applicable exact command in [Contributing](../CONTRIBUTING.md#required-checks-by-change-scope)
   on the final source, including Windows helper-based Rust gates, Flutter widget
   and Windows golden suites, Android Rust/Kotlin gates and hosted compile-only
   builds. Preserve lockfiles and record unavailable platform checks separately.
3. Merge through the required checks, requery live protections and exact-main CI,
   and let the maintainer create only the new v0.3.1 tag.
4. Obtain signing approval, inspect the staged candidate and matching symbols,
   and review rendered notes with validated public signer fingerprints.
5. Obtain publication approval and verify the final eight primary / 18 total
   assets and their hashes, identities, inventory and provenance from the same
   run. Optional protected reports never substitute for these requirements.

先同步版本并重查文案，再按 CONTRIBUTING 完成最终源码适用检查、合入与线上保护
复核，之后才创建新标签、审批签名、核对同一暂存候选和符号，最后审批发布并验证资产。

## Commands and results / 命令与结果

Python used: verified bundled **3.12.14**. In PowerShell, `$releasePython` below
was the following executable; commands started at the repository root:

```powershell
$releasePython = 'C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe'
& $releasePython tool/check_repository_policy.py
git diff --check
& $releasePython -m unittest discover -s tool -p "test_release_contract.py" -v
& $releasePython tool/release_contract.py verify-version --root . --tag v0.3.0 --android-version-code 24
& $releasePython tool/release_contract.py verify-version --root . --tag v0.3.1 --android-version-code 25
```

| Check | Result / 结果 |
| --- | --- |
| Repository policy | **Passed**, `REPOSITORY_POLICY_OK` / 通过 |
| `git diff --check` | **Passed** / 通过 |
| Release contract and renderer tests | **Passed**, 29 tests; one earlier prose assertion failed, corrected in the template, then all 29 passed / 29 项通过，先前文案断言失败已修复并重跑 |
| Existing v0.3.0 version contract | **Passed**; proves only unchanged version surfaces / 通过，仅说明旧版本元数据一致 |
| Planned v0.3.1 version contract | **Rejected**: Cargo workspace version '0.3.0' does not match 'v0.3.1' / 被拒绝，版本尚未同步 |
| Heading anchors | Local normalization audit passed for 105 fragment links across 76 Markdown files, including two planned template fragments; important edited anchors also reviewed / 本地规范化审查通过，重要修改锚点另行审阅 |
| Rendered v0.3.1 preview | **Passed**, existing renderer with fixture fingerprints, official-only links and six installer links; no official package evidence / 测试指纹预览通过，不是正式软件包证据 |
| MSI version conversion | **Passed**, `v0.3.1` → `0.3.199`; no package built or installed / 映射通过，未构建或安装包 |

Sandboxed version attempts could not resolve the workspace path; the completed
version and renderer checks used approved unrestricted execution. The v0.3.1
rejection above is from the completed read-only contract check, not that sandbox
error. No validation check was weakened.

The temporary anchor audit and rendered fixture output stayed under ignored
`target/doc-review-v0.3.1`. Preview fingerprints were 64 repetitions of `b`
(Windows) and `c` (Android), not official identities. The additional commands
were:

```powershell
& $releasePython target/doc-review-v0.3.1/check_doc_anchors.py
& ./tool/convert_to_msi_version.ps1 -SemVer v0.3.1
& $releasePython tool/release_contract.py render-release-notes --template .github/RELEASE_NOTES_TEMPLATE.md --tag v0.3.1 --repository GeorgeXie2333/usque-app --windows-signer-sha256 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb --android-signer-sha256 cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc --output target/doc-review-v0.3.1/release-notes-fixture.md
```

版本检查最初受沙箱路径访问限制；重试后旧版本合同通过，v0.3.1 因真实版本不符被
拒绝，未放宽检查。文档检查结果不等于整套 CI、原生行为或发布候选已经通过。

Read-only GitHub commands used `gh api` with JSON projections:

```shell
gh api repos/GeorgeXie2333/usque-app/releases/latest
gh api repos/GeorgeXie2333/usque-app/commits/main
gh api "repos/GeorgeXie2333/usque-app/actions/runs?head_sha=740d611db305afe84eb04130fe82c984212530b4&per_page=100"
gh api repos/GeorgeXie2333/usque-app/git/ref/tags/v0.3.0
gh api repos/GeorgeXie2333/usque-app/git/tags/beacee24574c4237999efc9e9582f37d8a575851
gh api repos/GeorgeXie2333/usque-app/rulesets
gh api repos/GeorgeXie2333/usque-app/rulesets/20729045
gh api repos/GeorgeXie2333/usque-app/environments/release-signing
gh api repos/GeorgeXie2333/usque-app/environments/release-publish
```

Key external references were opened separately: Cloudflare's
[DoH endpoint](https://developers.cloudflare.com/1.1.1.1/encryption/dns-over-https/make-api-requests/)
and [DoT endpoint](https://developers.cloudflare.com/1.1.1.1/encryption/dns-over-tls/),
Microsoft's [signature command](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.security/get-authenticodesignature),
Android's [developer verification](https://developer.android.com/developer-verification)
and [apksigner](https://developer.android.com/tools/apksigner),
[GitHub attestation verification](https://cli.github.com/manual/gh_attestation_verify)
and the [upstream Ads category](https://github.com/v2fly/domain-list-community/blob/master/data/category-ads-all).
This is not exhaustive external-URL validation or live resolver reachability.
Planned v0.3.1 asset/tag links are not expected to resolve before publication.

## Not run / 未运行

Native Rust, Flutter/widget/golden, Android Kotlin/JNI and architecture builds
were not rerun in this documentation-only task. Their prior records are not
final v0.3.1 evidence. Local MSI/APK packaging, official signing, Android Console
registration checks, candidate/package verification, approvals and publication
were not performed. These remain required where applicable in the final release
workflow; this review does not waive them.

Windows installation/upgrade/connected-uninstall/Wintun recovery, Android
phone/TV lifecycle/Doze/Always-on/Lockdown, external IPv4/IPv6/DNS/leak checks and
controlled repeated performance sampling are all **`not_run`**. Protected-runner
validation is supplemental and non-blocking. Do not invoke its runner or alter
workstation networking to fill these gaps. Any later report must match the
exact signed candidate and required isolated environment; raw captures and
restricted evidence stay private.

本次未重跑平台源码测试与构建，也未打包、签名、核验最终候选或发布。隔离环境的
安装升级、清理恢复、Android 生命周期、外部泄漏及性能测试均为 **`not_run`**。
受保护验证是可选补充，不阻止发布，但缺失结果不得标为通过，原始受限证据不得公开。
