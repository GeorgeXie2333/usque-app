# Security policy

Usque is security-sensitive networking software. Please report suspected vulnerabilities privately and give maintainers a reasonable opportunity to investigate and coordinate a fix.

## Supported versions

Usque has not published its first public beta. Until then, security fixes apply only to the latest `main` branch.

After the first public beta, the project supports:

| Version | Supported |
| --- | --- |
| Latest published beta | Yes |
| `main` | Yes |
| Older prereleases and development packages | No |

Local validation packages, fork builds, Actions development outputs, and untagged binaries are never supported releases.

## What to report privately

Examples include:

- IPv4, IPv6, DNS, route, or bypass traffic escaping the selected policy;
- Kill Switch, reconnect, crash, sleep/wake, upgrade, or uninstall leakage;
- endpoint-pin bypass, unsafe certificate acceptance, or unauthenticated pin refresh;
- exposure of a WARP Secret, private key, token, device identifier, license, or endpoint pin;
- privilege-boundary problems in the Windows Agent, WFP/Wintun control, Android `VpnService`, Binder, JNI, or secure storage;
- installer, updater, signature, provenance, dependency, or release-chain compromise;
- an unintended non-loopback proxy listener, authentication bypass, or cross-user IPC access;
- redaction failures in logs, diagnostics, errors, crash dumps, or exported data.

An unavailable feature, an ordinary connection failure, a UI defect, or expected platform permission behavior is normally a public bug rather than a vulnerability, unless it creates a confidentiality, integrity, privilege, or leak-prevention impact.

## How to report

Do not open a public Issue. After this repository becomes public, use GitHub's **Report a vulnerability** button or open the [private reporting form](https://github.com/GeorgeXie2333/usque-app/security/advisories/new).

Include:

- the affected commit or release, platform, OS version, architecture, and output mode;
- clear reproduction steps and the expected versus observed behavior;
- whether IPv4, IPv6, DNS, credentials, IPC, proxy listeners, or privileged state are exposed;
- the minimum sanitized logs, packet captures, or screenshots needed to reproduce the problem;
- whether the issue is already public or being reported elsewhere.

Never submit a real WARP Secret, private key, access token, device identifier, license value, endpoint pin, signing key, or unredacted diagnostic bundle. Replace sensitive IP addresses and identifiers consistently so packet or event relationships remain understandable.

Use the same private form with a `[CODE OF CONDUCT]` title for conduct reports. Those reports follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), not the vulnerability triage process.

## Response targets

These are good-faith targets, not a legal service-level agreement:

- acknowledge a new report within 72 hours;
- provide an initial assessment or request for more information as soon as practical;
- provide a status update at least every 7 days while the report remains active;
- normally coordinate remediation and disclosure within 90 days, adjusting when user safety, upstream dependencies, or release validation require a different schedule.

Maintainers may request more time for a complex leak, platform, or supply-chain issue. Reporters are asked not to disclose exploit details until a fix or agreed disclosure date is available.

## Coordinated disclosure

Confirmed vulnerabilities are handled in a private GitHub Security Advisory. The project will develop and validate a fix, assess affected versions, prepare release and upgrade guidance, and request a CVE when appropriate. Public credit is offered only with the reporter's consent.

The first public beta remains blocked until its declared targets pass endpoint-pin, credential, redaction, IPv4, IPv6, DNS, reconnect, crash, sleep/wake, upgrade, uninstall, signature, provenance, and independent leak-prevention gates.

## Research boundaries

Test only systems, devices, profiles, and accounts that you own or are explicitly authorized to assess. Do not disrupt Cloudflare or third-party services, access another person's data, retain unnecessary personal data, or use denial-of-service techniques. Stop and report privately if testing exposes data or privileged access outside the intended scope.
