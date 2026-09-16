## Summary

<!-- Explain the user-visible or technical outcome and why it is needed. -->

## Change scope

- Platforms: <!-- Windows / Android / Android TV / shared -->
- Outputs: <!-- VPN / SOCKS5 / HTTP / system proxy / none -->
- Security or privileged-state impact: <!-- Describe explicitly, or write "None". -->

## Validation

<!-- List exact commands and environments. Check only completed items; annotate inapplicable items with N/A. Record missing checks below. Do not claim unsafe host tests. -->

- [ ] Relevant format, lint, and unit tests pass.
- [ ] Protocol or parser changes include malformed-input/interoperability coverage.
- [ ] UI changes cover English/Chinese, themes, focus, scaling, and TV navigation as applicable.
- [ ] New logs, errors, and diagnostics were reviewed for sensitive data.
- [ ] Documentation and lockfiles are updated where required.

## Isolated validation

<!-- For privileged network, installer, update, or uninstall changes, report cleanup and leak-prevention validation here. Use N/A when this scope is unaffected. -->

- Status: <!-- passed / failed / not_run / N/A -->
- Evidence or reason not run: <!-- Identify the exact candidate and isolated environment for any evidence. -->

Protected-runner execution and reports are supplemental and do not gate
publication. Missing or failed evidence is never a pass. All applicable
deterministic checks, compile-only gates, and release approvals remain required.

## Not tested

<!-- State anything not run and why. "None" is acceptable. -->

## Checklist

- [ ] The PR title follows Conventional Commits.
- [ ] No signing key, WARP Secret, token, license, device ID, endpoint pin, diagnostic bundle, or generated package is committed.
- [ ] This change does not add WebView UI, insecure TLS, automatic telemetry, or automatic diagnostic upload.
- [ ] I read `CONTRIBUTING.md`, `SECURITY.md`, and the Code of Conduct.
