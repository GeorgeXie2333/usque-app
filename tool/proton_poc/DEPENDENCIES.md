# PoC dependencies and provenance

This separate Python experiment does not change product dependencies. Install
only published wheels using [requirements.lock](requirements.lock) with
`--require-hashes --only-binary=:all:`. Every downloaded wheel digest was checked
against its versioned PyPI release metadata. The lock includes all published
wheel hashes for each exact release so it can resolve on supported Python hosts;
an unavailable compatible wheel is a failure, not permission to compile sources.

| Package | Exact version | License in inspected wheel | Purpose |
| --- | --- | --- | --- |
| httpx | 0.28.1 | BSD-3-Clause | Verified HTTPS and explicit proxy routing |
| httpcore | 1.0.9 | BSD-3-Clause | HTTP transport |
| h11 | 0.16.0 | MIT | HTTP/1.1 framing |
| socksio | 1.0.0 | MIT (`.dist-info/LICENSE`) | SOCKS5 remote-hostname transport |
| anyio | 4.15.1 | MIT | httpx dependency |
| idna | 3.20 | BSD-3-Clause | Hostname handling |
| certifi | 2026.10.18 | MPL-2.0 | Mozilla CA bundle |
| PyNaCl | 1.6.2 | Apache-2.0; bundled libsodium ISC | Ed25519 and reviewed X25519 conversion |
| cryptography | 46.0.3 | Apache-2.0 OR BSD-3-Clause | X.509 parsing/key/time checks; SRP modulus signature verification |
| bcrypt | 5.0.0 | Apache-2.0 | Proton SRP v4 password derivation |
| cffi | 2.1.1 | MIT-0, additional bundled notices retained | Native-library interface |
| pycparser | 3.11 | BSD-3-Clause | cffi dependency |
| typing_extensions | 4.16.0 | PSF-2.0 | anyio dependency |
| ruff | 0.16.0 | MIT | Repository-required validation only |

Their own license files remain inside the installed wheels. No dependency wheel
or native library is vendored here. A distribution that bundles these wheels
must retain all bundled license and notice files, including transitive native
components. Certifi's CA data keeps its MPL notice; importing it does not relicense
this independent tool. GPL Android/Core sources are protocol references and are
not copied into the MIT implementation. See the [fixed protocol
contract](../../docs/PROTON_VPN_PROTOCOL.md) for source versions and references.

The initial Linux x86-64 / CPython 3.14 environment occupied approximately 65 MiB
and the downloaded wheel directory 18 MiB (including Ruff). Paths were under
`/tmp`; no global Python environment, GUI SDK, Cargo target, Android emulator or
system network setting was changed. These are allocated disk sizes from `du -sh`,
not installed product size measurements.
