# WARP WireGuard upstream reference

The endpoint address pools and UDP port list in `usque-core` reference
[vernette/warpscout](https://github.com/vernette/warpscout) at commit
[`b49ef5e8a466164a64d669951521b58944346bc8`](https://github.com/vernette/warpscout/tree/b49ef5e8a466164a64d669951521b58944346bc8).
The registration and Cloudflare metadata requests were also reviewed against
that source. The implementation uses Usque's existing Rust transport and
platform encryption. No Go executable, runtime, network stack, terminal UI,
AmneziaWG implementation, registration relay, shared private key, or download
speed test is included.

The pinned data contains fourteen IPv4 /24 pools, two IPv6 /48 pools, four
primary UDP ports and fifty alternate UDP ports. IPv4 full discovery tests
193,536 distinct IP/port combinations. IPv6 discovery samples addresses; it
does not enumerate a /48. Updating this data requires an explicit source
change and review of the reference revision.

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
