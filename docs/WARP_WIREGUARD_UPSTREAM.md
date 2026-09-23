# WARP WireGuard upstream reference

The endpoint address pools and UDP port list in `usque-core` reference
[vernette/warpscout](https://github.com/vernette/warpscout) at commit
[`b49ef5e8a466164a64d669951521b58944346bc8`](https://github.com/vernette/warpscout/tree/b49ef5e8a466164a64d669951521b58944346bc8).
The Cloudflare metadata requests were also reviewed against that source.
WireGuard registration instead follows [ViRb3/wgcf](https://github.com/ViRb3/wgcf)
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

The pinned data contains fourteen IPv4 /24 pools, two IPv6 /48 pools, four
primary UDP ports and fifty alternate UDP ports. Current discovery tests one
common port per IP: 70 IPv4 or 10 IPv6 quick candidates, 3,584 full IPv4
candidates, or one target candidate. Pool scans rotate 2408/500/1701/4500
between addresses; a target uses 2408. The full port table remains only for
interpreting older job cursors. IPv6 discovery does not enumerate a /48.
Updating this data requires an explicit source change and review of the
reference revision; saved plan versions must retain their port ordering.

Upstream defaults differ from Usque's original exhaustive implementation:
`flags.go` sets a two-second request timeout and ten ordinary tunnel workers;
nesting (`-through`) defaults to one worker. `discovery.go` first samples
reachable ports, and `tunnel.go` normally returns the first working port per IP.
`-sweep-ports all` explicitly requests the exhaustive IP/port behavior.
Usque's single-port rule is stricter: an unsuccessful IP is not retried on any
other port. It still requires in-tunnel trace HTTPS and separately preserves
IPv4/IPv6 Cloudflare metadata, with five-second request deadlines.

Upstream's WARP-in-WARP documentation describes a WireGuard outer tunnel and
warns about concurrent nested probes. Usque retains its MASQUE outer tunnel
and serializes discovery. It reports measured Cloudflare countries and does
not promise that changing an inner endpoint changes the exit country.

The following notice is also included in the application's license screen.

## License

MIT License

Copyright (c) 2026 Nikita S.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
