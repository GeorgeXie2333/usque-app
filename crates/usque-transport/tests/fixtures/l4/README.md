# L4 interoperability vectors

The semantic vectors in [contract.json](contract.json) were transcribed from
[Diniboy1123/usque `api/l4proxy.go`, fixed commit
`6aa03fc97d12848dce34eedbd187fb1077b5d1ea`](https://github.com/Diniboy1123/usque/blob/6aa03fc97d12848dce34eedbd187fb1077b5d1ea/api/l4proxy.go).
Upstream is MIT-licensed, copyright 2025 github.com/Diniboy1123; its license is
retained in [LICENSE.md](LICENSE.md). No upstream Go source or captures are
bundled here. The frozen `oracle/go` is unchanged.

Classic CONNECT pseudo-headers and DATA framing follow
[RFC 9114 section 4.4](https://www.rfc-editor.org/rfc/rfc9114.html#section-4.4).
The Zero Trust SNI is a separate regression vector from
[upstream issue 126](https://github.com/Diniboy1123/usque/issues/126), not a
claim of an officially stable Cloudflare protocol or a live test result.

`l4::actor_tests` exercises the locked quiche peer in memory.
`l4::client_tests` uses a loopback QUIC peer with ephemeral pinned identities,
checks both SNIs, concurrent reuse, HTTP pre-read bytes and SOCKS UDP rejection.
`l4::tun_tests` uses an in-memory IP peer, not Wintun/VpnService.
These fixtures prove local interoperability boundaries; they do not prove
Cloudflare account reachability, external leak safety or throughput gains.
