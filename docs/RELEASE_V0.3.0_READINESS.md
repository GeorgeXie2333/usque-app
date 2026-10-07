# v0.3.0 release readiness review / 发布准备审查

Review date: **2026-10-07 (UTC)**. Historical review status: documentation
preparation; **the reviewed source was not yet a releasable v0.3.0 candidate**.

审查日期：**2026-10-07（UTC）**。本记录保留当时的文档准备状态与证据范围，
**不代表最终 v0.3.0 候选已通过发布门槛**。

The observations and test results below belong to that historical source
review. Later version coordination uses `0.3.0+24`, but does not retroactively
turn these results into final-candidate validation. Follow the current
[release contract](RELEASE.md#preparing-v030) and the exact published manifest
for final commit, package and approval evidence.

以下观察与测试结果保留其历史审查范围。后续版本统一使用 `0.3.0+24`，但这不会
将历史结果改写为最终候选验证。最终提交、软件包和审批依据以当前[发布合同](RELEASE.md#preparing-v030)
及实际发布清单为准。

## Reviewed source and evidence / 审查范围与证据

- Reviewed source: `ae12d6750049df8da30cdfa20990debb891db62d`, local branch `dev`.
  The worktree was clean before this documentation change.
- Comparison baseline: published v0.2.9, commit
  `57e0e9c33d6b9908ee49d93860d136753a9d1fc1`; 29 later source commits.
- This review includes the accompanying uncommitted documentation patch. It
  does not identify a final committed v0.3.0 source snapshot or signed package
  manifest. Revalidate after the version change and final merge.
- Read-only GitHub queries on the review date reported v0.2.9 as the latest
  published release, remote `main` at the comparison baseline, and no Actions
  runs for the reviewed `dev` HEAD. These observations are time-specific;
  recheck the exact final release commit.
- Both `release-signing` and `release-publish` returned one required reviewer.
  The API also reported self-review prevention disabled and administrator
  bypass allowed. The ruleset list returned one active branch ruleset,
  `Protect main`. Its detail required `PR Check / gate`, `CI / gate` and
  `Build / gate` with strict checks, pull requests and resolved review threads.
  The approving-review count was zero, matching the documented sole-maintainer
  policy. Both environment APIs returned `deployment_branch_policy: null`;
  no environment branch/tag restriction was reported. The custom-policy list
  endpoints returned HTTP 404, so they provide no additional restriction
  evidence. The queries did not establish maintainer-only release-tag
  permissions. No server-side setting was changed. Verify tag permissions and
  approval configuration against [governance](GITHUB_GOVERNANCE.md) before release.

本次核对发布流程、六语言 README、安装说明、发布说明模板及当前技术参考。
历史记录的原始候选、测试数量和 `not_run` 状态未改写；历史通过结果不能覆盖
当前源码。文档检查也不能代替最终提交的 CI、签名、包体或原生网络验证。

## Documented changes / 已核对的发布增量

| Change / 变化 | Current reference / 当前说明 |
| --- | --- |
| WARP exit DoH/DoT, separate from direct and chain DNS / WARP 出口加密 DNS，与直连及链 DNS 分离 | [WARP DNS](WARP_DNS.md) |
| Confirmed Zero Trust endpoint edits and a Home risk notice / Zero Trust 端点风险确认与首页提示 | [Network settings](NETWORK_SETTINGS.md#zero-trust-endpoint-editing--zero-trust-端点编辑) |
| Native localized Windows setup/removal, tray status, background notices, saved window placement and shortcuts / Windows 安装卸载、托盘、通知、窗口位置与快捷键 | [Installation](INSTALLATION.md), [Windows lifecycle](windows-lifecycle.md), [GUI conventions](../apps/usque_gui/README.md) |
| Established Android ordinary CONNECT-IP recovery after protection failure / Android 已建立普通 CONNECT-IP 的保护失败恢复 | [H3 reliability](h3-client-reliability.md) |
| Chain UDP receive corrections and smoltcp 0.14.0 / 链式 UDP 接收修复与网络栈更新 | [Chain DNS record](CHAIN_DNS_VALIDATION.md), [workspace dependencies](../Cargo.toml) |
| Home traffic charts, readable exit details, refreshed branding and screenshots / 首页图表、出口信息、品牌与截图更新 | Six root READMEs and [implementation progress](IMPLEMENTATION.md) |

The release-note template now describes changes since v0.2.9. Its download and
verification links become usable only when the approved workflow publishes the
matching v0.3.0 assets. The template is not an announcement that publication
has occurred.

## Compatibility and corrected claims / 兼容性与已修正表述

- Configuration schema is **23**. Schema 22 adds WARP DNS; schema 23 adds
  per-account Zero Trust endpoint overrides. Registered Zero Trust addresses
  remain identity metadata. The supported value and migrations are defined by
  [configuration](../crates/usque-core/src/config/mod.rs) and
  [storage](../crates/usque-core/src/storage.rs).
- v0.2.9 supports schema 21 and rejects newer configurations. After migration,
  reinstalling v0.2.9 does not make the new settings readable. Do not lower a
  stored schema number manually. Keep any older-version recovery material
  separately and privately before upgrading; no reverse migration is promised.
- Agent protocol **3**, recovery journal schema **5**, and recovery export
  schema **2** remain unchanged. Those facts do not establish downgrade
  compatibility or successful upgrade/restoration tests.
- Supported frontend changes can retain MASQUE. Replacing a SOCKS/HTTP
  listener closes its existing client flows; this is not a promise that all
  application connections remain uninterrupted. VPN attachment follows the
  [core classification](../crates/usque-core/src/reconfigure.rs), including
  routing, DNS, endpoint protection and chain requirements.
- Zero Trust, L4 and BBRv3 keep their documented experimental boundaries.
  DoH/DoT does not imply anonymity or measured leak prevention, and local
  charts/Doctor results do not establish independent leak or performance tests.

## Publication requirements / 发布前必要事项

The executable contracts remain authoritative. Follow [release preparation](RELEASE.md#preparing-v030)
and the exact [change-scoped checks](../CONTRIBUTING.md#required-checks-by-change-scope)
when implementing the version change; do not use this review as a replacement.

| Requirement / 必要事项 | Status at this review / 本次状态 | Completion evidence / 完成依据 |
| --- | --- | --- |
| Coordinated v0.3.0 version metadata / 统一版本元数据 | **Open**: Cargo and its workspace lock entries remain 0.2.9; Flutter remains 0.2.9+23; registered locale catalogs, CI version check and release tag contract remain v0.2.9 | Change all coordinated inputs, advance Android versionCode beyond 23, then pass `verify-version` for v0.3.0 |
| Exact release commit on current main / 精确发布提交位于当前 main | **Open**: reviewed dev HEAD differs from remote main; no v0.3.0 tag is created by this review | Final merged commit must equal main when the tag workflow verifies it |
| Successful exact-commit CI / gate / 精确提交 CI | **Not available** for reviewed HEAD | Successful `push` CI workflow for the final SHA; PR or older-SHA results are insufficient |
| Windows and Android package/signing gates / 平台包与签名门槛 | `not_run` in this documentation task | Approved tag workflow builds all eight packages, verifies architecture, signer, symbols and locked inputs |
| Staged candidate integrity / 暂存候选完整性 | `not_run`; no candidate manifest exists for v0.3.0 in this review | Exact package hashes, signer fingerprints, manifest, eight SPDX SBOMs and attestations |
| Protected signing/publication approvals / 签名与发布审批 | `not_run`; no approval inferred | Required `release-signing` and `release-publish` approvals; publish reverifies the same staged bytes |

No local package, downloaded development artifact or previous-release checksum
can substitute for a failed official workflow. This review authorizes no tag
push, installation, signing-secret access, artifact upload or publication.

本次实际运行的 v0.3.0 版本检查被拒绝：Cargo workspace 仍为 `0.2.9`。
先完成版本同步与精确提交的 CI，再进入正式候选构建和审批。

## Maintainer checks / 维护者核验

- Confirm the Android application ID and actual release certificate registration
  in Android Developer Console before making a current registration claim.
  Public documentation describes the policy; this source review cannot inspect
  the account's registration state.
- Record the maintainer-supplied OpenVPN, WireGuard and WARP protocol SVG
  provenance/permission. [The chain guide](CHAIN_PROXY.md) currently says the
  upstream source/license is not recorded; missing documentation alone does
  not establish that permission is absent.
- Review rendered bilingual release notes for the final version, package names,
  upgrade warning and approved signer fingerprints. Renderer tests with inert
  fingerprints do not verify a real signing identity.

## Validation record / 本次验证记录

Commands run from the repository root using the verified bundled Python
**3.12.14** executable:

```powershell
$releasePython = 'C:\Users\George\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe'
& $releasePython tool/check_repository_policy.py
git diff --check
& $releasePython -m unittest discover -s tool -p "test_release_contract.py" -v
& $releasePython tool/release_contract.py verify-version --root . --tag v0.2.9 --android-version-code 23
& $releasePython tool/release_contract.py verify-version --root . --tag v0.3.0 --android-version-code 24
```

The v0.3.0 command uses **24 as a proposed next build number**, not an already
updated or approved Android versionCode. The final chosen code must be greater
than the published 23 and consistent across the coordinated inputs.

| Check / 检查 | Result / 结果 |
| --- | --- |
| Repository policy: local link targets, UTF-8 and repository rules | **Passed**, `REPOSITORY_POLICY_OK` |
| Whitespace (`git diff --check`) | **Passed**, exit 0 |
| Release-contract/renderer unit tests | **Passed**, 29 tests |
| Local heading references in modified/new Markdown | **Reviewed**: 68 references in 22 documents resolved; external URL availability is not covered by this check |
| Existing v0.2.9 version contract | **Passed**; does not establish v0.3.0 readiness |
| Proposed v0.3.0 version contract | **Rejected**: Cargo workspace version 0.2.9 does not match v0.3.0 |

Initial sandboxed version-check attempts hit Windows `Path.resolve` access
restrictions. The v0.3.0 rejection above comes from the completed read-only
check outside that sandbox, not from the permission error.

An early policy run started before this new readiness file existed and reported
its links missing; the completed rerun passed. The first full renderer suite
found the required bilingual release-summary wording missing. The template was
corrected without changing tests or weakening the contract; the full rerun
passed all 29 tests.

Read-only GitHub commands used for the source/release observations:

```shell
gh run list --repo GeorgeXie2333/usque-app --commit ae12d6750049df8da30cdfa20990debb891db62d --limit 20 --json databaseId,workflowName,headSha,status,conclusion,url
gh api repos/GeorgeXie2333/usque-app/branches/main --jq .commit.sha
gh release view --repo GeorgeXie2333/usque-app --json tagName,isDraft,isPrerelease,publishedAt,url
```

The two environment APIs and repository ruleset API were also read for
protection metadata. No secret or certificate material was read. Public
installation-command references were reviewed; the Wiki Home fetch returned a
cache miss, so its availability remains unconfirmed rather than a broken-link
finding.

Rust, Flutter, Kotlin, native builds, MSI authoring, release signing and packaging
were **not run for this Markdown-only change**. Run applicable deterministic
and compile checks for the final version/source change as required by
[Contributing](../CONTRIBUTING.md). No aggregate source-check result is claimed.

Windows snapshot-VM, Android device/TV, independent network-observer and
controlled performance validation are **`not_run`**. They are optional,
supplemental evidence, not publication prerequisites; their absence is never a
pass. The public repository skips protected lab jobs, and raw lab evidence must
stay private. Any later evidence must match the exact candidate and required
isolated environment. See [runner isolation](RELEASE.md#runner-isolation-boundary).
