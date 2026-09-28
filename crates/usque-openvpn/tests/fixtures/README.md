# Public protocol test identities

These generated RSA identities are public test fixtures for the in-memory
OpenVPN peer. They do not identify a VPN Gate server, WARP account, user, or
release signer. Never use them outside tests. The CA private key is discarded.
Certificates are valid from 2020 through 2050 so test execution is independent
of the generation date.

`tls-crypt.key` is a fixed, deliberately public synthetic key used only by the
memory peer. The password-only peer accepts `fixture-user` / `fixture-password`.
The protocol tests cover accepted and rejected credentials without an OS socket.

`client-encrypted.key` is the same public client fixture encrypted with the
public test passphrase `fixture-key-password` (OpenSSL PKCS#8, AES-256-CBC).
It exercises the private-key-password path during an actual memory handshake.
