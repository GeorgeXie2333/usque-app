# Usque protocol-only build patch

The two Windows guards in `openvpn/transport/dco.hpp` and the DCO persistence
guard in `openvpn/client/cliopt.hpp` additionally exclude
`OPENVPN_EXTERNAL_TUN_FACTORY`. The external memory TUN cannot use DCO or the
native Windows adapter setup implementation. This avoids pulling TAP headers
and native network mutation code into a build that never uses either.

In `openvpn/mbedtls/ssl/sslctx.hpp`, two logging statements put their Mbed TLS
version condition outside the macro arguments. MSVC rejects preprocessor
directives inside those arguments. Neither cryptography nor logging content
changes in those statements.

The same file explicitly calls `mbedtls_ssl_set_hostname` when the OpenVPN
caller supplies no DNS hostname. Mbed TLS 3.6.7 requires this explicit choice.
OpenVPN's CA chain verification, key usage, extended key usage, and configured
`verify-x509-name` checks remain in its verification callback, with
`MBEDTLS_SSL_VERIFY_REQUIRED` unchanged. Public VPN Gate profiles authenticate
with their embedded CA; certificate names need not match the numeric remote.
This does not add an insecure certificate-verification option. See the
[Mbed TLS API contract](https://mbed-tls.readthedocs.io/projects/api/en/v3.6.7/api/file/ssl_8h/).

The Mbed TLS configuration now honors `set_tls_version_max`, loads an explicit
`tls-version-max`, and recognizes TLS 1.3 as a valid minimum on Mbed TLS 3.6.
The memory interoperability peer sets a TLS 1.2 maximum to exercise that path.

The Mbed TLS loader also preserves `remote-cert-tls` key-usage and extended
key-usage defaults when no explicit override is present. Its key-usage check
requires the correct extension and treats Mbed TLS's zero return as success.
The memory peer tests reject an incompatible certificate role and key usage,
in addition to invalid CA and authentication failures.

`openvpn/mbedtls/crypto/cipher.hpp` explicitly selects PKCS#7 padding for CBC.
Mbed TLS 3.6.7 no longer initializes a default padding mode in cipher setup;
without this call CBC data encryption fails. No wire format or padding
validation is replaced; Mbed TLS supplies both operations.

`openvpn/ssl/mssparms.hpp` treats explicit `mssfix 0` as disabled rather
than retaining the default flag. `openvpn/ssl/proto.hpp` returns a zero MSS
adjustment before overhead arithmetic for this disabled case. Positive MSS
values retain upstream range and modifier semantics. The memory peer checks
that an explicit zero preserves TCP MSS bytes after encryption/decryption.

All other upstream source files are unchanged. `SOURCE-FILES.sha256` records
the original upstream contents, including the original versions of those files.
The archive and fixed revision are recorded in `SOURCE.md`.

The first-party native wrapper also supplies a hardware-address helper that
returns no identifier and disables free-form core logging. Those overlays are
part of the embedding application, not modifications of upstream source.
