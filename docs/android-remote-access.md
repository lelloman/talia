# Android remote access

Talìa uses the shared Homelab Access Gateway at `https://access.lelloman.com`
and its source Kotlin client from `../homelab/access-gateway/client-kotlin`.
Override the checkout with `HOMELAB_ACCESS_CLIENT_DIRECTORY` when building.
No system VPN, private CA import, or public Talìa web endpoint is needed.

## Setup and usage

Install the new shell APK 0.1.2 (3), with embedded minified payload 5. APK 2
cannot accept this feature as a VPK: network-state permission, `singleTop`, and
the exact `com.lelloman.talia://gateway/callback` intent filter change its contract.
The APK 3 baseline is pinned under `android/paravoid-baselines/apk-3`.

On home Wi-Fi, sign in with LelloAuth. For the default Talìa server, gateway
registration runs first, then the app starts Talìa's existing browser sign-in.
Users must have access to both the Homelab Access and Talìa applications in
LelloAuth. These are separate credentials and authorization checks.
After Talìa sign-in, switch back to the app to complete its foreground exchange.
Existing signed-in users can select **Enable remote access** in Settings without
losing their Talìa session. **Renew remote access** starts a fresh gateway
registration; **Disable remote access** revokes the current gateway device.

At each native request, the app verifies the Talìa health endpoint over a
Wi-Fi-bound socket with a two-second timeout and normal TLS hostname/certificate
verification. If unavailable, the shared client tunnels the same HTTPS authority
through the gateway. The outer connection uses Android's default network. This
works on mobile data and unrelated Wi-Fi once authorized; an unrelated network
cannot impersonate the LAN service merely by using the same private address.
The Overview displays Home network or Secure tunnel after selecting a route.
There is no automatic replay of failed requests when the route changes.
Non-default user-entered servers remain direct HTTPS connections.

Initial Talìa sign-in still requires home Wi-Fi: the external browser cannot use
the app's private tunnel. If Talìa's session expires away from home, sign-in must
wait until the browser can reach the private endpoint. VPN-based/custom endpoint
onboarding has not been added to the new managed setup flow.

Gateway credentials and pending enrollment are encrypted in a separate
Keystore-backed preference namespace. Callback state, enrollment, code, expiry,
exact URI, and duplicate parameters are validated by the shared client.
A gateway denial does not erase the independent Talìa session. Sign-out revokes
Talìa first over the selected route, clears its private data, then revokes gateway
access. If gateway revocation fails, its local credential remains for an explicit
retry through Disable remote access; the server device can also be revoked by
an operator. Logs contain exception class names only, never tokens or URLs.

## Server configuration

Homelab adds service `talia` targeting **`caddy:9444`**, with authority
`talia.lan.lelloman.com:443`, and client `talia` with the exact native callback.
Port 9444 is internal to Docker and serves only Talìa; unmatched hosts get 404.
Do not point a raw tunnel at shared Caddy port 443, which exposes other virtual
hosts. The original LAN route and source restrictions remain in place.

The shared gateway grants all its authorized users access to all configured
services. Talìa continues to enforce its own session and administrator checks.
The app does not reuse Casina credentials. Gateway policy reload disconnects
existing tunnels, which reconnect normally.

## Validation and rollout (2026-09-28)

- Homelab configuration committed as `8dcf15e` and applied with validated reloads.
- Remote rollback copies: `/home/lelloman/talia-gateway-backup-20260928T021732Z`.
  Restore the two files in place (they are bind-mounted), validate, then reload
  Caddy and `access-gateway admin reload`. No state database or secrets changed.
- Dedicated TLS health endpoint: 200; unauthenticated native session: 401;
  wrong Host: 404; public unauthenticated gateway tunnel: 401.
- Both Talìa and the gateway report healthy.
- Shared Kotlin client tests, Android lint, debug/release builds, eleven device
  tests, and the new shell compatibility check passed.
- Signed/minified emulator enrollment reached browser handoff and retained pending
  enrollment across a cold start, with an empty crash buffer. Cancellation returned
  to sign-in, and setup with emulator Wi-Fi disabled showed the home-Wi-Fi prompt.
- Full authenticated tunnel use, account/device revocation and a real mobile-data
  round trip require the user's LelloAuth enrollment and remain acceptance work.
- APK 0.1.2 (3) and embedded payload 5 are published and verified on LelloStore
  (publication revision 10). The APK is 4,174,873 bytes; the payload is 3,666,377
  bytes. Install the APK update from LelloStore before enrolling on home Wi-Fi.
