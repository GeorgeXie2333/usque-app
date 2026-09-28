# WARP WireGuard registration reference

WireGuard registration follows [ViRb3/wgcf](https://github.com/ViRb3/wgcf)
at commit [`ace873cbaa618365beebde5790a7fb3481e5a211`](https://github.com/ViRb3/wgcf/tree/ace873cbaa618365beebde5790a7fb3481e5a211),
specifically `cloudflare/api.go`, its API contract and ClientHello fixture.
It uses API `v0a5641`, the corresponding Android headers/body, then an
authenticated device read to obtain the configuration. Address entries with
placeholder port zero use the returned port list (default 2408); they never
produce a zero-port WireGuard profile. No obsolete `warp_enabled` PATCH is needed.

The implementation uses Usque's existing Rust transport and
platform encryption. No Go executable, runtime, network stack, terminal UI,
AmneziaWG implementation, registration relay, shared private key, or download
speed test is included.

The registration TLS profile reuses the existing BoringSSL dependency and
matches wgcf's initial Android TLS 1.2 ClientHello. WebPKI verifies the fixed
API hostname, validity, server usage and CA trust. Ordinary private HTTPS
requests retain their existing TLS configuration; API requests stay inside
MASQUE. The wgcf MIT notice is included in
[`assets/licenses/wgcf.txt`](../apps/usque_gui/assets/licenses/wgcf.txt) and the
application license screen.

Endpoint scanning, pool/port tables, Cloudflare trace/metadata probes and their
runtime assets have been removed. Configuration generation/import and manual
endpoint editing remain. Historical behavior is recorded in
[the scan validation](WARP_WIREGUARD_SCAN_VALIDATION.md); it does not describe
the current product.
