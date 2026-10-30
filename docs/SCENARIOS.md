# Profiles and scenes / 配置与场景

Open **Profiles → Scenes** to save and reuse a local network setup. **Accounts**
is the second tab on the same page. Home's account entry opens Accounts; its
scene entry opens Scenes. Ordinary navigation remembers the last tab for the
current application run. The existing Ctrl+2 shortcut opens Profiles.

在 **配置 → 场景** 保存和使用本机网络方案；同一页面的 **账号** 标签保留账号管理。
首页账号入口直接打开账号标签，场景入口打开场景标签。普通导航记住本次运行访问的
标签，Ctrl+2 仍打开配置页。

## Create and edit / 创建与编辑

1. Choose **New scene** to capture the current confirmed saved network settings.
2. Name the scene. Keep **Use the current account**, or choose an existing WARP
   account to bind to it.
3. Edit outputs, routing, DNS, the saved chain exit, and connection preferences.
4. **Save scene** updates the template without changing the running connection.
   **Save and use** saves it and opens a separate change preview; cancelling that
   preview keeps the saved template.

选择 **新建场景**，填写名称并选择沿用当前账号或绑定一个已有 WARP 账号。
编辑输出、分流、DNS、已有出口及连接偏好后，**保存场景** 只保存模板；
**保存并使用** 先保存，再显示切换预览。取消预览不会撤销模板保存。
离开尚未保存的编辑器时，可以继续编辑或放弃修改。

Scenes contain the VPN/TUN, SOCKS5, HTTP and system-proxy switches; custom
routing, Ads, country direct choices and LAN access; WARP, direct and shared
local-proxy DNS; chain-exit references and Proton server preferences; transport
and IP-family preferences. Listener addresses and credentials, Kill Switch,
auto-connect, Android per-app selection and advanced transport tuning remain
device settings. Account-specific Zero Trust endpoints remain account-owned.
The selected chain exit retains its own final DNS configuration.

场景保存常用网络设置。监听地址与认证、Kill Switch、自动连接、Android 按应用
选择及高级调优仍沿用设备设置；Zero Trust 专属端点由账号管理。链式出口的最终
DNS 沿用所选出口配置，场景编辑不会修改出口库中的凭据或配置。

Choose an already saved and prepared exit in the editor. Import, generate,
sign in to, and manage exits in **Proxy**. Required routing data must already
be available before using a scene. A referenced configuration is retained by
normal library cleanup. A missing dependency makes that scene unavailable,
without preventing other scenes or accounts from loading.

编辑器选择已有且已准备的出口。导入、生成、登录和资源管理仍在 **代理** 页进行。
使用场景前须准备所需分流数据。正常清理会保留场景引用；依赖失效只影响对应场景。

## Use a scene / 使用场景

Choose **Use scene**, review the account and settings changes, then confirm.
The native owner submits the account and network settings together. When
disconnected, selecting a scene leaves the connection disconnected. While
connected, the preview identifies whether reconnection is needed. Hot routing
updates apply to new connections; existing streams can retain their old paths.
Reconnect when all connections need to adopt the new settings.

选择 **使用场景**，检查账号和设置变更后确认。原生层一次提交账号和网络设置。
未连接时选择场景保持断开；连接中会按差异应用或重连。规则热更新对新连接生效，
已有连接可能保持原路径；需要统一切换时应重连。

On Android, a proxy-only session cannot silently obtain the system's first VPN
permission. If permission is required, disconnect, select the scene, and connect
using the normal system authorization flow.

Android 首次启用 VPN 需要系统授权。如果当前代理连接缺少该授权，请先断开、
选择场景，再通过正常连接流程授权。

**Selected** and **Saved; waiting to apply** are not runtime success. The page
distinguishes confirmed application, failure and an unconfirmed result.
Refresh an uncertain result before deciding whether to retry; the app does not
automatically replay a lost activation request. A failed selected exit never
authorizes switching to direct access or another exit.

**已选择**、**已保存，待应用** 不代表当前连接已经采用设置。页面区分已确认应用、
失败与结果待确认。结果不确定时先刷新，应用不会自动重放丢失回复的切换请求；
所选出口失败不会擅自改为直连或其他出口。

## Changes, binding and recovery / 修改、绑定与恢复

- Editing current network settings does not overwrite the scene template.
  **Modified** indicates a difference; save a new scene, update the template,
  or use the saved scene again.
- Manually selecting another account is allowed for a bound scene. It reports
  that the account differs from the binding. Reusing the scene restores the
  bound account. Updating a bound template from current settings previews the
  binding change before saving; an unbound scene stays unbound.
- Editing a template can show **Scene has an update** while the existing
  connection continues using its previous snapshot.
- Deleting a scene keeps current network settings and shows **Custom**.
  Deleting a bound account removes its credentials normally; repair the scene's
  binding before using it again.
- **Restore previous settings** previews the latest switch's saved prior
  account and scene settings. Later account or scene-setting edits invalidate
  that recovery. Global settings remain current. Application success is checked
  separately from restoring the saved configuration.

修改当前网络设置不会覆盖模板；手动换账号可以使绑定场景显示 **账号与绑定不同**。
再次使用会恢复绑定账号。更新模板不会打断当前连接。删除模板保留当前设置并显示
**自定义**；删除账号照常移除凭据，场景需重新绑定后才能使用。
**恢复切换前配置** 只针对最近切换，先显示预览；后续修改账号或场景受管设置会使
旧恢复操作失效，防止覆盖新修改。恢复保存值与确认运行生效仍是两个结果。

## Compatibility and validation / 兼容与验证

Schema 27 adds an empty scene library and no selected scene when upgrading;
existing account and network choices are preserved. Older engines advertise no
scene capability and retain account management. Scene references contain no
account credentials, and scene values are excluded from logs and diagnostics.
There is no scene synchronization, import/export or automatic network trigger.

升级至 schema 27 时增加空场景库，保留原账号与网络设置。旧引擎仍可管理账号，
不会假装支持场景。场景引用不含账号凭据，场景内容不进入日志或诊断。
本功能不包含同步、导入导出或网络条件自动切换。

Deterministic core, owner and fake-engine UI tests cover configuration and
interaction contracts. They do not establish native VPN handoff or leak
prevention. Those checks require the isolated environments described in
[Contributing](../CONTRIBUTING.md); unavailable checks are recorded as `not_run`.
