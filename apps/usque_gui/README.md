# Usque GUI

Flutter host for the Windows, macOS, Android, and Android TV UI. There is no WebView.

Desktop builds start the Rust sidecar and talk to it over current-user IPC. Android talks to the Rust library inside the isolated `:vpn` process.

Build and test commands are in the repository [README](../../README.md) and [CONTRIBUTING.md](../../CONTRIBUTING.md). Feature status is in [docs/IMPLEMENTATION.md](../../docs/IMPLEMENTATION.md).

## Editing and navigation

- Accounts select WARP identities; network settings are shared across accounts.
- Proxy addresses, ports, and DNS are drafts until **Apply changes** succeeds. Credentials use their separate **Save credentials** action and never enter the network draft.
- Advanced settings have a persistent apply bar and a back-navigation guard for unapplied edits. Reset loads defaults into the draft; it does not apply them immediately.
- Output badges distinguish enabled configuration from observed runtime state, including unavailable, limited, and failed states.
- Phone Home uses three cards: a centered connection control with Kill Switch state, shared upload/download traces, and a connection overview with quality/diagnostic actions. Connected sessions show exit region, duration, and protocol; idle/error sessions explain the configured outputs. Full addresses and output runtime details remain under **Connection details**.
- Desktop and mobile traces share the timestamped 60-second quality history. New samples with unchanged values are retained; repaint timers do not generate observations. Source-aligned time slots tolerate scheduling jitter, and averages use actual elapsed time. Missing samples stay gaps; delayed, paused, unavailable, and disconnected readings are identified explicitly. The view does not start additional probes or persist traffic history.
- Settings groups connection/protection, proxy/routing, and application preferences. Home links to network quality when the engine supports it.

The workflow widget tests use a fake engine and cover connected detail expansion, repeated collapse/restore, selectable-value copying, narrow/landscape layouts, 200% text, and reduced motion. The `golden` suite additionally checks real-font layouts and exact Windows-pinned screenshots, including expanded details in both themes; neither suite starts a VPN or proves native networking behavior.
