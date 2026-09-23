# Configurable alerts

Implementation is tracked by [TALIA-47](https://crumbles.lelloman.com/w/LLPR/TALIA/47).
This contract captures the accepted design; qualification records separately state
which paths have implementation evidence. Alerts precede deployment and homelab
migration. LLM investigations and Crumbles delegation follow migration.

## Identity, lifecycle and authority

A stable string key identifies the monitored condition (for example
`disk-space:host-a:/var`). Repeated observations update the current occurrence.
Recovery resolves it; a subsequent breach creates a new occurrence, with fresh
acknowledgement and delivery state. History retains previous occurrences.
Server state is authoritative. Acknowledgement records actor/time and is global,
but does not resolve a condition. Policies decide which actions stop on acknowledgement
and which transitions reset it. Dashboard presentation remains active until recovery.

Alert operations use explicit permissions: view, acknowledge, silence, configure,
and raise/update. They are independently grantable and audited. Client identity is
not permission. Revision/occurrence guards reject stale acknowledgement and mutation;
no client silently queues acknowledgement while disconnected. Dashboard failure must
not prevent the host from presenting or acknowledging alerts.

## Policies

Policies are reusable definitions referenced by bindings with independent parameters,
inputs and private state. A binding maps named engine input IDs to a policy, a stable
alert key and labels. Each evaluation receives current sample value, quality and
original timestamp, current occurrence, parameters, private state and trusted time.
Bounded server JavaScript returns the current condition, severity, message, stage,
updated private state and requested named actions. Missing/stale data must be handled
explicitly; evaluation failure freezes notification progression and is visible.
The script has no network or credential access. Host APIs commit state and schedule
only declared actions; I/O yields outside SQLite transactions.

Named stages define actions, delays, repeat intervals, stop-on-acknowledgement and
optional acknowledgement reset on entry. No actions means dashboard-only. A response
policy can show a warning immediately, request Telegram after ten minutes, repeat
until acknowledged, then request push on critical escalation. Resolution cancels
obsolete active actions and can schedule separately declared recovery actions.

Shared definition updates adopt the latest version for every binding without restart.
Instance parameter updates affect only that binding. Preserve occurrence history and
acknowledgement unless the policy explicitly resets it. Removing an occupied stage
requires an explicit old-to-new stage mapping; otherwise reject the entire update.
Configuration updates use optimistic version guards. Changed policy/action definitions
invalidate obsolete pending deliveries before the next dispatch.

## Silences, scheduling and recovery

Silences match a key or labels, have an expiry and a recorded actor/reason. They suppress
notifications, not evaluation or dashboard visibility. On expiry evaluate current state;
do not replay missed reminders. Delay and repeat schedules persist in SQLite.
Before dispatch or retry recheck occurrence, stage, acknowledgement, policy revision,
silence, expiry and evaluation health. Delivery is tracked per destination. Successful
destinations are not retried because a different destination failed. Configure retry
delay and maximum attempts. Failed delivery remains visible independently of alert state.

On restart mark in-flight delivery as uncertain, re-evaluate current conditions before
sending and coalesce missed intervals to at most one overdue action per destination.
Then schedule forward from current time, not from a catch-up loop. No network operation
holds the engine queue or a SQLite transaction. Acknowledgement cannot undo an external
effect already dispatched. An accepted message with a lost response can be delivered
twice after retry: no exactly-once claim is made.

## Destinations and Android

Named destinations select email, Telegram, Android push, or browser Web Push. Policies reference names;
credentials and provider addresses are host-managed and never enter guest JS. Each
installation has a stable ID across restart/update and a separate renewable token.
Reinstall/data clearing creates a new identity. Registration requires authentication;
a client cannot claim another user's devices. Destinations can target one installation,
a named group or every installation owned by a user. Token rotation must not create
a second installation. Revoked/disabled destinations cannot dispatch pending work.

Push is an expiring snapshot. Opening it fetches current state; direct notification
acknowledgement and in-app acknowledgement both require server confirmation and stale
occurrence guards. Offline failure is visible and requires an explicit retry. The
installation identity may also own dashboard assignments; it is distinct from the
short-lived live-dashboard instance ID. A push token is not authentication.

Email uses configurable SMTP transport with TLS and optional authentication; Telegram
uses the Bot API. FCM is the implementation default for Android push; the shared delivery envelope
and lifecycle remain independent of the provider adapter.
Production credentials and designated recipients are deployment configuration, never
committed test fixtures. Qualification uses local provider fixtures and an Android
emulator, and must distinguish fixture evidence from real provider delivery.

## Public operation envelope

The authenticated alert API accepts `op` and operation-specific `args`. Mutations use
expected configuration/occurrence revisions and stable request IDs where effects can
be retried. Operations cover snapshot/history, observe, acknowledge, policies/bindings,
silences, destinations and device registration. MCP exposes discoverable dedicated
alert tools with schemas. Both platform hosts expose the same read/action operations
to their shared JS layer; no bearer token enters a dashboard definition or ViewModel.

Reference provider documentation: [Telegram Bot API](https://core.telegram.org/bots/api),
[SMTP transport](https://docs.rs/lettre/latest/lettre/transport/smtp/index.html),
[Android FCM setup](https://firebase.google.com/docs/cloud-messaging/android/get-started).

## Provider configuration

Set `TALIA_ALERT_PROVIDERS` to a private operator-owned JSON file mapping provider
names to configurations. It is read asynchronously for each dispatch; replace the
file atomically to rotate credentials without restart. The authored destination
contains only the provider name. Do not put this file in source control.

```json
{
  "mail": {"kind":"smtp","host":"smtp.example.test","port":587,
           "tls":"starttls","from":"talia@example.test",
           "username":"operator-supplied","password":"operator-supplied"},
  "chat": {"kind":"telegram","token":"operator-supplied"}
}
```

SMTP supports required STARTTLS or implicit TLS (`tls`); plaintext `none_loopback`
is restricted to loopback fixtures/relays. Telegram defaults to its HTTPS Bot API;
HTTP overrides are restricted to loopback test fixtures. Redirects are disabled.
Requests have time/response bounds, and provider error details never expose remote
bodies, credentials or credential-bearing URLs. A provider's positive acceptance is
recorded as `sent`; this does not prove that the human read the message.


## Android push setup

The server's FCM provider uses HTTP v1 and service-account OAuth (RS256). Add a
provider such as `"android":{"kind":"fcm","project":"your-project-id",
"service_account":"/run/secrets/talia-firebase.json"}` to the operator file. The
service-account JSON is read asynchronously and never returned by the alert API.
Configure Firebase project credentials in the Android build using Gradle properties
`firebaseAppId`, `firebaseApiKey`, `firebaseProjectId`, and `firebaseSenderId`.
Without these the APK still provides dashboards and alert controls, and explicitly
reports that push is not configured. No Firebase project is created by this work.

The app creates a stable installation UUID, keeps it across updates/restarts, and
registers its renewable FCM token under the authenticated alert principal. It retries
registration through an Android network-constrained JobScheduler job. Device groups
are operator-assigned; registration cannot grant group membership. The alert access
token is host configuration, separate from the push token and ViewModel state.
Grant `alerts` family `register`, `read` and `acknowledge` permissions as needed;
`configure`, `silence` and `audit` remain independently grantable.

Enable notifications through the native Alerts screen to request Android's runtime
notification permission. Tapping a push opens current server state. Direct
acknowledgement uses the pushed occurrence and revision; stale or disconnected
requests show an acknowledgement failure and never claim success. Expired payloads
are ignored and displayed notifications expire automatically. Clearing application
data or reinstalling produces a new installation UUID.

The current development host still uses the existing loopback connection and ADB
reverse port. Reachable authenticated production endpoints and operator provisioning
belong to deployment preparation; do not treat fixture success as a production rollout.
Debug APKs include a clearly isolated fixture Activity for injecting test payloads and
exercising PendingIntent actions. It is absent from release sources/manifests.

References: [FCM server authorization](https://firebase.google.com/docs/cloud-messaging/auth-server),
[Android receipt](https://firebase.google.com/docs/cloud-messaging/android/receive-messages).


## Initial implementation limits

Policies have at most 32 stages and 32 actions per stage; actions reference at most
32 named destinations. Evaluation uses the existing bounded QuickJS runtime, at most
32 concurrently evaluating bindings, and independent per-binding state. At most eight
provider dispatches run concurrently with a 15-second outer timeout. History/state
entity categories and idempotency records have a 10,000-record capacity; reaching a
capacity reports an error instead of silently dropping pending work. Audit history
retains the latest 10,000 events. Production retention/archival and operational sizing
remain part of production qualification.

A binding's alert key and policy identity are fixed after creation; parameters, inputs,
labels, schedule and enabled state can change at runtime. Shared policy edits update
all referencing bindings. The provider file path is selected at service start, while
its contents are reread for each delivery. See the qualification report for verified
failure/recovery cases and the limits of fixture-only provider evidence.

## Browser Web Push destinations

The web shell's **Settings → Browser notifications** enrolls a browser profile
with an explicit notification-permission gesture. Each enrollment has a stable
`browser-<UUID>` installation ID, owned by the signed-in OIDC subject, and creates
a destination with that same ID, channel `web_push`, and target `device:<ID>`.
An agent can reference this destination in normal alert response actions, including
repeats until acknowledgement. Registration itself never sends a notification.
In-dashboard presentation remains independent of Web Push.

Enrollment currently requires an administrator, matching the existing global
alert-access boundary. Viewer dashboard grants do not grant push access to all
server alerts. Subscriptions are host-managed secrets, absent from dashboard VMs,
MCP configuration responses and delivery history. Android devices retain channel
`push`; browser destinations resolve only `web_push` devices. Named browser
selectors can also use `user:<OIDC subject>` or administrator-managed `group:<name>`.

Configure one provider in the private `TALIA_ALERT_PROVIDERS` JSON file:

```json
{
  "browser": {
    "kind": "web_push",
    "private_key": "/run/talia/web-push.pem",
    "subject": "mailto:operator@example.com"
  }
}
```

Generate a persistent P-256 VAPID key once with
`openssl ecparam -name prime256v1 -genkey -noout -out web-push.pem` under a private
umask. Mount it read-only, readable by container UID 65532, alongside the provider
file. Keep this key through rebuilds and backups; replacing it requires browser
re-enrollment. No Firebase project or browser-vendor account is needed for Web
Push. Provider settings and key contents are read asynchronously at runtime. If
multiple Web Push providers exist, enrollment selects the first name in sorted
order; use one unless deliberately managing a transition.

Default permitted push origins are `https://fcm.googleapis.com`,
`https://updates.push.services.mozilla.com`, and `https://web.push.apple.com`.
An optional `allowed_origins` array replaces that exact-origin allowlist. Add only
trusted browser-vendor origins when needed. Redirects are never followed. HTTPS
is required except numeric loopback HTTP fixtures. Outbound HTTPS must reach the
selected push service. Delivery uses RFC 8291 encrypted payloads and VAPID;
201/202 means vendor acceptance, not display or acknowledgement. Temporary
429/5xx rejection follows the existing bounded retry policy. A 404/410 retires
only the subscription that failed, never a newer rotated address.

The root-scoped `/push-sw.js` service worker handles notifications without a live
dashboard connection; it does not cache or intercept app requests. Encrypted
wakeups contain alert identity/revision/expiry, not alert text or credentials.
Before display the worker fetches current details using the browser's authenticated
session. The server checks current admin access, installation ownership/enabled
state, occurrence, revision and acknowledgement. Stale, revoked and acknowledged
notifications are suppressed. Offline fetch failure can show a generic notice
without alert details. Clicking opens the current Alerts panel; it never silently
acknowledges an alert or follows an external notification URL.

Disable unregisters delivery authority, unsubscribes and closes this worker's
notifications. Normal web sign-out attempts the same cleanup; server session
revocation still prevents fetching alert details if cleanup fails. Account
switches require explicit enrollment for the new owner. Foreground refresh syncs
rotated subscription addresses; missing subscriptions require re-enrollment.
Changes to browser notification permission are reflected in Settings.

A secure context, browser permission and background Web Push support are required.
Delivery while the dashboard tab is closed depends on the browser/OS continuing
to process push. A private-LAN deployment must be reachable when the worker fetches
alert details (for example through the LAN or VPN). Expired login requires signing
in again; there is no persistent bearer token embedded in the worker.
