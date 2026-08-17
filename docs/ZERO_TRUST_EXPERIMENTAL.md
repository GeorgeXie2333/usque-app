# Experimental Cloudflare Zero Trust enrollment

Usque can experimentally register a new profile with a Cloudflare Zero Trust organization on Windows and Android. This feature is intentionally narrower than the Cloudflare One Client: it uses the organization account to create a persistent device identity, then carries Internet traffic through Usque's existing MASQUE tunnel.

It does not implement organization policy synchronization, device posture, managed DNS, Split Tunnels, private-network routing, WARP-to-WARP, service-token enrollment, or automatic client-session reauthentication. Gateway policy can still affect traffic on Cloudflare's side, but Usque does not claim full Cloudflare One Client compatibility.

## Enrollment flow

1. Create a profile and select **Cloudflare Zero Trust (Experimental)**.
2. Enter the organization's single-label team name.
3. Accept the existing Cloudflare terms and open `https://<team>.cloudflareaccess.com/warp` in the system browser.
4. Complete the organization's Access/IdP login.
5. On Android, choose Usque if Android presents an app chooser. Windows users paste the complete `com.cloudflare.warp://.../auth?token=...` callback URL manually. Android also keeps the manual fallback.
6. Usque exchanges the one-time assertion for a device ID/token and P-256 MASQUE enrollment, saves the returned Zero Trust endpoint, and commits the identity and profile atomically.

The Access assertion is never written to the profile, vault, Android saved state, or logs. It is bounded to 64 KiB, accepted only for the expected team and exact callback shape, held in memory, consumed once, and discarded after submission. A restarted Android process has no active login and rejects the callback.

## Identity boundaries

- Consumer profiles cannot be converted to Zero Trust profiles.
- A Zero Trust profile can sign in again only to the same organization. This refreshes its device registration and endpoint.
- Provider and organization are mirrored in a versioned, non-secret profile binding. The vault metadata must match it; missing or conflicting metadata is invalid and may only be repaired by signing in to the bound organization. Unbound pre-feature profiles remain legacy Consumer identities.
- The registered Zero Trust endpoint is owned by that binding. Generic profile edits and network-default resets cannot replace it with a Consumer endpoint.
- Zero Trust profiles have no Usque WARP License operation. License copy, bind/unbind, and WARP Secret export are hidden and rejected by the engine.
- Deleting a profile removes only local credentials. It does not revoke the device registration in the organization dashboard; an administrator must remove residual or test registrations there.
- Registration never falls back to a Consumer endpoint, Consumer SNI, or Consumer identity after a Zero Trust failure.

## Platform behavior

Windows does not register or take over the `com.cloudflare.warp` protocol and the MSI has no protocol association. The complete callback must be pasted into the setup dialog.

Android declares a restricted browsable intent for `com.cloudflare.warp://*.cloudflareaccess.com/auth`. An in-memory login session additionally requires the exact expected team. `onCreate` and `onNewIntent` feed the same one-shot gate; callbacks without an active login, for another team, after cancellation, after process restart, or after the first accepted callback are discarded. Co-installation with the official WARP app is allowed to produce Android's normal app chooser.

Re-authenticating the connected profile disconnects the active tunnel before replacing credentials and reconnects only after the refreshed endpoint is loaded.

## Release gate

The enrollment exchange and `zt-masque.cloudflareclient.com` contract are experimental. Do not describe or ship this feature as production-supported until a dedicated real organization passes all of the following:

- enrollment policy permits the test identity and does not require unsupported posture;
- the dashboard attributes the device to the expected user;
- the returned IPv4 and IPv6 endpoints remain in Cloudflare's documented Zero Trust ranges;
- H3, H2 fallback, IPv4, IPv6, endpoint-pin refresh, and restart reconnection work through SOCKS5/HTTP on Windows without starting Windows VPN mode or changing routes/DNS;
- Android VPN validation passes only on an isolated test device or emulator;
- the vault, logs, diagnostic bundle, profile JSON, and Android state contain no Access assertion;
- the administrator removes every test or orphaned registration afterward.

If live validation fails, stop the release. Do not silently fall back to Consumer registration or probe undocumented API variants.
