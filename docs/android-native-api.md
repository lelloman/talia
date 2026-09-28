# Native sign-in and Overview

The Android app now has native connection/sign-in and a read-only Overview. The
matching server endpoints must be deployed before using it against production;
older servers return the app's unsupported-server message. The default address
is `https://talia.lan.lelloman.com`. APK 3 adds authenticated gateway access
away from home after enrollment on home Wi-Fi; see
[Android remote access](android-remote-access.md). Users can choose
another HTTPS origin. No insecure HTTP or certificate-bypass option is exposed.

## Authentication boundary

The existing confidential LelloAuth client remains owned by the server. Android
opens its system browser for sign-in and never handles a provider password or
provider token. This slice uses a server-brokered native session rather than
registering another public client at LelloAuth.

1. The app generates a random 32-byte verifier and sends its S256 challenge to
   `POST /native/start`. The server returns a random attempt ID, an authorization
   URL, and a ten-minute lifetime. Pending grants are bounded to 256 per process.
2. The browser opens `GET /native/authorize?attempt=…`. Each attempt can start
   once. The existing LelloAuth flow still checks browser binding, state, nonce,
   provider PKCE, token identity and introspection.
3. On success the browser offers a `com.lelloman.talia://signin` return link.
   The published APK 2 shell has no callback intent filter, so VPK 3 users
   switch back to Talìa manually; foreground polling completes the same exchange.
   The normal development variant supports the return link.
   This link carries no credential and is only a navigation hint. Authenticating
   the handoff does not depend on a custom-scheme handler being exclusive.
4. Foreground Android exchanges the attempt ID and verifier at
   `POST /native/exchange`. Pending returns 202; a completed grant is consumed
   exactly once and returns a random opaque `n.`-prefixed Talìa session handle.
   Wrong proofs, expired attempts and replays fail closed. A server restart or
   lost exchange response requires restarting sign-in.
5. The handle is stored using AES-GCM with an Android Keystore key. Provider
   refresh/access tokens remain encrypted server-side. Existing token refresh,
   provider revocation checks and absolute session expiry still apply.

`GET /native/session`, `GET /native/overview` and `POST /native/logout` accept
these native bearer handles. Browser cookies alone cannot authorize those
endpoints, and native handles cannot authorize browser-only or machine APIs.
Unexpected Origin headers are rejected. Logout deletes the server session before
clearing local state; network failure leaves a retryable sign-out operation.
The app never persists Overview data. Expiry clears identity, credentials and
in-memory data; a connection outage retains the last successful response with
an explicit warning. Permission denial clears protected data.

## Overview projection

Overview checks the current Talìa administrator role on every request. Viewers
receive HTTP 403; dashboard-scoped viewer Overview is outside this slice.

- Service samples come from the existing `homelab-summary` variable's targets.
  This is Prometheus scrape reachability, not a complete application-health claim.
  Responses include sample time; absent, bad-quality or >120-second-old samples
  are stale. Only service name/status are projected, up to 200 entries.
- The most recent 20 report runs are sorted by creation time and ID. The response
  contains run ID, report ID, status, creation time, and bounded content
  title/summary. It excludes scripts, raw step outputs, delivery destinations and
  actor identity. No report execution or cancellation is added here.
- Empty successful responses differ from loading, offline, forbidden and expired
  states. Foreground refresh runs every 30 seconds; pending sign-in polls every
  two seconds. Requests have timeouts, a 1 MiB response limit and no redirects.

## Verification

```sh
cargo test --manifest-path engine/Cargo.toml native --offline
cargo test --manifest-path engine/Cargo.toml deployment::tests --offline
cd android
ANDROID_HOME="$HOME/Android/Sdk" ANDROID_SERIAL=emulator-5588 ./gradlew \
  :app:connectedNormalDebugAndroidTest :app:lintNormalDebug \
  :app:assembleParavoidAndroidRelease
```

Server tests use a local mock OIDC provider and cover one-time PKCE redemption,
expiry/replay, browser/native separation, logout, role denial, stale samples and
report projection. Device tests cover encrypted storage, HTTPS validation, PKCE
handoff, expiry, network outages and revoked permissions. Production sign-in
needs a deployed backend and an interactive LelloAuth login; fixture results
must not be described as a production end-to-end login.
