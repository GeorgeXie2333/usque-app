# Security policy

## Development status

Usque has not reached its first public beta. The current repository contains security boundaries and fail-closed scaffolding, but it has not completed the interoperability, privilege-separation, or leak-prevention audit required for real VPN use.

Do not report an unavailable native data channel as a vulnerability; it is deliberately disabled until it can safely own the tunnel. Route, DNS, firewall, endpoint-pin, credential, and redaction failures are security issues.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Use GitHub's private vulnerability reporting feature for this repository and include:

- affected commit and platform;
- reproduction steps;
- expected and observed routing/DNS behavior;
- whether IPv4, IPv6, DNS, credentials, or local proxy listeners are exposed;
- sanitized logs or packet captures.

Never include a WARP Secret, private key, access token, device identifier, license value, endpoint pin, or unredacted diagnostic bundle.

No response-time SLA is offered before the first public beta.

## Release security bar

A public beta is blocked unless all declared platforms pass:

- endpoint-pin mismatch and rotation tests;
- IPv4, IPv6, DNS, reconnect, crash, sleep/wake, and uninstall leak tests;
- credential-vault and diagnostic-redaction tests;
- reproducible dependency locks, SBOM, SHA-256, provenance, and signature checks.
