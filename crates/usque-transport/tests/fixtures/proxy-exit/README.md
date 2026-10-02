# Final proxy interoperability vectors

The HTTP success line and authentication status in [contract.json](contract.json)
are transcribed from the frozen [Go HTTP proxy](../../../../../oracle/go/cmd/httpproxy.go).
The SOCKS5 TCP-only rejection follows the frozen
[Go SOCKS5 server](../../../../../oracle/go/internal/socks5.go) and
[RFC 1928](https://www.rfc-editor.org/rfc/rfc1928.html).

`proxy_exit::tests` consumes these vectors using memory-only peers.
`socks5::dns_tests` also uses the UDP rejection vector to verify that local DNS
queries still use final TCP connections. Authentication values and addresses
are synthetic. These tests do not establish live proxy
availability or external leak safety. The frozen Go source remains unchanged.
