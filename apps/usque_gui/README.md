# Usque GUI

Flutter host for the Windows, macOS, Android, and Android TV UI. There is no WebView.

Desktop builds start the Rust sidecar and talk to it over current-user IPC. Android talks to the Rust library inside the isolated `:vpn` process.

Build and test commands are in the repository [README](../../README.md) and [CONTRIBUTING.md](../../CONTRIBUTING.md). Feature status is in [docs/IMPLEMENTATION.md](../../docs/IMPLEMENTATION.md).

## Editing and navigation

- Accounts select WARP identities; network settings are shared across accounts.
- Proxy addresses, ports, and DNS are drafts until **Apply changes** succeeds. Credentials use their separate **Save credentials** action and never enter the network draft.
- Advanced settings have a persistent apply bar and a back-navigation guard for unapplied edits. Reset loads defaults into the draft; it does not apply them immediately.
- Inline output statuses distinguish enabled configuration from observed runtime state, including unavailable, limited, and failed states. Labels and icons carry the meaning without decorative badge surfaces.
- Home uses open sections: the original connection ring and Kill Switch state, shared upload/download traces, and a connection overview with quality/diagnostic actions. Desktop places the connection control beside the status/location readouts; narrow content or text above 150% stacks them. Connected sessions show exit region, duration, and protocol; idle/error sessions explain the configured outputs. On phones, full addresses and output runtime details remain under **Connection details**.
- Desktop and mobile traces share the timestamped 60-second quality history. New samples with unchanged values are retained; repaint timers do not generate observations. Source-aligned time slots tolerate scheduling jitter, and averages use actual elapsed time. Missing samples stay gaps; delayed, paused, unavailable, and disconnected readings are identified explicitly. The view does not start additional probes or persist traffic history.
- Settings groups connection/protection, proxy/routing, and application preferences. Home links to network quality when the engine supports it.

## Native UI composition

- `ContentSection` and `ContentHeading` group ordinary content with typography and spacing, not a card background. `ContentList` separates adjacent entries; `ActionRow` provides native ink, keyboard/D-pad activation and a visible, layout-stable focus outline. `InlineStatus` pairs a readable state label with a supplementary icon.
- Accounts are continuous rows. The active identity has a light selection wash, marker and explicit status; clicking the row itself never switches accounts. Identity configuration, activation, rename and delete remain separate controls.
- Settings and proxy editors use open groups with a maximum form width of 880 logical pixels. General page content remains capped at 1120. Navigation retains its existing 760/1050 breakpoints and phone/rail behavior.
- Network quality uses continuous metric sections rather than a card grid. Diagnostics keeps its checks, timeline, export and destructive actions distinct. Dialog field groups and onboarding forms do not nest card surfaces.
- Background fills are reserved for controls, selection, warnings, dialogs and save bars. The legacy `Panel` remains available for explicitly isolated danger areas; it has not been globally made transparent. Keep the connection ring's drawing, proportions, phase mapping and motion unchanged.
- Brand colors, bundled fonts and locale fallbacks are unchanged. Primary controls have a minimum 48-pixel target; status indicators do not rely on color alone. No new probes, sampling timers or persistence are introduced by the layout.

The workflow and native-layout widget tests use a fake engine and cover connected detail expansion, repeated collapse/restore, selectable-value copying, action-row focus and keyboard/D-pad activation, status-label contrast, narrow/landscape layouts, 200% text, and reduced motion. The `golden` suite additionally checks real-font layouts and exact Windows-pinned screenshots, including accounts, settings, proxy editors, onboarding, diagnostics, dialogs and expanded home details; neither suite starts a VPN or proves native networking behavior.
