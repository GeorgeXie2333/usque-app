## Summary

<!-- Explain the user-visible or technical outcome and why it is needed. -->

## Change scope

- Platforms: <!-- Windows / Android / Android TV / shared -->
- Outputs: <!-- VPN / SOCKS5 / HTTP / system proxy / none -->
- Security or privileged-state impact: <!-- Describe explicitly, or write "None". -->

## Validation

<!-- List exact commands and environments. Do not claim unsafe host tests. -->

- [ ] Relevant format, lint, and unit tests pass.
- [ ] Protocol or parser changes include malformed-input/interoperability coverage.
- [ ] Privileged network changes include isolated cleanup and leak-prevention evidence.
- [ ] UI changes cover English/Chinese, themes, focus, scaling, and TV navigation as applicable.
- [ ] New logs, errors, and diagnostics were reviewed for sensitive data.
- [ ] Documentation and lockfiles are updated where required.

## Not tested

<!-- State anything not run and why. "None" is acceptable. -->

## Checklist

- [ ] The PR title follows Conventional Commits.
- [ ] No signing key, WARP Secret, token, license, device ID, endpoint pin, diagnostic bundle, or generated package is committed.
- [ ] This change does not add WebView UI, insecure TLS, automatic telemetry, or automatic diagnostic upload.
- [ ] I read `CONTRIBUTING.md`, `SECURITY.md`, and the Code of Conduct.
