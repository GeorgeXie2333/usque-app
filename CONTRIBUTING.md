# Contributing to Usque

Usque is still establishing its security-critical foundations. Discuss large protocol, privilege, storage, or UX changes before investing in a long implementation.

## Required checks

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked

cd apps/usque_gui
flutter pub get --enforce-lockfile
flutter analyze --no-pub
flutter test --no-pub
```

The archived Go project under `oracle/go` is a behavioral oracle. It must remain buildable and its attribution must remain intact:

```shell
cd oracle/go
go test ./...
```

## Change requirements

- Protocol changes need unit/property tests and a Go-oracle interoperability fixture.
- Parser and frame changes need malformed-input tests; externally reachable parsers need fuzz coverage.
- TUN, route, DNS, firewall, system-proxy, sleep/wake, update, and uninstall changes need cleanup and leak-prevention tests.
- New logs must be reviewed for secrets, tokens, keys, licenses, pins, and sensitive addresses.
- UI changes must work in English and Simplified Chinese, light and dark themes, keyboard focus, 200% scaling, and Android TV D-pad navigation.
- Use Lucide icons. Do not add emoji as interface icons.
- Do not introduce WebView UI or an insecure TLS toggle.

Keep dependency additions narrow, pinned through the relevant lockfile, and compatible with every declared target.
