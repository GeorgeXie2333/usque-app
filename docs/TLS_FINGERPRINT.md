# MASQUE TLS fingerprint / MASQUE TLS 指纹

The TLS fingerprint setting chooses the ClientHello preferences used when
connecting to Cloudflare WARP over HTTP/2 or HTTP/3. It is a shared network
setting, so changing accounts keeps the selection.

## Choose a profile

1. Open **Settings → Advanced network settings**.
2. In **Transport**, find **TLS fingerprint**.
3. Choose **Chrome 155** (the default) to use the fixed Chrome 155 ClientHello
   profile, or **unsafe** to use the original handshake behavior.
4. Choose **Apply changes**.

New installations and configurations without a saved fingerprint use
**Chrome 155**. Existing explicit selections are preserved; the former **Default**
option is now labeled **unsafe**. Restoring network defaults stages **Chrome 155**;
choose **Apply changes** to save the reset.

For a stable active connection, applying a different fingerprint uses the normal
controlled reconnect. When disconnected, the setting is saved for the next
connection. Other connection states follow the existing
[network-settings application rules](NETWORK_SETTINGS.md#application-rules);
a saved value is not necessarily in use by the current connection yet.
If saving fails, the page keeps the draft. If the selected profile cannot
connect on your network, choose **unsafe**, apply it, and connect again.

If the page asks you to update the engine, the connected engine does not support
fingerprint selection. The picker is disabled and a saved Chrome selection
cannot silently connect using unsafe. Update the engine before using it.

When Auto connect is enabled with Chrome 155, startup waits for the engine's
capability response. Temporary query failures are retried, including with
Custom endpoints and Zero Trust accounts. Disconnect cancels that pending
automatic connection.

## Scope and limits

- **Chrome 155** is a fixed profile, not an arbitrary uTLS template editor or
  a profile that automatically tracks installed Chrome versions.
- HTTP/2 uses the Chrome 155 TLS preferences. HTTP/3 adapts them for QUIC:
  TLS 1.3, `h3` ALPN and the required QUIC transport parameters.
- Chrome 155 keeps the same cipher order on devices with and without AES
  acceleration. unsafe retains BoringSSL's device-dependent preferences;
  selecting Chrome does not enable unsupported CPU instructions.
- The same selection applies when Auto transport tries HTTP/3 and falls back
  to HTTP/2, and to the HTTP/3 connection used by experimental L4. It does not
  change which transport is allowed.
- The setting affects MASQUE TLS only. Registration requests, Direct DNS and
  chain-proxy TLS retain their existing behavior.
- The fingerprint does not reproduce Chrome's entire traffic behavior, QUIC
  packetization, transport parameters or HTTP SETTINGS. ECH GREASE is included
  in the Chrome profile; it is not real ECH encryption or a guarantee that SNI
  is hidden.
- Chrome 155 adds H2 ALPS 17613, GREASE and ML-DSA signature identifiers, and
  28 public trust-anchor IDs from Chrome 155.0.8059.40. The IDs do not install
  trusted roots. This BoringSSL build advertises ML-DSA but cannot verify
  ML-DSA certificates/signatures; a peer requiring them fails closed. The
  [Chrome 155 experiment](CHROME155_WARP_VALIDATION.md) observed ECDSA at the
  tested Consumer WARP ingress.
- WARP client credentials and the registered endpoint public-key pin remain
  mandatory. Selecting a fingerprint does not bypass authentication or pin
  verification.

The original Chrome 131 [compatibility experiment](CHROME_FINGERPRINT_VALIDATION.md)
confirmed authenticated H2 and H3 CONNECT-IP at one Consumer endpoint on
2026-10-14. Other account types, endpoints and networks were not tested.

The [implementation validation record](TLS_FINGERPRINT_IMPLEMENTATION_VALIDATION.md)
lists the separate production-source checks and unavailable environments.
The [L4 compatibility record](L4_CHROME_FINGERPRINT_VALIDATION.md) separately
verifies Consumer L4 CONNECT and an HTTPS exchange with both profiles.

## 选择与应用

在 **设置 → 高级网络设置 → 传输协议 → TLS 指纹** 中，选择 **Chrome 155**（默认）或
**unsafe**（原握手行为），再点击 **应用修改**。这是所有账号共用的网络设置。
新安装和未保存指纹的配置默认使用 Chrome 155；已保存的显式选择保持不变。
原“默认”选项现显示为 **unsafe**。恢复网络默认值会暂存 Chrome 155，仍需应用修改后才保存。

连接稳定时修改指纹会按现有流程重新连接；断开时保存，供下次连接使用。
其他连接状态遵循[网络设置应用规则](NETWORK_SETTINGS.md#application-rules)。
保存成功不等于当前连接已使用新指纹；保存失败会保留草稿。若 Chrome 155
在当前网络连接失败，可切回 **unsafe**、应用并重新连接。
若页面提示更新引擎，指纹选择器会禁用；已保存的 Chrome 配置不会在旧引擎上
悄悄改用 unsafe 指纹连接，应先更新引擎。
启用自动连接并选择 Chrome 155 时，启动会等待引擎返回能力信息；查询暂时失败
会自动重试，自定义端点和 Zero Trust 账号也适用。主动断开会取消这次等待中的
自动连接。

Chrome 155 是固定的 TLS ClientHello 配置，H3 使用符合 QUIC 要求的适配。
不论设备是否支持 AES 加速，Chrome 配置都保持相同的密码套件顺序；unsafe 配置
继续使用 BoringSSL 根据设备能力选择的顺序，不会启用设备不支持的 CPU 指令。
自动传输模式的 H3 尝试、H2 回退及实验性 L4 的 H3 连接都使用同一选择。
它仅作用于 MASQUE TLS，不改变注册请求、直连 DNS 或链式代理的 TLS。
这不等于完整模拟 Chrome 流量，也不保证隐藏 SNI；ECH GREASE 不是真实 ECH 加密。WARP 客户端凭据认证
和注册端点公钥校验始终保留。[历史实测记录](CHROME_FINGERPRINT_VALIDATION.md)
只测试了一个 Consumer 端点，其他账号类型、端点和网络未经测试。

Chrome 155 新增 H2 ALPS 17613、GREASE 与 ML-DSA 签名算法声明，以及固定的 28 个
trust-anchor ID。这些 ID 不会导入可信根。目前只声明 ML-DSA，尚未实现其证书与
签名验证；服务端若要求该算法，连接会安全失败。[155 实测](CHROME155_WARP_VALIDATION.md)
中的 WARP 服务端均选择 ECDSA。

---

WARP is a trademark and/or registered trademark of Cloudflare, Inc. in the United States and other jurisdictions.
