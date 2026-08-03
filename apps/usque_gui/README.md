# Usque GUI

Flutter hosts the native Usque interface for Windows, macOS, Android, and Android TV. It intentionally contains no WebView.

This directory is a development target, not a production VPN. Desktop hosts launch the Rust sidecar and communicate over current-user IPC. Android can either register a Consumer identity through Rust or validate a manually entered identity before Android Keystore persistence. H3/H2, proxy, TUN, reconnect, and fail-closed platform paths are implemented, but the clean-machine, hardware, stability, performance, and independent leak gates are not complete. See the repository [README](../../README.md) and [implementation gates](../../docs/IMPLEMENTATION.md).
