# Security policy

Usque handles network state and credentials. Report suspected vulnerabilities privately and give maintainers time to look at them.

## Supported versions

| Version | Supported |
| --- | --- |
| Latest published release | Yes |
| `main` | Yes |
| Older prereleases and development packages | No |

Local validation packages, fork builds, Actions development outputs, and untagged binaries are not supported releases.

## What to report privately

Examples:

- IPv4, IPv6, DNS, route, or bypass traffic escaping the selected policy
- Kill Switch, reconnect, crash, sleep/wake, upgrade, or uninstall leakage
- endpoint-pin bypass, unsafe certificate acceptance, or unauthenticated pin refresh
- exposure of a WARP Secret, private key, token, device identifier, license, or endpoint pin
- privilege problems in the Windows Agent, WFP/Wintun, Android `VpnService`, Binder, JNI, or secure storage
- installer, updater, signature, provenance, dependency, or release-chain compromise
- an unintended non-loopback proxy listener, authentication bypass, or cross-user IPC access
- redaction failures in logs, diagnostics, errors, crash dumps, or exported data

An unavailable feature, an ordinary connection failure, a UI bug, or expected platform permission behavior is usually a public bug, unless it creates a confidentiality, integrity, privilege, or leak impact.

## How to report

Do not open a public Issue. Use GitHub's **Report a vulnerability** button or the [private reporting form](https://github.com/GeorgeXie2333/usque-app/security/advisories/new).

Include:

- the affected commit or release, platform, OS version, architecture, and output mode
- reproduction steps, plus expected versus observed behavior
- whether IPv4, IPv6, DNS, credentials, IPC, proxy listeners, or privileged state are exposed
- the smallest sanitized logs, captures, or screenshots that show the problem
- whether the issue is already public or reported elsewhere

Never send a real WARP Secret, private key, access token, device identifier, license, endpoint pin, signing key, or raw diagnostic bundle. Replace sensitive addresses and identifiers consistently so the relationships still make sense.

For conduct reports, use the same form with a `[CODE OF CONDUCT]` title. Those are handled under [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), not this vulnerability process.

## Response times

These are usual targets, not a promise:

- acknowledge a new report within 72 hours
- send an initial assessment or ask for more detail as soon as we can
- update at least every 7 days while the report is open
- aim to fix and disclose within 90 days, longer if user safety, an upstream dependency, or release validation needs it

Please do not publish exploit details until a fix or an agreed date exists.

## Coordinated disclosure

Confirmed issues are handled in a private GitHub Security Advisory. We will write and test a fix, list affected versions, prepare upgrade notes, and request a CVE when that makes sense. Public credit is only with the reporter's consent.

How official packages are signed and published is in [docs/RELEASE.md](docs/RELEASE.md).

## Research

Test only systems, devices, profiles, and accounts you own or are allowed to test. Do not disrupt Cloudflare or other people's services, keep extra personal data, or use denial-of-service. If testing exposes data or access outside the intended scope, stop and report privately.
